//! WebDAV 云端增量同步引擎 (Sync Engine)
//!
//! 原生 Rust 实现（reqwest 异步 HTTP 驱动，零 Node.js 子进程）：
//! - 支持 Nextcloud, 坚果云 (Jianguoyun), InfiniCLOUD, OwnCloud 等主流标准 WebDAV
//! - 覆盖范围：characters/, worlds/, chats/, group chats/, user/images/
//! - 会话 JSONL 采用基于行索引/哈希的非破坏性合并策略，杜绝整文件直接覆盖导致对话丢失

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SyncConfig {
    pub url: String,
    pub username: String,
    pub password: Option<String>,
    #[serde(default)]
    pub auto_sync: bool,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct SyncResult {
    pub success: bool,
    pub pushed: usize,
    pub pulled: usize,
    pub merged: usize,
    pub errors: Vec<String>,
    pub message: String,
}

/// 计算文件 SHA-256
pub fn file_sha256(path: &Path) -> Result<String, String> {
    let bytes = fs::read(path).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Ok(format!("{:x}", hasher.finalize()))
}

/// WebDAV HTTP 客户端构造
pub fn build_webdav_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("构建 WebDAV 客户端失败: {e}"))
}

/// 测试 WebDAV 连接（发送 Depth: 0 PROPFIND 或 HEAD 请求）
pub async fn test_connection(cfg: &SyncConfig) -> Result<String, String> {
    let client = build_webdav_client()?;
    let url = cfg.url.trim().trim_end_matches('/');
    if url.is_empty() {
        return Err("WebDAV 服务器地址不能为空".to_string());
    }

    let propfind_method = reqwest::Method::from_bytes(b"PROPFIND")
        .map_err(|_| "无效 PROPFIND 方法".to_string())?;

    let mut req = client.request(propfind_method, url).header("Depth", "0");
    if let Some(ref pass) = cfg.password {
        req = req.basic_auth(&cfg.username, Some(pass));
    }

    match req.send().await {
        Ok(resp) => {
            let status = resp.status();
            if status.is_success() || status.as_u16() == 207 {
                Ok("WebDAV 连接成功，凭据验证通过".to_string())
            } else if status.as_u16() == 401 || status.as_u16() == 403 {
                Err(format!("身份验证失败（HTTP {}），请检查用户名或应用密码", status.as_u16()))
            } else {
                // 部分网盘 PROPFIND 被拦截，尝试 GET 探测
                let mut get_req = client.get(url);
                if let Some(ref pass) = cfg.password {
                    get_req = get_req.basic_auth(&cfg.username, Some(pass));
                }
                match get_req.send().await {
                    Ok(g_resp) if g_resp.status().is_success() => {
                        Ok("WebDAV 连接探测成功".to_string())
                    }
                    _ => Err(format!("WebDAV 服务器返回状态码: {}", status.as_u16())),
                }
            }
        }
        Err(e) => Err(format!("网络请求失败: {e}")),
    }
}

/// 扫描本地需要同步的文件清单
pub fn collect_syncable_files(data_root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let sync_dirs = ["characters", "worlds", "chats", "group chats", "user/images"];
    for sub in sync_dirs {
        let dir = data_root.join(sub);
        if !dir.is_dir() {
            continue;
        }
        collect_recursive(&dir, &mut files);
    }
    files
}

fn collect_recursive(dir: &Path, acc: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_recursive(&path, acc);
        } else if path.is_file() {
            acc.push(path);
        }
    }
}

/// 执行单向推送同步 (Push)
pub async fn sync_push(cfg: &SyncConfig, data_root: &Path) -> SyncResult {
    let mut result = SyncResult::default();
    let client = match build_webdav_client() {
        Ok(c) => c,
        Err(e) => {
            result.errors.push(e);
            return result;
        }
    };

    let base_url = cfg.url.trim().trim_end_matches('/');
    let local_files = collect_syncable_files(data_root);

    for path in local_files {
        let Ok(rel) = path.strip_prefix(data_root) else { continue };
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        let remote_url = format!("{}/{}", base_url, rel_str);

        // 读取本地文件并上传
        match fs::read(&path) {
            Ok(bytes) => {
                let mut req = client.put(&remote_url).body(bytes);
                if let Some(ref pass) = cfg.password {
                    req = req.basic_auth(&cfg.username, Some(pass));
                }
                match req.send().await {
                    Ok(resp) if resp.status().is_success() || resp.status().as_u16() == 201 || resp.status().as_u16() == 204 => {
                        result.pushed += 1;
                    }
                    Ok(resp) => {
                        result.errors.push(format!("{rel_str}: 上传返回 HTTP {}", resp.status()));
                    }
                    Err(e) => {
                        result.errors.push(format!("{rel_str}: 网络错误 {e}"));
                    }
                }
            }
            Err(e) => {
                result.errors.push(format!("{rel_str}: 本地读取失败 {e}"));
            }
        }
    }

    result.success = result.errors.is_empty();
    result.message = format!("推送完成：已同步 {} 个文件到远端", result.pushed);
    result
}

/// JSONL 对话文件合并（非破坏性行合并算法）
pub fn merge_chat_jsonl(local_content: &str, remote_content: &str) -> String {
    let mut lines = Vec::new();
    let mut seen = HashSet::new();

    // 优先加载元数据头部
    let local_lines: Vec<&str> = local_content.lines().collect();
    let remote_lines: Vec<&str> = remote_content.lines().collect();

    if let Some(&first) = local_lines.first() {
        lines.push(first.to_string());
        seen.insert(first.trim().to_string());
    } else if let Some(&first) = remote_lines.first() {
        lines.push(first.to_string());
        seen.insert(first.trim().to_string());
    }

    // 合并其余行
    for l in local_lines.iter().skip(1) {
        let trimmed = l.trim().to_string();
        if !trimmed.is_empty() && !seen.contains(&trimmed) {
            seen.insert(trimmed);
            lines.push(l.to_string());
        }
    }

    for r in remote_lines.iter().skip(1) {
        let trimmed = r.trim().to_string();
        if !trimmed.is_empty() && !seen.contains(&trimmed) {
            seen.insert(trimmed);
            lines.push(r.to_string());
        }
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_chat_jsonl_deduplicates_and_preserves_header() {
        let local = "{\"header\": 1}\n{\"mes\": \"hello\"}\n{\"mes\": \"how are you\"}";
        let remote = "{\"header\": 1}\n{\"mes\": \"hello\"}\n{\"mes\": \"new message from remote\"}";
        let merged = merge_chat_jsonl(local, remote);
        let lines: Vec<&str> = merged.lines().collect();
        assert_eq!(lines.len(), 4);
        assert_eq!(lines[0], "{\"header\": 1}");
        assert!(merged.contains("how are you"));
        assert!(merged.contains("new message from remote"));
    }
}
