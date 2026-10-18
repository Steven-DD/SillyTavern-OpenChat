//! 全局：MSVC linker 本地化 stdout 警告抑制（仓库测试文件公约）
#![allow(linker_messages)]
//! T4 最小双跑框架（批18 · task-6 工作项 3）
//!
//! 定位（方案 §4.2 · 双跑铁律）：双跑期同一请求若重复有副作用，会撕裂
//! ST/native 两份数据 —— 因此按 endpoint-manifest 的「方法+路径」得出
//! 端点读写分类表，收敛为一个 dual_send 门：
//! - **写端点 → 直接 Err，一次都不发**（防双发铁律，验收字面）；
//! - **读端点 → 同请求双发 → 双侧归一化（剔时间戳 / 随机 id / usage 数值）→
//!   差异报告**（无差异为空报告，有差异逐条列出路径）。
//!
//! 归一化是对称的（两侧同一函数），剔除域内的真随机值不会造成假差异；
//! 剔除只是去掉不可比字段 —— 非剔除域任何差异都会进报告，不吞错误。
//!
//! 本文件自带四条验收测试 + 归一化/分类表单测；上游 mock 一律
//! 127.0.0.1 本地起（不出网）。custom 源 generate 用 https 字面 IP
//! URL 只测请求组装面（URL 拼接 + 网关放行 + ST 错误信封），
//! 连接目标是本机拒绝列表之外必然失败的 127.0.0.1:1 端口 —— 零 DNS、零出站。

use serde_json::{json, Map, Value};
use st_chat_lib::backend;
use std::ops::Drop;
use std::path::PathBuf;

/* ------------------------------------------------------------------ *
 * 端点读写分类表（按 manifest 方法+路径推导后硬编码；与
 * docs/agents/endpoint-manifest.md 同步维护 —— 前端出口变更须回写清单后改表）
 * ------------------------------------------------------------------ */

/// 写端点清单（method, path）：manifest 各族的副作用面。
const WRITE_ENDPOINTS: &[(&str, &str)] = &[
    // 设置族（#29）
    ("POST", "/api/settings/save"),
    // 会话族写面（#10/#11/#12/#16/#17；#9/#13/#14/#15/#18 为读）
    ("POST", "/api/chats/save"),
    ("POST", "/api/chats/rename"),
    ("POST", "/api/chats/delete"),
    ("POST", "/api/chats/group/save"),
    ("POST", "/api/chats/group/delete"),
    // 角色卡族写面（#4-#8 multipart/析构面；#1/#2/#3 读）
    ("POST", "/api/characters/create"),
    ("POST", "/api/characters/edit"),
    ("POST", "/api/characters/import"),
    ("POST", "/api/characters/delete"),
    ("POST", "/api/characters/rename"),
    // 群组族（#20-#22；#19 读）
    ("POST", "/api/groups/create"),
    ("POST", "/api/groups/edit"),
    ("POST", "/api/groups/delete"),
    // 世界书族写面（#26/#27；#25 edit 整文件覆盖同写）
    ("POST", "/api/worldinfo/edit"),
    ("POST", "/api/worldinfo/import"),
    ("POST", "/api/worldinfo/delete"),
    // 资产族写面（#36/#37/#39；avatars/get {读}、thumbnails/静态 为 GET 读）
    ("POST", "/api/avatars/upload"),
    ("POST", "/api/avatars/delete"),
    ("POST", "/api/images/upload"),
    // 扩展 git 管理族（#69-#75 全部有副作用；#68 discover 为 GET 读）
    ("POST", "/api/extensions/install"),
    ("POST", "/api/extensions/update"),
    ("POST", "/api/extensions/version"),
    ("POST", "/api/extensions/branches"),
    ("POST", "/api/extensions/switch"),
    ("POST", "/api/extensions/move"),
    ("POST", "/api/extensions/delete"),
    // secrets / quick-replies（#50/#67）
    ("POST", "/api/secrets/write"),
    ("POST", "/api/quick-replies/save"),
    // data-maid 副作用面（#63/#64；vector/insert|delete 同 destruction 家族）
    ("POST", "/api/data-maid/finalize"),
    ("POST", "/api/data-maid/delete"),
    ("POST", "/api/vector/insert"),
    ("POST", "/api/vector/delete"),
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Class {
    Write,
    Read,
}

fn classify(method: &str, path: &str) -> Class {
    if WRITE_ENDPOINTS
        .iter()
        .any(|(m, p)| *m == method && *p == path)
    {
        Class::Write
    } else {
        Class::Read
    }
}

/* ------------------------------------------------------------------ *
 * 双跑门：dual_send
 * ------------------------------------------------------------------ */

#[derive(Debug)]
enum DualError {
    /// 防双发铁律：写端点拒绝双发，一次请求都不发出
    WriteRefused { endpoint: String },
    /// 传输/客户端层失败（单发层面错误）
    Transport(String),
}

impl DualError {
    fn message(&self) -> String {
        match self {
            DualError::WriteRefused { endpoint } => format!(
                "dual_send 拒绝写端点 {endpoint}（防双发铁律：写操作在双跑期只允许单发，未发出任何请求）"
            ),
            DualError::Transport(e) => format!("dual_send 传输失败: {e}"),
        }
    }
}

#[derive(Debug)]
struct DualOutcome {
    endpoint: String,
    status: (u16, u16),
    /// 归一化后的双侧响应体（SSE 端点 = 归一化事件序列数组）
    norm: (Value, Value),
    /// 差异报告：空 = 双发输出一致
    diffs: Vec<String>,
}

impl DualOutcome {
    fn report_text(&self) -> String {
        let mut s = format!(
            "[双跑差异报告] {}：状态 ({}, {})，差异 {} 条\n",
            self.endpoint,
            self.status.0,
            self.status.1,
            self.diffs.len()
        );
        for d in &self.diffs {
            s.push_str("  - ");
            s.push_str(d);
            s.push('\n');
        }
        s
    }
}

/// 双发门：写端点直接 Err（不发）；读端点同请求双发 → 归一化 → 差异。
async fn dual_send(
    ctx: &Ctx,
    method: &str,
    path: &str,
    body: Option<Value>,
) -> Result<DualOutcome, DualError> {
    if classify(method, path) == Class::Write {
        return Err(DualError::WriteRefused {
            endpoint: format!("{method} {path}"),
        });
    }
    let r1 = send_once(ctx, method, path, body.as_ref()).await?;
    let r2 = send_once(ctx, method, path, body.as_ref()).await?;

    let mut diffs = Vec::new();
    if r1.status != r2.status {
        diffs.push(format!("HTTP 状态：双侧不一致（{} vs {}）", r1.status, r2.status));
    }
    collect_diffs(&r1.norm, &r2.norm, String::new(), &mut diffs);

    Ok(DualOutcome {
        endpoint: format!("{method} {path}"),
        status: (r1.status, r2.status),
        norm: (r1.norm, r2.norm),
        diffs,
    })
}

struct Single {
    status: u16,
    norm: Value,
}

/// 单发 + 归一化：SSE 流 → 逐帧事件序列；普通响应 → 归一化 JSON。
async fn send_once(ctx: &Ctx, method: &str, path: &str, body: Option<&Value>) -> Result<Single, DualError> {
    let is_sse_body = body
        .map(|b| b.get("stream").and_then(|v| v.as_bool()).unwrap_or(false))
        .unwrap_or(false);
    let req_builder = if method == "GET" {
        ctx.http.get(format!("{}{}", ctx.base, path))
    } else {
        ctx.http.post(format!("{}{}", ctx.base, path))
    };
    let mut req = req_builder
        .header("x-relay-auth", &ctx.token)
        .header("x-csrf-token", &ctx.token);
    match body {
        Some(v) => {
            req = req.json(v);
        }
        None => {
            req = req.header("content-type", "application/json");
        }
    }
    let resp = req.send().await.map_err(|e| DualError::Transport(e.to_string()))?;
    let status = resp.status().as_u16();
    let ctype = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();

    let text = resp.text().await.map_err(|e| DualError::Transport(e.to_string()))?;
    let norm = if ctype.contains("text/event-stream") || (is_sse_body && !text.trim_start().starts_with('{')) {
        Value::Array(sse_event_sequence(&text))
    } else if let Ok(v) = serde_json::from_str::<Value>(&text) {
        normalize(v)
    } else {
        Value::String(text)
    };
    Ok(Single { status, norm })
}

/* ------------------------------------------------------------------ *
 * 归一化：剔时间戳 / 随机 id / usage 数值字段
 * ------------------------------------------------------------------ */

/// 时间戳语义的键（值替换为掩码；键保留以便结构可比）
const TIMESTAMP_KEYS: &[&str] = &[
    "gen_started",
    "gen_finished",
    "send_date",
    "last_mes",
    "last_modified",
    "created",
    "created_at",
    "updated_at",
    "modified_at",
    "deleted_at",
    "timestamp",
    "expires",
    "expires_at",
    "lastTriggered",
];
/// 随机 id 语义的键（值形似随机串时替换为掩码）
const RANDOM_ID_KEYS: &[&str] = &[
    "id",
    "_id",
    "request_id",
    "requestId",
    "message_id",
    "messageId",
    "task_id",
    "taskId",
    "job_id",
    "jobId",
];

const MASK_CLOCK: &str = "<ts>";
const MASK_RANDOM: &str = "<random-id>";

fn normalize(v: Value) -> Value {
    match v {
        Value::Object(map) => {
            let mut out = Map::new();
            for (k, val) in map {
                // usage：数值域整体剔除
                if k == "usage" {
                    continue;
                }
                if val.is_string() && is_iso_timestamp(val.as_str().unwrap()) {
                    out.insert(k, json!(MASK_CLOCK));
                    continue;
                }
                if (
                    TIMESTAMP_KEYS.contains(&k.as_str())
                        && (val.is_string() || val.is_number())
                ) || (RANDOM_ID_KEYS.contains(&k.as_str()) && is_randomish(&val)) {
                    out.insert(k, json!(MASK_RANDOM));
                    continue;
                }
                out.insert(k, normalize(val));
            }
            Value::Object(out)
        }
        Value::Array(a) => Value::Array(a.into_iter().map(normalize).collect()),
        other => other,
    }
}

/// RFC3339/ISO-8601 时刻形值（ST 时间戳面）
fn is_iso_timestamp(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() >= 20
        && b[4] == b'-'
        && b[7] == b'-'
        && (b[10] == b'T' || b[10] == b' ')
        && b[13] == b':'
        && b[16] == b':'
}

/// 随机串判定：24+ 位的 hex/hex-连字符串（uuid 与 64-bit token 族）
fn is_randomish(v: &Value) -> bool {
    let Some(s) = v.as_str() else {
        return false;
    };
    s.len() >= 24 && s.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
}

/* ------------------------------------------------------------------ *
 * SSE → 事件序列（data 帧 + [DONE] 标记；注释/空行按 SSE 语义忽略）
 * ------------------------------------------------------------------ */

fn sse_event_sequence(text: &str) -> Vec<Value> {
    let mut out = Vec::new();
    for line in text.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("data:") {
            let rest = rest.trim();
            if rest == "[DONE]" {
                out.push(json!("done"));
            } else if let Ok(v) = serde_json::from_str::<Value>(rest) {
                out.push(normalize(v));
            } else {
                out.push(json!(rest));
            }
        }
    }
    out
}

/* ------------------------------------------------------------------ *
 * 差异报告（发生在叶级：路径 + 双侧渲染）
 * ------------------------------------------------------------------ */

fn collect_diffs(a: &Value, b: &Value, path: String, out: &mut Vec<String>) {
    if a == b {
        return;
    }
    match (a, b) {
        (Value::Object(ma), Value::Object(mb)) => {
            let mut keys: Vec<&String> = ma.keys().chain(mb.keys()).collect();
            keys.sort();
            keys.dedup();
            for k in keys {
                let child = if path.is_empty() {
                    format!("/{}", k)
                } else {
                    format!("{}/{}", path, k)
                };
                match (ma.get(k), mb.get(k)) {
                    (Some(x), Some(y)) => collect_diffs(x, y, child, out),
                    (Some(x), None) => out.push(format!("{child}: 仅响应甲有 {x}")),
                    (None, Some(y)) => out.push(format!("{child}: 仅响应乙有 {y}")),
                    (None, None) => unreachable!(),
                }
            }
        }
        (Value::Array(la), Value::Array(lb)) => {
            for (i, (x, y)) in la.iter().zip(lb.iter()).enumerate() {
                let child = format!("{path}[{i}]");
                collect_diffs(x, y, child, out);
            }
            if la.len() != lb.len() {
                out.push(format!(
                    "{path}: 长度不同（{} vs {}）",
                    la.len(),
                    lb.len()
                ));
            }
        }
        _ => out.push(format!("{path}: 甲={a} | 乙={b}")),
    }
}

/* ------------------------------------------------------------------ *
 * backend 启动件（形态抄 backend/mod.rs 既有测试；不出网）
 * ------------------------------------------------------------------ */

struct Ctx {
    base: String,
    token: String,
    root: PathBuf,
    http: reqwest::Client,
}

impl Drop for Ctx {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

async fn spawn_backend(label: &str) -> Ctx {
    let root =
        std::env::temp_dir().join(format!("stchat-dualrun-{label}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let ep = backend::start(root.clone()).await.expect("backend 启动失败");
    Ctx {
        base: format!("http://127.0.0.1:{}", ep.port),
        token: ep.token,
        root,
        http: reqwest::Client::builder()
            .no_proxy()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("reqwest client 构建失败"),
    }
}

/* ------------------------------------------------------------------ *
 * 验收 1：/api/chats/save 双发 → Err（验收字面 + 零副作用证物）
 * ------------------------------------------------------------------ */

#[tokio::test]
async fn dual_send_refuses_write_endpoint_entirely() {
    let ctx = spawn_backend("write-refuse").await;
    let body = json!({
        "avatar_url": "testchar-fixture",
        "file_name": "dual-refuse",
        "chat": [json!({ "user_name": "u", "character_name": "c", "chat_metadata": {} })],
        "force": true,
    });
    let err = dual_send(&ctx, "POST", "/api/chats/save", Some(body))
        .await
        .unwrap_err();
    assert!(
        matches!(err, DualError::WriteRefused { .. }),
        "写端点 dual_send 必须 Err，实际 {err:?}"
    );
    let msg = err.message();
    assert!(msg.contains("/api/chats/save"), "错误应点名端点: {msg}");

    // 证物：除 Err 之外，磁盘零副作用（chats 目录树内没有任何 .jsonl）
    let chats_dir = ctx.root.join("default-user").join("chats");
    let jsonl_exists = std::fs::read_dir(&chats_dir)
        .map(|d| {
            d.flatten()
                .any(|e| e.path().extension().map(|x| x == "jsonl").unwrap_or(false))
        })
        .unwrap_or(false);
    assert!(!jsonl_exists, "写端点被拒后不得有任何落盘副作用");

    // 分类表自证：write 是查表所得（防表漏）
    assert_eq!(classify("POST", "/api/chats/save"), Class::Write);
}

/* ------------------------------------------------------------------ *
 * 验收 2：/api/settings/get 双发 → 差异报告（空差异 = 绿）
 * ------------------------------------------------------------------ */

#[tokio::test]
async fn dual_send_read_endpoint_settings_diff_report() {
    let ctx = spawn_backend("read-dual").await;
    // 先放一份 settings 内容（写端点用单发直发 —— 本测试只走 fix 语义，不经 dual_send）
    let r = ctx
        .http
        .post(format!("{}/api/settings/save", ctx.base))
        .header("x-relay-auth", &ctx.token)
        .json(&json!({ "username": "双跑甲板员", "dpr_run": true, "unknown_key_0": "保真" }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 200);

    let out = dual_send(&ctx, "POST", "/api/settings/get", Some(json!({})))
        .await
        .unwrap_or_else(|e| panic!("读端点 dual_send 不应失败: {e:?}"));
    let report = out.report_text();
    // 差异报告本体要真的“输出”出来（验收字面：输出差异报告）
    eprintln!("{report}");
    assert_eq!(out.status, (200, 200), "双发都应 200");
    assert!(
        out.diffs.is_empty(),
        "settings/get 双发输出差异报告非空 —— 见 {report}"
    );
    // 归一化对真数据不动手术：中文与未知键原样可比
    // （settings/get 响应是 {settings: <原文字符串>} 包装 —— 解开后再比）
    let served_str = out.norm.0["settings"].as_str().expect("settings 应为原文字符串");
    let served: Value = serde_json::from_str(served_str).expect("包装内应为合法 JSON");
    assert_eq!(served["username"], "双跑甲板员");
    assert_eq!(served["unknown_key_0"], "保真");
    let served2: Value =
        serde_json::from_str(out.norm.1["settings"].as_str().unwrap()).unwrap();
    assert_eq!(served, served2, "双发解包后逐键一致");
}

/* ------------------------------------------------------------------ *
 * 验收 3：textcomp generate SSE 双发 → 归一化事件序列 == fixture 帧序列
 * （api_server=环回地址；上游 = 127.0.0.1 mock，本进程起，不出网）
 * ------------------------------------------------------------------ */

async fn mock_sse_upstream(body: &'static str) -> u16 {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                return;
            };
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                let _ = sock.read(&mut buf).await; // 只读请求（含 body），回固定帧体
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                if sock.write_all(head.as_bytes()).await.is_err() {
                    return;
                }
                let _ = sock.write_all(body.as_bytes()).await;
                let _ = sock.flush().await;
                let _ = sock.shutdown().await;
            });
        }
    });
    port
}

#[tokio::test]
async fn textcomp_sse_dual_run_matches_fixture_frames() {
    let ctx = spawn_backend("sse").await;
    let up_port = mock_sse_upstream(crate_fixtures::SSE_FRAMES).await;

    let body = json!({
        "api_type": "generic",
        "api_server": format!("http://127.0.0.1:{up_port}"),
        "model": "fixture-model",
        "prompt": "二更起灯",
        "stream": true,
        "max_tokens": 16,
        "temperature": 0.1,
    });

    let out = dual_send(
        &ctx,
        "POST",
        "/api/backends/text-completions/generate",
        Some(body),
    )
    .await
    .unwrap_or_else(|e| panic!("textcomp generate 双发不应失败: {e:?}"));

    // 双发自身：SSE 两次透传的归一化事件序列应一致
    assert!(out.diffs.is_empty(), "双发差异报告非空: {}", out.report_text());

    // 与黄金帧序列对照（fixture 也过同一归一化 —— 对称）
    let fixture_seq = sse_event_sequence(crate_fixtures::SSE_FRAMES);
    assert_eq!(out.norm.0, Value::Array(fixture_seq), "归一化事件序列应与 fixture 一致");
    // 帧序字面：3 内容/usage 帧 → 错误帧 → [DONE]
    let seq = match &out.norm.0 {
        Value::Array(a) => a,
        _ => unreachable!(),
    };
    assert_eq!(seq.len(), 5, "事件序列帧数 + 衔接");
    assert!(seq.iter().any(|f| f.get("error").is_some()), "黄金错误帧在序列内");
    assert_eq!(seq[4], json!("done"), "[DONE] 终止帧");
    assert_eq!(seq[2]["usage"], Value::Null, "usage 域被归一化剔除");
    assert_eq!(seq[0]["fixture_probe"]["帧序"], 1, "非剔除域原样保留");
}

/* ------------------------------------------------------------------ *
 * 验收 4：custom 源 generate —— https 字面 URL 只测请求组装面（不出网）
 * ------------------------------------------------------------------ */

#[tokio::test]
async fn custom_source_https_literal_request_assembly_no_egress() {
    let ctx = spawn_backend("custom-asm").await;
    // 本机必然拒绝的环回端口 + https 字面：URL 组装可验、零 DNS（IP 字面）、零外联
    let body = json!({
        "chat_completion_source": "custom",
        "custom_url": "https://127.0.0.1:1",
        "model": "fixture-model",
        "messages": [{ "role": "user", "content": "巡灯" }],
        "temperature": 0.4,
        "max_tokens": 24,
        "custom_include_body": "fixture_probe: 保真",
        "custom_include_headers": "X-Fixture: 组装面探针",
    });
    let r = ctx
        .http
        .post(format!("{}/api/backends/chat-completions/generate", ctx.base))
        .header("x-relay-auth", &ctx.token)
        .json(&body)
        .send()
        .await
        .unwrap();
    let status = r.status().as_u16();
    let v: Value = r.json().await.unwrap();

    // 组装面证据链：
    // ① https 字面 URL 通过出站网关（不 400「仅允许 https」—— 协议校验真实放行了该 URL）；
    // ② 组装后的请求被投递到出站层（reqwest 连接错 → 502 信封；
    //    reqwest 0.12 连接错误 Display 不含 URL —— 上游 URL 字面由
    //    字面上的 custom_url 直读 + 网关行为两个证据承担，登记限制）；
    // ③ 错误信封 = 502 {error:{message:"上游连接失败: …"}}（连接失败语义）。
    assert_eq!(status, 502, "连接失败应 502，实际 {status} {v}");
    let msg = v["error"]["message"].as_str().unwrap_or_default();
    assert!(msg.contains("上游连接失败"), "信封语义: {msg}");

    // 反证控制：custom 源对环回 http 依出站策略拒绝（协议收紧是真实生效的）
    let body_http = json!({
        "chat_completion_source": "custom",
        "custom_url": "http://127.0.0.1:1",
        "model": "fixture-model",
        "messages": [{ "role": "user", "content": "巡灯" }]
    });
    let r = ctx
        .http
        .post(format!("{}/api/backends/chat-completions/generate", ctx.base))
        .header("x-relay-auth", &ctx.token)
        .json(&body_http)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 400, "custom 源环回 http 应被协议网关拒绝");
    let v: Value = r.json().await.unwrap();
    assert!(
        v["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("https"),
        "400 应说明仅允许 https: {v}"
    );
}

/* ------------------------------------------------------------------ *
 * 单测：归一化（随机 id / 时间戳 / usage 域剔除 & 真差异不被吞）+ 分类表
 * ------------------------------------------------------------------ */

#[test]
fn normalizer_masks_random_ids_timestamps_and_usage_and_keeps_real_diffs() {
    let a = json!({
        "id": "aabbccddeeff00112233445566778899aa",
        "created": 1690000000,
        "usage": { "prompt_tokens": 10, "completion_tokens": 2 },
        "send_date": "2026-10-18T09:00:00.000Z",
        "content": "中文实数据"
    });
    let b = json!({
        "id": "00112233445566778899aabbccddeeffff",
        "created": 42,
        "usage": { "prompt_tokens": 99, "completion_tokens": 88 },
        "send_date": "2020-01-01T00:00:00.000Z",
        "content": "中文实数据"
    });
    assert_eq!(normalize(a.clone()), normalize(b), "剔除域内的随机差异不得造成假差异");

    // 差异报告在非剔除域必须真报（不吞）
    let c = json!({ "content": "不同的另一份" });
    let mut diffs = Vec::new();
    collect_diffs(&normalize(a.clone()), &normalize(c.clone()), "/resp".to_string(), &mut diffs);
    assert!(!diffs.is_empty(), "真差异必须进报告");
    assert!(diffs[0].contains("content"), "差异应点出根: {diffs:?}");

    // usage 键删除但保留其余结构
    let d = normalize(json!({ "usage": { "total": 1 }, "kept": "值" }));
    assert!(d.get("usage").is_none());
    assert_eq!(d["kept"], "值");
    assert_eq!(normalize(c)["content"], "不同的另一份", "非剔除域不改写");
}

#[test]
fn classification_covers_manifest_write_face() {
    for src in [
        ("POST", "/api/settings/save"),
        ("POST", "/api/chats/save"),
        ("POST", "/api/chats/rename"),
        ("POST", "/api/chats/delete"),
        ("POST", "/api/chats/group/save"),
        ("POST", "/api/chats/group/delete"),
        ("POST", "/api/characters/create"),
        ("POST", "/api/characters/edit"),
        ("POST", "/api/characters/import"),
        ("POST", "/api/characters/delete"),
        ("POST", "/api/characters/rename"),
        ("POST", "/api/groups/create"),
        ("POST", "/api/groups/edit"),
        ("POST", "/api/groups/delete"),
        ("POST", "/api/worldinfo/edit"),
        ("POST", "/api/worldinfo/import"),
        ("POST", "/api/worldinfo/delete"),
        ("POST", "/api/images/upload"),
        ("POST", "/api/avatars/upload"),
        ("POST", "/api/avatars/delete"),
        ("POST", "/api/extensions/install"),
        ("POST", "/api/extensions/update"),
        ("POST", "/api/extensions/version"),
        ("POST", "/api/extensions/branches"),
        ("POST", "/api/extensions/switch"),
        ("POST", "/api/extensions/move"),
        ("POST", "/api/extensions/delete"),
        ("POST", "/api/secrets/write"),
        ("POST", "/api/quick-replies/save"),
        ("POST", "/api/data-maid/delete"),
        ("POST", "/api/data-maid/finalize"),
    ] {
        assert_eq!(classify(src.0, src.1), Class::Write, "{src:?} 必须分类为写");
    }
    // 生成/查询通道不在写表（可重试读类）
    assert_eq!(classify("POST", "/api/settings/get"), Class::Read);
    assert_eq!(classify("POST", "/api/chats/get"), Class::Read);
    assert_eq!(
        classify("POST", "/api/backends/text-completions/generate"),
        Class::Read
    );
    assert_eq!(classify("POST", "/api/vector/list"), Class::Read);
    assert_eq!(classify("POST", "/api/vector/insert"), Class::Write);
}

/* fixture 语料（与 golden_fixtures.rs 共一套黄金态） */
mod crate_fixtures {
    pub const SSE_FRAMES: &str = include_str!("fixtures/sse_frames_golden.txt");
}
