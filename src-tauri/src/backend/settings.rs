//! backend/settings —— settings.json 数据服务（Phase 1 族一 · manifest #28/#29）
//!
//! ST 契约（settings.js:206-296，已逐行核对）：
//! - `/get`：返回 `{settings: <文件原文字符串>, ...presets 聚合}`；
//!   前端 data.ts:503-514 只消费 `settings`（worlds/QR 预设走独立端点或 settings 本体）。
//!   读失败 → 500。App 形态单用户，presets 聚合字段返回空数组占位（前端不消费）。
//! - `/save`：body 即 settings 本体，`JSON.stringify(body, null, 4)` 原子写；
//!   触发 autosave 备份（10 分钟节流 + 与最新备份字节级去重，settings.js:36-46/136-148）；
//!   返回 `{result:'ok'}`。
//! - 备份策略（评审 A3 要求复刻）：文件名 `settings_default-user_<yyyymmdd-hhmmss>.json`，
//!   存 `<data_root>/default-user/backups/`，保留上限 50（config backups.common.numberOfBackups），
//!   超限删最旧。
//!
//! ⚠ 与评审 A3 的对应：前端写已串行化（settings-writequeue.ts）；本文件是后端
//! 单写入点 —— Rust 侧另有 Mutex 串行化（单用户本地无并发写者，但保持纪律）。

use super::BackendState;
use axum::http::HeaderValue;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::{json, Value};
use std::sync::Mutex;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// 备份保留上限（ST config.yaml:92 `backups.common.numberOfBackups: 50`）
const MAX_BACKUPS: usize = 50;
/// autosave 节流窗口（ST settings.js:23 AUTOSAVE_INTERVAL = 10 分钟）
const AUTOSAVE_INTERVAL: std::time::Duration = std::time::Duration::from_secs(10 * 60);

/// 备份节流状态（进程内；单用户单进程，Mutex 足够）
#[derive(Default)]
pub struct AutosaveState {
    last_fire: Option<Instant>,
}

/// native 配置段（patch_native_config）的读-改-写串行锁：
/// 防并发 read-modify-write 互相吞键（store_sync 与 migrate_existing 双写入者）。
/// 模块级而非 SettingsService 字段 —— vault.rs 调用方只传 data_root，签名不动。
static NATIVE_CONFIG_LOCK: Mutex<()> = Mutex::new(());

pub struct SettingsService {
    pub autosave: Mutex<AutosaveState>,
}

impl SettingsService {
    pub fn new() -> Self {
        Self {
            autosave: Mutex::new(AutosaveState::default()),
        }
    }
}

impl Default for SettingsService {
    fn default() -> Self {
        Self::new()
    }
}

fn settings_path(data_root: &std::path::Path) -> std::path::PathBuf {
    data_root.join("default-user").join("settings.json")
}

fn backups_dir(data_root: &std::path::Path) -> std::path::PathBuf {
    data_root.join("default-user").join("backups")
}

/// 4 空格 pretty —— 对齐 ST settings.js:209 的 JSON.stringify(settings, null, 4)。
/// serde_json 的 to_string_pretty 固定 2 空格，这里显式用 PrettyFormatter 对齐
/// ST 落盘形态（不引新依赖；与 providers::json_pretty4 同口径）。
fn to_pretty_4(v: &Value) -> String {
    let fmt = serde_json::ser::PrettyFormatter::with_indent(b"    ");
    let mut ser = serde_json::Serializer::with_formatter(Vec::new(), fmt);
    let _ = serde::Serialize::serialize(v, &mut ser);
    String::from_utf8_lossy(&ser.into_inner()).to_string()
}

/// ST util.js generateTimestamp：`yyyymmdd-hhmmss`（本地时区）。
/// ⚠ 追加毫秒 —— ST 原版秒级在同秒内两次备份会同名互相覆盖（其 10 分钟节流
/// 使碰撞概率可忽略）；测试与高频写入需要唯一性，毫秒精度不会影响排序。
fn generate_timestamp() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs() as i64;
    let ms = now.subsec_millis();
    let dt = civil_utc(secs);
    format!(
        "{:04}{:02}{:02}-{:02}{:02}{:02}-{:03}",
        dt.0, dt.1, dt.2, dt.3, dt.4, dt.5, ms
    )
}

/// 简易 UTC 时间分解（避免新增 chrono 依赖；文件名仅作排序/唯一用途）。
/// pub 供 chats.rs 的时间戳共用（civil_from_days，Howard Hinnant 算法）。
pub fn civil_utc(secs: i64) -> (i64, u32, u32, u32, u32, u32) {
    let days = secs.div_euclid(86400);
    let rem = secs.rem_euclid(86400);
    let (h, m, s) = (
        (rem / 3600) as u32,
        ((rem % 3600) / 60) as u32,
        (rem % 60) as u32,
    );
    // civil_from_days（Howard Hinnant 算法）
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let mth = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = if mth <= 2 { y + 1 } else { y };
    (year, mth, d, h, m, s)
}

const SETTINGS_BACKUP_PREFIX: &str = "settings_default-user_";

/// 备份当前 settings（等价 backupUserSettings：去重 + 保留上限）
fn backup_user_settings(
    data_root: &std::path::Path,
    prevent_duplicates: bool,
) -> std::io::Result<()> {
    let source = settings_path(data_root);
    if !source.exists() {
        return Ok(());
    }
    let content = std::fs::read(&source)?;
    let dir = backups_dir(data_root);
    std::fs::create_dir_all(&dir)?;

    if prevent_duplicates && is_duplicate_backup(&dir, &content) {
        return Ok(());
    }

    let backup = dir.join(format!(
        "{SETTINGS_BACKUP_PREFIX}{}.json",
        generate_timestamp()
    ));
    std::fs::write(&backup, &content)?;
    remove_old_backups(&dir);
    log::debug!(target: super::logx::FS, "settings 备份 → {}", super::logx::redact(&backup.display().to_string()));
    Ok(())
}

/// 等价 isDuplicateBackup（settings.js:164-170）：与最新备份字节级比对
fn is_duplicate_backup(dir: &std::path::Path, content: &[u8]) -> bool {
    let latest = latest_backup(dir);
    let Some(latest) = latest else { return false };
    std::fs::read(&latest)
        .map(|c| c == content)
        .unwrap_or(false)
}

/// 等价 getLatestBackup（settings.js:192-202）：按 ctime 取最新
fn latest_backup(dir: &std::path::Path) -> Option<std::path::PathBuf> {
    let mut best: Option<(std::path::PathBuf, std::time::SystemTime)> = None;
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = entry.path();
        if !p.is_file() {
            continue;
        }
        let name_ok = p
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.starts_with(SETTINGS_BACKUP_PREFIX))
            .unwrap_or(false);
        if !name_ok {
            continue;
        }
        let ctime = entry
            .metadata()
            .and_then(|m| m.created().or_else(|_| m.modified()))
            .unwrap_or(UNIX_EPOCH);
        if best.as_ref().map(|(_, t)| ctime > *t).unwrap_or(true) {
            best = Some((p, ctime));
        }
    }
    best.map(|(p, _)| p)
}

/// 等价 removeOldBackups（util.js）：超 50 删最旧（按 mtime）
fn remove_old_backups(dir: &std::path::Path) {
    let mut files: Vec<(std::path::PathBuf, std::time::SystemTime)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| {
            e.file_name()
                .to_str()
                .map(|n| n.starts_with(SETTINGS_BACKUP_PREFIX))
                .unwrap_or(false)
        })
        .map(|e| {
            let mtime = e
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(UNIX_EPOCH);
            (e.path(), mtime)
        })
        .collect();
    if files.len() <= MAX_BACKUPS {
        return;
    }
    files.sort_by_key(|(_, t)| *t);
    while files.len() > MAX_BACKUPS {
        let oldest = files.remove(0);
        let _ = std::fs::remove_file(oldest.0);
    }
}

/// 触发 autosave（等价 triggerAutoSave：_.throttle 10 分钟 + 去重）
/// 返回是否真的执行了备份（测试断言用）。
pub fn trigger_autosave(st: &BackendState, svc: &SettingsService) -> bool {
    let mut state = svc.autosave.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(t) = state.last_fire {
        if t.elapsed() < AUTOSAVE_INTERVAL {
            return false;
        }
    }
    state.last_fire = Some(Instant::now());
    drop(state);
    backup_user_settings(&st.data_root, true).is_ok()
}

/* ------------------------------------------------------------------ *
 * Handlers（由 api.rs route() 分发接入）
 * ------------------------------------------------------------------ */

/// POST /api/settings/get —— settings 为文件原文字符串（前端 data.ts:503 双形态兼容）
pub fn get(st: &BackendState, origin: Option<HeaderValue>) -> Response {
    let p = settings_path(&st.data_root);
    let settings = match std::fs::read_to_string(&p) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // ST getUserSettings 语义：用户 settings.json 缺失 = 首次运行 →
            // 落地默认模板并照常返回（裸 500 会让干净机器首启全页设置读写 500）
            if let Err(e) = seed_default_settings(&st.data_root) {
                let resp = (
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": { "message": format!("settings.json 初始化失败：{e}") } })),
                )
                    .into_response();
                return super::apply_cors_resp(resp, origin);
            }
            DEFAULT_SETTINGS_TEMPLATE.to_string()
        }
        Err(e) => {
            // ST 对读失败回 500（settings.js:225）
            log::error!(target: super::logx::FS, "settings/get 读取失败: {e}");
            let resp = (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": { "message": format!("settings.json 读取失败：{e}") } })),
            )
                .into_response();
            return super::apply_cors_resp(resp, origin);
        }
    };
    // 前端只消费 settings 字段；presets 聚合字段占位空数组（App 形态无多预设目录）
    let body = Json(json!({ "settings": settings }));
    super::apply_cors_resp(body.into_response(), origin)
}

/// 默认 settings 模板（ST default/settings.json 的 App 子集形态）：
/// App 前端自有默认值兜底，这里只保证合法 JSON 空配置落地。
const DEFAULT_SETTINGS_TEMPLATE: &str = "{}";

/// 首启种子：default-user/settings.json 缺失时写默认模板（原子写，目录一并创建）
fn seed_default_settings(data_root: &std::path::Path) -> std::io::Result<()> {
    let p = settings_path(data_root);
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = p.with_extension("json.tmp");
    std::fs::write(&tmp, DEFAULT_SETTINGS_TEMPLATE.as_bytes())
        .and_then(|_| std::fs::rename(&tmp, &p))
        .inspect_err(|_e| {
            let _ = std::fs::remove_file(&tmp);
        })
}

/// POST /api/settings/save —— body 即 settings 本体（整体覆盖）。
/// 缩进：已对齐 ST 的 4 空格（settings.js:209 JSON.stringify(body, null, 4)）。
pub fn save(
    st: &BackendState,
    svc: &SettingsService,
    body: &Value,
    origin: Option<HeaderValue>,
) -> Response {
    // body 必须是 JSON 对象（数组/标量/空都不是合法 settings 本体 → 400）
    if !body.is_object() {
        let resp = (
            axum::http::StatusCode::BAD_REQUEST,
            Json(json!({ "error": { "message": "settings/save 的 body 必须是 JSON 对象" } })),
        )
            .into_response();
        return super::apply_cors_resp(resp, origin);
    }
    // 三铁律之一（data.ts:564-577）：禁包装对象 —— 防把 {settings:"..."} 写成配置本体
    if body.get("settings").is_some() && body.as_object().map(|o| o.len()).unwrap_or(0) == 1 {
        let resp = (
            axum::http::StatusCode::BAD_REQUEST,
            Json(json!({ "error": { "message": "settings/save 的 body 必须是 settings 本体，禁止 {settings:...} 包装对象" } })),
        )
            .into_response();
        return super::apply_cors_resp(resp, origin);
    }

    let p = settings_path(&st.data_root);
    if let Some(parent) = p.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            let resp = (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": { "message": format!("数据目录创建失败：{e}") } })),
            )
                .into_response();
            return super::apply_cors_resp(resp, origin);
        }
    }
    // 已对齐 ST 的 4 空格（settings.js:209 JSON.stringify(body, null, 4)），
    // 不再有 serde 默认 2 空格的偏差
    let pretty = to_pretty_4(body);
    // 原子写：临时文件 + rename（等价 write-file-atomic；规避损坏半写）
    let tmp = p.with_extension("json.tmp");
    if let Err(e) = std::fs::write(&tmp, pretty.as_bytes()).and_then(|_| std::fs::rename(&tmp, &p))
    {
        let _ = std::fs::remove_file(&tmp);
        log::error!(target: super::logx::FS, "settings/save 写入失败: {e}");
        let resp = (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": { "message": format!("settings 写入失败：{e}") } })),
        )
            .into_response();
        return super::apply_cors_resp(resp, origin);
    }
    log::info!(target: super::logx::FS, "settings/save 落盘 bytes={}", pretty.len());

    // 触发 autosave（10 分钟节流 + 去重）—— 评审 A3：复刻 ST 的节流策略，
    // 与前端「先比对一致就不写」配合，防止 backups 目录爆量
    let backed_up = trigger_autosave(st, svc);
    log::debug!(target: super::logx::FS, "settings autosave triggered={backed_up}");

    let resp = Json(json!({ "result": "ok" }));
    super::apply_cors_resp(resp.into_response(), origin)
}

/* ------------------------------------------------------------------ *
 * T12 配置类 vault 的 settings 面（extension_settings.stoc_native）
 * 任务 B：命名空间 stchat_* → stoc_*（含 stchat_native → stoc_native），
 * 并提供一次性幂等迁移 migrate_stchat_namespace（启动时由 vault 调用）。
 * ------------------------------------------------------------------ */

/// 读 `extension_settings.stoc_native.{key}`（None = 未配置；空串按未配置）
pub(crate) fn read_native_config(data_root: &std::path::Path, key: &str) -> Option<String> {
    let p = settings_path(data_root);
    let text = std::fs::read_to_string(p).ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    v.pointer(&format!("/extension_settings/stoc_native/{key}"))
        .and_then(|x| x.as_str())
        .map(String::from)
        .filter(|s| !s.is_empty())
}

/// 写 `extension_settings.stoc_native.{key}`（读-改-写全程持 NATIVE_CONFIG_LOCK ——
/// 两个写入者（store_sync / migrate_existing）并发时不得互相吞键；触发备份节流面由
/// settings/writequeue 面管理 —— 这里只同步配置类 vault 值）。
/// 返回是否成功就位（false = 目录/落盘失败 → 调用方走镜像兜底，明文不得静默丢）。
pub(crate) fn patch_native_config(data_root: &std::path::Path, key: &str, value: &str) -> bool {
    let p = settings_path(data_root);
    if let Some(parent) = p.parent() {
        if std::fs::create_dir_all(parent).is_err() {
            return false;
        }
    }
    let _guard = NATIVE_CONFIG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut v: Value = std::fs::read_to_string(&p)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_else(|| json!({}));
    let Some(obj) = v.as_object_mut() else {
        return false;
    };
    let ext = obj
        .entry("extension_settings")
        .or_insert_with(|| json!({}));
    let Some(ext) = ext.as_object_mut() else {
        return false;
    };
    let native = ext.entry("stoc_native").or_insert_with(|| json!({}));
    let Some(native) = native.as_object_mut() else {
        return false;
    };
    if native.get(key).and_then(|x| x.as_str()) == Some(value) {
        return true; // 同值免写（与前端写队列去重同语义）
    }
    native.insert(key.to_string(), json!(value));
    let pretty = to_pretty_4(&v);
    let tmp = p.with_extension("json.tmp");
    match std::fs::write(&tmp, pretty.as_bytes()).and_then(|_| std::fs::rename(&tmp, &p)) {
        Ok(()) => true,
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            log::error!(target: super::logx::FS, "native 配置写入失败（key={key}）：{e}");
            false
        }
    }
}

/// 一次性幂等迁移：`extension_settings` 下 `stchat_*` → `stoc_*`
///（含 stchat_native → stoc_native）。
/// 规则：旧键值搬到新键；新键已存在则不覆盖（保留新键值）并删除旧键；
/// 无旧键时零写入（幂等）。全程持 NATIVE_CONFIG_LOCK，4 空格 pretty + 原子写。
/// 启动时由 `vault::migrate_existing` 调用。
pub(crate) fn migrate_stchat_namespace(data_root: &std::path::Path) {
    let p = settings_path(data_root);
    let _guard = NATIVE_CONFIG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Ok(text) = std::fs::read_to_string(&p) else { return };
    let Ok(mut v) = serde_json::from_str::<Value>(&text) else { return };
    let Some(ext) = v
        .get_mut("extension_settings")
        .and_then(|e| e.as_object_mut())
    else {
        return;
    };
    let old_keys: Vec<String> = ext
        .keys()
        .filter(|k| k.starts_with("stchat_"))
        .cloned()
        .collect();
    if old_keys.is_empty() {
        return; // 幂等：已迁移过 / 本无旧命名空间 → 零写入
    }
    let mut moved = 0usize;
    let mut kept_new = 0usize;
    for old in old_keys {
        let Some(suffix) = old.strip_prefix("stchat_") else {
            continue;
        };
        let new_key = format!("stoc_{suffix}");
        let Some(value) = ext.remove(&old) else { continue };
        if ext.contains_key(&new_key) {
            // 已有新键 → 不覆盖（保留新键值），旧键删除
            kept_new += 1;
            log::warn!(target: super::logx::FS, "settings 命名空间迁移：新键 {new_key} 已存在 → 保留新键值并删除旧键 {old}");
        } else {
            ext.insert(new_key, value);
            moved += 1;
        }
    }
    let pretty = to_pretty_4(&v);
    let tmp = p.with_extension("json.tmp");
    if let Err(e) = std::fs::write(&tmp, pretty.as_bytes()).and_then(|_| std::fs::rename(&tmp, &p)) {
        let _ = std::fs::remove_file(&tmp);
        log::error!(target: super::logx::FS, "settings 命名空间迁移写回失败：{e}");
        return;
    }
    log::info!(target: super::logx::FS, "settings 命名空间迁移完成：stchat_* → stoc_*，搬移 {moved} 项、新键已存在保留 {kept_new} 项");
}

/* ------------------------------------------------------------------ *
 * 单测
 * ------------------------------------------------------------------ */

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::test_support::TestBackend;
    fn new_chats() -> std::sync::Arc<super::super::chats::ChatsService> {
        std::sync::Arc::new(super::super::chats::ChatsService::new())
    }

    fn new_maid() -> std::sync::Arc<super::super::extras::MaidTokens> {
        std::sync::Arc::new(super::super::extras::MaidTokens::new())
    }
    use std::sync::Arc;

    fn tmp_root(tag: &str) -> tempfile::TempDir {
        TestBackend::temp(&format!("stoc-settings-{tag}-"))
    }

    #[test]
    fn get_returns_raw_text_and_save_roundtrips() {
        let tmp = tmp_root("roundtrip");
        let root = tmp.path().to_path_buf();
        let st = BackendState {
            token: Arc::new("t".into()),
            data_root: root.clone(),
            max_body_bytes: 1024,
            settings_svc: Arc::new(SettingsService::new()),
            chats_svc: new_chats(),
            maid_tokens: new_maid(),
          vault: std::sync::Arc::new(super::super::vault::Vault::new_memory()),
        };
        let svc = SettingsService::new();

        // 初次 get：文件不存在 → 首启种子（默认落地 + 照常 200，ST getUserSettings 语义）
        let r = get(&st, None);
        assert_eq!(r.status().as_u16(), 200);
        assert!(settings_path(&root).exists());

        // save 全量对象
        let body = json!({ "username": "老板", "power_user": {"keep": true}, "extension_settings": {"regex": [1,2]} });
        let r = save(&st, &svc, &body, None);
        assert_eq!(r.status().as_u16(), 200);

        // get 返回原文字符串且解析后等值
        let r = get(&st, None);
        assert_eq!(r.status().as_u16(), 200);
        // Response body 读回：转成 axum body 字节
        let bytes = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(async { axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap() });
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        let raw = v["settings"].as_str().unwrap();
        let parsed: Value = serde_json::from_str(raw).unwrap();
        assert_eq!(parsed, body, "save→get 往返等值（未知键零丢失）");
        assert_eq!(parsed["power_user"]["keep"], json!(true));
    }

    #[test]
    fn save_rejects_wrapped_object() {
        let tmp = tmp_root("wrapped");
        let root = tmp.path().to_path_buf();
        let st = BackendState {
            token: Arc::new("t".into()),
            data_root: root.clone(),
            max_body_bytes: 1024,
            settings_svc: Arc::new(SettingsService::new()),
            chats_svc: new_chats(),
            maid_tokens: new_maid(),
          vault: std::sync::Arc::new(super::super::vault::Vault::new_memory()),
        };
        let svc = SettingsService::new();
        let body = json!({ "settings": "{\"a\":1}" });
        let r = save(&st, &svc, &body, None);
        assert_eq!(r.status().as_u16(), 400, "包装对象必须 400（三铁律之一）");
    }

    #[test]
    fn save_rejects_non_object_body() {
        // body 必须是 JSON 对象：数组 / 标量 → 400（api.rs 的 JSON 解析允许任意 Value）
        let tmp = tmp_root("nonobject");
        let root = tmp.path().to_path_buf();
        let st = BackendState {
            token: Arc::new("t".into()),
            data_root: root.clone(),
            max_body_bytes: 1024,
            settings_svc: Arc::new(SettingsService::new()),
            chats_svc: new_chats(),
            maid_tokens: new_maid(),
            vault: std::sync::Arc::new(super::super::vault::Vault::new_memory()),
        };
        let svc = SettingsService::new();
        for bad in [json!([1, 2, 3]), json!("string-body"), json!(42), json!(null)] {
            let r = save(&st, &svc, &bad, None);
            assert_eq!(r.status().as_u16(), 400, "非对象 body 必须 400：{bad}");
        }
        // 拒绝后不得落盘半写状态
        assert!(
            !settings_path(&root).exists() || {
                let raw = std::fs::read_to_string(settings_path(&root)).unwrap();
                serde_json::from_str::<Value>(&raw).map(|v| v.is_object()).unwrap_or(false)
            },
            "落盘内容必须是合法 JSON 对象"
        );
    }

    #[test]
    fn patch_native_config_concurrent_writers_keep_all_keys() {
        // 锁内读改写：并发 patch 不同 key 不得互相吞键（lost update 消除）
        let tmp = tmp_root("patchlock");
        let root = tmp.path().to_path_buf();
        let k1 = "libre_url";
        let k2 = "lingva_url";
        let (h1, h2) = {
            let r1 = root.clone();
            let r2 = root.clone();
            (
                std::thread::spawn(move || {
                    for i in 0..20 {
                        patch_native_config(&r1, k1, &format!("http://127.0.0.1:{i}"));
                    }
                }),
                std::thread::spawn(move || {
                    for i in 0..20 {
                        patch_native_config(&r2, k2, &format!("http://127.0.0.2:{i}"));
                    }
                }),
            )
        };
        h1.join().unwrap();
        h2.join().unwrap();
        assert!(
            read_native_config(&root, k1).is_some(),
            "并发写者 A 的键不得被吞"
        );
        assert!(
            read_native_config(&root, k2).is_some(),
            "并发写者 B 的键不得被吞"
        );
    }

    /// ⑤ 键迁移：extension_settings 下 stchat_speech / stchat_native → stoc_*
    ///（已有新键不覆盖；幂等：第二次零写入）
    #[test]
    fn stchat_namespace_migrates_to_stoc_once() {
        let tmp = tmp_root("ns-migrate");
        let root = tmp.path().to_path_buf();
        let p = settings_path(&root);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(
            &p,
            json!({
                "username": "船长",
                "extension_settings": {
                    "stchat_speech": "voice-A",
                    "stchat_native": { "libre_url": "http://127.0.0.1:7550" },
                    "stoc_speech": "keep-new",
                    "other_ext": { "keep": true }
                }
            })
            .to_string(),
        )
        .unwrap();

        migrate_stchat_namespace(&root);

        let raw = std::fs::read_to_string(&p).unwrap();
        let v: Value = serde_json::from_str(&raw).unwrap();
        let ext = v["extension_settings"].as_object().unwrap();
        assert_eq!(ext["stoc_speech"], "keep-new", "已有新键不被覆盖");
        assert_eq!(ext["stoc_native"]["libre_url"], "http://127.0.0.1:7550");
        assert!(!ext.contains_key("stchat_speech"), "旧键已删除：{raw}");
        assert!(!ext.contains_key("stchat_native"), "旧 native 命名空间已删除：{raw}");
        assert_eq!(v["username"], "船长", "无关键保留");
        assert_eq!(ext["other_ext"]["keep"], true);
        assert_eq!(
            read_native_config(&root, "libre_url").as_deref(),
            Some("http://127.0.0.1:7550"),
            "迁移后 native 配置走 stoc_native 可读"
        );
        // patch 返回值：成功就位 true
        assert!(patch_native_config(&root, "lingva_url", "https://lingva.example"));
        assert_eq!(
            read_native_config(&root, "lingva_url").as_deref(),
            Some("https://lingva.example")
        );

        // 幂等：第二次调用零写入（字节级不变）
        let after_first = std::fs::read_to_string(&p).unwrap();
        migrate_stchat_namespace(&root);
        assert_eq!(
            std::fs::read_to_string(&p).unwrap(),
            after_first,
            "幂等：已迁移后不再重写"
        );
    }

    #[test]
    fn autosave_throttles_and_dedupes() {
        let tmp = tmp_root("autosave");
        let root = tmp.path().to_path_buf();
        let st = BackendState {
            token: Arc::new("t".into()),
            data_root: root.clone(),
            max_body_bytes: 1024,
            settings_svc: Arc::new(SettingsService::new()),
            chats_svc: new_chats(),
            maid_tokens: new_maid(),
          vault: std::sync::Arc::new(super::super::vault::Vault::new_memory()),
        };
        let svc = SettingsService::new();

        let body = json!({ "a": 1 });
        save(&st, &svc, &body, None);
        // save() 内部已触发过一次 autosave（节流位已置）—— 清零后验证触发路径
        {
            let mut state = svc.autosave.lock().unwrap();
            state.last_fire = None;
        }
        assert!(trigger_autosave(&st, &svc), "首次 autosave 应执行");
        let dir = backups_dir(&root);
        let count = || std::fs::read_dir(&dir).unwrap().count();
        let after_first = count();
        assert!(after_first >= 1, "应产生至少 1 个备份");

        // 内容未变 → 去重，不新增
        assert!(
            !trigger_autosave(&st, &svc) || count() == after_first,
            "节流窗口内不重复触发"
        );
        // 手动清节流位模拟窗口过后再触发：内容相同 → 去重不新增
        {
            let mut state = svc.autosave.lock().unwrap();
            state.last_fire = None;
        }
        trigger_autosave(&st, &svc);
        assert_eq!(count(), after_first, "内容未变的备份应被去重");

        // 内容变了 → 新备份
        let body2 = json!({ "a": 2 });
        save(&st, &svc, &body2, None);
        {
            let mut state = svc.autosave.lock().unwrap();
            state.last_fire = None;
        }
        trigger_autosave(&st, &svc);
        assert_eq!(count(), after_first + 1, "内容变化应产生新备份");
    }

    #[test]
    fn backup_rotation_caps_at_50() {
        let tmp = tmp_root("rotation");
        let root = tmp.path().to_path_buf();
        let dir = backups_dir(&root);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(settings_path(&root), "{}").unwrap();
        for i in 0..55 {
            let p = dir.join(format!("{SETTINGS_BACKUP_PREFIX}2026010{i:02}-000000.json"));
            std::fs::write(&p, format!("{{\"i\":{i}}}")).unwrap();
            // mtime 递增由写序保证；手动拨动以稳定排序
            let t = UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000 + i as u64 * 60);
            let _ = filetime_set(&p, t);
        }
        remove_old_backups(&dir);
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), MAX_BACKUPS);
        // 最旧的 5 个被删（i=0..4），i=5 起保留
        assert!(!dir
            .join(format!("{SETTINGS_BACKUP_PREFIX}202601001-000000.json"))
            .exists());
        assert!(dir
            .join(format!("{SETTINGS_BACKUP_PREFIX}202601006-000000.json"))
            .exists());
    }

    #[test]
    fn get_seeds_default_when_missing() {
        // 干净数据根（settings.json 与目录都不存在）→ 首启种子而非 500
        let tmp = tmp_root("seed-default");
        let root = tmp.path().to_path_buf();
        let st = BackendState {
            token: Arc::new("t".into()),
            data_root: root.clone(),
            max_body_bytes: 1024,
            settings_svc: Arc::new(SettingsService::new()),
            chats_svc: new_chats(),
            maid_tokens: new_maid(),
            vault: std::sync::Arc::new(super::super::vault::Vault::new_memory()),
        };
        let r = get(&st, None);
        assert_eq!(r.status().as_u16(), 200, "缺失 settings.json 应落地默认模板");
        let p = settings_path(&root);
        assert!(p.exists(), "默认模板应已写入 default-user/settings.json");
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "{}");
        // 第二次读 = 正常读文件，仍是 200
        let r2 = get(&st, None);
        assert_eq!(r2.status().as_u16(), 200);
    }

    #[test]
    fn save_writes_four_space_indent_like_st() {
        // ST settings.js:209 JSON.stringify(settings, null, 4) —— 落盘缩进 4 空格
        let tmp = tmp_root("pretty4");
        let root = tmp.path().to_path_buf();
        let st = BackendState {
            token: Arc::new("t".into()),
            data_root: root.clone(),
            max_body_bytes: 1024,
            settings_svc: Arc::new(SettingsService::new()),
            chats_svc: new_chats(),
            maid_tokens: new_maid(),
            vault: std::sync::Arc::new(super::super::vault::Vault::new_memory()),
        };
        let svc = SettingsService::new();
        let r = save(&st, &svc, &json!({ "a": { "b": 1 } }), None);
        assert_eq!(r.status().as_u16(), 200);
        let raw = std::fs::read_to_string(settings_path(&root)).unwrap();
        let first_nested = raw.lines().nth(1).expect("落盘应为多行 pretty");
        assert_eq!(
            first_nested.len() - first_nested.trim_start().len(),
            4,
            "settings 落盘缩进应为 4 空格（ST settings.js:209）：{raw}"
        );
        for line in raw.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let indent = line.len() - line.trim_start().len();
            assert_eq!(indent % 4, 0, "缩进必须是 4 的倍数（ST 口径）：{line}");
        }
    }

    /// 把文件 mtime/ctime 拨到指定时刻（测试排序稳定性用）
    fn filetime_set(p: &std::path::Path, t: SystemTime) -> std::io::Result<()> {
        std::fs::File::options()
            .write(true)
            .open(p)?
            .set_modified(t)
    }
}
