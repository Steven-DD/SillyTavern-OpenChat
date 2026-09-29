//! HTTP 中继集成测试（生产环境跨源方案的核心验证）
//!
//! 分两个测试，各自解决一个独立问题：
//!
//! 1. `relay_proxies_real_st` —— 对**真实 SillyTavern** 验证跨源方案：
//!    Cookie 会话代持（CSRF 双提交能过）、CORS 回显、二进制静态资源透传。
//!    要求 ST 已在 127.0.0.1:8000 运行。
//!
//! 2. `relay_streams_sse_without_buffering` —— 用**本地 mock 上游**验证 SSE 透传：
//!    逐帧下发、间隔 250ms，断言首帧**显著早于**整体结束时刻。
//!    若中继做了缓冲，首帧会拖到最后才到 —— 这是流式聊天可用性的命门。
//!    不用真模型 → 不消耗配额、离线也能跑。
//!
//! 运行：cargo test --test relay_e2e -- --nocapture
//! 注意：Rust 编译在本环境需非沙箱权限。

use futures_util::StreamExt;
use st_chat_lib::relay;
use std::net::{TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

const ST_PORT: u16 = 8000;

fn st_is_up() -> bool {
    match format!("127.0.0.1:{ST_PORT}").to_socket_addrs() {
        Ok(mut it) => it
            .next()
            .map(|sa| TcpStream::connect_timeout(&sa, Duration::from_millis(500)).is_ok())
            .unwrap_or(false),
        Err(_) => false,
    }
}

/* ================================================================ *
 * 1. 真实 ST：跨源方案（Cookie 代持 + CORS + 静态透传）
 * ================================================================ */

#[tokio::test]
async fn relay_proxies_real_st() {
    if !st_is_up() {
        // 外部依赖缺失时**跳过而不是失败**：否则任何没起 ST 的环境（CI、新机器、刚构建完）
        // 都会红，久了就没人看测试结果了。需要严格验证时设 REQUIRE_REAL_ST=1。
        if std::env::var("REQUIRE_REAL_ST").is_ok() {
            panic!("REQUIRE_REAL_ST 已设置，但 127.0.0.1:{ST_PORT} 无响应");
        }
        eprintln!("⚠️ 跳过 relay_proxies_real_st：SillyTavern 未在 {ST_PORT} 运行");
        return;
    }

    let ep = relay::start(ST_PORT).await.expect("中继启动失败");
    let port = ep.port;
    let base = format!("http://127.0.0.1:{port}");
    let c = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(60))
        .build()
        .unwrap();

    // ── 1) CSRF：token 正常返回，且 set-cookie 被中继吃掉（不给浏览器）──
    //    中继自身也要求 X-Relay-Auth 鉴权（/csrf-token 不在静态豁免清单里）
    let r = c
        .get(format!("{base}/csrf-token"))
        .header("x-relay-auth", &ep.token)
        .send()
        .await
        .unwrap();
    assert!(r.status().is_success(), "取 token 失败: {}", r.status());
    let leaked: Vec<String> = r
        .headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|v| v.to_str().ok().map(String::from))
        .collect();
    assert!(
        leaked.is_empty(),
        "中继不应把 set-cookie 透给客户端（会造成 SameSite 问题），实际: {leaked:?}"
    );
    let token = {
        let text = r.text().await.expect("读取 token 响应失败");
        let v: serde_json::Value =
            serde_json::from_str(&text).unwrap_or_else(|e| panic!("token 响应非 JSON: {e} / {text}"));
        v["token"]
            .as_str()
            .expect("token 字段缺失")
            .to_string()
    };
    assert!(!token.is_empty());

    // ── 2) 双提交 + 中继代持 Cookie → 200（跨源方案的**核心断言**）──
    //     若把 cookie 交给浏览器，这里必然 403（SameSite=Lax 不随跨站 fetch 发送）
    let r = c
        .post(format!("{base}/api/settings/get"))
        .header("content-type", "application/json")
        .header("x-csrf-token", &token)
        .header("x-relay-auth", &ep.token)
        .header("origin", "http://tauri.localhost")
        .body("{}")
        .send()
        .await
        .unwrap();
    let status = r.status().as_u16();
    let acao = r
        .headers()
        .get("access-control-allow-origin")
        .and_then(|v| v.to_str().ok().map(String::from))
        .unwrap_or_default();
    let body = r.text().await.unwrap();
    assert_eq!(
        status,
        200,
        "带中继代持 cookie 的 POST 应通过 CSRF，实际 {status}，body 前 200 字: {}",
        &body[..body.len().min(200)]
    );

    // ── 3) CORS：回显 Origin（浏览器跨源 fetch 的前提）──
    assert_eq!(
        acao, "http://tauri.localhost",
        "应回显请求 Origin，实际 {acao:?}"
    );

    // ── 4) 二进制透传（角色头像缩略图）──
    //     故意**不带** X-Relay-Auth：<img> 无法携带自定义头，静态资源 GET 必须豁免鉴权
    let r = c
        .get(format!(
            "{base}/thumbnail?type=avatar&file=default_Seraphina.png"
        ))
        .send()
        .await
        .unwrap();
    assert!(r.status().is_success(), "缩略图状态 {}", r.status());
    let bytes = r.bytes().await.unwrap();
    assert!(bytes.len() > 1000, "缩略图字节数异常: {}", bytes.len());

    // ── 5) 生成端点的鉴权链路 ──
    //   不断言一定 200：上游配额耗尽时 ST 返回**结构化 429/503 JSON**（不是 SSE 流）。
    //   关键是**不能是 403** —— 403 才说明 CSRF/Cookie 链路断了。
    //   真正的流式验证见下面的 mock 测试（不依赖配额）。
    let r = c
        .post(format!("{base}/api/backends/chat-completions/generate"))
        .header("content-type", "application/json")
        .header("x-csrf-token", &token)
        .header("x-relay-auth", &ep.token)
        .header("origin", "http://tauri.localhost")
        .body(
            r#"{"messages":[{"role":"user","content":"ping"}],"model":"gemini-3.8-flash","temperature":0,"max_tokens":16,"stream":true,"chat_completion_source":"makersuite"}"#,
        )
        .send()
        .await
        .unwrap();
    let gstatus = r.status().as_u16();
    let gbody = r.text().await.unwrap();
    let head = &gbody[..gbody.len().min(220)];
    assert_ne!(gstatus, 403, "403 = CSRF 会话代持失效；body: {head}");
    // 400 = ST 可达但未配置生成后端（全新安装没有 API key）—— 这同样是
    // 「请求经中继完整到达 ST 并带回了 ST 的结构化响应」的证据，不应判失败
    assert!(
        matches!(gstatus, 200 | 400 | 429 | 503),
        "生成端点应为 200(SSE)、400(未配置后端) 或上游结构化错误(429/503)，实际 {gstatus}: {head}"
    );
    if gstatus == 200 {
        assert!(gbody.contains("data:"), "200 时应为 SSE 帧，实际: {head}");
        println!("✅ 生成端点返回真实 SSE 流（上游配额可用）");
    } else {
        assert!(
            gbody.contains("error"),
            "非 200 时应为上游结构化错误透传，实际: {head}"
        );
        if gstatus == 400 {
            println!("ℹ️  生成端点返回 400（ST 可达但未配置 API key，全新安装的正常状态）；鉴权链路正常");
        } else {
            println!("ℹ️  生成端点返回上游 {gstatus}（配额/过载），鉴权链路正常；流式验证由 mock 测试覆盖");
        }
    }

    println!("✅ 真实 ST 验证通过：CSRF 代持 / CORS 回显 / 静态透传 / 鉴权链路 全部正常");
}

/* ================================================================ *
 * 2. mock 上游：SSE 必须逐帧透传、不得缓冲
 * ================================================================ */

/// 起一个只回 SSE 的极简 HTTP 上游，3 帧间隔 250ms
async fn mock_sse_upstream() -> u16 {
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
                let _ = sock.read(&mut buf).await; // 只读请求头，不解析
                let head = "HTTP/1.1 200 OK\r\n\
                            Content-Type: text/event-stream\r\n\
                            Cache-Control: no-cache\r\n\
                            Connection: close\r\n\r\n";
                if sock.write_all(head.as_bytes()).await.is_err() {
                    return;
                }
                for i in 0..3 {
                    let frame = format!("data: {{\"chunk\":{i}}}\r\n\r\n");
                    if sock.write_all(frame.as_bytes()).await.is_err() {
                        return;
                    }
                    let _ = sock.flush().await;
                    tokio::time::sleep(Duration::from_millis(250)).await;
                }
                let _ = sock.write_all(b"data: [DONE]\r\n\r\n").await;
                let _ = sock.flush().await;
                let _ = sock.shutdown().await;
            });
        }
    });

    port
}

#[tokio::test]
async fn relay_streams_sse_without_buffering() {
    let up = mock_sse_upstream().await;
    let ep = relay::start(up).await.expect("中继启动失败");
    let c = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(30))
        .build()
        .unwrap();

    let t0 = Instant::now();
    let r = c
        .get(format!(
            "http://127.0.0.1:{}/api/backends/chat-completions/generate",
            ep.port
        ))
        .header("x-relay-auth", &ep.token)
        .send()
        .await
        .unwrap();

    assert!(r.status().is_success(), "状态 {}", r.status());
    assert_eq!(
        r.headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok()),
        Some("text/event-stream"),
        "content-type 应原样透传"
    );

    let mut stream = r.bytes_stream();
    let mut first_at: Option<Duration> = None;
    let mut batches = 0usize;
    let mut all = String::new();

    while let Some(chunk) = stream.next().await {
        let b = chunk.expect("读取流失败");
        if first_at.is_none() {
            first_at = Some(t0.elapsed());
        }
        batches += 1;
        all.push_str(&String::from_utf8_lossy(&b));
    }

    let total = t0.elapsed();
    let first = first_at.expect("未收到任何分片");

    assert!(all.contains("[DONE]"), "流内容不完整: {all:?}");
    assert!(all.contains("\"chunk\":2"), "流内容不完整: {all:?}");
    assert!(
        batches >= 2,
        "应分批到达（逐帧透传），实际只有 {batches} 批 —— 说明中继做了缓冲"
    );
    assert!(
        first * 2 < total,
        "疑似缓冲：首帧在 {first:?} 到达，整体耗时 {total:?}（首帧应显著早于结束时刻）"
    );

    println!(
        "✅ SSE 流式验证通过：首帧 {first:?} / 整体 {total:?} / 分 {batches} 批到达（无缓冲）"
    );
}

/* ================================================================ *
 * 3. mock 上游：鉴权与 CORS 白名单（不依赖真实 ST，离线可跑）
 *
 *    中继代持着 ST 会话 Cookie，必须验证：
 *    - 无/错 token 的 API 请求 → 403（本机其它进程、恶意网页两条路都走不通）
 *    - 静态资源 GET 豁免（<img> 带不了自定义头，不能误杀头像）
 *    - CORS 只对白名单 Origin 反射 ACAO
 * ================================================================ */

#[tokio::test]
async fn relay_rejects_unauthenticated_requests() {
    let up = mock_sse_upstream().await;
    let ep = relay::start(up).await.expect("中继启动失败");
    let base = format!("http://127.0.0.1:{}", ep.port);
    let c = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap();

    // ── 1) POST /api/*：无 token → 403（token 缺失时请求不得到达上游）──
    let r = c
        .post(format!("{base}/api/settings/get"))
        .header("content-type", "application/json")
        .body("{}")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 403, "无 token 的 POST 必须被拒");

    // ── 2) 错误 token → 403 ──
    let r = c
        .post(format!("{base}/api/settings/get"))
        .header("content-type", "application/json")
        .header("x-relay-auth", "deadbeef")
        .body("{}")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 403, "错误 token 必须被拒");

    // ── 3) 正确 token → 透传到 mock 上游（mock 恒回 200 SSE）──
    let r = c
        .post(format!("{base}/api/settings/get"))
        .header("content-type", "application/json")
        .header("x-relay-auth", &ep.token)
        .body("{}")
        .send()
        .await
        .unwrap();
    assert!(
        r.status().is_success(),
        "正确 token 应放行，实际 {}",
        r.status()
    );

    // ── 4) GET /api/*：无 token → 403（读接口同样是攻击面）──
    let r = c
        .get(format!("{base}/api/characters/all"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 403, "无 token 的 GET /api 必须被拒");

    // ── 5) 静态资源 GET 豁免：无 token 也放行（<img> 场景）──
    let r = c
        .get(format!("{base}/thumbnail?type=avatar&file=x.png"))
        .send()
        .await
        .unwrap();
    assert!(
        r.status().is_success(),
        "静态 GET 应豁免鉴权，实际 {}",
        r.status()
    );

    // ── 6) CORS 白名单：恶意 Origin 的预检不放行 ACAO ──
    let r = c
        .request(reqwest::Method::OPTIONS, format!("{base}/api/settings/get"))
        .header("origin", "https://evil.example")
        .header("access-control-request-method", "POST")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 204);
    assert!(
        r.headers().get("access-control-allow-origin").is_none(),
        "非白名单 Origin 不得拿到 ACAO"
    );

    // ── 7) 白名单 Origin：正常反射 ──
    let r = c
        .request(reqwest::Method::OPTIONS, format!("{base}/api/settings/get"))
        .header("origin", "http://tauri.localhost")
        .header("access-control-request-method", "POST")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 204);
    assert_eq!(
        r.headers()
            .get("access-control-allow-origin")
            .and_then(|v| v.to_str().ok()),
        Some("http://tauri.localhost"),
        "白名单 Origin 应反射 ACAO"
    );

    // ── 8) 实际响应同样只对白名单 Origin 发 ACAO（防「简单请求」绕过预检后读取）──
    let r = c
        .get(format!("{base}/thumbnail?type=avatar&file=x.png"))
        .header("origin", "https://evil.example")
        .send()
        .await
        .unwrap();
    assert!(r.status().is_success());
    assert!(
        r.headers().get("access-control-allow-origin").is_none(),
        "实际响应不得向非白名单 Origin 发 ACAO"
    );

    println!("✅ 中继鉴权验证通过：无/错 token 403、静态 GET 豁免、CORS 白名单生效");
}
