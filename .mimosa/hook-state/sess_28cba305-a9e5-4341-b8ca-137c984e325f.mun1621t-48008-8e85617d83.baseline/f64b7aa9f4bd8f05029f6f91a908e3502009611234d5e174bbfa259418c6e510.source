//! auto 兜底探测的端到端验证
//!
//! 场景：用户把 ST 绑定重置为 auto（`plugins.json` 里 `source: "auto", path: null`），
//! 宿主安装布局 `D:\ST Chat\plugins\sillytavern\1.19.0` 真实存在。
//! 期望：probe 走 L4（安装根目录邻近）自动找回，状态 ready。
//!
//! ⚠ 本测试依赖本机真实存在的 `D:\ST Chat`，在无此布局的机器上会跳过。

use std::path::PathBuf;

#[test]
fn auto_probe_finds_host_installed_st_via_install_root() {
    let real_root = PathBuf::from(r"D:\ST Chat\plugins\sillytavern\1.19.0");
    if !real_root.join("server.js").is_file() {
        eprintln!("跳过：本机不存在宿主安装布局 {}", real_root.display());
        return;
    }

    // 临时 config_dir，钉 install_root 到 D:\ST Chat，ST 绑定 auto
    let cfg = std::env::temp_dir().join(format!("stchat-e2e-probe-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&cfg);
    std::fs::create_dir_all(&cfg).unwrap();
    std::fs::write(
        cfg.join("app-settings.json"),
        r#"{"install_root": "D:\\ST Chat"}"#,
    )
    .unwrap();
    std::fs::write(
        cfg.join("plugins.json"),
        r#"{"bindings": {"sillytavern": {"source": "auto", "path": null}}}"#,
    )
    .unwrap();

    let host = st_chat_lib::plugin::PluginHost::new(&cfg);
    let reps = host.reports();
    let st = reps.iter().find(|r| r.id == "sillytavern").unwrap();

    assert_eq!(st.status, "ready", "探测失败：{}", st.message);
    let got = st.path.as_deref().unwrap().replace('/', "\\");
    assert!(
        got.to_lowercase().ends_with(r"st chat\plugins\sillytavern\1.19.0"),
        "路径不符：{got}"
    );
    assert!(
        st.message.contains("安装根目录"),
        "来源标注应为「安装根目录邻近」：{}",
        st.message
    );
    eprintln!("✓ auto 探测命中：{} ({})", got, st.message);

    // 第二次探测应走 auto 缓存快查（plugins.json 已落缓存），结果一致
    let reps2 = host.reports();
    let st2 = reps2.iter().find(|r| r.id == "sillytavern").unwrap();
    assert_eq!(st2.status, "ready", "二次探测失败：{}", st2.message);
    assert!(
        st2.message.contains("缓存命中"),
        "二次探测应命中缓存：{}",
        st2.message
    );
    assert_eq!(st2.path, st.path, "缓存路径应与首次一致");
    eprintln!("✓ 二次探测走缓存：{}", st2.message);

    // 缓存确实落盘（模拟重启后仍生效）
    let state = std::fs::read_to_string(cfg.join("plugins.json")).unwrap();
    assert!(
        state.contains("auto_cache") && state.contains("1.19.0"),
        "plugins.json 应含 auto_cache：{state}"
    );

    let _ = std::fs::remove_dir_all(&cfg);
}
