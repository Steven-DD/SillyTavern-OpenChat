//! backend/extensions —— 扩展发现 + QR 套装文件（Phase 1 族九 · manifest #68/#67）
//!
//! ST 契约（extensions.js:629-662 discover + quick-replies.js）：
//! - GET /discover：{{system(内置 public/scripts/extensions/**，与仓库同源)，
//!   local（extensions/<user>/third-party/），global(公共 third-party)}} 三组；
//!   **无 manifest.json 的目录跳过**（幽灵扩展防线，extensions.js:497-506）；
//!   local/global 的 name 带 `third-party/` 前缀。
//! - POST /quick-replies/save {name, ...}：QuickReplies/<name>.json 整文件
//!   4 空格缩进原子写 → 200 空 body。前端 extensions.ts:95-121 消费（纯 void）。
//!
//! ⚠ Phase 5 范围的 git 安装面（/install/update/version/branches/switch/move）
//! 按 T14 规格未实现 → 501 NOT_MIGRATED（T14：git2、仅 https、禁 ext::、
//! 服务端校验、fail→清理；灰度允许，连续性清单 §1.4 登记在案）。

use super::{apply_cors_resp, BackendState};
use axum::body::Body;
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::{json, Value};

fn err_json(status: StatusCode, msg: &str) -> Response {
    (status, Json(json!({ "error": { "message": msg } }))).into_response()
}

fn ok_json(origin: Option<HeaderValue>, v: Value) -> Response {
    apply_cors_resp(Json(v).into_response(), origin)
}

/// GET /api/extensions/discover（manifest #68）
/// App 形态无内置 public 目录（安装包不含 ST 仓库）→ system 组恒空；
/// local/ 组扫 extensions/third-party/（ST 前缀约定即从讲座读取）。
pub fn discover(st: &BackendState, origin: Option<HeaderValue>) -> Response {
    let ext_dir = st.data_root.join("default-user").join("extensions");
    let _ = std::fs::create_dir_all(&ext_dir);
    let mut out: Vec<Value> = Vec::new();

    // local：extensions/<root>/<dir>（用户级目录挂 third-party 根）
    let user_dir = ext_dir;
    let user_name_base = "third-party/";
    if let Ok(rd) = std::fs::read_dir(&user_dir) {
        let mut names: Vec<String> = rd
            .flatten()
            .filter(|e| e.path().is_dir())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        names.sort();
        for d in names {
            if !user_dir.join(&d).join("manifest.json").is_file() {
                continue; // 幽灵扩展跳过（extensions.js:497-506）
            }
            out.push(json!({ "type": "local", "name": format!("{user_name_base}{d}") }));
        }
    }
    ok_json(origin, json!(out))
}

/// POST /api/quick-replies/save {name, ...}（manifest #67）
/// QR 集整文件落盘 QuickReplies/<name>.json；200 空 body（前端 stPostVoid）。
pub fn qr_save(st: &BackendState, body: &Value, origin: Option<HeaderValue>) -> Response {
    let Some(name) = body
        .get("name")
        .and_then(|v| v.as_str())
        .filter(|n| !n.trim().is_empty())
    else {
        return apply_cors_resp(StatusCode::BAD_REQUEST.into_response(), origin);
    };
    let Ok(safe) = crate::pathguard::component(&format!("{name}.json")) else {
        return err_json(StatusCode::FORBIDDEN, "非法文件名");
    };
    let dir = st.data_root.join("default-user").join("QuickReplies");
    if let Some(parent) = dir.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::create_dir_all(&dir);
    let pretty = serde_json::to_string_pretty(body).unwrap_or_default();
    let p = dir.join(&safe);
    let tmp = p.with_extension("json.tmp");
    match std::fs::write(&tmp, &pretty).and_then(|_| std::fs::rename(&tmp, &p)) {
        Ok(()) => apply_cors_resp(StatusCode::OK.into_response(), origin),
        Err(e) => err_json(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("QR 写入失败：{e}"),
        ),
    }
}

/// extensions git 安装族：Phase 5，S20 降级占位（T14 规格已入库）
pub fn git_ops_not_migrated(op: &str, origin: Option<HeaderValue>) -> Response {
    let resp = (
        StatusCode::NOT_IMPLEMENTED,
        Json(json!({
            "error": {
                "message": format!("扩展 git 操作 {op} 尚未迁移（Phase 5；T14 规格落地批次）"),
                "code": "NOT_MIGRATED",
            }
        })),
    );
    apply_cors_resp(resp.into_response(), origin)
}

#[allow(dead_code)]
fn _witness(_: Body) {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn st_with(tag: &str) -> BackendState {
        let root = std::env::temp_dir().join(format!("stchat-ext-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        BackendState {
            token: Arc::new("t".into()),
            data_root: root,
            max_body_bytes: 1024,
            settings_svc: Arc::new(super::super::settings::SettingsService::new()),
            chats_svc: Arc::new(super::super::chats::ChatsService::new()),
            maid_tokens: Arc::new(super::super::extras::MaidTokens::new()),
        }
    }

    #[test]
    fn discover_skips_manifestless() {
        let st = st_with("disc");
        let dir = st.data_root.join("default-user").join("extensions");
        std::fs::create_dir_all(dir.join("good-ext")).unwrap();
        std::fs::write(dir.join("good-ext").join("manifest.json"), "{}").unwrap();
        std::fs::create_dir_all(dir.join("ghost-ext")).unwrap(); // 无 manifest

        let r = discover(&st, None);
        let bytes = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(async { axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap() });
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        let arr = v.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["type"], "local");
        assert_eq!(arr[0]["name"], "third-party/good-ext");
    }

    #[test]
    fn qr_save_roundtrip() {
        let st = st_with("qr");
        let body = json!({ "name": "我的集", "qrList": [{ "label": "hi" }] });
        let r = qr_save(&st, &body, None);
        assert_eq!(r.status().as_u16(), 200);
        let pretty = std::fs::read_to_string(
            st.data_root
                .join("default-user")
                .join("QuickReplies")
                .join("我的集.json"),
        )
        .unwrap();
        assert!(pretty.contains("qrList"), "整文件写入");
        // 无 name 400
        let r = qr_save(&st, &json!({}), None);
        assert_eq!(r.status().as_u16(), 400);
    }

    #[test]
    fn git_ops_501() {
        let r = git_ops_not_migrated("install", None);
        assert_eq!(r.status().as_u16(), 501);
    }
}
