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

use super::{apply_cors_resp, forbidden, is_exempt_from_auth, origin_allowed, preflight, BackendState};
use axum::{
    body::Body,
    extract::{OriginalUri, State},
    http::{header, HeaderMap, HeaderValue, Method, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

/// 鉴权 + CORS 统一入口（S2-S5）。所有请求（含已实现端点与 501 兜底）经此。
/// body 当前不消费（Phase 1 无 body 端点）；保留在签名中以匹配 axum extractor 顺序，
/// 后续 handler 直接在 route() 内取用。
pub async fn authorize(
    State(st): State<BackendState>,
    method: Method,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    _body: Body,
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

    route(st, method, origin, path_q).await
}

/// 路由分发：已实现族 → handler；否则 501 兜底
async fn route(
    st: BackendState,
    method: Method,
    origin: Option<HeaderValue>,
    path_q: &str,
) -> Response {
    let path = path_q.split('?').next().unwrap_or("/");
    match (method.as_str(), path) {
        ("GET", "/csrf-token") => csrf_token(st, origin),
        ("POST", "/api/ping") => ping(origin),
        _ => not_implemented(origin, path_q),
    }
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
