//! backend/api —— Phase 1 已实现端点 + 未实现兜底
//!
//! 已实现：
//! - `/csrf-token`（GET）：D1 方案 a —— 前端 client.ts:57 无条件获取；
//!   本服务无会话体系，返回与鉴权 token 相同的值（双提交形态保留，X-CSRF-Token
//!   收下不校验：本地单用户无 CSRF 面，X-Relay-Auth 即鉴权）。
//! - `/api/ping`（POST）：健康检查。
//!
//! 未实现：fallback 统一 501 + `{error:{message}}`（S20 降级契约：
//! 可判定的「未迁移」错误，禁止 200+空结果伪造成功）。

use super::{
    apply_cors_resp, forbidden, is_exempt_from_auth, origin_allowed, preflight, BackendState,
};
use axum::{
    body::Body,
    extract::{OriginalUri, State},
    http::{header, HeaderMap, HeaderValue, Method, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

/// 鉴权 + CORS 统一入口（S2-S5）。所有请求（含已实现端点与 501 兜底）经此。


pub async fn authorize(
    State(st): State<BackendState>,
    method: Method,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    body: Body,
) -> Response {
    // 非白名单 Origin 从 CORS 反射中剔除
    let origin = headers.get(header::ORIGIN).cloned().filter(|o| origin_allowed(o));

    if method == Method::OPTIONS {
        return preflight(origin);
    }

    let path_q = uri.path_and_query().map(|p| p.as_str()).unwrap_or("/");

    // S5：token 只走 X-Relay-Auth 头，query 里的 token 一律无效
    let auth_ok = is_exempt_from_auth(&method, path_q)
        || headers.get("x-relay-auth").and_then(|v| v.to_str().ok()) == Some(st.token.as_str());
    if !auth_ok {
        return forbidden();
    }

    // 读取请求体（写端点需要；上限 S15）
    let body_bytes = match axum::body::to_bytes(body, st.max_body_bytes).await {
        Ok(b) => b,
        Err(_) => {
            return apply_cors_resp(
                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": { "message": "请求体读取失败或超过上限" } })),
                )
                    .into_response(),
                origin,
            );
        }
    };

    let content_type = headers.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok());
    route(st, method, origin, path_q, &body_bytes, content_type).await
}

/// 路由分发：已实现族 → handler；否则 501 兜底
async fn route(
    st: BackendState,
    method: Method,
    origin: Option<HeaderValue>,
    path_q: &str,
    body_bytes: &[u8],
    content_type: Option<&str>,
) -> Response {
    let path = path_q.split('?').next().unwrap_or("/");
    match (method.as_str(), path) {
        ("GET", "/csrf-token") => csrf_token(st, origin),
        ("POST", "/api/ping") => ping(origin),
        // settings 族（Phase 1 · manifest #28/#29）
        ("POST", "/api/settings/get") => super::settings::get(&st, origin),
        ("POST", "/api/settings/save") => match serde_json::from_slice::<serde_json::Value>(body_bytes) {
            Ok(v) => super::settings::save(&st, &st.settings_svc, &v, origin),
            Err(_) => bad_json(origin),
        },
        // characters 族 JSON 面（Phase 1 · manifest #1/#2/#3/#6/#7）
        ("POST", "/api/characters/all") => super::characters::all(&st, origin),
        ("POST", "/api/characters/get") => match serde_json::from_slice::<serde_json::Value>(body_bytes) {
            Ok(v) => super::characters::get(&st, &v, origin),
            Err(_) => bad_json(origin),
        },
        ("POST", "/api/characters/chats") => match serde_json::from_slice::<serde_json::Value>(body_bytes) {
            Ok(v) => super::characters::chats(&st, &v, origin),
            Err(_) => bad_json(origin),
        },
        ("POST", "/api/characters/delete") => match serde_json::from_slice::<serde_json::Value>(body_bytes) {
            Ok(v) => super::characters::delete(&st, &v, origin),
            Err(_) => bad_json(origin),
        },
        ("POST", "/api/characters/rename") => match serde_json::from_slice::<serde_json::Value>(body_bytes) {
            Ok(v) => super::characters::rename(&st, &v, origin),
            Err(_) => bad_json(origin),
        },
        // characters 族三（multipart 面 + export · manifest #4/#5/#8）
        ("POST", "/api/characters/create") => {
            super::characters::create(&st, content_type, body_bytes.to_vec().into(), origin).await
        }
        ("POST", "/api/characters/edit") => {
            super::characters::edit(&st, content_type, body_bytes.to_vec().into(), origin).await
        }
        ("POST", "/api/characters/import") => {
            super::characters::import(&st, content_type, body_bytes.to_vec().into(), origin).await
        }
        ("POST", "/api/characters/export") => match serde_json::from_slice::<serde_json::Value>(body_bytes) {
            Ok(v) => super::characters::export(&st, &v, origin),
            Err(_) => bad_json(origin),
        },
        // chats 族（Phase 1 · manifest #9-#18）
        ("POST", "/api/chats/save") => match serde_json::from_slice::<serde_json::Value>(body_bytes) {
            Ok(v) => super::chats::save(&st, &st.chats_svc, &v, origin),
            Err(_) => bad_json(origin),
        },
        ("POST", "/api/chats/get") => match serde_json::from_slice::<serde_json::Value>(body_bytes) {
            Ok(v) => super::chats::get(&st, &v, origin),
            Err(_) => bad_json(origin),
        },
        ("POST", "/api/chats/rename") => match serde_json::from_slice::<serde_json::Value>(body_bytes) {
            Ok(v) => super::chats::rename(&st, &v, origin),
            Err(_) => bad_json(origin),
        },
        ("POST", "/api/chats/delete") => match serde_json::from_slice::<serde_json::Value>(body_bytes) {
            Ok(v) => super::chats::delete(&st, &v, origin),
            Err(_) => bad_json(origin),
        },
        ("POST", "/api/chats/search") => match serde_json::from_slice::<serde_json::Value>(body_bytes) {
            Ok(v) => super::chats::search(&st, &v, origin),
            Err(_) => bad_json(origin),
        },
        ("POST", "/api/chats/recent") => match serde_json::from_slice::<serde_json::Value>(body_bytes) {
            Ok(v) => super::chats::recent(&st, &st.chats_svc, &v, origin),
            Err(_) => bad_json(origin),
        },
        ("POST", "/api/chats/group/get") => match serde_json::from_slice::<serde_json::Value>(body_bytes) {
            Ok(v) => super::chats::group_get(&st, &v, origin),
            Err(_) => bad_json(origin),
        },
        ("POST", "/api/chats/group/info") => match serde_json::from_slice::<serde_json::Value>(body_bytes) {
            Ok(v) => super::chats::group_info(&st, &v, origin),
            Err(_) => bad_json(origin),
        },
        ("POST", "/api/chats/group/delete") => match serde_json::from_slice::<serde_json::Value>(body_bytes) {
            Ok(v) => super::chats::group_delete(&st, &v, origin),
            Err(_) => bad_json(origin),
        },
        ("POST", "/api/chats/group/save") => match serde_json::from_slice::<serde_json::Value>(body_bytes) {
            Ok(v) => super::chats::group_save(&st, &st.chats_svc, &v, origin),
            Err(_) => bad_json(origin),
        },
        // worldinfo 族（Phase 1 · manifest #19-#22）
        ("POST", "/api/worldinfo/list") => super::worldinfo_groups::wi_list(&st, origin),
        ("POST", "/api/worldinfo/get") => match serde_json::from_slice::<serde_json::Value>(body_bytes) {
            Ok(v) => super::worldinfo_groups::wi_get(&st, &v, origin),
            Err(_) => bad_json(origin),
        },
        ("POST", "/api/worldinfo/edit") => match serde_json::from_slice::<serde_json::Value>(body_bytes) {
            Ok(v) => super::worldinfo_groups::wi_edit(&st, &v, origin),
            Err(_) => bad_json(origin),
        },
        ("POST", "/api/worldinfo/delete") => match serde_json::from_slice::<serde_json::Value>(body_bytes) {
            Ok(v) => super::worldinfo_groups::wi_delete(&st, &v, origin),
            Err(_) => bad_json(origin),
        },
        ("POST", "/api/worldinfo/import") => {
            super::worldinfo_groups::wi_import(&st, content_type, body_bytes.to_vec().into(), origin).await
        }
        // groups 族（Phase 1 · groups.js）
        ("POST", "/api/groups/all") => super::worldinfo_groups::gr_all(&st, origin),
        ("POST", "/api/groups/create") => match serde_json::from_slice::<serde_json::Value>(body_bytes) {
            Ok(v) => super::worldinfo_groups::gr_create(&st, &v, origin),
            Err(_) => bad_json(origin),
        },
        ("POST", "/api/groups/edit") => match serde_json::from_slice::<serde_json::Value>(body_bytes) {
            Ok(v) => super::worldinfo_groups::gr_edit(&st, &v, origin),
            Err(_) => bad_json(origin),
        },
        ("POST", "/api/groups/delete") => match serde_json::from_slice::<serde_json::Value>(body_bytes) {
            Ok(v) => super::worldinfo_groups::gr_delete(&st, &v, origin),
            Err(_) => bad_json(origin),
        },
        _ => not_implemented(origin, path_q),
    }
}

/// JSON 解析失败（400）
fn bad_json(origin: Option<HeaderValue>) -> Response {
    apply_cors_resp(
        (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": { "message": "请求体不是合法 JSON" } })),
        )
            .into_response(),
        origin,
    )
}

/// GET /csrf-token —— 与 ST 响应形态对齐 `{token}`（client.ts:62 读 data.token）
fn csrf_token(st: BackendState, origin: Option<HeaderValue>) -> Response {
    let body = Json(json!({ "token": &*st.token }));
    apply_cors_resp(body.into_response(), origin)
}

/// POST /api/ping —— 健康检查（manifest #77）
fn ping(origin: Option<HeaderValue>) -> Response {
    let body = Json(json!({ "ok": true, "backend": "native" }));
    apply_cors_resp(body.into_response(), origin)
}

/// 兜底：未实现端点（S20）。
/// 501 + `{error:{message:"该端点尚未迁移（native 后端 Phase X 范围）"}}`。
/// 前端既有错误处理（error.message 语义）可直接显示，不会当成功。
fn not_implemented(origin: Option<HeaderValue>, path_q: &str) -> Response {
    let path = path_q.split('?').next().unwrap_or("/");
    let resp = (
        StatusCode::NOT_IMPLEMENTED,
        Json(json!({
            "error": {
                "message": format!("该端点尚未迁移到 native 后端（Phase 1 进行中）：{path}"),
                "code": "NOT_MIGRATED",
            }
        })),
    )
        .into_response();
    apply_cors_resp(resp, origin)
}
