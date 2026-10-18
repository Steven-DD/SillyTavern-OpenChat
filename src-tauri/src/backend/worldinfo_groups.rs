//! backend/worldinfo_groups —— 世界书 + 群组数据服务（Phase 1 族五 · manifest #19-#22/#19g）
//!
//! ST 契约（worldinfo.js:39-157 + groups.js:113-234，已逐行核对）：
//! - worldinfo/list：worlds/*.json → [{file_id, name, extensions}]（localeCompare 排序；
//!   单文件解析失败跳过）。
//! - worldinfo/get {name}：不存在时 allowDummy=true → `{entries:{}}`（ST /get 用
//!   dummy —— 前端新建即 edit 空书）。
//! - worldinfo/edit {name, data}：data 必须**含 entries 键**（400 'Is not a valid
//!   world info file'）；4 空格缩进原子写；返回 {ok:true}。
//! - worldinfo/delete {name}：不存在 → **抛异常 → 500**（ST 直接 throw）。
//! - worldinfo/import（multipart）：文件名 sanitize 取 stem + '.json'；顶层必须
//!   含 entries；返回 {name}。
//! - groups/all：groups/*.json + chat_size/date_last_chat 统计（birthtime ISO）。
//! - groups/create：id=Date.now() 字符串；chat_id 默认 id；chats 默认 [id]；
//!   弃用键 chat_metadata/past_metadata 剥除（warnOnGroupMetadata）；返回完整元数据。
//! - groups/edit {id}：整文件覆盖（剥弃用键）；{ok:true}。
//! - groups/delete {id}：连带删 group chats/<chat>.jsonl 再删组文件；{ok:true}。

use super::{apply_cors_resp, BackendState};
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::{json, Value};
use std::time::UNIX_EPOCH;

fn worlds_dir(st: &BackendState) -> std::path::PathBuf {
    st.data_root.join("default-user").join("worlds")
}

fn groups_dir(st: &BackendState) -> std::path::PathBuf {
    st.data_root.join("default-user").join("groups")
}

fn group_chats_dir(st: &BackendState) -> std::path::PathBuf {
    st.data_root.join("default-user").join("group chats")
}

fn err_json(status: StatusCode, msg: &str) -> Response {
    log::warn!(target: super::logx::FS, "worldinfo/groups 错误 {status}: {}", super::logx::redact(msg));
    (status, Json(json!({ "error": { "message": msg } }))).into_response()
}

fn ok_json(origin: Option<HeaderValue>, v: Value) -> Response {
    apply_cors_resp(Json(v).into_response(), origin)
}

/// 弃用键剥除（groups.js:18-28 warnOnGroupMetadata）
fn strip_deprecated_keys(v: &mut Value) {
    if let Some(o) = v.as_object_mut() {
        o.remove("chat_metadata");
        o.remove("past_metadata");
    }
}

fn iso_from_ms(ms: u64) -> String {
    super::settings::civil_utc((ms / 1000) as i64);
    let (y, mth, d, h, mi, s) = super::settings::civil_utc((ms / 1000) as i64);
    format!("{y:04}-{mth:02}-{d:02}T{h:02}:{mi:02}:{s:02}.000Z")
}

/* ---------------- worldinfo（manifest #19-#22） ---------------- */

/// POST /api/worldinfo/list（manifest #19）
pub fn wi_list(st: &BackendState, origin: Option<HeaderValue>) -> Response {
    let dir = worlds_dir(st);
    let _ = std::fs::create_dir_all(&dir);
    let mut names: Vec<String> = match std::fs::read_dir(&dir) {
        Ok(rd) => rd
            .flatten()
            .filter(|e| {
                e.path().is_file()
                    && e.path()
                        .extension()
                        .and_then(|x| x.to_str())
                        .map(|x| x.to_lowercase())
                        == Some("json".into())
            })
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect(),
        Err(e) => {
            return err_json(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("worlds 目录读取失败：{e}"),
            )
        }
    };
    names.sort_by_key(|a| a.to_lowercase());
    let mut out = Vec::new();
    for f in names {
        let p = dir.join(&f);
        let Ok(text) = std::fs::read_to_string(&p) else {
            continue;
        };
        let Ok(parsed) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        let file_id = f.trim_end_matches(".json").to_string();
        let name = parsed
            .get("name")
            .and_then(|n| n.as_str())
            .unwrap_or(&file_id)
            .to_string();
        let ext = parsed
            .get("extensions")
            .cloned()
            .unwrap_or_else(|| json!({}));
        let ext = if ext.is_object() { ext } else { json!({}) };
        out.push(json!({ "file_id": file_id, "name": name, "extensions": ext }));
    }
    ok_json(origin, json!(out))
}

/// POST /api/worldinfo/get {name}（manifest #20）—— 不存在返回 {entries:{}}（dummy）
pub fn wi_get(st: &BackendState, body: &Value, origin: Option<HeaderValue>) -> Response {
    let Some(name) = body.get("name").and_then(|v| v.as_str()) else {
        return apply_cors_resp(StatusCode::BAD_REQUEST.into_response(), origin);
    };
    let Ok(safe) = crate::pathguard::component(&format!("{name}.json")) else {
        return apply_cors_resp(StatusCode::FORBIDDEN.into_response(), origin);
    };
    let p = worlds_dir(st).join(safe);
    if !p.is_file() {
        return ok_json(origin, json!({ "entries": {} }));
    }
    match std::fs::read_to_string(&p) {
        Ok(text) => match serde_json::from_str::<Value>(&text) {
            Ok(v) => ok_json(origin, v),
            Err(e) => err_json(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("world JSON 解析失败：{e}"),
            ),
        },
        Err(e) => err_json(StatusCode::INTERNAL_SERVER_ERROR, &format!("读取失败：{e}")),
    }
}

/// POST /api/worldinfo/edit {name, data}（manifest #21）—— data 必须含 entries
pub fn wi_edit(st: &BackendState, body: &Value, origin: Option<HeaderValue>) -> Response {
    let Some(name) = body.get("name").and_then(|v| v.as_str()) else {
        return apply_cors_resp(
            (StatusCode::BAD_REQUEST, "World file must have a name").into_response(),
            origin,
        );
    };
    let Some(data) = body.get("data") else {
        return apply_cors_resp(
            (StatusCode::BAD_REQUEST, "Is not a valid world info file").into_response(),
            origin,
        );
    };
    if data.get("entries").is_none() {
        return apply_cors_resp(
            (StatusCode::BAD_REQUEST, "Is not a valid world info file").into_response(),
            origin,
        );
    }
    let Ok(safe) = crate::pathguard::component(&format!("{name}.json")) else {
        return apply_cors_resp(StatusCode::FORBIDDEN.into_response(), origin);
    };
    let p = worlds_dir(st).join(safe);
    if let Some(parent) = p.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let pretty = serde_json::to_string_pretty(data).unwrap_or_else(|_| data.to_string());
    let tmp = p.with_extension("json.tmp");
    log::info!(target: super::logx::FS, "worldinfo/edit name={} bytes={}", super::logx::redact(name), pretty.len());
    match std::fs::write(&tmp, &pretty).and_then(|_| std::fs::rename(&tmp, &p)) {
        Ok(()) => ok_json(origin, json!({ "ok": true })),
        Err(e) => err_json(StatusCode::INTERNAL_SERVER_ERROR, &format!("写入失败：{e}")),
    }
}

/// POST /api/worldinfo/delete {name}（manifest #22）—— 不存在 500（ST throw）
pub fn wi_delete(st: &BackendState, body: &Value, origin: Option<HeaderValue>) -> Response {
    let Some(name) = body.get("name").and_then(|v| v.as_str()) else {
        return apply_cors_resp(StatusCode::BAD_REQUEST.into_response(), origin);
    };
    let Ok(safe) = crate::pathguard::component(&format!("{name}.json")) else {
        return apply_cors_resp(StatusCode::FORBIDDEN.into_response(), origin);
    };
    let p = worlds_dir(st).join(safe);
    if !p.is_file() {
        return apply_cors_resp(StatusCode::INTERNAL_SERVER_ERROR.into_response(), origin);
    }
    log::info!(target: super::logx::FS, "worldinfo/delete name={}", super::logx::redact(name));
    match std::fs::remove_file(&p) {
        Ok(()) => apply_cors_resp(StatusCode::OK.into_response(), origin),
        Err(e) => err_json(StatusCode::INTERNAL_SERVER_ERROR, &format!("删除失败：{e}")),
    }
}

/// POST /api/worldinfo/import（manifest #21 附属）—— multipart `avatar` 文件。
/// 返回 {name}。前端 data.ts:241-255 按 text() 解析 {name} 或纯文本错误。
pub async fn wi_import(
    st: &BackendState,
    content_type: Option<&str>,
    body: axum::body::Bytes,
    origin: Option<HeaderValue>,
) -> Response {
    if !content_type
        .map(|c| c.starts_with("multipart/form-data"))
        .unwrap_or(false)
    {
        return apply_cors_resp(StatusCode::BAD_REQUEST.into_response(), origin);
    }
    // 复用 characters 的解析器（同 multer single('avatar') 字段名）
    let m = match super::characters::parse_multipart(content_type.unwrap(), body).await {
        Ok(m) => m,
        Err(_) => return apply_cors_resp(StatusCode::BAD_REQUEST.into_response(), origin),
    };
    let Some(bytes) = m.avatar else {
        return apply_cors_resp(StatusCode::BAD_REQUEST.into_response(), origin);
    };
    let raw_name = m.avatar_filename.unwrap_or_else(|| "imported.json".into());
    let stem = raw_name.trim_end_matches(".json").to_string();
    let Ok(safe) = crate::pathguard::component(&format!("{stem}.json")) else {
        return apply_cors_resp(StatusCode::FORBIDDEN.into_response(), origin);
    };
    let text = String::from_utf8_lossy(&bytes).to_string();
    let Ok(world) = serde_json::from_str::<Value>(&text) else {
        return apply_cors_resp(
            (StatusCode::BAD_REQUEST, "Is not a valid world info file").into_response(),
            origin,
        );
    };
    if world.get("entries").is_none() {
        return apply_cors_resp(
            (StatusCode::BAD_REQUEST, "Is not a valid world info file").into_response(),
            origin,
        );
    }
    let file_id = safe.trim_end_matches(".json").to_string();
    if file_id.is_empty() {
        return apply_cors_resp(
            (StatusCode::BAD_REQUEST, "World file must have a name").into_response(),
            origin,
        );
    }
    let p = worlds_dir(st).join(safe);
    if let Some(parent) = p.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    // 原子写：tmp + rename（对齐同模块 wi_edit / settings 的原子写纪律，规避半写损坏）
    let tmp = p.with_extension("json.tmp");
    match std::fs::write(&tmp, text.as_bytes()).and_then(|_| std::fs::rename(&tmp, &p)) {
        Ok(()) => ok_json(origin, json!({ "name": file_id })),
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            err_json(StatusCode::INTERNAL_SERVER_ERROR, &format!("写入失败：{e}"))
        }
    }
}

/* ---------------- groups（manifest #19g：groups/all/create/edit/delete） ---------------- */

/// POST /api/groups/all（manifest #19g-1）
pub fn gr_all(st: &BackendState, origin: Option<HeaderValue>) -> Response {
    let gdir = groups_dir(st);
    let _ = std::fs::create_dir_all(&gdir);
    let _ = std::fs::create_dir_all(group_chats_dir(st));
    let mut out = Vec::new();
    let mut names: Vec<String> = match std::fs::read_dir(&gdir) {
        Ok(rd) => rd
            .flatten()
            .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("json"))
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect(),
        Err(e) => {
            return err_json(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("groups 目录读取失败：{e}"),
            )
        }
    };
    names.sort();
    for f in names {
        let p = gdir.join(&f);
        let Ok(text) = std::fs::read_to_string(&p) else {
            continue;
        };
        let Ok(mut group) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        let stat = match std::fs::metadata(&p) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let birth_ms = stat
            .created()
            .or_else(|_| stat.modified())
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        // chat_size / date_last_chat（groups.js:132-143）
        let (mut chat_size, mut last) = (0u64, 0u64);
        if let Some(chats) = group.get("chats").and_then(|c| c.as_array()) {
            if let Ok(rd) = std::fs::read_dir(group_chats_dir(st)) {
                for e in rd.flatten() {
                    let stem = e
                        .path()
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("")
                        .to_string();
                    if chats.iter().any(|c| c.as_str() == Some(stem.as_str())) {
                        if let Ok(m) = e.metadata() {
                            chat_size += m.len();
                            last = last.max(
                                m.modified()
                                    .ok()
                                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                                    .map(|d| d.as_millis() as u64)
                                    .unwrap_or(0),
                            );
                        }
                    }
                }
            }
        }
        if let Some(o) = group.as_object_mut() {
            o.insert("date_added".into(), json!(birth_ms));
            o.insert("create_date".into(), json!(iso_from_ms(birth_ms)));
            o.insert("date_last_chat".into(), json!(last));
            o.insert("chat_size".into(), json!(chat_size));
        }
        out.push(group);
    }
    ok_json(origin, json!(out))
}

/// POST /api/groups/create（manifest #19g-2）—— id=Date.now()，返回完整元数据
pub fn gr_create(st: &BackendState, body: &Value, origin: Option<HeaderValue>) -> Response {
    if !body.is_object() {
        return apply_cors_resp(StatusCode::BAD_REQUEST.into_response(), origin);
    }
    let mut b = body.clone();
    strip_deprecated_keys(&mut b);
    let id = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis().to_string())
        .unwrap_or_default();
    let gdir = groups_dir(st);
    let _ = std::fs::create_dir_all(&gdir);
    let _ = std::fs::create_dir_all(group_chats_dir(st));
    let metadata = json!({
        "id": id,
        "name": b.get("name").and_then(|n| n.as_str()).unwrap_or("New Group"),
        "members": b.get("members").cloned().unwrap_or_else(|| json!([])),
        "avatar_url": b.get("avatar_url").cloned(),
        "allow_self_responses": b.get("allow_self_responses").and_then(|v| v.as_bool()).unwrap_or(false),
        "activation_strategy": b.get("activation_strategy").cloned().unwrap_or_else(|| json!(0)),
        "generation_mode": b.get("generation_mode").cloned().unwrap_or_else(|| json!(0)),
        "disabled_members": b.get("disabled_members").cloned().unwrap_or_else(|| json!([])),
        "fav": b.get("fav").cloned(),
        "chat_id": b.get("chat_id").cloned().unwrap_or_else(|| json!(id.clone())),
        "chats": b.get("chats").cloned().unwrap_or_else(|| json!([id.clone()])),
        "auto_mode_delay": b.get("auto_mode_delay").cloned().unwrap_or_else(|| json!(5)),
        "generation_mode_join_prefix": b.get("generation_mode_join_prefix").cloned().unwrap_or_else(|| json!("")),
        "generation_mode_join_suffix": b.get("generation_mode_join_suffix").cloned().unwrap_or_else(|| json!("")),
    });
    let Ok(safe) = crate::pathguard::component(&format!("{id}.json")) else {
        return apply_cors_resp(StatusCode::FORBIDDEN.into_response(), origin);
    };
    let p = gdir.join(safe);
    log::info!(target: super::logx::FS, "groups/create id={id}");
    let pretty = serde_json::to_string_pretty(&metadata).unwrap_or_default();
    match std::fs::write(&p, &pretty) {
        Ok(()) => ok_json(origin, metadata),
        Err(e) => err_json(StatusCode::INTERNAL_SERVER_ERROR, &format!("写入失败：{e}")),
    }
}

/// POST /api/groups/edit {id, ...}（manifest #19g-3）—— 整文件覆盖（剥弃用键）
pub fn gr_edit(st: &BackendState, body: &Value, origin: Option<HeaderValue>) -> Response {
    let Some(id) = body.get("id").and_then(|v| v.as_str()) else {
        return apply_cors_resp(StatusCode::BAD_REQUEST.into_response(), origin);
    };
    let mut b = body.clone();
    strip_deprecated_keys(&mut b);
    let Ok(safe) = crate::pathguard::component(&format!("{id}.json")) else {
        return apply_cors_resp(StatusCode::FORBIDDEN.into_response(), origin);
    };
    let p = groups_dir(st).join(safe);
    if let Some(parent) = p.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let pretty = serde_json::to_string_pretty(&b).unwrap_or_default();
    log::info!(target: super::logx::FS, "groups/edit id={}", super::logx::redact(id));
    let tmp = p.with_extension("json.tmp");
    match std::fs::write(&tmp, &pretty).and_then(|_| std::fs::rename(&tmp, &p)) {
        Ok(()) => ok_json(origin, json!({ "ok": true })),
        Err(e) => err_json(StatusCode::INTERNAL_SERVER_ERROR, &format!("写入失败：{e}")),
    }
}

/// POST /api/groups/delete {id}（manifest #19g-4）—— 连带删群聊文件
pub fn gr_delete(st: &BackendState, body: &Value, origin: Option<HeaderValue>) -> Response {
    let Some(id) = body.get("id").and_then(|v| v.as_str()) else {
        return apply_cors_resp(StatusCode::BAD_REQUEST.into_response(), origin);
    };
    let Ok(safe) = crate::pathguard::component(&format!("{id}.json")) else {
        return apply_cors_resp(StatusCode::FORBIDDEN.into_response(), origin);
    };
    let p = groups_dir(st).join(safe);
    log::info!(target: super::logx::FS, "groups/delete id={}", super::logx::redact(id));
    // 先读 chats[] 删群聊文件（groups.js:211-227：读失败仅告警继续）
    if let Ok(text) = std::fs::read_to_string(&p) {
        if let Ok(group) = serde_json::from_str::<Value>(&text) {
            if let Some(chats) = group.get("chats").and_then(|c| c.as_array()) {
                for c in chats {
                    if let Some(chat) = c.as_str() {
                        if let Ok(safe_chat) = crate::pathguard::component(&format!("{chat}.jsonl"))
                        {
                            let _ = std::fs::remove_file(group_chats_dir(st).join(safe_chat));
                        }
                    }
                }
            }
        }
    }
    if p.is_file() {
        let _ = std::fs::remove_file(&p);
    }
    ok_json(origin, json!({ "ok": true }))
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
        let root = std::env::temp_dir().join(format!("stoc-wig-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        BackendState {
            token: Arc::new("t".into()),
            data_root: root,
            max_body_bytes: 1024,
            settings_svc: Arc::new(super::super::settings::SettingsService::new()),
            chats_svc: new_chats(),
            maid_tokens: new_maid(),
          vault: std::sync::Arc::new(super::super::vault::Vault::new_memory()),
        }
    }

    #[test]
    fn worldinfo_roundtrip_and_dummy() {
        let st = st_with("wi");
        // get 不存在 → dummy
        let r = wi_get(&st, &json!({ "name": "nope" }), None);
        let bytes = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(async { axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap() });
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["entries"], json!({}));

        // edit → get
        let r = wi_edit(
            &st,
            &json!({ "name": "书", "data": { "entries": { "0": { "uid": 0 } }, "x": 1 } }),
            None,
        );
        let bytes = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(async { axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap() });
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["ok"], true);

        let r = wi_get(&st, &json!({ "name": "书" }), None);
        let bytes = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(async { axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap() });
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert!(v["entries"].is_object());

        // list 带元数据
        let r = wi_list(&st, None);
        let bytes = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(async { axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap() });
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        let arr = v.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["file_id"], "书");

        // edit 无 entries → 400
        let r = wi_edit(&st, &json!({ "name": "bad", "data": { "nope": 1 } }), None);
        assert_eq!(r.status().as_u16(), 400);

        // delete → 再 get 回 dummy
        let r = wi_delete(&st, &json!({ "name": "书" }), None);
        assert_eq!(r.status().as_u16(), 200);
        // 删除不存在 → 500（ST throw 语义）
        let r = wi_delete(&st, &json!({ "name": "书" }), None);
        assert_eq!(r.status().as_u16(), 500);
    }

    #[tokio::test]
    async fn worldinfo_import() {
        let st = st_with("wiimp");
        let content = json!({ "entries": { "0": { "uid": 0 } } }).to_string();
        let mp = format!(
            "--B\r\nContent-Disposition: form-data; name=\"avatar\"; filename=\"我的书.json\"\r\n\r\n{content}\r\n--B--\r\n"
        );
        let r = wi_import(
            &st,
            Some("multipart/form-data; boundary=B"),
            axum::body::Bytes::from(mp),
            None,
        )
        .await;
        assert_eq!(r.status().as_u16(), 200);
        let bytes = axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap();
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["name"], "我的书");
        // 非 world 文件 → 400
        let mp2 = "--B\r\nContent-Disposition: form-data; name=\"avatar\"; filename=\"x.json\"\r\n\r\n{\"nope\":1}\r\n--B--\r\n";
        let r = wi_import(
            &st,
            Some("multipart/form-data; boundary=B"),
            axum::body::Bytes::from(mp2),
            None,
        )
        .await;
        assert_eq!(r.status().as_u16(), 400);
    }

    #[test]
    fn worldinfo_traversal_names_rejected_and_leave_no_files() {
        // S7：worldinfo name 穿越 → 403，且数据目录外无新文件
        //（确定性目标路径断言 —— 目录计数快照对并行测试不抗扰）
        let st = st_with("witrav");
        let outside_target = st.data_root.parent().unwrap().join("evil.json");
        let _ = std::fs::remove_file(&outside_target);
        for name in ["../../evil", "a/b", "..\\evil", "../x/../y"] {
            let r = wi_get(&st, &json!({ "name": name }), None);
            assert_eq!(r.status().as_u16(), 403, "wi_get 穿越名 {name:?} 必须 403");
            let r = wi_edit(&st, &json!({ "name": name, "data": { "entries": {} } }), None);
            assert_eq!(r.status().as_u16(), 403, "wi_edit 穿越名 {name:?} 必须 403");
            let r = wi_delete(&st, &json!({ "name": name }), None);
            assert_eq!(r.status().as_u16(), 403, "wi_delete 穿越名 {name:?} 必须 403");
        }
        assert!(!outside_target.exists(), "数据目录外无新文件");
        // worlds 目录本身也不应被创建出越界文件
        let worlds = worlds_dir(&st);
        if worlds.is_dir() {
            assert!(
                std::fs::read_dir(&worlds).unwrap().count() == 0,
                "worlds 目录内也不得有越界落盘"
            );
        }
    }

    #[test]
    fn groups_lifecycle() {
        let st = st_with("gr");
        // create
        let r = gr_create(
            &st,
            &json!({ "name": "测试群", "members": ["a.png"] }),
            None,
        );
        let bytes = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(async { axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap() });
        let created: Value = serde_json::from_slice(&bytes).unwrap();
        let id = created["id"].as_str().unwrap().to_string();
        assert_eq!(created["chats"], json!([id]), "chats 默认 [id]");
        assert!(groups_dir(&st).join(format!("{id}.json")).is_file());

        // all 带统计
        let r = gr_all(&st, None);
        let bytes = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(async { axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap() });
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        let arr = v.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert!(arr[0]["create_date"].as_str().unwrap().ends_with('Z'));
        assert_eq!(arr[0]["chat_size"], 0);

        // edit
        let mut edited = created.clone();
        edited["name"] = json!("改名群");
        let r = gr_edit(&st, &edited, None);
        let bytes = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(async { axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap() });
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["ok"], true);

        // edit 剥弃用键
        edited["chat_metadata"] = json!({ "should": "vanish" });
        let _ = gr_edit(&st, &edited, None);
        let raw = std::fs::read_to_string(groups_dir(&st).join(format!("{id}.json"))).unwrap();
        assert!(!raw.contains("chat_metadata"), "弃用键被剥除");

        // delete 连带群聊
        std::fs::write(group_chats_dir(&st).join(format!("{id}.jsonl")), "{}\n").unwrap();
        let r = gr_delete(&st, &json!({ "id": id }), None);
        let bytes = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(async { axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap() });
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["ok"], true);
        assert!(!groups_dir(&st).join(format!("{id}.json")).exists());
        assert!(
            !group_chats_dir(&st).join(format!("{id}.jsonl")).exists(),
            "群聊文件连带删除"
        );
    }
}
