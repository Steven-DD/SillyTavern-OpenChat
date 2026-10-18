//! backend/providers_google —— MakerSuite（Google AI Studio）/ VertexAI-express
//! sender（Phase 2 批5）。
//!
//! ST `sendMakerSuiteRequest`（chat-completions.js:454-793）逐行核对：
//! - generationConfig 组装（stopSequences/candidateCount/maxOutputTokens/
//!   temperature/topP/topK||undefined/responseMimeType+Schema/seed）
//! - noSamplingModel（gemini-3.7/3.6-flash、3.5-flash-lite）删除采样三件套
//! - isThinkingConfigModel（gemini-2.5-flash|pro 非 image 档、gemini-3*）
//!   → thinkingConfig(thinkingBudget/thinkingLevel/includeThoughts；Vertex
//!   禁混合分支保留)
//! - convertGooglePrompt（convert 模块）；useSysPrompt 面随请求字段
//! - safetySettings = GEMINI_SAFETY + (vertexai → VERTEX_SAFETY)
//! - URL：v1beta `/models/{model}:generateContent[?alt=sse]`；vertexai
//!   express 模式 key 查询串 + 项目路径分支
//!
//! 输出面：流式 = Google 原生 SSE 原样透传（App stStream 的 candidates
//! 解析器直接可读，零转换）；非流式 = ST 包装（candidates 检查/
//! promptFeedback.blockReason/resp Text join '\n\n'/thought 过滤）。
//!
//! 偏差登记：vertexai **full 模式**（服务账号 JWT→OAuth2 换 token）在
//! Phase 2 尾批实装（当前 express 模式全量可用；full 请求返回 400 可判定
//! 错误，非静默）。enable_web_search / request_images /
//! responseMimeType 明面字段 ST 都有分支，App 不发送 → 保留了 tools/
//! json_schema 分支以防直连。

use super::providers_convert as conv;
use super::BackendState;
use axum::http::{HeaderValue, StatusCode};
use axum::response::Response;
use serde_json::{json, Map, Value};

const NO_SEARCH_MODELS: &[&str] = &[
    "gemini-2.0-flash-lite",
    "gemini-2.0-flash-lite-001",
    "gemini-2.0-flash-lite-preview-02-05",
    "gemini-robotics-er-1.5-preview",
];

fn gemini_safety() -> Value {
    json!([
        { "category": "HARM_CATEGORY_HARASSMENT", "threshold": "OFF" },
        { "category": "HARM_CATEGORY_HATE_SPEECH", "threshold": "OFF" },
        { "category": "HARM_CATEGORY_SEXUALLY_EXPLICIT", "threshold": "OFF" },
        { "category": "HARM_CATEGORY_DANGEROUS_CONTENT", "threshold": "OFF" },
        { "category": "HARM_CATEGORY_CIVIC_INTEGRITY", "threshold": "OFF" },
    ])
}

fn vertex_safety() -> Vec<Value> {
    serde_json::from_value(json!([
        { "category": "HARM_CATEGORY_IMAGE_HATE", "threshold": "OFF" },
        { "category": "HARM_CATEGORY_IMAGE_DANGEROUS_CONTENT", "threshold": "OFF" },
        { "category": "HARM_CATEGORY_IMAGE_HARASSMENT", "threshold": "OFF" },
        { "category": "HARM_CATEGORY_IMAGE_SEXUALLY_EXPLICIT", "threshold": "OFF" },
        { "category": "HARM_CATEGORY_JAILBREAK", "threshold": "OFF" },
    ]))
    .unwrap_or_default()
}

pub async fn send_makersuite(
    st: &BackendState,
    body: &Value,
    origin: Option<HeaderValue>,
) -> Response {
    super::providers::log_outbound(
        body.get("chat_completion_source")
            .and_then(|v| v.as_str())
            .unwrap_or("makersuite"),
        body,
    );
    let is_vertex = body.get("chat_completion_source").and_then(|v| v.as_str()) == Some("vertexai");
    let api_name = if is_vertex {
        "Google Vertex AI"
    } else {
        "Google AI Studio"
    };

    let mut auth_url = String::new();
    // full 模式（服务账号）授权产物：Bearer token + SA project_id
    let mut vertex_full: Option<(String, String)> = None;
    let api_key: String = if is_vertex {
        // express（API key）/ full（服务账号 JWT→OAuth2）
        let auth_mode = body
            .get("vertexai_auth_mode")
            .and_then(|v| v.as_str())
            .unwrap_or("express");
        if auth_mode == "full" {
            let sa_json = super::providers::read_secret(st, "vertexai_service_account_json")
                .filter(|s| !s.is_empty());
            let Some(sa_json) = sa_json else {
                return super::providers::json_resp(
                    StatusCode::BAD_REQUEST,
                    json!({ "error": { "message": format!("{api_name} 服务账号 JSON 缺失（full 模式需在密钥面保存 vertexai_service_account_json）") } }),
                    origin,
                );
            };
            match super::vertexai_auth::authorize_full(&sa_json).await {
                Ok((token, project_id, _)) => {
                    vertex_full = Some((token, project_id));
                    String::new() // full 走 Bearer 头，不再用 query key
                }
                Err(e) => {
                    // ST auth 失败字面：400 {error:true,message}
                    return super::providers::json_resp(
                        StatusCode::BAD_REQUEST,
                        json!({ "error": { "message": e } }),
                        origin,
                    );
                }
            }
        } else {
            let k = super::providers::read_secret(st, "api_key_vertexai").unwrap_or_default();
            if k.is_empty() {
                return super::providers::json_resp(
                    StatusCode::BAD_REQUEST,
                    json!({ "error": { "message": format!("{api_name} API key 缺失") } }),
                    origin,
                );
            }
            k
        }
    } else {
        let use_rp = body
            .get("reverse_proxy")
            .and_then(|v| v.as_str())
            .map(|s| !s.is_empty())
            .unwrap_or(false);
        let url = if use_rp {
            // HIGH-5：reverse_proxy 出站网关（ProxyUrl 面）
            match super::providers::validate_egress_url(
                body.get("reverse_proxy")
                    .and_then(|v| v.as_str())
                    .unwrap_or(""),
                super::providers::EgressPolicy::ProxyUrl,
            ) {
                Ok(u) => u,
                Err(e) => {
                    return super::providers::json_resp(
                        StatusCode::BAD_REQUEST,
                        json!({ "error": { "message": format!("reverse_proxy 被出站网关拒绝: {e}") } }),
                        origin,
                    )
                }
            }
        } else {
            super::providers::API_MAKERSUITE.to_string()
        };
        auth_url = url;
        let k = if use_rp {
            body.get("proxy_password")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        } else {
            super::providers::read_secret(st, "api_key_makersuite").unwrap_or_default()
        };
        if k.is_empty() {
            // ST 字面：缺 key → 400 {error:true}
            return super::providers::json_resp(
                StatusCode::BAD_REQUEST,
                json!({ "error": true }),
                origin,
            );
        }
        k
    };

    let model = body
        .get("model")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let model = model.as_str();
    let stream = body
        .get("stream")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let is_gemma3 = model.contains("gemma-3");
    let is_learnlm = model.contains("learnlm");
    let reasoning_effort = body.get("reasoning_effort").and_then(|v| v.as_str());
    let include_reasoning = body
        .get("include_reasoning")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // generationConfig
    let mut gc = Map::new();
    if let Some(stop) = body
        .get("stop")
        .and_then(|s| s.as_array())
        .filter(|a| !a.is_empty())
    {
        gc.insert("stopSequences".into(), Value::Array(stop.clone()));
    }
    gc.insert("candidateCount".into(), json!(1));
    gc.insert(
        "maxOutputTokens".into(),
        body.get("max_tokens").cloned().unwrap_or(Value::Null),
    );
    gc.insert(
        "temperature".into(),
        body.get("temperature").cloned().unwrap_or(Value::Null),
    );
    gc.insert(
        "topP".into(),
        body.get("top_p").cloned().unwrap_or(Value::Null),
    );
    // topK: 0 或缺失 → undefined（ST `top_undefined||` 语义）
    match body.get("top_k").and_then(|v| v.as_f64()) {
        Some(k) if k != 0.0 => gc.insert("topK".into(), json!(k)),
        _ => None::<Value>,
    };
    gc.insert(
        "seed".into(),
        body.get("seed").cloned().unwrap_or(Value::Null),
    );

    let enable_image_modality = false; // request_images App 不发（ST 需要 true 才启用）
    let _ = enable_image_modality;

    // noSamplingModel：/gemini-3\.[67]-flash|gemini-3\.5-flash-lite/
    let no_sampling = model.contains("gemini-3.7-flash")
        || model.contains("gemini-3.6-flash")
        || model.contains("gemini-3.5-flash-lite");
    if no_sampling {
        gc.remove("temperature");
        gc.remove("topP");
        gc.remove("topK");
        gc.remove("candidateCount");
    }

    // thinkingConfig
    let is_thinking_config = is_thinking_config_model(model);
    if is_thinking_config {
        let mut tc = json!({ "includeThoughts": include_reasoning });
        if let Some(b) = conv::calculate_google_budget_tokens(
            body.get("max_tokens")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0),
            reasoning_effort,
            model,
        ) {
            if let Some(n) = b.as_i64() {
                if b == json!(0) && is_vertex && include_reasoning {
                    // Vertex：budget 0 与 includeThoughts 混用被禁
                    tc.as_object_mut()
                        .unwrap()
                        .insert("includeThoughts".into(), json!(false));
                }
                tc.as_object_mut()
                    .unwrap()
                    .insert("thinkingBudget".into(), json!(n));
            } else if let Some(s) = b.as_str() {
                tc.as_object_mut()
                    .unwrap()
                    .insert("thinkingLevel".into(), json!(s));
            }
        }
        gc.insert("thinkingConfig".into(), tc);
    }

    // prompt 转换
    let use_sys_prompt = body
        .get("use_sysprompt")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
        && !enable_image_modality
        && !is_gemma3;
    let use_sys_prompt_for_convert = body
        .get("use_sysprompt")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let mut msgs = body
        .get("messages")
        .and_then(|m| m.as_array())
        .cloned()
        .unwrap_or_default();
    let (contents, system_instruction) = conv::convert_google_prompt(
        &mut msgs,
        model,
        use_sys_prompt_for_convert,
        &conv::PromptNames::from_body(body),
    );

    let mut safety: Vec<Value> = Vec::new();
    if let Some(a) = gemini_safety().as_array() {
        safety.extend(a.clone());
    }
    if is_vertex {
        safety.extend(vertex_safety());
    }

    let mut rb = Map::new();
    rb.insert("contents".into(), contents);
    rb.insert("safetySettings".into(), Value::Array(safety));
    rb.insert("generationConfig".into(), Value::Object(gc));

    if use_sys_prompt {
        let parts = system_instruction
            .get("parts")
            .and_then(|p| p.as_array())
            .cloned()
            .unwrap_or_default();
        if !parts.is_empty() {
            rb.insert("systemInstruction".into(), system_instruction);
        }
    }

    // tools / web search（保留 ST 分支；App 常明面都是空）
    let mut tools: Vec<Value> = Vec::new();
    if let Some(list) = body
        .get("tools")
        .and_then(|t| t.as_array())
        .filter(|a| !a.is_empty())
    {
        // tools 分支复用（App 不发，保持 ST 语义以便直连）
        let use_tools = !enable_image_modality && !is_gemma3;
        if use_tools {
            let mut declarations: Vec<Value> = Vec::new();
            let mut custom: Vec<Value> = Vec::new();
            for tool in list {
                if tool.get("type").and_then(|t| t.as_str()) == Some("function") {
                    let mut fnc = tool.get("function").cloned().unwrap_or(json!({}));
                    if let Some(p) = fnc.get_mut("parameters") {
                        if let Some(pm) = p.as_object_mut() {
                            pm.remove("$schema");
                            if pm
                                .get("properties")
                                .map(|props| {
                                    props.as_object().map(|o| o.is_empty()).unwrap_or(false)
                                })
                                .unwrap_or(false)
                            {
                                fnc.as_object_mut().unwrap().remove("parameters");
                            }
                        }
                    }
                    declarations.push(fnc);
                } else if let Some(t) =
                    tool.get(tool.get("type").and_then(|x| x.as_str()).unwrap_or(""))
                {
                    let ttype = tool
                        .get("type")
                        .and_then(|x| x.as_str())
                        .unwrap_or("")
                        .to_string();
                    let mut o = Map::new();
                    o.insert(ttype, t.clone());
                    custom.push(Value::Object(o));
                }
            }
            if !declarations.is_empty() {
                tools.push(json!({ "function_declarations": declarations }));
            }
            if declarations.is_empty() && !custom.is_empty() {
                tools.extend(custom);
            }
        }
    }
    if body
        .get("enable_web_search")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
        && !enable_image_modality
        && !is_gemma3
        && !is_learnlm
        && !NO_SEARCH_MODELS.contains(&model)
        && !tools
            .iter()
            .any(|t| t.get("function_declarations").is_some())
    {
        tools.push(json!({ "google_search": {} }));
    }
    if !tools.is_empty() {
        rb.insert("tools".into(), Value::Array(tools));
    }

    // URL
    let response_type = if stream {
        "streamGenerateContent"
    } else {
        "generateContent"
    };
    // 请求额外头：full 模式带 Bearer token（express 仍是 query key，无头）
    let mut extra_headers: Vec<(String, String)> = Vec::new();
    let url = if is_vertex {
        let region = body
            .get("vertexai_region")
            .and_then(|v| v.as_str())
            .unwrap_or("us-central1");
        if let Some((token, project_id)) = &vertex_full {
            // full 模式（ST :716-722 字面：Bearer 头 + SA project_id URL）
            extra_headers.push(("Authorization".into(), format!("Bearer {token}")));
            super::vertexai_auth::full_mode_url(region, project_id, model, response_type, stream)
        } else {
            // express 模式（key 查询串）；项目/区域分支按 ST 字面
            let project = body
                .get("vertexai_express_project_id")
                .and_then(|v| v.as_str());
            match project {
                Some(pid) => format!(
                    "https://aiplatform.googleapis.com/v1/projects/{pid}/locations/{region}/publishers/google/models/{model}:{response_type}?key={api_key}{}",
                    if stream { "&alt=sse" } else { "" }
                ),
                None => {
                    let base = if region == "global" {
                        "https://aiplatform.googleapis.com".to_string()
                    } else {
                        format!("https://{region}-aiplatform.googleapis.com")
                    };
                    format!(
                        "{base}/v1/publishers/google/models/{model}:{response_type}?key={api_key}{}",
                        if stream { "&alt=sse" } else { "" }
                    )
                }
            }
        }
    } else {
        format!(
            "{}/v1beta/models/{model}:{response_type}?key={api_key}{}",
            auth_url.trim_end_matches('/'),
            if stream { "&alt=sse" } else { "" }
        )
    };

    // 请求发出（full 模式走 Bearer 头；express/Google AI 仍 query key）
    let resp_res =
        super::providers::upstream_post(&url, "", extra_headers, &Value::Object(rb)).await;
    let resp = match resp_res {
        Ok(r) => r,
        Err(_) => {
            return super::providers::json_resp(
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({ "error": true }),
                origin,
            )
        }
    };

    if stream {
        return super::providers::passthrough(resp, origin).await;
    }

    let status_ok = resp.status().is_success();
    let text = resp.text().await.unwrap_or_default();
    if !status_ok {
        // tryParse ?? {error:true}
        let parsed = serde_json::from_str::<Value>(&text).unwrap_or(json!({ "error": true }));
        return super::providers::json_resp(StatusCode::INTERNAL_SERVER_ERROR, parsed, origin);
    }
    let v = serde_json::from_str::<Value>(&text).unwrap_or(json!({}));
    super::providers::json_resp(
        StatusCode::OK,
        wrap_makersuite_response(v, api_name),
        origin,
    )
}

/// ST 包装（:758-786）：candidates 检查 / thought 过滤 / join '\n\n'
pub fn wrap_makersuite_response(v: Value, api_name: &str) -> Value {
    let candidates_empty = v
        .get("candidates")
        .map(|c| c.as_array().map(|a| a.is_empty()).unwrap_or(true))
        .unwrap_or(true);
    if candidates_empty {
        let mut message = format!("{api_name} API returned no candidate");
        if let Some(br) = v
            .pointer("/promptFeedback/blockReason")
            .and_then(|b| b.as_str())
        {
            message = format!("{message}\nPrompt was blocked due to : {br}");
        }
        return json!({ "error": { "message": message } });
    }
    let response_content = v
        .pointer("/candidates/0/content")
        .or_else(|| v.pointer("/candidates/0/output"))
        .cloned()
        .unwrap_or(Value::Null);
    let parts = response_content
        .get("parts")
        .and_then(|p| p.as_array())
        .cloned()
        .unwrap_or_default();
    let function_call = parts.iter().any(|p| p.get("functionCall").is_some());
    let inline_data = parts.iter().any(|p| p.get("inlineData").is_some());
    let is_string = response_content.is_string();
    let response_text = if is_string {
        response_content.as_str().unwrap_or("").to_string()
    } else {
        let joined: Vec<&str> = parts
            .iter()
            .filter(|p| !p.get("thought").and_then(|t| t.as_bool()).unwrap_or(false))
            .filter_map(|p| p.get("text"))
            .filter_map(|t| t.as_str())
            .collect();
        joined.join("\n\n")
    };
    if response_text.is_empty() && !function_call && !inline_data {
        return json!({ "error": { "message": format!("{api_name} Candidate text empty") } });
    }
    json!({
        "choices": [{ "message": { "content": response_text } }],
        "responseContent": response_content,
    })
}

/* ------------------------------------------------------------ *
 * /status（makersuite 模型发现 + vertexai ST 字面 400）
 * ------------------------------------------------------------ */

const GOOGLE_MODELS_PAGE_SIZE: &str = "1000";

/// POST /api/backends/chat-completions/status 的 google 分支（chat-completions.js:1908-1933）
pub(crate) async fn status(
    st: &BackendState,
    body: &Value,
    origin: Option<HeaderValue>,
) -> Response {
    let is_vertex = body.get("chat_completion_source").and_then(|v| v.as_str()) == Some("vertexai");
    if is_vertex {
        // ST 1.19 /status 无 vertexai 分支（前端走静态模型下拉）→ 字面 else 400
        return super::providers::json_resp(
            StatusCode::BAD_REQUEST,
            json!({ "error": true }),
            origin,
        );
    }

    let use_rp = body
        .get("reverse_proxy")
        .and_then(|v| v.as_str())
        .map(|s| !s.is_empty())
        .unwrap_or(false);
    let api_key = if use_rp {
        body.get("proxy_password")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    } else {
        super::providers::read_secret(st, "api_key_makersuite").unwrap_or_default()
    };
    if api_key.is_empty() && !use_rp {
        // ST 字面：缺 key → 400 {error:true}
        return super::providers::json_resp(
            StatusCode::BAD_REQUEST,
            json!({ "error": true }),
            origin,
        );
    }
    let api_root = match body
        .get("reverse_proxy")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
    {
        // HIGH-5：reverse_proxy 出站网关（ProxyUrl 面）
        Some(rp) => match super::providers::validate_egress_url(
            rp,
            super::providers::EgressPolicy::ProxyUrl,
        ) {
            Ok(u) => u,
            Err(_) => {
                return super::providers::json_resp(
                    StatusCode::BAD_REQUEST,
                    json!({ "error": true }),
                    origin,
                )
            }
        },
        None => super::providers::API_MAKERSUITE.to_string(),
    };
    let api_root = api_root.trim_end_matches('/').to_string();
    let models_url = if api_key.is_empty() {
        // rp 路径：不带 key 查询串（ST 字面）
        format!("{api_root}/v1beta/models")
    } else {
        format!("{api_root}/v1beta/models?key={api_key}")
    };

    match fetch_google_models(&models_url).await {
        Ok(models) => {
            super::providers::json_resp(StatusCode::OK, json!({ "data": models }), origin)
        }
        Err(_) => {
            // GoogleModelsHttpError/ResponseError 统一 → {error:true,bypass:true}（api.ts:533）
            super::providers::json_resp(
                StatusCode::OK,
                json!({ "error": true, "bypass": true, "data": { "data": [] } }),
                origin,
            )
        }
    }
}

/// fetchGoogleModels（google-models.js:36-99）：分页 pageSize=1000 +
/// supportedGenerationMethods 过滤 + id = name 去掉 models/ 前缀
pub(crate) async fn fetch_google_models(models_url: &str) -> Result<Vec<Value>, String> {
    let client = reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap_or_default();
    let base_sep = if models_url.contains('?') { '&' } else { '?' };
    let mut url = format!("{models_url}{base_sep}pageSize={GOOGLE_MODELS_PAGE_SIZE}");
    let mut page_tokens: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut models: Vec<Value> = Vec::new();

    loop {
        let resp = client
            .get(&url)
            .header("accept-encoding", "identity")
            .timeout(std::time::Duration::from_secs(60))
            .send()
            .await
            .map_err(|e| {
                format!(
                    "Failed to fetch models page: {}",
                    super::providers::fmt_upstream_err(&e)
                )
            })?;
        if !resp.status().is_success() {
            return Err(format!("Google models endpoint returned {}", resp.status()));
        }
        let data = resp
            .json::<Value>()
            .await
            .map_err(|_| "Failed to parse Google AI Studio models page.".to_string())?;
        let page = data
            .get("models")
            .and_then(|m| m.as_array())
            .cloned()
            .ok_or_else(|| {
                "Google AI Studio models page has an invalid models field.".to_string()
            })?;
        models.extend(page);

        match data.get("nextPageToken").and_then(|t| t.as_str()) {
            None => break,
            Some("") => break,
            Some(token) => {
                if page_tokens.contains(token) {
                    return Err("pagination repeated token".into());
                }
                page_tokens.insert(token.to_string());
                url = format!(
                    "{models_url}{base_sep}pageSize={GOOGLE_MODELS_PAGE_SIZE}&pageToken={token}"
                );
            }
        }
    }

    let out: Vec<Value> = models
        .into_iter()
        .filter(|m| {
            let methods: Option<&Vec<Value>> = m
                .get("supportedGenerationMethods")
                .and_then(|x| x.as_array());
            methods
                .map(|a| a.iter().any(|x| x.as_str() == Some("generateContent")))
                .unwrap_or(false)
        })
        .map(|m| {
            let mut o = m;
            if let Some(name) = o.get("name").and_then(|n| n.as_str()).map(String::from) {
                o.as_object_mut()
                    .unwrap()
                    .insert("id".into(), json!(name.replace("models/", "")));
            }
            o
        })
        .collect();
    Ok(out)
}

/* ---------------- 字面判别 ---------------- */

fn starts_with_prefixes(model: &str, prefixes: &[&str]) -> bool {
    prefixes.iter().any(|p| model.starts_with(p))
}

/// isThinkingConfigModel（ST :526 字面）：
/// `/^gemini-2.5-(flash|pro)/ && !/-image(-preview)?$/` 或
/// `/^gemini-3[.\d]*-(flash|pro)/`
fn is_thinking_config_model(model: &str) -> bool {
    (starts_with_prefixes(model, &["gemini-2.5-flash", "gemini-2.5-pro"])
        && !model.ends_with("-image")
        && !model.ends_with("-image-preview"))
        || starts_with_any_regex3(model)
}

/// ^gemini-3[.\d]*-(flash|pro)（thinking config 档判别；锚定 model 起始，
/// 不再子串命中 —— ST 同款正则本就 ^ 锚定）
fn starts_with_any_regex3(model: &str) -> bool {
    let rest = match model.strip_prefix("gemini-3") {
        Some(rest) => rest,
        None => return false,
    };
    let mut idx = 0usize;
    for b in rest.bytes() {
        if b == b'.' || b.is_ascii_digit() {
            idx += 1;
        } else {
            break;
        }
    }
    (rest[idx..].starts_with("-flash") || rest[idx..].starts_with("-pro"))
        && !(model.ends_with("-image") || model.ends_with("-image-preview"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_sampling_and_thinking_gates() {
        // noSampling 家族（generate 内联 contains 判别）
        assert!("gemini-3.7-flash".contains("gemini-3.7-flash"));
        assert!(!"gemini-2.5-flash".contains("gemini-3.7-flash"));
        // thinkingConfig 档：2.5 前缀族（-image/-image-preview 排除）
        assert!(is_thinking_config_model("gemini-2.5-flash"));
        assert!(is_thinking_config_model("gemini-2.5-pro"));
        assert!(!is_thinking_config_model("gemini-2.0-flash"));
        assert!(!is_thinking_config_model("gemini-2.5-flash-image"));
        assert!(!is_thinking_config_model("gemini-2.5-pro-image-preview"));
        // gemini-3 档：^ 锚定判定（gemini-3 前缀 + [.\d]* 段 + -flash/-pro）
        assert!(is_thinking_config_model("gemini-3-pro"));
        assert!(is_thinking_config_model("gemini-3.7-flash"));
        assert!(is_thinking_config_model("gemini-3.5-flash-lite"));
        // 加锚后：前缀伪装不再命中
        assert!(!is_thinking_config_model("claude-gemini-3-pro"));
        assert!(!is_thinking_config_model("my-gemini-3-flash"));
        // -image / -image-preview 排除（ST !/-image(-preview)?$/）
        assert!(!is_thinking_config_model("gemini-3-flash-image"));
        assert!(!is_thinking_config_model("gemini-3-pro-image-preview"));
        // 非 flash/pro 后缀
        assert!(!is_thinking_config_model("gemini-3-lite"));
    }

    #[test]
    fn wraps_makersuite_candidates() {
        let ok = wrap_makersuite_response(
            serde_json::json!({
                "candidates": [
                    { "content": { "parts": [{ "text": "A" }, { "text": "B" }] } }
                ]
            }),
            "Google AI Studio",
        );
        assert_eq!(ok["choices"][0]["message"]["content"], "A\n\nB");
        assert!(ok.get("responseContent").is_some());

        let blocked = wrap_makersuite_response(
            serde_json::json!({
                "promptFeedback": { "blockReason": "SAFETY" },
            }),
            "Google AI Studio",
        );
        assert!(blocked["error"]["message"]
            .as_str()
            .unwrap()
            .contains("Prompt was blocked"));

        let empty_text = wrap_makersuite_response(
            serde_json::json!({
                "candidates": [{ "content": { "parts": [] } }],
            }),
            "Google AI Studio",
        );
        assert_eq!(
            empty_text["error"]["message"],
            "Google AI Studio Candidate text empty"
        );
    }

    #[test]
    fn wrap_empty_parts_with_thought_filter() {
        let ok = wrap_makersuite_response(
            serde_json::json!({
                "candidates": [{
                    "content": { "parts": [
                        { "text": "t", "thought": true },
                        { "text": "real" },
                    ] },
                }],
            }),
            "Google AI Studio",
        );
        assert_eq!(ok["choices"][0]["message"]["content"], "real");
    }
}
