//! providers_text —— Text/Chat Completion 模型名单共享件（providers 内部共用）
//!
//! TEXT_COMPLETION_MODELS（chat-completions.js:2734 附近同名列表）：
//! 这些模型走 `/completions` 而非 `/chat/completions`。

/// gpt-3.5/4 时代的 base 模型名单（与 ST 名单同步Slice）
pub const TEXT_COMPLETION_MODELS: [&str; 12] = [
    "gpt-3.5-turbo-instruct",
    "gpt-3.5-turbo-instruct-0914",
    "text-davinci-003",
    "text-davinci-002",
    "text-davinci-001",
    "text-curie-001",
    "text-babbage-001",
    "text-ada-001",
    "davinci-002",
    "babbage-002",
    "davinci",
    "gpt-4o-instruct",
];

/// 该模型是否属 text completion 家族（走 /completions）
pub fn is_text_completion_model(model: &str) -> bool {
    TEXT_COMPLETION_MODELS.iter().any(|m| model.trim() == *m)
}

/// OPENAI_REASONING_EFFORT_MODELS（constants.js:461-495）——
/// 支持 reasoning_effort 的 OpenAI 模型名单（Azure/reasoning_effort 分流共用）
pub const OPENAI_REASONING_EFFORT_MODELS: [&str; 33] = [
    "o1",
    "o3-mini",
    "o3-mini-2025-01-31",
    "o4-mini",
    "o4-mini-2025-04-16",
    "o3",
    "o3-2025-04-16",
    "gpt-5",
    "gpt-5-2025-08-07",
    "gpt-5-mini",
    "gpt-5-mini-2025-08-07",
    "gpt-5-nano",
    "gpt-5-nano-2025-08-07",
    "gpt-5.1",
    "gpt-5.1-2025-11-13",
    "gpt-5.1-chat-latest",
    "gpt-5.2",
    "gpt-5.2-2025-12-11",
    "gpt-5.2-chat-latest",
    "gpt-5.3-chat-latest",
    "gpt-5.4",
    "gpt-5.4-2026-03-05",
    "gpt-5.4-mini",
    "gpt-5.4-mini-2026-03-17",
    "gpt-5.4-nano",
    "gpt-5.4-nano-2026-03-17",
    "gpt-5.5",
    "gpt-5.5-2026-04-23",
    "gpt-5.6",
    "gpt-5.6-sol",
    "gpt-5.6-terra",
    "gpt-5.6-luna",
    "gpt-6-astra",
];

pub fn is_openai_reasoning_effort_model(model: &str) -> bool {
    OPENAI_REASONING_EFFORT_MODELS.contains(&model)
}

/// OPENAI_REASONING_EFFORT_MAP / OPENAI_FIXED_REASONING_EFFORT（constants.js）
pub fn map_openai_reasoning_effort(model: &str, effort: &str) -> String {
    if model == "gpt-5.3-chat-latest" {
        return "medium".into(); // 固定档优先
    }
    if effort == "min" {
        return "minimal".into();
    }
    effort.into()
}

/// OPENAI_VERBOSITY_MODELS = /^(?:gpt-5|gpt-6-astra)/
pub fn is_openai_verbosity_model(model: &str) -> bool {
    model.starts_with("gpt-5") || model.starts_with("gpt-6-astra")
}

/// NANOGPT_REASONING_EFFORT_MAP（constants.js:509-515）
pub fn map_nanogpt_reasoning_effort(effort: &str) -> &'static str {
    match effort {
        "min" => "none",
        "low" => "minimal",
        "medium" => "low",
        "high" => "medium",
        "max" => "high",
        _ => "",
    }
}
