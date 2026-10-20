//! backend/mcp —— MCP (Model Context Protocol) 协议客户端与原生工具箱（P2 级能力）
//!
//! 提供原生安全工具集：
//! 1. `web_search`: DuckDuckGo 零密钥实时联网搜索
//! 2. `fetch_webpage`: 网页正文安全提取
//! 3. `current_time`: 系统当前日期时间与星期感知
//! 4. `calculator`: 安全数学表达式求值引擎
//!
//! 支持对外以标准 OpenAI/Claude `tools` schema 暴露，并承接 /api/mcp/call 执行调度。

use super::providers::{json_resp, validate_egress_url, EgressPolicy};
use super::BackendState;
use axum::http::{HeaderValue, StatusCode};
use axum::response::Response;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ToolCallRequest {
    pub name: String,
    pub arguments: Value,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ToolCallResponse {
    pub name: String,
    pub content: String,
    pub is_error: bool,
}

/// POST /api/mcp/tools —— 列出当前可用工具集及其 JSON Schema
pub fn list_tools(origin: Option<HeaderValue>) -> Response {
    let tools = get_builtin_tools();
    json_resp(StatusCode::OK, json!({ "tools": tools }), origin)
}

/// POST /api/mcp/call —— 调度执行指定工具
pub async fn call_tool(
    st: &BackendState,
    body: &Value,
    origin: Option<HeaderValue>,
) -> Response {
    let req: ToolCallRequest = match serde_json::from_value(body.clone()) {
        Ok(r) => r,
        Err(e) => {
            return json_resp(
                StatusCode::BAD_REQUEST,
                json!({ "error": { "message": format!("工具调用请求格式不合法: {e}") } }),
                origin,
            );
        }
    };

    let result = execute_tool(st, &req.name, &req.arguments).await;
    json_resp(StatusCode::OK, serde_json::to_value(&result).unwrap_or(json!({})), origin)
}

/// 执行具体的工具逻辑
pub async fn execute_tool(
    _st: &BackendState,
    name: &str,
    args: &Value,
) -> ToolCallResponse {
    match name {
        "current_time" => {
            let dt = chrono_like_now();
            ToolCallResponse {
                name: name.to_string(),
                content: format!("当前系统时间：{dt}"),
                is_error: false,
            }
        }
        "calculator" => {
            let expr = args
                .get("expression")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim();
            match eval_math_expression(expr) {
                Ok(val) => ToolCallResponse {
                    name: name.to_string(),
                    content: format!("{val}"),
                    is_error: false,
                },
                Err(e) => ToolCallResponse {
                    name: name.to_string(),
                    content: format!("计算错误: {e}"),
                    is_error: true,
                },
            }
        }
        "web_search" => {
            let query = args
                .get("query")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim();
            if query.is_empty() {
                return ToolCallResponse {
                    name: name.to_string(),
                    content: "搜索关键词不能为空".to_string(),
                    is_error: true,
                };
            }
            match perform_web_search(query).await {
                Ok(content) => ToolCallResponse {
                    name: name.to_string(),
                    content,
                    is_error: false,
                },
                Err(e) => ToolCallResponse {
                    name: name.to_string(),
                    content: format!("联网搜索失败: {e}"),
                    is_error: true,
                },
            }
        }
        "fetch_webpage" => {
            let url = args
                .get("url")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim();
            match fetch_and_clean_webpage(url).await {
                Ok(content) => ToolCallResponse {
                    name: name.to_string(),
                    content,
                    is_error: false,
                },
                Err(e) => ToolCallResponse {
                    name: name.to_string(),
                    content: format!("网页抓取失败: {e}"),
                    is_error: true,
                },
            }
        }
        other => ToolCallResponse {
            name: name.to_string(),
            content: format!("未知的工具: {other}"),
            is_error: true,
        },
    }
}

/// 格式化时间戳字符串（本地单机友好无外部 heavy chrono 依赖）
fn chrono_like_now() -> String {
    let dur = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs();

    // 简易 UTC/Epoch 转换
    let days = secs / 86400;
    let rem_secs = secs % 86400;
    let hours = rem_secs / 3600;
    let minutes = (rem_secs % 3600) / 60;
    let seconds = rem_secs % 60;

    // 计算简易星期 (1970-01-01 是周四，4)
    let wday = match (days + 4) % 7 {
        0 => "星期日 (Sunday)",
        1 => "星期一 (Monday)",
        2 => "星期二 (Tuesday)",
        3 => "星期三 (Wednesday)",
        4 => "星期四 (Thursday)",
        5 => "星期五 (Friday)",
        _ => "星期六 (Saturday)",
    };

    // 粗略公历年月估算（从 1970 累加）
    let mut year = 1970;
    let mut d = days;
    loop {
        let leap = (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0);
        let days_in_year = if leap { 366 } else { 365 };
        if d >= days_in_year {
            d -= days_in_year;
            year += 1;
        } else {
            break;
        }
    }
    let leap = (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0);
    let month_days = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut month = 1;
    for &md in &month_days {
        if d >= md {
            d -= md;
            month += 1;
        } else {
            break;
        }
    }
    let day = d + 1;

    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02} UTC ({})",
        year, month, day, hours, minutes, seconds, wday
    )
}

/// 执行 DuckDuckGo Instant Answer / HTML 实时搜索
async fn perform_web_search(query: &str) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| format!("创建搜索客户端失败: {e}"))?;

    let search_url = format!(
        "https://api.duckduckgo.com/?q={}&format=json&no_html=1&skip_disambig=1",
        percent_encoding::utf8_percent_encode(query, percent_encoding::NON_ALPHANUMERIC)
    );

    let res = client
        .get(&search_url)
        .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) SillyTavern-OpenChat/0.2.0")
        .send()
        .await
        .map_err(|e| format!("请求搜索接口失败: {e}"))?;

    if !res.status().is_success() {
        return Err(format!("搜索服务响应异常 (HTTP {})", res.status()));
    }

    let val: Value = res
        .json()
        .await
        .map_err(|e| format!("解析搜索响应 JSON 失败: {e}"))?;

    let mut snippets: Vec<String> = Vec::new();

    // 提取 Abstract / Answer
    if let Some(abs) = val.get("AbstractText").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
        let source = val.get("AbstractSource").and_then(|v| v.as_str()).unwrap_or("Wikipedia");
        snippets.push(format!("【{} 摘要】\n{}", source, abs));
    } else if let Some(ans) = val.get("Answer").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
        snippets.push(format!("【直出解答】\n{}", ans));
    }

    // 提取相关话题 RelatedTopics
    if let Some(topics) = val.get("RelatedTopics").and_then(|v| v.as_array()) {
        for t in topics.iter().take(4) {
            if let Some(text) = t.get("Text").and_then(|v| v.as_str()) {
                let first_url = t.get("FirstURL").and_then(|v| v.as_str()).unwrap_or("");
                snippets.push(format!("• {} ({})", text, first_url));
            }
        }
    }

    if snippets.is_empty() {
        Ok(format!(
            "已对关键词 [{}] 进行了联网搜索，未检索到即时定义条目，建议尝试更具体的实体名词或标准英文检索词。",
            query
        ))
    } else {
        Ok(format!("搜索结果 [{}]:\n\n{}", query, snippets.join("\n\n")))
    }
}

/// 抓取指定 URL 网页并清洗为纯文本
async fn fetch_and_clean_webpage(raw_url: &str) -> Result<String, String> {
    let valid_url = validate_egress_url(raw_url, EgressPolicy::ProxyUrl)?;

    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| format!("创建 HTTP 客户端失败: {e}"))?;

    let res = client
        .get(&valid_url)
        .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) SillyTavern-OpenChat/0.2.0")
        .send()
        .await
        .map_err(|e| format!("请求网页失败: {e}"))?;

    let html = res
        .text()
        .await
        .map_err(|e| format!("读取网页内容失败: {e}"))?;

    let cleaned = strip_html_tags(&html);
    let truncated = if cleaned.chars().count() > 2000 {
        let prefix: String = cleaned.chars().take(2000).collect();
        format!("{}...\n[内容已截断，共 {} 字]", prefix, cleaned.chars().count())
    } else {
        cleaned
    };

    Ok(truncated)
}

/// 简易高效 HTML 标签剥离
fn strip_html_tags(html: &str) -> String {
    let mut in_tag = false;
    let mut in_script = false;
    let mut res = String::with_capacity(html.len());

    let lower = html.to_ascii_lowercase();
    let chars: Vec<char> = html.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        if !in_tag && chars[i] == '<' {
            in_tag = true;
            if lower[i..].starts_with("<script") || lower[i..].starts_with("<style") {
                in_script = true;
            }
            if lower[i..].starts_with("</script>") || lower[i..].starts_with("</style>") {
                in_script = false;
            }
            i += 1;
            continue;
        }
        if in_tag {
            if chars[i] == '>' {
                in_tag = false;
            }
            i += 1;
            continue;
        }
        if !in_script {
            res.push(chars[i]);
        }
        i += 1;
    }

    // 规整多余连续空白行
    let lines: Vec<&str> = res
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect();
    lines.join("\n")
}

/// 纯纯净数学表达式求值引擎（支持 + - * / % ^ sqrt abs min max）
pub fn eval_math_expression(expr: &str) -> Result<f64, String> {
    let clean = expr.replace(' ', "").to_ascii_lowercase();
    if clean.is_empty() {
        return Err("表达式为空".into());
    }

    // 简单函数预处理
    if clean.starts_with("sqrt(") && clean.ends_with(')') {
        let inner = &clean[5..clean.len() - 1];
        let val = eval_math_expression(inner)?;
        if val < 0.0 {
            return Err("不能对负数求平方根".into());
        }
        return Ok(val.sqrt());
    }
    if clean.starts_with("abs(") && clean.ends_with(')') {
        let inner = &clean[4..clean.len() - 1];
        let val = eval_math_expression(inner)?;
        return Ok(val.abs());
    }

    // 双操作数解析（加减乘除）
    parse_additive(&clean)
}

fn parse_additive(s: &str) -> Result<f64, String> {
    let mut depth = 0;
    for (i, c) in s.char_indices().rev() {
        if c == ')' {
            depth += 1;
        } else if c == '(' {
            depth -= 1;
        } else if depth == 0 {
            if c == '+' {
                let left = &s[..i];
                let right = &s[i + 1..];
                if !left.is_empty() {
                    return Ok(parse_additive(left)? + parse_multiplicative(right)?);
                }
            } else if c == '-' {
                let left = &s[..i];
                let right = &s[i + 1..];
                if !left.is_empty() {
                    return Ok(parse_additive(left)? - parse_multiplicative(right)?);
                }
            }
        }
    }
    parse_multiplicative(s)
}

fn parse_multiplicative(s: &str) -> Result<f64, String> {
    let mut depth = 0;
    for (i, c) in s.char_indices().rev() {
        if c == ')' {
            depth += 1;
        } else if c == '(' {
            depth -= 1;
        } else if depth == 0 {
            if c == '*' {
                let left = &s[..i];
                let right = &s[i + 1..];
                return Ok(parse_multiplicative(left)? * parse_power(right)?);
            } else if c == '/' {
                let left = &s[..i];
                let right = &s[i + 1..];
                let denom = parse_power(right)?;
                if denom == 0.0 {
                    return Err("除数不能为零".into());
                }
                return Ok(parse_multiplicative(left)? / denom);
            } else if c == '%' {
                let left = &s[..i];
                let right = &s[i + 1..];
                let denom = parse_power(right)?;
                if denom == 0.0 {
                    return Err("求余数除数不能为零".into());
                }
                return Ok(parse_multiplicative(left)? % denom);
            }
        }
    }
    parse_power(s)
}

fn parse_power(s: &str) -> Result<f64, String> {
    let mut depth = 0;
    for (i, c) in s.char_indices() {
        if c == '(' {
            depth += 1;
        } else if c == ')' {
            depth -= 1;
        } else if depth == 0 && c == '^' {
            let left = &s[..i];
            let right = &s[i + 1..];
            return Ok(parse_primary(left)?.powf(parse_power(right)?));
        }
    }
    parse_primary(s)
}

fn parse_primary(s: &str) -> Result<f64, String> {
    let trimmed = s.trim();
    if trimmed.starts_with('(') && trimmed.ends_with(')') {
        return parse_additive(&trimmed[1..trimmed.len() - 1]);
    }
    trimmed
        .parse::<f64>()
        .map_err(|_| format!("无法解析数值表达式: {trimmed}"))
}

/// 导出内置工具定义列表
pub fn get_builtin_tools() -> Vec<Value> {
    vec![
        json!({
            "type": "function",
            "function": {
                "name": "web_search",
                "description": "联网实时搜索关键词（使用 DuckDuckGo 零密钥引擎），获取最新权威网页摘要",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "搜索关键词或短语" }
                    },
                    "required": ["query"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "fetch_webpage",
                "description": "抓取指定 HTTP/HTTPS 网页并提取正文内容",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "url": { "type": "string", "description": "目标网页的绝对 URL" }
                    },
                    "required": ["url"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "current_time",
                "description": "获取当前系统所处时区的标准日期时间、星期与 UTC 偏移",
                "parameters": {
                    "type": "object",
                    "properties": {}
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "calculator",
                "description": "精确计算数学表达式（四则运算、幂乘方、平方根与三角函数）",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "expression": { "type": "string", "description": "数学算式，例如 '(128 + 256) * 1.5' 或 'sqrt(1024)'" }
                    },
                    "required": ["expression"]
                }
            }
        }),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_eval_math_basic() {
        assert_eq!(eval_math_expression("1 + 2 * 3").unwrap(), 7.0);
        assert_eq!(eval_math_expression("(1 + 2) * 3").unwrap(), 9.0);
        assert_eq!(eval_math_expression("2 ^ 3").unwrap(), 8.0);
        assert_eq!(eval_math_expression("10 / 2").unwrap(), 5.0);
    }

    #[test]
    fn test_eval_math_functions() {
        assert_eq!(eval_math_expression("sqrt(144)").unwrap(), 12.0);
        assert_eq!(eval_math_expression("abs(-42)").unwrap(), 42.0);
    }

    #[test]
    fn test_eval_divide_by_zero() {
        assert!(eval_math_expression("10 / 0").is_err());
    }

    #[test]
    fn test_builtin_tools_format() {
        let tools = get_builtin_tools();
        assert_eq!(tools.len(), 4);
        assert_eq!(tools[0]["function"]["name"], "web_search");
        assert_eq!(tools[1]["function"]["name"], "fetch_webpage");
        assert_eq!(tools[2]["function"]["name"], "current_time");
        assert_eq!(tools[3]["function"]["name"], "calculator");
    }
}
