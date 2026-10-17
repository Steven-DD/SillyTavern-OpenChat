//! backend/providers_misc —— 独立协议 chat-completion sender 族（Phase 2 批3）。
//!
//! ST `src/endpoints/backends/chat-completions.js`（1.19.0）逐行核对：
//! - sendMistralAIRequest（:881-964）
//! - sendAI21Request（:800-874）、sendDeepSeekRequest（:1071-1176）
//! - sendXaiRequest（:1183-1282）、sendAimlapiRequest（:1289-1387）
//! - sendElectronHubRequest（:1394-1499）、sendChutesRequest（:1506-1600）
//! - sendMinimaxRequest（:1607-1681）、sendAzureOpenAIRequest（:1687-1774）
//! - sendCohereRequest（:971-1064）
//!
//! 错误语义（ST 字面）：缺密钥→400 {error:true}；上游非 2xx（非流式）→
//! 500 + tryParse(errorText) ?? {error:true}；连接失败→ 500 {error:true}
//!（claude/azure 例外各自字面）；流式→ 原样透传（401→400 降级，
//! forwardFetchResponse.ts:732-761）。
//!
//! 输出面偏差（登记）：`cohere` 上游是 Cohere v2 原生协议（SSE/JSON 均
//! 不是 OpenAI 形），App 前端 stStream/stGenerateBlocking 只有 OpenAI/
//! Google 双解析器 → 本模块把 cohere 的**响应**转成 OpenAI 形
//!（choices[0].delta.content / message.content，finish_reason 映射；
//! 请求面保持 Cohere v2 ST 字面）。其余源原生 OpenAI 兼容 → 原样透传。

use super::providers_convert as conv;
use super::providers_convert::PromptNames;
use super::BackendState;
use axum::http::{HeaderValue, StatusCode};
use axum::response::Response;
use serde_json::{json, Map, Value};

fn names_of(body: &Value) -> PromptNames {
    PromptNames::from_body(body)
}

/// take 语义：字段**存在**即取（含 null —— JS JSON.stringify 会保留 null），
/// 不存在才跳过。对齐 ST `'field': request.body.field` 直通字面。
fn take(body: &Value, key: &str) -> Option<Value> {
    body.get(key).cloned()
}

/* ---------------------------------------------------------------- *
 * MistralAI（OpenAI 兼容协议 + Mistral 转换 + random_seed 负值剔除）
 * ---------------------------------------------------------------- */
pub async fn send_mistral(
    st: &BackendState,
    body: &Value,
    origin: Option<HeaderValue>,
) -> Response {
    let (use_rp, api_root) = match body
        .get("reverse_proxy")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
    {
        Some(rp) => (true, rp.to_string()),
        None => (false, super::providers::API_MISTRAL.to_string()),
    };
    let key = if use_rp {
        body.get("proxy_password")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    } else {
        super::providers::read_secret(st, "api_key_mistralai").unwrap_or_default()
    };
    if key.is_empty() {
        return super::providers::json_resp(
            StatusCode::BAD_REQUEST,
            json!({ "error": true }),
            origin,
        );
    }

    let mut msgs = take_array_else_empty(body, "messages");
    let messages = conv::convert_mistral_messages(&mut msgs, &names_of(body));

    let mut rb = Map::new();
    put(&mut rb, "model", take(body, "model"));
    rb.insert("messages".into(), messages);
    put(&mut rb, "temperature", take(body, "temperature"));
    put(&mut rb, "top_p", take(body, "top_p"));
    put(
        &mut rb,
        "frequency_penalty",
        take(body, "frequency_penalty"),
    );
    put(&mut rb, "presence_penalty", take(body, "presence_penalty"));
    put(&mut rb, "max_tokens", take(body, "max_tokens"));
    put(&mut rb, "stream", take(body, "stream"));
    put(&mut rb, "safe_prompt", take(body, "safe_prompt"));
    // random_seed: seed === -1 ? undefined : seed（ST 字面）
    match take(body, "seed") {
        Some(v) if v.as_i64() == Some(-1) => {}
        Some(v) => {
            rb.insert("random_seed".into(), v);
        }
        None => {}
    };
    if take(body, "stop")
        .map(|s| s.is_array() && !s.as_array().unwrap().is_empty())
        .unwrap_or(false)
    {
        put(&mut rb, "stop", take(body, "stop"));
    }
    tools_into(&mut rb, body);
    json_schema_response_format(&mut rb, body, true, true);

    super::providers::relay_independent(
        super::providers::upstream_post(
            &format!("{api_root}/chat/completions"),
            &key,
            Vec::new(),
            &Value::Object(rb),
        )
        .await,
        stream_of(body),
        origin,
        super::providers::OutShape::Pass,
    )
    .await
}

/* ---------------------------------------------------------------- *
 * AI21（Jamba：OpenAI 兼容 + convertAI21Messages + json_object hack）
 * ---------------------------------------------------------------- */
pub async fn send_ai21(st: &BackendState, body: &Value, origin: Option<HeaderValue>) -> Response {
    let key = super::providers::read_secret(st, "api_key_ai21").unwrap_or_default();
    if key.is_empty() {
        return super::providers::json_resp(
            StatusCode::BAD_REQUEST,
            json!({ "error": true }),
            origin,
        );
    }

    let mut body_params = Map::new();
    let mut msgs = take_array_else_empty(body, "messages");
    if body
        .get("json_schema")
        .map(|j| j.is_object())
        .unwrap_or(false)
    {
        body_params.insert("response_format".into(), json!({ "type": "json_object" }));
        let schema_text = super::providers::json_pretty4(
            body.get("json_schema")
                .and_then(|j| j.get("value"))
                .unwrap_or(&json!({})),
        );
        msgs.push(json!({
            "role": "user",
            "content": format!("JSON schema for the response:\n{schema_text}"),
        }));
    }
    let converted = conv::convert_ai21_messages(&mut msgs, &names_of(body));
    let mut rb = Map::new();
    rb.insert("messages".into(), converted);
    put(&mut rb, "model", take(body, "model"));
    put(&mut rb, "max_tokens", take(body, "max_tokens"));
    put(&mut rb, "temperature", take(body, "temperature"));
    put(&mut rb, "top_p", take(body, "top_p"));
    put(&mut rb, "stop", take(body, "stop"));
    put(&mut rb, "stream", take(body, "stream"));
    put(&mut rb, "tools", take(body, "tools"));
    rb.extend(body_params);

    super::providers::relay_independent(
        super::providers::upstream_post(
            &format!("{}{}/chat/completions", super::providers::API_AI21, ""),
            &key,
            Vec::new(),
            &Value::Object(rb),
        )
        .await,
        stream_of(body),
        origin,
        super::providers::OutShape::Pass,
    )
    .await
}

/* ---------------------------------------------------------------- *
 * DeepSeek（OpenAI 兼容 + SEMI_TOOLS 后处理 + thinking + 前缀补丁）
 * ---------------------------------------------------------------- */
pub async fn send_deepseek(
    st: &BackendState,
    body: &Value,
    origin: Option<HeaderValue>,
) -> Response {
    let (use_rp, api_root) = match body
        .get("reverse_proxy")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
    {
        Some(rp) => (true, rp.to_string()),
        None => (false, super::providers::API_DEEPSEEK.to_string()), // /beta 基线
    };
    let key = if use_rp {
        body.get("proxy_password")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    } else {
        super::providers::read_secret(st, "api_key_deepseek").unwrap_or_default()
    };
    if key.is_empty() {
        return super::providers::json_resp(
            StatusCode::BAD_REQUEST,
            json!({ "error": true }),
            origin,
        );
    }

    let mut body_params = Map::new();
    let mut msgs = take_array_else_empty(body, "messages");
    if body
        .get("json_schema")
        .map(|j| j.is_object())
        .unwrap_or(false)
    {
        body_params.insert("response_format".into(), json!({ "type": "json_object" }));
        let schema_text = super::providers::json_pretty4(
            body.get("json_schema")
                .and_then(|j| j.get("value"))
                .unwrap_or(&json!({})),
        );
        msgs.push(json!({
            "role": "user",
            "content": format!("JSON schema for the response:\n{schema_text}"),
        }));
    }

    let mut processed =
        conv::post_process_prompt(&Value::Array(msgs), "semi_tools", &names_of(body));
    let processed_arr = processed.as_array_mut().unwrap();
    conv::add_assistant_prefix(processed_arr, body_params.get("tools"), "prefix");
    conv::add_reasoning_content_to_tool_calls(processed_arr);

    if body
        .get("include_reasoning")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        if let Some(effort) = body.get("reasoning_effort").and_then(|v| v.as_str()) {
            body_params.insert("reasoning_effort".into(), json!(effort));
        }
    }

    let mut rb = Map::new();
    rb.insert("messages".into(), processed);
    put(&mut rb, "model", take(body, "model"));
    put(&mut rb, "temperature", take(body, "temperature"));
    put(&mut rb, "max_tokens", take(body, "max_tokens"));
    put(&mut rb, "stream", take(body, "stream"));
    put(&mut rb, "presence_penalty", take(body, "presence_penalty"));
    put(
        &mut rb,
        "frequency_penalty",
        take(body, "frequency_penalty"),
    );
    put(&mut rb, "top_p", take(body, "top_p"));
    put(&mut rb, "stop", take(body, "stop"));
    put(&mut rb, "seed", take(body, "seed"));
    rb.insert(
        "thinking".into(),
        json!({ "type": if body.get("include_reasoning").and_then(|v| v.as_bool()).unwrap_or(false) { "enabled" } else { "disabled" } }),
    );
    rb.extend(body_params);

    super::providers::relay_independent(
        super::providers::upstream_post(
            &format!("{api_root}/chat/completions"),
            &key,
            Vec::new(),
            &Value::Object(rb),
        )
        .await,
        stream_of(body),
        origin,
        super::providers::OutShape::Pass,
    )
    .await
}

/* ---------------------------------------------------------------- *
 * xAI（OpenAI 兼容 + 转换 + reasoning_effort 二值化 high/low）
 * ---------------------------------------------------------------- */
pub async fn send_xai(st: &BackendState, body: &Value, origin: Option<HeaderValue>) -> Response {
    let (use_rp, api_root) = match body
        .get("reverse_proxy")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
    {
        Some(rp) => (true, rp.to_string()),
        None => (false, super::providers::API_XAI.to_string()),
    };
    let key = if use_rp {
        body.get("proxy_password")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    } else {
        super::providers::read_secret(st, "api_key_xai").unwrap_or_default()
    };
    if key.is_empty() {
        return super::providers::json_resp(
            StatusCode::BAD_REQUEST,
            json!({ "error": true }),
            origin,
        );
    }

    let mut body_params = Map::new();
    logprobs_into(&mut body_params, body);
    tools_into(&mut body_params, body);
    if take(body, "stop")
        .map(|s| s.is_array() && !s.as_array().unwrap().is_empty())
        .unwrap_or(false)
    {
        body_params.insert("stop".into(), take(body, "stop").unwrap());
    }
    if let Some(effort) = body.get("reasoning_effort").and_then(|v| v.as_str()) {
        body_params.insert(
            "reasoning_effort".into(),
            json!(if effort == "high" { "high" } else { "low" }),
        );
    }
    json_schema_response_format(&mut body_params, body, false, false);

    let mut msgs = take_array_else_empty(body, "messages");
    let messages = conv::convert_xai_messages(&mut msgs, &names_of(body));

    let mut rb = Map::new();
    rb.insert("messages".into(), messages);
    put(&mut rb, "model", take(body, "model"));
    put(&mut rb, "temperature", take(body, "temperature"));
    put(&mut rb, "max_tokens", take(body, "max_tokens"));
    put(
        &mut rb,
        "max_completion_tokens",
        take(body, "max_completion_tokens"),
    );
    put(&mut rb, "stream", take(body, "stream"));
    put(&mut rb, "presence_penalty", take(body, "presence_penalty"));
    put(
        &mut rb,
        "frequency_penalty",
        take(body, "frequency_penalty"),
    );
    put(&mut rb, "top_p", take(body, "top_p"));
    put(&mut rb, "seed", take(body, "seed"));
    put(&mut rb, "n", take(body, "n"));
    rb.extend(body_params);

    super::providers::relay_independent(
        super::providers::upstream_post(
            &format!("{api_root}/chat/completions"),
            &key,
            Vec::new(),
            &Value::Object(rb),
        )
        .await,
        stream_of(body),
        origin,
        super::providers::OutShape::Pass,
    )
    .await
}

/* ---------------------------------------------------------------- *
 * AI/ML API（OpenAI 兼容 + 平台 Referer/X-Title 头）
 * ---------------------------------------------------------------- */
pub async fn send_aimlapi(
    st: &BackendState,
    body: &Value,
    origin: Option<HeaderValue>,
) -> Response {
    let key = super::providers::read_secret(st, "api_key_aimlapi").unwrap_or_default();
    if key.is_empty() {
        return super::providers::json_resp(
            StatusCode::BAD_REQUEST,
            json!({ "error": true }),
            origin,
        );
    }

    let mut body_params = Map::new();
    logprobs_into(&mut body_params, body);
    tools_into(&mut body_params, body);
    if take(body, "stop")
        .map(|s| s.is_array() && !s.as_array().unwrap().is_empty())
        .unwrap_or(false)
    {
        body_params.insert("stop".into(), take(body, "stop").unwrap());
    }
    put(
        &mut body_params,
        "reasoning_effort",
        take(body, "reasoning_effort"),
    );
    json_schema_response_format(&mut body_params, body, false, true);

    let mut rb = Map::new();
    put(&mut rb, "messages", take(body, "messages"));
    put(&mut rb, "model", take(body, "model"));
    put(&mut rb, "temperature", take(body, "temperature"));
    put(&mut rb, "max_tokens", take(body, "max_tokens"));
    put(&mut rb, "stream", take(body, "stream"));
    put(&mut rb, "presence_penalty", take(body, "presence_penalty"));
    put(
        &mut rb,
        "frequency_penalty",
        take(body, "frequency_penalty"),
    );
    put(&mut rb, "top_p", take(body, "top_p"));
    put(&mut rb, "seed", take(body, "seed"));
    put(&mut rb, "n", take(body, "n"));
    rb.extend(body_params);

    super::providers::relay_independent(
        super::providers::upstream_post(
            &format!("{}{}/chat/completions", super::providers::API_AIMLAPI, ""),
            &key,
            super::providers::aimlapi_headers(),
            &Value::Object(rb),
        )
        .await,
        stream_of(body),
        origin,
        super::providers::OutShape::Pass,
    )
    .await
}

/* ---------------------------------------------------------------- *
 * ElectronHub（OpenAI 兼容）
 * ---------------------------------------------------------------- */
pub async fn send_electronhub(
    st: &BackendState,
    body: &Value,
    origin: Option<HeaderValue>,
) -> Response {
    let key = super::providers::read_secret(st, "api_key_electronhub").unwrap_or_default();
    if key.is_empty() {
        return super::providers::json_resp(
            StatusCode::BAD_REQUEST,
            json!({ "error": true }),
            origin,
        );
    }

    let mut body_params = Map::new();
    if body
        .get("enable_web_search")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        body_params.insert("web_search".into(), Value::Bool(true));
    }
    tools_into(&mut body_params, body);
    put(
        &mut body_params,
        "reasoning_effort",
        take(body, "reasoning_effort"),
    );
    json_schema_response_format(&mut body_params, body, false, true);

    let mut rb = Map::new();
    put(&mut rb, "messages", take(body, "messages"));
    put(&mut rb, "model", take(body, "model"));
    put(&mut rb, "temperature", take(body, "temperature"));
    put(&mut rb, "max_tokens", take(body, "max_tokens"));
    put(&mut rb, "stream", take(body, "stream"));
    put(&mut rb, "presence_penalty", take(body, "presence_penalty"));
    put(
        &mut rb,
        "frequency_penalty",
        take(body, "frequency_penalty"),
    );
    put(&mut rb, "top_p", take(body, "top_p"));
    put(&mut rb, "top_k", take(body, "top_k"));
    put(&mut rb, "logit_bias", take(body, "logit_bias"));
    put(&mut rb, "seed", take(body, "seed"));
    rb.extend(body_params);

    super::providers::relay_independent(
        super::providers::upstream_post(
            &format!(
                "{}{}/chat/completions",
                super::providers::API_ELECTRONHUB,
                ""
            ),
            &key,
            Vec::new(),
            &Value::Object(rb),
        )
        .await,
        stream_of(body),
        origin,
        super::providers::OutShape::Pass,
    )
    .await
}

/* ---------------------------------------------------------------- *
 * Chutes（OpenAI 兼容 + repetition_penalty/min_p 宽采样）
 * ---------------------------------------------------------------- */
pub async fn send_chutes(st: &BackendState, body: &Value, origin: Option<HeaderValue>) -> Response {
    let key = super::providers::read_secret(st, "api_key_chutes").unwrap_or_default();
    if key.is_empty() {
        return super::providers::json_resp(
            StatusCode::BAD_REQUEST,
            json!({ "error": true }),
            origin,
        );
    }

    let mut body_params = Map::new();
    tools_into(&mut body_params, body);
    logprobs_into(&mut body_params, body);
    json_schema_response_format(&mut body_params, body, false, true);

    let mut rb = Map::new();
    put(&mut rb, "messages", take(body, "messages"));
    put(&mut rb, "model", take(body, "model"));
    put(&mut rb, "temperature", take(body, "temperature"));
    put(&mut rb, "max_tokens", take(body, "max_tokens"));
    put(&mut rb, "stream", take(body, "stream"));
    put(&mut rb, "presence_penalty", take(body, "presence_penalty"));
    put(
        &mut rb,
        "frequency_penalty",
        take(body, "frequency_penalty"),
    );
    put(
        &mut rb,
        "repetition_penalty",
        take(body, "repetition_penalty"),
    );
    put(&mut rb, "min_p", take(body, "min_p"));
    put(&mut rb, "top_p", take(body, "top_p"));
    put(&mut rb, "top_k", take(body, "top_k"));
    put(&mut rb, "seed", take(body, "seed"));
    put(&mut rb, "stop", take(body, "stop"));
    put(&mut rb, "reasoning_effort", take(body, "reasoning_effort"));
    put(&mut rb, "logit_bias", take(body, "logit_bias"));
    rb.extend(body_params);

    super::providers::relay_independent(
        super::providers::upstream_post(
            &format!("{}{}/chat/completions", super::providers::API_CHUTES, ""),
            &key,
            Vec::new(),
            &Value::Object(rb),
        )
        .await,
        stream_of(body),
        origin,
        super::providers::OutShape::Pass,
    )
    .await
}

/* ---------------------------------------------------------------- *
 * MiniMax（OpenAI 兼容 + MERGE_TOOLS 合并 + M2-her 上限钳制 + CN 端点分流）
 * ---------------------------------------------------------------- */
pub async fn send_minimax(
    st: &BackendState,
    body: &Value,
    origin: Option<HeaderValue>,
) -> Response {
    let api_root = if body.get("minimax_endpoint").and_then(|v| v.as_str()) == Some("cn") {
        super::providers::API_MINIMAX_CN
    } else {
        super::providers::API_MINIMAX
    };
    let key = super::providers::read_secret(st, "api_key_minimax").unwrap_or_default();
    if key.is_empty() {
        return super::providers::json_resp(
            StatusCode::BAD_REQUEST,
            json!({ "error": true }),
            origin,
        );
    }

    let messages = conv::post_process_prompt(
        &Value::Array(take_array_else_empty(body, "messages")),
        "merge_tools",
        &names_of(body),
    );

    let mut body_params = Map::new();
    tools_into(&mut body_params, body);

    let mut rb = Map::new();
    rb.insert("messages".into(), messages);
    put(&mut rb, "model", take(body, "model"));
    put(&mut rb, "temperature", take(body, "temperature"));
    // M2-her 模型 max_tokens 钳制 2048（ST 字面：max_tokens = model === 'M2-her' ? min(maxTokens, 2048) : maxTokens）
    if let Some(mut t) = take(body, "max_tokens") {
        if body.get("model").and_then(|m| m.as_str()) == Some("M2-her") {
            if let Some(n) = t.as_i64() {
                t = json!(n.min(2048));
            }
        }
        rb.insert("max_tokens".into(), t);
    }
    put(&mut rb, "stream", take(body, "stream"));
    put(&mut rb, "top_p", take(body, "top_p"));
    put(&mut rb, "stop", take(body, "stop"));
    rb.extend(body_params);

    super::providers::relay_independent(
        super::providers::upstream_post(
            &format!("{api_root}/chat/completions"),
            &key,
            Vec::new(),
            &Value::Object(rb),
        )
        .await,
        stream_of(body),
        origin,
        super::providers::OutShape::Pass,
    )
    .await
}

/* ---------------------------------------------------------------- *
 * Azure OpenAI（deployments URL + api-key 头 + OPENAI keys 子集直传）
 * ---------------------------------------------------------------- */
pub async fn send_azure(st: &BackendState, body: &Value, origin: Option<HeaderValue>) -> Response {
    let azure_base_url = body
        .get("azure_base_url")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let deployment = body
        .get("azure_deployment_name")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let api_version = body
        .get("azure_api_version")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let key = super::providers::read_secret(st, "api_key_azure_openai").unwrap_or_default();
    if azure_base_url.is_empty()
        || deployment.is_empty()
        || api_version.is_empty()
        || key.is_empty()
    {
        return super::providers::json_resp(
            StatusCode::BAD_REQUEST,
            json!({ "error": { "message": "Azure OpenAI configuration is incomplete. Please provide Base URL, Deployment Name, API Version, and API Key in the connection settings." } }),
            origin,
        );
    }

    let url = format!(
        "{}/openai/deployments/{deployment}/chat/completions?api-version={api_version}",
        azure_base_url.trim_end_matches('/')
    );

    let mut rb = Map::new();
    for k in [
        "messages",
        "temperature",
        "frequency_penalty",
        "presence_penalty",
        "top_p",
        "max_tokens",
        "max_completion_tokens",
        "stream",
        "logit_bias",
        "stop",
        "n",
        "logprobs",
        "seed",
        "tools",
        "tool_choice",
        "reasoning_effort",
    ] {
        put(&mut rb, k, take(body, k));
    }
    // logprobs 语义对齐（top_logprobs/logprobs 双写）
    if rb
        .get("logprobs")
        .map(|v| v.as_i64().unwrap_or(0) > 0)
        .unwrap_or(false)
    {
        let n = rb["logprobs"].as_i64().unwrap();
        rb.insert("top_logprobs".into(), json!(n));
        rb.insert("logprobs".into(), Value::Bool(true));
    }
    // reasoning_effort 白名单过滤（ST 同表）
    let model = body.get("model").and_then(|m| m.as_str()).unwrap_or("");
    if !super::providers_text::is_openai_reasoning_effort_model(model) {
        rb.remove("reasoning_effort");
    }

    let resp_res = super::providers::upstream_post_azure(&url, &key, &Value::Object(rb)).await;

    let resp = match resp_res {
        Ok(r) => r,
        Err(e) => {
            return super::providers::json_resp(
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({ "error": { "message": e } }),
                origin,
            )
        }
    };

    if stream_of(body) {
        return super::providers::passthrough(resp, origin).await;
    }
    let status_ok = resp.status().is_success();
    let text = resp.text().await.unwrap_or_default();
    if status_ok {
        let v = serde_json::from_str::<Value>(&text).unwrap_or(json!({}));
        return super::providers::json_resp(StatusCode::OK, v, origin);
    }
    let parsed = serde_json::from_str::<Value>(&text)
        .unwrap_or_else(|_| json!({ "error": { "message": "Unknown error occurred" } }));
    super::providers::json_resp(StatusCode::INTERNAL_SERVER_ERROR, parsed, origin)
}

/* ---------------------------------------------------------------- *
 * Cohere（v2 原生协议请求面 ST 字面 + **输出面转 OpenAI 形**）
 * ---------------------------------------------------------------- */
pub async fn send_cohere(st: &BackendState, body: &Value, origin: Option<HeaderValue>) -> Response {
    let key = super::providers::read_secret(st, "api_key_cohere").unwrap_or_default();
    if key.is_empty() {
        return super::providers::json_resp(
            StatusCode::BAD_REQUEST,
            json!({ "error": true }),
            origin,
        );
    }

    let mut msgs = take_array_else_empty(body, "messages");
    let message = conv::convert_cohere_messages(&mut msgs, &names_of(body));

    let mut tools: Vec<Value> = Vec::new();
    if let Some(list) = body.get("tools").and_then(|t| t.as_array()) {
        for mut tool in list.clone() {
            if let Some(p) = tool
                .get_mut("function")
                .and_then(|f| f.get_mut("parameters"))
            {
                if let Some(pm) = p.as_object_mut() {
                    pm.remove("$schema");
                }
            }
            tools.push(tool);
        }
    }

    let mut rb = Map::new();
    rb.insert("stream".into(), json!(stream_of(body)));
    put(&mut rb, "model", take(body, "model"));
    rb.insert("messages".into(), message);
    put(&mut rb, "temperature", take(body, "temperature"));
    put(&mut rb, "max_tokens", take(body, "max_tokens"));
    put(&mut rb, "k", take(body, "top_k"));
    put(&mut rb, "p", take(body, "top_p"));
    put(&mut rb, "seed", take(body, "seed"));
    put(&mut rb, "stop_sequences", take(body, "stop"));
    put(
        &mut rb,
        "frequency_penalty",
        take(body, "frequency_penalty"),
    );
    put(&mut rb, "presence_penalty", take(body, "presence_penalty"));
    rb.insert("documents".into(), json!([]));
    rb.insert("tools".into(), Value::Array(tools));
    // safety_mode 仅 08-2024 后缀模型（ST 字面）
    if body
        .get("model")
        .and_then(|m| m.as_str())
        .map(|m| m.ends_with("08-2024"))
        .unwrap_or(false)
    {
        rb.insert("safety_mode".into(), json!("OFF"));
    }
    if body
        .get("json_schema")
        .map(|j| j.is_object())
        .unwrap_or(false)
    {
        rb.insert(
            "response_format".into(),
            json!({ "type": "json_schema", "schema": body["json_schema"]["value"].clone() }),
        );
    }

    let url = format!("{}{}/chat", super::providers::API_COHERE_V2, "");

    let resp_res =
        super::providers::upstream_post(&url, &key, Vec::new(), &Value::Object(rb)).await;
    super::providers::relay_independent(
        resp_res,
        stream_of(body),
        origin,
        super::providers::OutShape::Cohere,
    )
    .await
}

/* ================= 共享小件 ================= */

fn stream_of(body: &Value) -> bool {
    body.get("stream")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

fn put(rb: &mut Map<String, Value>, key: &str, value: Option<Value>) {
    if let Some(v) = value {
        rb.insert(key.to_string(), v);
    }
}

fn take_array_else_empty(body: &Value, key: &str) -> Vec<Value> {
    body.get(key)
        .and_then(|m| m.as_array())
        .cloned()
        .unwrap_or_default()
}

fn logprobs_into(rb: &mut Map<String, Value>, body: &Value) {
    if body
        .get("logprobs")
        .and_then(|v| v.as_i64())
        .map(|n| n > 0)
        .unwrap_or(false)
    {
        let n = body["logprobs"].as_i64().unwrap();
        rb.insert("top_logprobs".into(), json!(n));
        rb.insert("logprobs".into(), Value::Bool(true));
    }
}

fn tools_into(rb: &mut Map<String, Value>, body: &Value) {
    if body
        .get("tools")
        .and_then(|t| t.as_array())
        .map(|a| !a.is_empty())
        .unwrap_or(false)
    {
        rb.insert("tools".into(), body["tools"].clone());
        if let Some(tc) = body.get("tool_choice") {
            rb.insert("tool_choice".into(), tc.clone());
        }
    }
}

/// json_schema → response_format（openai 形包装；差异点：description 字段开关）
fn json_schema_response_format(
    rb: &mut Map<String, Value>,
    body: &Value,
    with_description: bool,
    _strict_default_true: bool,
) {
    let Some(schema) = body.get("json_schema").filter(|j| j.is_object()) else {
        return;
    };
    let mut inner = Map::new();
    inner.insert(
        "name".into(),
        schema.get("name").cloned().unwrap_or(json!("")),
    );
    if with_description {
        inner.insert(
            "description".into(),
            schema.get("description").cloned().unwrap_or(json!("")),
        );
    }
    inner.insert(
        "strict".into(),
        schema
            .get("strict")
            .and_then(|s| s.as_bool())
            .unwrap_or(true)
            .into(),
    );
    inner.insert(
        "schema".into(),
        schema.get("value").cloned().unwrap_or(json!({})),
    );
    rb.insert(
        "response_format".into(),
        json!({ "type": "json_schema", "json_schema": Value::Object(inner) }),
    );
}
