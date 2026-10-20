//! backend/providers —— 生成通道网关（Phase 2 全量 · manifest #41/#42）
//!
//! 覆盖（ST `backends/chat-completions.js` 26 源全量）：
//! - **独立协议 sender**：claude（providers_claude）/ makersuite+vertexai
//!   （providers_google）/ mistralai、ai21、deepseek、xai、aimlapi、
//!   electronhub、chutes、minimax、azure_openai、cohere（providers_misc）
//! - **OpenAI 兼容主路**（本文件 generic）：openai、openrouter、custom、
//!   groq、fireworks、nanogpt、pollinations、moonshot、zai、siliconflow、
//!   workers_ai、perplexity —— per-source bodyParams/headers/URL 分流
//! - cometapi：ST 1.19 硬禁用（后端 throw），原样复刻（502 可判定错误）
//!
//! 已核对契约（status :1778-2156 / generate :2242-2729）：
//! - /status {chat_completion_source, custom_url?}：成功 → 上游 JSON 原样；
//!   密钥缺失 → 400；上游故障 → 200 + {error:true,...}（ST 字面；
//!   makersuite/vertexai 加 bypass:true —— api.ts:533）
//! - /generate：stream=true → 上游响应体透传（SSE 不缓冲，S14 显式
//!   accept-encoding identity）；401 流式降级 400（forwardFetchResponse
//!   字面）；非流式：generic 族 = E1 上游错误原样注释透传（Phase 1 定档），
//!   独立 sender = 500 + tryParse ?? {error:true}
//! - S12/HIGH-5：用户可控出站 URL 全走 `validate_egress_url` 网关
//!   （SelfHosted＝用户自建/自填端点：custom_url、textcomp api_server、translate
//!   三家自建 URL —— http/https 均可且放行私网/环回，仅拦 link-local；
//!   ProxyUrl＝reverse_proxy / azure_base_url —— 仅 https 且拒私网）
//! - 密钥只从服务端 secrets 读（api_key_<source>；S8 面）
//! - 输出面偏差登记：claude 流（Anthropic SSE→OpenAI chunk）、cohere 流/非流
//!   （Cohere v2→OpenAI 形）—— App 前端 stStream/stGenerateBlocking 双解析器
//!   约束；请求面保持 ST 字面

use super::{apply_cors_resp, BackendState};
use axum::body::Body;
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use futures_util::StreamExt;
use serde_json::{json, Map, Value};

/* ================================================================ *
 * API 常量（chat-completions.js:73-101 逐字对齐）
 * ================================================================ */
pub(crate) const API_OPENAI: &str = "https://api.openai.com/v1";
pub(crate) const API_CLAUDE: &str = "https://api.anthropic.com/v1";
pub(crate) const API_MISTRAL: &str = "https://api.mistral.ai/v1";
pub(crate) const API_COHERE_V1: &str = "https://api.cohere.ai/v1";
pub(crate) const API_COHERE_V2: &str = "https://api.cohere.ai/v2";
pub(crate) const API_PERPLEXITY: &str = "https://api.perplexity.ai";
pub(crate) const API_GROQ: &str = "https://api.groq.com/openai/v1";
pub(crate) const API_MAKERSUITE: &str = "https://generativelanguage.googleapis.com";
pub(crate) const API_AI21: &str = "https://api.ai21.com/studio/v1";
pub(crate) const API_CHUTES: &str = "https://llm.chutes.ai/v1";
pub(crate) const API_ELECTRONHUB: &str = "https://api.electronhub.ai/v1";
pub(crate) const API_NANOGPT: &str = "https://nano-gpt.com/api/v1";
pub(crate) const API_DEEPSEEK: &str = "https://api.deepseek.com/beta";
pub(crate) const API_DEEPSEEK_STATUS: &str = "https://api.deepseek.com";
pub(crate) const API_XAI: &str = "https://api.x.ai/v1";
pub(crate) const API_AIMLAPI: &str = "https://api.aimlapi.com/v1";
pub(crate) const API_POLLINATIONS: &str = "https://gen.pollinations.ai/v1";
pub(crate) const API_POLLINATIONS_ANON: &str = "https://text.pollinations.ai/v1";
pub(crate) const API_MOONSHOT: &str = "https://api.moonshot.ai/v1";
pub(crate) const API_FIREWORKS: &str = "https://api.fireworks.ai/inference/v1";
pub(crate) const API_ZAI_COMMON: &str = "https://api.z.ai/api/paas/v4";
pub(crate) const API_ZAI_CODING: &str = "https://api.z.ai/api/coding/paas/v4";
pub(crate) const API_SILICONFLOW: &str = "https://api.siliconflow.com/v1";
pub(crate) const API_SILICONFLOW_CN: &str = "https://api.siliconflow.cn/v1";
pub(crate) const API_MINIMAX: &str = "https://api.minimax.io/v1";
pub(crate) const API_MINIMAX_CN: &str = "https://api.minimaxi.com/v1";
pub(crate) const API_OPENROUTER: &str = "https://openrouter.ai/api/v1";
pub(crate) const API_WORKERS_AI: &str = "https://api.cloudflare.com/client/v4/accounts";

/* ================================================================ *
 * 通用响应 / 共享小件
 * ================================================================ */

pub(crate) fn err_json(status: StatusCode, msg: &str) -> Response {
    log::warn!(target: super::logx::PROVIDER, "provider 错误 {status}: {}", super::logx::redact(msg));
    (status, Json(json!({ "error": { "message": msg } }))).into_response()
}

pub(crate) fn json_resp(status: StatusCode, v: Value, origin: Option<HeaderValue>) -> Response {
    apply_cors_resp((status, Json(v)).into_response(), origin)
}

pub(crate) fn not_migrated(source: &str, origin: Option<HeaderValue>) -> Response {
    log::warn!(target: super::logx::PROVIDER, "生成源未支持: {}", super::logx::redact(source));
    let resp = (
        StatusCode::NOT_IMPLEMENTED,
        Json(json!({
            "error": {
                "message": format!("生成源 {source} 尚未支持（Provider 全量对齐中）"),
                "code": "NOT_MIGRATED",
            }
        })),
    );
    apply_cors_resp(resp.into_response(), origin)
}

/// 服务端读密钥（T12 vault）：keychain/settings 权威 → 镜像回落（vault.rs）
pub(crate) fn read_secret(st: &BackendState, key: &str) -> Option<String> {
    st.vault.read_active(st, key)
}

/// 出站统一日志（源/模型/流式/消息数）—— 各 send_* 入口调用，避免逐个手写格式。
/// 只记元信息，不记 body（S13：请求体可能含私密对话）。
pub(crate) fn log_outbound(source: &str, body: &Value) {
    let model = body.get("model").and_then(|v| v.as_str()).unwrap_or("-");
    let stream = body.get("stream").and_then(|v| v.as_bool()).unwrap_or(false);
    let n = body
        .get("messages")
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(0);
    log::info!(
        target: super::logx::PROVIDER,
        "出站 source={} model={} stream={stream} messages={n}",
        super::logx::redact(source),
        super::logx::redact(model)
    );
}

/// JSON.stringify(value, null, 4)（4 空格缩进）
pub(crate) fn json_pretty4(v: &Value) -> String {
    let fmt = serde_json::ser::PrettyFormatter::with_indent(b"    ");
    let mut ser = serde_json::Serializer::with_formatter(Vec::new(), fmt);
    let _ = serde::Serialize::serialize(v, &mut ser);
    String::from_utf8_lossy(&ser.into_inner()).to_string()
}

/* ================================================================ *
 * S12/HIGH-5 统一出站网关（SSRF 面）
 * ================================================================ */

/// 出站 URL 策略（HIGH-5）
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum EgressPolicy {
    /// 用户自建/自填端点：custom_url（自定义 OpenAI 兼容）、textcomp api_server
    /// （KoboldCpp / llama.cpp 等）、translate 三家自建 URL（libre_url / deeplx_url /
    /// oneringtranslator_url）。**http/https 均可**，且放行环回与私网 —— 本机与局域网
    /// 部署是典型形态（http://127.0.0.1:1234/v1、http://192.168.1.50:5000/v1）。
    /// 仍拦 link-local（169.254/16、fe80::/10）与 0.0.0.0（云元数据面）。
    SelfHosted,
    /// 代理面（reverse_proxy / azure_base_url）：仅 https，且私网一律拒
    ProxyUrl,
}

/// 禁用网段（字面与解析后同判）：10/8、172.16/12、192.168/16、169.254/16、
/// 0.0.0.0、fc00::/7、fe80::/10
fn ip_banned(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => {
            let [a, b, _, _] = v4.octets();
            a == 10
                || (a == 172 && (16..=31).contains(&b))
                || (a == 192 && b == 168)
                || (a == 169 && b == 254)
                || v4.is_unspecified()
        }
        std::net::IpAddr::V6(v6) => {
            let seg = v6.segments();
            (seg[0] & 0xfe00) == 0xfc00 // fc00::/7
                || (seg[0] & 0xffc0) == 0xfe80 // fe80::/10
        }
    }
}

/// link-local / 未指定地址（云元数据 169.254.169.254 与 0.0.0.0 面）——
/// 即使自建端点策略也拦
fn ip_link_local_or_unspecified(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => {
            let [a, b, _, _] = v4.octets();
            (a == 169 && b == 254) || v4.is_unspecified()
        }
        std::net::IpAddr::V6(v6) => (v6.segments()[0] & 0xffc0) == 0xfe80,
    }
}

/// 按策略判定禁用网段：自建端点放行环回/私网，代理面用严格表
fn ip_banned_for(ip: std::net::IpAddr, policy: EgressPolicy) -> bool {
    match policy {
        EgressPolicy::SelfHosted => ip_link_local_or_unspecified(ip),
        EgressPolicy::ProxyUrl => ip_banned(ip),
    }
}

/// 真实解析器（ToSocketAddrs；由调用方在 async 语境自行权衡阻塞成本）
fn resolve_host_ips(host: &str, port: u16) -> std::io::Result<Vec<std::net::IpAddr>> {
    use std::net::ToSocketAddrs;
    Ok((host, port).to_socket_addrs()?.map(|s| s.ip()).collect())
}

/// 统一出站网关（HIGH-5/S12）：
/// - scheme 必须 http/https。http 放行面：SelfHosted（用户自建/自填端点：custom_url、
///   textcomp api_server、translate 三家自建 URL）http/https 均可；ProxyUrl
///   （reverse_proxy / azure_base_url）一律 https。
/// - 禁用网段按策略：SelfHosted 放行环回/私网，仅拦 link-local 与 0.0.0.0；
///   ProxyUrl 拒绝字面私网/link-local（含 https 指向私网）。
/// - hostname 非字面 IP 时解析（ToSocketAddrs），任一 IP 落禁用段即拒
///   （解析失败 fail-closed：无法核验的出站一律不放行）。
///
/// 成功返回规范化 URL（去尾斜杠，可直接作 api_root 拼 path）。
pub(crate) fn validate_egress_url(raw: &str, policy: EgressPolicy) -> Result<String, String> {
    validate_egress_url_with(raw, policy, resolve_host_ips)
}

/// 可注入解析器的网关本体（单测用假解析器，不打真实 DNS）
pub(crate) fn validate_egress_url_with<F>(
    raw: &str,
    policy: EgressPolicy,
    resolve: F,
) -> Result<String, String>
where
    F: Fn(&str, u16) -> std::io::Result<Vec<std::net::IpAddr>>,
{
    let url = reqwest::Url::parse(raw.trim())
        .map_err(|_| "URL 解析失败（需 http/https 绝对地址）".to_string())?;
    let scheme = url.scheme();
    if scheme != "https" && scheme != "http" {
        return Err(format!("出站 URL 协议不合法: {scheme}"));
    }
    let host_raw = url
        .host_str()
        .filter(|h| !h.is_empty())
        .ok_or_else(|| "出站 URL 缺少主机名".to_string())?;
    // url crate 对 IPv6 字面量 host_str 带 方括号（"[::1]"）→ 剥掉再判
    let host = host_raw.trim_start_matches('[').trim_end_matches(']');
    // http 放行面按策略：自建端点（SelfHosted）http/https 均可；代理面（ProxyUrl）一律 https
    if scheme == "http" && policy == EgressPolicy::ProxyUrl {
        return Err("出站 URL 仅允许 https（反向代理 / Azure Base URL 面；自建端点可填 http）".into());
    }
    let port = url.port_or_known_default().unwrap_or(0);
    match host.parse::<std::net::IpAddr>() {
        Ok(ip) => {
            if ip_banned_for(ip, policy) {
                return Err(format!("出站 URL 指向禁用网段: {ip}"));
            }
        }
        Err(_) => {
            let ips = resolve(host, port).map_err(|e| format!("出站 URL 域名解析失败: {e}"))?;
            if ips.is_empty() {
                return Err("出站 URL 域名解析结果为空".into());
            }
            if ips.iter().any(|ip| ip_banned_for(*ip, policy)) {
                return Err("出站 URL 域名解析到禁用网段".into());
            }
        }
    }
    Ok(url.to_string().trim_end_matches('/').to_string())
}

/// flattenSchema（util.js:1459 收敛面）—— App 全源不发 tools/json_schema 时为
/// no-op；保留 $schema 键剥离以防直连调用。
pub(crate) fn flatten_schema(schema: &Value, _api: &str) -> Value {
    fn strip(v: &mut Value) {
        match v {
            Value::Object(m) => {
                m.remove("$schema");
                for (_, child) in m.iter_mut() {
                    strip(child);
                }
            }
            Value::Array(a) => {
                for child in a.iter_mut() {
                    strip(child);
                }
            }
            _ => {}
        }
    }
    let mut v = schema.clone();
    strip(&mut v);
    v
}

pub(crate) fn aimlapi_headers() -> Vec<(String, String)> {
    vec![
        ("HTTP-Referer".into(), "https://github.com/Steven-DD/SillyTavern-OpenChat".into()),
        ("X-Title".into(), "SillyTavern-OpenChat".into()),
    ]
}

pub(crate) fn openrouter_headers() -> Vec<(String, String)> {
    aimlapi_headers()
}

pub(crate) fn zai_headers() -> Vec<(String, String)> {
    vec![("Accept-Language".into(), "en-US,en".into())]
}

/// Claude 出站头构造（HIGH-3，ST chat-completions.js:246-247/:404-406/:414-419
/// 字面）：x-api-key（非 Bearer）+ anthropic-version + 无条件 anthropic-beta
/// 基线（output-128k/context-1m）；useTools 追加 tools-2024-05-16（ST :290）。
/// reverse_proxy 分支同形 —— ST :232 仅换 key 来源，fetch 头共用一套（:410-420）。
pub(crate) fn claude_outbound_headers(key: &str, use_tools: bool) -> Vec<(String, String)> {
    let mut beta: Vec<&str> = vec!["output-128k-2025-02-19", "context-1m-2025-08-07"];
    if use_tools {
        beta.push("tools-2024-05-16");
    }
    vec![
        ("anthropic-version".into(), "2023-06-01".into()),
        ("x-api-key".into(), key.to_string()),
        ("anthropic-beta".into(), beta.join(",")),
    ]
}

pub(crate) fn gemini_safety() -> Value {
    json!([
        { "category": "HARM_CATEGORY_HARASSMENT", "threshold": "OFF" },
        { "category": "HARM_CATEGORY_HATE_SPEECH", "threshold": "OFF" },
        { "category": "HARM_CATEGORY_SEXUALLY_EXPLICIT", "threshold": "OFF" },
        { "category": "HARM_CATEGORY_DANGEROUS_CONTENT", "threshold": "OFF" },
        { "category": "HARM_CATEGORY_CIVIC_INTEGRITY", "threshold": "OFF" },
    ])
}

/* ================================================================ *
 * 出站共享件
 * ================================================================ */

async fn post_client() -> reqwest::Client {
    reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap_or_default()
}

/// S13：reqwest 错误 Display 内嵌完整上游 URL（可含 key 查询串，如 gemini
/// ?key=… / vertexai express）→ 统一剥除 URL，保留状态码与错误描述。
pub(crate) fn fmt_upstream_err(e: &reqwest::Error) -> String {
    let display = e.to_string();
    let Some(u) = e.url() else {
        return display;
    };
    let raw = u.as_str();
    let tail = format!(" for url ({raw})");
    if display.contains(&tail) {
        display.replace(&tail, "")
    } else {
        display.replace(raw, "[URL 已剥除]")
    }
}

/// 通用 POST（Bearer 鉴权；key 为空不带头 —— custom 源匿名放行 ST 字面）+
/// S14 identity + 300s 出站保护。
pub(crate) async fn upstream_post(
    url: &str,
    key: &str,
    headers: Vec<(String, String)>,
    body: &Value,
) -> Result<reqwest::Response, String> {
    let client = post_client().await;
    let mut req = client
        .post(url)
        .header("Content-Type", "application/json")
        .header("accept-encoding", "identity")
        .timeout(std::time::Duration::from_secs(300));
    if !key.is_empty() {
        req = req.header("Authorization", format!("Bearer {key}"));
    }
    for (k, v) in headers {
        req = req.header(k.as_str(), v.as_str());
    }
    req.json(body)
        .send()
        .await
        .map_err(|e| fmt_upstream_err(&e))
}

/// Claude 专用出站（HIGH-3，ST chat-completions.js:410-420 字面）：
/// x-api-key 头（非 Bearer）+ anthropic-version + 无条件 anthropic-beta。
pub(crate) async fn upstream_post_claude(
    url: &str,
    key: &str,
    use_tools: bool,
    body: &Value,
) -> Result<reqwest::Response, String> {
    let client = post_client().await;
    let mut req = client
        .post(url)
        .header("Content-Type", "application/json")
        .header("accept-encoding", "identity")
        .timeout(std::time::Duration::from_secs(300));
    for (k, v) in claude_outbound_headers(key, use_tools) {
        req = req.header(k.as_str(), v.as_str());
    }
    req.json(body)
        .send()
        .await
        .map_err(|e| fmt_upstream_err(&e))
}

/// Azure：`api-key` 头（非 Bearer）
pub(crate) async fn upstream_post_azure(
    url: &str,
    key: &str,
    body: &Value,
) -> Result<reqwest::Response, String> {
    let client = post_client().await;
    client
        .post(url)
        .header("Content-Type", "application/json")
        .header("accept-encoding", "identity")
        .header("api-key", key)
        .timeout(std::time::Duration::from_secs(300))
        .json(body)
        .send()
        .await
        .map_err(|e| fmt_upstream_err(&e))
}

/// 401 → 400 降级（forwardFetchResponse 防 Basic-Auth 重置字面）
pub(crate) fn downgrade_status(status: StatusCode) -> StatusCode {
    if status == StatusCode::UNAUTHORIZED {
        StatusCode::BAD_REQUEST
    } else {
        status
    }
}

/// 原样透传：保留上游状态码（401→400 降级）+ 逐字节流（SSE 不缓冲 S14）
pub(crate) async fn passthrough(
    upstream: reqwest::Response,
    origin: Option<HeaderValue>,
) -> Response {
    let status = downgrade_status(upstream.status());
    let ct = upstream
        .headers()
        .get("content-type")
        .cloned()
        .unwrap_or_else(|| HeaderValue::from_static("application/octet-stream"));
    let stream = upstream
        .bytes_stream()
        .map(|r| r.map_err(std::io::Error::other));
    apply_cors_resp(
        Response::builder()
            .status(status)
            .header("content-type", ct)
            .body(Body::from_stream(stream))
            .unwrap()
            .into_response(),
        origin,
    )
}

/// SSE 转换动作
pub(crate) enum SseAction {
    /// 发一帧目标格式 data JSON
    Frame(Value),
    /// 不发帧（跳过该上游事件）
    FrameOnly,
    /// 终止流（发送 data: [DONE] 后关闭）
    Done,
}

/// 上游 SSE → 目标格式 SSE 的逐事件转换透传（逐块处理，不整流缓冲）
pub(crate) async fn transform_sse_reply(
    upstream: reqwest::Response,
    origin: Option<HeaderValue>,
    map: fn(&Value) -> SseAction,
) -> Response {
    struct SseState {
        stream: std::pin::Pin<
            Box<dyn futures_util::Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Send>,
        >,
        buf: Vec<u8>,
        finished: bool,
    }

    let stream = Box::pin(upstream.bytes_stream());
    let sse = futures_util::stream::unfold(
        (
            map,
            SseState {
                stream,
                buf: Vec::new(),
                finished: false,
            },
        ),
        |(map, mut st)| async move {
            if st.finished {
                return None;
            }
            loop {
                // 冲刷缓冲里的完整事件块并保留已产出帧
                let mut out: Vec<u8> = Vec::new();
                while let Some(pos) = find_sse_sep(&st.buf) {
                    let sep_len = sse_sep_len(&st.buf, pos);
                    let block: Vec<u8> = st.buf.drain(0..pos + sep_len).collect();
                    for payload in data_lines(&block) {
                        if payload == "[DONE]" {
                            st.finished = true;
                            out.extend_from_slice(b"data: [DONE]\n\n");
                            return Some((Ok(out), (map, st)));
                        }
                        if let Ok(ev) = serde_json::from_slice::<Value>(payload.as_bytes()) {
                            match map(&ev) {
                                SseAction::Frame(f) => {
                                    out.extend_from_slice(format!("data: {f}\n\n").as_bytes())
                                }
                                SseAction::FrameOnly => {}
                                SseAction::Done => {
                                    st.finished = true;
                                    out.extend_from_slice(b"data: [DONE]\n\n");
                                    return Some((Ok(out), (map, st)));
                                }
                            }
                        }
                    }
                }
                if !out.is_empty() {
                    return Some((Ok(out), (map, st)));
                }
                match st.stream.next().await {
                    Some(Ok(chunk)) => st.buf.extend_from_slice(chunk.as_ref()),
                    Some(Err(e)) => {
                        st.finished = true;
                        let io = std::io::Error::other(e);
                        return Some((Err(io), (map, st)));
                    }
                    None => {
                        // 上游结束：冲掉残余尾块（无分隔符）
                        st.finished = true;
                        let tail = std::mem::take(&mut st.buf);
                        let mut out: Vec<u8> = Vec::new();
                        for payload in data_lines(&tail) {
                            if payload.is_empty() {
                                continue;
                            }
                            if let Ok(ev) = serde_json::from_slice::<Value>(payload.as_bytes()) {
                                if let SseAction::Frame(f) = map(&ev) {
                                    out.extend_from_slice(format!("data: {f}\n\n").as_bytes());
                                }
                            }
                        }
                        out.extend_from_slice(b"data: [DONE]\n\n");
                        return Some((Ok(out), (map, st)));
                    }
                }
            }
        },
    );

    apply_cors_resp(
        Response::builder()
            .status(StatusCode::OK)
            .header("Content-Type", "text/event-stream")
            .body(Body::from_stream(sse))
            .unwrap()
            .into_response(),
        origin,
    )
}

fn sse_sep_len(buf: &[u8], pos: usize) -> usize {
    if buf.get(pos) == Some(&b'\n') && buf.get(pos + 1) == Some(&b'\n') {
        return 2;
    }
    if buf.get(pos) == Some(&b'\r')
        && buf.get(pos + 1) == Some(&b'\n')
        && buf.get(pos + 3) == Some(&b'\n')
    {
        return 4;
    }
    2
}

fn find_sse_sep(buf: &[u8]) -> Option<usize> {
    buf.windows(2)
        .position(|w| w == b"\n\n")
        .or_else(|| buf.windows(4).position(|w| w == b"\r\n\r\n"))
}

fn data_lines(block: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(block);
    let mut out = Vec::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("data:") {
            out.push(rest.strip_prefix(' ').unwrap_or(rest).to_string());
        }
    }
    out
}

/* ================================================================ *
 * 独立 sender 的统一转发（错误语义 ST 字面）
 * ================================================================ */

#[derive(Clone, Copy)]
pub(crate) enum OutShape {
    /// OpenAI 兼容原生（原样）
    Pass,
    /// Cohere v2 → OpenAI 形（App 解析器约束）
    Cohere,
}

pub(crate) async fn relay_independent(
    upstream: Result<reqwest::Response, String>,
    stream: bool,
    origin: Option<HeaderValue>,
    shape: OutShape,
) -> Response {
    let resp = match upstream {
        Ok(r) => r,
        // 连接失败（catch 分支）→ 500 {error:true}
        Err(e) => {
            log::error!(target: super::logx::PROVIDER, "上游连接失败: {}", super::logx::redact(&e));
            return json_resp(
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({ "error": true }),
                origin,
            );
        }
    };
    let status_ok = resp.status().is_success();
    log::info!(
        target: super::logx::PROVIDER,
        "上游响应 status={} stream={stream} ok={status_ok}",
        resp.status().as_u16()
    );

    if stream {
        if status_ok {
            return match shape {
                OutShape::Pass => passthrough(resp, origin).await,
                OutShape::Cohere => transform_sse_reply(resp, origin, cohere_to_openai).await,
            };
        }
        log::warn!(target: super::logx::PROVIDER, "流式上游非 2xx（{}）→ 原样透传", resp.status().as_u16());
        return passthrough(resp, origin).await;
    }

    let text = resp.text().await.unwrap_or_default();
    if !status_ok {
        // ST 字面：500 + tryParse(errorText) ?? {error:true}
        let parsed = serde_json::from_str::<Value>(&text).unwrap_or(json!({ "error": true }));
        // 只记错误摘要字段（不记完整上游 body —— 部分上游会在 4xx 里回显请求内容）
        let msg = parsed
            .get("error")
            .and_then(|e| e.get("message").and_then(|m| m.as_str()).or_else(|| e.as_str()))
            .or_else(|| parsed.get("message").and_then(|m| m.as_str()))
            .unwrap_or("");
        let brief: String = super::logx::redact(msg).chars().take(300).collect();
        log::warn!(target: super::logx::PROVIDER, "上游非 2xx 错误摘要: {brief}");
        return json_resp(StatusCode::INTERNAL_SERVER_ERROR, parsed, origin);
    }
    let v = serde_json::from_str::<Value>(&text).unwrap_or(json!({}));
    match shape {
        OutShape::Pass => json_resp(StatusCode::OK, v, origin),
        OutShape::Cohere => json_resp(StatusCode::OK, wrap_cohere_response(v), origin),
    }
}

/// Cohere v2 SSE 事件 → OpenAI chunk
pub(crate) fn cohere_to_openai(ev: &Value) -> SseAction {
    let etype = ev.get("type").and_then(|t| t.as_str()).unwrap_or("");
    match etype {
        "content-delta" => {
            let text = ev
                .pointer("/delta/message/content/text")
                .and_then(|t| t.as_str())
                .unwrap_or("");
            SseAction::Frame(json!({
                "choices": [{ "index": 0, "delta": { "content": text } }],
            }))
        }
        "thinking-delta" => {
            let t = ev
                .pointer("/delta/message/thinking")
                .and_then(|t| t.as_str())
                .unwrap_or("");
            SseAction::Frame(json!({
                "choices": [{ "index": 0, "delta": { "reasoning_content": t } }],
            }))
        }
        "message-end" => {
            let mut chunk = json!({ "choices": [{ "index": 0, "delta": {} }] });
            if let Some(fr) = ev.pointer("/delta/finish_reason") {
                chunk["choices"][0]["finish_reason"] = map_finish_reason(fr.as_str().unwrap_or(""));
            }
            SseAction::Frame(chunk)
        }
        // message-start / content-start / tool-* 忽略（App 不解析工具面）
        _ => SseAction::FrameOnly,
    }
}

pub(crate) fn map_finish_reason(s: &str) -> Value {
    // Cohere v1 遗留 "COMPLETE" 大小写不敏感兼容
    let low = s.to_ascii_lowercase();
    json!(match low.as_str() {
        "end_turn" | "complete" => "stop",
        "max_tokens" => "length",
        "stop_sequence" => "stop",
        "tool_call" | "tool_use" => "tool_calls",
        other => other,
    })
}

/// Cohere v2 非流式 JSON → OpenAI 形（content[type=text] 拼接；空串放下游处理）
pub(crate) fn wrap_cohere_response(v: Value) -> Value {
    let content_text: String = v
        .pointer("/message/content")
        .and_then(|c| c.as_array())
        .map(|arr| {
            arr.iter()
                .filter(|p| p.get("type").and_then(|t| t.as_str()) == Some("text"))
                .filter_map(|p| p.get("text"))
                .filter_map(|t| t.as_str())
                .collect::<Vec<_>>()
                .join("")
        })
        .unwrap_or_default();
    let mut reply = json!({
        "choices": [{ "message": { "content": content_text } }],
    });
    if let Some(fr) = v.get("finish_reason") {
        reply["choices"][0]["finish_reason"] = map_finish_reason(fr.as_str().unwrap_or(""));
    }
    reply
}

/* ================================================================ *
 * 入口：/generate
 * ================================================================ */

/// POST /api/backends/chat-completions/generate（manifest #41）
pub async fn generate(st: &BackendState, body: &Value, origin: Option<HeaderValue>) -> Response {
    let Some(source) = body.get("chat_completion_source").and_then(|v| v.as_str()) else {
        return err_json(StatusCode::BAD_REQUEST, "缺少 chat_completion_source");
    };
    let body = ensure_post_processing(body);
    let body = enrich_multimodal_messages(st, &body);
    let model = body.get("model").and_then(|v| v.as_str()).unwrap_or("-");
    let stream = body.get("stream").and_then(|v| v.as_bool()).unwrap_or(false);
    let n_msgs = body
        .get("messages")
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(0);
    log::info!(
        target: super::logx::PROVIDER,
        "chat-completions/generate source={} model={} stream={stream} messages={n_msgs}",
        super::logx::redact(source),
        super::logx::redact(model)
    );
    let started = std::time::Instant::now();
    let resp = match source {
        "claude" => super::providers_claude::send_claude(st, &body, origin).await,
        "makersuite" | "vertexai" => {
            super::providers_google::send_makersuite(st, &body, origin).await
        }
        "ai21" => super::providers_misc::send_ai21(st, &body, origin).await,
        "mistralai" => super::providers_misc::send_mistral(st, &body, origin).await,
        "cohere" => super::providers_misc::send_cohere(st, &body, origin).await,
        "deepseek" => super::providers_misc::send_deepseek(st, &body, origin).await,
        "xai" => super::providers_misc::send_xai(st, &body, origin).await,
        "aimlapi" => super::providers_misc::send_aimlapi(st, &body, origin).await,
        "electronhub" => super::providers_misc::send_electronhub(st, &body, origin).await,
        "chutes" => super::providers_misc::send_chutes(st, &body, origin).await,
        "minimax" => super::providers_misc::send_minimax(st, &body, origin).await,
        "azure_openai" => super::providers_misc::send_azure(st, &body, origin).await,
        "cometapi" => json_resp(
            StatusCode::BAD_GATEWAY,
            json!({ "error": { "message": "This provider is temporarily disabled." } }),
            origin,
        ),
        "openai" | "openrouter" | "custom" | "perplexity" | "groq" | "fireworks" | "nanogpt"
        | "pollinations" | "moonshot" | "zai" | "siliconflow" | "workers_ai" => {
            generic_generate(st, &body, source, origin).await
        }
        other => not_migrated(other, origin),
    };
    log::info!(
        target: super::logx::PROVIDER,
        "chat-completions/generate 完成 source={} status={} {}ms",
        super::logx::redact(source),
        resp.status().as_u16(),
        started.elapsed().as_millis()
    );
    resp
}

/// custom_prompt_post_processing 面支持（generate 入口，chat-completions.js:2246-2253）
pub(crate) fn ensure_post_processing(body: &Value) -> Value {
    let Some(ptype) = body
        .get("custom_prompt_post_processing")
        .and_then(|v| v.as_str())
        .map(|v| v.to_string())
    else {
        return body.clone();
    };
    if ptype.is_empty() || !body.get("messages").map(|m| m.is_array()).unwrap_or(false) {
        return body.clone();
    }
    let processed = super::providers_convert::post_process_prompt(
        body.get("messages").unwrap(),
        &ptype,
        &super::providers_convert::PromptNames::from_body(body),
    );
    let mut out = body.clone();
    out.as_object_mut()
        .unwrap()
        .insert("messages".into(), processed);
    out
}

fn resolve_image_to_data_url(st: &BackendState, url: &str) -> Option<String> {
    if url.starts_with("data:") || url.starts_with("http://") || url.starts_with("https://") {
        return Some(url.to_string());
    }
    let clean = url.trim().trim_start_matches('/');
    let rel_path = if let Some(stripped) = clean.strip_prefix("user-images/") {
        stripped
    } else if let Some(stripped) = clean.strip_prefix("user/images/") {
        stripped
    } else {
        clean
    };
    let base_dir = st.data_root.join("default-user").join("user").join("images");
    let segs: Vec<&str> = rel_path.split('/').filter(|s| !s.is_empty()).collect();
    let safe_path = crate::pathguard::join_under(&base_dir, &segs).ok()?;
    if !safe_path.is_file() {
        return None;
    }
    let bytes = std::fs::read(&safe_path).ok()?;
    let ext = safe_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("png")
        .to_ascii_lowercase();
    let mime = match ext.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        _ => "image/png",
    };
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Some(format!("data:{mime};base64,{b64}"))
}

pub(crate) fn enrich_multimodal_messages(st: &BackendState, body: &Value) -> Value {
    let Some(msgs) = body.get("messages").and_then(|m| m.as_array()) else {
        return body.clone();
    };
    let mut modified = false;
    let mut new_msgs = Vec::new();
    for msg in msgs {
        let mut msg_obj = match msg.as_object() {
            Some(o) => o.clone(),
            None => {
                new_msgs.push(msg.clone());
                continue;
            }
        };

        if let Some(img_val) = msg_obj.remove("image") {
            if let Some(img_str) = img_val.as_str() {
                if let Some(data_url) = resolve_image_to_data_url(st, img_str) {
                    let current_text = msg_obj
                        .get("content")
                        .and_then(|c| c.as_str())
                        .unwrap_or("")
                        .to_string();
                    let content_arr = json!([
                        { "type": "text", "text": current_text },
                        { "type": "image_url", "image_url": { "url": data_url } }
                    ]);
                    msg_obj.insert("content".into(), content_arr);
                    modified = true;
                }
            }
        } else if let Some(content_arr) = msg_obj.get("content").and_then(|c| c.as_array()) {
            let mut parts_modified = false;
            let mut new_parts = Vec::new();
            for part in content_arr {
                let mut part_obj = match part.as_object() {
                    Some(o) => o.clone(),
                    None => {
                        new_parts.push(part.clone());
                        continue;
                    }
                };
                if part_obj.get("type").and_then(|t| t.as_str()) == Some("image_url") {
                    if let Some(iu) = part_obj.get("image_url").and_then(|u| u.as_object()) {
                        if let Some(url_str) = iu.get("url").and_then(|u| u.as_str()) {
                            if !url_str.starts_with("data:") {
                                if let Some(data_url) = resolve_image_to_data_url(st, url_str) {
                                    part_obj.insert(
                                        "image_url".into(),
                                        json!({ "url": data_url }),
                                    );
                                    parts_modified = true;
                                }
                            }
                        }
                    }
                }
                new_parts.push(Value::Object(part_obj));
            }
            if parts_modified {
                msg_obj.insert("content".into(), Value::Array(new_parts));
                modified = true;
            }
        }
        new_msgs.push(Value::Object(msg_obj));
    }

    if !modified {
        return body.clone();
    }
    let mut out = body.clone();
    out.as_object_mut()
        .unwrap()
        .insert("messages".into(), Value::Array(new_msgs));
    out
}

/* ================================================================ *
 * OpenAI 兼容主路（generic 族，chat-completions.js:2281-2729）
 * ================================================================ */

fn take(body: &Value, key: &str) -> Option<Value> {
    body.get(key).cloned()
}

/// 字符串字段直取（避免 take 临时值的借用逃逸）
fn s_field(body: &Value, key: &str) -> Option<String> {
    body.get(key).and_then(|v| v.as_str().map(String::from))
}

fn put(rb: &mut Map<String, Value>, key: &str, value: Option<Value>) {
    if let Some(v) = value {
        rb.insert(key.to_string(), v);
    }
}

fn is_non_empty_array(v: &Option<Value>) -> bool {
    v.as_ref()
        .map(|s| s.is_array() && !s.as_array().unwrap().is_empty())
        .unwrap_or(false)
}

fn reverse_proxy_on(body: &Value) -> bool {
    body.get("reverse_proxy")
        .and_then(|v| v.as_str())
        .map(|s| !s.is_empty())
        .unwrap_or(false)
}

/// reverse_proxy URL 出站校验（HIGH-5 ProxyUrl 面）——Err 为 400 文案
fn rp_validated(body: &Value) -> Result<String, String> {
    let raw = body
        .get("reverse_proxy")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    validate_egress_url(raw, EgressPolicy::ProxyUrl)
        .map_err(|e| format!("reverse_proxy 被出站网关拒绝: {e}"))
}

fn proxy_password(body: &Value) -> String {
    body.get("proxy_password")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

fn stream_of(body: &Value) -> bool {
    body.get("stream")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

async fn generic_generate(
    st: &BackendState,
    body: &Value,
    source: &str,
    origin: Option<HeaderValue>,
) -> Response {
    log_outbound(source, body);
    let mut msg_mutated = body.clone(); // embed / 后处理面在本地副本上做
    let mut headers: Vec<(String, String)> = Vec::new();
    let mut body_params = Map::new();

    let model = take(body, "model")
        .and_then(|m| m.as_str().map(String::from))
        .unwrap_or_default();
    let model = model.as_str();

    let api_root: String;
    let direct_key: Option<String>;

    match source {
        "openai" => {
            let use_rp = reverse_proxy_on(body);
            api_root = if use_rp {
                match rp_validated(body) {
                    Ok(u) => u,
                    Err(e) => return err_json(StatusCode::BAD_REQUEST, &e),
                }
            } else {
                API_OPENAI.to_string()
            };
            direct_key = Some(if use_rp {
                proxy_password(body)
            } else {
                read_secret(st, "api_key_openai").unwrap_or_default()
            });
            // embedOpenRouterMedia audio only（openai 同款 audio_url → input_audio）
            embed_media_messages(&mut msg_mutated, true);
        }
        "openrouter" => {
            api_root = API_OPENROUTER.to_string();
            direct_key = Some(read_secret(st, "api_key_openrouter").unwrap_or_default());
            headers = openrouter_headers();
            match s_field(body, "middleout") {
                Some(m) if m == "on" => {
                    body_params.insert("transforms".into(), json!(["middle-out"]));
                }
                Some(m) if m == "off" => {
                    body_params.insert("transforms".into(), json!([]));
                }
                _ => {}
            };
            body_params.insert("plugins".into(), json!([])); // web 关闭 → 空数组（ST 字面）
            let include_reasoning = take(body, "include_reasoning")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let mut reasoning = json!({ "exclude": !include_reasoning });
            put(
                reasoning.as_object_mut().unwrap(),
                "effort",
                take(body, "reasoning_effort"),
            );
            body_params.insert("reasoning".into(), reasoning);
            if let Some(v) = take(body, "min_p") {
                body_params.insert("min_p".into(), v);
            }
            if let Some(v) = take(body, "top_a") {
                body_params.insert("top_a".into(), v);
            }
            if let Some(v) = take(body, "repetition_penalty") {
                body_params.insert("repetition_penalty".into(), v);
            }
            embed_media_messages(&mut msg_mutated, true);
            if let Some(m) = msg_mutated.get_mut("messages") {
                super::providers_convert::add_open_router_signatures(m, model);
            }
            if model.contains("google/gemini") {
                body_params.insert("safety_settings".into(), gemini_safety());
            }
        }
        "custom" => {
            let url = match s_field(body, "custom_url")
                .map(|u| validate_egress_url(&u, EgressPolicy::SelfHosted))
            {
                Some(Ok(u)) => u,
                Some(Err(e)) => return err_json(StatusCode::BAD_REQUEST, &e),
                None => return err_json(StatusCode::BAD_REQUEST, "缺少 custom_url"),
            };
            api_root = url;
            direct_key = Some(read_secret(st, "api_key_custom").unwrap_or_default());
            if let Some(h) = take(body, "custom_include_headers") {
                merge_yaml_into_headers(&mut headers, h);
            }
            if let Some(b) = take(body, "custom_include_body") {
                merge_yaml_into_body_params(&mut body_params, b);
            }
            json_schema_response_format(&mut body_params, body);
            embed_media_messages(&mut msg_mutated, true);
        }
        "perplexity" => {
            api_root = API_PERPLEXITY.to_string();
            direct_key = Some(read_secret(st, "api_key_perplexity").unwrap_or_default());
            put(
                &mut body_params,
                "reasoning_effort",
                take(body, "reasoning_effort"),
            );
            post_process_messages(&mut msg_mutated, "strict");
            if body
                .get("json_schema")
                .map(|j| j.is_object())
                .unwrap_or(false)
            {
                body_params.insert(
                    "response_format".into(),
                    json!({ "type": "json_schema", "json_schema": { "schema": body["json_schema"]["value"].clone() } }),
                );
            }
        }
        "groq" => {
            api_root = API_GROQ.to_string();
            direct_key = Some(read_secret(st, "api_key_groq").unwrap_or_default());
            json_schema_response_format(&mut body_params, body);
        }
        "fireworks" => {
            api_root = API_FIREWORKS.to_string();
            direct_key = Some(read_secret(st, "api_key_fireworks").unwrap_or_default());
            put(
                &mut body_params,
                "reasoning_effort",
                take(body, "reasoning_effort"),
            );
            json_schema_response_format(&mut body_params, body);
        }
        "nanogpt" => {
            api_root = API_NANOGPT.to_string();
            direct_key = Some(read_secret(st, "api_key_nanogpt").unwrap_or_default());
            if let Some(p) = take(body, "nanogpt_provider") {
                headers.push(("X-Provider".into(), p.as_str().unwrap_or("").to_string()));
            }
            if take(body, "nanogpt_payg_override")
                .map(|v| !v.is_null())
                .unwrap_or(false)
            {
                headers.push(("X-Billing-Mode".into(), "paygo".into()));
                body_params.insert("billing_mode".into(), json!("paygo"));
            }
            if take(body, "enable_web_search")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
                && !model.ends_with(":online")
            {
                if let Some(obj) = msg_mutated.as_object_mut() {
                    obj.insert("model".into(), json!(format!("{model}:online")));
                }
            }
            if let Some(v) = take(body, "min_p") {
                body_params.insert("min_p".into(), v);
            }
            if let Some(v) = take(body, "top_a") {
                body_params.insert("top_a".into(), v);
            }
            if let Some(v) = take(body, "repetition_penalty") {
                body_params.insert("repetition_penalty".into(), v);
            }
            if let Some(effort) =
                take(body, "reasoning_effort").and_then(|e| e.as_str().map(String::from))
            {
                let mapped = super::providers_text::map_nanogpt_reasoning_effort(&effort);
                if !mapped.is_empty() {
                    body_params.insert("reasoning".into(), json!({ "effort": mapped }));
                }
            }
        }
        "pollinations" => {
            let is_anon = s_field(body, "pollinations_endpoint").as_deref() == Some("anonymous");
            if is_anon {
                api_root = API_POLLINATIONS_ANON.to_string();
                direct_key = Some("anonymous".to_string());
            } else {
                api_root = API_POLLINATIONS.to_string();
                direct_key = Some(read_secret(st, "api_key_pollinations").unwrap_or_default());
            }
            let seed = match take(body, "seed") {
                Some(v) => v,
                None => json!(random_seed()),
            };
            body_params.insert("seed".into(), seed);
            if !is_anon {
                put(
                    &mut body_params,
                    "reasoning_effort",
                    take(body, "reasoning_effort"),
                );
                if body
                    .get("json_schema")
                    .map(|j| j.is_object())
                    .unwrap_or(false)
                {
                    body_params.insert(
                        "response_format".into(),
                        json!({ "type": "json_schema", "json_schema": { "schema": body["json_schema"]["value"].clone() } }),
                    );
                }
            }
        }
        "moonshot" => {
            let use_rp = reverse_proxy_on(body);
            api_root = if use_rp {
                match rp_validated(body) {
                    Ok(u) => u,
                    Err(e) => return err_json(StatusCode::BAD_REQUEST, &e),
                }
            } else {
                API_MOONSHOT.to_string()
            };
            direct_key = Some(if use_rp {
                proxy_password(body)
            } else {
                read_secret(st, "api_key_moonshot").unwrap_or_default()
            });
            let include_reasoning = take(body, "include_reasoning")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            body_params.insert(
                "thinking".into(),
                json!({ "type": if include_reasoning { "enabled" } else { "disabled" } }),
            );
            if body
                .get("json_schema")
                .map(|j| j.is_object())
                .unwrap_or(false)
            {
                set_json_object_format(&mut body_params, &mut msg_mutated, body);
            } else {
                add_partial_prefix(&mut msg_mutated);
            }
        }
        "zai" => {
            let default = if s_field(body, "zai_endpoint").as_deref() == Some("coding") {
                API_ZAI_CODING
            } else {
                API_ZAI_COMMON
            };
            let use_rp = reverse_proxy_on(body);
            api_root = if use_rp {
                match rp_validated(body) {
                    Ok(u) => u,
                    Err(e) => return err_json(StatusCode::BAD_REQUEST, &e),
                }
            } else {
                default.to_string()
            };
            direct_key = Some(if use_rp {
                proxy_password(body)
            } else {
                read_secret(st, "api_key_zai").unwrap_or_default()
            });
            headers = zai_headers();
            let include_reasoning = take(body, "include_reasoning")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            body_params.insert(
                "thinking".into(),
                json!({ "type": if include_reasoning { "enabled" } else { "disabled" } }),
            );
            if body
                .get("json_schema")
                .map(|j| j.is_object())
                .unwrap_or(false)
            {
                set_json_object_format(&mut body_params, &mut msg_mutated, body);
            }
        }
        "siliconflow" => {
            api_root = if s_field(body, "siliconflow_endpoint").as_deref() == Some("cn") {
                API_SILICONFLOW_CN
            } else {
                API_SILICONFLOW
            }
            .to_string();
            direct_key = Some(read_secret(st, "api_key_siliconflow").unwrap_or_default());
            if body
                .get("json_schema")
                .map(|j| j.is_object())
                .unwrap_or(false)
            {
                set_json_object_format(&mut body_params, &mut msg_mutated, body);
            }
        }
        "workers_ai" => {
            let account_id = s_field(body, "workers_ai_account_id")
                .map(|s| s.trim().to_string())
                .unwrap_or_default();
            if account_id.is_empty() {
                return json_resp(StatusCode::BAD_REQUEST, json!({ "error": true }), origin);
            }
            direct_key = Some(read_secret(st, "api_key_workers_ai").unwrap_or_default());
            api_root = format!("{API_WORKERS_AI}/{account_id}/ai/v1");
            put(
                &mut body_params,
                "repetition_penalty",
                take(body, "repetition_penalty"),
            );
            if body
                .get("json_schema")
                .map(|j| j.is_object())
                .unwrap_or(false)
            {
                body_params.insert(
                    "response_format".into(),
                    json!({ "type": "json_schema", "json_schema": body["json_schema"]["value"].clone() }),
                );
            }
        }
        other => return not_migrated(other, origin), // dispatch 已白名单（防御）
    }

    let key = direct_key.unwrap_or_default();

    // reasoning_effort / verbosity（custom + openai 白名单分流，chat-completions.js:2599-2613）
    let effort = s_field(body, "reasoning_effort");
    if (source == "openai" || source == "custom") && effort.is_some() {
        if super::providers_text::is_openai_reasoning_effort_model(model) {
            let mapped = super::providers_text::map_openai_reasoning_effort(
                model,
                effort.as_deref().unwrap_or(""),
            );
            body_params.insert("reasoning_effort".into(), json!(mapped));
        }
        if source == "custom"
            && model
                .split_once("koboldcpp/")
                .map(|x| x.0.is_empty())
                .unwrap_or(false)
        {
            body_params.insert(
                "reasoning_effort".into(),
                json!(effort.as_deref().unwrap_or("")),
            );
        }
    }
    let verbosity = s_field(body, "verbosity");
    if (source == "openai" || source == "custom") && verbosity.is_some()
        && super::providers_text::is_openai_verbosity_model(model) {
            body_params.insert(
                "verbosity".into(),
                json!(verbosity.as_deref().unwrap_or("")),
            );
        }

    // 缺 key（非 custom、无 reverse_proxy）→ 400（ST 字面 :2615-2618）
    if key.is_empty() && source != "custom" && !reverse_proxy_on(body) {
        return json_resp(StatusCode::BAD_REQUEST, json!({ "error": true }), origin);
    }

    // stop 数组非空 → bodyParams
    let stop = take(body, "stop");
    if is_non_empty_array(&stop) {
        body_params.insert("stop".into(), stop.unwrap().clone());
    }

    let is_text_completion = super::providers_text::is_text_completion_model(model)
        || msg_mutated
            .get("messages")
            .map(|m| m.is_string())
            .unwrap_or(false);

    // tools / tool_choice（chat-completions.js:2636-2639）
    let tools =
        take(body, "tools").filter(|t| t.as_array().map(|a| !a.is_empty()).unwrap_or(false));
    if !is_text_completion {
        if let Some(t) = tools {
            body_params.insert("tools".into(), t);
            put(&mut body_params, "tool_choice", take(body, "tool_choice"));
        }
    }
    // json_schema 统一兜底（:2641-2649）
    if body
        .get("json_schema")
        .map(|j| j.is_object())
        .unwrap_or(false)
        && !body_params.contains_key("response_format")
    {
        json_schema_response_format(&mut body_params, body);
    }

    // requestBody 装配（message/prompt 互斥 + bodyParams 摊平）
    let mut rb = Map::new();
    if !is_text_completion {
        rb.insert(
            "messages".into(),
            take(&msg_mutated, "messages").unwrap_or(json!([])),
        );
    } else {
        let prompt = super::providers_convert::convert_text_completion_prompt(
            &take(&msg_mutated, "messages").unwrap_or(Value::Null),
        );
        rb.insert("prompt".into(), json!(prompt));
    }
    put(&mut rb, "model", take(&msg_mutated, "model"));
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
    put(&mut rb, "top_k", take(body, "top_k"));
    if !is_text_completion {
        put(&mut rb, "stop", take(body, "stop"));
    }
    put(&mut rb, "logit_bias", take(body, "logit_bias"));
    put(&mut rb, "seed", take(body, "seed"));
    put(&mut rb, "n", take(body, "n"));
    rb.extend(body_params);

    // custom 源 excludeKeysByYaml（:2671-2673）
    if source == "custom" {
        if let Some(ex) =
            take(body, "custom_exclude_body").and_then(|e| e.as_str().map(String::from))
        {
            exclude_yaml_keys(&mut rb, &ex);
        }
    }

    let endpoint = if is_text_completion && source != "openrouter" {
        "completions"
    } else {
        "chat/completions"
    };
    let url = format!("{api_root}/{endpoint}");

    let resp_res = upstream_post(&url, &key, headers, &Value::Object(rb)).await;
    let resp = match resp_res {
        Ok(r) => r,
        Err(e) => return err_json(StatusCode::BAD_GATEWAY, &format!("上游连接失败: {e}")),
    };

    let status_ok = resp.status().is_success();
    if stream_of(body) {
        // ST 字面：流式无条件透传（含非 2xx 上游体；401→400 降级）
        return passthrough(resp, origin).await;
    }

    // 非流式错误：E1 —— 上游体 + 状态码原样透传（Phase 1 定档注册偏差，
    // 下游 stGenerateBlocking 依靠 error.message 文案判定可重试）
    if !status_ok {
        let code = downgrade_status(resp.status());
        let text = resp.text().await.unwrap_or_default();
        return apply_cors_resp((code, text).into_response(), origin);
    }
    let text = resp.text().await.unwrap_or_default();
    let v = serde_json::from_str::<Value>(&text).unwrap_or(json!({}));
    apply_cors_resp(Json(v).into_response(), origin)
}

fn random_seed() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    (n as u64) % 99999999
}

fn embed_media_messages(body: &mut Value, audio: bool) {
    if let Some(m) = body.get_mut("messages") {
        super::providers_convert::embed_openrouter_media(m, audio, false);
    }
}

fn post_process_messages(body: &mut Value, ptype: &str) {
    if !body.get("messages").map(|m| m.is_array()).unwrap_or(false) {
        return;
    }
    let processed = super::providers_convert::post_process_prompt(
        body.get("messages").unwrap(),
        ptype,
        &super::providers_convert::PromptNames::from_body(body),
    );
    body.as_object_mut()
        .unwrap()
        .insert("messages".into(), processed);
}

/// {type: json_object} + push schema user message（moonshot/zai/siliconflow ST 字面）
fn set_json_object_format(
    body_params: &mut Map<String, Value>,
    msg_mutated: &mut Value,
    body: &Value,
) {
    body_params.insert("response_format".into(), json!({ "type": "json_object" }));
    if let Some(messages) = msg_mutated
        .get_mut("messages")
        .and_then(|m| m.as_array_mut())
    {
        let schema_text = json_pretty4(
            body.get("json_schema")
                .and_then(|j| j.get("value"))
                .unwrap_or(&json!({})),
        );
        messages.push(json!({
            "role": "user",
            "content": format!("JSON schema for the response:\n{schema_text}"),
        }));
    }
}

/// moonshot 无 json_schema 分支：末条 assistant（无 tools 时）加 partial=true
fn add_partial_prefix(msg_mutated: &mut Value) {
    if let Some(messages) = msg_mutated
        .get_mut("messages")
        .and_then(|m| m.as_array_mut())
    {
        super::providers_convert::add_assistant_prefix(messages, None, "partial");
    }
}

/// mergeObjectWithYaml（headers 面）：仅对象成员逐项并入（字符串值→头，
/// bool/数字 → to_string；数组/标量 no-op —— YAML 解析失败 try/catch no-op）
fn merge_yaml_into_headers(headers: &mut Vec<(String, String)>, source: Value) {
    let parsed = yaml_value(source);
    if let Some(obj) = parsed.as_object() {
        for (k, v) in obj {
            let val = if let Some(s) = v.as_str() {
                s.to_string()
            } else if v.is_boolean() || v.is_number() {
                v.to_string()
            } else {
                continue;
            };
            headers.push((k.clone(), val));
        }
    }
}

fn merge_yaml_into_body_params(body_params: &mut Map<String, Value>, source: Value) {
    let parsed = yaml_value(source);
    if let Some(obj) = parsed.as_object() {
        for (k, v) in obj {
            body_params.insert(k.clone(), v.clone());
        }
    }
}

fn exclude_yaml_keys(rb: &mut Map<String, Value>, yaml: &str) {
    let parsed: Result<serde_yaml::Value, _> = serde_yaml::from_str(yaml);
    let Ok(parsed) = parsed else { return };
    let parsed = serde_json::to_value(parsed).unwrap_or(Value::Null);
    match parsed {
        Value::Array(a) => {
            for item in a {
                if let Some(k) = item.as_str() {
                    rb.remove(k);
                }
            }
        }
        Value::Object(o) => {
            for k in o.keys() {
                rb.remove(k);
            }
        }
        Value::String(s) => {
            rb.remove(&s);
        }
        _ => {}
    }
}

/// ST mergeObjectWithYaml 语义：非字符串输入原样（bool → no-op）、
/// YAML 字符串 → 解析（失败 → null no-op）
fn yaml_value(source: Value) -> Value {
    match source {
        Value::String(s) => serde_yaml::from_str::<serde_yaml::Value>(&s)
            .ok()
            .and_then(|y| serde_json::to_value(y).ok())
            .unwrap_or(Value::Null),
        other => other,
    }
}

/// json_schema → response_format（generic 统一包装，含 name/strict）
fn json_schema_response_format(rb: &mut Map<String, Value>, body: &Value) {
    let Some(schema) = body.get("json_schema").filter(|j| j.is_object()) else {
        return;
    };
    rb.insert(
        "response_format".into(),
        json!({
            "type": "json_schema",
            "json_schema": {
                "name": schema.get("name").cloned().unwrap_or(json!("default")),
                "strict": schema.get("strict").and_then(|s| s.as_bool()).unwrap_or(true),
                "schema": schema.get("value").cloned().unwrap_or(json!({})),
            },
        }),
    );
}

/* ================================================================ *
 * 入口：/status（chat-completions.js:1778-2156）
 * ================================================================ */

/// POST /api/backends/chat-completions/status（manifest #42）
pub async fn status(st: &BackendState, body: &Value, origin: Option<HeaderValue>) -> Response {
    let Some(source) = body.get("chat_completion_source").and_then(|v| v.as_str()) else {
        return err_json(StatusCode::BAD_REQUEST, "缺少 chat_completion_source");
    };
    log::info!(target: super::logx::PROVIDER, "chat-completions/status source={}", super::logx::redact(source));

    match source {
        "openai" => {
            let key = if reverse_proxy_on(body) {
                proxy_password(body)
            } else {
                read_secret(st, "api_key_openai").unwrap_or_default()
            };
            let api_root = match rp_or(body, API_OPENAI) {
                Ok(u) => u,
                Err(e) => return err_json(StatusCode::BAD_REQUEST, &e),
            };
            fetch_models_common(st, &api_root, &key, Vec::new(), &[], origin).await
        }
        "custom" => {
            let url = match s_field(body, "custom_url")
                .map(|u| validate_egress_url(&u, EgressPolicy::SelfHosted))
            {
                Some(Ok(u)) => u,
                Some(Err(e)) => return err_json(StatusCode::BAD_REQUEST, &e),
                None => return err_json(StatusCode::BAD_REQUEST, "缺少 custom_url"),
            };
            let mut headers = Vec::new();
            if let Some(h) = take(body, "custom_include_headers") {
                merge_yaml_into_headers(&mut headers, h);
            }
            fetch_models_common(
                st,
                &url,
                &read_secret(st, "api_key_custom").unwrap_or_default(),
                headers,
                &[],
                origin,
            )
            .await
        }
        "openrouter" => {
            fetch_models_common(
                st,
                API_OPENROUTER,
                &read_secret(st, "api_key_openrouter").unwrap_or_default(),
                openrouter_headers(),
                &[],
                origin,
            )
            .await
        }
        "mistralai" => {
            let (use_rp, url) = match rp_or_pair(body, API_MISTRAL) {
                Ok(p) => p,
                Err(e) => return err_json(StatusCode::BAD_REQUEST, &e),
            };
            let key = if use_rp {
                proxy_password(body)
            } else {
                read_secret(st, "api_key_mistralai").unwrap_or_default()
            };
            fetch_models_common(st, &url, &key, Vec::new(), &[], origin).await
        }
        "deepseek" => {
            let (use_rp, url) = match rp_or_pair(body, API_DEEPSEEK_STATUS) {
                Ok(p) => p,
                Err(e) => return err_json(StatusCode::BAD_REQUEST, &e),
            };
            let key = if use_rp {
                proxy_password(body)
            } else {
                read_secret(st, "api_key_deepseek").unwrap_or_default()
            };
            fetch_models_common(st, &url, &key, Vec::new(), &[], origin).await
        }
        "xai" => {
            let (use_rp, url) = match rp_or_pair(body, API_XAI) {
                Ok(p) => p,
                Err(e) => return err_json(StatusCode::BAD_REQUEST, &e),
            };
            let key = if use_rp {
                proxy_password(body)
            } else {
                read_secret(st, "api_key_xai").unwrap_or_default()
            };
            fetch_models_common(st, &url, &key, Vec::new(), &[], origin).await
        }
        "moonshot" => {
            let (use_rp, url) = match rp_or_pair(body, API_MOONSHOT) {
                Ok(p) => p,
                Err(e) => return err_json(StatusCode::BAD_REQUEST, &e),
            };
            let key = if use_rp {
                proxy_password(body)
            } else {
                read_secret(st, "api_key_moonshot").unwrap_or_default()
            };
            fetch_models_common(st, &url, &key, Vec::new(), &[], origin).await
        }
        "cohere" => {
            fetch_models_common(
                st,
                API_COHERE_V1,
                &read_secret(st, "api_key_cohere").unwrap_or_default(),
                Vec::new(),
                &[],
                origin,
            )
            .await
        }
        "chutes" => {
            fetch_models_common(
                st,
                API_CHUTES,
                &read_secret(st, "api_key_chutes").unwrap_or_default(),
                Vec::new(),
                &[],
                origin,
            )
            .await
        }
        "electronhub" => {
            fetch_models_common(
                st,
                API_ELECTRONHUB,
                &read_secret(st, "api_key_electronhub").unwrap_or_default(),
                Vec::new(),
                &[],
                origin,
            )
            .await
        }
        "nanogpt" => {
            fetch_models_common(
                st,
                API_NANOGPT,
                &read_secret(st, "api_key_nanogpt").unwrap_or_default(),
                Vec::new(),
                &[("detailed", "true")],
                origin,
            )
            .await
        }
        "aimlapi" => {
            fetch_models_common(
                st,
                API_AIMLAPI,
                &read_secret(st, "api_key_aimlapi").unwrap_or_default(),
                aimlapi_headers(),
                &[],
                origin,
            )
            .await
        }
        "groq" => {
            fetch_models_common(
                st,
                API_GROQ,
                &read_secret(st, "api_key_groq").unwrap_or_default(),
                Vec::new(),
                &[],
                origin,
            )
            .await
        }
        "siliconflow" => {
            let base = if s_field(body, "siliconflow_endpoint").as_deref() == Some("cn") {
                API_SILICONFLOW_CN
            } else {
                API_SILICONFLOW
            };
            fetch_models_common(
                st,
                base,
                &read_secret(st, "api_key_siliconflow").unwrap_or_default(),
                Vec::new(),
                &[("type", "text"), ("sub_type", "chat")],
                origin,
            )
            .await
        }
        "fireworks" => fireworks_status(st, origin).await,
        "makersuite" | "vertexai" => super::providers_google::status(st, body, origin).await,
        "azure_openai" => azure_status(st, body, origin).await,
        "workers_ai" => workers_ai_status(st, body, origin).await,
        "pollinations" => pollinations_status(st, body, origin).await,
        "cometapi" => {
            // ST 字面 throw → catch → {error:true}
            json_resp(StatusCode::OK, json!({ "error": true }), origin)
        }
        _ => json_resp(StatusCode::BAD_REQUEST, json!({ "error": true }), origin),
    }
}

fn rp_or(body: &Value, base: &str) -> Result<String, String> {
    match body
        .get("reverse_proxy")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
    {
        Some(rp) => validate_egress_url(rp, EgressPolicy::ProxyUrl),
        None => Ok(base.to_string()),
    }
}

fn rp_or_pair(body: &Value, base: &str) -> Result<(bool, String), String> {
    match body
        .get("reverse_proxy")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
    {
        Some(rp) => Ok((true, validate_egress_url(rp, EgressPolicy::ProxyUrl)?)),
        None => Ok((false, base.to_string())),
    }
}

/// 通用 /models 探针（ST 通用分支 :2067-2146：缺 key 400；上游故障 200+{error:true}）
async fn fetch_models_common(
    st: &BackendState,
    api_root: &str,
    key: &str,
    headers: Vec<(String, String)>,
    query: &[(&str, &str)],
    origin: Option<HeaderValue>,
) -> Response {
    let _ = st;
    if key.is_empty() {
        return apply_cors_resp(StatusCode::BAD_REQUEST.into_response(), origin);
    }
    let client = post_client().await;
    let mut req = client
        .get(format!("{api_root}/models"))
        .header("accept-encoding", "identity")
        .timeout(std::time::Duration::from_secs(30))
        .header("Authorization", format!("Bearer {key}"));
    for (k, v) in headers {
        req = req.header(k.as_str(), v.as_str());
    }
    if !query.is_empty() {
        req = req.query(&query);
    }
    let resp = req.send().await;
    match resp {
        Ok(r) if r.status().is_success() => {
            let text = r.text().await.unwrap_or_default();
            apply_cors_resp(
                Json(
                    serde_json::from_str::<Value>(&text)
                        .unwrap_or(json!({ "error": true, "data": { "data": [] } })),
                )
                .into_response(),
                origin,
            )
        }
        _ => apply_cors_resp(
            Json(json!({ "error": true, "data": { "data": [] } })).into_response(),
            origin,
        ),
    }
}

/// Fireworks 专用模型发现（:1852-1907：字段映射 + fast router 附加）
async fn fireworks_status(st: &BackendState, origin: Option<HeaderValue>) -> Response {
    let key = read_secret(st, "api_key_fireworks").unwrap_or_default();
    if key.is_empty() {
        return apply_cors_resp(StatusCode::BAD_REQUEST.into_response(), origin);
    }
    let url = "https://api.fireworks.ai/v1/accounts/fireworks/models?filter=supports_serverless%3Dtrue&pageSize=200";
    let resp = post_client()
        .await
        .get(url)
        .header("Authorization", format!("Bearer {key}"))
        .header("accept-encoding", "identity")
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await;
    let Ok(resp) = resp else {
        return json_resp(
            StatusCode::OK,
            json!({ "error": true, "data": { "data": [] } }),
            origin,
        );
    };
    if !resp.status().is_success() {
        return json_resp(
            StatusCode::OK,
            json!({ "error": true, "data": { "data": [] } }),
            origin,
        );
    }
    let data = resp.json::<Value>().await.unwrap_or(json!({}));
    let models_raw = data
        .get("models")
        .and_then(|m| m.as_array())
        .cloned()
        .unwrap_or_default();
    let mut models: Vec<Value> = models_raw
        .into_iter()
        .filter(|m| {
            m.get("contextLength").and_then(|c| c.as_i64()).unwrap_or(0) > 0
                && m.get("kind").and_then(|k| k.as_str()) != Some("EMBEDDING_MODEL")
        })
        .map(|m| {
            json!({
                "id": m.get("name").cloned().unwrap_or(json!("")),
                "name": m.get("displayName").cloned().unwrap_or(json!("")),
                "context_length": m.get("contextLength").cloned().unwrap_or(json!(0)),
                "supports_tools": m.get("supportsTools").cloned().unwrap_or(json!(false)),
                "supports_image_input": m.get("supportsImageInput").cloned().unwrap_or(json!(false)),
            })
        })
        .collect();
    let fast_routers: &[(&str, &str)] = &[
        (
            "accounts/fireworks/models/glm-5p2",
            "accounts/fireworks/routers/glm-5p2-fast",
        ),
        (
            "accounts/fireworks/models/kimi-k2p6",
            "accounts/fireworks/routers/kimi-k2p6-fast",
        ),
        (
            "accounts/fireworks/models/kimi-k2p7-code",
            "accounts/fireworks/routers/kimi-k2p7-code-fast",
        ),
        (
            "accounts/fireworks/models/kimi-k3",
            "accounts/fireworks/routers/kimi-k3-fast",
        ),
    ];
    for (std_id, fast_id) in fast_routers {
        if let Some(base_model) = models
            .iter()
            .find(|m| m.get("id").and_then(|i| i.as_str()) == Some(*std_id))
            .cloned()
        {
            let name_of_base = stdpara_name(&base_model);
            let mut fast = base_model;
            if let Some(o) = fast.as_object_mut() {
                o.insert("id".into(), json!(fast_id));
                if let Some(name) = name_of_base {
                    o.insert("name".into(), json!(format!("{name} (fast)")));
                }
            }
            models.push(fast);
        }
    }
    json_resp(StatusCode::OK, json!({ "data": models }), origin)
}

fn stdpara_name(m: &Value) -> Option<String> {
    m.get("name").and_then(|n| n.as_str()).map(String::from)
}

/// Azure 状态探针（:1934-2011：models 检查 + chat probe 读 model）
async fn azure_status(st: &BackendState, body: &Value, origin: Option<HeaderValue>) -> Response {
    let base = s_field(body, "azure_base_url").unwrap_or_default();
    let dep = s_field(body, "azure_deployment_name").unwrap_or_default();
    let ver = s_field(body, "azure_api_version").unwrap_or_default();
    let key = read_secret(st, "api_key_azure_openai").unwrap_or_default();
    if key.is_empty() || base.is_empty() || dep.is_empty() || ver.is_empty() {
        return apply_cors_resp(StatusCode::BAD_REQUEST.into_response(), origin);
    }
    let base = base.trim_end_matches('/');
    let base = match validate_egress_url(base, EgressPolicy::ProxyUrl) {
        Ok(b) => b,
        Err(e) => {
            return json_resp(
                StatusCode::BAD_REQUEST,
                json!({ "error": true, "message": format!("Azure Base URL 被出站网关拒绝: {e}") }),
                origin,
            )
        }
    };
    let client = post_client().await;

    let models_url = format!("{base}/openai/models?api-version={ver}");
    let probe = client
        .get(&models_url)
        .header("api-key", &key)
        .header("Accept", "application/json")
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await;
    let Ok(probe) = probe else {
        return json_resp(
            StatusCode::INTERNAL_SERVER_ERROR,
            json!({ "error": true, "message": "Failed to connect to the Azure endpoint." }),
            origin,
        );
    };
    if !probe.status().is_success() {
        let message = azure_status_error_map(probe.status().as_u16())
            .unwrap_or("Azure Models endpoint error");
        let code = StatusCode::from_u16(probe.status().as_u16())
            .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        return json_resp(code, json!({ "error": true, "message": message }), origin);
    }

    let chat_url = format!("{base}/openai/deployments/{dep}/chat/completions?api-version={ver}");
    let payload = json!({
        "messages": [{ "role": "user", "content": "Say word Hi" }],
        "stream": false,
        "max_completion_tokens": 5,
    });
    let probe2 = client
        .post(&chat_url)
        .header("api-key", &key)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json")
        .timeout(std::time::Duration::from_secs(60))
        .json(&payload)
        .send()
        .await;
    let Ok(probe2) = probe2 else {
        return json_resp(
            StatusCode::INTERNAL_SERVER_ERROR,
            json!({ "error": true, "message": "Failed to connect to the Azure endpoint." }),
            origin,
        );
    };
    let model_json = probe2.json::<Value>().await.unwrap_or(json!({}));
    match model_json
        .get("model")
        .and_then(|m| m.as_str())
        .map(String::from)
    {
        Some(model_id) => json_resp(
            StatusCode::OK,
            json!({ "data": [{ "id": model_id }] }),
            origin,
        ),
        None => json_resp(StatusCode::OK, json!({ "data": [] }), origin),
    }
}

fn azure_status_error_map(status: u16) -> Option<&'static str> {
    match status {
        400 => Some("API version may be invalid for this resource."),
        401 | 403 => Some("Invalid API key or insufficient permissions."),
        404 => Some("Endpoint URL appears incorrect (404)."),
        _ => None,
    }
}

/// workers_ai：/ai/models/search?task=Text+Generation&per_page=1000（:2019-2061）
async fn workers_ai_status(
    st: &BackendState,
    body: &Value,
    origin: Option<HeaderValue>,
) -> Response {
    let key = read_secret(st, "api_key_workers_ai").unwrap_or_default();
    let account_id = s_field(body, "workers_ai_account_id")
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    if key.is_empty() || account_id.is_empty() {
        return apply_cors_resp(StatusCode::BAD_REQUEST.into_response(), origin);
    }
    let url = format!(
        "{API_WORKERS_AI}/{account_id}/ai/models/search?task=Text+Generation&per_page=1000"
    );
    let resp = post_client()
        .await
        .get(&url)
        .header("Authorization", format!("Bearer {key}"))
        .header("accept-encoding", "identity")
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await;
    match resp {
        Ok(r) if r.status().is_success() => {
            let data = r.json::<Value>().await.unwrap_or(json!({}));
            let models: Vec<Value> = data
                .get("result")
                .and_then(|r| r.as_array())
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .map(|m| {
                    let mut o = m;
                    if let Some(name) = o.get("name").cloned() {
                        o.as_object_mut().map(|om| om.insert("id".into(), name));
                    }
                    o
                })
                .collect();
            json_resp(StatusCode::OK, json!({ "data": models }), origin)
        }
        Ok(r) => apply_cors_resp(
            StatusCode::from_u16(r.status().as_u16())
                .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
                .into_response(),
            origin,
        ),
        Err(_) => apply_cors_resp(StatusCode::INTERNAL_SERVER_ERROR.into_response(), origin),
    }
}

/// pollinations：gen.pollinations.ai/text 裸数组 → {data:[{...}|{id:name}]}
async fn pollinations_status(
    st: &BackendState,
    body: &Value,
    origin: Option<HeaderValue>,
) -> Response {
    let is_anon = s_field(body, "pollinations_endpoint").as_deref() == Some("anonymous");
    let key = if is_anon {
        "anonymous".to_string()
    } else {
        read_secret(st, "api_key_pollinations").unwrap_or_default()
    };
    if key.is_empty() {
        return apply_cors_resp(StatusCode::BAD_REQUEST.into_response(), origin);
    }
    let resp = post_client()
        .await
        .get("https://gen.pollinations.ai/text/models")
        .header("Authorization", format!("Bearer {key}"))
        .header("accept-encoding", "identity")
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await;
    let Ok(resp) = resp else {
        return json_resp(
            StatusCode::OK,
            json!({ "error": true, "data": { "data": [] } }),
            origin,
        );
    };
    if !resp.status().is_success() {
        return json_resp(
            StatusCode::OK,
            json!({ "error": true, "data": { "data": [] } }),
            origin,
        );
    }
    let text = resp.text().await.unwrap_or_default();
    let v = serde_json::from_str::<Value>(&text).unwrap_or(json!([]));
    if v.is_array() {
        let data: Vec<Value> = v
            .as_array()
            .unwrap()
            .iter()
            .map(|m| {
                let mut o = m.clone();
                if let Some(name) = m.get("name").cloned() {
                    o.as_object_mut().map(|om| om.insert("id".into(), name));
                }
                o
            })
            .collect();
        return json_resp(StatusCode::OK, json!({ "data": data }), origin);
    }
    json_resp(StatusCode::OK, v, origin)
}

/* ============================ tests ============================ */

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn enrich_multimodal_resolves_image_and_data_url() {
        let tmp = super::super::test_support::TestBackend::temp("stoc-provider-multimodal-");
        let root = tmp.path().to_path_buf();
        let user_img_dir = root.join("default-user").join("user").join("images").join("test_char");
        std::fs::create_dir_all(&user_img_dir).unwrap();
        let png_bytes = crate::compat::png::minimal_png();
        std::fs::write(user_img_dir.join("photo.png"), &png_bytes).unwrap();

        let st = super::super::test_support::TestBackend::new(
            tmp,
            BackendState {
                token: std::sync::Arc::new("t".into()),
                data_root: root,
                max_body_bytes: 10 << 20,
                settings_svc: Default::default(),
                chats_svc: Default::default(),
                maid_tokens: Default::default(),
                vault: std::sync::Arc::new(crate::backend::vault::Vault::new_memory()),
            },
        );

        let body = json!({
            "messages": [
                { "role": "user", "content": "看图", "image": "user-images/test_char/photo.png" }
            ]
        });
        let enriched = enrich_multimodal_messages(&st, &body);
        let msgs = enriched["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 1);
        let content = msgs[0]["content"].as_array().unwrap();
        assert_eq!(content.len(), 2);
        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[0]["text"], "看图");
        assert_eq!(content[1]["type"], "image_url");
        let url = content[1]["image_url"]["url"].as_str().unwrap();
        assert!(url.starts_with("data:image/png;base64,"));
        assert!(msgs[0].get("image").is_none());
    }

    #[test]
    fn egress_gateway_rules() {
        use EgressPolicy::*;
        // link-local / 0.0.0.0（云元数据面）—— 任何 policy 都拒
        for policy in [SelfHosted, ProxyUrl] {
            assert!(
                validate_egress_url("http://169.254.169.254/latest", policy).is_err(),
                "metadata http 拒"
            );
            assert!(
                validate_egress_url("https://169.254.169.254/latest", policy).is_err(),
                "metadata https 拒"
            );
            assert!(validate_egress_url("https://0.0.0.0/v1", policy).is_err());
            assert!(validate_egress_url("https://[fe80::1]/v1", policy).is_err());
        }
        // 字面私网：代理面仍拒
        assert!(validate_egress_url("https://10.0.0.5/v1", ProxyUrl).is_err());
        assert!(validate_egress_url("https://172.16.1.5/v1", ProxyUrl).is_err());
        assert!(validate_egress_url("https://192.168.1.5/v1", ProxyUrl).is_err());
        assert!(validate_egress_url("https://[fc00::5]/v1", ProxyUrl).is_err());
        // SelfHosted（custom_url / textcomp api_server / translate 自建 URL）：
        // http/https 均可，环回与私网放行 —— LM Studio / vLLM / KoboldCpp /
        // LibreTranslate / DeepLX 的本机与局域网部署就是这些地址形态
        assert!(
            validate_egress_url("http://127.0.0.1:5001/v1", SelfHosted).is_ok(),
            "环回 http 放行"
        );
        assert!(validate_egress_url("http://localhost:8008/v1", SelfHosted).is_ok());
        assert!(validate_egress_url("http://[::1]:8080", SelfHosted).is_ok());
        assert!(validate_egress_url("http://192.168.1.50:1234/v1", SelfHosted).is_ok());
        assert!(
            validate_egress_url("http://10.0.0.5:5001", SelfHosted).is_ok(),
            "局域网 textgen api_server http 放行"
        );
        assert!(validate_egress_url("https://192.168.1.5:8443/v1", SelfHosted).is_ok());
        assert!(validate_egress_url("https://[fc00::5]/v1", SelfHosted).is_ok());
        assert!(validate_egress_url("https://api.x.com/v1", SelfHosted).is_ok());
        // ProxyUrl（reverse_proxy / azure_base_url）：http 一律拒
        assert!(validate_egress_url("http://10.0.0.5:9000", ProxyUrl).is_err());
        assert!(validate_egress_url("http://api.x.com/v1", ProxyUrl).is_err());
        // scheme 白名单 / 无协议
        assert!(validate_egress_url("ftp://x", SelfHosted).is_err());
        assert!(
            validate_egress_url("127.0.0.1:5001", SelfHosted).is_err(),
            "无协议拒"
        );
    }

    #[test]
    fn egress_dns_pin_with_injected_resolver() {
        // 解析后任一 IP 落禁用段 → 拒（注入解析器，不打真实 DNS）
        let resolve_private = |_host: &str, _port: u16| -> std::io::Result<Vec<std::net::IpAddr>> {
            Ok(vec!["10.1.2.3".parse().unwrap()])
        };
        assert!(validate_egress_url_with(
            "https://rebind.example.com/v1",
            EgressPolicy::ProxyUrl,
            resolve_private
        )
        .is_err());
        // 解析失败 → fail-closed
        let resolve_fail = |_host: &str, _port: u16| -> std::io::Result<Vec<std::net::IpAddr>> {
            Err(std::io::Error::other("dns down"))
        };
        assert!(validate_egress_url_with(
            "https://api.openai.com/v1",
            EgressPolicy::ProxyUrl,
            resolve_fail
        )
        .is_err());
        // 解析为公网 IP → 放行
        let resolve_public = |_host: &str, _port: u16| -> std::io::Result<Vec<std::net::IpAddr>> {
            Ok(vec!["93.184.216.34".parse().unwrap()])
        };
        assert!(validate_egress_url_with(
            "https://api.openai.com/v1",
            EgressPolicy::ProxyUrl,
            resolve_public
        )
        .is_ok());
        // 自建端点（SelfHosted）：域名解析到私网 → 放行（局域网自建服务器）
        let resolve_lan = |_host: &str, _port: u16| -> std::io::Result<Vec<std::net::IpAddr>> {
            Ok(vec!["192.168.1.50".parse().unwrap()])
        };
        assert!(validate_egress_url_with(
            "http://inference.lan:1234/v1",
            EgressPolicy::SelfHosted,
            resolve_lan
        )
        .is_ok());
        // 自建端点：域名解析到 link-local → 仍拒（云元数据面）
        let resolve_meta = |_host: &str, _port: u16| -> std::io::Result<Vec<std::net::IpAddr>> {
            Ok(vec!["169.254.169.254".parse().unwrap()])
        };
        assert!(validate_egress_url_with(
            "http://rebind.example.com/v1",
            EgressPolicy::SelfHosted,
            resolve_meta
        )
        .is_err());
    }

    #[test]
    fn claude_outbound_header_shape() {
        fn header<'a>(hs: &'a [(String, String)], k: &str) -> &'a str {
            hs.iter()
                .find(|(key, _)| key == k)
                .map(|(_, v)| v.as_str())
                .unwrap_or("")
        }
        let hs = claude_outbound_headers("sk-ant-key", false);
        // x-api-key（非 Bearer）+ anthropic-version + 无条件 anthropic-beta（ST:247）
        assert_eq!(header(&hs, "x-api-key"), "sk-ant-key");
        assert!(
            !hs.iter().any(|(k, _)| k == "Authorization"),
            "claude 不发 Bearer"
        );
        assert_eq!(header(&hs, "anthropic-version"), "2023-06-01");
        assert_eq!(
            header(&hs, "anthropic-beta"),
            "output-128k-2025-02-19,context-1m-2025-08-07"
        );
        // useTools 追加 tools beta（ST:290）
        let hs_tools = claude_outbound_headers("k", true);
        assert_eq!(
            header(&hs_tools, "anthropic-beta"),
            "output-128k-2025-02-19,context-1m-2025-08-07,tools-2024-05-16"
        );
    }

    #[test]
    fn upstream_err_display_strips_url() {
        // 真实 reqwest 连接错误（环回拒绝连接，离线可复现）→ Display 剥 URL
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let err = rt.block_on(async {
            reqwest::Client::builder()
                .no_proxy()
                .build()
                .unwrap()
                .post("http://127.0.0.1:1/v1/chat/completions?key=sk-secret")
                .send()
                .await
                .expect_err("环回 :1 应连接失败")
        });
        let scrubbed = fmt_upstream_err(&err);
        assert!(
            !scrubbed.contains("127.0.0.1:1"),
            "URL 应被剥除: {scrubbed}"
        );
        assert!(
            !scrubbed.contains("sk-secret"),
            "查询串不应残留: {scrubbed}"
        );
        assert!(!scrubbed.is_empty(), "错误描述应保留");
        assert!(!err.to_string().is_empty());
    }

    #[test]
    fn json_schema_wraps_openai_shape() {
        let body = json!({
            "json_schema": { "name": "out", "strict": true, "value": { "type": "object" } },
        });
        let mut body_params = Map::new();
        json_schema_response_format(&mut body_params, &body);
        assert!(body_params["response_format"]["json_schema"]["schema"].is_object());
        assert_eq!(body_params["response_format"]["json_schema"]["name"], "out");
    }

    #[test]
    fn yaml_merge_semantics() {
        // boolean true 输入（App 的 status 分支字面）→ no-op
        let mut headers = Vec::new();
        merge_yaml_into_headers(&mut headers, Value::Bool(true));
        assert!(headers.is_empty());
        // YAML 字符串（App 的 generate 分支字面）→ 并入
        let mut body_params = Map::new();
        merge_yaml_into_body_params(
            &mut body_params,
            Value::String("chat_template_kwargs:\n  enable_thinking: true".into()),
        );
        assert_eq!(body_params["chat_template_kwargs"]["enable_thinking"], true);
        // exclude：数组字面
        let mut rb: Map<String, Value> = Map::new();
        rb.insert("top_p".into(), json!(0.5));
        exclude_yaml_keys(&mut rb, "- top_p");
        assert!(rb.get("top_p").is_none());
    }

    #[test]
    fn effort_mappings() {
        use crate::backend::providers_text as txt;
        assert_eq!(
            txt::map_openai_reasoning_effort("gpt-5.3-chat-latest", "high"),
            "medium"
        );
        assert_eq!(txt::map_openai_reasoning_effort("o3", "min"), "minimal");
        assert_eq!(txt::map_nanogpt_reasoning_effort("min"), "none");
        assert!(txt::is_openai_reasoning_effort_model("gpt-6-astra"));
        assert!(txt::is_openai_verbosity_model("gpt-5"));
    }

    #[test]
    fn cohere_wrap_joins_text_parts() {
        let wrapped = wrap_cohere_response(json!({
            "id": "x",
            "finish_reason": "COMPLETE",
            "message": { "role": "assistant", "content": [
                { "type": "thinking", "thinking": "h" },
                { "type": "text", "text": "answer" },
            ] },
        }));
        assert_eq!(wrapped["choices"][0]["message"]["content"], "answer");
        assert_eq!(wrapped["choices"][0]["finish_reason"], "stop");
    }

    #[test]
    fn find_sse_sep_shapes() {
        assert_eq!(find_sse_sep(b"abc\n\ndef"), Some(3));
        assert_eq!(find_sse_sep(b"abc\r\n\r\ndef"), Some(3));
        assert_eq!(find_sse_sep(b"abc"), None);
        assert_eq!(sse_sep_len(b"x\n\ny", 1), 2);
        assert_eq!(sse_sep_len(b"x\r\n\r\ny", 1), 4);
    }

    #[test]
    fn data_lines_extraction() {
        let got = data_lines(b"event: ping\r\ndata: {\"a\":1}\r\ndata: [DONE]");
        assert_eq!(got, vec!["{\"a\":1}".to_string(), "[DONE]".to_string()]);
    }
}
