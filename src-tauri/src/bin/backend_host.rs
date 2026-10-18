//! 见 lib.rs 同名注释
#![allow(linker_messages)]
//! backend-host —— native 冒烟/自动化用的**独立后端进程**（无 Tauri 窗口）。
//!
//! browser automation（smoke-browser.mjs）需要把页面 st.base 指向真 native
//! 后端 —— 本 bin 复用 `backend::start`，把 (PORT, TOKEN) 打到 stdout 一行
//! `META BACKEND port=<p> token=<t>` 供宿主解析后注入浏览器 localStorage。
//!
//! 用法：backend-host.exe <data_root>
//!   data_root 目录如不存在会按需创建（family handler 自建子目录）。

fn main() {
    let data_root = std::env::args()
        .nth(1)
        .unwrap_or_else(|| std::env::temp_dir().join("stchat-smoke-root").display().to_string());
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    let handle = rt
        .block_on(st_chat_lib::backend::start(std::path::PathBuf::from(&data_root)))
        .expect("backend 启动失败");
    println!("[backend] data_root = {}", data_root);
    println!("META BACKEND port={} token={}", handle.port, handle.token);
    println!("[backend-host] 运行中（Ctrl-C 退出）");
    // park：server 在 tokio spawn 里跑着
    std::thread::park();
}

