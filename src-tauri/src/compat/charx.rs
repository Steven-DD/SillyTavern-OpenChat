//! compat/charx —— CharX / CCv3 角色卡 ZIP 容器解包与资产解析
//!
//! 规范依据：
//! - Character Card V3 Specification (CCv3)
//! - SillyTavern characters.js / character-cards.js parseCharX
//!
//! 容器结构：
//! - `card.json`：CCv3 (spec: chara_card_v3) 或 V2 格式元数据
//! - `assets/`：立绘、表情、背景、音频等外置资产
//! - `lorebooks/` 或 `module/`：绑定的世界书文件 (.json)
//! - 图标/主头像：由 `assets` 清单中的 icon 项指定，或默认为 `icon.png` / `avatar.png`

use super::chara_card;
use super::png;
use serde_json::Value;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct CharXImportResult {
    /// 归一化后的角色卡 JSON 数据
    pub card: Value,
    /// 角色显示名
    pub display_name: String,
    /// 主头像图片字节（若未提供或格式不符，自动回落为 minimal_png）
    pub avatar_image: Vec<u8>,
    /// 伴生资产：(角色目录下的相对安全路径, 文件内容)
    pub assets: Vec<(PathBuf, Vec<u8>)>,
    /// 绑定世界书：(世界书安全文件名, JSON 字节)
    pub lorebooks: Vec<(String, Vec<u8>)>,
}

/// 检查路径是否包含穿越（Zip Slip 防护）
fn is_safe_zip_entry(name: &str) -> bool {
    let p = Path::new(name);
    // 拒绝绝对路径或带盘符
    if p.is_absolute() || p.has_root() {
        return false;
    }
    // 拒绝包含 .. 或父级引用
    for comp in p.components() {
        match comp {
            std::path::Component::ParentDir => return false,
            std::path::Component::Prefix(_) => return false,
            _ => {}
        }
    }
    // 拒绝非法反斜杠尝试穿越
    if name.contains("../") || name.contains("..\\") || name.starts_with('/') || name.starts_with('\\') {
        return false;
    }
    true
}

/// 解析 .charx 二进制数据流
pub fn parse_charx(bytes: &[u8]) -> Result<CharXImportResult, String> {
    let reader = Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(reader).map_err(|e| format!("无法解压 CharX 文件：{e}"))?;

    // 1. 读取全部文件内容至内存映射（过滤掉 macOS 垃圾元数据文件）
    let mut entries: Vec<(String, Vec<u8>)> = Vec::new();
    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|e| format!("读取压缩包条目失败：{e}"))?;
        if file.is_dir() {
            continue;
        }
        let raw_name = file.name().to_string();
        if raw_name.contains("__MACOSX") || raw_name.ends_with(".DS_Store") {
            continue;
        }
        if !is_safe_zip_entry(&raw_name) {
            return Err(format!("检测到不安全的文件路径穿越：{raw_name}"));
        }
        let mut buf = Vec::new();
        file.read_to_end(&mut buf)
            .map_err(|e| format!("读取条目 {raw_name} 失败：{e}"))?;
        entries.push((raw_name, buf));
    }

    // 2. 寻找 card.json
    let card_json_bytes = entries
        .iter()
        .find(|(name, _)| {
            name.eq_ignore_ascii_case("card.json")
                || name.ends_with("/card.json")
                || name.ends_with("\\card.json")
        })
        .map(|(_, b)| b)
        .ok_or_else(|| "CharX 压缩包中缺少 card.json 角色元数据".to_string())?;

    let text = String::from_utf8_lossy(card_json_bytes);
    let text = text.trim_start_matches('\u{FEFF}');
    let raw_card: Value = serde_json::from_str(text)
        .map_err(|e| format!("解析 card.json 失败：{e}"))?;

    // 提取角色名称
    let display_name = raw_card
        .get("data")
        .and_then(|d| d.get("name"))
        .and_then(|n| n.as_str())
        .or_else(|| raw_card.get("name").and_then(|n| n.as_str()))
        .unwrap_or("")
        .trim()
        .to_string();

    if display_name.is_empty() {
        return Err("card.json 缺少有效的角色名称".into());
    }

    // 3. 规范化卡数据：保留 CCv3 / V2 结构
    let normalized_card = if raw_card.get("spec").is_some() {
        raw_card.clone()
    } else {
        chara_card::to_v2_response(raw_card.clone())
    };

    // 4. 定位主头像
    // 优先从 CCv3 的 assets 声明列表中找 type == "icon" 或 name == "main"
    let mut icon_uri: Option<String> = None;
    if let Some(assets) = raw_card
        .get("data")
        .and_then(|d| d.get("assets"))
        .and_then(|a| a.as_array())
    {
        for a in assets {
            let atype = a.get("type").and_then(|t| t.as_str()).unwrap_or("");
            let aname = a.get("name").and_then(|n| n.as_str()).unwrap_or("");
            if atype.eq_ignore_ascii_case("icon") || aname.eq_ignore_ascii_case("main") {
                if let Some(uri) = a.get("uri").and_then(|u| u.as_str()) {
                    icon_uri = Some(uri.to_string());
                    break;
                }
            }
        }
    }

    // 查找头像文件
    let mut avatar_bytes: Option<Vec<u8>> = None;
    if let Some(ref uri) = icon_uri {
        avatar_bytes = entries
            .iter()
            .find(|(name, _)| name == uri || name.ends_with(&format!("/{uri}")) || name.ends_with(&format!("\\{uri}")))
            .map(|(_, b)| b.clone());
    }

    if avatar_bytes.is_none() {
        // 候选名称列表
        let candidates = [
            "icon.png", "icon.jpg", "icon.jpeg", "icon.webp",
            "assets/icon.png", "assets/icon.jpg", "assets/icon.webp",
            "avatar.png", "avatar.jpg", "avatar.webp",
            "assets/avatar.png", "assets/avatar.jpg", "assets/avatar.webp",
            "card.png", "assets/card.png",
        ];
        for cand in candidates {
            if let Some((_, b)) = entries.iter().find(|(name, _)| name.eq_ignore_ascii_case(cand)) {
                avatar_bytes = Some(b.clone());
                break;
            }
        }
    }

    // 若仍无头像或非 PNG，使用 minimal_png 作为安全基础容器
    let avatar_image = match avatar_bytes {
        Some(b) => {
            if png::extract_chunks(&b).is_ok() {
                b
            } else {
                png::minimal_png()
            }
        }
        None => png::minimal_png(),
    };

    // 5. 提取 assets 资产与 lorebooks 世界书
    let mut assets: Vec<(PathBuf, Vec<u8>)> = Vec::new();
    let mut lorebooks: Vec<(String, Vec<u8>)> = Vec::new();

    for (raw_path, content) in entries {
        // 跳过 card.json 本身
        if raw_path.eq_ignore_ascii_case("card.json")
            || raw_path.ends_with("/card.json")
            || raw_path.ends_with("\\card.json")
        {
            continue;
        }

        let norm_path = raw_path.replace('\\', "/");
        // 世界书条目判定：以 lorebooks/ 或 module/ 或 worlds/ 开头，且为 .json
        if (norm_path.starts_with("lorebooks/") || norm_path.starts_with("module/") || norm_path.starts_with("worlds/"))
            && norm_path.ends_with(".json")
        {
            let filename = norm_path
                .rsplit('/')
                .next()
                .unwrap_or("lorebook.json")
                .to_string();
            let safe_name = crate::pathguard::sanitize(&filename);
            if !safe_name.is_empty() {
                lorebooks.push((safe_name, content));
            }
            continue;
        }

        // 普通资产（立绘、表情、背景）：
        // 去除 assets/ 前缀，构建角色子目录下的相对路径
        let stripped = norm_path.strip_prefix("assets/").unwrap_or(&norm_path);
        let mut rel_buf = PathBuf::new();
        for seg in stripped.split('/') {
            let seg_trim = seg.trim();
            if seg_trim.is_empty() || seg_trim == "." {
                continue;
            }
            let safe_seg = crate::pathguard::sanitize(seg_trim);
            if !safe_seg.is_empty() {
                rel_buf.push(safe_seg);
            }
        }
        if rel_buf.as_os_str().is_empty() {
            continue;
        }
        assets.push((rel_buf, content));
    }

    Ok(CharXImportResult {
        card: normalized_card,
        display_name,
        avatar_image,
        assets,
        lorebooks,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    fn create_test_zip(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let mut writer = zip::ZipWriter::new(Cursor::new(&mut buf));
            for (name, content) in files {
                writer
                    .start_file(*name, SimpleFileOptions::default())
                    .unwrap();
                writer.write_all(content).unwrap();
            }
            writer.finish().unwrap();
        }
        buf
    }

    #[test]
    fn parse_valid_charx_v3() {
        let card_json = serde_json::json!({
            "spec": "chara_card_v3",
            "spec_version": "3.0",
            "data": {
                "name": "测试角色",
                "description": "一个用于测试的 CharX 角色",
                "first_mes": "你好！",
                "assets": [
                    { "type": "icon", "uri": "assets/icon.png", "name": "main" }
                ]
            }
        });
        let card_str = serde_json::to_string(&card_json).unwrap();
        let minimal_png = png::minimal_png();

        let zip_bytes = create_test_zip(&[
            ("card.json", card_str.as_bytes()),
            ("assets/icon.png", &minimal_png),
            ("assets/expressions/joy.png", &minimal_png),
            ("lorebooks/world.json", b"{\"entries\":{}}"),
        ]);

        let res = parse_charx(&zip_bytes).unwrap();
        assert_eq!(res.display_name, "测试角色");
        assert_eq!(res.card["spec"], "chara_card_v3");
        assert_eq!(res.card["data"]["name"], "测试角色");
        assert_eq!(res.avatar_image, minimal_png);
        assert_eq!(res.lorebooks.len(), 1);
        assert_eq!(res.lorebooks[0].0, "world.json");
        assert!(res.assets.iter().any(|(p, _)| p == Path::new("expressions/joy.png")));
    }

    #[test]
    fn reject_zip_slip() {
        let card_str = r#"{"spec":"chara_card_v3","data":{"name":"Hacker"}}"#;
        let zip_bytes = create_test_zip(&[
            ("card.json", card_str.as_bytes()),
            ("../../etc/passwd", b"malicious"),
        ]);

        let res = parse_charx(&zip_bytes);
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("路径穿越"));
    }

    #[test]
    fn reject_missing_card_json() {
        let zip_bytes = create_test_zip(&[("readme.txt", b"hello")]);
        let res = parse_charx(&zip_bytes);
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("缺少 card.json"));
    }
}
