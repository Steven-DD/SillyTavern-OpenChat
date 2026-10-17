//! backend/providers_convert —— ST `src/prompt-converters.js`（1.19.0）的 Rust 忠实移植。
//!
//! 纯函数层：不含任何出站/HTTP 语义（arch-boundary 规则 2 适配）。
//! 契约来源（Phase 2 逐行核对）：
//! - postProcessPrompt / mergeMessages（prompt-converters.js:85-105, 827-954）
//! - convertClaudeMessages（:197-376）、convertGooglePrompt（:432-623）
//! - convertCohereMessages（:384-422）、convertAI21Messages（:631-695）
//! - convertMistralMessages（:703-777）、convertXAIMessages（:785-814）
//! - calculateClaudeBudgetTokens（:1124-1173）、calculateGoogleBudgetTokens（:1182-1326）
//! - addAssistantPrefix（:67-76）、addReasoningContentToToolCalls（:1377-1389）
//! - convertTextCompletionPrompt（:961-977）、embedOpenRouterMedia（:1336-1370）
//! - addOpenRouterSignatures（:1397-1451）
//!
//! 口径说明（登记到 manifest）：
//! - `promptPlaceholder` ST 默认 `Let's get started.`（无配置面 → 取默认）。
//! - claude 缓存三开关（enableSystemPromptCache / cachingAtDepth / extendedTTL）
//!   ST 默认全关 → 相应 cache_control 注入函数不移植（native 无该配置面，行为=恒否）。
//! - `gemini.thoughtSignatures` ST 默认 true → 签名注入按启用移植。

use serde_json::{json, Map, Value};

pub const PROMPT_PLACEHOLDER: &str = "Let's get started.";
/// gemini.thoughtSignatures 配置默认值（ST config 默认 true）
pub const ENABLE_THOUGHT_SIGNATURES: bool = true;

/// getPromptNames（prompt-converters.js:49-58）
#[derive(Default, Debug, Clone)]
pub struct PromptNames {
    pub char_name: String,
    pub user_name: String,
    pub group_names: Vec<String>,
}

impl PromptNames {
    pub fn from_body(body: &Value) -> Self {
        Self {
            char_name: body
                .get("char_name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            user_name: body
                .get("user_name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            group_names: body
                .get("group_names")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str())
                        .map(String::from)
                        .collect()
                })
                .unwrap_or_default(),
        }
    }
    /// startsWithGroupName（:54-56）
    pub fn starts_with_group_name(&self, message: &str) -> bool {
        self.group_names
            .iter()
            .any(|name| message.starts_with(&format!("{name}: ")))
    }
}

/* ---------------- 基础工具 ---------------- */

fn obj(v: &mut Value) -> &mut Map<String, Value> {
    if !v.is_object() {
        *v = json!({});
    }
    v.as_object_mut().unwrap()
}

/// 从 map 里读 role（避免持两重借用）
fn role_in(m: &Map<String, Value>) -> String {
    m.get("role").and_then(|r| r.as_str()).unwrap_or("").into()
}

fn name_in(m: &Map<String, Value>) -> String {
    m.get("name").and_then(|n| n.as_str()).unwrap_or("").into()
}

fn content_in(m: &Map<String, Value>) -> String {
    str_of(m.get("content").unwrap_or(&json!("")))
}

fn role_of(msg: &Value) -> String {
    msg.get("role").and_then(|r| r.as_str()).unwrap_or("").into()
}

fn set_role(msg: &mut Value, r: &str) {
    obj(msg).insert("role".into(), json!(r));
}

fn name_of(msg: &Value) -> String {
    msg.get("name").and_then(|n| n.as_str()).unwrap_or("").into()
}

fn str_of(v: &Value) -> String {
    match v {
        Value::String(t) => t.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// JS `typeof str === 'string' ? JSON.parse(str) : str`（parse 失败退回原串，避免 500 面扩大）
fn parse_jsonish(v: &Value) -> Value {
    if let Some(t) = v.as_str() {
        if let Ok(parsed) = serde_json::from_str::<Value>(t) {
            return parsed;
        }
        return json!(t);
    }
    v.clone()
}

/* ---------------- addAssistantPrefix / reasoning 补丁 ---------------- */

/// addAssistantPrefix（:67-76）—— 无 tools 且末条 assistant 时给末条加 `property=true`
pub fn add_assistant_prefix(prompt: &mut [Value], tools: Option<&Value>, property: &str) {
    if prompt.is_empty() {
        return;
    }
    let has_any_tools = tools
        .map(|t| t.as_array().map(|a| !a.is_empty()).unwrap_or(false))
        .unwrap_or(false)
        || prompt.iter().any(|x| role_of(x) == "tool");
    if !has_any_tools && role_of(&prompt[prompt.len() - 1]) == "assistant" {
        obj(&mut prompt[prompt.len() - 1]).insert(property.into(), Value::Bool(true));
    }
}

/// addReasoningContentToToolCalls（:1377-1389）—— DeepSeek reasoner 补空 reasoning_content
pub fn add_reasoning_content_to_tool_calls(messages: &mut [Value]) {
    for message in messages.iter_mut() {
        let has_tool_calls = message
            .get("tool_calls")
            .and_then(|t| t.as_array())
            .map(|a| !a.is_empty())
            .unwrap_or(false);
        if !has_tool_calls {
            continue;
        }
        let m = obj(message);
        if !m.contains_key("reasoning_content") {
            m.insert("reasoning_content".into(), json!(""));
        }
    }
}

/* ---------------- mergeMessages 家族 ---------------- */

/// postProcessPrompt（:85-105）—— 类型白名单外的原样返回
pub fn post_process_prompt(messages: &Value, ptype: &str, names: &PromptNames) -> Value {
    let (strict, placeholders, single, tools) = match ptype {
        "merge" | "claude" => (false, false, false, false),
        "merge_tools" => (false, false, false, true),
        "semi" => (true, false, false, false),
        "semi_tools" => (true, false, false, true),
        "strict" => (true, true, false, false),
        "strict_tools" => (true, true, false, true),
        "single" => (true, false, true, false),
        _ => return messages.clone(),
    };
    merge_messages(messages, names, strict, placeholders, single, tools)
}

/// mergeMessages（:827-954）。flags: strict / placeholders / single / tools
pub fn merge_messages(
    messages: &Value,
    names: &PromptNames,
    strict: bool,
    placeholders: bool,
    single: bool,
    tools: bool,
) -> Value {
    use base64::Engine;
    let mut vec: Vec<Value> = messages.as_array().cloned().unwrap_or_default();
    // 媒体占位 token → 原 content part
    let mut content_tokens: std::collections::HashMap<String, Value> =
        std::collections::HashMap::new();

    for message in vec.iter_mut() {
        let m = obj(message);
        if !m.contains_key("content") || m.get("content").map(|c| c.is_null()).unwrap_or(true) {
            m.insert("content".into(), json!(""));
        }
        // 数组 content 扁平化：text 直取；image/video/audio 用随机 token 占位
        if m.get("content").map(|c| c.is_array()).unwrap_or(false) {
            let parts: Vec<Value> = m["content"].as_array().cloned().unwrap_or_default();
            let mut text_pieces: Vec<String> = Vec::new();
            for content in &parts {
                let ctype = content.get("type").and_then(|t| t.as_str()).unwrap_or("");
                if ctype == "text" {
                    text_pieces.push(str_of(content.get("text").unwrap_or(&json!(""))));
                } else if ctype == "image_url" || ctype == "video_url" || ctype == "audio_url" {
                    let raw: Vec<u8> = (0..32).map(|_| rand_byte()).collect();
                    let token = base64::engine::general_purpose::STANDARD.encode(&raw);
                    text_pieces.push(token.clone());
                    content_tokens.insert(token, content.clone());
                } else {
                    text_pieces.push(String::new());
                }
            }
            m.insert("content".into(), json!(text_pieces.join("\n\n")));
        }

        let role = role_in(m);
        let name = name_in(m);
        let content = content_in(m);

        if role == "system" {
            if name == "example_assistant"
                && !names.char_name.is_empty()
                && !content.starts_with(&format!("{}: ", names.char_name))
                && !names.starts_with_group_name(&content)
            {
                m.insert(
                    "content".into(),
                    json!(format!("{}: {}", names.char_name, content)),
                );
            }
            if name == "example_user"
                && !names.user_name.is_empty()
                && !content.starts_with(&format!("{}: ", names.user_name))
            {
                m.insert(
                    "content".into(),
                    json!(format!("{}: {}", names.user_name, content)),
                );
            }
        }

        let name = name_in(m);
        let role = role_in(m);
        let content = content_in(m);
        if !name.is_empty() && role != "system" && !content.starts_with(&format!("{name}: ")) {
            m.insert("content".into(), json!(format!("{name}: {content}")));
        }

        if single {
            let role = role_in(m);
            let content = content_in(m);
            if role == "assistant"
                && !names.char_name.is_empty()
                && !content.starts_with(&format!("{}: ", names.char_name))
                && !names.starts_with_group_name(&content)
            {
                m.insert(
                    "content".into(),
                    json!(format!("{}: {}", names.char_name, content)),
                );
            }
            let content = content_in(m);
            if role == "user"
                && !names.user_name.is_empty()
                && !content.starts_with(&format!("{}: ", names.user_name))
            {
                m.insert(
                    "content".into(),
                    json!(format!("{}: {}", names.user_name, content)),
                );
            }
            m.insert("role".into(), json!("user"));
        }

        m.remove("name");
        if !tools {
            m.remove("tool_calls");
            m.remove("tool_call_id");
        }
    }

    // 连续同 role 合并（content 非空且非 tool）
    let mut merged: Vec<Value> = Vec::new();
    for message in vec {
        let can_merge = merged
            .last()
            .map(|last| {
                role_of(last) == role_of(&message)
                    && role_of(&message) != "tool"
                    && message
                        .get("content")
                        .and_then(|c| c.as_str())
                        .map(|c| !c.is_empty())
                        .unwrap_or(false)
            })
            .unwrap_or(false);
        if can_merge {
            let last = merged.last_mut().unwrap();
            let prev = str_of(last.get("content").unwrap_or(&json!("")));
            let cur = str_of(message.get("content").unwrap_or(&json!("")));
            obj(last).insert("content".into(), json!(format!("{prev}\n\n{cur}")));
        } else {
            merged.push(message);
        }
    }

    if merged.is_empty() {
        merged.push(json!({ "role": "user", "content": PROMPT_PLACEHOLDER }));
    }

    // 媒体 token 还原：content 按 '\n\n' 切分重建数组
    if !content_tokens.is_empty() {
        for message in merged.iter_mut() {
            let content = message
                .get("content")
                .and_then(|c| c.as_str())
                .unwrap_or_default()
                .to_string();
            let has_valid_token = content_tokens.keys().any(|t| content.contains(t));
            if !has_valid_token {
                continue;
            }
            let mut rebuilt: Vec<Value> = Vec::new();
            for piece in content.split("\n\n") {
                if let Some(part) = content_tokens.get(piece) {
                    rebuilt.push(part.clone());
                    continue;
                }
                let is_text = rebuilt
                    .last()
                    .and_then(|v| v.get("type").and_then(|t| t.as_str()))
                    == Some("text");
                if is_text {
                    let last = rebuilt.last_mut().unwrap();
                    let prev = str_of(last.get("text").unwrap_or(&json!("")));
                    obj(last).insert("text".into(), json!(format!("{prev}\n\n{piece}")));
                } else {
                    rebuilt.push(json!({ "type": "text", "text": piece }));
                }
            }
            obj(message).insert("content".into(), Value::Array(rebuilt));
        }
    }

    if strict {
        for i in 0..merged.len() {
            if i > 0 && role_of(&merged[i]) == "system" {
                set_role(&mut merged[i], "user");
            }
        }
        if !merged.is_empty() && placeholders {
            let head_is_system = role_of(&merged[0]) == "system";
            let next_is_user = merged.get(1).map(|m| role_of(m) == "user").unwrap_or(false);
            if head_is_system && (merged.len() == 1 || !next_is_user) {
                merged.insert(1, json!({ "role": "user", "content": PROMPT_PLACEHOLDER }));
            } else if role_of(&merged[0]) != "system" && role_of(&merged[0]) != "user" {
                merged.insert(0, json!({ "role": "user", "content": PROMPT_PLACEHOLDER }));
            }
        }
        return merge_messages(&Value::Array(merged), names, false, placeholders, false, tools);
    }

    Value::Array(merged)
}

fn rand_byte() -> u8 {
    use std::cell::Cell;
    use std::time::{SystemTime, UNIX_EPOCH};
    thread_local! {
        static STATE: Cell<u64> = const { Cell::new(0) };
    }
    STATE.with(|st| {
        let mut x = st.get();
        if x == 0 {
            x = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0x2545_F491_4F6C_DD1D)
                | 1;
        }
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        st.set(x);
        (x >> 24) as u8
    })
}

/* ---------------- Claude（messages API） ---------------- */

/// convertClaudeMessages（:197-376）—— 返回 (merged messages, system prompt blocks)
pub fn convert_claude_messages(
    messages: &mut Vec<Value>,
    prefill: Option<&str>,
    use_sys_prompt: bool,
    use_tools: bool,
    names: &PromptNames,
) -> (Value, Value) {
    let mut system_prompt: Vec<Value> = Vec::new();

    if use_sys_prompt {
        let mut i = 0usize;
        while i < messages.len() {
            if role_of(&messages[i]) != "system" {
                break;
            }
            let m = &mut messages[i];
            let name = name_of(m);
            let content = str_of(m.get("content").unwrap_or(&json!("")));
            if !names.user_name.is_empty() && name == "example_user" {
                if !content.starts_with(&format!("{}: ", names.user_name)) {
                    obj(m).insert(
                        "content".into(),
                        json!(format!("{}: {}", names.user_name, content)),
                    );
                }
            }
            if !names.char_name.is_empty() && name == "example_assistant" {
                if !content.starts_with(&format!("{}: ", names.char_name))
                    && !names.starts_with_group_name(&content)
                {
                    obj(m).insert(
                        "content".into(),
                        json!(format!("{}: {}", names.char_name, content)),
                    );
                }
            }
            system_prompt.push(json!({ "type": "text", "text": content }));
            i += 1;
        }
        messages.splice(0..i, []);
        if messages.is_empty() {
            messages.insert(0, json!({ "role": "user", "content": PROMPT_PLACEHOLDER }));
        }
    }

    for message in messages.iter_mut() {
        let role = role_of(message);

        if role == "assistant" && message.get("tool_calls").map(|t| t.is_array()).unwrap_or(false) {
            let tool_use: Vec<Value> = message
                .get("tool_calls")
                .and_then(|t| t.as_array())
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .map(|tc| {
                    json!({
                        "type": "tool_use",
                        "id": tc.get("id").cloned().unwrap_or(Value::Null),
                        "name": tc.get("function").and_then(|f| f.get("name")).cloned().unwrap_or(Value::Null),
                        "input": tc.get("function").and_then(|f| f.get("arguments")).map(parse_jsonish).unwrap_or(json!({})),
                    })
                })
                .collect();
            obj(message).insert("content".into(), Value::Array(tool_use));
        }

        if role == "tool" {
            let tool_call_id = message.get("tool_call_id").cloned().unwrap_or(Value::Null);
            let content = message.get("content").cloned().unwrap_or(Value::Null);
            let m = obj(message);
            m.insert("role".into(), json!("user"));
            m.insert(
                "content".into(),
                json!([{ "type": "tool_result", "tool_use_id": tool_call_id, "content": content }]),
            );
        }

        if role_of(message) == "system" {
            let name = name_of(message);
            let content = str_of(message.get("content").unwrap_or(&json!("")));
            let m = obj(message);
            if !names.user_name.is_empty() && name == "example_user" {
                if !content.starts_with(&format!("{}: ", names.user_name)) {
                    m.insert(
                        "content".into(),
                        json!(format!("{}: {}", names.user_name, content)),
                    );
                }
            }
            if !names.char_name.is_empty() && name == "example_assistant" {
                if !content.starts_with(&format!("{}: ", names.char_name))
                    && !names.starts_with_group_name(&content)
                {
                    m.insert(
                        "content".into(),
                        json!(format!("{}: {}", names.char_name, content)),
                    );
                }
            }
            m.insert("role".into(), json!("user"));
            m.remove("name");
        }

        // string → [{type:text}]（含 name 前缀）；数组 → 逐 part 转换
        let name = name_of(message);
        match message.get("content").cloned().unwrap_or(json!("")) {
            Value::String(text) => {
                let final_text = if !name.is_empty() {
                    format!("{name}: {text}")
                } else {
                    text
                };
                obj(message).insert(
                    "content".into(),
                    json!([{ "type": "text", "text": final_text }]),
                );
            }
            Value::Array(parts) => {
                let converted: Vec<Value> = parts
                    .into_iter()
                    .map(|c| {
                        let ctype = c.get("type").and_then(|t| t.as_str()).unwrap_or("");
                        if ctype == "image_url" {
                            let data_url = c
                                .get("image_url")
                                .and_then(|iu| iu.get("url"))
                                .and_then(|u| u.as_str())
                                .unwrap_or("");
                            let mime = data_url
                                .split(';')
                                .next()
                                .unwrap_or("")
                                .split(':')
                                .nth(1)
                                .unwrap_or("");
                            let b64 = data_url.split(',').nth(1).unwrap_or("");
                            return json!({
                                "type": "image",
                                "source": { "type": "base64", "media_type": mime, "data": b64 },
                            });
                        }
                        if ctype == "text" {
                            let mut text = str_of(c.get("text").unwrap_or(&json!("")));
                            if !name.is_empty() {
                                text = format!("{name}: {text}");
                            }
                            if text.is_empty() {
                                text = "\u{200b}".into();
                            }
                            return json!({ "type": "text", "text": text });
                        }
                        c
                    })
                    .collect();
                obj(message).insert("content".into(), Value::Array(converted));
            }
            _ => {}
        }

        // 移除违规属性（JS delete name/tool_calls/tool_call_id）
        let m = obj(message);
        m.remove("name");
        m.remove("tool_calls");
        m.remove("tool_call_id");
    }

    // assistant 层图片挪到下一个 user 消息（无则插入空 user）
    let mut i = 0usize;
    while i < messages.len() {
        let has_image = messages[i]
            .get("content")
            .and_then(|c| c.as_array())
            .map(|a| a.iter().any(|p| p.get("type").and_then(|t| t.as_str()) == Some("image")))
            .unwrap_or(false);
        if role_of(&messages[i]) != "assistant" || !has_image {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        while j < messages.len() && role_of(&messages[j]) != "user" {
            j += 1;
        }
        if j >= messages.len() {
            messages.insert(i + 1, json!({ "role": "user", "content": [] }));
            j = i + 1;
        }
        let images: Vec<Value> = messages[i]
            .get("content")
            .and_then(|c| c.as_array())
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|p| p.get("type").and_then(|t| t.as_str()) == Some("image"))
            .collect();
        let kept: Vec<Value> = messages[i]
            .get("content")
            .and_then(|c| c.as_array())
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|p| p.get("type").and_then(|t| t.as_str()) != Some("image"))
            .collect();
        obj(&mut messages[i]).insert("content".into(), Value::Array(kept));
        let arr = messages[j]
            .get("content")
            .and_then(|c| c.as_array())
            .cloned()
            .unwrap_or_default();
        let mut arr = arr;
        arr.extend(images);
        obj(&mut messages[j]).insert("content".into(), Value::Array(arr));
        i += 1;
    }

    // prefill：注释明确"除非显式 prefill"，最后 role 应为 user
    if let Some(p) = prefill {
        if !p.is_empty() {
            // 悬空空白不允许 prefill（trimEnd）
            messages.push(json!({
                "role": "assistant",
                "content": [{ "type": "text", "text": p.trim_end() }],
            }));
        }
    }

    // 相邻同 role 合并（messages API 只支持 user/assistant 交替）
    let mut merged: Vec<Value> = Vec::new();
    for message in messages {
        let role = role_of(message);
        match merged.last_mut() {
            Some(last) if role_of(last) == role => {
                let parts = message
                    .get("content")
                    .and_then(|c| c.as_array())
                    .cloned()
                    .unwrap_or_default();
                let arr = last
                    .get("content")
                    .and_then(|c| c.as_array())
                    .cloned()
                    .unwrap_or_default();
                let mut arr = arr;
                arr.extend(parts);
                obj(last).insert("content".into(), Value::Array(arr));
            }
            _ => merged.push(message.clone()),
        }
    }

    // tools 未启用：tool_use/tool_result 降级为纯文本
    if !use_tools {
        for message in merged.iter_mut() {
            let parts = message
                .get("content")
                .and_then(|c| c.as_array())
                .cloned()
                .unwrap_or_default();
            let converted: Vec<Value> = parts
                .into_iter()
                .map(|c| {
                    let ctype = c.get("type").and_then(|t| t.as_str()).unwrap_or("");
                    if ctype == "tool_use" {
                        let text = serde_json::to_string(c.get("input").unwrap_or(&Value::Null))
                            .unwrap_or_default();
                        return json!({ "type": "text", "text": text });
                    }
                    if ctype == "tool_result" {
                        let text = c.get("content").cloned().unwrap_or(Value::Null);
                        return json!({ "type": "text", "text": text });
                    }
                    c
                })
                .collect();
            obj(message).insert("content".into(), Value::Array(converted));
        }
    }

    (Value::Array(merged), Value::Array(system_prompt))
}

/* ---------------- Cohere ---------------- */

/// convertCohereMessages（:384-422）→ { chatHistory: [...] }
pub fn convert_cohere_messages(messages: &mut Vec<Value>, names: &PromptNames) -> Value {
    if messages.is_empty() {
        messages.insert(0, json!({ "role": "user", "content": PROMPT_PLACEHOLDER }));
    }
    let mut idx = 0usize;
    while idx < messages.len() {
        let has_tool_calls = messages[idx]
            .get("tool_calls")
            .and_then(|t| t.as_array())
            .map(|a| !a.is_empty())
            .unwrap_or(false);
        if has_tool_calls {
            if idx > 0 && role_of(&messages[idx - 1]) == "assistant" {
                let prev_content = messages[idx - 1].get("content").cloned().unwrap_or(Value::Null);
                obj(&mut messages[idx]).insert("content".into(), prev_content);
                messages.remove(idx - 1);
                // JS forEach 移除后不回看：工具消息现在落在 idx-1，下一步继续 idx（已滑动）
            } else {
                let list: Vec<String> = messages[idx]
                    .get("tool_calls")
                    .and_then(|t| t.as_array())
                    .cloned()
                    .unwrap_or_default()
                    .iter()
                    .map(|tc| {
                        tc.get("function")
                            .and_then(|f| f.get("name"))
                            .and_then(|n| n.as_str())
                            .unwrap_or("")
                            .to_string()
                    })
                    .collect();
                obj(&mut messages[idx]).insert(
                    "content".into(),
                    json!(format!("I'm going to call a tool for that: {}", list.join(", "))),
                );
            }
        }
        idx += 1;
    }
    for m in messages.iter_mut() {
        let name = name_of(m);
        if name.is_empty() {
            continue;
        }
        let role = role_of(m);
        let content = str_of(m.get("content").unwrap_or(&json!("")));
        let mref = obj(m);
        if role == "system" && name == "example_assistant" {
            if !names.char_name.is_empty()
                && !content.starts_with(&format!("{}: ", names.char_name))
                && !names.starts_with_group_name(&content)
            {
                mref.insert("content".into(), json!(format!("{}: {}", names.char_name, content)));
            }
        }
        if role == "system" && name == "example_user" {
            if !names.user_name.is_empty()
                && !content.starts_with(&format!("{}: ", names.user_name))
            {
                mref.insert("content".into(), json!(format!("{}: {}", names.user_name, content)));
            }
        }
        let content = str_of(m.get("content").unwrap_or(&json!("")));
        if role != "system" && !content.starts_with(&format!("{name}: ")) {
            obj(m).insert("content".into(), json!(format!("{name}: {content}")));
        }
        obj(m).remove("name");
    }
    Value::Array(messages.clone())
}

/* ---------------- AI21 ---------------- */

/// convertAI21Messages（:631-695）
pub fn convert_ai21_messages(messages: &mut Vec<Value>, names: &PromptNames) -> Value {
    let mut system_prompt = String::new();
    let mut i = 0usize;
    while i < messages.len() {
        if role_of(&messages[i]) != "system" {
            break;
        }
        let m = &mut messages[i];
        let name = name_of(m);
        let content = str_of(m.get("content").unwrap_or(&json!("")));
        if !names.user_name.is_empty() && name == "example_user" {
            if !content.starts_with(&format!("{}: ", names.user_name)) {
                obj(m).insert("content".into(), json!(format!("{}: {}", names.user_name, content)));
            }
        }
        if !names.char_name.is_empty() && name == "example_assistant" {
            if !content.starts_with(&format!("{}: ", names.char_name))
                && !names.starts_with_group_name(&content)
            {
                obj(m).insert("content".into(), json!(format!("{}: {}", names.char_name, content)));
            }
        }
        let content = str_of(m.get("content").unwrap_or(&json!("")));
        system_prompt.push_str(&format!("{content}\n\n"));
        i += 1;
    }
    messages.splice(0..i, []);
    if messages.is_empty() {
        messages.insert(0, json!({ "role": "user", "content": PROMPT_PLACEHOLDER }));
    }
    if !system_prompt.is_empty() {
        messages.insert(
            0,
            json!({ "role": "system", "content": system_prompt.trim().to_string() }),
        );
    }
    for m in messages.iter_mut() {
        if m.get("name").is_some() {
            let name = name_of(m);
            let role = role_of(m);
            let content = str_of(m.get("content").unwrap_or(&json!("")));
            if role != "system" && !content.starts_with(&format!("{name}: ")) {
                obj(m).insert("content".into(), json!(format!("{name}: {content}")));
            }
            obj(m).remove("name");
        }
    }
    let mut merged: Vec<Value> = Vec::new();
    for message in messages.clone() {
        let can_merge = merged
            .last()
            .map(|last| role_of(last) == role_of(&message))
            .unwrap_or(false);
        if can_merge {
            let last = merged.last_mut().unwrap();
            let prev = str_of(last.get("content").unwrap_or(&json!("")));
            let cur = str_of(message.get("content").unwrap_or(&json!("")));
            obj(last).insert("content".into(), json!(format!("{prev}\n\n{cur}")));
        } else {
            merged.push(message);
        }
    }
    Value::Array(merged)
}

/* ---------------- Mistral ---------------- */

/// convertMistralMessages（:703-777）
pub fn convert_mistral_messages(messages: &mut Vec<Value>, names: &PromptNames) -> Value {
    use sha2::{Digest, Sha512};

    // mistral.enablePrefix 配置 ST 默认 false（native 无配置面）
    const ENABLE_PREFIX: bool = false;
    if ENABLE_PREFIX {
        if let Some(last) = messages.last_mut() {
            if role_of(last) == "assistant" {
                obj(last).insert("prefix".into(), Value::Bool(true));
            }
        }
    }

    let sanitize = |id: &str| {
        let mut h = Sha512::new();
        h.update(id.as_bytes());
        let hex: String = h
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        hex.chars().take(9).collect::<String>()
    };

    for m in messages.iter_mut() {
        if m.get("tool_calls").map(|t| t.is_array()).unwrap_or(false) {
            let sanitized: Vec<Value> = m
                .get("tool_calls")
                .and_then(|t| t.as_array())
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .map(|mut tool| {
                    let id = tool.get("id").and_then(|i| i.as_str()).unwrap_or("").to_string();
                    obj(&mut tool).insert("id".into(), json!(sanitize(&id)));
                    tool
                })
                .collect();
            obj(m).insert("tool_calls".into(), Value::Array(sanitized));
        }
        if m.get("tool_call_id").is_some() && role_of(m) == "tool" {
            let id = m.get("tool_call_id").and_then(|i| i.as_str()).unwrap_or("").to_string();
            obj(m).insert("tool_call_id".into(), json!(sanitize(&id)));
        }
        let name = name_of(m);
        let role = role_of(m);
        let content = str_of(m.get("content").unwrap_or(&json!("")));
        if role == "system" && name == "example_assistant" {
            if !names.char_name.is_empty()
                && !content.starts_with(&format!("{}: ", names.char_name))
                && !names.starts_with_group_name(&content)
            {
                obj(m).insert(
                    "content".into(),
                    json!(format!("{}: {}", names.char_name, content)),
                );
            }
            obj(m).remove("name");
        }
        if role == "system" && name == "example_user" {
            if !names.user_name.is_empty()
                && !content.starts_with(&format!("{}: ", names.user_name))
            {
                obj(m).insert(
                    "content".into(),
                    json!(format!("{}: {}", names.user_name, content)),
                );
            }
            obj(m).remove("name");
        }
        if !name.is_empty() && role != "system" && !content.starts_with(&format!("{name}: ")) {
            obj(m).insert("content".into(), json!(format!("{name}: {content}")));
            obj(m).remove("name");
        }
    }

    // tool 后紧跟 user 的消息追加回上一个有 content 的 user（循环到不动点，JS 全等语义）
    loop {
        let mut rerun = false;
        let mut i = 0usize;
        while i < messages.len() {
            let is_last = i == messages.len() - 1;
            let tool_then_user =
                !is_last && role_of(&messages[i]) == "tool" && role_of(&messages[i + 1]) == "user";
            if tool_then_user {
                let mut last_user = None;
                for k in (0..i).rev() {
                    let is_user = role_of(&messages[k]) == "user";
                    // JS truthy：非空串 / 数组（含空）/ 对象
                    let truthy = match messages[k].get("content") {
                        Some(Value::String(t)) => !t.is_empty(),
                        Some(Value::Array(_)) | Some(Value::Object(_)) => true,
                        Some(_) => false, // number/bool：0/false 非 truthy
                        None => false,
                    };
                    if is_user && truthy {
                        last_user = Some(k);
                        break;
                    }
                }
                if let Some(k) = last_user {
                    let prev = str_of(messages[k].get("content").unwrap_or(&json!("")));
                    let extra = str_of(messages[i + 1].get("content").unwrap_or(&json!("")));
                    obj(&mut messages[k]).insert(
                        "content".into(),
                        Value::String(format!("{prev}\n\n{extra}")),
                    );
                    messages.remove(i + 1);
                    rerun = true;
                }
            }
            i += 1;
        }
        if !rerun {
            break;
        }
    }

    // assistant 后紧跟 system → 改 user
    if messages.len() > 1 {
        for i in 0..messages.len() - 1 {
            if role_of(&messages[i]) == "assistant" && role_of(&messages[i + 1]) == "system" {
                set_role(&mut messages[i + 1], "user");
            }
        }
    }

    Value::Array(messages.clone())
}

/* ---------------- xAI ---------------- */

/// convertXAIMessages（:785-814）
pub fn convert_xai_messages(messages: &mut Vec<Value>, names: &PromptNames) -> Value {
    for m in messages.iter_mut() {
        let name = name_of(m);
        let role = role_of(m);
        if name.is_empty() || role == "user" {
            continue;
        }
        let content = str_of(m.get("content").unwrap_or(&json!("")));
        let char_cond = !names.char_name.is_empty()
            && !content.starts_with(&format!("{}: ", names.char_name))
            && !names.starts_with_group_name(&content);
        let user_cond = !names.user_name.is_empty()
            && !content.starts_with(&format!("{}: ", names.user_name));
        let matching = (role == "assistant" && char_cond)
            || (role == "system" && name == "example_assistant" && char_cond)
            || (role == "system" && name == "example_user" && user_cond);
        if matching {
            let prefix = if role == "system" && name == "example_user" {
                names.user_name.clone()
            } else {
                names.char_name.clone()
            };
            obj(m).insert("content".into(), json!(format!("{prefix}: {content}")));
        }
        obj(m).remove("name");
    }
    Value::Array(messages.clone())
}

/* ---------------- Google（MakerSuite/VertexAI） ---------------- */

fn media_resolution(detail: &str) -> Option<&'static str> {
    match detail {
        "low" => Some("media_resolution_low"),
        "high" => Some("media_resolution_high"),
        _ => None,
    }
}

/// /gemini-3[.\d]*-flash/ 或 /gemini-3[.\d]*-pro/ 判别（返回 ('pro'|'flash')|None）
fn gemini3_variant(model: &str) -> Option<&'static str> {
    let rest = match model.find("gemini-3") {
        Some(i) => &model[i + 8..],
        None => return None,
    };
    let mut idx = 0;
    for b in rest.bytes() {
        if b == b'.' || b.is_ascii_digit() {
            idx += 1;
        } else {
            break;
        }
    }
    let tail = &rest[idx..];
    if tail.starts_with("-pro") {
        Some("pro")
    } else if tail.starts_with("-flash") {
        Some("flash")
    } else {
        None
    }
}

/// convertGooglePrompt（:432-623）→ (contents 数组, system_instruction 对象)
pub fn convert_google_prompt(
    messages: &mut Vec<Value>,
    model: &str,
    use_sys_prompt: bool,
    names: &PromptNames,
) -> (Value, Value) {
    let mut sys_prompt: Vec<Value> = Vec::new();

    if use_sys_prompt {
        while messages.len() > 1 && role_of(&messages[0]) == "system" {
            let m = &mut messages[0];
            let name = name_of(m);
            let content = str_of(m.get("content").unwrap_or(&json!("")));
            if !names.user_name.is_empty() && name == "example_user" {
                if !content.starts_with(&format!("{}: ", names.user_name)) {
                    obj(m).insert("content".into(), json!(format!("{}: {}", names.user_name, content)));
                }
            }
            if !names.char_name.is_empty() && name == "example_assistant" {
                if !content.starts_with(&format!("{}: ", names.char_name))
                    && !names.starts_with_group_name(&content)
                {
                    obj(m).insert("content".into(), json!(format!("{}: {}", names.char_name, content)));
                }
            }
            sys_prompt.push(m.get("content").cloned().unwrap_or(json!("")));
            messages.remove(0);
        }
    }

    let system_instruction = json!({
        "parts": sys_prompt.iter().map(|t| json!({ "text": t })).collect::<Vec<_>>()
    });

    // noPrefillModel = /gemini-3\.[67]-flash|gemini-3\.5-flash-lite/
    let no_prefill_model = model.contains("gemini-3.7-flash")
        || model.contains("gemini-3.6-flash")
        || model.contains("gemini-3.5-flash-lite");
    let mut tool_name_map: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    let mut contents: Vec<Value> = Vec::new();
    let total = messages.len();

    for (index, message) in messages.iter_mut().enumerate() {
        let role = role_of(message);
        if role == "system" || role == "tool" {
            set_role(message, "user");
        } else if role == "assistant" {
            let new_role = if no_prefill_model && index + 1 == total {
                "user"
            } else {
                "model"
            };
            set_role(message, new_role);
        }

        // content → parts 数组
        if !message.get("content").map(|c| c.is_array()).unwrap_or(false) {
            let has_tool_calls = message
                .get("tool_calls")
                .and_then(|t| t.as_array())
                .map(|a| !a.is_empty())
                .unwrap_or(false);
            let has_tool_call_id = message
                .get("tool_call_id")
                .and_then(|t| t.as_str())
                .map(|t| !t.is_empty())
                .unwrap_or(false);
            let content_obj = if has_tool_calls {
                json!({
                    "type": "tool_calls",
                    "tool_calls": message.get("tool_calls").cloned().unwrap_or(json!([])),
                })
            } else if has_tool_call_id {
                json!({
                    "type": "tool_call_id",
                    "tool_call_id": message.get("tool_call_id").cloned().unwrap_or(json!("")),
                    "content": str_of(message.get("content").unwrap_or(&json!(""))),
                })
            } else {
                json!({ "type": "text", "text": str_of(message.get("content").unwrap_or(&json!(""))) })
            };
            obj(message).insert("content".into(), json!([content_obj]));
        }

        // name 前缀（text part；example 名特判）
        if !name_of(message).is_empty() {
            let name = name_of(message);
            let parts = message
                .get("content")
                .and_then(|c| c.as_array())
                .cloned()
                .unwrap_or_default();
            let mut fixed: Vec<Value> = Vec::new();
            for mut part in parts {
                if part.get("type").and_then(|t| t.as_str()) == Some("text") {
                    let text = part.get("text").and_then(|t| t.as_str()).unwrap_or("").to_string();
                    if name == "example_user" {
                        if !names.user_name.is_empty()
                            && !text.starts_with(&format!("{}: ", names.user_name))
                        {
                            obj(&mut part).insert(
                                "text".into(),
                                json!(format!("{}: {}", names.user_name, text)),
                            );
                        }
                    } else if name == "example_assistant" {
                        if !names.char_name.is_empty()
                            && !text.starts_with(&format!("{}: ", names.char_name))
                            && !names.starts_with_group_name(&text)
                        {
                            obj(&mut part).insert(
                                "text".into(),
                                json!(format!("{}: {}", names.char_name, text)),
                            );
                        }
                    } else if !text.starts_with(&format!("{name}: ")) {
                        obj(&mut part).insert("text".into(), json!(format!("{name}: {text}")));
                    }
                }
                fixed.push(part);
            }
            let m = obj(message);
            m.insert("content".into(), Value::Array(fixed));
            m.remove("name");
        }

        // 生成 prompt parts
        let mut parts: Vec<Value> = Vec::new();
        let content_parts = message
            .get("content")
            .and_then(|c| c.as_array())
            .cloned()
            .unwrap_or_default();
        let role = role_of(message);
        for part in content_parts {
            let ptype = part.get("type").and_then(|t| t.as_str()).unwrap_or("");
            match ptype {
                "text" => parts.push(json!({ "text": part.get("text").cloned().unwrap_or(json!("")) })),
                "tool_call_id" => {
                    let call_id = part.get("tool_call_id").and_then(|t| t.as_str()).unwrap_or("unknown");
                    let tname = tool_name_map
                        .get(call_id)
                        .cloned()
                        .unwrap_or_else(|| "unknown".into());
                    let pcontent = part.get("content").cloned().unwrap_or(Value::Null);
                    parts.push(json!({
                        "functionResponse": {
                            "name": tname,
                            "response": { "name": tname, "content": pcontent },
                        }
                    }));
                }
                "tool_calls" => {
                    for tc in part
                        .get("tool_calls")
                        .and_then(|t| t.as_array())
                        .cloned()
                        .unwrap_or_default()
                    {
                        let mut p = json!({
                            "functionCall": {
                                "name": tc.get("function").and_then(|f| f.get("name")).cloned().unwrap_or(Value::Null),
                                "args": tc.get("function").and_then(|f| f.get("arguments")).map(parse_jsonish).unwrap_or(Value::Null),
                            }
                        });
                        if let Some(sig) = tc.get("signature").and_then(|sg| sg.as_str()) {
                            obj(&mut p).insert("thoughtSignature".into(), json!(sig));
                        }
                        parts.push(p);
                        let id = tc.get("id").and_then(|i| i.as_str()).unwrap_or("").to_string();
                        let fnname = tc
                            .get("function")
                            .and_then(|f| f.get("name"))
                            .and_then(|n| n.as_str())
                            .unwrap_or("")
                            .to_string();
                        tool_name_map.insert(id, fnname);
                    }
                }
                "image_url" => {
                    let url = part
                        .get("image_url")
                        .and_then(|i| i.get("url"))
                        .and_then(|u| u.as_str())
                        .unwrap_or("");
                    let detail = part
                        .get("image_url")
                        .and_then(|i| i.get("detail"))
                        .and_then(|d| d.as_str())
                        .unwrap_or("");
                    add_data_url_part(&mut parts, url, "image/png", detail, model);
                }
                "video_url" => {
                    let url = part
                        .get("video_url")
                        .and_then(|i| i.get("url"))
                        .and_then(|u| u.as_str())
                        .unwrap_or("");
                    let detail = part
                        .get("video_url")
                        .and_then(|i| i.get("detail"))
                        .and_then(|d| d.as_str())
                        .unwrap_or("");
                    add_data_url_part(&mut parts, url, "video/mp4", detail, model);
                }
                "audio_url" => {
                    let url = part
                        .get("audio_url")
                        .and_then(|i| i.get("url"))
                        .and_then(|u| u.as_str())
                        .unwrap_or("");
                    add_data_url_part(&mut parts, url, "audio/mpeg", "", model);
                }
                _ => {}
            }
        }

        // thought signature 注入（gemini-3 / gemini-2.5）
        if model.contains("gemini-3") || model.contains("gemini-2.5") {
            const SKIP_MAGIC: &str = "skip_thought_signature_validator";
            let text_sig = message.get("signature").and_then(|sg| sg.as_str()).map(String::from);
            for part in parts.iter_mut() {
                let m = obj(part);
                let has_text = m.get("text").map(|t| t.is_string()).unwrap_or(false);
                if ENABLE_THOUGHT_SIGNATURES {
                    if let Some(sig) = &text_sig {
                        if has_text {
                            m.insert("thoughtSignature".into(), json!(sig));
                        }
                    } else if model.contains("gemini-3") {
                        if m.get("functionCall").is_some() && !m.contains_key("thoughtSignature") {
                            m.insert("thoughtSignature".into(), json!(SKIP_MAGIC));
                        }
                        if model.contains("-image") && role == "model"
                            && (has_text || m.get("inlineData").is_some())
                        {
                            m.insert("thoughtSignature".into(), json!(SKIP_MAGIC));
                        }
                    }
                } else if model.contains("gemini-3") {
                    if m.get("functionCall").is_some() && !m.contains_key("thoughtSignature") {
                        m.insert("thoughtSignature".into(), json!(SKIP_MAGIC));
                    }
                    if model.contains("-image") && role == "model"
                        && (has_text || m.get("inlineData").is_some())
                    {
                        m.insert("thoughtSignature".into(), json!(SKIP_MAGIC));
                    }
                }
            }
        }

        // 相邻同 role 合并（text 拼接；内联数据/函数调用/签名直接推入）
        if index > 0 && contents.last().map(|c| role_of(c) == role).unwrap_or(false) {
            let last = contents.last_mut().unwrap();
            let mut last_parts = last
                .get("parts")
                .and_then(|p| p.as_array())
                .cloned()
                .unwrap_or_default();
            for part in parts {
                if part.get("text").map(|t| t.is_string()).unwrap_or(false) {
                    if let Some(text_part) = last_parts
                        .iter_mut()
                        .find(|p| p.get("text").map(|t| t.is_string()).unwrap_or(false))
                    {
                        let prev = str_of(text_part.get("text").unwrap_or(&json!("")));
                        let cur = str_of(part.get("text").unwrap_or(&json!("")));
                        obj(text_part).insert("text".into(), json!(format!("{prev}\n\n{cur}")));
                    } else {
                        last_parts.push(part.clone());
                    }
                }
                let special = part.get("inlineData").is_some()
                    || part.get("functionCall").is_some()
                    || part.get("functionResponse").is_some()
                    || part.get("thoughtSignature").is_some()
                    || part.get("mediaResolution").is_some();
                if special {
                    last_parts.push(part);
                }
            }
            obj(last).insert("parts".into(), Value::Array(last_parts));
        } else {
            contents.push(json!({ "role": role, "parts": parts }));
        }
    }

    (Value::Array(contents), system_instruction)
}

fn add_data_url_part(
    parts: &mut Vec<Value>,
    raw_url: &str,
    default_mime: &str,
    detail: &str,
    model: &str,
) {
    if raw_url.is_empty() || !raw_url.starts_with("data:") {
        return;
    }
    let (header, base64_data) = match raw_url.split_once(',') {
        Some((h, d)) => (h, d),
        None => (raw_url, ""),
    };
    let mime = header
        .split_once("data:")
        .map(|(_, m)| m.split(';').next().unwrap_or(default_mime))
        .filter(|m| !m.is_empty())
        .unwrap_or(default_mime);
    let mut part = json!({ "inlineData": { "mimeType": mime, "data": base64_data } });
    if let Some(level) = media_resolution(detail) {
        if model.contains("gemini-3") {
            part.as_object_mut().unwrap().insert(
                "mediaResolution".into(),
                json!({ "level": level }),
            );
        }
    }
    parts.push(part);
}

/* ---------------- 预算 tokens ---------------- */

/// calculateClaudeBudgetTokens（:1124-1173）
/// 返回：None = auto / adaptive 未匹配；Value(String) = adaptive 档位；Value(Number) = 预算
pub fn calculate_claude_budget_tokens(
    max_tokens: f64,
    reasoning_effort: Option<&str>,
    stream: bool,
    is_adaptive_model: bool,
) -> Option<Value> {
    if is_adaptive_model {
        return match reasoning_effort.unwrap_or("") {
            "low" | "min" => Some(json!("low")),
            "medium" => Some(json!("medium")),
            "high" => Some(json!("high")),
            "max" => Some(json!("max")),
            _ => None, // 'auto' / 未匹配
        };
    }
    let effort = reasoning_effort.unwrap_or("");
    let mut budget: f64 = match effort {
        "auto" => return None,
        "min" => 1024.0,
        "low" => (max_tokens * 0.1).floor(),
        "medium" => (max_tokens * 0.25).floor(),
        "high" => (max_tokens * 0.5).floor(),
        "max" => (max_tokens * 0.95).floor(),
        _ => 0.0, // 未匹配档位保持 0（JS switch 无 default 的字面行为）
    };
    budget = budget.max(1024.0);
    if !stream {
        budget = budget.min(21333.0);
    }
    Some(json!(budget as u64))
}

/// calculateGoogleBudgetTokens（:1182-1326）。返回 -1 = 动态（API 字面）、0 = 关闭、字符串 = 档位
pub fn calculate_google_budget_tokens(
    max_tokens: f64,
    reasoning_effort: Option<&str>,
    model: &str,
) -> Option<Value> {
    let effort = reasoning_effort.unwrap_or("");
    let floor = |f: f64| f.floor();

    if let Some(variant) = gemini3_variant(model) {
        return match (variant, effort) {
            ("pro", "min") | ("pro", "low") => Some(json!("low")),
            ("pro", "medium") => Some(json!("low")),
            ("pro", "high") | ("pro", "max") => Some(json!("high")),
            ("pro", "auto") | ("pro", "") => None,
            ("flash", "min") => Some(json!(if model.contains("gemini-3.7-flash") { "low" } else { "minimal" })),
            ("flash", "low") => Some(json!("low")),
            ("flash", "medium") => Some(json!("medium")),
            ("flash", "high") | ("flash", "max") => Some(json!("high")),
            ("flash", _) => None,
            _ => None,
        };
    }

    // flash-lite：cap 24576 floor 512；flash：cap 24576；pro：cap 32768 floor 128
    let (cap, floor_val, min_const) = if model.contains("flash-lite") {
        (24576.0, 512.0, 0.0)
    } else if model.contains("flash") {
        (24576.0, 0.0, 0.0)
    } else if model.contains("pro") {
        (32768.0, 128.0, 128.0)
    } else {
        return None;
    };

    let budget = match effort {
        "auto" => return Some(json!(-1)),
        "min" => return Some(json!(min_const as i64)),
        "low" => floor(max_tokens * 0.1),
        "medium" => floor(max_tokens * 0.25),
        "high" => floor(max_tokens * 0.5),
        "max" => max_tokens,
        _ => 0.0,
    };
    let budget = budget.min(cap);
    let budget = if floor_val > 0.0 { budget.max(floor_val) } else { budget };
    Some(json!(budget as i64))
}

/* ---------------- Text completion 拼接 ---------------- */

/// convertTextCompletionPrompt（:961-977）
pub fn convert_text_completion_prompt(messages: &Value) -> String {
    if let Some(t) = messages.as_str() {
        return t.to_string();
    }
    let mut outs: Vec<String> = Vec::new();
    for m in messages.as_array().cloned().unwrap_or_default() {
        let role = m.get("role").and_then(|r| r.as_str()).unwrap_or("");
        let name = m.get("name").and_then(|n| n.as_str());
        let content = m.get("content").and_then(|c| c.as_str()).unwrap_or("");
        if role == "system" && name.is_none() {
            outs.push(format!("System: {content}"));
        } else if role == "system" {
            outs.push(format!("{}: {}", name.unwrap_or(""), content));
        } else {
            outs.push(format!("{role}: {content}"));
        }
    }
    format!("{}\nassistant:", outs.join("\n"))
}

/* ---------------- OpenRouter 媒体 / 签名 ---------------- */

/// embedOpenRouterMedia（:1336-1370）—— audio_url → {type: input_audio}
pub fn embed_openrouter_media(messages: &mut Value, audio: bool, _video: bool) {
    if !messages.is_array() {
        return;
    }
    for message in messages.as_array_mut().unwrap() {
        if !message.get("content").map(|c| c.is_array()).unwrap_or(false) {
            continue;
        }
        let parts = message
            .get("content")
            .and_then(|c| c.as_array())
            .cloned()
            .unwrap_or_default();
        let mut fixed: Vec<Value> = Vec::new();
        for mut part in parts {
            let ptype = part.get("type").and_then(|t| t.as_str()).unwrap_or("");
            if audio && ptype == "audio_url" {
                let url = part
                    .get("audio_url")
                    .and_then(|a| a.get("url"))
                    .and_then(|u| u.as_str())
                    .unwrap_or("");
                if url.starts_with("data:") {
                    let (header, b64) = match url.split_once(',') {
                        Some((h, d)) => (h, d),
                        None => ("", ""),
                    };
                    let mime = header
                        .split_once("data:")
                        .map(|(_, m)| m.split(';').next().unwrap_or("audio/mpeg").to_string())
                        .filter(|m| !m.is_empty())
                        .unwrap_or_else(|| "audio/mpeg".into());
                    let fmt = match mime.as_str() {
                        "audio/mpeg" => "mp3",
                        "audio/wav" => "wav",
                        _ => "mp3",
                    };
                    let mut np = json!({
                        "type": "input_audio",
                        "input_audio": { "format": fmt, "data": b64 },
                    });
                    np.as_object_mut().unwrap().remove("audio_url");
                    part = np;
                }
            }
            fixed.push(part);
        }
        obj(message).insert("content".into(), Value::Array(fixed));
    }
}

/// addOpenRouterSignatures（:1397-1451）—— signature → reasoning_details[].reasoning.encrypted
pub fn add_open_router_signatures(messages: &mut Value, model: &str) {
    let fmt = if model.contains("google/gemini") {
        "google-gemini-v1"
    } else if model.contains("anthropic/claude") {
        "anthropic-claude-v1"
    } else if model.contains("openai/gpt") {
        "openai-responses-v1"
    } else if model.contains("x-ai/grok") {
        "xai-responses-v1"
    } else {
        "unknown"
    };
    if !messages.is_array() {
        return;
    }
    for message in messages.as_array_mut().unwrap() {
        let mut details: Vec<Value> = Vec::new();
        let add_detail = |details: &mut Vec<Value>, data: &str, id: Option<&str>| {            if data.is_empty() {
                return;
            }
            details.push(json!({
                "index": details.len(),
                "id": id.filter(|i| !i.is_empty()).map(String::from)
                    .unwrap_or_else(|| format!("signature-{}", details.len())),
                "type": "reasoning.encrypted",
                "data": data,
                "format": fmt,
            }));
        };
        let sig = message.get("signature").and_then(|sg| sg.as_str()).map(String::from);
        if let Some(data) = sig {
            if ENABLE_THOUGHT_SIGNATURES {
                add_detail(&mut details, &data, None);
            }
            obj(message).remove("signature");
        }
        if let Some(tcs) = message.get_mut("tool_calls").and_then(|t| t.as_array_mut()) {
            for tc in tcs.iter_mut() {
                let tcsig = tc.get("signature").and_then(|sg| sg.as_str()).map(String::from);
                if let Some(data) = tcsig {
                    let id = tc.get("id").and_then(|i| i.as_str()).map(String::from);
                    add_detail(&mut details, &data, id.as_deref());
                    obj(tc).remove("signature");
                }
            }
        }
        if !details.is_empty() {
            obj(message).insert("reasoning_details".into(), Value::Array(details));
        }
    }
}

/* ============================ tests ============================ */

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn names() -> PromptNames {
        PromptNames::default()
    }

    fn arr(v: Value) -> Vec<Value> {
        v.as_array().unwrap().clone()
    }

    #[test]
    fn merge_semi_squash_and_strict_placeholder() {
        let msgs = json!([
            { "role": "system", "content": "a" },
            { "role": "system", "content": "b" },
            { "role": "user", "content": "hi" },
            { "role": "user", "content": "again" },
            { "role": "assistant", "content": "ok" },
        ]);
        let semi = post_process_prompt(&msgs, "semi", &names());
        assert_eq!(semi.as_array().unwrap().len(), 3);
        assert_eq!(semi[0]["content"], "a\n\nb");
        assert_eq!(semi[1]["content"], "hi\n\nagain");
        // STRICT：无占位可插（system 后紧跟 user），与 SEMI 同构
        let strict = post_process_prompt(&msgs, "strict", &names());
        assert_eq!(strict.to_string(), semi.to_string());
    }

    #[test]
    fn merge_single_all_user() {
        let msgs = json!([
            { "role": "system", "content": "sys" },
            { "role": "assistant", "content": "hi" },
        ]);
        let single = post_process_prompt(&msgs, "single", &names());
        let a = single.as_array().unwrap();
        assert_eq!(a.len(), 1, "single 全并为一条");
        assert_eq!(a[0]["role"], "user");
    }

    #[test]
    fn google_roles_and_merging() {
        let mut msgs = arr(json!([
            { "role": "system", "content": "S" },
            { "role": "user", "content": "U" },
            { "role": "assistant", "content": "A" },
        ]));
        let (contents, _) = convert_google_prompt(&mut msgs, "gemini-2.0-flash", false, &names());
        // system→user 后与 user 连续段合并（首个 text part 后接 "\n\n"+text —— JS 字面）
        assert_eq!(
            contents.to_string(),
            json!([
                { "role": "user", "parts": [{ "text": "S\n\nU" }] },
                { "role": "model", "parts": [{ "text": "A" }] },
            ])
            .to_string()
        );
    }

    #[test]
    fn google_no_prefill_model_turns_trailing_assistant_to_user() {
        let mut msgs = arr(json!([
            { "role": "user", "content": "U" },
            { "role": "assistant", "content": "A" },
        ]));
        let (contents, _) = convert_google_prompt(&mut msgs, "gemini-3.7-flash", false, &names());
        // 两条都变 user 后连续合并成单轮
        assert_eq!(
            contents.to_string(),
            json!([{ "role": "user", "parts": [{ "text": "U\n\nA" }] }]).to_string()
        );
    }

    #[test]
    fn google_merge_same_role_parts_text() {
        let mut msgs = arr(json!([
            { "role": "user", "content": "U1" },
            { "role": "user", "content": "U2" },
        ]));
        let (contents, _) = convert_google_prompt(&mut msgs, "gemini-2.0-flash", false, &names());
        assert_eq!(contents.as_array().unwrap().len(), 1);
        assert_eq!(contents[0]["parts"][0]["text"], "U1\n\nU2");
    }

    #[test]
    fn claude_wrap_merge_and_system_drop() {
        let mut msgs = arr(json!([
            { "role": "system", "content": "S" },
            { "role": "user", "content": "U1" },
            { "role": "user", "content": "U2" },
            { "role": "assistant", "content": "A" },
        ]));
        let (merged, sys) = convert_claude_messages(&mut msgs, None, false, false, &names());
        let a = merged.as_array().unwrap();
        assert!(sys.as_array().unwrap().is_empty(), "未启用 sysprompt → 空 system");
        assert_eq!(a.len(), 2, "system→user 与连续 user 合并");
        assert_eq!(a[0]["role"], "user");
        assert_eq!(
            a[0]["content"].to_string(),
            json!([
                { "type": "text", "text": "S" },
                { "type": "text", "text": "U1" },
                { "type": "text", "text": "U2" },
            ])
            .to_string()
        );
        assert_eq!(a[1]["role"], "assistant");
    }

    #[test]
    fn claude_prefill_and_tool_conversion() {
        let mut msgs = arr(json!([
            { "role": "user", "content": "U" },
            { "role": "assistant", "tool_calls": [{ "id": "c1", "function": { "name": "f", "arguments": "{\"a\":1}" } }] },
            { "role": "tool", "tool_call_id": "c1", "content": "result-data" },
        ]));
        let (merged, _) = convert_claude_messages(&mut msgs, Some(" prefill "), false, true, &names());
        let a = merged.as_array().unwrap();
        assert_eq!(a[1]["content"][0]["type"], "tool_use");
        assert_eq!(a[1]["content"][0]["input"], json!({ "a": 1 }));
        assert_eq!(a[2]["content"][0]["type"], "tool_result");
        let last = a.last().unwrap();
        assert_eq!(last["role"], "assistant");
        assert_eq!(last["content"][0]["text"], " prefill"); // JS trimEnd 只去尾随空白
    }

    #[test]
    fn cohere_tool_assistant_primer() {
        let mut msgs = arr(json!([
            { "role": "user", "content": "U" },
            { "role": "assistant", "content": "P" },
            { "role": "assistant", "tool_calls": [{ "id": "i", "function": { "name": "f", "arguments": "{}" } }] },
        ]));
        let out = convert_cohere_messages(&mut msgs, &names());
        let a = out.as_array().unwrap();
        // tool_calls 消息继承前一条 assistant 内容并吃掉它（JS forEach 不回看）
        assert_eq!(a.len(), 2);
        assert_eq!(a[1]["content"], "P");
    }

    #[test]
    fn xai_name_prefix() {
        let mut msgs = vec![json!({ "role": "assistant", "name": "Alice", "content": "hello" })];
        let out = convert_xai_messages(
            &mut msgs,
            &PromptNames { char_name: "Alice".into(), ..Default::default() },
        );
        assert_eq!(out.as_array().unwrap()[0]["content"], "Alice: hello");
    }

    #[test]
    fn mistral_tool_id_sanitize_sha512_9() {
        let mut msgs = vec![json!({
            "role": "tool",
            "tool_call_id": "x",
            "content": "r",
        })];
        let out = convert_mistral_messages(&mut msgs, &names());
        let got = out.as_array().unwrap()[0]["tool_call_id"]
            .as_str()
            .unwrap()
            .to_string();
        assert_eq!(got.len(), 9);
        assert!(got.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn budget_claude_literal_paths() {
        // 'auto' → None（不开 thinking）；high → max*0.5 且 ≥1024；非流式 ≤21333
        assert_eq!(calculate_claude_budget_tokens(2048.0, Some("auto"), true, false), None);
        assert_eq!(
            calculate_claude_budget_tokens(2048.0, Some("high"), true, false),
            Some(json!(1024u64))
        );
        assert_eq!(
            calculate_claude_budget_tokens(99999.0, Some("high"), false, false),
            Some(json!(21333u64))
        );
        // 未匹配档位（App 不传 reasoning_effort）→ 1024（ST 字面）
        assert_eq!(
            calculate_claude_budget_tokens(2048.0, None, true, false),
            Some(json!(1024u64))
        );
        assert_eq!(
            calculate_claude_budget_tokens(99999.0, Some("max"), true, true),
            Some(json!("max"))
        );
    }

    #[test]
    fn budget_google_family_paths() {
        assert_eq!(
            calculate_google_budget_tokens(1000.0, Some("min"), "gemini-2.5-flash"),
            Some(json!(0))
        );
        assert_eq!(
            calculate_google_budget_tokens(1000.0, Some("auto"), "gemini-2.5-flash"),
            Some(json!(-1))
        );
        assert_eq!(
            calculate_google_budget_tokens(1000.0, Some("max"), "gemini-2.5-flash"),
            Some(json!(1000)) // max → maxTokens 本身，上限 24576 未触发
        );
        assert_eq!(
            calculate_google_budget_tokens(1000.0, Some("max"), "gemini-3.7-flash"),
            Some(json!("high"))
        );
        assert_eq!(
            calculate_google_budget_tokens(1000.0, Some("min"), "gemini-2.5-pro"),
            Some(json!(128))
        );
        assert_eq!(
            calculate_google_budget_tokens(1000.0, Some("max"), "gemini-2.5-pro"),
            Some(json!(1000))
        );
        assert_eq!(calculate_google_budget_tokens(1000.0, Some("min"), "mystery"), None);
    }

    #[test]
    fn text_completion_prompt_shape() {
        let msgs = json!([
            { "role": "system", "content": "S" },
            { "role": "user", "content": "U" },
        ]);
        assert_eq!(
            convert_text_completion_prompt(&msgs),
            "System: S\nuser: U\nassistant:"
        );
        assert_eq!(convert_text_completion_prompt(&json!("raw")), "raw");
    }

    #[test]
    fn openrouter_media_audio_conversion() {
        let mut msgs = json!([
            { "role": "user", "content": [
                { "type": "text", "text": "t" },
                { "type": "audio_url", "audio_url": { "url": "data:audio/wav;base64,QUJD" } },
            ] },
        ]);
        embed_openrouter_media(&mut msgs, true, true);
        let a = msgs.as_array().unwrap()[0]["content"].as_array().unwrap();
        assert_eq!(a.len(), 2);
        assert_eq!(a[1]["type"], "input_audio");
        assert_eq!(a[1]["input_audio"]["format"], "wav");
        assert_eq!(a[1]["input_audio"]["data"], "QUJD");
    }

    #[test]
    fn openrouter_signatures_conversion() {
        let mut msgs = json!([{ "role": "assistant", "content": "A", "signature": "SGE=" }]);
        add_open_router_signatures(&mut msgs, "anthropic/claude-3");
        let m = msgs.as_array().unwrap()[0].clone();
        assert_eq!(m["reasoning_details"][0]["type"], "reasoning.encrypted");
        assert_eq!(m["reasoning_details"][0]["format"], "anthropic-claude-v1");
        assert!(m.get("signature").is_none());
    }

    #[test]
    fn deepseek_reasoning_content_added() {
        let mut msgs = vec![json!({
            "role": "assistant",
            "tool_calls": [{ "id": "i", "function": { "name": "f", "arguments": "{}" } }],
        })];
        add_reasoning_content_to_tool_calls(&mut msgs);
        assert_eq!(msgs[0]["reasoning_content"], "");
    }
}
