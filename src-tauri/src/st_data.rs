//! st_data —— 数据目录路径与数据袋（导入/导出）
//!
//! native 架构下数据由应用自包含维护在应用数据目录（`<config>/st-runtime/data`），
//! 保持与 ST 兼容的目录结构（角色卡 /JSONL 会话 / 世界书 / settings.json），
//! 便于继续与 ST 生态互通。

use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

/// 应用数据根目录名（位于应用配置目录下）
pub const RUNTIME_DIR: &str = "st-runtime";

pub fn external_root(config_dir: &Path) -> PathBuf {
    config_dir.join(RUNTIME_DIR)
}

/// 路径显示规范化：Windows 上统一成反斜杠。
pub fn display_path(p: &Path) -> String {
    let s = p.to_string_lossy().into_owned();
    if cfg!(windows) {
        s.replace('/', "\\")
    } else {
        s
    }
}

pub fn external_data_root(config_dir: &Path) -> PathBuf {
    external_root(config_dir).join("data")
}

pub fn external_config_path(config_dir: &Path) -> PathBuf {
    external_root(config_dir).join("config.yaml")
}

/* ------------------------------------------------------------------ *
 * 目录统计
 * ------------------------------------------------------------------ */

#[derive(Debug, Clone, Default, Serialize)]
pub struct TreeStat {
    pub files: u64,
    pub bytes: u64,
}

/// 统计目录（不跟随符号链接，避免死循环）
pub fn stat_tree(root: &Path) -> TreeStat {
    let mut acc = TreeStat::default();
    walk(root, &mut |p| {
        if let Ok(md) = fs::symlink_metadata(p) {
            if md.is_file() {
                acc.files += 1;
                acc.bytes += md.len();
            }
        }
    });
    acc
}

fn walk(dir: &Path, f: &mut impl FnMut(&Path)) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for ent in rd.flatten() {
        let p = ent.path();
        match fs::symlink_metadata(&p) {
            Ok(md) if md.is_dir() => walk(&p, f),
            Ok(_) => f(&p),
            Err(_) => {}
        }
    }
}

/* ------------------------------------------------------------------ *
 * 导出 / 导入（数据袋：数据目录内容 + config.yaml）
 * ------------------------------------------------------------------ */

#[derive(Debug, Clone, Serialize)]
pub struct DataBagResult {
    pub path: String,
    pub files: u64,
    pub bytes: u64,
    pub message: String,
}

/// 导出：把数据目录内容 + config.yaml 打成一个 zip（zip 布局 = 数据内容在根 + config.yaml）。
/// 同步实现，调用方放阻塞线程执行。
pub fn export_bag(
    data_root: &Path,
    config_path: Option<&Path>,
    dest: &Path,
    include_secrets: bool,
) -> Result<DataBagResult, String> {
    if !data_root.is_dir() {
        return Err(format!("数据目录不存在：{}", display_path(data_root)));
    }
    // 先写 .part 临时文件、完成后 rename 到目标（中途崩溃不留下半个 zip）
    let part = dest.with_extension("zip.part");
    if let Some(parent) = dest.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let file = fs::File::create(&part).map_err(|e| format!("创建导出文件失败：{e}"))?;
    let mut zip = zip::ZipWriter::new(file);
    let opts: zip::write::FileOptions<'_, ()> =
        zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    let mut files = 0u64;
    let mut bytes = 0u64;
    let mut excluded = 0u64;
    // 明文密钥面排除清单（secrets.json = API 密钥多值结构；
    // cookie-secret.txt = 会话签名密钥）。include_secrets=false 时跳过。
    const SECRET_FILES: [&str; 2] = ["default-user/secrets.json", "cookie-secret.txt"];
    let mut stack = vec![data_root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let rd = fs::read_dir(&dir).map_err(|e| format!("读取目录失败 {}: {e}", dir.display()))?;
        for e in rd.flatten() {
            let p = e.path();
            let rel = p
                .strip_prefix(data_root)
                .map_err(|_| "相对路径计算失败".to_string())?;
            let rel_norm = rel.to_string_lossy().replace('\\', "/");
            if p.is_dir() {
                stack.push(p);
                continue;
            }
            if !include_secrets && SECRET_FILES.iter().any(|s| rel_norm == *s) {
                excluded += 1;
                continue;
            }
            zip.start_file(rel_norm, opts)
                .map_err(|e| format!("写入 zip 条目失败：{e}"))?;
            let mut f = fs::File::open(&p).map_err(|e| format!("打开文件失败：{e}"))?;
            std::io::copy(&mut f, &mut zip).map_err(|e| format!("压缩写入失败：{e}"))?;
            files += 1;
            bytes += fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
        }
    }
    if let Some(cp) = config_path {
        if cp.is_file() {
            zip.start_file("config.yaml", opts)
                .map_err(|e| format!("写入 config.yaml 失败：{e}"))?;
            let mut f = fs::File::open(cp).map_err(|e| format!("打开 config.yaml 失败：{e}"))?;
            std::io::copy(&mut f, &mut zip).map_err(|e| format!("压缩 config.yaml 失败：{e}"))?;
            files += 1;
        }
    }
    zip.finish().map_err(|e| format!("收尾 zip 失败：{e}"))?;
    fs::rename(&part, dest).map_err(|e| format!("落盘导出文件失败：{e}"))?;

    let message = if !include_secrets && excluded > 0 {
        format!(
            "已导出 {files} 个文件（{:.1} MB）——已排除 {excluded} 个明文密钥文件（API 密钥 / 会话签名密钥）；导入到新机器后需重新填写 API 密钥",
            bytes as f64 / 1048576.0
        )
    } else if include_secrets {
        format!(
            "已导出 {files} 个文件（{:.1} MB）——⚠ 本数据袋包含 API 密钥**明文**，请妥善保管并在备份用途结束后删除",
            bytes as f64 / 1048576.0
        )
    } else {
        format!(
            "已导出 {files} 个文件（{:.1} MB）",
            bytes as f64 / 1048576.0
        )
    };

    Ok(DataBagResult {
        path: display_path(dest),
        files,
        bytes,
        message,
    })
}

/// 导入：清点 zip → 备份现有数据目录（改名 .bak-<ts>）→ 解压 → 返回摘要。
/// 兼容两种 zip 布局：① 本应用导出的格式（数据内容位于根目录，含 config.yaml）
/// ② 带顶层 data/ 前缀的。同步实现。
pub fn import_bag(
    zip_path: &Path,
    data_root: &Path,
    config_path: Option<&Path>,
) -> Result<DataBagResult, String> {
    let file = fs::File::open(zip_path).map_err(|e| format!("打开导入文件失败：{e}"))?;
    let mut archive = zip::ZipArchive::new(std::io::BufReader::new(file))
        .map_err(|e| format!("不是有效的 zip：{e}"))?;

    let mut has_data_prefix = false;
    for i in 0..archive.len() {
        let Ok(e) = archive.by_index(i) else { continue };
        if let Some(name) = e.enclosed_name() {
            if name
                .components()
                .next()
                .map(|c| c.as_os_str() == "data")
                .unwrap_or(false)
            {
                has_data_prefix = true;
                break;
            }
        }
    }

    // 1) 备份现有数据（有内容才备份；空目录直接清掉）
    if data_root.is_dir() {
        let st = stat_tree(data_root);
        if st.files > 0 {
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let bak = data_root.with_extension(format!("bak-{ts}"));
            fs::rename(data_root, &bak).map_err(|e| format!("备份现有数据失败：{e}"))?;
            println!(
                "[st_data] 导入前备份 → {}（{} 文件）",
                display_path(&bak),
                st.files
            );
        } else {
            let _ = fs::remove_dir_all(data_root);
        }
    }
    fs::create_dir_all(data_root).map_err(|e| format!("重建数据目录失败：{e}"))?;

    // 2) 解压（zip-slip 防护用 enclosed_name）
    let mut files = 0u64;
    let mut bytes = 0u64;
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| format!("读取条目失败：{e}"))?;
        let Some(rel) = entry.enclosed_name() else {
            continue;
        };
        let rel = rel.to_path_buf();

        // config.yaml 单独处理（放 st-runtime 根，不放数据目录）
        let is_config = rel.file_name().map(|f| f == "config.yaml").unwrap_or(false)
            && rel
                .parent()
                .map(|p| p.as_os_str().is_empty())
                .unwrap_or(true);
        if is_config {
            let Some(cp) = config_path else { continue };
            if let Some(parent) = cp.parent() {
                fs::create_dir_all(parent).map_err(|e| format!("建目录失败：{e}"))?;
            }
            let mut out =
                fs::File::create(cp).map_err(|e| format!("写入 config.yaml 失败：{e}"))?;
            let n = std::io::copy(&mut entry, &mut out).map_err(|e| format!("解压失败：{e}"))?;
            files += 1;
            bytes += n;
            continue;
        }

        let rel = if has_data_prefix {
            let mut it = rel.components();
            it.next();
            match it.as_path() {
                p if p.as_os_str().is_empty() => continue,
                p => p.to_path_buf(),
            }
        } else {
            rel
        };
        let out = data_root.join(&rel);
        if entry.is_dir() {
            fs::create_dir_all(&out).map_err(|e| format!("建目录失败：{e}"))?;
            continue;
        }
        if let Some(parent) = out.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("建目录失败：{e}"))?;
        }
        let mut outf =
            fs::File::create(&out).map_err(|e| format!("写入失败 {}: {e}", out.display()))?;
        let n = std::io::copy(&mut entry, &mut outf).map_err(|e| format!("解压失败：{e}"))?;
        files += 1;
        bytes += n;
    }

    Ok(DataBagResult {
        path: display_path(data_root),
        files,
        bytes,
        message: format!(
            "已导入 {files} 个文件（{:.1} MB）。重启应用后生效。",
            bytes as f64 / 1048576.0
        ),
    })
}

/* ------------------------------------------------------------------ *
 * 测试
 * ------------------------------------------------------------------ */

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stat_tree_counts_files_and_bytes() {
        let base = std::env::temp_dir().join(format!("stchat-data-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join("sub")).unwrap();
        fs::write(base.join("a.txt"), b"hello").unwrap();
        fs::write(base.join("sub/b.txt"), b"world!!!").unwrap();

        let s = stat_tree(&base);
        assert_eq!(s.files, 2, "files");
        assert_eq!(s.bytes, 5 + 8, "bytes");
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn export_import_roundtrip() {
        let base = std::env::temp_dir().join(format!("stchat-bag-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let data = base.join("st-runtime/data");
        fs::create_dir_all(data.join("default-user/characters")).unwrap();
        fs::write(data.join("default-user/characters/c1.png"), b"png1").unwrap();
        fs::write(data.join("cookie-secret.txt"), b"secret").unwrap();
        let config = base.join("st-runtime/config.yaml");
        fs::write(&config, b"port: 8000\n").unwrap();

        // 导出（含密钥）
        let zip_path = base.join("bag.zip");
        let out = export_bag(&data, Some(&config), &zip_path, true).expect("export ok");
        assert_eq!(out.files, 3, "2 个数据文件 + config.yaml");
        assert!(out.message.contains("明文"), "含密钥导出必须带警告");

        // 排除密钥导出（默认面）
        let zip_safe = base.join("bag-safe.zip");
        let out2 = export_bag(&data, Some(&config), &zip_safe, false).expect("export safe ok");
        assert_eq!(out2.files, 2, "排除 secrets + cookie-secret 后仅剩 c1.png + config");
        assert!(out2.message.contains("已排除"), "排除面消息必须带提示");

        // 破坏现场，再导入 → 内容应恢复
        fs::remove_dir_all(&data).unwrap();
        let r = import_bag(&zip_path, &data, Some(&config)).expect("import ok");
        assert_eq!(r.files, 3, "回读 3 个文件");
        assert_eq!(
            fs::read(data.join("default-user/characters/c1.png")).unwrap(),
            b"png1"
        );
        assert_eq!(fs::read(data.join("cookie-secret.txt")).unwrap(), b"secret");
        assert_eq!(fs::read(&config).unwrap(), b"port: 8000\n");

        // 第二次导入 → 必须出现 .bak-*（导入前备份）
        fs::write(data.join("stale.txt"), b"old").unwrap();
        let r2 = import_bag(&zip_path, &data, Some(&config)).expect("import 2 ok");
        assert_eq!(r2.files, 3);
        let has_bak = fs::read_dir(base.join("st-runtime"))
            .unwrap()
            .flatten()
            .any(|e| e.file_name().to_string_lossy().starts_with("data.bak-"));
        assert!(has_bak, "导入前必须把现有数据备份成 data.bak-*");

        let _ = fs::remove_dir_all(&base);
    }
}
