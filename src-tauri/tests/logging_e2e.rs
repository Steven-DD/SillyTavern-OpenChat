//! 全局：MSVC linker 本地化 stdout 警告抑制（仓库测试文件公约）
#![allow(linker_messages)]
//! 后端日志 e2e —— 生产可诊断性的回归门。
//!
//! 断言"自建后端的全量日志确实进了 `log` crate"（release GUI 无控制台时，
//! 这是唯一可诊断通道）：
//! 1. 启动日志（`stoc::boot`）
//! 2. 成功请求一行 `← 200 … /api/ping`（`stoc::api`）
//! 3. 鉴权拒绝 403 的 warn（`stoc::api`）
//! 4. 未实现端点 501 的 warn
//!
//! 用进程内捕获 logger（不落盘、不出网）—— 与 tauri-plugin-log 无关，
//! 只验证"埋点存在且分级正确"。

use std::sync::{Mutex, OnceLock};

static LINES: OnceLock<Mutex<Vec<String>>> = OnceLock::new();

struct Capture;

impl log::Log for Capture {
    fn enabled(&self, _: &log::Metadata) -> bool {
        true
    }
    fn log(&self, r: &log::Record) {
        if let Ok(mut g) = LINES.get().unwrap().lock() {
            g.push(format!("{}|{}", r.target(), r.args()));
        }
    }
    fn flush(&self) {}
}

fn captured() -> Vec<String> {
    LINES
        .get()
        .map(|m| m.lock().map(|g| g.clone()).unwrap_or_default())
        .unwrap_or_default()
}

#[tokio::test]
async fn backend_request_lifecycle_is_logged() {
    let _ = log::set_boxed_logger(Box::new(Capture));
    log::set_max_level(log::LevelFilter::Info);
    LINES.get_or_init(|| Mutex::new(Vec::new()));

    let _tmp = tempfile::Builder::new()
        .prefix("stoc-logtest-")
        .tempdir()
        .unwrap();
    let root = _tmp.path().to_path_buf();
    let ep = sillytavern_openchat_lib::backend::start(root.clone())
        .await
        .expect("backend 启动失败");
    let c = reqwest::Client::builder().no_proxy().build().unwrap();
    let base = format!("http://127.0.0.1:{}", ep.port);

    // 1) 正常请求 → 200
    let r = c
        .post(format!("{base}/api/ping"))
        .header("x-relay-auth", &ep.token)
        .send()
        .await
        .unwrap();
    assert!(r.status().is_success());

    // 2) 无 token → 403（auth 拒绝分支）
    let r = c.post(format!("{base}/api/ping")).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 403);

    // 3) 未实现端点 → 501（降级契约分支）
    let r = c
        .post(format!("{base}/api/sd/generate"))
        .header("x-relay-auth", &ep.token)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 501);

    let lines = captured();
    let api: Vec<&String> = lines
        .iter()
        .filter(|l| l.starts_with("stoc::api|"))
        .collect();

    assert!(
        lines.iter().any(|l| l.starts_with("stoc::boot|")),
        "缺启动日志（stoc::boot）：{lines:?}"
    );
    assert!(
        api.iter()
            .any(|l| l.contains("← 200") && l.contains("/api/ping")),
        "缺成功请求日志（stoc::api ← 200 /api/ping）：{api:?}"
    );
    assert!(
        api.iter().any(|l| l.contains("403") && l.contains("auth 拒绝")),
        "缺鉴权拒绝日志（stoc::api 403）：{api:?}"
    );
    assert!(
        api.iter().any(|l| l.contains("501") && l.contains("未实现端点")),
        "缺未实现端点日志（stoc::api 501）：{api:?}"
    );
}
