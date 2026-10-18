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
    apply_cors_resp, forbidden, is_exempt_from_auth, logx, origin_allowed, preflight, BackendState,
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
///
/// 日志：一次请求一条 `← 状态码 耗时 方法 路径 origin=… bytes=…`（logx::ReqLog），
/// 鉴权拒绝与超限另记 warn —— 生产「全部请求 403 / 大卡上传失败」可据此直接定位。
pub async fn authorize(
    State(st): State<BackendState>,
    method: Method,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    body: Body,
) -> Response {
    // 非白名单 Origin 从 CORS 反射中剔除
    let origin = headers
        .get(header::ORIGIN)
        .cloned()
        .filter(origin_allowed);

    let path_q = uri.path_and_query().map(|p| p.as_str()).unwrap_or("/");

    if method == Method::OPTIONS {
        log::debug!(target: logx::API, "OPTIONS 预检 {}", logx::redact(path_q));
        return preflight(origin);
    }

    let origin_str = origin.as_ref().and_then(|o| o.to_str().ok());
    let ua = headers.get(header::USER_AGENT).and_then(|v| v.to_str().ok());
    let req = logx::ReqLog::new(method.as_str(), path_q, origin_str);

    // S5：token 只走 X-Relay-Auth 头，query 里的 token 一律无效
    let auth_ok = is_exempt_from_auth(&method, path_q)
        || headers.get("x-relay-auth").and_then(|v| v.to_str().ok()) == Some(st.token.as_str());
    if !auth_ok {
        logx::auth_rejected(method.as_str(), path_q, origin_str, ua);
        let resp = forbidden();
        req.finish(resp.status().as_u16(), "auth=rejected");
        return resp;
    }

    // 读取请求体（写端点需要；上限 S15）
    let body_bytes = match axum::body::to_bytes(body, st.max_body_bytes).await {
        Ok(b) => b,
        Err(e) => {
            log::warn!(
                target: logx::API,
                "请求体读取失败或超过上限（{}B）：{e} {}",
                st.max_body_bytes,
                logx::redact(path_q)
            );
            let resp = apply_cors_resp(
                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": { "message": "请求体读取失败或超过上限" } })),
                )
                    .into_response(),
                origin,
            );
            req.finish(resp.status().as_u16(), "body=too_large_or_broken");
            return resp;
        }
    };

    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok());
    let n = body_bytes.len();
    let resp = route(st, method, origin, path_q, &body_bytes, content_type).await;
    req.finish(resp.status().as_u16(), &format!("bytes={n}"));
    resp
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
        ("POST", "/api/settings/save") => {
            match serde_json::from_slice::<serde_json::Value>(body_bytes) {
                Ok(v) => super::settings::save(&st, &st.settings_svc, &v, origin),
                Err(_) => bad_json(origin),
            }
        }
        // characters 族 JSON 面（Phase 1 · manifest #1/#2/#3/#6/#7）
        ("POST", "/api/characters/all") => super::characters::all(&st, origin),
        ("POST", "/api/characters/get") => {
            match serde_json::from_slice::<serde_json::Value>(body_bytes) {
                Ok(v) => super::characters::get(&st, &v, origin),
                Err(_) => bad_json(origin),
            }
        }
        ("POST", "/api/characters/chats") => {
            match serde_json::from_slice::<serde_json::Value>(body_bytes) {
                Ok(v) => super::characters::chats(&st, &v, origin),
                Err(_) => bad_json(origin),
            }
        }
        ("POST", "/api/characters/delete") => {
            match serde_json::from_slice::<serde_json::Value>(body_bytes) {
                Ok(v) => super::characters::delete(&st, &v, origin),
                Err(_) => bad_json(origin),
            }
        }
        ("POST", "/api/characters/rename") => {
            match serde_json::from_slice::<serde_json::Value>(body_bytes) {
                Ok(v) => super::characters::rename(&st, &v, origin),
                Err(_) => bad_json(origin),
            }
        }
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
        ("POST", "/api/characters/export") => {
            match serde_json::from_slice::<serde_json::Value>(body_bytes) {
                Ok(v) => super::characters::export(&st, &v, origin),
                Err(_) => bad_json(origin),
            }
        }
        // chats 族（Phase 1 · manifest #9-#18）
        ("POST", "/api/chats/save") => {
            match serde_json::from_slice::<serde_json::Value>(body_bytes) {
                Ok(v) => super::chats::save(&st, &st.chats_svc, &v, origin),
                Err(_) => bad_json(origin),
            }
        }
        ("POST", "/api/chats/get") => match serde_json::from_slice::<serde_json::Value>(body_bytes)
        {
            Ok(v) => super::chats::get(&st, &v, origin),
            Err(_) => bad_json(origin),
        },
        ("POST", "/api/chats/rename") => {
            match serde_json::from_slice::<serde_json::Value>(body_bytes) {
                Ok(v) => super::chats::rename(&st, &v, origin),
                Err(_) => bad_json(origin),
            }
        }
        ("POST", "/api/chats/delete") => {
            match serde_json::from_slice::<serde_json::Value>(body_bytes) {
                Ok(v) => super::chats::delete(&st, &v, origin),
                Err(_) => bad_json(origin),
            }
        }
        ("POST", "/api/chats/search") => {
            match serde_json::from_slice::<serde_json::Value>(body_bytes) {
                Ok(v) => super::chats::search(&st, &v, origin),
                Err(_) => bad_json(origin),
            }
        }
        ("POST", "/api/chats/recent") => {
            match serde_json::from_slice::<serde_json::Value>(body_bytes) {
                Ok(v) => super::chats::recent(&st, &st.chats_svc, &v, origin),
                Err(_) => bad_json(origin),
            }
        }
        ("POST", "/api/chats/group/get") => {
            match serde_json::from_slice::<serde_json::Value>(body_bytes) {
                Ok(v) => super::chats::group_get(&st, &v, origin),
                Err(_) => bad_json(origin),
            }
        }
        ("POST", "/api/chats/group/info") => {
            match serde_json::from_slice::<serde_json::Value>(body_bytes) {
                Ok(v) => super::chats::group_info(&st, &v, origin),
                Err(_) => bad_json(origin),
            }
        }
        ("POST", "/api/chats/group/delete") => {
            match serde_json::from_slice::<serde_json::Value>(body_bytes) {
                Ok(v) => super::chats::group_delete(&st, &v, origin),
                Err(_) => bad_json(origin),
            }
        }
        ("POST", "/api/chats/group/save") => {
            match serde_json::from_slice::<serde_json::Value>(body_bytes) {
                Ok(v) => super::chats::group_save(&st, &st.chats_svc, &v, origin),
                Err(_) => bad_json(origin),
            }
        }
        // worldinfo 族（Phase 1 · manifest #19-#22）
        ("POST", "/api/worldinfo/list") => super::worldinfo_groups::wi_list(&st, origin),
        ("POST", "/api/worldinfo/get") => {
            match serde_json::from_slice::<serde_json::Value>(body_bytes) {
                Ok(v) => super::worldinfo_groups::wi_get(&st, &v, origin),
                Err(_) => bad_json(origin),
            }
        }
        ("POST", "/api/worldinfo/edit") => {
            match serde_json::from_slice::<serde_json::Value>(body_bytes) {
                Ok(v) => super::worldinfo_groups::wi_edit(&st, &v, origin),
                Err(_) => bad_json(origin),
            }
        }
        ("POST", "/api/worldinfo/delete") => {
            match serde_json::from_slice::<serde_json::Value>(body_bytes) {
                Ok(v) => super::worldinfo_groups::wi_delete(&st, &v, origin),
                Err(_) => bad_json(origin),
            }
        }
        ("POST", "/api/worldinfo/import") => {
            super::worldinfo_groups::wi_import(
                &st,
                content_type,
                body_bytes.to_vec().into(),
                origin,
            )
            .await
        }
        // groups 族（Phase 1 · groups.js）
        ("POST", "/api/groups/all") => super::worldinfo_groups::gr_all(&st, origin),
        ("POST", "/api/groups/create") => {
            match serde_json::from_slice::<serde_json::Value>(body_bytes) {
                Ok(v) => super::worldinfo_groups::gr_create(&st, &v, origin),
                Err(_) => bad_json(origin),
            }
        }
        ("POST", "/api/groups/edit") => {
            match serde_json::from_slice::<serde_json::Value>(body_bytes) {
                Ok(v) => super::worldinfo_groups::gr_edit(&st, &v, origin),
                Err(_) => bad_json(origin),
            }
        }
        ("POST", "/api/groups/delete") => {
            match serde_json::from_slice::<serde_json::Value>(body_bytes) {
                Ok(v) => super::worldinfo_groups::gr_delete(&st, &v, origin),
                Err(_) => bad_json(origin),
            }
        }
        // 资产族（Phase 1 · manifest #30-#40）
        ("GET", "/thumbnail") => {
            let q = path_q.split_once('?').map(|(_, q)| q).unwrap_or("");
            super::assets::thumbnail(&st, q, origin)
        }
        ("POST", "/api/avatars/get") => super::assets::avatars_get(&st, origin),
        ("POST", "/api/avatars/upload") => {
            super::assets::avatars_upload(&st, content_type, body_bytes.to_vec().into(), origin)
                .await
        }
        ("POST", "/api/avatars/delete") => {
            match serde_json::from_slice::<serde_json::Value>(body_bytes) {
                Ok(v) => super::assets::avatars_delete(&st, &v, origin),
                Err(_) => bad_json(origin),
            }
        }
        ("POST", "/api/backgrounds/all") => super::assets::backgrounds_all(&st, origin),
        ("POST", "/api/images/upload") => {
            match serde_json::from_slice::<serde_json::Value>(body_bytes) {
                Ok(v) => super::assets::images_upload(&st, &v, origin),
                Err(_) => bad_json(origin),
            }
        }
        ("GET", "/api/sprites/get") => {
            let q = path_q.split_once('?').map(|(_, q)| q).unwrap_or("");
            super::assets::sprites_get(&st, q, origin)
        }
        // tokenizers + secrets 族（Phase 1 · manifest #50-#53；Phase 2 批7 精确分层）
        ("POST", "/api/tokenizers/openai/count") => {
            // path_q = "?model=<m>..."; route() 传整串路径 + 查询
            let q = path_q
                .split_once('?')
                .map(|(_, q)| q)
                .unwrap_or(path_q)
                .split('&')
                .filter_map(|kv| kv.split_once('='))
                .find(|(k, _)| *k == "model")
                .map(|(_, v)| v.to_string());
            super::tokenizers_secrets::count_with_model(&st, q.as_deref(), body_bytes, origin)
        }
        ("POST", p) if p.starts_with("/api/tokenizers/") && p.ends_with("/encode") => {
            super::tokenizers_secrets::encode_with_body(
                p.trim_start_matches("/api/tokenizers/")
                    .trim_end_matches("/encode"),
                body_bytes,
                origin,
            )
        }
        ("POST", "/api/secrets/write") => {
            match serde_json::from_slice::<serde_json::Value>(body_bytes) {
                Ok(v) => super::tokenizers_secrets::secrets_write(&st, &v, origin),
                Err(_) => bad_json(origin),
            }
        }
        ("POST", "/api/secrets/read") => super::tokenizers_secrets::secrets_read(&st, origin),
        ("POST", "/api/secrets/view") => super::tokenizers_secrets::secrets_view(origin),
        // extras 族（Phase 1 · manifest #54-#65）：data-maid / translate / speech / vectors
        ("POST", "/api/data-maid/report") => {
            super::extras::maid_report(&st, &st.maid_tokens, origin)
        }
        ("POST", "/api/data-maid/finalize") => {
            match serde_json::from_slice::<serde_json::Value>(body_bytes) {
                Ok(v) => super::extras::maid_finalize(&st, &st.maid_tokens, &v, origin),
                Err(_) => bad_json(origin),
            }
        }
        ("POST", "/api/data-maid/delete") => {
            match serde_json::from_slice::<serde_json::Value>(body_bytes) {
                Ok(v) => super::extras::maid_delete(&st, &st.maid_tokens, &v, origin),
                Err(_) => bad_json(origin),
            }
        }
        ("GET", "/api/data-maid/view") => {
            let q = path_q.split_once('?').map(|(_, q)| q).unwrap_or("");
            super::extras::maid_view(&st, &st.maid_tokens, q, origin)
        }
        // 翻译族（Phase 2 批6 · manifest #54/#55：八家全量出站）
        ("POST", "/api/translate/onering") => match serde_json::from_slice::<serde_json::Value>(body_bytes) {
            Ok(v) => super::translate::onering(&st, &v, origin).await,
            Err(_) => bad_json(origin),
        },
        ("POST", p) if p.starts_with("/api/translate/") => {
            let provider = p.trim_start_matches("/api/translate/").to_string();
            match serde_json::from_slice::<serde_json::Value>(body_bytes) {
                Ok(v) => super::translate::dispatch(&st, &provider, &v, origin).await,
                Err(_) => bad_json(origin),
            }
        }
        ("POST", "/api/speech/synthesize") => super::extras::speech_local_unavailable(origin),
        // ElevenLabs TTS 出站（HIGH-4 · ST speech.js:234-272 契约：xi-api-key 服务端
        // 读 vault，上游锚点固定 api.elevenlabs.io，音频流透传，缺 key 400）
        ("POST", "/api/speech/elevenlabs/synthesize") => {
            match serde_json::from_slice::<serde_json::Value>(body_bytes) {
                Ok(v) => super::extras::speech_elevenlabs(&st, &v, origin).await,
                Err(_) => bad_json(origin),
            }
        }
        // OpenAI TTS 出站（Phase 2 批6 · manifest #58）
        ("POST", "/api/openai/generate-voice") => match serde_json::from_slice::<serde_json::Value>(body_bytes) {
            Ok(v) => super::extras::speech_openai(&st, &v, origin).await,
            Err(_) => bad_json(origin),
        },
        ("POST", "/api/vector/list") => {
            match serde_json::from_slice::<serde_json::Value>(body_bytes) {
                Ok(v) => super::extras::vector_list(&st, &v, origin),
                Err(_) => bad_json(origin),
            }
        }
        ("POST", "/api/vector/insert") => {
            match serde_json::from_slice::<serde_json::Value>(body_bytes) {
                Ok(v) => super::extras::vector_insert(&st, &v, origin),
                Err(_) => bad_json(origin),
            }
        }
        ("POST", "/api/vector/query") => {
            match serde_json::from_slice::<serde_json::Value>(body_bytes) {
                Ok(v) => super::extras::vector_query(&st, &v, origin),
                Err(_) => bad_json(origin),
            }
        }
        ("POST", "/api/vector/delete") => {
            match serde_json::from_slice::<serde_json::Value>(body_bytes) {
                Ok(v) => super::extras::vector_delete(&st, &v, origin),
                Err(_) => bad_json(origin),
            }
        }
        // providers 族（Phase 2 · manifest #41/#42：26 源全量）
        ("POST", "/api/backends/chat-completions/status") => {
            match serde_json::from_slice::<serde_json::Value>(body_bytes) {
                Ok(v) => super::providers::status(&st, &v, origin).await,
                Err(_) => bad_json(origin),
            }
        }
        ("POST", "/api/backends/chat-completions/generate") => {
            match serde_json::from_slice::<serde_json::Value>(body_bytes) {
                Ok(v) => super::providers::generate(&st, &v, origin).await,
                Err(_) => bad_json(origin),
            }
        }
        // text completion 通道（Phase 2 批3 · manifest #48/#49：15 后端）
        ("POST", "/api/backends/text-completions/status") => {
            match serde_json::from_slice::<serde_json::Value>(body_bytes) {
                Ok(v) => super::providers_textcomp::status(&st, &v, origin).await,
                Err(_) => bad_json(origin),
            }
        }
        ("POST", "/api/backends/text-completions/generate") => {
            match serde_json::from_slice::<serde_json::Value>(body_bytes) {
                Ok(v) => super::providers_textcomp::generate(&st, &v, origin).await,
                Err(_) => bad_json(origin),
            }
        }
        // AI Horde 独立通道（Phase 2 批4 · manifest #44-#47）
        ("POST", "/api/horde/generate-text") => {
            match serde_json::from_slice::<serde_json::Value>(body_bytes) {
                Ok(v) => super::horde::generate_text(&st, &v, origin).await,
                Err(_) => bad_json(origin),
            }
        }
        ("POST", "/api/horde/task-status") => {
            match serde_json::from_slice::<serde_json::Value>(body_bytes) {
                Ok(v) => super::horde::task_status(&st, &v, origin).await,
                Err(_) => bad_json(origin),
            }
        }
        ("POST", "/api/horde/cancel-task") => {
            match serde_json::from_slice::<serde_json::Value>(body_bytes) {
                Ok(v) => super::horde::cancel_task(&st, &v, origin).await,
                Err(_) => bad_json(origin),
            }
        }
        ("POST", "/api/horde/text-models") => {
            match serde_json::from_slice::<serde_json::Value>(body_bytes) {
                Ok(v) => super::horde::text_models(&st, &v, origin).await,
                Err(_) => bad_json(origin),
            }
        }
        // NovelAI 文本生成（Phase 2 批5 · manifest #43）
        ("POST", "/api/novelai/generate") => {
            match serde_json::from_slice::<serde_json::Value>(body_bytes) {
                Ok(v) => super::novelai::generate(&st, &v, origin).await,
                Err(_) => bad_json(origin),
            }
        }
        // extensions 族（Phase 1/5 · manifest #67-#68 + T14 git 管理族全量）
        ("GET", "/api/extensions/discover") => super::extensions::discover(&st, origin),
        ("POST", "/api/quick-replies/save") => {
            match serde_json::from_slice::<serde_json::Value>(body_bytes) {
                Ok(v) => super::extensions::qr_save(&st, &v, origin),
                Err(_) => bad_json(origin),
            }
        }
        // 扩展 git 管理族（T14：git2/libgit2 本地库 + 仅 https 收紧）
        ("POST", "/api/extensions/install") => match serde_json::from_slice::<serde_json::Value>(body_bytes) {
            Ok(v) => super::extensions::install(&st, &v, origin).await,
            Err(_) => bad_json(origin),
        },
        ("POST", "/api/extensions/update") => match serde_json::from_slice::<serde_json::Value>(body_bytes) {
            Ok(v) => super::extensions::update(&st, &v, origin).await,
            Err(_) => bad_json(origin),
        },
        ("POST", "/api/extensions/version") => match serde_json::from_slice::<serde_json::Value>(body_bytes) {
            Ok(v) => super::extensions::version(&st, &v, origin).await,
            Err(_) => bad_json(origin),
        },
        ("POST", "/api/extensions/branches") => match serde_json::from_slice::<serde_json::Value>(body_bytes) {
            Ok(v) => super::extensions::branches(&st, &v, origin).await,
            Err(_) => bad_json(origin),
        },
        ("POST", "/api/extensions/switch") => match serde_json::from_slice::<serde_json::Value>(body_bytes) {
            Ok(v) => super::extensions::switch_branch(&st, &v, origin).await,
            Err(_) => bad_json(origin),
        },
        ("POST", "/api/extensions/move") => match serde_json::from_slice::<serde_json::Value>(body_bytes) {
            Ok(v) => super::extensions::move_ext(&st, &v, origin).await,
            Err(_) => bad_json(origin),
        },
        ("POST", "/api/extensions/delete") => match serde_json::from_slice::<serde_json::Value>(body_bytes) {
            Ok(v) => super::extensions::delete(&st, &v, origin).await,
            Err(_) => bad_json(origin),
        }
        // 静态资源前缀（manifest #31-#33；鉴权豁免路径的落盘读取）
        (m, p)
            if m == "GET"
                && (p.starts_with("/characters/")
                    || p.starts_with("/backgrounds/")
                    || p.starts_with("/user-images/")) =>
        {
            super::assets::static_asset(&st, p, origin)
        }
        _ => not_implemented(origin, path_q),
    }
}

/// JSON 解析失败（400）
fn bad_json(origin: Option<HeaderValue>) -> Response {
    log::warn!(target: logx::API, "请求体不是合法 JSON（400）");
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
/// 501 + `{error:{message:"该端点尚未支持",code:"NOT_MIGRATED"}}`。
/// 前端既有错误处理（error.message 语义）可直接显示，不会当成功。
fn not_implemented(origin: Option<HeaderValue>, path_q: &str) -> Response {
    let path = path_q.split('?').next().unwrap_or("/");
    log::warn!(target: logx::API, "未实现端点（501）{path}");
    let resp = (
        StatusCode::NOT_IMPLEMENTED,
        Json(json!({
            "error": {
                "message": format!("该端点尚未支持：{path}"),
                "code": "NOT_MIGRATED",
            }
        })),
    )
        .into_response();
    apply_cors_resp(resp, origin)
}
