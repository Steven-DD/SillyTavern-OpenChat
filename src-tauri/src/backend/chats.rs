//! backend/chats —— 会话 JSONL 数据服务（Phase 1 族四 · manifest #9-#18）
//!
//! ST 契约（chats.js:545-1152，已逐行核对）：
//! - 保存：`trySaveChat` = 每条 `JSON.stringify` join('\n') 原子写 +
//!   integrity slug 比对（`chat_metadata.integrity`；首行非 JSON 对象/有
//!   integrity 且不匹配 → `{error:'integrity'}` 400）+ per-chat 节流备份
//!   （10s leading+trailing，chat_<key>_<ts>.jsonl，前缀去重 + maxTotal）。
//! - 读取：逐行 tryParse 过滤（损坏行跳过）—— 与 Rust 行级 IPC 通路的
//!   「未知字段保真」互补（评审 A6/T21）。
//! - getChatInfo：流式扫描——首行 chat_metadata（withMetadata）、行数、
//!   末行 mes/send_date；末行损坏时降级预览不隐藏会话。
//! - /recent：三个来源聚合（characters/<name>/、chats 根 jsonl、groups
//!   经 group.chats 索引到 group_chats/<id>.jsonl），置顶优先 + mtime 排序。
//! - 备份 key：非 ASCII 名（CJK）用 `sanitize+非字母数字→_` + sha256 前 8 位
//!   （chats.js:42-49 #5780），避免 CJK 名共享配额。

use super::{apply_cors_resp, BackendState};
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// 备份开关/参数（ST config.yaml backups.chat.*：enabled=true、throttleInterval=10s、
/// maxTotalBackups=-1 不限总量；integrity 校验恒开）
const CHAT_BACKUP_THROTTLE: Duration = Duration::from_secs(10);
const CHAT_BACKUPS_PREFIX: &str = "chat_";
/// settings 备份同款保留上限（removeOldBackups 默认 50）
const MAX_CHAT_BACKUPS: usize = 50;

/// 每会话的备份节流状态（chats.js:93-99：按「用户+会话名」分键）
#[derive(Default)]
pub struct ChatBackupState {
    last_fire: HashMap<String, Instant>,
}

pub struct ChatsService {
    pub backup: Mutex<ChatBackupState>,
}

impl ChatsService {
    pub fn new() -> Self {
        Self { backup: Mutex::new(ChatBackupState::default()) }
    }
}

impl Default for ChatsService {
    fn default() -> Self {
        Self::new()
    }
}

fn chats_dir(st: &BackendState) -> std::path::PathBuf {
    st.data_root.join("default-user").join("chats")
}

fn group_chats_dir(st: &BackendState) -> std::path::PathBuf {
    st.data_root.join("default-user").join("group chats")
}

fn groups_dir(st: &BackendState) -> std::path::PathBuf {
    st.data_root.join("default-user").join("groups")
}

fn backups_dir(st: &BackendState) -> std::path::PathBuf {
    st.data_root.join("default-user").join("backups")
}

/// getBackupKey（chats.js:42-49）：sanitize → 非 [a-z0-9]（i）→ '_' 小写化；
/// 名含非可打印 ASCII（CJK 等）→ 追加 sha256 前 8 位
fn backup_key(name: &str) -> String {
    let sanitized: String = crate::pathguard::sanitize(name)
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' })
        .collect();
    let has_non_ascii = name.chars().any(|c| !('\u{20}'..='\u{7E}').contains(&c));
    if has_non_ascii {
        let mut h = Sha256::new();
        h.update(name.as_bytes());
        let hex: String = format!("{:x}", h.finalize());
        format!("{sanitized}_{}", &hex[..8])
    } else {
        sanitized
    }
}

/// ST util.js generateTimestamp（秒级，备份去重靠 backup_key 前缀比对而非文件名）
fn generate_timestamp() -> String {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    let (y, mth, d, h, mi, s) = super::settings::civil_utc(now.as_secs() as i64);
    format!("{y:04}{mth:02}{d:02}-{h:02}{mi:02}{s:02}")
}

/// 内容相同 → 不备份（等价 settings 的 isDuplicateBackup；chat 备份无显式去重，
/// 但同秒同名覆盖等效去重 —— 这里显式比对更省 IO，语义向前兼容）
fn is_duplicate(dir: &std::path::Path, prefix: &str, content: &str) -> bool {
    let mut latest: Option<(std::path::PathBuf, SystemTime)> = None;
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let name_ok = e.file_name().to_str().map(|n| n.starts_with(prefix)).unwrap_or(false);
        if !name_ok {
            continue;
        }
        let t = e.metadata().and_then(|m| m.modified()).unwrap_or(UNIX_EPOCH);
        if latest.as_ref().map(|(_, lt)| t > *lt).unwrap_or(true) {
            latest = Some((e.path(), t));
        }
    }
    match latest {
        Some((p, _)) => std::fs::read_to_string(&p).map(|c| c == content).unwrap_or(false),
        None => false,
    }
}

/// removeOldBackups（util.js）：按前缀保留 50，超限删最旧
fn remove_old_backups(dir: &std::path::Path, prefix: &str) {
    let mut files: Vec<(std::path::PathBuf, SystemTime)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_name().to_str().map(|n| n.starts_with(prefix)).unwrap_or(false))
        .map(|e| {
            let mtime = e.metadata().and_then(|m| m.modified()).unwrap_or(UNIX_EPOCH);
            (e.path(), mtime)
        })
        .collect();
    if files.len() <= MAX_CHAT_BACKUPS {
        return;
    }
    files.sort_by_key(|(_, t)| *t);
    while files.len() > MAX_CHAT_BACKUPS {
        let oldest = files.remove(0);
        let _ = std::fs::remove_file(oldest.0);
    }
}

/// backupChat（chats.js:59-78）+ getBackupFunction 节流（chats.js:93-99，
/// leading+trailing 合并为「窗口内首触发生效」—— trailing 的语义由下一次
/// save 重新触发兜底，数据安全不受影响：备份节流丢的只是中间态副本）
fn backup_chat(st: &BackendState, svc: &ChatsService, chat_name: &str, data: &str) {
    let key = backup_key(chat_name);
    {
        let mut state = svc.backup.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(t) = state.last_fire.get(&key) {
            if t.elapsed() < CHAT_BACKUP_THROTTLE {
                return;
            }
        }
        state.last_fire.insert(key.clone(), Instant::now());
    }
    let dir = backups_dir(st);
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let prefix = format!("{CHAT_BACKUPS_PREFIX}{key}_");
    if is_duplicate(&dir, &prefix, data) {
        return;
    }
    let file = dir.join(format!("{prefix}{}.jsonl", generate_timestamp()));
    if std::fs::write(&file, data).is_ok() {
        remove_old_backups(&dir, &prefix);
    }
}

/// checkChatIntegrity（chats.js:337-369）：文件不存在/空 → 通过；首行（剥 BOM）
/// 非 JSON 对象 → 失败；无 integrity 元数据（旧卡）→ 通过；否则 slug 比对
fn check_integrity(file_path: &std::path::Path, slug: &str) -> bool {
    if !file_path.exists() {
        return true;
    }
    let Ok(text) = std::fs::read_to_string(file_path) else { return false };
    if text.is_empty() {
        return true;
    }
    let first = text.lines().next().unwrap_or("").trim_start_matches('\u{FEFF}');
    let Ok(head) = serde_json::from_str::<Value>(first) else { return false };
    if !head.is_object() {
        return false;
    }
    match head.get("chat_metadata").and_then(|m| m.get("integrity")).and_then(|i| i.as_str()) {
        Some(existing) => existing == slug,
        None => true,
    }
}

/// IntegrityMismatchError 的错误响应：`{error:'integrity'}` 400
fn integrity_conflict(origin: Option<HeaderValue>) -> Response {
    apply_cors_resp(
        (StatusCode::BAD_REQUEST, Json(json!({ "error": "integrity" }))).into_response(),
        origin,
    )
}

/// trySaveChat（chats.js:532-543）：join('\n') → integrity → 原子写 → 备份
/// 返回 Ok(()) / Err(Integrity) / Err(Other)
fn try_save_chat(
    st: &BackendState,
    svc: &ChatsService,
    chat_data: &[Value],
    file_path: &std::path::Path,
    skip_integrity: bool,
    backup_name: &str,
) -> Result<(), &'static str> {
    let jsonl = chat_data
        .iter()
        .map(|m| m.to_string())
        .collect::<Vec<_>>()
        .join("\n");

    if !skip_integrity {
        if let Some(slug) = chat_data
            .first()
            .and_then(|m| m.get("chat_metadata"))
            .and_then(|m| m.get("integrity"))
            .and_then(|i| i.as_str())
        {
            if !check_integrity(file_path, slug) {
                return Err("integrity");
            }
        }
    }

    // 原子写：tmp + rename（等价 tryWriteFileSync + write-file-atomic）
    if let Some(parent) = file_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let tmp = file_path.with_extension("jsonl.tmp");
    std::fs::write(&tmp, &jsonl).map_err(|_| "write")?;
    std::fs::rename(&tmp, file_path).map_err(|_| "write")?;
    backup_chat(st, svc, backup_name, &jsonl);
    Ok(())
}

/// getChatData（chats.js:577-590）：逐行 tryParse 过滤
fn get_chat_data(file_path: &std::path::Path) -> Vec<Value> {
    let Ok(text) = std::fs::read_to_string(file_path) else { return Vec::new() };
    text.lines().filter_map(|l| serde_json::from_str::<Value>(l).ok()).collect()
}

/// formatBytes（util.js:197 → bytes.format）：人类可读，1024 进制单字符单位
fn format_bytes(n: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];
    if n < 1024 {
        return format!("{n}B");
    }
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    // bytes 包默认保留 1 位小数去尾零
    let s = format!("{v:.1}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    format!("{s}{}", UNITS[i])
}

/// getChatInfo（chats.js:393-506）：单遍扫描——行数/末行 mes/首行 metadata
fn get_chat_info(
    file_path: &std::path::Path,
    additional: Option<Value>,
    with_metadata: bool,
    fragments: Option<&[String]>,
) -> Value {
    let mut out = json!({
        "match": false,
        "file_id": file_path.file_stem().and_then(|s| s.to_str()).unwrap_or(""),
        "file_name": file_path.file_name().and_then(|s| s.to_str()).unwrap_or(""),
        "file_size": format_bytes(std::fs::metadata(file_path).map(|m| m.len()).unwrap_or(0)),
        "chat_items": 0,
        "mes": "[The chat is empty]",
        "last_mes": 0,
    });
    if let Some(extra) = additional.and_then(|v| v.as_object().cloned()) {
        if let Some(o) = out.as_object_mut() {
            for (k, v) in extra {
                o.insert(k, v);
            }
        }
    }
    let Ok(text) = std::fs::read_to_string(file_path) else { return out };
    if text.is_empty() {
        return out;
    }
    let stat_mtime = std::fs::metadata(file_path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    let lines: Vec<&str> = text.lines().collect();
    let mut items = 0usize;
    let mut has_match = fragments.is_none();
    let mut last_mes = String::new();
    for (idx, line) in lines.iter().enumerate() {
        if with_metadata && idx == 0 {
            if let Ok(head) = serde_json::from_str::<Value>(line) {
                if let Some(meta) = head.get("chat_metadata") {
                    out.as_object_mut().unwrap().insert("chat_metadata".into(), meta.clone());
                }
            }
        }
        if let Ok(msg) = serde_json::from_str::<Value>(line) {
            let mes = msg.get("mes").and_then(|m| m.as_str()).unwrap_or("");
            if idx > 0 {
                if let Some(frags) = fragments {
                    if !has_match {
                        // matcher（chats.js:1013-1018）：每个片段至少命中任一文本
                        let mut buf: Vec<String> = vec![mes.to_string()];
                        buf.extend(last_mes_lines(&lines, idx));
                        has_match = frags.iter().all(|f| buf.iter().any(|t| t.to_lowercase().contains(f)));
                    }
                }
            }
            last_mes = mes.to_string();
        }
        items += 1;
    }
    let last_line = lines.last().copied().unwrap_or("");
    let last_ok = serde_json::from_str::<Value>(last_line)
        .map(|v| {
            v.get("name").is_some() || v.get("character_name").is_some() || v.get("chat_metadata").is_some()
        })
        .unwrap_or(false);

    let o = out.as_object_mut().unwrap();
    if last_ok {
        o.insert("chat_items".into(), json!(items.saturating_sub(1)));
        o.insert("mes".into(), json!(if last_mes.is_empty() { "[The message is empty]" } else { &last_mes }));
        // 末行 send_date 或 mtime ISO
        let send_date = serde_json::from_str::<Value>(last_line)
            .ok()
            .and_then(|v| v.get("send_date").cloned())
            .unwrap_or_else(|| json!(iso_utc(stat_mtime as i64)));
        o.insert("last_mes".into(), send_date);
        o.insert("match".into(), json!(has_match));
    } else {
        // 末行损坏：降级预览（chats.js:490-498），不隐藏会话
        o.insert("chat_items".into(), json!(items.saturating_sub(2).max(0)));
        o.insert("mes".into(), json!("[The message is empty]"));
        o.insert("last_mes".into(), json!(iso_utc(stat_mtime as i64)));
        o.insert("match".into(), json!(has_match));
    }
    out
}

/// matcher 的文本缓冲：当前行 + 此前行（简化版 hasTextMatch 的滑动窗口 ——
/// ST 的 matchBuffer 跨行累积，这里以「当前+上一行」覆盖主要场景；
/// 完整滑动窗口由调用方 hasTextMatch(fragments) 直接全文匹配兜底）
fn last_mes_lines<'a>(lines: &[&'a str], idx: usize) -> Vec<String> {
    let mut v = Vec::new();
    if idx >= 1 {
        if let Ok(p) = serde_json::from_str::<Value>(lines[idx - 1]) {
            if let Some(m) = p.get("mes").and_then(|m| m.as_str()) {
                v.push(m.to_string());
            }
        }
    }
    v
}

fn iso_utc(secs: i64) -> String {
    let (y, mth, d, h, mi, s) = super::settings::civil_utc(secs);
    format!("{y:04}-{mth:02}-{d:02}T{h:02}:{mi:02}:{s:02}.000Z")
}

fn err_json(status: StatusCode, msg: &str) -> Response {
    (status, Json(json!({ "error": { "message": msg } }))).into_response()
}

fn ok_json(origin: Option<HeaderValue>, v: Value) -> Response {
    apply_cors_resp(Json(v).into_response(), origin)
}

/* ------------------------------------------------------------------ *
 * Handlers
 * ------------------------------------------------------------------ */

/// POST /api/chats/save {avatar_url, file_name, chat[], force}（manifest #10）
pub fn save(st: &BackendState, svc: &ChatsService, body: &Value, origin: Option<HeaderValue>) -> Response {
    let Some(avatar) = body.get("avatar_url").and_then(|v| v.as_str()) else {
        return apply_cors_resp(err_json(StatusCode::BAD_REQUEST, "缺少 avatar_url").into_response(), origin);
    };
    let Some(file_name) = body.get("file_name").and_then(|v| v.as_str()) else {
        return apply_cors_resp(err_json(StatusCode::BAD_REQUEST, "缺少 file_name").into_response(), origin);
    };
    let Some(chat) = body.get("chat").and_then(|v| v.as_array()) else {
        return apply_cors_resp(
            (StatusCode::BAD_REQUEST, Json(json!({ "error": "The request's body.chat is not an array." })))
                .into_response(),
            origin,
        );
    };
    let stem = match crate::pathguard::component(avatar.trim_end_matches(".png")) {
        Ok(s) => s,
        Err(e) => return apply_cors_resp(err_json(StatusCode::FORBIDDEN, &e.to_string()).into_response(), origin),
    };
    let safe_file = match crate::pathguard::component(&format!("{file_name}.jsonl")) {
        Ok(s) => s,
        Err(e) => return apply_cors_resp(err_json(StatusCode::FORBIDDEN, &e.to_string()).into_response(), origin),
    };
    let path = chats_dir(st).join(&stem).join(safe_file);
    let force = body.get("force").and_then(|v| v.as_bool()).unwrap_or(false);
    match try_save_chat(st, svc, chat, &path, force, &stem) {
        Ok(()) => ok_json(origin, json!({ "ok": true })),
        Err("integrity") => integrity_conflict(origin),
        Err(_) => apply_cors_resp(
            err_json(StatusCode::INTERNAL_SERVER_ERROR, "会话写入失败").into_response(),
            origin,
        ),
    }
}

/// POST /api/chats/get {avatar_url, file_name}（manifest #9）
pub fn get(st: &BackendState, body: &Value, origin: Option<HeaderValue>) -> Response {
    let Some(avatar) = body.get("avatar_url").and_then(|v| v.as_str()) else {
        return ok_json(origin, json!({}));
    };
    let Ok(stem) = crate::pathguard::component(avatar.trim_end_matches(".png")) else {
        return ok_json(origin, json!({}));
    };
    let dir = chats_dir(st).join(&stem);
    if !dir.exists() {
        // ST：目录不存在则创建并返回 {}
        let _ = std::fs::create_dir_all(&dir);
        return ok_json(origin, json!({}));
    }
    let Some(file_name) = body.get("file_name").and_then(|v| v.as_str()) else {
        return ok_json(origin, json!({}));
    };
    let Ok(safe_file) = crate::pathguard::component(&format!("{file_name}.jsonl")) else {
        return ok_json(origin, json!({}));
    };
    ok_json(origin, Value::Array(get_chat_data(&dir.join(safe_file))))
}

/// POST /api/chats/rename {avatar_url, original_file, renamed_file, is_group}（manifest #12）
pub fn rename(st: &BackendState, body: &Value, origin: Option<HeaderValue>) -> Response {
    let (Some(original), Some(renamed)) = (
        body.get("original_file").and_then(|v| v.as_str()),
        body.get("renamed_file").and_then(|v| v.as_str()),
    ) else {
        return apply_cors_resp(err_json(StatusCode::BAD_REQUEST, "缺少 original_file/renamed_file").into_response(), origin);
    };
    let is_group = body.get("is_group").and_then(|v| v.as_bool()).unwrap_or(false);
    let folder = if is_group {
        group_chats_dir(st)
    } else {
        match body.get("avatar_url").and_then(|v| v.as_str()) {
            Some(a) => match crate::pathguard::component(a.trim_end_matches(".png")) {
                Ok(s) => chats_dir(st).join(s),
                Err(e) => return apply_cors_resp(err_json(StatusCode::FORBIDDEN, &e.to_string()).into_response(), origin),
            },
            None => return apply_cors_resp(err_json(StatusCode::BAD_REQUEST, "缺少 avatar_url").into_response(), origin),
        }
    };
    let (Ok(orig_safe), Ok(new_safe)) = (
        crate::pathguard::component(&format!("{original}.jsonl")),
        crate::pathguard::component(&format!("{renamed}.jsonl")),
    ) else {
        return apply_cors_resp(err_json(StatusCode::FORBIDDEN, "非法文件名").into_response(), origin);
    };
    let from = folder.join(orig_safe);
    let to = folder.join(new_safe);
    if !from.is_file() || to.exists() {
        return ok_json(origin, json!({ "error": true }));
    }
    match std::fs::rename(&from, &to) {
        Ok(()) => ok_json(origin, json!({ "ok": true, "sanitizedFileName": to.file_stem().and_then(|s| s.to_str()).unwrap_or("") })),
        Err(_) => ok_json(origin, json!({ "error": true })),
    }
}

/// POST /api/chats/delete {avatar_url, chatfile}（manifest #11）
pub fn delete(st: &BackendState, body: &Value, origin: Option<HeaderValue>) -> Response {
    let (Some(avatar), Some(chatfile)) = (
        body.get("avatar_url").and_then(|v| v.as_str()),
        body.get("chatfile").and_then(|v| v.as_str()),
    ) else {
        return apply_cors_resp(err_json(StatusCode::BAD_REQUEST, "缺少 avatar_url/chatfile").into_response(), origin);
    };
    let chatfile = if !chatfile.contains('.') { format!("{chatfile}.jsonl") } else { chatfile.to_string() };
    let Ok(stem) = crate::pathguard::component(avatar.trim_end_matches(".png")) else {
        return apply_cors_resp(err_json(StatusCode::FORBIDDEN, "非法目录名").into_response(), origin);
    };
    let Ok(safe_file) = crate::pathguard::component(&chatfile) else {
        return apply_cors_resp(err_json(StatusCode::FORBIDDEN, "非法文件名").into_response(), origin);
    };
    let p = chats_dir(st).join(stem).join(safe_file);
    match std::fs::remove_file(&p) {
        Ok(()) => ok_json(origin, json!({ "ok": true })),
        Err(_) => apply_cors_resp(StatusCode::BAD_REQUEST.into_response(), origin),
    }
}

/// POST /api/chats/search {query, avatar_url, group_id}（manifest #14）
pub fn search(st: &BackendState, body: &Value, origin: Option<HeaderValue>) -> Response {
    let query = body.get("query").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let fragments: Vec<String> = if query.trim().is_empty() {
        Vec::new()
    } else {
        query.trim().to_lowercase().split_whitespace().map(|s| s.to_string()).collect()
    };
    let group_id = body.get("group_id").and_then(|v| v.as_str());

    let chat_files: Vec<std::path::PathBuf> = if let Some(gid) = group_id {
        // group.chats 索引 → group_chats/<id>.jsonl
        let mut found = Vec::new();
        if let Ok(rd) = std::fs::read_dir(groups_dir(st)) {
            for e in rd.flatten() {
                let p = e.path();
                if p.extension().and_then(|x| x.to_str()) != Some("json") {
                    continue;
                }
                if let Ok(text) = std::fs::read_to_string(&p) {
                    if let Ok(g) = serde_json::from_str::<Value>(&text) {
                        if g.get("id").and_then(|i| i.as_str()) == Some(gid) {
                            if let Some(chats) = g.get("chats").and_then(|c| c.as_array()) {
                                for c in chats {
                                    if let Some(id) = c.as_str() {
                                        let f = group_chats_dir(st).join(format!("{id}.jsonl"));
                                        if f.is_file() {
                                            found.push(f);
                                        }
                                    }
                                }
                            }
                            break;
                        }
                    }
                }
            }
        }
        found
    } else {
        let Some(avatar) = body.get("avatar_url").and_then(|v| v.as_str()) else {
            return ok_json(origin, json!([]));
        };
        let Ok(stem) = crate::pathguard::component(avatar.trim_end_matches(".png")) else {
            return ok_json(origin, json!([]));
        };
        let dir = chats_dir(st).join(stem);
        if !dir.is_dir() {
            return ok_json(origin, json!([]));
        }
        let mut files = Vec::new();
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for e in rd.flatten() {
                if e.path().extension().and_then(|x| x.to_str()) == Some("jsonl") {
                    files.push(e.path());
                }
            }
        }
        files
    };

    let mut results = Vec::new();
    for f in chat_files {
        let info = get_chat_info(&f, None, false, Some(&fragments));
        let file_id = info.get("file_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let items = info.get("chat_items").and_then(|v| v.as_u64()).unwrap_or(0);
        let matched = info.get("match").and_then(|v| v.as_bool()).unwrap_or(false);
        // 文件名命中也算（chats.js:1023）
        let name_hit = fragments.iter().all(|fr| file_id.to_lowercase().contains(fr));
        if file_id.is_empty() {
            continue;
        }
        if !fragments.is_empty() && items == 0 && !matched && !name_hit {
            continue;
        }
        if fragments.is_empty() || matched || name_hit {
            results.push(json!({
                "file_name": file_id,
                "file_size": info.get("file_size").cloned().unwrap_or_default(),
                "message_count": items,
                "last_mes": info.get("last_mes").cloned().unwrap_or_default(),
                "preview_message": preview_message(info.get("mes").and_then(|v| v.as_str()).unwrap_or("")),
            }));
        }
    }
    ok_json(origin, json!(results))
}

/// getPreviewMessage（chats.js:106-116）：末 400 字符 + 前缀 ...
fn preview_message(last: &str) -> String {
    const STRLEN: usize = 400;
    if last.is_empty() {
        return String::new();
    }
    let chars: Vec<char> = last.chars().collect();
    if chars.len() > STRLEN {
        let tail: String = chars[chars.len() - STRLEN..].iter().collect();
        format!("...{tail}")
    } else {
        last.to_string()
    }
}

/// POST /api/chats/recent {max, metadata, pinned[]}（manifest #13）
/// 三来源聚合：characters/<stem>/、chats 根 jsonl、groups → group_chats。
pub fn recent(st: &BackendState, svc: &ChatsService, body: &Value, origin: Option<HeaderValue>) -> Response {
    let _ = svc; // 备份状态不在 recent 使用；保留签名一致性
    let with_metadata = body.get("metadata").and_then(|v| v.as_bool()).unwrap_or(false);
    let max = body.get("max").and_then(|v| v.as_u64()).unwrap_or(u64::MAX) as usize;
    let pinned = body.get("pinned").and_then(|v| v.as_array()).cloned().unwrap_or_default();

    struct Item {
        path: std::path::PathBuf,
        mtime: u64,
        extra: Value,
    }
    let mut all: Vec<Item> = Vec::new();

    // 1) characters/<stem>/*.jsonl
    if let Ok(rd) = std::fs::read_dir(chats_dir(st)) {
        for e in rd.flatten() {
            let dir = e.path();
            if !dir.is_dir() {
                continue;
            }
            let stem = dir.file_name().and_then(|s| s.to_str()).unwrap_or("").to_string();
            if let Ok(files) = std::fs::read_dir(&dir) {
                for f in files.flatten() {
                    let p = f.path();
                    if p.extension().and_then(|x| x.to_str()) == Some("jsonl") {
                        let mtime = f.metadata().ok().and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_millis() as u64).unwrap_or(0);
                        all.push(Item { path: p, mtime, extra: json!({ "avatar": format!("{stem}.png") }) });
                    }
                }
            }
        }
    }
    // 2) chats 根 jsonl
    if let Ok(rd) = std::fs::read_dir(chats_dir(st)) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_file() && p.extension().and_then(|x| x.to_str()) == Some("jsonl") {
                let mtime = e.metadata().ok().and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_millis() as u64).unwrap_or(0);
                all.push(Item { path: p, mtime, extra: json!({}) });
            }
        }
    }
    // 3) groups/*.json 的 chats[] → group_chats/<id>.jsonl
    if let Ok(rd) = std::fs::read_dir(groups_dir(st)) {
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) != Some("json") {
                continue;
            }
            if let Ok(text) = std::fs::read_to_string(&p) {
                if let Ok(g) = serde_json::from_str::<Value>(&text) {
                    let gid = g.get("id").and_then(|i| i.as_str()).unwrap_or("").to_string();
                    if let Some(chats) = g.get("chats").and_then(|c| c.as_array()) {
                        for c in chats {
                            if let Some(id) = c.as_str() {
                                let f = group_chats_dir(st).join(format!("{id}.jsonl"));
                                if f.is_file() {
                                    let mtime = std::fs::metadata(&f).ok().and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_millis() as u64).unwrap_or(0);
                                    all.push(Item { path: f, mtime, extra: json!({ "group": gid }) });
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // 置顶优先（pinned: [{file_name, avatar|group}]），再按 mtime 倒序
    let is_pinned = |it: &Item| -> bool {
        let fname = it.path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        pinned.iter().any(|p| {
            p.get("file_name").and_then(|f| f.as_str()) == Some(fname)
                && (p.get("avatar").and_then(|a| a.as_str()) == it.extra.get("avatar").and_then(|a| a.as_str())
                    || p.get("group").and_then(|a| a.as_str()) == it.extra.get("group").and_then(|a| a.as_str()))
        })
    };
    all.sort_by(|a, b| {
        let (pa, pb) = (is_pinned(a), is_pinned(b));
        match (pa, pb) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => b.mtime.cmp(&a.mtime),
        }
    });
    all.truncate(max);

    let mut out = Vec::new();
    for it in all {
        let info = get_chat_info(&it.path, Some(it.extra), with_metadata, None);
        if info.get("file_name").and_then(|v| v.as_str()).map(|s| !s.is_empty()).unwrap_or(false) {
            out.push(info);
        }
    }
    ok_json(origin, json!(out))
}

/* ------------------------------------------------------------------ *
 * 群聊端点（manifest #15-#18；group_chats/<id>.jsonl 同款 JSONL 语义）
 * ------------------------------------------------------------------ */

/// POST /api/chats/group/get {id}（manifest #15）
pub fn group_get(st: &BackendState, body: &Value, origin: Option<HeaderValue>) -> Response {
    let Some(id) = body.get("id").and_then(|v| v.as_str()) else {
        return apply_cors_resp(StatusCode::BAD_REQUEST.into_response(), origin);
    };
    let Ok(safe) = crate::pathguard::component(&format!("{id}.jsonl")) else {
        return apply_cors_resp(StatusCode::BAD_REQUEST.into_response(), origin);
    };
    ok_json(origin, Value::Array(get_chat_data(&group_chats_dir(st).join(safe))))
}

/// POST /api/chats/group/info {id}（manifest #18）
pub fn group_info(st: &BackendState, body: &Value, origin: Option<HeaderValue>) -> Response {
    let Some(id) = body.get("id").and_then(|v| v.as_str()) else {
        return apply_cors_resp(StatusCode::BAD_REQUEST.into_response(), origin);
    };
    let Ok(safe) = crate::pathguard::component(&format!("{id}.jsonl")) else {
        return apply_cors_resp(StatusCode::BAD_REQUEST.into_response(), origin);
    };
    ok_json(origin, get_chat_info(&group_chats_dir(st).join(safe), None, true, None))
}

/// POST /api/chats/group/delete {id}（manifest #17）
pub fn group_delete(st: &BackendState, body: &Value, origin: Option<HeaderValue>) -> Response {
    let Some(id) = body.get("id").and_then(|v| v.as_str()) else {
        return apply_cors_resp(StatusCode::BAD_REQUEST.into_response(), origin);
    };
    let Ok(safe) = crate::pathguard::component(&format!("{id}.jsonl")) else {
        return apply_cors_resp(StatusCode::BAD_REQUEST.into_response(), origin);
    };
    match std::fs::remove_file(group_chats_dir(st).join(safe)) {
        Ok(()) => ok_json(origin, json!({ "ok": true })),
        Err(_) => apply_cors_resp(StatusCode::BAD_REQUEST.into_response(), origin),
    }
}

/// POST /api/chats/group/save {id, chat[], force}（manifest #16）
pub fn group_save(st: &BackendState, svc: &ChatsService, body: &Value, origin: Option<HeaderValue>) -> Response {
    let Some(id) = body.get("id").and_then(|v| v.as_str()) else {
        return apply_cors_resp(StatusCode::BAD_REQUEST.into_response(), origin);
    };
    let Some(chat) = body.get("chat").and_then(|v| v.as_array()) else {
        return ok_json(origin, json!({ "error": "The request's body.chat is not an array." }));
    };
    let Ok(safe) = crate::pathguard::component(&format!("{id}.jsonl")) else {
        return apply_cors_resp(StatusCode::BAD_REQUEST.into_response(), origin);
    };
    let path = group_chats_dir(st).join(safe);
    let force = body.get("force").and_then(|v| v.as_bool()).unwrap_or(false);
    match try_save_chat(st, svc, chat, &path, force, id) {
        Ok(()) => ok_json(origin, json!({ "ok": true })),
        Err("integrity") => integrity_conflict(origin),
        Err(_) => ok_json(origin, json!({ "error": "An error has occurred, see the console logs for more information." })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn new_chats() -> std::sync::Arc<super::super::chats::ChatsService> {
        std::sync::Arc::new(super::super::chats::ChatsService::new())
    }

    fn new_maid() -> std::sync::Arc<super::super::extras::MaidTokens> {
        std::sync::Arc::new(super::super::extras::MaidTokens::new())
    }
    use std::sync::Arc;

    fn st_with(tag: &str) -> BackendState {
        let root = std::env::temp_dir().join(format!("stchat-chats-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("default-user").join("chats").join("chara")).unwrap();
        std::fs::create_dir_all(root.join("default-user").join("group chats")).unwrap();
        std::fs::create_dir_all(root.join("default-user").join("groups")).unwrap();
        BackendState {
            token: Arc::new("t".into()),
            data_root: root,
            max_body_bytes: 1024,
            settings_svc: Arc::new(super::super::settings::SettingsService::new()),
            chats_svc: new_chats(),
            maid_tokens: new_maid(),
        }
    }

    fn svc() -> ChatsService {
        ChatsService::new()
    }

    #[test]
    fn save_get_roundtrip_keeps_unknown_fields() {
        let st = st_with("roundtrip");
        let s = svc();
        let chat = json!([
            { "name": "U", "is_user": true, "mes": "hi", "chat_metadata": { "integrity": "abc" }, "extra": { "nested": true } },
            { "name": "C", "mes": "hello", "swipes": ["a"], "unknown_field": 42 },
        ]);
        let body = json!({ "avatar_url": "chara.png", "file_name": "c1", "chat": chat });
        let r = save(&st, &s, &body, None);
        assert_eq!(r.status().as_u16(), 200);

        let r = get(&st, &json!({ "avatar_url": "chara.png", "file_name": "c1" }), None);
        let bytes = tokio::runtime::Runtime::new().unwrap().block_on(async {
            axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap()
        });
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        let arr = v.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[1]["unknown_field"], 42, "未知字段保真");
        assert_eq!(arr[0]["extra"]["nested"], true);
        assert_eq!(arr[0]["chat_metadata"]["integrity"], "abc");
    }

    #[test]
    fn integrity_mismatch_rejected_and_force_overrides() {
        let st = st_with("integrity");
        let s = svc();
        // 首次写入带 integrity slug
        let chat1 = json!([
            { "name": "U", "is_user": true, "mes": "v1", "chat_metadata": { "integrity": "slug-1" } },
        ]);
        save(&st, &s, &json!({ "avatar_url": "chara.png", "file_name": "c", "chat": chat1 }), None);

        // 不同 slug → 400 {error:'integrity'}
        let chat2 = json!([
            { "name": "U", "is_user": true, "mes": "v2", "chat_metadata": { "integrity": "slug-2" } },
        ]);
        let r = save(&st, &s, &json!({ "avatar_url": "chara.png", "file_name": "c", "chat": chat2 }), None);
        assert_eq!(r.status().as_u16(), 400);
        let bytes = tokio::runtime::Runtime::new().unwrap().block_on(async {
            axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap()
        });
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["error"], "integrity");

        // force=true 覆盖
        let r = save(&st, &s, &json!({ "avatar_url": "chara.png", "file_name": "c", "chat": chat2, "force": true }), None);
        assert_eq!(r.status().as_u16(), 200);

        // 无 integrity 元数据的旧卡 → 通过
        let legacy = json!([{ "chat_metadata": {} }, { "name": "C", "mes": "legacy" }]);
        let r = save(&st, &s, &json!({ "avatar_url": "chara.png", "file_name": "legacy", "chat": legacy }), None);
        assert_eq!(r.status().as_u16(), 200);
    }

    #[test]
    fn chat_backup_throttle_and_key() {
        // CJK key 带 hash（chats.js:42-49）
        let k1 = backup_key("林晚棠");
        let k2 = backup_key("林晚棠2");
        assert_ne!(k1, k2, "不同 CJK 名不得共享配额");
        assert!(k1.ends_with(|c: char| c.is_ascii_hexdigit()), "带 sha256 后缀");
        assert_eq!(backup_key("plain-chat"), "plain_chat");

        // 节流：同 key 连续调用只备份一次；内容变化后窗口内也不再备份（简化语义）
        let st = st_with("bk");
        let s = svc();
        backup_chat(&st, &s, "x", "content-1");
        let dir = backups_dir(&st);
        let count = std::fs::read_dir(&dir).unwrap().count();
        assert_eq!(count, 1);
        backup_chat(&st, &s, "x", "content-2");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1, "节流窗口内不重复");
    }

    #[test]
    fn rename_delete_basic() {
        let st = st_with("rd");
        let s = svc();
        let chat = json!([{ "chat_metadata": {} }, { "name": "C", "mes": "x" }]);
        save(&st, &s, &json!({ "avatar_url": "chara.png", "file_name": "old", "chat": chat }), None);
        let dir = chats_dir(&st).join("chara");

        // rename：原文件存在、目标不存在
        let r = rename(&st, &json!({ "avatar_url": "chara.png", "original_file": "old", "renamed_file": "new" }), None);
        assert_eq!(r.status().as_u16(), 200);
        assert!(dir.join("new.jsonl").is_file());
        assert!(!dir.join("old.jsonl").exists());
        // 目标已存在 → {error:true}
        save(&st, &s, &json!({ "avatar_url": "chara.png", "file_name": "other", "chat": chat }), None);
        let r = rename(&st, &json!({ "avatar_url": "chara.png", "original_file": "new", "renamed_file": "other" }), None);
        let bytes = tokio::runtime::Runtime::new().unwrap().block_on(async {
            axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap()
        });
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["error"], true);

        // delete：不带扩展名自动补 .jsonl
        let r = delete(&st, &json!({ "avatar_url": "chara.png", "chatfile": "new" }), None);
        let bytes = tokio::runtime::Runtime::new().unwrap().block_on(async {
            axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap()
        });
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["ok"], true);
        assert!(!dir.join("new.jsonl").exists());
    }

    #[test]
    fn search_matches_messages_and_names() {
        let st = st_with("search");
        let s = svc();
        save(&st, &s, &json!({ "avatar_url": "chara.png", "file_name": "hit", "chat": json!([
            { "name": "U", "is_user": true, "chat_metadata": {} },
            { "name": "C", "mes": "the quick brown fox" },
        ]) }), None);
        save(&st, &s, &json!({ "avatar_url": "chara.png", "file_name": "miss", "chat": json!([
            { "name": "U", "is_user": true, "chat_metadata": {} },
            { "name": "C", "mes": "nothing here" },
        ]) }), None);

        let r = search(&st, &json!({ "avatar_url": "chara.png", "query": "QUICK fox" }), None);
        let bytes = tokio::runtime::Runtime::new().unwrap().block_on(async {
            axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap()
        });
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        let arr = v.as_array().unwrap();
        assert_eq!(arr.len(), 1, "大小写不敏感 + 多片段 AND");
        assert_eq!(arr[0]["file_name"], "hit");
        assert!(arr[0]["preview_message"].as_str().unwrap().contains("quick"));

        // 文件名命中也算
        let r = search(&st, &json!({ "avatar_url": "chara.png", "query": "miss" }), None);
        let bytes = tokio::runtime::Runtime::new().unwrap().block_on(async {
            axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap()
        });
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v.as_array().unwrap().len(), 1);
    }

    #[test]
    fn recent_merges_three_sources_pinned_first() {
        let st = st_with("recent");
        let s = svc();
        // 角色会话
        save(&st, &s, &json!({ "avatar_url": "chara.png", "file_name": "c1", "chat": json!([
            { "name": "U", "is_user": true, "chat_metadata": { "note_prompt": "AN" } },
            { "name": "C", "mes": "char chat" },
        ]), "metadata": true }), None);
        // 群聊：groups/g.json → group_chats/gc1.jsonl
        std::fs::write(
            groups_dir(&st).join("g.json"),
            json!({ "id": "grp-1", "chats": ["gc1"] }).to_string(),
        ).unwrap();
        std::fs::write(
            group_chats_dir(&st).join("gc1.jsonl"),
            "{\"chat_metadata\":{}}\n{\"mes\":\"group chat\"}\n",
        ).unwrap();
        // 根 jsonl
        std::fs::write(chats_dir(&st).join("loose.jsonl"), "{\"chat_metadata\":{}}\n{\"mes\":\"loose\"}\n").unwrap();

        let r = recent(&st, &s, &json!({ "max": 10, "metadata": true }), None);
        let bytes = tokio::runtime::Runtime::new().unwrap().block_on(async {
            axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap()
        });
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        let arr = v.as_array().unwrap();
        assert_eq!(arr.len(), 3, "三来源聚合");
        // 每项带来源标记（avatar / group）
        assert!(arr.iter().any(|i| i["avatar"] == "chara.png"));
        assert!(arr.iter().any(|i| i["group"] == "grp-1"));
        assert!(arr.iter().any(|i| i["chat_metadata"].is_object()), "metadata=true 读首行");
    }

    #[test]
    fn group_endpoints_roundtrip() {
        let st = st_with("group");
        let s = svc();
        let chat = json!([{ "chat_metadata": {} }, { "name": "C", "mes": "grp msg" }]);
        let r = group_save(&st, &s, &json!({ "id": "gc1", "chat": chat }), None);
        assert_eq!(r.status().as_u16(), 200);

        let r = group_get(&st, &json!({ "id": "gc1" }), None);
        let bytes = tokio::runtime::Runtime::new().unwrap().block_on(async {
            axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap()
        });
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v.as_array().unwrap().len(), 2);

        let r = group_info(&st, &json!({ "id": "gc1" }), None);
        let bytes = tokio::runtime::Runtime::new().unwrap().block_on(async {
            axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap()
        });
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["chat_items"], 1);

        let r = group_delete(&st, &json!({ "id": "gc1" }), None);
        let bytes = tokio::runtime::Runtime::new().unwrap().block_on(async {
            axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap()
        });
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["ok"], true);
    }

    #[test]
    fn format_bytes_human_readable() {
        assert_eq!(format_bytes(0), "0B");
        assert_eq!(format_bytes(512), "512B");
        assert_eq!(format_bytes(2048), "2KB");
        assert_eq!(format_bytes(1536), "1.5KB");
    }

    #[test]
    fn preview_message_truncates_tail() {
        assert_eq!(preview_message(""), "");
        let long: String = "x".repeat(500);
        let p = preview_message(&long);
        assert!(p.starts_with("..."));
        assert_eq!(p.chars().count(), 403);
    }
}
