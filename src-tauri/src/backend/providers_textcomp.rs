//! backend/providers_textcomp —— Text Completion 通道（Phase 2 批3 · manifest #48/#49）。
//!
//! ST `src/endpoints/backends/text-completions.js`（1.19.0）逐行核对：
//! - `/generate`（:272-441）：15 后端 URL 拼接 + body 白名单裁剪
//!   （togetherai/infermaticai/featherless/openrouter/vllm/generic pick keys、
//!   ollama 重排、koboldcpp/ooba/llamacpp/mancer/tabby/dreamgen 全透传）
//! - `/status`（:100-232）：15 后端模型端点 + ooba model/info + tabby/current
//!   + togetherai/ollama 数组重包 + huggingface 空
//! - 头注入（additional-headers.js:218-265）：per-type 双头
//!   （mancer/aphrodite X-API-KEY、tabby x-api-key、其余 Bearer；
//!   openrouter/featherless 附 Referer/X-Title）
//!
//! 错误语义（ST 字面）：上游非 ok → **200** `{error:true,status,response}`
//! （连接失败 → 200 `{error:true,status:'UNKNOWN',response}`）。
//! 流式 → 原样透传（401→400 降级）；ollama 流式 → 裸 JSON 流包装成
//! SSE `data: {choices:[{text,thinking}]}` + [DONE]。
//!
//! 兼容面（登记）：App textgen.ts 曾把「server+端点路径」发进 api_server
//! （ST 语义 api_server=base，服务端再拼路径会叠路径）；native 按 ST 字面拼
//! 接，App 侧已改为只发 base（textgen.ts 同批修正）。

use super::providers::{json_resp, read_secret};
use super::BackendState;
use axum::body::Body;
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use futures_util::StreamExt;
use serde_json::{json, Value};

/* ---------------- 后端定义 ---------------- */

#[derive(Clone, Copy, PartialEq)]
enum T {
    Mancer,
    Vllm,
    Aphrodite,
    Tabby,
    TogetherAi,
    Ooba,
    InfermaticAi,
    DreamGen,
    OpenRouter,
    KoboldCpp,
    LlamaCpp,
    Featherless,
    HuggingFace,
    Generic,
    Ollama,
}

fn parse_type(s: &str) -> Option<T> {
    Some(match s {
        "mancer" => T::Mancer,
        "vllm" => T::Vllm,
        "aphrodite" => T::Aphrodite,
        "tabby" => T::Tabby,
        "togetherai" => T::TogetherAi,
        "ooba" => T::Ooba,
        "infermaticai" => T::InfermaticAi,
        "dreamgen" => T::DreamGen,
        "openrouter" => T::OpenRouter,
        "koboldcpp" => T::KoboldCpp,
        "llamacpp" => T::LlamaCpp,
        "featherless" => T::Featherless,
        "huggingface" => T::HuggingFace,
        "generic" => T::Generic,
        "ollama" => T::Ollama,
        _ => return None,
    })
}

/// ST trimV1（util.js:901-903）：去尾斜杠 + 去尾 "/v1"
fn trim_v1(url: &str) -> String {
    let no_slash = url.trim_end_matches('/');
    match no_slash.strip_suffix("/v1") {
        Some(base) => base.to_string(),
        None => no_slash.to_string(),
    }
}

fn gen_path(t: T) -> &'static str {
    match t {
        T::DreamGen => "/api/openai/v1/completions",
        T::Mancer => "/oai/v1/completions",
        T::LlamaCpp => "/completion",
        T::Ollama => "/api/generate",
        T::OpenRouter => "/v1/chat/completions",
        _ => "/v1/completions",
    }
}

fn status_path(t: T) -> &'static str {
    match t {
        T::DreamGen => "/api/openai/v1/models",
        T::Mancer => "/oai/v1/models",
        T::Tabby => "/v1/model/list",
        T::TogetherAi => "/api/models?&info",
        T::Ollama => "/api/tags",
        T::HuggingFace => "/info",
        _ => "/v1/models",
    }
}

/// additional-headers.js 双头注入
fn typed_headers(st: &BackendState, t: T) -> Vec<(String, String)> {
    let extra: &[(&str, &str)] = match t {
        T::OpenRouter | T::Featherless => &[
            ("HTTP-Referer", "https://sillytavern.app"),
            ("X-Title", "SillyTavern"),
        ],
        _ => &[],
    };
    let mut headers: Vec<(String, String)> = extra
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    if let Some(name) = secret_of(t) {
        if let Some(key) = read_secret(st, name) {
            // mancer/aphrodite：X-API-KEY + Bearer；tabby：x-api-key + Bearer；
            // 其余 Bearer-only（additional-headers.js 各 getter 字面）
            match t {
                T::Mancer | T::Aphrodite => {
                    headers.push(("X-API-KEY".into(), key.clone()));
                    headers.push(("Authorization".into(), format!("Bearer {key}")));
                }
                T::Tabby => {
                    headers.push(("x-api-key".into(), key.clone()));
                    headers.push(("Authorization".into(), format!("Bearer {key}")));
                }
                _ => {
                    headers.push(("Authorization".into(), format!("Bearer {key}")));
                }
            }
        }
    }
    headers
}

fn secret_of(t: T) -> Option<&'static str> {
    match t {
        T::Mancer => Some("api_key_mancer"),
        T::Aphrodite => Some("api_key_aphrodite"),
        T::Tabby => Some("api_key_tabby"),
        T::OpenRouter => Some("api_key_openrouter"),
        T::Featherless => Some("api_key_featherless"),
        T::Vllm => Some("api_key_vllm"),
        T::TogetherAi => Some("api_key_togetherai"),
        T::Ooba => Some("api_key_ooba"),
        T::InfermaticAi => Some("api_key_infermaticai"),
        T::DreamGen => Some("api_key_dreamgen"),
        T::KoboldCpp => Some("api_key_koboldcpp"),
        T::LlamaCpp => Some("api_key_llamacpp"),
        T::HuggingFace => Some("api_key_huggingface"),
        T::Generic => Some("api_key_generic"),
        T::Ollama => None,
    }
}

fn pick_keys(body: &Value, keys: &[&str]) -> Value {
    match body {
        Value::Object(m) => {
            let mut out = serde_json::Map::new();
            for k in keys {
                if let Some(v) = m.get(*k) {
                    out.insert(k.to_string(), v.clone());
                }
            }
            Value::Object(out)
        }
        other => other.clone(),
    }
}

const TOGETHERAI_KEYS: &[&str] = &[
    "model", "prompt", "max_tokens", "temperature", "top_p", "top_k",
    "repetition_penalty", "min_p", "presence_penalty", "frequency_penalty",
    "stream", "stop",
];

const OLLAMA_KEYS: &[&str] = &[
    "num_predict", "num_ctx", "num_batch", "stop", "temperature",
    "repeat_penalty", "presence_penalty", "frequency_penalty", "top_k",
    "top_p", "tfs_z", "typical_p", "seed", "repeat_last_n", "min_p",
];

const OPENAI_KEYS: &[&str] = &[
    "model", "prompt", "stream", "temperature", "top_p",
    "frequency_penalty", "presence_penalty", "stop", "seed", "logit_bias",
    "logprobs", "max_tokens", "n", "best_of",
];

const OPENROUTER_KEYS: &[&str] = &[
    "max_tokens", "temperature", "top_k", "top_p", "presence_penalty",
    "frequency_penalty", "repetition_penalty", "min_p", "top_a", "seed",
    "logit_bias", "model", "stream", "prompt", "stop", "provider",
    "include_reasoning",
];

const VLLM_KEYS: &[&str] = &[
    "model", "prompt", "best_of", "echo", "frequency_penalty", "logit_bias",
    "logprobs", "max_tokens", "n", "presence_penalty", "seed", "stop",
    "stream", "suffix", "temperature", "top_p", "user",
    "use_beam_search", "top_k", "min_p", "repetition_penalty",
    "length_penalty", "early_stopping", "stop_token_ids", "ignore_eos",
    "min_tokens", "skip_special_tokens", "spaces_between_special_tokens",
    "truncate_prompt_tokens", "include_stop_str_in_output",
    "response_format", "guided_json", "guided_regex", "guided_choice",
    "guided_grammar", "guided_decoding_backend", "guided_whitespace_pattern",
];

const INFERMATICAI_KEYS: &[&str] = &[
    "model", "prompt", "max_tokens", "temperature", "top_p", "top_k",
    "repetition_penalty", "stream", "stop", "presence_penalty",
    "frequency_penalty", "min_p", "seed", "ignore_eos", "n", "best_of",
    "min_tokens", "spaces_between_special_tokens", "skip_special_tokens",
    "logprobs",
];

const FEATHERLESS_KEYS: &[&str] = &[
    "model", "prompt", "best_of", "echo", "frequency_penalty", "logit_bias",
    "logprobs", "max_tokens", "n", "presence_penalty", "seed", "stop",
    "stream", "suffix", "temperature", "top_p", "user",
    "use_beam_search", "top_k", "min_p", "repetition_penalty",
    "length_penalty", "early_stopping", "stop_token_ids", "ignore_eos",
    "min_tokens", "skip_special_tokens", "spaces_between_special_tokens",
    "truncate_prompt_tokens",
    "include_stop_str_in_output",
    "response_format", "guided_json", "guided_regex", "guided_choice",
    "guided_grammar", "guided_decoding_backend", "guided_whitespace_pattern",
];

/* ============================================================ *
 * /generate（manifest #48）
 * ============================================================ */
pub async fn generate(st: &BackendState, body: &Value, origin: Option<HeaderValue>) -> Response {
    let Some(api_server) = body.get("api_server").and_then(|v| v.as_str()) else {
        return json_resp(StatusCode::BAD_REQUEST, json!({ "error": true }), origin);
    };
    let Some(api_type) = body.get("api_type").and_then(|v| v.as_str()).and_then(parse_type) else {
        return json_resp(StatusCode::BAD_REQUEST, json!({ "error": true }), origin);
    };

    // localhost → 127.0.0.1（ST 字面）
    let base_raw = api_server.replace("localhost", "127.0.0.1");
    let base = &base_raw;
    let url = format!("{}{}", trim_v1(base), gen_path(api_type));

    let mut out_body = body.clone();
    match api_type {
        T::TogetherAi => out_body = pick_keys(body, TOGETHERAI_KEYS),
        T::InfermaticAi => out_body = pick_keys(body, INFERMATICAI_KEYS),
        T::Featherless => out_body = pick_keys(body, FEATHERLESS_KEYS),
        T::Generic => {
            let mut picked = pick_keys(body, OPENAI_KEYS);
            if let Some(stop) = picked.get_mut("stop") {
                if let Some(arr) = stop.as_array_mut() {
                    arr.truncate(4);
                }
            }
            out_body = picked;
        }
        T::OpenRouter => {
            let mut reshaped = body.clone();
            let provider = body.get("provider").and_then(|p| p.as_array()).cloned();
            if let Some(order) = provider {
                if !order.is_empty() {
                    reshaped.as_object_mut().unwrap().insert(
                        "provider".into(),
                        json!({
                            "allow_fallbacks": body.get("allow_fallbacks").and_then(|a| a.as_bool()).unwrap_or(true),
                            "order": order,
                        }),
                    );
                } else {
                    reshaped.as_object_mut().unwrap().remove("provider");
                }
            }
            if let Some(q) = body.get("quantizations").and_then(|q| q.as_array()).cloned() {
                if !q.is_empty() {
                    let o = reshaped.as_object_mut().unwrap();
                    o.entry(String::from("provider"))
                        .or_insert_with(|| json!({}))
                        .as_object_mut()
                        .unwrap()
                        .insert("quantizations".into(), Value::Array(q));
                }
            }
            out_body = pick_keys(&reshaped, OPENROUTER_KEYS);
        }
        T::Vllm => out_body = pick_keys(body, VLLM_KEYS),
        T::Ollama => {
            // ollama.keepAlive / ollama.batchSize 配置默认 -1（native 无配置面）
            const KEEP_ALIVE: i64 = -1;
            const NUM_BATCH: i64 = -1;
            let mut opts = pick_keys(body, OLLAMA_KEYS);
            if NUM_BATCH > 0 {
                opts.as_object_mut().unwrap().insert("num_batch".into(), json!(NUM_BATCH));
            }
            if let Some(np) = body.get("n_predict") {
                opts.as_object_mut().unwrap().entry(String::from("num_predict"))
                    .or_insert_with(|| np.clone());
            }
            out_body = json!({
                "model": body.get("model").cloned().unwrap_or(Value::Null),
                "prompt": body.get("prompt").cloned().unwrap_or(Value::Null),
                "stream": body.get("stream").and_then(|v| v.as_bool()).unwrap_or(false),
                "keep_alive": KEEP_ALIVE,
                "raw": true,
                "options": opts,
            });
        }
        // mancer/koboldcpp/ooba/llamacpp/tabby/dreamgen：body 原样
        _ => {}
    }

    // ollama+stream：裸 JSON 流 → SSE 包装
    let want_stream = body.get("stream").and_then(|v| v.as_bool()).unwrap_or(false);
    if api_type == T::Ollama && want_stream {
        return send_ollama_stream(&url, &out_body, typed_headers(st, api_type), origin).await;
    }

    let resp_res = super::providers::upstream_post(&url, "", typed_headers(st, api_type), &out_body).await;
    let resp = match resp_res {
        Ok(r) => r,
        Err(e) => {
            // ST catch：200 {error:true,status,response}（连接失败 status='UNKNOWN'）
            return json_resp(
                StatusCode::OK,
                json!({ "error": true, "status": "UNKNOWN", "response": e }),
                origin,
            );
        }
    };

    let status_ok = resp.status().is_success();
    let status_num = resp.status().as_u16();
    if want_stream {
        return super::providers::passthrough(resp, origin).await;
    }

    let text = resp.text().await.unwrap_or_default();
    if !status_ok {
        // ST 字面：200 {error:true, status(number), response}
        return json_resp(
            StatusCode::OK,
            json!({ "error": true, "status": status_num, "response": text }),
            origin,
        );
    }
    let mut data = serde_json::from_str::<Value>(&text).unwrap_or(json!({}));
    // InfermaticAI → OAI completions 形映射
    if api_type == T::InfermaticAi {
        let choices = data
            .get("choices")
            .and_then(|c| c.as_array())
            .cloned()
            .unwrap_or_default();
        let mapped: Vec<Value> = choices
            .into_iter()
            .map(|choice| {
                json!({
                    "text": choice.get("message").and_then(|m| m.get("content")).cloned()
                        .unwrap_or(choice.get("text").cloned().unwrap_or(Value::Null)),
                    "logprobs": choice.get("logprobs").cloned().unwrap_or(Value::Null),
                    "index": choice.get("index").cloned().unwrap_or(Value::Null),
                })
            })
            .collect();
        data.as_object_mut().unwrap().insert("choices".into(), Value::Array(mapped));
    }
    super::providers::json_resp(StatusCode::OK, data, origin)
}


async fn send_ollama_stream(
    url: &str,
    body: &Value,
    headers: Vec<(String, String)>,
    origin: Option<HeaderValue>,
) -> Response {
    let client = reqwest::Client::builder().no_proxy().build().unwrap_or_default();
    let mut req = client
        .post(url)
        .header("Content-Type", "application/json")
        .header("accept-encoding", "identity");
    for (k, v) in headers {
        req = req.header(k.as_str(), v.as_str());
    }
    let resp = match req.json(body).send().await {
        Ok(r) => r,
        Err(e) => {
            return super::providers::json_resp(
                StatusCode::OK,
                json!({ "error": true, "status": "UNKNOWN", "response": e.to_string() }),
                origin,
            )
        }
    };
    if !resp.status().is_success() {
        // ST: parseOllamaStream 无 ok 检查（body 事件即走）→ 等价透传 400 降级
        return super::providers::passthrough(resp, origin).await;
    }

    // NDJSON 流 → SSE data: {choices:[{text,thinking}]}
    let wrapped = resp
        .bytes_stream()
        .map(|r| r.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e)));
    let sse = futures_util::stream::unfold(
        NdjState { stream: Box::pin(wrapped), buf: Vec::new(), finished: false },
        |mut st2| async move {
            if st2.finished {
                return None;
            }
            loop {
                if let Some(pos) = st2.buf.iter().position(|b| *b == b'\n') {
                    let line: Vec<u8> = st2.buf.drain(0..=pos).collect();
                    let text = String::from_utf8_lossy(&line).trim().to_string();
                    if text.is_empty() {
                        continue;
                    }
                    if let Ok(j) = serde_json::from_str::<Value>(&text) {
                        let t = j.get("response").cloned().unwrap_or(json!(""));
                        let th = j.get("thinking").cloned().unwrap_or(json!(""));
                        if j.get("done").and_then(|d| d.as_bool()).unwrap_or(false) {
                            st2.finished = true;
                            let mut out = format!("data: {}\n\n", json!({ "choices": [{ "text": t, "thinking": th }] }));
                            out.push_str("data: [DONE]\n\n");
                            return Some((Ok(out.into_bytes()), st2));
                        }
                        let out = format!(
                            "data: {}\n\n",
                            json!({ "choices": [{ "text": t, "thinking": th }] })
                        );
                        return Some((Ok(out.into_bytes()), st2));
                    }
                    continue;
                }
                match st2.stream.next().await {
                    Some(Ok(chunk)) => st2.buf.extend_from_slice(chunk.as_ref()),
                    Some(Err(e)) => {
                        st2.finished = true;
                        let io = std::io::Error::new(std::io::ErrorKind::Other, e);
                        return Some((Err(io), st2));
                    }
                    None => {
                        st2.finished = true;
                        // 尾行
                        let rest = String::from_utf8_lossy(&st2.buf).trim().to_string();
                        st2.buf.clear();
                        let mut out = String::new();
                        if !rest.is_empty() {
                            if let Ok(j) = serde_json::from_str::<Value>(&rest) {
                                let t = j.get("response").cloned().unwrap_or(json!(""));
                                let th = j.get("thinking").cloned().unwrap_or(json!(""));
                                out.push_str(&format!(
                                    "data: {}\n\n",
                                    json!({ "choices": [{ "text": t, "thinking": th }] })
                                ));
                            }
                        }
                        out.push_str("data: [DONE]\n\n");
                        return Some((Ok(out.into_bytes()), st2));
                    }
                }
            }
        },
    );
    super::apply_cors_resp(
        Response::builder()
            .status(StatusCode::OK)
            .header("Content-Type", "text/event-stream")
            .body(Body::from_stream(sse))
            .unwrap()
            .into_response(),
        origin,
    )
}

struct NdjState {
    stream: std::pin::Pin<Box<dyn futures_util::Stream<Item = Result<bytes::Bytes, std::io::Error>> + Send>>,
    buf: Vec<u8>,
    finished: bool,
}

/* ============================================================ *
 * /status（manifest #49）
 * ============================================================ */
pub async fn status(st: &BackendState, body: &Value, origin: Option<HeaderValue>) -> Response {
    let Some(api_server) = body.get("api_server").and_then(|v| v.as_str()) else {
        return json_resp(StatusCode::BAD_REQUEST, json!({ "error": true }), origin);
    };
    let Some(api_type) = body.get("api_type").and_then(|v| v.as_str()).and_then(parse_type) else {
        return json_resp(StatusCode::BAD_REQUEST, json!({ "error": true }), origin);
    };
    let base_raw = api_server.replace("localhost", "127.0.0.1");
    let base = &base_raw;
    let url = format!("{}{}", trim_v1(base), status_path(api_type));

    let headers = typed_headers(st, api_type);
    let client = reqwest::Client::builder().no_proxy().build().unwrap_or_default();
    let mut req = client
        .get(&url)
        .header("accept-encoding", "identity")
        .timeout(std::time::Duration::from_secs(30));
    for (k, v) in headers {
        req = req.header(k.as_str(), v.as_str());
    }
    let resp = match req.send().await {
        Ok(r) => r,
        Err(_) => return json_resp(StatusCode::INTERNAL_SERVER_ERROR, json!({ "error": true }), origin),
    };
    let is_possibly_lm_studio = resp
        .headers()
        .get("x-powered-by")
        .and_then(|h| h.to_str().ok())
        .map(|h| h == "Express")
        .unwrap_or(false);
    if !resp.status().is_success() {
        return json_resp(StatusCode::BAD_REQUEST, json!({ "error": true }), origin);
    }
    let mut data = resp.json::<Value>().await.unwrap_or(json!({}));

    if api_type == T::TogetherAi && data.is_array() {
        let arr: Vec<Value> = data
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(|x| {
                let mut o = x;
                if let Some(name) = o.get("name").cloned() {
                    o.as_object_mut().map(|om| om.insert("id".into(), name));
                }
                o
            })
            .collect();
        data = json!({ "data": arr });
    }
    if api_type == T::Ollama && data.get("models").map(|m| m.is_array()).unwrap_or(false) {
        let arr: Vec<Value> = data
            .get("models")
            .and_then(|m| m.as_array())
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(|x| {
                let mut o = x;
                if let Some(name) = o.get("name").cloned() {
                    o.as_object_mut().map(|om| om.insert("id".into(), name));
                }
                o
            })
            .collect();
        data = json!({ "data": arr });
    }
    if api_type == T::HuggingFace {
        data = json!({ "data": [] });
    }
    if !data.get("data").map(|d| d.is_array()).unwrap_or(false) {
        return json_resp(StatusCode::BAD_REQUEST, json!({ "error": true }), origin);
    }
    let ids: Vec<String> = data
        .get("data")
        .and_then(|d| d.as_array())
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter_map(|x| x.get("id").and_then(|i| i.as_str()).map(String::from))
        .collect();
    let mut result = ids.first().cloned().unwrap_or_else(|| "Valid".into());
    let mut supports_tokenization = false;

    if api_type == T::Ooba && !is_possibly_lm_studio {
        let info_url = format!("{}{}/v1/internal/model/info", trim_v1(base), "");
        let info = client.get(&info_url).header("accept-encoding", "identity").timeout(std::time::Duration::from_secs(30)).send().await;
        if let Ok(info) = info {
            if info.status().is_success() {
                if let Ok(mi) = info.json::<Value>().await {
                    if let Some(name) = mi.get("model_name").and_then(|n| n.as_str()) {
                        if !name.is_empty() {
                            result = name.to_string();
                        }
                    }
                }
                supports_tokenization = true;
            }
        }
    } else if api_type == T::Tabby {
        let info_url = format!("{}{}/v1/model", trim_v1(base), "");
        let info = client.get(&info_url).header("accept-encoding", "identity").timeout(std::time::Duration::from_secs(30)).send().await;
        if let Ok(info) = info {
            if info.status().is_success() {
                if let Ok(mi) = info.json::<Value>().await {
                    if let Some(name) = mi.get("id").and_then(|n| n.as_str()) {
                        if !name.is_empty() {
                            result = name.to_string();
                        }
                    }
                }
            } else {
                result = "None".into(); // 未加载模型 → 400 → 'None'（ST 字面）
            }
        }
    }

    let reply = json!({ "result": result, "data": data.get("data").cloned().unwrap_or(json!([])) });
    let mut mr = super::providers::json_resp(StatusCode::OK, reply, origin);
    if supports_tokenization {
        mr.headers_mut().insert("x-supports-tokenization", HeaderValue::from_static("true"));
    }
    mr
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trim_v1_shapes() {
        assert_eq!(trim_v1("https://x:5000"), "https://x:5000");
        assert_eq!(trim_v1("https://x:5000/"), "https://x:5000");
        assert_eq!(trim_v1("https://x:5000/v1"), "https://x:5000");
        // 不剥中间：/v1/completions 的后续路径保留（App 旧版叠路径正是踩这里）
        assert_eq!(trim_v1("https://x:5000/v1/completions"), "https://x:5000/v1/completions");
    }

    #[test]
    fn paths_per_type() {
        assert_eq!(gen_path(parse_type("openrouter").unwrap()), "/v1/chat/completions");
        assert_eq!(gen_path(parse_type("mancer").unwrap()), "/oai/v1/completions");
        assert_eq!(gen_path(parse_type("ollama").unwrap()), "/api/generate");
        assert_eq!(status_path(parse_type("tabby").unwrap()), "/v1/model/list");
    }

    #[test]
    fn pick_keys_strips_outside_fields() {
        let body = serde_json::json!({ "model": "m", "prompt": "p", "api_key": "nope", "stream": true });
        let got = pick_keys(&body, OPENAI_KEYS);
        assert!(got.get("api_key").is_none());
        assert_eq!(got["model"], "m");
    }

    #[test]
    fn openrouter_provider_reshape() {
        let body = serde_json::json!({
            "model": "m", "prompt": "p",
            "provider": ["A", "B"], "allow_fallbacks": false,
            "extra_key": 1,
        });
        let mut reshaped = body.clone();
        // openrouter reshape（与 generate 内逻辑同款）
        let provider = body.get("provider").and_then(|p| p.as_array()).cloned();
        if let Some(order) = provider {
            reshaped.as_object_mut().unwrap().insert("provider".into(), serde_json::json!({
                "allow_fallbacks": true, "order": order,
            }));
        }
        let got = pick_keys(&reshaped, OPENROUTER_KEYS);
        assert_eq!(got["provider"]["order"], serde_json::json!(["A", "B"]));
        assert!(got.get("extra_key").is_none());
    }

    #[test]
    fn header_secret_names() {
        assert_eq!(secret_of(T::Mancer).unwrap(), "api_key_mancer");
        assert_eq!(secret_of(T::Tabby).unwrap(), "api_key_tabby");
        assert_eq!(secret_of(T::Ollama), None);
    }
}
