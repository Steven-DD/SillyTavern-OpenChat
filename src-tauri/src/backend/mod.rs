//! backend —— Rust 本地 HTTP 后端骨架（Phase 1 · 评审整改 T7/T9 承载体）
//!
//! ── 定位（方案 §4.1 + 决策 D1 方案 a）──
//! 与 relay.rs 的关键区别：这里**不是反向代理**，是数据服务的本体。
//! Strangler 期间与 ST 并存（互斥由 T6 闸门管），native 模式下前端 `st.base`
//! 直接指向本服务。relay 在 native 模式移除，其四件套并入本模块：
//!   1. 鉴权（X-Relay-Auth + 256-bit token，同 relay.rs:76-81 生成方式）
//!   2. CORS 白名单反射（同 relay.rs origin_allowed）
//!   3. 静态 GET 豁免四前缀（同 relay.rs is_exempt_from_auth）
//!   4. SSE 无缓冲透传语义（本服务直接产流，无上游）
//!
//! ── 前端不动（D1）──
//! 前端 client.ts 无条件发 `/csrf-token` + `X-CSRF-Token` + `X-Relay-Auth`
//! （client.ts:41-131）。本服务承接同样形态：提供 `/csrf-token`（固定 token，
//! 不轮换 session——无会话体系），POST 校验 `X-Relay-Auth`；`X-CSRF-Token`
//! 收下不校验（本地单用户无 CSRF 面，token 即是鉴权）。
//!
//! ── 兼容面 ──
//! manifest（docs/agents/endpoint-manifest.md）81 契约点按族分批实现；
//! 未实现端点统一返回 501 + JSON `{error: {message}}`（S20：可判定的
//! 「迁移中」错误，禁止 200+空结果伪造成功）。

mod api;
mod auth;
pub mod assets;
pub mod characters;
pub mod chats;
pub mod extensions;
pub mod extras;
pub mod gate;
pub mod providers;
pub mod providers_convert;
mod providers_text;
pub mod settings;
pub mod tokenizers_secrets;
mod uploads;
pub mod worldinfo_groups;

pub use auth::gen_token;

use axum::{
    body::Body,
    http::{header, HeaderValue, Method, StatusCode},
    response::{IntoResponse, Response},
    Router,
};

/// 共享状态
#[derive(Clone)]
pub struct BackendState {
    /// 启动时生成的鉴权 token（S1：256-bit OS CSPRNG）
    pub token: Arc<String>,
    /// 数据根目录（与 ST 兼容格式同位置；T6 的 data_paths 单一裁决）
    pub data_root: std::path::PathBuf,
    /// 上传字节上限（S15：fileSize 语义；裁定值 32MB —— 与 relay 请求体上限一致，
    /// 覆盖实测最大角色卡；ST multer 的 fieldSize 500MB 是已知过宽面，不照抄）
    pub max_body_bytes: usize,
    /// settings 服务的 autosave 节流状态（Phase 1 族一）
    pub settings_svc: Arc<settings::SettingsService>,
    /// chats 服务的备份节流状态（Phase 1 族四）
    pub chats_svc: Arc<chats::ChatsService>,
    /// data-maid 内存 token 表（族八）
    pub maid_tokens: Arc<extras::MaidTokens>,
}

use std::sync::Arc;

/// backend 控制句柄
pub struct BackendHandle {
    pub port: u16,
    pub token: String,
}

/// 启动 backend（绑定 127.0.0.1 随机端口）
pub async fn start(data_root: std::path::PathBuf) -> std::io::Result<BackendHandle> {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await?;
    let port = listener.local_addr()?.port();
    let token = auth::gen_token();

    let state = BackendState {
        token: Arc::new(token.clone()),
        data_root,
        max_body_bytes: 32 * 1024 * 1024,
        settings_svc: Arc::new(settings::SettingsService::new()),
        chats_svc: Arc::new(chats::ChatsService::new()),
        maid_tokens: Arc::new(extras::MaidTokens::new()),
    };

    let app = router(state);

    tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            eprintln!("[backend] 服务异常退出: {e}");
        }
    });

    println!("[backend] 已启动 127.0.0.1:{port}（鉴权已启用，token 256-bit）");
    Ok(BackendHandle { port, token })
}

/// 路由表：所有请求经统一 fallback（鉴权 + CORS + 分发一体）
fn router(state: BackendState) -> Router {
    Router::new().fallback(api::authorize).with_state(state)
}

/* ------------------------------------------------------------------ *
 * 鉴权 + CORS 中间件（并入 relay.rs 四件套；S2/S3/S4/S5）
 * ------------------------------------------------------------------ */

/// 允许反射 CORS 的 Origin 白名单（与 relay.rs:162-175 字面一致 —— 单一口径，
/// 未来 relay 删除时以这里为准）
fn origin_allowed(o: &HeaderValue) -> bool {
    o.to_str()
        .map(|s| {
            matches!(
                s,
                "tauri://localhost"
                    | "http://tauri.localhost"
                    | "https://tauri.localhost"
                    | "http://127.0.0.1:1420"
                    | "http://localhost:1420"
            )
        })
        .unwrap_or(false)
}

/// 静态资源 GET 豁免（对齐 relay.rs:180-186 语义并收紧为段边界匹配）：
/// `<img>/CSS url()` 无法带自定义头；/api/* 与 /csrf-token 永不免检（S3）。
/// 段边界：`/thumbnailX` 不算 `/thumbnail` 前缀（防 `/thumbnailX/api/x` 式伪装）。
fn is_exempt_from_auth(method: &Method, path: &str) -> bool {
    if !(method == Method::GET || method == Method::HEAD) {
        return false;
    }
    const EXEMPT_PREFIXES: [&str; 4] = ["/thumbnail", "/characters", "/backgrounds", "/user-images"];
    EXEMPT_PREFIXES.iter().any(|p| {
        path == *p || path.starts_with(&format!("{p}/")) || path.starts_with(&format!("{p}?"))
    })
}

/// Builder 版 CORS 注入（构造中响应用；Response 已构造好用 apply_cors_resp）
#[allow(dead_code)] // api.rs 现走 apply_cors_resp；本函数留给 Phase 1 流式 handler
fn apply_cors(b: axum::http::response::Builder, origin: Option<HeaderValue>) -> axum::http::response::Builder {
    match origin {
        Some(o) => b
            .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, o)
            .header(header::ACCESS_CONTROL_ALLOW_CREDENTIALS, "true")
            .header(header::VARY, "Origin"),
        None => b,
    }
}

/// Response 版 CORS 注入（handler 已构造好 Response 时用）
fn apply_cors_resp(mut resp: Response, origin: Option<HeaderValue>) -> Response {
    if let Some(o) = origin {
        let h = resp.headers_mut();
        h.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, o);
        h.insert(header::ACCESS_CONTROL_ALLOW_CREDENTIALS, HeaderValue::from_static("true"));
        h.insert(header::VARY, HeaderValue::from_static("Origin"));
    }
    resp
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
                "content-type, x-csrf-token, x-relay-auth, accept",
            )
            .header(header::ACCESS_CONTROL_MAX_AGE, "600")
            .header(header::VARY, "Origin");
    }
    b.body(Body::empty())
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

/// 403 响应（S2：不带任何数据回显）
fn forbidden() -> Response {
    (StatusCode::FORBIDDEN, "缺少或错误的鉴权 token（X-Relay-Auth）").into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exempt_list_only_four_readonly_prefixes() {
        // S3：豁免集合显式且只含四个只读资产前缀
        let get = Method::GET;
        assert!(is_exempt_from_auth(&get, "/thumbnail?type=avatar&file=x.png"));
        assert!(is_exempt_from_auth(&get, "/characters/a.png"));
        assert!(is_exempt_from_auth(&get, "/backgrounds/b.jpg"));
        assert!(is_exempt_from_auth(&get, "/user-images/c.png"));
        // POST 静态前缀不豁免
        assert!(!is_exempt_from_auth(&Method::POST, "/thumbnail"));
        // /api/* 与 /csrf-token 永不免检
        assert!(!is_exempt_from_auth(&get, "/api/secrets/write"));
        assert!(!is_exempt_from_auth(&get, "/api/settings/get"));
        assert!(!is_exempt_from_auth(&get, "/csrf-token"));
        // 豁免前缀下的 /api 伪装路径也不行（前缀必须精确）
        assert!(!is_exempt_from_auth(&get, "/thumbnailX/api/x"));
    }

    #[test]
    fn origin_whitelist_matches_relay() {
        let ok = [
            "tauri://localhost",
            "http://tauri.localhost",
            "https://tauri.localhost",
            "http://127.0.0.1:1420",
            "http://localhost:1420",
        ];
        for o in ok {
            let v = HeaderValue::from_str(o).unwrap();
            assert!(origin_allowed(&v), "{o} 应放行");
        }
        let evil = HeaderValue::from_static("https://evil.example");
        assert!(!origin_allowed(&evil));
        let other = HeaderValue::from_static("http://127.0.0.1:9999");
        assert!(!origin_allowed(&other));
    }

    #[tokio::test]
    async fn csrf_token_requires_auth() {
        // /csrf-token 不在豁免清单 —— 无 token 必 403（S3）
        let ep = start(std::env::temp_dir().join("stchat-backend-test-csrf"))
            .await
            .expect("启动失败");
        let c = reqwest::Client::builder().no_proxy().build().unwrap();
        let base = format!("http://127.0.0.1:{}", ep.port);

        let r = c.get(format!("{base}/csrf-token")).send().await.unwrap();
        assert_eq!(r.status().as_u16(), 403, "无 token 的 /csrf-token 必须被拒");

        let r = c
            .get(format!("{base}/csrf-token"))
            .header("x-relay-auth", &ep.token)
            .send()
            .await
            .unwrap();
        assert!(r.status().is_success());
        let v: serde_json::Value = r.json().await.unwrap();
        assert!(v["token"].as_str().is_some(), "响应应含 token 字段");
    }

    #[tokio::test]
    async fn unimplemented_endpoints_return_501_json_error() {
        // S20：未迁移端点返回可判定的错误，禁止 200+空结果
        let ep = start(std::env::temp_dir().join("stchat-backend-test-501"))
            .await
            .expect("启动失败");
        let c = reqwest::Client::builder().no_proxy().build().unwrap();
        let base = format!("http://127.0.0.1:{}", ep.port);

        let r = c
            .post(format!("{base}/api/sd/generate"))
            .header("x-relay-auth", &ep.token)
            .header("x-csrf-token", "t")
            .header("content-type", "application/json")
            .body("{}")
            .send()
            .await
            .unwrap();
        assert_eq!(r.status().as_u16(), 501);
        let v: serde_json::Value = r.json().await.unwrap();
        assert!(
            v["error"]["message"].as_str().unwrap().contains("未迁移"),
            "错误体应说明未迁移: {v}"
        );
    }

    #[tokio::test]
    async fn auth_enforced_on_all_non_exempt() {
        let ep = start(std::env::temp_dir().join("stchat-backend-test-auth"))
            .await
            .expect("启动失败");
        let c = reqwest::Client::builder().no_proxy().build().unwrap();
        let base = format!("http://127.0.0.1:{}", ep.port);

        // 无 token → 403；错 token → 403；query token → 403（S5）
        let r = c.post(format!("{base}/api/ping")).send().await.unwrap();
        assert_eq!(r.status().as_u16(), 403);
        let r = c
            .post(format!("{base}/api/ping"))
            .header("x-relay-auth", "deadbeef")
            .send()
            .await
            .unwrap();
        assert_eq!(r.status().as_u16(), 403);
        let r = c
            .post(format!("{base}/api/ping?token={}", ep.token))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status().as_u16(), 403, "query token 不被接受（S5）");

        // 正确 token → 200
        let r = c
            .post(format!("{base}/api/ping"))
            .header("x-relay-auth", &ep.token)
            .send()
            .await
            .unwrap();
        assert!(r.status().is_success());

        // 恶意 Origin：不放 ACAO（S4）
        let r = c
            .post(format!("{base}/api/ping"))
            .header("x-relay-auth", &ep.token)
            .header("origin", "https://evil.example")
            .send()
            .await
            .unwrap();
        assert!(r.status().is_success());
        assert!(r.headers().get("access-control-allow-origin").is_none());
    }

    #[tokio::test]
    async fn settings_family_roundtrip_with_unknown_keys() {
        // Phase 1 族一验收（评审 A3/D3 + manifest #28/#29）：
        // 200+ 未知键读-改-写零丢失；保存语义为整体覆盖；401/403 面不被打开
        let root = std::env::temp_dir().join(format!("stchat-backend-settings-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let ep = start(root.clone()).await.expect("启动失败");
        let c = reqwest::Client::builder().no_proxy().build().unwrap();
        let base = format!("http://127.0.0.1:{}", ep.port);
        let auth = ("x-relay-auth", ep.token.as_str());

        // 无 token 的 settings/get → 403（数据面在鉴权后面）
        let r = c.post(format!("{base}/api/settings/get")).send().await.unwrap();
        assert_eq!(r.status().as_u16(), 403);

        // 构造 200+ 未知键的全量 settings
        let mut body = serde_json::Map::new();
        body.insert("username".into(), serde_json::json!("老板"));
        for i in 0..200 {
            body.insert(format!("unknown_key_{i}"), serde_json::json!({ "i": i }));
        }
        body.insert(
            "extension_settings".into(),
            serde_json::json!({ "stchat_quick_replies": [{ "label": "qr" }] }),
        );

        // 模块 A 写入（QR）
        let r = c
            .post(format!("{base}/api/settings/save"))
            .header(auth.0, auth.1)
            .json(&serde_json::Value::Object(body.clone()))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status().as_u16(), 200);
        let v: serde_json::Value = r.json().await.unwrap();
        assert_eq!(v["result"], "ok");

        // 模块 B 读全量 → 合并新键 → 写全量（模拟 settings-writequeue 的事务）
        let r = c
            .post(format!("{base}/api/settings/get"))
            .header(auth.0, auth.1)
            .send()
            .await
            .unwrap();
        let v: serde_json::Value = r.json().await.unwrap();
        let raw = v["settings"].as_str().unwrap();
        let mut cur: serde_json::Value = serde_json::from_str(raw).unwrap();
        cur["extension_settings"]["regex"] = serde_json::json!([{ "script": "s1" }]);

        let r = c
            .post(format!("{base}/api/settings/save"))
            .header(auth.0, auth.1)
            .json(&cur)
            .send()
            .await
            .unwrap();
        assert_eq!(r.status().as_u16(), 200);

        // 读回：A 的键与 200 未知键全部保留（lost update 消除）
        let r = c
            .post(format!("{base}/api/settings/get"))
            .header(auth.0, auth.1)
            .send()
            .await
            .unwrap();
        let v: serde_json::Value = r.json().await.unwrap();
        let final_settings: serde_json::Value =
            serde_json::from_str(v["settings"].as_str().unwrap()).unwrap();

        assert_eq!(final_settings["username"], "老板");
        assert!(
            final_settings["extension_settings"]["stchat_quick_replies"].is_array(),
            "模块 A 的键保留"
        );
        assert!(
            final_settings["extension_settings"]["regex"].is_array(),
            "模块 B 的键写入"
        );
        let lost: Vec<_> = (0..200)
            .filter(|i| final_settings[format!("unknown_key_{i}")].is_null())
            .collect();
        assert!(lost.is_empty(), "未知键零丢失（丢 {:?}）", lost);

        // 包装对象 → 400（三铁律）
        let r = c
            .post(format!("{base}/api/settings/save"))
            .header(auth.0, auth.1)
            .json(&serde_json::json!({ "settings": "{}" }))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status().as_u16(), 400);

        // 备份产生（save 触发 autosave）
        let backups = root.join("default-user").join("backups");
        let n = std::fs::read_dir(&backups).map(|d| d.count()).unwrap_or(0);
        assert!(n >= 1, "save 应触发备份，实际 {n}");

        let _ = std::fs::remove_dir_all(&root);
    }
}
