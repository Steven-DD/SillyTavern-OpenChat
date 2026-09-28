//! ST HTTP 中继（生产环境跨源方案 · 《技术选型与项目架构 v1.0》P0 项）
//!
//! ── 为什么需要它 ──
//! 生产环境下前端跑在 Tauri 的自定义协议上（Windows 为 `http://tauri.localhost`），
//! 而 ST 在 `http://127.0.0.1:8000`，两个 origin 不同源：
//!   1. CORS 会拦掉所有 /api 调用；
//!   2. 更麻烦的是 **CSRF**：ST 的会话 Cookie 是 `SameSite=Lax`，
//!      跨站 fetch 根本不会带上它 —— 即使解决了 CORS 也会 403。
//!
//! ── 方案：由 Rust 中继「代持会话」──
//! 中继在本机环回上开一个随机端口，前端把它当作 ST 基地址（`localStorage['st.base']`）。
//! 中继自己维护一个 Cookie 罐：从上游响应里抓 `set-cookie`，转发请求时自己补 `cookie` 头。
//! 于是浏览器完全不参与 Cookie → **没有 SameSite / 第三方 Cookie 的任何坑**，
//! 前端代码（CSRF 取 token + `X-CSRF-Token` 双提交）一行都不用改。
//!
//! ── 鉴权（必须有）──
//! 代持 Cookie 意味着**任何能打到中继的进程/网页都等于拿到了 ST 全权**（含 /api/secrets）。
//! 因此启动时生成一次性随机 token，经 `st_relay_base` 连同基地址一起交给前端，
//! 除「浏览器 <img>/CSS 无法携带自定义头的静态资源 GET」外，所有请求必须带
//! `X-Relay-Auth` 头；CORS 只对白名单 Origin（Tauri webview / Vite 开发服务器）反射，
//! 其余来源拿不到 ACAO —— 双保险封死「本机其它进程」与「恶意网页 drive-by」两条路。
//!
//! ── SSE 必须原样透传 ──
//! 响应体用 `Body::from_stream` 直接转发，绝不缓冲；请求头不带 `accept-encoding`
//! （reqwest 未启用 gzip feature），上游返回的就是明文，可以安全透传。

use axum::{
    body::Body,
    extract::{OriginalUri, State},
    http::{header, HeaderMap, HeaderValue, Method, StatusCode},
    response::{IntoResponse, Response},
    Router,
};
use futures_util::StreamExt;
use futures_util::TryStreamExt;
use std::sync::{Arc, Mutex, RwLock};

#[derive(Clone)]
struct RelayState {
    upstream: Arc<RwLock<String>>,
    client: reqwest::Client,
    /// ST 会话 Cookie 罐（name=value 列表）—— 由中继代持，前端不参与
    jar: Arc<Mutex<Vec<String>>>,
    /// 启动时生成的鉴权 token（前端经 st_relay_base 获取，请求须带 `X-Relay-Auth`）
    token: Arc<String>,
}

/// 中继控制句柄：供壳层在运行中更换上游端口
/// （st_retry_start 重新解析出不同端口 / 用户在插件页改端口 —— P1：此前上游
/// 一次性固化，改端口后中继仍指向旧端口，前端请求全部打空）
#[derive(Clone)]
pub struct RelayHandle {
    upstream: Arc<RwLock<String>>,
}

impl RelayHandle {
    pub fn set_upstream_port(&self, port: u16) {
        if let Ok(mut g) = self.upstream.write() {
            let next = format!("http://127.0.0.1:{port}");
            if *g != next {
                println!("[relay] 上游切换 → {next}");
                *g = next;
            }
        }
    }
}

/// 中继端点信息：随机端口 + 本次会话的鉴权 token + 上游控制句柄
pub struct RelayEndpoint {
    pub port: u16,
    pub token: String,
    pub handle: RelayHandle,
}

/// 生成 256-bit 随机 token（hex）。环形回环上的本地 token，熵源用 OS CSPRNG。
fn gen_token() -> std::io::Result<String> {
    let mut buf = [0u8; 32];
    getrandom::getrandom(&mut buf)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
    Ok(buf.iter().map(|b| format!("{b:02x}")).collect())
}

/// 启动中继，返回实际绑定的端口、鉴权 token 与上游控制句柄。
/// `st_port` 为 SillyTavern 的监听端口。
pub async fn start(st_port: u16) -> std::io::Result<RelayEndpoint> {
    let client = reqwest::Client::builder()
        // 关键：本机有 HTTP_PROXY/HTTPS_PROXY（沙箱或用户的 Clash）。
        // 若不屏蔽，环回请求会被送进代理 → 必然失败。
        .no_proxy()
        .build()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;

    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await?;
    let port = listener.local_addr()?.port();
    let token = gen_token()?;

    let state = RelayState {
        upstream: Arc::new(RwLock::new(format!("http://127.0.0.1:{st_port}"))),
        client,
        jar: Arc::new(Mutex::new(Vec::new())),
        token: Arc::new(token.clone()),
    };
    let handle = RelayHandle {
        upstream: Arc::clone(&state.upstream),
    };

    let app = Router::new().fallback(proxy).with_state(state);

    tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            eprintln!("[relay] 服务异常退出: {e}");
        }
    });

    println!("[relay] 已启动 127.0.0.1:{port} → http://127.0.0.1:{st_port}（鉴权已启用）");
    Ok(RelayEndpoint { port, token, handle })
}

/* ------------------------------------------------------------------ *
 * Cookie 罐
 * ------------------------------------------------------------------ */

/// 从 `set-cookie` 头部合并进罐子（按 cookie 名去重；Max-Age<=0 / 过期则删除）
fn merge_cookies(jar: &mut Vec<String>, set_cookies: &[HeaderValue]) {
    for sc in set_cookies {
        let Ok(raw) = sc.to_str() else { continue };
        let Some(pair) = raw.split(';').next() else { continue };
        let Some((name, _)) = pair.split_once('=') else { continue };
        let name = name.trim();
        if name.is_empty() {
            continue;
        }
        let expired = raw
            .split(';')
            .skip(1)
            .any(|a| {
                let a = a.trim().to_ascii_lowercase();
                a == "max-age=0" || a.starts_with("max-age=-")
            });
        jar.retain(|c| !c.starts_with(&format!("{name}=")));
        if !expired {
            jar.push(pair.trim().to_string());
        }
    }
}

/* ------------------------------------------------------------------ *
 * 代理
 * ------------------------------------------------------------------ */

/// 逐跳头部（hop-by-hop）：不能转发，由各自的 HTTP 栈管理
fn is_hop_by_hop(name: &str) -> bool {
    matches!(
        name,
        "host" | "connection" | "keep-alive" | "transfer-encoding" | "upgrade" | "content-length"
    )
}

/// 允许反射 CORS 的 Origin 白名单：只服务本应用自己的前端
/// （Tauri webview 的三种平台形态 + Vite 开发服务器）。
/// 非白名单来源一律不发 ACAO —— 浏览器会拦掉读取，配合 token 鉴权双保险。
fn origin_allowed(o: &HeaderValue) -> bool {
    o.to_str()
        .map(|s| {
            matches!(
                s,
                "tauri://localhost"         // macOS / iOS 自定义协议
                    | "http://tauri.localhost" // Windows 生产
                    | "https://tauri.localhost" // Android
                    | "http://127.0.0.1:1420" // Vite 开发服务器
                    | "http://localhost:1420"
            )
        })
        .unwrap_or(false)
}

/// 浏览器 `<img>` / CSS `url()` 发起的静态资源 GET 无法携带自定义头，这些路径豁免鉴权
/// （前端实际用到的只有 /thumbnail、/characters、/backgrounds、/user-images 四个只读资产前缀）。
/// /api/* 与 /csrf-token 一律要求 token —— 否则本机任意进程可借代持 Cookie 读取密钥与数据。
fn is_exempt_from_auth(method: &Method, path: &str) -> bool {
    (method == Method::GET || method == Method::HEAD)
        && (path.starts_with("/thumbnail")
            || path.starts_with("/characters")
            || path.starts_with("/backgrounds")
            || path.starts_with("/user-images"))
}

async fn proxy(
    State(st): State<RelayState>,
    method: Method,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    body: Body,
) -> Response {
    // 非白名单 Origin 直接从 CORS 反射中剔除（origin=None → 不发 ACAO）
    let origin = headers.get(header::ORIGIN).cloned().filter(|o| origin_allowed(o));

    if method == Method::OPTIONS {
        return preflight(origin);
    }

    let path_q = uri.path_and_query().map(|p| p.as_str()).unwrap_or("/");
    let target = format!(
        "{}{}",
        st.upstream.read().map(|g| g.clone()).unwrap_or_default(),
        path_q
    );

    // 鉴权：除静态资源 GET 豁免外，一律要求 `X-Relay-Auth` 等于启动时下发的 token。
    // token 是 OS CSPRNG 的 256-bit 值，环回场景下逐字节比较的时序侧信道不可利用。
    let auth_ok = is_exempt_from_auth(&method, path_q)
        || headers.get("x-relay-auth").and_then(|v| v.to_str().ok()) == Some(st.token.as_str());
    if !auth_ok {
        return (
            StatusCode::FORBIDDEN,
            "relay: 缺少或错误的鉴权 token（X-Relay-Auth）",
        )
            .into_response();
    }

    // 组装转发头
    let mut fwd = reqwest::header::HeaderMap::new();
    for (k, v) in headers.iter() {
        let name = k.as_str().to_ascii_lowercase();
        if is_hop_by_hop(&name)
            || name == "origin"
            || name == "referer"
            || name == "cookie"          // 浏览器 Cookie 一律忽略，用中继自己的罐
            || name == "accept-encoding" // 要求上游给明文，便于原样透传
        {
            continue;
        }
        if let (Ok(n), Ok(val)) = (
            reqwest::header::HeaderName::from_bytes(k.as_ref()),
            reqwest::header::HeaderValue::from_bytes(v.as_bytes()),
        ) {
            fwd.insert(n, val);
        }
    }
    // 补上中继代持的会话 Cookie
    if let Ok(jar) = st.jar.lock() {
        if !jar.is_empty() {
            if let Ok(v) = reqwest::header::HeaderValue::from_str(&jar.join("; ")) {
                fwd.insert(reqwest::header::COOKIE, v);
            }
        }
    }

    let body_bytes = match axum::body::to_bytes(body, 32 * 1024 * 1024).await {
        Ok(b) => b,
        Err(e) => {
            return (StatusCode::BAD_REQUEST, format!("relay: 读取请求体失败: {e}")).into_response()
        }
    };

    let sent = st
        .client
        .request(method, &target)
        .headers(fwd)
        .body(body_bytes.to_vec())
        .send()
        .await;

    let up = match sent {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::BAD_GATEWAY,
                format!("relay: 无法连接 SillyTavern（{target}）: {e}"),
            )
                .into_response()
        }
    };

    // 抓取并更新会话 Cookie
    let set_cookies = up.headers().get_all(header::SET_COOKIE);
    if set_cookies.iter().next().is_some() {
        let vals: Vec<HeaderValue> = set_cookies.iter().cloned().collect();
        if let Ok(mut jar) = st.jar.lock() {
            merge_cookies(&mut jar, &vals);
        }
    }

    let status = up.status();
    let up_headers = up.headers().clone();

    let mut out = Response::builder().status(status.as_u16());
    for (k, v) in up_headers.iter() {
        let name = k.as_str().to_ascii_lowercase();
        // 逐跳头不转发；set-cookie 由中继吃掉（不给浏览器，避免 SameSite 问题）
        if is_hop_by_hop(&name) || name == "set-cookie" {
            continue;
        }
        // ST 的安全中间件给所有响应盖 `Cross-Origin-Resource-Policy: same-origin`，
        // 语义是「禁止跨源页面把我嵌入 <img>/<video>」。
        // 但本中继的存在意义就是跨源服务：前端 origin 是 tauri://localhost（打包）
        // 或 vite 1420（dev），永远 ≠ 中继 origin。Chromium 会在**发请求之前**直接拦掉
        // 这类 <img>（curl 不理 CORP 所以全链路测试都是 200 —— 别被它骗了）。
        // → 必须拦下不转发，统一改写为 cross-origin（见下方 insert）。
        if name == "cross-origin-resource-policy" {
            continue;
        }
        out = out.header(k, v);
    }
    out = apply_cors(out, origin);
    out = out.header("cross-origin-resource-policy", "cross-origin");

    // 流式透传（SSE 关键：绝不缓冲）
    let stream = up
        .bytes_stream()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e));

    let mut pinned = Box::pin(stream);

    // 仅对「上游声明为图片」的响应做嗅探校正（见 sniff_image_mime 注释），
    // HTML / JSON / SSE 一律不动，避免无谓的 peek 开销与语义风险。
    let declared_image = up_headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("image/"));

    let mut corrected: Option<&'static str> = None;

    let body = if declared_image {
        match StreamExt::next(&mut pinned).await {
            Some(Ok(head)) => {
                corrected = sniff_image_mime(&head);
                // 首块要还回去 —— 不能因为嗅探丢字节
                let head_again = futures_util::stream::once(async move {
                    Ok::<_, std::io::Error>(head)
                });
                Body::from_stream(head_again.chain(pinned))
            }
            Some(Err(e)) => {
                return (
                    StatusCode::BAD_GATEWAY,
                    format!("relay: 读取 SillyTavern 响应失败: {e}"),
                )
                    .into_response()
            }
            None => Body::empty(),
        }
    } else {
        Body::from_stream(pinned)
    };

    let mut resp = match out.body(body) {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("relay: 构造响应失败: {e}"),
            )
                .into_response()
        }
    };

    // insert 是替换语义（builder 的 header() 才是追加）
    if let Some(mime) = corrected {
        resp.headers_mut()
            .insert(header::CONTENT_TYPE, HeaderValue::from_static(mime));
    }

    resp
}

/* ------------------------------------------------------------------ *
 * 单测
 * ------------------------------------------------------------------ */

#[cfg(test)]
mod tests {
    use super::sniff_image_mime;

    #[test]
    fn sniff_recognizes_real_image_formats() {
        // 本次事故现场：ST 缩略图把 .png 转码成 JPEG 返回，魔数是唯一真相
        assert_eq!(
            sniff_image_mime(&[0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, b'J', b'F', b'I', b'F']),
            Some("image/jpeg")
        );
        assert_eq!(
            sniff_image_mime(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0x00]),
            Some("image/png")
        );
        assert_eq!(sniff_image_mime(b"GIF89a whatever"), Some("image/gif"));
        assert_eq!(sniff_image_mime(b"GIF87a whatever"), Some("image/gif"));
        assert_eq!(
            sniff_image_mime(b"RIFF\x00\x00\x00\x00WEBPVP8 "),
            Some("image/webp")
        );
    }

    #[test]
    fn sniff_returns_none_for_non_images() {
        // 非图片一律不干预 —— 避免误伤 HTML / JSON / SSE / 压缩包
        assert_eq!(sniff_image_mime(b"<html><body>hi</body></html>"), None);
        assert_eq!(sniff_image_mime(b"{\"ok\":true}"), None);
        assert_eq!(sniff_image_mime(&[0x1F, 0x8B, 0x08, 0x00]), None);
        assert_eq!(sniff_image_mime(b""), None);
    }
}

/// 按魔数判断真实图片类型（返回标准 MIME 名），非已知格式返回 None。
///
/// ── 为什么需要它 ──
/// ST 的 `/thumbnail` 会把原图按 `config.yaml` 的 `thumbnails.format` 重新编码
/// （默认为 **jpg**），但响应头里的 `Content-Type` 是按**原文件扩展名**判定的 ——
/// 原图叫 `xxx.png` 时，就会返回「内容是 JPEG，却声明 `image/png`」的响应。
/// 再叠加 ST 自带的 `X-Content-Type-Options: nosniff`，Chromium 会严格按声明解码，
/// 结果是 `<img>` 触发 error → 头像全部降级为占位色块（curl 看着是 200，极易漏诊）。
/// 这里在转发前校正 Content-Type，魔数是唯一可信依据。
fn sniff_image_mime(bytes: &[u8]) -> Option<&'static str> {
    match bytes {
        // JPEG: FF D8 FF
        [0xFF, 0xD8, 0xFF, ..] => Some("image/jpeg"),
        // PNG: 89 50 4E 47 0D 0A 1A 0A
        [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, ..] => Some("image/png"),
        // GIF87a / GIF89a
        [b'G', b'I', b'F', b'8', _, b'a', ..] => Some("image/gif"),
        // WebP: "RIFF" + 4 字节长度 + "WEBP"
        [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'E', b'B', b'P', ..] => Some("image/webp"),
        _ => None,
    }
}

fn apply_cors(
    b: axum::http::response::Builder,
    origin: Option<HeaderValue>,
) -> axum::http::response::Builder {
    match origin {
        Some(o) => b
            .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, o)
            .header(header::ACCESS_CONTROL_ALLOW_CREDENTIALS, "true")
            .header(header::VARY, "Origin"),
        None => b,
    }
}

fn preflight(origin: Option<HeaderValue>) -> Response {
    let mut b = Response::builder().status(StatusCode::NO_CONTENT);
    if let Some(o) = origin {
        b = b
            .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, o)
            .header(header::ACCESS_CONTROL_ALLOW_CREDENTIALS, "true")
            .header(
                header::ACCESS_CONTROL_ALLOW_METHODS,
                "GET, POST, PUT, DELETE, OPTIONS",
            )
            .header(
                header::ACCESS_CONTROL_ALLOW_HEADERS,
                // x-relay-auth：中继鉴权头，实际请求会携带，预检必须放行
                "content-type, x-csrf-token, x-relay-auth, accept",
            )
            .header(header::ACCESS_CONTROL_MAX_AGE, "600")
            .header(header::VARY, "Origin");
    }
    b.body(Body::empty())
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}
