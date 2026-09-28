//! 插件宿主集成测试
//!
//! 验证这条主线：**用户本机已装的东西能被探测到并直接绑定，不需要下载**。
//! 用的是真实环境（本机 node、本机 SillyTavern 目录），不做 mock ——
//! 探测逻辑的价值全在「真的能读到版本、约束真的被判对」。
//!
//! 运行：cargo test --test plugin_host -- --nocapture
//! 注意：Rust 编译/运行在本环境需非沙箱权限。

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use st_chat_lib::plugin::PluginHost;

/// 测试并行执行，因此每次调用都要拿到**独立**目录。
/// （只用进程 id 命名会互相覆盖 —— 曾导致 binding 往返测试偶发失败）
static SEQ: AtomicUsize = AtomicUsize::new(0);

fn temp_config_dir() -> PathBuf {
    let n = SEQ.fetch_add(1, Ordering::SeqCst);
    let d = std::env::temp_dir().join(format!("stchat-plugin-test-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("创建临时配置目录失败");
    d
}

fn repo_st_dir() -> Option<PathBuf> {
    // ⚠ 只找**正式名**：应用探测只会认 `sillytavern`，不会认改名后的备份目录。
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("sillytavern");
    p.join("server.js").is_file().then_some(p)
}

#[test]
fn builtin_manifests_load() {
    let dir = temp_config_dir();
    let host = PluginHost::new(&dir);
    let stack = host.stack();

    let ids: Vec<&str> = stack.plugins.iter().map(|p| p.id.as_str()).collect();
    for want in ["node", "sillytavern", "git"] {
        assert!(ids.contains(&want), "缺少内置清单 {want}，实际 {ids:?}");
    }

    // 必需/可选分组与执行模型
    let by = |id: &str| stack.plugins.iter().find(|p| p.id == id).unwrap();
    assert_eq!(by("node").tier, "required", "node 应为必要插件");
    assert_eq!(by("sillytavern").tier, "required", "sillytavern 应为必要插件");
    assert_eq!(
        by("git").tier,
        "optional",
        "git 只在装扩展时需要，必须是可选插件（否则会阻塞聊天）"
    );
    for p in &stack.plugins {
        assert_eq!(p.kind, "process", "当前内置插件都是进程型：{}", p.id);
    }

    // 能力表
    let caps: Vec<&str> = stack.capabilities.iter().map(|c| c.id.as_str()).collect();
    for want in ["chat", "characters", "extension-install", "theme"] {
        assert!(caps.contains(&want), "缺少能力 {want}，实际 {caps:?}");
    }
    assert!(
        stack.capabilities.iter().find(|c| c.id == "theme").unwrap().ok,
        "资源型能力应始终可用"
    );
}

#[test]
fn probes_local_node_and_sillytavern() {
    let dir = temp_config_dir();
    let host = PluginHost::new(&dir);
    let stack = host.stack();

    let node = stack.plugins.iter().find(|p| p.id == "node").unwrap();
    let st = stack.plugins.iter().find(|p| p.id == "sillytavern").unwrap();

    println!("--- node ---");
    println!("  status  = {}", node.status);
    println!("  source  = {}", node.source);
    println!("  version = {:?}", node.version);
    println!("  path    = {:?}", node.path);
    println!("  message = {}", node.message);
    println!("--- sillytavern ---");
    println!("  status  = {}", st.status);
    println!("  version = {:?}", st.version);
    println!("  path    = {:?}", st.path);
    println!("  deps    = {:?}", st.dependencies);

    // node：本机装了就应该被探测到（PATH 查找）
    assert!(node.path.is_some(), "未探测到本机 node：{}", node.message);
    assert!(node.version.is_some(), "未读到 node 版本：{}", node.message);
    assert_eq!(node.source, "auto", "默认应为自动探测");
    assert_eq!(node.status, "ready", "node 应就绪：{}", node.message);

    // SillyTavern：在开发机上应能通过兜底候选找到
    match repo_st_dir() {
        Some(expected) => {
            assert_eq!(st.status, "ready", "ST 未就绪：{}", st.message);
            let got = PathBuf::from(st.path.clone().expect("ST 路径为空"));
            assert_eq!(
                std::fs::canonicalize(&got).unwrap(),
                std::fs::canonicalize(&expected).unwrap(),
                "ST 路径不符"
            );
            assert!(st.version.is_some(), "未读到 ST 版本（package.json:version）");
            // 依赖校验：ST requires node，必须判定通过
            let dep = st
                .dependencies
                .iter()
                .find(|d| d.id == "node")
                .expect("ST 应声明对 node 的依赖");
            assert!(dep.ok, "依赖校验未通过：{}", dep.message);
            println!(
                "OK: detected local node {} and SillyTavern {}",
                node.version.as_deref().unwrap_or("?"),
                st.version.as_deref().unwrap_or("?")
            );
        }
        None => {
            println!("⚠️ 未找到开发目录下的 sillytavern，跳过 ST 断言（仅验证 node 探测）");
        }
    }
}

#[test]
fn binding_roundtrip_and_validation() {
    let dir = temp_config_dir();
    let host = PluginHost::new(&dir);

    // 非法插件 id
    assert!(host.set_binding("nope", "auto", None).is_err());
    // manual 必须给路径
    assert!(host.set_binding("node", "manual", None).is_err());
    // 不存在的路径必须被拒（避免留下一个坏绑定）
    assert!(host
        .set_binding("node", "manual", Some("Z:\\definitely\\not\\here".into()))
        .is_err());
    // managed 绑定必须带路径（不带路径要明确报错，而不是假装成功）
    assert!(
        host.set_binding("node", "managed", None).is_err(),
        "managed 绑定缺少路径时必须明确拒绝"
    );
    // managed 绑定不存在的路径也必须被拒
    assert!(host
        .set_binding("node", "managed", Some("Z:\\definitely\\not\\here".into()))
        .is_err());

    // 正常指定一个真实存在的路径 → 成功且落盘为 manual
    // 用临时目录自建一个最小 ST 夹具，测试不依赖开发机的 sillytavern 目录
    let fixture = temp_config_dir();
    std::fs::write(fixture.join("server.js"), "// fixture").unwrap();
    std::fs::write(
        fixture.join("package.json"),
        r#"{"name":"st-fixture","version":"9.9.9"}"#,
    )
    .unwrap();
    let st = fixture;
    host.set_binding("sillytavern", "manual", Some(st.to_string_lossy().into_owned()))
        .expect("指定真实路径应成功");

    let state_file = dir.join("plugins.json");
    assert!(state_file.is_file(), "状态未落盘: {}", state_file.display());
    let text = std::fs::read_to_string(&state_file).unwrap();
    assert!(text.contains("manual"), "落盘内容缺少 manual 绑定: {text}");

    let st_rep = host
        .stack()
        .plugins
        .into_iter()
        .find(|p| p.id == "sillytavern")
        .unwrap();
    assert_eq!(st_rep.source, "manual");
    assert_eq!(st_rep.status, "ready");

    // 状态可被重新读入（持久化闭环）
    let host2 = PluginHost::new(&dir);
    let st2 = host2
        .stack()
        .plugins
        .into_iter()
        .find(|p| p.id == "sillytavern")
        .unwrap();
    assert_eq!(st2.source, "manual", "重新载入后绑定应保持");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn disabling_required_plugin_is_rejected() {
    let dir = temp_config_dir();
    let host = PluginHost::new(&dir);

    // node 是系统插件（tier=required）：不可停用，任何入口调 set_enabled(false) 都要被拒
    let before = host.stack();
    let node_before = before.plugins.iter().find(|p| p.id == "node").unwrap();
    assert_eq!(node_before.tier, "required", "前置条件：node 应为 required");

    let err = host
        .set_enabled("node", false)
        .expect_err("系统插件停用应被拒绝");
    assert!(err.contains("不可停用"), "报错应说明原因：{err}");

    // 状态不得被改动
    let after = host.stack();
    let node_after = after.plugins.iter().find(|p| p.id == "node").unwrap();
    assert_ne!(node_after.status, "disabled", "系统插件不得进入 disabled");
    assert!(
        !node_after.required_by.is_empty(),
        "required_by（影响预告）信息仍应保留"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
