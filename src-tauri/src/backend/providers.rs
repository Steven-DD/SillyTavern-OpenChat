//! backend/providers —— 生成通道网关（Phase 1 切片 · manifest #41/#42 · 灰度面）
//!
//! 灰度口径（用户已裁决"native = 功能子集"）：
//! - **P0 切片：`openai` 与 `custom` 源**（同为 OpenAI Chat Completions 协议，
//!   覆盖 ST bodyParams 直通模型）。Claude/MakersSuite 及其余 20+ 源的
//!   prompt-converters 移植属「Provider 全量对齐」（方案 §十一 8-12 周），
//!   一期返回 501 NOT_MIGRATED（S20 降级，连续性清单 §1.2「未迁移源置灰」）。
//!
//! 已核对契约（chat-completions.js:1778-2200 status、2242-2734 generate）：
//! - /status {chat_completion_source, custom_url?, reverse_proxy?}：
//!   成功 → 原样返回上游 JSON（{data:[{id}...]}）；密钥缺失 → **400**（前端
//!   api.ts:531 直接抛错提示保存密钥）；上游故障 → **200 + {error:true}**
//!   （ST 字面 —— 前端 api.ts:573 按 r?.error 判定抛错）。
//! - /generate {chat_completion_source, messages[], model, stream, ...}：
//!   upstream body = ST 挑选的直通字段子集 + tools/json_schema 透传；
//!   custom_url 仅 custom 源（S12：http 仅限用户显式本地端点；SSRF 面在
//!   Phase 2 T13 收紧 —— 一期拦截非 https 且非环回）。
//!   stream=true → **上游响应体逐字节透传**（SSE 不缓冲，S14：上游请求显式
//!   accept-encoding: identity）；abort → 客户端断连时 drop 流即关闭上游
//!   （reqwest stream release connection）；错误 → {error:{message}}（S13：
//!   不回显完整 URL 与密钥）。
//! - 密钥：服务端读 secrets（active 条目；secrets.json 明文形态为 T12 前基线）。

use super::{apply_cors_resp, BackendState};
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use futures_util::StreamExt;
use serde_json::{json, Value};

fn err_json(status: StatusCode, msg: &str) -> Response {
    (status, Json(json!({ "error": { "message": msg } }))).into_response()
}

fn not_migrated(source: &str, origin: Option<HeaderValue>) -> Response {
    let resp = (
        StatusCode::NOT_IMPLEMENTED,
        Json(json!({
            "error": {
                "message": format!("生成源 {source} 尚未迁移到 native 后端（Provider 全量对齐属 Phase 2）：请切回 `st` 模式使用"),
                "code": "NOT_MIGRATED",
            }
        })),
    );
    apply_cors_resp(resp.into_response(), origin)
}

/// 服务端读密钥：取该 key 下唯一 active 条目（T12 前明文 JSON 基线）
fn read_secret(st: &BackendState, key: &str) -> Option<String> {
    let p = st.data_root.join("default-user").join("secrets.json");
    let text = std::fs::read_to_string(&p).ok()?;
    let secrets = serde_json::from_str::<Value>(&text).ok()?;
    secrets
        .get(key)?
        .as_array()?
        .iter()
        .find(|s| s.get("active").and_then(|b| b.as_bool()).unwrap_or(false))
        .and_then(|s| s.get("value").and_then(|v| v.as_str()))
        .map(|s| s.to_string())
}

/// 源 →（固定 API 根，secrets key）
fn openai_compatible_source(source: &str) -> Option<(&'static str, &'static str)> {
    match source {
        // ST: API_OPENAI（chat-completions.js:1803）；xai/groq 等同协议族在
        // Phase 2 批量接入 —— 一期只接 openai/custom 两个 raw 通道
        "openai" => Some(("https://api.openai.com/v1", "api_key_openai")),
        _ => None,
    }
}

/// S12 一期版：custom_url 必须 https（环回 http 放行给本地推理服务器，
/// 如 koboldcpp/ooba 的 textgen —— chat 源的 custom_url 默认全 https）
fn validate_custom_url(raw: &str) -> Result<String, String> {
    let is_local = |host: &str| host == "127.0.0.1" || host == "localhost" || host == "::1";
    if raw.starts_with("https://") {
        return Ok(raw.trim_end_matches('/').to_string());
    }
    if raw.starts_with("http://") {
        let host = raw
            .trim_start_matches("http://")
            .split('/')
            .next()
            .unwrap_or("")
            .split(':')
            .next()
            .unwrap_or("");
        if is_local(host) {
            return Ok(raw.trim_end_matches('/').to_string());
        }
        return Err("custom_url 仅允许 https（或环回 http 端点）".into());
    }
    Err("custom_url 协议不合法".into())
}

/// POST /api/backends/chat-completions/status（manifest #42）
/// 成功 → 上游 JSON 原样（含 data[]）；失败 → 200+{error:true}（ST 字面）；
/// 密钥缺失 → 400。
pub async fn status(st: &BackendState, body: &Value, origin: Option<HeaderValue>) -> Response {
    let Some(source) = body.get("chat_completion_source").and_then(|v| v.as_str()) else {
        return err_json(StatusCode::BAD_REQUEST, "缺少 chat_completion_source");
    };
    let (api_root, secret_key) = match openai_compatible_source(source) {
        Some(pair) => pair,
        None => {
            let url = if source == "custom" {
                body.get("custom_url")
                    .and_then(|u| u.as_str())
                    .map(|s| s.to_string())
            } else {
                None
            };
            if let Some(u) = url {
                // custom 源也走 OpenAI 协议（ST chat-completions.js:1804-1805）
                return match validate_custom_url(&u) {
                    Ok(root) => fetch_models(st, &root, read_secret(st, "api_key_custom"), origin).await,
                    Err(e) => err_json(StatusCode::BAD_REQUEST, &e),
                };
            }
            return not_migrated(source, origin);
        }
    };
    let key = read_secret(st, secret_key);
    fetch_models(st, api_root, key, origin).await
}

async fn fetch_models(st: &BackendState, api_root: &str, key: Option<String>, origin: Option<HeaderValue>) -> Response {
    let _ = st;
    let Some(key) = key else {
        // ST：密钥缺失 → 400（前端 api.ts:531「请先保存该通道的密钥」）
        return apply_cors_resp(StatusCode::BAD_REQUEST.into_response(), origin);
    };
    let client = reqwest::Client::builder().no_proxy().build().unwrap_or_default();
    let resp = client
        .get(format!("{api_root}/models"))
        .header("Authorization", format!("Bearer {key}"))
        .header("accept-encoding", "identity") // S14
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await;
    match resp {
        Ok(r) if r.status().is_success() => {
            let body = r.text().await.unwrap_or_default();
            match serde_json::from_str::<Value>(&body) {
                Ok(v) => apply_cors_resp(Json(v).into_response(), origin),
                Err(_) => apply_cors_resp(
                    Json(json!({ "error": true, "data": { "data": [] } })).into_response(),
                    origin,
                ),
            }
        }
        _ => {
            // 上游故障：ST 字面 200 + {error:true}（前端判 r?.error 后抛可读错误）
            apply_cors_resp(
                Json(json!({ "error": true, "data": { "data": [] } })).into_response(),
                origin,
            )
        }
    }
}

/// 构造 upstream 请求体（ST requestBody 字段子集直传，chat-completions.js:2651-2662）
fn build_upstream_body(body: &Value) -> Value {
    let pick = |keys: &[&str]| -> Value {
        let mut o = serde_json::Map::new();
        for k in keys {
            if let Some(v) = body.get(*k) {
                if !v.is_null() {
                    o.insert((*k).to_string(), v.clone());
                }
            }
        }
        Value::Object(o)
    };
    let mut up = pick(&[
        "model",
        "temperature",
        "max_tokens",
        "max_completion_tokens",
        "stream",
        "presence_penalty",
        "frequency_penalty",
        "top_p",
        "top_k",
        "logit_bias",
        "seed",
        "n",
        "reasoning_effort",
    ]);
    // messages（必需）
    if let Some(messages) = body.get("messages") {
        up.as_object_mut().unwrap().insert("messages".into(), messages.clone());
    }
    // tools（chat-completions.js:2632-2636）
    if let Some(tools) = body.get("tools").and_then(|t| t.as_array().cloned()) {
        if !tools.is_empty() {
            let o = up.as_object_mut().unwrap();
            o.insert("tools".into(), Value::Array(tools.clone()));
            if let Some(tc) = body.get("tool_choice") {
                o.insert("tool_choice".into(), tc.clone());
            }
        }
    }
    // json_schema（chat-completions.js:2638-2646：response_format 包一层）
    if let Some(schema_value) = body.get("json_schema").and_then(|j| j.get("value")) {
        up.as_object_mut().unwrap().insert(
            "response_format".into(),
            json!({
                "type": "json_schema",
                "json_schema": {
                    "name": body["json_schema"].get("name").cloned().unwrap_or_else(|| json!("default")),
                    "strict": body["json_schema"]["strict"].as_bool().unwrap_or(true),
                    "schema": schema_value.clone(),
                },
            }),
        );
    }
    up
}

/// POST /api/backends/chat-completions/generate（manifest #41）—— openai/custom 切片
pub async fn generate(st: &BackendState, body: &Value, origin: Option<HeaderValue>) -> Response {
    let Some(source) = body.get("chat_completion_source").and_then(|v| v.as_str()) else {
        return err_json(StatusCode::BAD_REQUEST, "缺少 chat_completion_source");
    };
    // 确定上游 URL 与密钥
    let (api_root, key, _extra_headers): (String, Option<String>, Vec<(String, String)>) = match source {
        "openai" => {
            // reverse_proxy：URL 直用，key 为明文 proxy_password（ST 同款语义）
            let with_rp = body.get("reverse_proxy").and_then(|v| v.as_str()).is_some();
            if with_rp {
                let url = body.get("reverse_proxy").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let pwd = body.get("proxy_password").and_then(|v| v.as_str()).unwrap_or("").to_string();
                (url, Some(pwd), Vec::new())
            } else {
                (
                    openai_compatible_source("openai").map(|(u, _)| u.to_string()).unwrap_or_default(),
                    read_secret(st, "api_key_openai"),
                    Vec::new(),
                )
            }
        }
        "custom" => {
            let url = match body.get("custom_url").and_then(|v| v.as_str()).map(validate_custom_url) {
                Some(Ok(u)) => u,
                Some(Err(e)) => return err_json(StatusCode::BAD_REQUEST, &e),
                None => return err_json(StatusCode::BAD_REQUEST, "缺少 custom_url"),
            };
            (url, read_secret(st, "api_key_custom"), Vec::new())
        }
        other => return not_migrated(other, origin),
    };

    if key.is_none() && body.get("reverse_proxy").and_then(|v| v.as_str()).is_none() {
        // ST：密钥缺失警告仍放行 custom（readSecret 返回空走上游 401 —— key为空时
        // 上游会报鉴权错；直接 400 更友好 —— 对齐 status 的 400 面）
        if source != "custom" {
            return apply_cors_resp(StatusCode::BAD_REQUEST.into_response(), origin);
        }
    }
    let key = key.unwrap_or_default();

    let upstream_url = if body
        .get("model")
        .and_then(|m| m.as_str())
        .map(|m| super::providers_text::is_text_completion_model(m))
        .unwrap_or(false)
    {
        format!("{api_root}/completions")
    } else {
        format!("{api_root}/chat/completions")
    };
    let upstream_body = build_upstream_body(body);

    let client = reqwest::Client::builder().no_proxy().build().unwrap_or_default();
    let resp = client
        .post(&upstream_url)
        .header("Authorization", format!("Bearer {key}"))
        .header("Content-Type", "application/json")
        .header("accept-encoding", "identity") // S14：上游明文，SSE 可透传
        .json(&upstream_body)
        .send()
        .await;

    let upstream = match resp {
        Ok(r) => r,
        Err(e) => {
            // S13：不回显 URL/密钥 —— 仅人类可读错误
            return err_json(StatusCode::BAD_GATEWAY, &format!("上游连接失败：{e}"));
        }
    };

    let status = upstream.status();
    if !status.is_success() {
        // E1：上游错误**原样文本**透传（前端 isRetryable 正则解析 429 文案）；
        // 状态码透传；不回显 URL。
        let text = upstream.text().await.unwrap_or_default();
        let code = axum::http::StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
        return apply_cors_resp((code, text).into_response(), origin);
    }

    if body.get("stream").and_then(|s| s.as_bool()).unwrap_or(true) {
        // SSE：逐字节透传（Body::from_stream —— 不缓冲）
        let stream = upstream
            .bytes_stream()
            .map(|r| r.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e)));
        return apply_cors_resp(
            axum::body::Body::from_stream(stream).into_response(),
            origin,
        );
    }

    // 非流式：整包 JSON 原样返回（ST response.send(json)）
    let text = upstream.text().await.unwrap_or_default();
    let v = serde_json::from_str::<Value>(&text).unwrap_or_else(|_| json!({}));
    apply_cors_resp(Json(v).into_response(), origin)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn st_with(tag: &str) -> BackendState {
        let root = std::env::temp_dir().join(format!("stchat-prov-{tag}-{}", std::process::id()));
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
    fn unknown_source_is_not_migrated() {
        let r = not_migrated("claude", None);
        assert_eq!(r.status().as_u16(), 501);
    }

    #[test]
    fn custom_url_validation() {
        assert!(validate_custom_url("https://api.x.com/v1").is_ok());
        assert!(validate_custom_url("http://127.0.0.1:5001/v1").is_ok(), "环回 http 放行");
        assert!(validate_custom_url("http://localhost:8008").is_ok());
        assert!(validate_custom_url("http://10.0.0.5/v1").is_err(), "非环回 http 拒");
        assert!(validate_custom_url("ftp://x").is_err());
        assert!(validate_custom_url("127.0.0.1:5001").is_err(), "无协议拒");
    }

    #[test]
    fn upstream_body_subset_and_schema() {
        let body = json!({
            "model": "gpt-4o",
            "messages": [{ "role": "user", "content": "hi" }],
            "temperature": 1,
            "stream": true,
            "unknown_front_only": "dropped",
            "json_schema": {
                "name": "out",
                "strict": true,
                "value": { "type": "object" },
            },
        });
        let up = build_upstream_body(&body);
        assert_eq!(up["model"], "gpt-4o");
        assert_eq!(up["messages"], body["messages"]);
        assert_eq!(up["stream"], true);
        assert!(up["response_format"]["json_schema"]["schema"].is_object());
        assert!(up.get("unknown_front_only").is_none(), "前端自有键不下发上游");
    }

    #[test]
    fn missing_status_key_is_400_sync() {
        // sync 支路覆盖：helper 直接读 secrets —— 空 → fetch_models 400
        let st = st_with("key");
        let r = tokio::runtime::Runtime::new().unwrap().block_on(async {
            fetch_models(&st, "https://api.openai.com/v1", None, None).await
        });
        assert_eq!(r.status().as_u16(), 400);
    }
}
