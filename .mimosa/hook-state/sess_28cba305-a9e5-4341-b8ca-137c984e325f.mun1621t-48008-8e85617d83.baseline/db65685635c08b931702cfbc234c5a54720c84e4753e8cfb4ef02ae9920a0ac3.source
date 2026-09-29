//! 应用级设置（进程侧需要的配置）
//!
//! 与前端 `localStorage` 里的生成参数（`app.gen`）不同：那些是**界面侧**的偏好，
//! 这些是 **Rust 进程侧**在下载/安装时真的要用的东西，比如走不走代理。
//!
//! ── 为什么代理要可配 ──
//! 实测本机直连 nodejs.org / GitHub 都**能通，但速度很不稳**：
//! node 官方源 **0.24 MB/s**、gh-proxy **0.34 MB/s**、某次又跑到 1.28 MB/s。
//! 有代理的用户填上代理会快很多（这是很多国内用户的真实情况），
//! 所以「代理」必须是**用户可见可改**的一等配置，而不是埋在环境变量里。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const FILE: &str = "app-settings.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    /// HTTP 代理地址，如 `http://127.0.0.1:7890`。**空 = 直连**（默认）。
    #[serde(default)]
    pub proxy: String,
    /// `npm install` 是否也走该代理
    #[serde(default = "default_true")]
    pub proxy_for_npm: bool,
    /// 组件（node / SillyTavern）的安装根目录。**空 = `%LOCALAPPDATA%\<app-id>`**（默认）。
    /// 想省 C 盘空间的用户可以填 `D:\STChatData` 之类。
    #[serde(default)]
    pub install_root: String,
    #[serde(rename = "_readme", default = "readme")]
    pub readme: String,
}

fn default_true() -> bool {
    true
}

fn readme() -> String {
    "本应用级设置（Rust 进程侧读取）。\n\
     proxy：下载 node / SillyTavern 时使用的 HTTP 代理，留空表示直连。\n\
       · 实测本机直连 nodejs.org / GitHub 都通，但速度不稳（0.24–1.28 MB/s）；\n\
         有代理的用户填上会明显更快，例如 http://127.0.0.1:7890\n\
     proxy_for_npm：npm install 是否也走该代理（会以 HTTP_PROXY/HTTPS_PROXY 传给 npm 子进程）。\n\
     环境变量 ST_CHAT_PROXY 的优先级高于本文件（排障时可临时覆盖）。"
        .to_string()
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            proxy: String::new(),
            proxy_for_npm: true,
            install_root: String::new(),
            readme: readme(),
        }
    }
}

pub fn path_of(config_dir: &Path) -> PathBuf {
    config_dir.join(FILE)
}

pub fn load(config_dir: &Path) -> AppSettings {
    match std::fs::read_to_string(path_of(config_dir)) {
        Ok(t) => match serde_json::from_str(&t) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[app-settings] 解析失败，改用默认：{e}");
                AppSettings::default()
            }
        },
        Err(_) => AppSettings::default(),
    }
}

pub fn save(config_dir: &Path, s: &AppSettings) -> Result<(), String> {
    std::fs::create_dir_all(config_dir).map_err(|e| format!("创建目录失败：{e}"))?;
    let text = serde_json::to_string_pretty(s).map_err(|e| e.to_string())?;
    // 原子写：防中断留下截断的 app-settings.json（坏档会静默回落默认设置）
    crate::st_data::atomic_write(&path_of(config_dir), text.as_bytes())
        .map_err(|e| format!("写入失败：{e}"))
}

/// 首次运行时落一份带说明的文件，方便用户手工改
pub fn ensure_exists(config_dir: &Path) {
    if !path_of(config_dir).exists() {
        let _ = save(config_dir, &AppSettings::default());
    }
}

/// 实际生效的代理：**环境变量 > 配置文件 > 不使用**。
///
/// 注意「不使用」时要显式 `no_proxy()` —— 本机常驻沙箱代理会让公网请求直接失败，
/// 所以不能放任 reqwest 去读 `HTTP_PROXY` 环境变量。
pub fn effective_proxy(config_dir: &Path) -> Option<String> {
    if let Ok(v) = std::env::var("ST_CHAT_PROXY") {
        let v = v.trim().to_string();
        if !v.is_empty() {
            return Some(v);
        }
    }
    let p = load(config_dir).proxy.trim().to_string();
    if p.is_empty() {
        None
    } else {
        Some(p)
    }
}

/* ------------------------------------------------------------------ *
 * 测试
 * ------------------------------------------------------------------ */

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("stchat-cfg-{}-{}", std::process::id(), name));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn default_is_direct_no_proxy() {
        let d = tmp("default");
        let s = load(&d);
        assert_eq!(s.proxy, "", "默认必须直连（实测直连可用，乱用代理反而会挂）");
        assert!(s.proxy_for_npm, "默认让 npm 跟随代理设置");
        assert!(effective_proxy(&d).is_none());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn roundtrip_and_blank_treated_as_none() {
        let d = tmp("roundtrip");
        let mut s = AppSettings::default();
        s.proxy = "http://127.0.0.1:7890".into();
        save(&d, &s).unwrap();

        let got = load(&d);
        assert_eq!(got.proxy, "http://127.0.0.1:7890");
        assert_eq!(
            effective_proxy(&d).as_deref(),
            Some("http://127.0.0.1:7890")
        );

        // 纯空白要视作"不配代理"，不能拿空格去建 Proxy
        s.proxy = "   ".into();
        save(&d, &s).unwrap();
        assert!(effective_proxy(&d).is_none(), "空白代理必须等价于不配");

        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn env_overrides_file() {
        let d = tmp("env");
        let mut s = AppSettings::default();
        s.proxy = "http://file-proxy:1111".into();
        save(&d, &s).unwrap();

        std::env::set_var("ST_CHAT_PROXY", "http://env-proxy:2222");
        assert_eq!(
            effective_proxy(&d).as_deref(),
            Some("http://env-proxy:2222"),
            "环境变量优先级最高（排障用）"
        );
        std::env::remove_var("ST_CHAT_PROXY");

        // 环境变量为空串时不能压过文件
        std::env::set_var("ST_CHAT_PROXY", "  ");
        assert_eq!(effective_proxy(&d).as_deref(), Some("http://file-proxy:1111"));
        std::env::remove_var("ST_CHAT_PROXY");

        let _ = std::fs::remove_dir_all(&d);
    }
}
