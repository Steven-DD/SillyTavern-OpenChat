//! backend/horde —— AI Horde 独立通道（Phase 2 批4 · manifest #44-#47）。
//!
//! ST `src/endpoints/horde.js`（1.19.0）核对：
//! - `/generate-text`：POST aihorde `/generate/text/async`，key=secret
//!   api_key_horde 或匿名 `0000000000`；非 ok → 200 {error:{message}}；
//!   连接失败 → 200 {error:true}
//! - `/task-status`/`/cancel-task`：`generate/text/status/{id}` GET/DELETE
//! - `/text-models`：`status/models?type=text` + text-model-reference db.json
//!   合并（is_whitelisted）；60s 内存缓存（ST Cache 字面）；force 跳缓存
//!
//! Client-Agent 格式 `name:version:contact`（ST getVersion().agent 语义）。
//! App use：api.ts hordeGenerate 三步轮询（3s 间隔、300s 上限、300s 上限
//! 取消）→ 后端只透出原样 JSON。

use super::providers::{json_resp, read_secret};
use super::BackendState;
use axum::http::{HeaderValue, StatusCode};
use axum::response::Response;
use serde_json::{json, Value};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const HORDE_ROOT: &str = "https://aihorde.net/api/v2";
const ANONYMOUS_KEY: &str = "0000000000";
const TEXT_MODEL_METADATA_URL: &str =
    "https://raw.githubusercontent.com/db0/AI-Horde-text-model-reference/main/db.json";
/// Client-Agent：`name:version:contact`（部署身份注记）
pub(crate) const CLIENT_AGENT: &str = "SillyTavern-OpenChat:1.0.0:Steven-DD";

fn cache() -> &'static Mutex<HordeCache> {
    static CACHE: std::sync::OnceLock<Mutex<HordeCache>> = std::sync::OnceLock::new();
    CACHE.get_or_init(|| {
        Mutex::new(HordeCache {
            models: None,
            models_at: None,
        })
    })
}

struct HordeCache {
    models: Option<Value>,
    models_at: Option<Instant>,
}

fn client_agent_header() -> (String, String) {
    ("Client-Agent".into(), CLIENT_AGENT.into())
}

async fn fetch_json(
    url: &str,
    method: reqwest::Method,
    key: Option<String>,
    body: Option<&Value>,
    timeout: Duration,
) -> (reqwest::StatusCode, Value) {
    let client = reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap_or_default();
    let mut req = client
        .request(method, url)
        .header("accept-encoding", "identity")
        .header(client_agent_header().0, client_agent_header().1)
        .timeout(timeout);
    if let Some(k) = key {
        req = req.header("apikey", k);
    }
    if let Some(b) = body {
        req = req.header("Content-Type", "application/json").json(b);
    }
    match req.send().await {
        Ok(r) => {
            let status = r.status();
            let text = r.text().await.unwrap_or_default();
            (
                status,
                serde_json::from_str::<Value>(&text).unwrap_or(json!(null)),
            )
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            json!({ "error": { "message": e.to_string() } }),
        ),
    }
}

/// POST /api/horde/generate-text（manifest #44）
pub async fn generate_text(
    st: &BackendState,
    body: &Value,
    origin: Option<HeaderValue>,
) -> Response {
    let Some(_) = body.as_object() else {
        return json_resp(StatusCode::OK, json!({ "error": true }), origin);
    };
    let api_key = read_secret(st, "api_key_horde").unwrap_or_else(|| ANONYMOUS_KEY.to_string());
    let url = format!("{HORDE_ROOT}/generate/text/async");
    let (status, data) = fetch_json(
        &url,
        reqwest::Method::POST,
        Some(api_key),
        Some(body),
        Duration::from_secs(60),
    )
    .await;
    if !status.is_success() {
        // ST 字面：{error:{message: 上游原文}}
        let msg = match data.get("message").and_then(|m| m.as_str()) {
            Some(m) => json!(m),
            None => json!(data.to_string()),
        };
        return json_resp(
            StatusCode::OK,
            json!({ "error": { "message": msg } }),
            origin,
        );
    }
    json_resp(StatusCode::OK, data, origin)
}

/// POST /api/horde/task-status（manifest #45）
pub async fn task_status(
    _st: &BackendState,
    body: &Value,
    origin: Option<HeaderValue>,
) -> Response {
    let Some(task_id) = body.get("taskId").and_then(|t| t.as_str()) else {
        return json_resp(
            StatusCode::INTERNAL_SERVER_ERROR,
            json!({ "error": true }),
            origin,
        );
    };
    let url = format!("{HORDE_ROOT}/generate/text/status/{task_id}");
    let (_, data) = fetch_json(
        &url,
        reqwest::Method::GET,
        None,
        None,
        Duration::from_secs(30),
    )
    .await;
    json_resp(StatusCode::OK, data, origin)
}

/// POST /api/horde/cancel-task（manifest #46）
pub async fn cancel_task(
    _st: &BackendState,
    body: &Value,
    origin: Option<HeaderValue>,
) -> Response {
    let Some(task_id) = body.get("taskId").and_then(|t| t.as_str()) else {
        return json_resp(
            StatusCode::INTERNAL_SERVER_ERROR,
            json!({ "error": true }),
            origin,
        );
    };
    let url = format!("{HORDE_ROOT}/generate/text/status/{task_id}");
    let (_, data) = fetch_json(
        &url,
        reqwest::Method::DELETE,
        None,
        None,
        Duration::from_secs(30),
    )
    .await;
    json_resp(StatusCode::OK, data, origin)
}

/// POST /api/horde/text-models（manifest #47：模型列表 + reference 元数据合并 + 60s 缓存）
pub async fn text_models(
    _st: &BackendState,
    body: &Value,
    origin: Option<HeaderValue>,
) -> Response {
    let force = body.get("force").and_then(|f| f.as_bool()).unwrap_or(false);
    {
        let c = cache().lock().unwrap();
        if !force {
            if let (Some(models), Some(at)) = (c.models.clone(), c.models_at) {
                if at.elapsed() < Duration::from_secs(60) {
                    return json_resp(StatusCode::OK, models, origin);
                }
            }
        }
    }

    let url = format!("{HORDE_ROOT}/status/models?type=text");
    let (status, mut data) = fetch_json(
        &url,
        reqwest::Method::GET,
        None,
        None,
        Duration::from_secs(30),
    )
    .await;
    if !status.is_success() {
        return json_resp(
            StatusCode::INTERNAL_SERVER_ERROR,
            json!({ "error": true }),
            origin,
        );
    }
    // reference db.json 合并（best-effort；失败不影响列表）
    match fetch_json(
        TEXT_MODEL_METADATA_URL,
        reqwest::Method::GET,
        None,
        None,
        Duration::from_secs(30),
    )
    .await
    {
        (_, metadata) if metadata.is_object() => {
            if data.is_null() {
                data = json!([]);
            }
            if !data.is_array() {
                data = json!([]);
            }
            let Some(arr) = data.as_array_mut() else {
                unreachable!("上文已保证数组形状");
            };
            for model in arr.iter_mut() {
                let name = model.get("name").and_then(|n| n.as_str()).unwrap_or("");
                match metadata.get(name) {
                    Some(m) => {
                        let mut merged = m.as_object().cloned().unwrap_or_default();
                        merged.insert("is_whitelisted".into(), json!(true));
                        if let Some(om) = model.as_object_mut() {
                            om.extend(merged);
                        }
                    }
                    None => {
                        model
                            .as_object_mut()
                            .map(|om| om.insert("is_whitelisted".into(), json!(false)));
                    }
                }
            }
        }
        _ => {}
    }
    {
        let mut c = cache().lock().unwrap();
        c.models = Some(data.clone());
        c.models_at = Some(Instant::now());
    }
    json_resp(StatusCode::OK, data, origin)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_shape() {
        // "name:version:contact" 三段
        assert_eq!(CLIENT_AGENT.split(':').count(), 3);
    }
}
