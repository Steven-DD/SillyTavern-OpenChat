//! backend/presets —— 预设管理数据服务（TextGen / OpenAI / Instruct / Context）
//!
//! 与 SillyTavern 契约对齐：
//! - presets 物理存放于 default-user/ 下的四个标准目录：
//!   1. `presets/` —— Text Completion / 本地采样预设
//!   2. `OpenAI Settings/` —— OpenAI / Claude / Chat Completion 采样预设
//!   3. `instruct/` —— 指令格式预设
//!   4. `context/` —— 上下文故事模板预设
//! - 端点：
//!   - `/api/presets/list` (POST)：{ "type": "text"|"openai"|"instruct"|"context" } → { "presets": [...] }
//!   - `/api/presets/get` (POST)：{ "name": "...", "type": "..." } → JSON 预设内容
//!   - `/api/presets/save` (POST)：{ "name": "...", "type": "...", "preset": { ... } } → { "ok": true }
//!   - `/api/presets/delete` (POST)：{ "name": "...", "type": "..." } → { "ok": true }

use super::{apply_cors_resp, logx, BackendState};
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;

fn resolve_preset_dir(st: &BackendState, type_str: &str) -> PathBuf {
    let sub = match type_str.to_lowercase().as_str() {
        "openai" | "chat" => "OpenAI Settings",
        "instruct" => "instruct",
        "context" => "context",
        _ => "presets",
    };
    st.data_root.join("default-user").join(sub)
}

fn err_json(status: StatusCode, msg: &str) -> Response {
    log::warn!(target: logx::FS, "presets 错误 {status}: {}", logx::redact(msg));
    (status, Json(json!({ "error": { "message": msg } }))).into_response()
}

fn ok_json(origin: Option<HeaderValue>, v: Value) -> Response {
    apply_cors_resp(Json(v).into_response(), origin)
}

/// POST /api/presets/list
pub fn list(st: &BackendState, body: &Value, origin: Option<HeaderValue>) -> Response {
    let type_str = body
        .get("type")
        .and_then(|v| v.as_str())
        .or_else(|| {
            body.get("api").and_then(|v| v.as_str()).map(|api| {
                if api.contains("openai") || api.contains("claude") {
                    "openai"
                } else {
                    "text"
                }
            })
        })
        .unwrap_or("text");

    let dir = resolve_preset_dir(st, type_str);
    let _ = fs::create_dir_all(&dir);

    let mut names = Vec::new();
    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_file() && p.extension().map(|e| e == "json").unwrap_or(false) {
                if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
                    names.push(stem.to_string());
                }
            }
        }
    }
    names.sort();
    ok_json(origin, json!({ "presets": names, "ok": true }))
}

/// POST /api/presets/get
pub fn get(st: &BackendState, body: &Value, origin: Option<HeaderValue>) -> Response {
    let name = match body.get("name").and_then(|v| v.as_str()) {
        Some(n) if !n.trim().is_empty() => n.trim(),
        _ => return err_json(StatusCode::BAD_REQUEST, "缺少预设名称参数 name"),
    };

    let type_str = body
        .get("type")
        .and_then(|v| v.as_str())
        .or_else(|| {
            body.get("api").and_then(|v| v.as_str()).map(|api| {
                if api.contains("openai") || api.contains("claude") {
                    "openai"
                } else {
                    "text"
                }
            })
        })
        .unwrap_or("text");

    if name != crate::pathguard::sanitize(name) {
        return err_json(StatusCode::BAD_REQUEST, "预设名称含非法字符");
    }
    let Ok(clean_name) = crate::pathguard::component(name) else {
        return err_json(StatusCode::BAD_REQUEST, "预设名称非法");
    };

    let dir = resolve_preset_dir(st, type_str);
    let filename = format!("{clean_name}.json");
    let file_path = match crate::pathguard::join_under(&dir, &[filename.as_str()]) {
        Ok(p) => p,
        Err(_) => return err_json(StatusCode::FORBIDDEN, "路径越界"),
    };

    if !file_path.is_file() {
        return err_json(StatusCode::NOT_FOUND, "预设文件不存在");
    }

    match fs::read_to_string(&file_path) {
        Ok(content) => match serde_json::from_str::<Value>(&content) {
            Ok(v) => ok_json(origin, v),
            Err(e) => err_json(StatusCode::INTERNAL_SERVER_ERROR, &format!("预设文件解析失败：{e}")),
        },
        Err(e) => err_json(StatusCode::INTERNAL_SERVER_ERROR, &format!("读取预设文件失败：{e}")),
    }
}

/// POST /api/presets/save
pub fn save(st: &BackendState, body: &Value, origin: Option<HeaderValue>) -> Response {
    let name = match body.get("name").and_then(|v| v.as_str()) {
        Some(n) if !n.trim().is_empty() => n.trim(),
        _ => return err_json(StatusCode::BAD_REQUEST, "缺少预设名称参数 name"),
    };

    let preset_data = match body.get("preset").or_else(|| body.get("data")) {
        Some(p) if p.is_object() => p,
        _ => return err_json(StatusCode::BAD_REQUEST, "缺少有效的预设内容 preset"),
    };

    let type_str = body
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("text");

    if name != crate::pathguard::sanitize(name) {
        return err_json(StatusCode::BAD_REQUEST, "预设名称含非法字符");
    }
    let Ok(clean_name) = crate::pathguard::component(name) else {
        return err_json(StatusCode::BAD_REQUEST, "预设名称非法");
    };

    let dir = resolve_preset_dir(st, type_str);
    if let Err(e) = fs::create_dir_all(&dir) {
        return err_json(StatusCode::INTERNAL_SERVER_ERROR, &format!("创建预设目录失败：{e}"));
    }

    let filename = format!("{clean_name}.json");
    let file_path = match crate::pathguard::join_under(&dir, &[filename.as_str()]) {
        Ok(p) => p,
        Err(_) => return err_json(StatusCode::FORBIDDEN, "路径越界"),
    };

    let tmp_path = file_path.with_extension("json.tmp");
    let content = match serde_json::to_string_pretty(preset_data) {
        Ok(s) => s,
        Err(e) => return err_json(StatusCode::BAD_REQUEST, &format!("JSON 序列化失败：{e}")),
    };

    if let Err(e) = fs::write(&tmp_path, content.as_bytes()) {
        let _ = fs::remove_file(&tmp_path);
        return err_json(StatusCode::INTERNAL_SERVER_ERROR, &format!("写入临时文件失败：{e}"));
    }

    if let Err(e) = fs::rename(&tmp_path, &file_path) {
        let _ = fs::remove_file(&tmp_path);
        return err_json(StatusCode::INTERNAL_SERVER_ERROR, &format!("重命名落地失败：{e}"));
    }

    ok_json(origin, json!({ "ok": true, "name": clean_name }))
}

/// POST /api/presets/delete
pub fn delete(st: &BackendState, body: &Value, origin: Option<HeaderValue>) -> Response {
    let name = match body.get("name").and_then(|v| v.as_str()) {
        Some(n) if !n.trim().is_empty() => n.trim(),
        _ => return err_json(StatusCode::BAD_REQUEST, "缺少预设名称参数 name"),
    };

    let type_str = body
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("text");

    if name != crate::pathguard::sanitize(name) {
        return err_json(StatusCode::BAD_REQUEST, "预设名称含非法字符");
    }
    let Ok(clean_name) = crate::pathguard::component(name) else {
        return err_json(StatusCode::BAD_REQUEST, "预设名称非法");
    };

    let dir = resolve_preset_dir(st, type_str);
    let filename = format!("{clean_name}.json");
    let file_path = match crate::pathguard::join_under(&dir, &[filename.as_str()]) {
        Ok(p) => p,
        Err(_) => return err_json(StatusCode::FORBIDDEN, "路径越界"),
    };

    if !file_path.exists() {
        return err_json(StatusCode::NOT_FOUND, "要删除的预设文件不存在");
    }

    if let Err(e) = fs::remove_file(&file_path) {
        return err_json(StatusCode::INTERNAL_SERVER_ERROR, &format!("删除预设失败：{e}"));
    }

    ok_json(origin, json!({ "ok": true, "name": clean_name }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::test_support::TestBackend;
    use std::sync::Arc;

    fn test_state(tag: &str) -> TestBackend {
        let tmp = TestBackend::temp(&format!("stoc-presets-{tag}-"));
        let root = tmp.path().to_path_buf();
        crate::st_data::ensure_user_skeleton(&root).unwrap();
        TestBackend::new(
            tmp,
            BackendState {
                token: Arc::new("t".into()),
                data_root: root,
                max_body_bytes: 1024 * 1024,
                settings_svc: Arc::new(crate::backend::settings::SettingsService::new()),
                chats_svc: Arc::new(crate::backend::chats::ChatsService::new()),
                maid_tokens: Arc::new(crate::backend::extras::MaidTokens::new()),
                vault: Arc::new(crate::backend::vault::Vault::new_memory()),
            },
        )
    }

    #[test]
    fn presets_crud_roundtrip() {
        let st = test_state("roundtrip");

        // 1. 保存预设
        let save_req = json!({
            "name": "My Test Preset",
            "type": "text",
            "preset": {
                "temperature": 0.85,
                "top_p": 0.95
            }
        });
        let res = save(&st, &save_req, None);
        assert_eq!(res.status(), StatusCode::OK);

        // 2. 列表
        let list_req = json!({ "type": "text" });
        let res = list(&st, &list_req, None);
        assert_eq!(res.status(), StatusCode::OK);

        // 3. 读取
        let get_req = json!({ "name": "My Test Preset", "type": "text" });
        let res = get(&st, &get_req, None);
        assert_eq!(res.status(), StatusCode::OK);

        // 4. 删除
        let del_req = json!({ "name": "My Test Preset", "type": "text" });
        let res = delete(&st, &del_req, None);
        assert_eq!(res.status(), StatusCode::OK);

        // 再次读取 404
        let res2 = get(&st, &get_req, None);
        assert_eq!(res2.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn path_traversal_is_blocked() {
        let st = test_state("traversal");

        let req = json!({
            "name": "../evil",
            "type": "text",
            "preset": { "a": 1 }
        });
        let res = save(&st, &req, None);
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }
}
