//! 内置插件清单
//!
//! 清单以真实 JSON 文件存放在 `src-tauri/plugins/`，编译期 `include_str!` 进二进制。
//! 这样既「开箱可用」（不依赖运行时资源目录），又能在仓库里直接阅读/评审。
//!
//! 另支持用户覆盖：`<app_config_dir>/plugins/*.json` 中同 id 的清单会替换内置项，
//! 便于后续第三方插件或下游定制（「万物皆可插件」的落地入口）。

use serde::Deserialize;
use std::path::{Path, PathBuf};

use super::{CapabilitySpec, Manifest};

const MANIFEST_NODE: &str = include_str!("../../plugins/plugin-node.json");
const MANIFEST_SILLYTAVERN: &str = include_str!("../../plugins/plugin-sillytavern.json");
const MANIFEST_GIT: &str = include_str!("../../plugins/plugin-git.json");
const CAPABILITIES: &str = include_str!("../../plugins/capabilities.json");

#[derive(Deserialize)]
struct CapabilityFile {
    capabilities: Vec<CapabilitySpec>,
}

/// 开发布局兜底：`<repo>/app/src-tauri` → `<repo>/sillytavern`
///
/// 生产环境该路径不存在，因此只是候选之一，不会影响已打包的应用。
fn dev_st_dir() -> Option<PathBuf> {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("sillytavern");
    p.join("server.js").is_file().then_some(p)
}

fn parse_one(raw: &str) -> Option<Manifest> {
    match serde_json::from_str::<Manifest>(raw) {
        Ok(m) => Some(m),
        Err(e) => {
            eprintln!("[plugin] 清单解析失败: {e}");
            None
        }
    }
}

/// 载入内置清单 + 用户覆盖
pub fn load(config_dir: &Path) -> (Vec<Manifest>, Vec<CapabilitySpec>) {
    let mut list: Vec<Manifest> = [MANIFEST_NODE, MANIFEST_SILLYTAVERN, MANIFEST_GIT]
        .iter()
        .filter_map(|r| parse_one(r))
        .collect();

    // 开发布局兜底：把本仓库的 sillytavern 目录塞进候选（仅当真实存在）
    if let Some(dev) = dev_st_dir() {
        if let Some(st) = list.iter_mut().find(|m| m.id == "sillytavern") {
            st.probe
                .search_paths
                .push(dev.to_string_lossy().into_owned());
        }
    }

    // 用户覆盖目录
    let user_dir = config_dir.join("plugins");
    if let Ok(entries) = std::fs::read_dir(&user_dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&p) else {
                continue;
            };
            if let Some(m) = parse_one(&text) {
                println!("[plugin] 载入用户清单 {} ← {}", m.id, p.display());
                list.retain(|x| x.id != m.id);
                list.push(m);
            }
        }
    }

    // UI 上必要插件排在前面，同级按 id 稳定排序
    list.sort_by(|a, b| {
        let rank = |t: &str| if t == "required" { 0 } else { 1 };
        rank(&a.tier).cmp(&rank(&b.tier)).then(a.id.cmp(&b.id))
    });

    let caps = serde_json::from_str::<CapabilityFile>(CAPABILITIES)
        .map(|f| f.capabilities)
        .unwrap_or_default();

    (list, caps)
}
