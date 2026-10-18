//! backend/logx —— 后端日志门面（生产可诊断性 · 全量埋点约定）
//!
//! ── 为什么需要它 ──
//! release 版在 Windows 上是 GUI 子系统程序（无控制台）：`println!/eprintln!`
//! 会写到无效句柄，**生产环境等于没有日志**。本模块把后端日志统一收敛到
//! `log` crate —— `tauri-plugin-log` 已注册全局 logger（stdout + appLogDir
//! 滚动文件，2MB KeepOne），因此后端与前端（webview target）日志落在同一份
//! 文件里，便于按时间线串联一次请求。
//!
//! ── target 约定（日志过滤与检索用）──
//! * `stchat::boot`     启动/退出/监听端口/数据根
//! * `stchat::api`      请求生命周期（进入、鉴权、状态码、耗时）
//! * `stchat::fs`       数据落盘（角色卡/会话/世界书/群组/资产/扩展）
//! * `stchat::provider` 上游出站（源/模型/流式/状态/耗时）
//! * `stchat::vault`    密钥读写与降级
//! * `stchat::ext`      AI extras（data-maid/翻译/TTS/向量）
//!
//! ── 脱敏铁律（安全规格 S13）──
//! 1. **绝不打印请求体/响应体原文**（可能含密钥与私密对话）
//! 2. query 中的 `token/key/api_key/password/secret/authorization` 值 → `***`
//! 3. `Bearer <token>`、`xi-api-key: <key>`、`authorization: <...>` → 值 `***`
//! 4. 上游 URL 只记 `scheme://host/path`（去 query 与 userinfo），错误不回显完整 URL

use std::time::Instant;

pub const API: &str = "stchat::api";
pub const FS: &str = "stchat::fs";
pub const PROVIDER: &str = "stchat::provider";
pub const VAULT: &str = "stchat::vault";
pub const BOOT: &str = "stchat::boot";
pub const EXT: &str = "stchat::ext";
pub const IPC: &str = "stchat::ipc";

/// query/表单里视为敏感、值需替换为 `***` 的键（小写比对）
const SENSITIVE_KEYS: [&str; 8] = [
    "token",
    "key",
    "api_key",
    "apikey",
    "password",
    "secret",
    "authorization",
    "xi-api-key",
];

fn is_token_char(c: u8) -> bool {
    let c = c as char;
    c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '+' | '/' | '=')
}

/// 把 `marker` 之后的一段 token 字符替换为 `***`（大小写不敏感匹配 marker）。
/// `skip_ws`：marker 以冒号结尾时，先跳过空格再吃 token（header 形态）。
fn redact_after(s: &str, marker: &str, skip_ws: bool) -> String {
    let lower = s.to_ascii_lowercase();
    let m = marker.to_ascii_lowercase();
    let mut out = String::with_capacity(s.len());
    let mut idx = 0usize;
    while let Some(pos) = lower[idx..].find(&m) {
        let start = idx + pos;
        let after = start + m.len();
        out.push_str(&s[idx..after]);
        let mut j = after;
        if skip_ws {
            while j < s.len() && s.as_bytes()[j] == b' ' {
                j += 1;
            }
        }
        out.push_str(&s[after..j]);
        let mut k = j;
        while k < s.len() && is_token_char(s.as_bytes()[k]) {
            k += 1;
        }
        if k > j {
            out.push_str("***");
        }
        idx = k;
    }
    out.push_str(&s[idx..]);
    out
}

/// `Authorization:` 头：保留 `Bearer`/`Basic` 方案词，脱敏其后的凭据。
fn redact_authorization(s: &str) -> String {
    let lower = s.to_ascii_lowercase();
    let marker = "authorization:";
    let mut out = String::with_capacity(s.len());
    let mut idx = 0usize;
    while let Some(pos) = lower[idx..].find(marker) {
        let start = idx + pos;
        let after = start + marker.len();
        out.push_str(&s[idx..after]);
        let mut j = after;
        while j < s.len() && s.as_bytes()[j] == b' ' {
            j += 1;
        }
        out.push_str(&s[after..j]);
        // 方案词（Bearer/Basic）原样保留，只脱敏其后的凭据
        let rest_lower = lower[j..].to_string();
        if let Some(n) = ["bearer ", "basic "]
            .iter()
            .find_map(|w| rest_lower.starts_with(w).then_some(w.len()))
        {
            out.push_str(&s[j..j + n]);
            j += n;
        }
        let mut k = j;
        while k < s.len() && is_token_char(s.as_bytes()[k]) {
            k += 1;
        }
        if k > j {
            out.push_str("***");
        }
        idx = k;
    }
    out.push_str(&s[idx..]);
    out
}

/// `k=v&k2=v2` 形态里敏感键的值替换为 `***`（支持 `path?token=x` 的首段形态）
fn redact_query_segments(s: &str) -> String {
    s.split('&')
        .map(|seg| {
            let (prefix, kv) = match seg.split_once('?') {
                Some((p, q)) => (format!("{p}?"), q),
                None => (String::new(), seg),
            };
            match kv.split_once('=') {
                Some((k, _)) if SENSITIVE_KEYS.contains(&k.trim().to_ascii_lowercase().as_str()) => {
                    format!("{prefix}{k}=***")
                }
                _ => seg.to_string(),
            }
        })
        .collect::<Vec<_>>()
        .join("&")
}

/// 日志脱敏（S13）。**任何用户可控字符串进日志前都应先过这里。**
pub fn redact(input: &str) -> String {
    let s = redact_query_segments(input);
    let s = redact_authorization(&s);
    let s = redact_after(&s, "Bearer ", false);
    let s = redact_after(&s, "xi-api-key:", true);
    let s = redact_after(&s, "api_key=", false);
    redact_after(&s, "key=", false)
}

/// 上游 URL 摘要：仅 `scheme://host/path`（去 userinfo 与 query），供出站日志使用。
pub fn url_summary(url: &str) -> String {
    let Some(scheme_end) = url.find("://") else {
        return redact(url);
    };
    let head = &url[..scheme_end + 3];
    let rest = &url[scheme_end + 3..];
    let rest = rest.split('?').next().unwrap_or(rest);
    let rest = match rest.split_once('@') {
        Some((_, host_part)) => host_part,
        None => rest,
    };
    redact(&format!("{head}{rest}"))
}

/// 一次请求的日志句柄：`authorize` 入口创建，响应构造完 `finish`。
///
/// 记录内容刻意保持"一行一次请求"：`← 200 12ms POST /api/chats/get origin=… bytes=42`，
/// 便于 grep 出"某端点是否被打到、耗时多少、是否 4xx/5xx"。
pub struct ReqLog {
    started: Instant,
    method: String,
    path: String,
    origin: String,
}

impl ReqLog {
    pub fn new(method: &str, path: &str, origin: Option<&str>) -> Self {
        let path = redact(path);
        let origin = origin.unwrap_or("-").to_string();
        log::debug!(target: API, "→ {method} {path} origin={origin}");
        Self {
            started: Instant::now(),
            method: method.to_string(),
            path,
            origin,
        }
    }

    pub fn elapsed_ms(&self) -> u128 {
        self.started.elapsed().as_millis()
    }

    /// 响应落地：按状态码分级（5xx=error / 4xx=warn / 其余=info）。
    /// `extra` 放端点特有摘要（如 bytes=…、count=…、avatar=…），无则传空串。
    pub fn finish(&self, status: u16, extra: &str) {
        let ms = self.elapsed_ms();
        let m = &self.method;
        let p = &self.path;
        let o = &self.origin;
        let tail = if extra.is_empty() {
            String::new()
        } else {
            format!(" {extra}")
        };
        if status >= 500 {
            log::error!(target: API, "← {status} {ms}ms {m} {p} origin={o}{tail}");
        } else if status >= 400 {
            log::warn!(target: API, "← {status} {ms}ms {m} {p} origin={o}{tail}");
        } else {
            log::info!(target: API, "← {status} {ms}ms {m} {p} origin={o}{tail}");
        }
    }
}

/// 鉴权拒绝（403）—— 生产最常见的"全部请求失败"根因，单独 warn 一条。
pub fn auth_rejected(method: &str, path: &str, origin: Option<&str>, ua: Option<&str>) {
    log::warn!(
        target: API,
        "auth 拒绝 403 {method} {} origin={} ua={}",
        redact(path),
        origin.unwrap_or("-"),
        ua.unwrap_or("-")
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_query_secrets() {
        let s = redact("/api/data-maid/view?token=deadbeef&hash=abc");
        assert!(s.contains("token=***"), "{s}");
        assert!(s.contains("hash=abc"), "非敏感键保留：{s}");
        assert!(!s.contains("deadbeef"), "{s}");
    }

    #[test]
    fn redacts_bearer_and_header_forms() {
        assert_eq!(
            redact("Authorization: Bearer sk-abc123"),
            "Authorization: Bearer ***"
        );
        let s = redact("headers { xi-api-key: el-secret-1 }");
        assert!(s.contains("xi-api-key: ***"), "{s}");
        assert!(!s.contains("el-secret-1"), "{s}");
    }

    #[test]
    fn url_summary_drops_query_and_userinfo() {
        let s = url_summary("https://user:pw@api.example.com/v1/chat?key=secret&x=1");
        assert_eq!(s, "https://api.example.com/v1/chat");
    }

    #[test]
    fn keeps_benign_text_intact() {
        assert_eq!(redact("/api/chats/get"), "/api/chats/get");
        assert_eq!(redact("avatar=林晚棠.png"), "avatar=林晚棠.png");
    }
}
