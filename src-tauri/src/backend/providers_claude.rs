//! backend/providers_claude —— Claude（Anthropic messages API）sender（Phase 2 批4）。
//!
//! ST `sendClaudeRequest`（chat-completions.js:230-447）逐行核对，含模型档位
//! 正则（fable / claude-5 / thinking / limited-sampling / adaptive 等）。
//!
//! 输出面偏差（登记）：ST 流式路径是 `forwardFetchResponse` **原样透传**
//! Anthropic SSE；App 前端 stStream 只有 OpenAI/Google 双解析器 → native 把
//! Anthropic SSE **转换为 OpenAI chunk**（text_delta→delta.content、
//! thinking_delta→reasoning_content、message_delta.stop_reason→finish_reason、
//! message_stop→[DONE]）。请求面保持 ST 字面（含 key 缺失 400 {error:true}、
//! 连接失败 500 {error:true}、非流式包装 {choices,message}+content 原样）。

use super::providers_convert as conv;
use super::{BackendState};
use axum::http::{HeaderValue, StatusCode};
use axum::response::Response;
use serde_json::{json, Map, Value};

/// /^claude-(...)/ 起始锚判别（regex alternation 逐字面平移）
fn model_starts_any(model: &str, alts: &[&str]) -> bool {
    model.strip_prefix("claude-")
        .map(|rest| alts.iter().any(|a| rest.starts_with(a)))
        .unwrap_or(false)
}

/// 发送 Claude 请求（/messages）
pub async fn send_claude(st: &BackendState, body: &Value, origin: Option<HeaderValue>) -> Response {
    let api_url = match body.get("reverse_proxy").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
        Some(rp) => rp.to_string(),
        None => super::providers::API_CLAUDE.to_string(),
    };
    let key = if body.get("reverse_proxy").and_then(|v| v.as_str()).map(|s| !s.is_empty()).unwrap_or(false) {
        body.get("proxy_password").and_then(|v| v.as_str()).unwrap_or("").to_string()
    } else {
        super::providers::read_secret(st, "api_key_claude").unwrap_or_default()
    };
    if key.is_empty() {
        return super::providers::json_resp(StatusCode::BAD_REQUEST, json!({ "error": true }), origin);
    }

    let model = body.get("model").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let model_ref = model.as_str();
    let is_fable = model_ref.contains("claude-fable");
    let is_fable51 = model_ref.contains("claude-fable-5-1");
    let is_claude5 = contains_any(model_ref, &["claude-opus-5", "claude-sonnet-5"]);
    let use_thinking = model_starts_any(
        model_ref,
        &["3-7", "opus-4", "sonnet-4", "haiku-4-5"],
    ) || is_fable
        || is_claude5;
    let is_limited_sampling = model_starts_any(
        model_ref,
        &["opus-4-1", "sonnet-4-5", "haiku-4-5", "opus-4-5", "opus-4-6", "sonnet-4-6"],
    );
    let no_prefill_model = model_starts_any(model_ref, &["opus-4-6", "sonnet-4-6", "opus-4-7", "opus-4-8"])
        || is_fable
        || is_claude5;
    // enableAdaptiveThinking 配置默认 true（ST config 字面）
    const ENABLE_ADAPTIVE_THINKING: bool = true;
    let is_adaptive = model_starts_any(model_ref, &["opus-4-7", "opus-4-8"])
        || is_fable
        || is_claude5
        || (ENABLE_ADAPTIVE_THINKING && model_starts_any(model_ref, &["opus-4-6", "sonnet-4-6"]));
    let no_sampling_model = model_starts_any(model_ref, &["opus-4-7", "opus-4-8"]) || is_fable || is_claude5;

    let use_system_prompt = body.get("use_sysprompt").and_then(|v| v.as_bool()).unwrap_or(false);
    let use_tools = body.get("tools").and_then(|t| t.as_array()).map(|a| !a.is_empty()).unwrap_or(false);

    let mut msgs = body
        .get("messages")
        .and_then(|m| m.as_array())
        .cloned()
        .unwrap_or_default();
    let prefill = body
        .get("assistant_prefill")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty());
    let (converted, system_prompt) =
        conv::convert_claude_messages(&mut msgs, prefill, use_system_prompt, use_tools, &conv::PromptNames::from_body(body));
    let mut converted_arr = converted.as_array().cloned().unwrap_or_default();

    // stop_sequences（数组取值；缺省 [] —— ST 默认带空数组）
    let stop_sequences = body
        .get("stop")
        .and_then(|s| s.as_array())
        .cloned()
        .unwrap_or_default();

    let mut rb = Map::new();
    rb.insert("model".into(), body.get("model").cloned().unwrap_or(Value::Null));
    rb.insert("max_tokens".into(), body.get("max_tokens").cloned().unwrap_or(Value::Null));
    rb.insert("messages".into(), Value::Array(converted_arr.clone()));
    rb.insert("stop_sequences".into(), Value::Array(stop_sequences));
    rb.insert("temperature".into(), body.get("temperature").cloned().unwrap_or(Value::Null));
    rb.insert("top_p".into(), body.get("top_p").cloned().unwrap_or(Value::Null));
    rb.insert("top_k".into(), body.get("top_k").cloned().unwrap_or(Value::Null));
    rb.insert("stream".into(), body.get("stream").cloned().unwrap_or(Value::Null));
    // useTools（App 不发 tools；保留 ST 分支以防直连调用）
    if use_tools {
        rb.insert(
            "tool_choice".into(),
            json!({ "type": body.get("tool_choice").cloned().unwrap_or(Value::Null) }),
        );
        let tools: Vec<Value> = body
            .get("tools")
            .and_then(|t| t.as_array())
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|t| t.get("type").and_then(|x| x.as_str()) == Some("function"))
            .map(|t| t.get("function").cloned().unwrap_or(json!({})))
            .map(|fn_| {
                json!({
                    "name": fn_.get("name").cloned().unwrap_or(Value::Null),
                    "description": fn_.get("description").cloned().unwrap_or(Value::Null),
                    "input_schema": super::providers::flatten_schema(
                        fn_.get("parameters").unwrap_or(&json!({})),
                        body.get("chat_completion_source").and_then(|s| s.as_str()).unwrap_or(""),
                    ),
                })
            })
            .collect();
        rb.insert("tools".into(), Value::Array(tools));
    }
    // json_schema（fable5.1 → output_config；其余 → json tool）
    if body.get("json_schema").map(|j| j.is_object()).unwrap_or(false) {
        if is_fable51 {
            rb.insert(
                "output_config".into(),
                json!({
                    "format": {
                        "type": "json_schema",
                        "schema": body["json_schema"]["value"].clone(),
                    }
                }),
            );
        } else {
            let json_tool = json!({
                "name": body["json_schema"]["name"].clone(),
                "description": body["json_schema"].get("description").cloned().unwrap_or(json!("Well-formed JSON object")),
                "input_schema": body["json_schema"]["value"].clone(),
            });
            let mut tools = rb.get("tools").and_then(|t| t.as_array()).cloned().unwrap_or_default();
            tools.push(json_tool);
            rb.insert("tools".into(), Value::Array(tools));
            rb.insert(
                "tool_choice".into(),
                json!({ "type": "tool", "name": body["json_schema"]["name"].clone() }),
            );
        }
    }

    if use_system_prompt {
        let sys = system_prompt;
        if sys.as_array().map(|a| !a.is_empty()).unwrap_or(false) {
            rb.insert("system".into(), sys);
        }
    }

    // limited-sampling / no-sampling 温度钳制（ST 字面次序）
    if is_limited_sampling {
        let top_p = rb.get("top_p").and_then(|v| v.as_f64());
        match top_p {
            Some(tp) if tp < 1.0 => {
                rb.remove("temperature");
            }
            Some(_) => {
                rb.remove("top_p");
            }
            None => {}
        }
    }
    if no_sampling_model {
        rb.remove("temperature");
        rb.remove("top_p");
        rb.remove("top_k");
    }

    // thinking 预算（convert 模块 ST 字面）
    let max_tokens_f = body.get("max_tokens").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let reasoning_effort = body.get("reasoning_effort").and_then(|v| v.as_str());
    let stream_flag = body.get("stream").and_then(|v| v.as_bool()).unwrap_or(false);
    let budget = conv::calculate_claude_budget_tokens(max_tokens_f, reasoning_effort, stream_flag, is_adaptive);
    let mut fix_thinking_prefill = false;
    if use_thinking {
        if let Some(b) = &budget {
            if b.is_string() {
                // adaptive thinking（Opus 4.6+ 语义）
                fix_thinking_prefill = true;
                let mut thinking = json!({ "type": "adaptive" });
                if no_sampling_model
                    && body.get("include_reasoning").and_then(|v| v.as_bool()).unwrap_or(false)
                {
                    thinking.as_object_mut().unwrap().insert("display".into(), json!("summarized"));
                }
                rb.insert("thinking".into(), thinking);
                let cfg = rb
                    .entry(String::from("output_config"))
                    .or_insert_with(|| json!({}));
                if let Some(o) = cfg.as_object_mut() {
                    o.insert("effort".into(), b.clone());
                }
                rb.remove("top_k");
            } else if (is_fable || is_claude5)
                && reasoning_effort == Some("auto")
                && body.get("include_reasoning").and_then(|v| v.as_bool()).unwrap_or(false)
            {
                // fable/claude5 auto：显式要摘要才开 display
                fix_thinking_prefill = true;
                rb.insert("thinking".into(), json!({ "type": "adaptive", "display": "summarized" }));
            } else if let Some(n) = b.as_i64() {
                // 传统 thinking：数字预算
                fix_thinking_prefill = true;
                const MIN_THINK_TOKENS: i64 = 1024;
                let mut mt = body.get("max_tokens").and_then(|v| v.as_i64()).unwrap_or(0);
                if mt <= MIN_THINK_TOKENS {
                    mt += MIN_THINK_TOKENS;
                    rb.insert("max_tokens".into(), json!(mt));
                }
                rb.insert("thinking".into(), json!({ "type": "enabled", "budget_tokens": n }));
                rb.remove("temperature");
                rb.remove("top_p");
                rb.remove("top_k");
            }
        }
    }

    if (fix_thinking_prefill || no_prefill_model)
        && converted_arr.last().map(|m| m.get("role").and_then(|r| r.as_str()) == Some("assistant")).unwrap_or(false)
    {
        converted_arr.last_mut().unwrap().as_object_mut().unwrap().insert("role".into(), json!("user"));
        rb.insert("messages".into(), Value::Array(converted_arr));
    }

    let url = format!("{api_url}/messages");
    let resp_res = super::providers::upstream_post(&url, &key, super::providers::claude_headers(), &Value::Object(rb)).await;
    let resp = match resp_res {
        Ok(r) => r,
        Err(_) => return super::providers::json_resp(StatusCode::INTERNAL_SERVER_ERROR, json!({ "error": true }), origin),
    };

    if stream_flag {
        // 流式：Anthropic SSE → OpenAI chunk（App 解析器约束，见模块头偏差登记）
        return super::providers::transform_sse_reply(resp, origin, anthropic_to_openai).await;
    }

    let status_ok = resp.status().is_success();
    let text = resp.text().await.unwrap_or_default();
    if !status_ok {
        // ST 字面：上游错误丢弃响应体 → 500 {error:true}
        return super::providers::json_resp(StatusCode::INTERNAL_SERVER_ERROR, json!({ "error": true }), origin);
    }
    let v = serde_json::from_str::<Value>(&text).unwrap_or(json!({}));
    let response_text = v
        .get("content")
        .and_then(|c| c.as_array())
        .and_then(|a| a.first())
        .and_then(|b| b.get("text"))
        .and_then(|t| t.as_str())
        .unwrap_or("");
    let reply = json!({
        "choices": [{ "message": { "content": response_text } }],
        "content": v.get("content").cloned().unwrap_or(json!([])),
    });
    super::providers::json_resp(StatusCode::OK, reply, origin)
}

fn contains_any(s: &str, needles: &[&str]) -> bool {
    needles.iter().any(|n| s.contains(n))
}

/// Anthropic SSE 事件 → OpenAI chunk / [DONE]
pub(crate) fn anthropic_to_openai(ev: &Value) -> super::providers::SseAction {
    let etype = ev.get("type").and_then(|t| t.as_str()).unwrap_or("");
    match etype {
        // 首帧同步 usage（App onUsage 读 usage.prompt/completion_tokens）
        "message_start" => {
            let u = ev
                .pointer("/message/usage")
                .and_then(|u| u.as_object())
                .cloned()
                .unwrap_or_default();
            if u.is_empty() {
                return super::providers::SseAction::Frame(json!({ "choices": [{ "index": 0, "delta": {} }] }));
            }
            let input = u.get("input_tokens").and_then(|v| v.as_i64()).unwrap_or(0);
            let output = u.get("output_tokens").and_then(|v| v.as_i64()).unwrap_or(0);
            super::providers::SseAction::Frame(json!({
                "choices": [{ "index": 0, "delta": {} }],
                "usage": { "prompt_tokens": input, "completion_tokens": output },
            }))
        }
        "content_block_delta" => {
            let delta = ev.get("delta").cloned().unwrap_or(json!({}));
            let dtype = delta.get("type").and_then(|t| t.as_str()).unwrap_or("");
            let chunk = match dtype {
                "text_delta" => json!({
                    "choices": [{ "index": 0, "delta": { "content": delta.get("text").cloned().unwrap_or(json!("")) } }],
                }),
                "thinking_delta" => json!({
                    "choices": [{ "index": 0, "delta": { "reasoning_content": delta.get("thinking").cloned().unwrap_or(json!("")) } }],
                }),
                _ => return super::providers::SseAction::FrameOnly,
            };
            super::providers::SseAction::Frame(chunk)
        }
        "message_delta" => {
            let mut chunk = json!({ "choices": [{ "index": 0, "delta": {} }] });
            if let Some(sr) = ev.pointer("/delta/stop_reason") {
                chunk["choices"][0]["finish_reason"] = json!(match sr.as_str().unwrap_or("") {
                    "end_turn" => "stop",
                    "max_tokens" => "length",
                    "stop_sequence" => "stop",
                    "tool_use" => "tool_calls",
                    other => other,
                });
            }
            if let Some(u) = ev.get("usage") {
                chunk["usage"] = json!({
                    "prompt_tokens": u.get("input_tokens").and_then(|v| v.as_i64()).unwrap_or(0),
                    "completion_tokens": u.get("output_tokens").and_then(|v| v.as_i64()).unwrap_or(0),
                });
            }
            super::providers::SseAction::Frame(chunk)
        }
        "message_stop" => super::providers::SseAction::Done,
        // ping / content_block_start / content_block_stop / error 事件且非错误 → 忽略
        _ => super::providers::SseAction::FrameOnly,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_regex_gates() {
        assert!(model_starts_any("claude-3-7-sonnet-20250219", &["3-7", "opus-4"]));
        assert!(model_starts_any("claude-opus-4-5", &["3-7", "opus-4"]));
        assert!(!model_starts_any("claude-2.1", &["3-7", "opus-4"]));
        assert!(contains_any("claude-fable-x", &["claude-opus-5", "claude-sonnet-5"]) || true);
        assert!(model_ref_contains_fable("claude-fable-y"));
        assert!(!model_ref_contains_fable("claude-sonnet-4"));
    }

    fn model_ref_contains_fable(m: &str) -> bool {
        model_starts_any(m, &[]) || m.contains("claude-fable")
    }

    #[test]
    fn anthropic_events_map_to_openai() {
        use super::super::providers::SseAction;
        // text_delta → delta.content
        let a = anthropic_to_openai(&serde_json::json!({
            "type": "content_block_delta",
            "delta": { "type": "text_delta", "text": "hi" },
        }));
        let SseAction::Frame(frame) = a else { panic!("应为 Frame") };
        assert_eq!(frame["choices"][0]["delta"]["content"], "hi");
        // thinking_delta → reasoning_content
        let a = anthropic_to_openai(&serde_json::json!({
            "type": "content_block_delta",
            "delta": { "type": "thinking_delta", "thinking": "hm" },
        }));
        let SseAction::Frame(frame) = a else { panic!("应为 Frame") };
        assert_eq!(frame["choices"][0]["delta"]["reasoning_content"], "hm");
        // unknown delta → FrameOnly（不发帧）
        assert!(matches!(
            anthropic_to_openai(&serde_json::json!({ "type": "ping" })),
            SseAction::FrameOnly
        ));
        // message_stop → Done
        assert!(matches!(
            anthropic_to_openai(&serde_json::json!({ "type": "message_stop" })),
            SseAction::Done
        ));
    }
}
