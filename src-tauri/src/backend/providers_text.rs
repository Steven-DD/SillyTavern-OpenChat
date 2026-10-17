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
