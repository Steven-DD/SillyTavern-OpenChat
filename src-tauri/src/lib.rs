//! ST Chat · Tauri 应用入口
//!
//! 启动顺序（顺序有依赖，勿随意调换）：
//!   0. `PluginHost::new` —— 载入插件清单、迁移旧配置、准备绑定与探测
//!   1. `PluginHost::resolve` → `st_sidecar::spawn_sillytavern`
//!      —— 由宿主解析出 ST 目录 / node / 端口，再托管 ST 子进程
//!   2. `relay::start(st_port)` —— 本机环回反向代理，解决生产环境跨源 + CSRF Cookie
//!   3. 把中继地址与鉴权 token 通过 `st_relay_base` 命令交给前端
//!      （前端写入 `localStorage['st.base']` / `['st.relayToken']`）
//!   4. 退出时 `stop_sillytavern` 清理自己拉起的 ST（外部实例不动）
//!
//! 设计约束：「路径解析」只发生在插件宿主里，sidecar 只消费结果 —— 单一事实来源。

pub mod app_settings;
pub mod downloader;
pub mod installer;
pub mod plugin;
pub mod relay;
pub mod st_data;
mod st_sidecar;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::{Emitter, Manager};

/// 中继端点（端口 + 鉴权 token）：setup 时随中继启动生成，st_relay_base 下发给前端
pub struct RelayAuth {
    pub port: u16,
    pub token: String,
}

/* ================================================================== *
 * 诊断模式
 * ================================================================== */

/// `st-chat.exe --dump-plugins <configDir>`
///
/// 把**插件页渲染所用的同一份 StackReport** 写到 `<configDir>/plugins-report.json`。
/// 用途：① 无 GUI 环境下验证宿主输出 ② 用户报错时一键取证（让用户把这个文件发来即可）
///
/// 为什么落盘而不是打印：release 版是 GUI 子系统程序（无控制台），
/// 直接 println! 在没有重定向时会写到无效句柄。
fn dump_plugins_report(config_dir: &Path) -> i32 {
    let _ = std::fs::create_dir_all(config_dir);
    let host = plugin::PluginHost::new(config_dir);
    let stack = host.stack();
    let out = config_dir.join("plugins-report.json");
    match serde_json::to_string_pretty(&stack) {
        Ok(json) => match std::fs::write(&out, json) {
            Ok(_) => {
                let _ = writeln!(std::io::stdout(), "written: {}", out.display());
                0
            }
            Err(e) => {
                let _ = writeln!(std::io::stderr(), "write failed: {e}");
                1
            }
        },
        Err(e) => {
            let _ = writeln!(std::io::stderr(), "serialize failed: {e}");
            1
        }
    }
}

/* ================================================================== *
 * 桌面壳命令
 * ================================================================== */

/// 中继端点信息（base + 鉴权 token），前端一并落 localStorage
#[derive(serde::Serialize)]
pub struct RelayEndpointInfo {
    pub base: String,
    pub token: String,
}

/// 前端启动时调用：拿到中继基地址与鉴权 token。
/// 中继代持着 ST 会话 Cookie（含 /api/secrets），token 是唯一的准入凭证 ——
/// 前端必须对每个请求携带 `X-Relay-Auth: <token>`（client.ts 统一注入）。
#[tauri::command]
fn st_relay_base(state: tauri::State<'_, RelayAuth>) -> RelayEndpointInfo {
    RelayEndpointInfo {
        base: format!("http://127.0.0.1:{}", state.port),
        token: state.token.clone(),
    }
}

/// 查询 SillyTavern 托管状态（未托管时返回 None）
#[tauri::command]
fn st_status(app: tauri::AppHandle) -> Option<st_sidecar::StatusDto> {
    app.try_state::<st_sidecar::SidecarState>().map(|s| s.report())
}

/// 前端启动屏的「重试」：ST 启动失败（超时/报错）后重新拉起。
/// 幂等：已就绪或正在启动时直接返回当前状态，不会重复拉进程。
#[tauri::command]
fn st_retry_start(
    app: tauri::AppHandle,
    host: tauri::State<'_, Arc<plugin::PluginHost>>,
) -> Result<Option<st_sidecar::StatusDto>, String> {
    if let Some(s) = app.try_state::<st_sidecar::SidecarState>() {
        let r = s.report();
        if r.state == "starting" || r.state == "ready" {
            return Ok(Some(r));
        }
    }
    let res = host.resolve().map_err(|e| format!("插件解析失败: {e}"))?;
    st_sidecar::ensure_running(&app, &res, &host.config_dir)?;
    Ok(app.try_state::<st_sidecar::SidecarState>().map(|s| s.report()))
}

/* ================================================================== *
 * 插件命令
 * ================================================================== */

type Host<'a> = tauri::State<'a, Arc<plugin::PluginHost>>;

/// 校验组件版本号 slug：version 会直接拼进安装/删除路径（version_dir → remove_dir_all），
/// 不校验的话前端传 `..\..` 可路径穿越到安装根之外任意目录删除/写入（P0）。
/// 白名单字符集顺带覆盖 `.`/`..`/分隔符/盘符/UNC（均含非法字符或以点开头被拒）。
fn ensure_version_slug(version: &str) -> Result<(), String> {
    let ok = !version.is_empty()
        && version.len() <= 100
        && !version.starts_with('.')
        && version
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    ok.then_some(())
        .ok_or_else(|| format!("非法的版本号：{version:?}"))
}

#[cfg(test)]
mod version_slug_tests {
    use super::ensure_version_slug;

    #[test]
    fn accepts_normal_version_strings() {
        for v in ["1.19.0", "v22.22.2", "2.0.0-rc.1", "latest", "1_2-x"] {
            assert!(ensure_version_slug(v).is_ok(), "{v} 应当被接受");
        }
    }

    #[test]
    fn rejects_traversal_and_specials() {
        for v in [
            "..", ".", "../..", "..\\..", "a/b", "a\\b", "C:", "", ".hidden", "a b", "a;b",
            "%2e%2e", "1.19.0\n",
        ] {
            assert!(ensure_version_slug(v).is_err(), "{v:?} 必须被拒绝");
        }
    }
}

/// 插件 + 能力的完整快照（插件页数据源）
#[tauri::command]
fn plugins_stack(host: Host<'_>) -> plugin::StackReport {
    host.stack()
}

/// 强制重新探测（探测本身每次实时执行，此命令供 UI 的「重新检测」按钮显式触发）
#[tauri::command]
fn plugins_rescan(host: Host<'_>) -> plugin::StackReport {
    host.rescan()
}

/// 切换插件绑定：auto（自动探测）/ manual（指定路径）/ managed（宿主安装，暂未提供）
#[tauri::command]
fn plugin_set_binding(
    host: Host<'_>,
    id: String,
    source: String,
    path: Option<String>,
) -> Result<plugin::StackReport, String> {
    host.set_binding(&id, &source, path)?;
    Ok(host.stack())
}

/// 启用 / 停用插件。必要插件也允许停用，但前端需依据 `required_by` 先给出影响预告
#[tauri::command]
fn plugin_set_enabled(
    host: Host<'_>,
    id: String,
    enabled: bool,
) -> Result<plugin::StackReport, String> {
    host.set_enabled(&id, enabled)?;
    Ok(host.stack())
}

/// 覆盖 ST 端口（None 表示回到清单默认值）
#[tauri::command]
fn plugin_set_port(host: Host<'_>, port: Option<u16>) -> Result<plugin::StackReport, String> {
    host.set_port(port)?;
    Ok(host.stack())
}

/* ================================================================== *
 * 组件安装命令（S4c：宿主安装 managed）
 * ================================================================== */

/// 全局安装锁：同一时间只允许一个组件在装（下载/解压/npm 都是重活，
/// 并发装两个既抢 IO 又会把进度事件搅在一起）
static INSTALL_IN_FLIGHT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// 最近一次安装进度（全局保存，不随前端视图切换丢失 ——
/// 切走再切回来，进度条要能从「最后已知位置」恢复显示）
static INSTALL_LAST: std::sync::Mutex<Option<installer::InstallProgress>> =
    std::sync::Mutex::new(None);

fn set_last_progress(p: installer::InstallProgress) {
    if let Ok(mut g) = INSTALL_LAST.lock() {
        *g = Some(p);
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct InstallStateDto {
    pub in_flight: bool,
    pub progress: Option<installer::InstallProgress>,
}

/// 查询全局安装状态（前端视图重挂载时拉一次，恢复进度条显示）
#[tauri::command]
fn component_install_state() -> InstallStateDto {
    InstallStateDto {
        in_flight: INSTALL_IN_FLIGHT.load(std::sync::atomic::Ordering::SeqCst),
        progress: INSTALL_LAST.lock().ok().and_then(|g| g.clone()),
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ManagedVersionInfo {
    pub version: String,
    /// required 文件齐全（可作为绑定候选）
    pub complete: bool,
    pub files: u64,
    pub bytes: u64,
    /// 是否是当前绑定的版本
    pub bound: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ComponentVersions {
    pub component: String,
    /// 清单里声明的推荐安装版本（UI 的输入框默认值）
    pub default_version: Option<String>,
    pub install_root: String,
    /// 清单是否启用了预打包源（前端用来标注「安装方式」）
    pub prebuilt_enabled: bool,
    /// 推荐版本是否已经装好（前端用来把按钮切成「已安装」）
    pub default_installed: bool,
    pub versions: Vec<ManagedVersionInfo>,
}

/// manifest 的 required 清单（安装产物校验与版本完整性共用这一份）
fn required_files_of(m: &plugin::Manifest) -> Vec<String> {
    m.install
        .as_ref()
        .and_then(|i| i.get("required"))
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

/// node 组件的绑定路径是版本目录里的可执行文件；ST 是目录本身。
/// 探测链路对两者一视同仁，但 sidecar 需要的可执行路径只有前者需要拼。
fn managed_binding_path(root: &Path, component: &str, version: &str) -> PathBuf {
    let dir = installer::version_dir(root, component, version);
    if component == "node" {
        let exe = dir.join("node.exe");
        if exe.is_file() {
            return exe;
        }
        return dir.join("node");
    }
    dir
}

fn same_path(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => a == b,
    }
}

/// 列出某组件已安装的版本（宿主安装区的数据源）
#[tauri::command]
fn component_versions(host: Host<'_>, id: String) -> Result<ComponentVersions, String> {
    let m = host
        .manifest(&id)
        .cloned()
        .ok_or_else(|| format!("未知组件：{id}"))?;
    let install = m.install.as_ref().ok_or_else(|| "该组件不支持安装".to_string())?;

    let default_version = install
        .get("default_version")
        .and_then(|v| v.as_str())
        .map(String::from);
    let prebuilt_enabled = install
        .get("prebuilt")
        .and_then(|p| p.get("enabled"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let root = installer::install_root(&host.config_dir);
    let comp_dir = installer::component_root(&root, &id);
    let bound_path = host
        .binding_path_of(&id)
        .map(PathBuf::from);

    let mut versions = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&comp_dir) {
        for e in rd.flatten() {
            if !e.path().is_dir() {
                continue; // 压缩包/临时文件不算版本
            }
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('_') || name.starts_with('.') {
                continue; // _cache 之类的内部目录
            }
            let (files, bytes) = installer::dir_stat(&e.path());
            let complete = required_files_of(&m)
                .iter()
                .all(|f| e.path().join(f).exists());
            let bound = bound_path
                .as_ref()
                .map(|bp| same_path(bp, &managed_binding_path(&root, &id, &name)))
                .unwrap_or(false);
            versions.push(ManagedVersionInfo {
                version: name,
                complete,
                files,
                bytes,
                bound,
            });
        }
    }
    versions.sort_by(|a, b| b.version.cmp(&a.version));

    let default_installed = default_version
        .as_ref()
        .map(|v| versions.iter().any(|x| &x.version == v && x.complete))
        .unwrap_or(false);

    Ok(ComponentVersions {
        component: id,
        default_version,
        install_root: st_data::display_path(&root),
        prebuilt_enabled,
        default_installed,
        versions,
    })
}

/// 安装组件（后台执行，进度走 `component-install` 事件）。
///
/// **装完不切绑定** —— 由用户在插件页点「使用此版本」，保证「装失败不影响现状」。
#[tauri::command]
async fn component_install(
    app: tauri::AppHandle,
    host: Host<'_>,
    id: String,
    version: String,
) -> Result<installer::InstallOutcome, String> {
    ensure_version_slug(&version)?;
    if INSTALL_IN_FLIGHT.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return Err("已有安装任务在进行中，请等它完成".into());
    }
    // 新任务开始：清掉上一轮的残留进度，避免 UI 显示旧进度条
    if let Ok(mut g) = INSTALL_LAST.lock() {
        *g = None;
    }
    // 无论成功失败，函数返回前都要释放锁
    let result = do_install(app, host, &id, &version).await;
    INSTALL_IN_FLIGHT.store(false, std::sync::atomic::Ordering::SeqCst);
    // 收尾事件：无论成败都推进度（phase=done 或 error），前端据此恢复按钮
    if let Ok(mut g) = INSTALL_LAST.lock() {
        if let Some(p) = g.as_mut() {
            if result.is_err() {
                p.phase = "error".into();
                p.message = result.as_ref().err().unwrap().clone();
            }
        }
    }
    result
}

async fn do_install(
    app: tauri::AppHandle,
    host: Host<'_>,
    id: &str,
    version: &str,
) -> Result<installer::InstallOutcome, String> {
    let m = host
        .manifest(id)
        .cloned()
        .ok_or_else(|| format!("未知组件：{id}"))?;

    let platform = installer::platform_key();
    let spec = installer::choose_spec(&m, version, &platform)?;

    // ST 走 npm 路径时需要 node；预打包不需要
    let node_exe = if spec.npm_install {
        let p = host
            .reports()
            .iter()
            .find(|r| r.id == "node")
            .and_then(|r| r.path.clone())
            .map(PathBuf::from);
        if p.is_none() {
            return Err("安装 SillyTavern 需要 node 运行时，请先安装 node（预打包版本则无需）".into());
        }
        p
    } else {
        None
    };

    let config_dir = host.config_dir.clone();
    let proxy = app_settings::effective_proxy(&config_dir);
    let settings = app_settings::load(&config_dir);
    let npm_proxy = if settings.proxy_for_npm {
        proxy.clone()
    } else {
        None
    };

    let client = downloader::build_client(proxy.as_deref())?;
    // ⚠ 大资产装到 LOCALAPPDATA（用户可在设置里改 install_root，见 installer::install_root）
    let root = installer::install_root(&config_dir);
    let cache = installer::component_root(&root, "_cache");

    // 进度节流：下载事件可能很密，150ms 一次足够流畅又不打爆 IPC
    let mut last_emit = std::time::Instant::now() - std::time::Duration::from_secs(1);
    let mut last_phase = String::new();
    let outcome = installer::install(
        &client,
        &root,
        node_exe.as_deref(),
        &spec,
        &cache,
        npm_proxy.as_deref(),
        |p| {
            set_last_progress(p.clone());
            let phase_switch = p.phase != last_phase;
            if phase_switch {
                last_phase = p.phase.clone();
                last_emit = std::time::Instant::now();
            }
            if phase_switch || last_emit.elapsed().as_millis() >= 150 || p.phase == "done" {
                last_emit = std::time::Instant::now();
                let _ = app.emit("component-install", &p);
            }
        },
    )
    .await?;

    // 装完顺手重新探测一次，让插件页状态即时反映（但不切绑定）
    let _ = host.rescan();
    Ok(outcome)
}

/// 绑定到宿主安装的某个版本（managed）。
/// 路径由 Rust 侧裁决（node → 版本目录里的 node.exe；ST → 版本目录本身），
/// 前端只传版本号 —— 避免前端拼路径产生混合斜杠。
#[tauri::command]
fn component_use(host: Host<'_>, id: String, version: String) -> Result<plugin::StackReport, String> {
    ensure_version_slug(&version)?;
    // 只允许绑定"完整"的版本：半成品可能是上次安装中断的残留
    let root = installer::install_root(&host.config_dir);
    let target = installer::version_dir(&root, &id, &version);
    if !target.is_dir() {
        return Err(format!("版本 {version} 尚未安装"));
    }
    let manifest = host
        .manifest(&id)
        .cloned()
        .ok_or_else(|| format!("未知组件：{id}"))?;
    let required = required_files_of(&manifest);
    let incomplete: Vec<&String> = required
        .iter()
        .filter(|f| !target.join(f.as_str()).exists())
        .collect();
    if !incomplete.is_empty() {
        return Err(format!(
            "版本 {version} 不完整（缺少 {}），不能绑定；请删除后重新安装",
            incomplete.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
        ));
    }

    let bind_path = managed_binding_path(&root, &id, &version);
    if !bind_path.exists() {
        return Err(format!(
            "绑定目标不存在：{}",
            st_data::display_path(&bind_path)
        ));
    }
    host.set_binding(&id, "managed", Some(bind_path.to_string_lossy().into_owned()))?;
    Ok(host.stack())
}

/// 删除一个已安装的版本。**绑定的版本与正在安装的组件不可删**。
#[tauri::command]
fn component_remove(host: Host<'_>, id: String, version: String) -> Result<ComponentVersions, String> {
    ensure_version_slug(&version)?;
    if INSTALL_IN_FLIGHT.load(std::sync::atomic::Ordering::SeqCst) {
        return Err("有安装任务在进行中，稍后再试".into());
    }
    let m = host
        .manifest(&id)
        .cloned()
        .ok_or_else(|| format!("未知组件：{id}"))?;

    let root = installer::install_root(&host.config_dir);
    let target = installer::version_dir(&root, &id, &version);
    if !target.is_dir() {
        return Err(format!("版本目录不存在：{}", st_data::display_path(&target)));
    }

    // 绑定中的版本不可删 —— 删了应用就跑不起来，这不是"清理"，是"弄坏"
    let bound_path = host.binding_path_of(&id).map(PathBuf::from);
    if let Some(bp) = bound_path {
        if same_path(&bp, &managed_binding_path(&root, &id, &version)) {
            return Err(format!(
                "版本 {version} 正在被使用，请先切换到其它版本（或改回自动探测）再删除"
            ));
        }
    }

    // required 校验只拦「完整版本」—— 半成品目录（可能上次装挂了）允许直接删
    let complete = required_files_of(&m)
        .iter()
        .all(|f| target.join(f).exists());
    if complete {
        // 完整版本删除前留一次确认责任给前端（前端已有二次确认），这里只负责不可逆提示
    }
    std::fs::remove_dir_all(&target)
        .map_err(|e| format!("删除失败：{e}"))?;

    let _ = host.rescan();
    component_versions(host, id)
}

/* ================================================================== *
 * 数据区命令（S2）
 * ================================================================== */

/// 数据区现状：当前位置、体积、关键数据清单（角色卡/会话/密钥…）
#[tauri::command]
fn st_data_status(host: Host<'_>) -> Result<st_data::DataStatus, String> {
    host.data_status()
}

/// 迁移计划（**纯只读预览**，不动任何文件）
#[tauri::command]
fn st_data_plan(host: Host<'_>) -> Result<st_data::MigrationPlan, String> {
    host.data_plan()
}

/// 为数据迁移暂停 SillyTavern（仅当由本应用托管）。
/// 外部实例会**拒绝** —— 不越权杀用户自己起的进程。
#[tauri::command]
fn st_pause_for_migration(app: tauri::AppHandle) -> Result<String, String> {
    match st_sidecar::prepare_for_migration(&app)? {
        Some(msg) => Ok(msg),
        None => Ok("SillyTavern 当前未运行，无需暂停".into()),
    }
}

/* ================================================================== *
 * 应用设置命令（代理 / 安装目录）
 * ================================================================== */

#[tauri::command]
fn get_app_settings(app: tauri::AppHandle) -> Result<app_settings::AppSettings, String> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| format!("获取配置目录失败：{e}"))?;
    app_settings::ensure_exists(&dir);
    Ok(app_settings::load(&dir))
}

#[tauri::command]
fn set_app_settings(
    app: tauri::AppHandle,
    settings: app_settings::AppSettings,
) -> Result<app_settings::AppSettings, String> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| format!("获取配置目录失败：{e}"))?;
    app_settings::save(&dir, &settings)?;
    Ok(app_settings::load(&dir))
}

/// 执行迁移。复制 → 逐文件校验 → 通过后才切换绑定；**源目录永远保留**。
/// 复制可能耗时（大数据目录），放阻塞线程池并推 `st-data-migrate` 进度事件。
#[tauri::command]
async fn st_data_migrate(
    app: tauri::AppHandle,
    host: tauri::State<'_, Arc<plugin::PluginHost>>,
) -> Result<st_data::MigrationResult, String> {
    let h = host.inner().clone();
    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        // 兜底：即使前端漏调 pause 命令，也绝不在 ST 运行中迁移
        st_sidecar::prepare_for_migration(&app2)?;

        // 进度节流：逐个文件发事件会把 IPC 打爆，限制到 ~150ms 一次
        let mut last = std::time::Instant::now();
        h.data_migrate(&mut |ev| {
            let should = ev.phase != "copying" || last.elapsed().as_millis() >= 150;
            if should {
                last = std::time::Instant::now();
                let _ = app2.emit("st-data-migrate", &ev);
            }
        })
    })
    .await
    .map_err(|e| format!("迁移任务异常终止：{e}"))?
}

/// 回滚到 ST 目录内的默认位置。
/// 因为迁移从不删源数据，回滚只是改绑定 —— 零数据风险。
#[tauri::command]
fn st_data_rollback(host: Host<'_>) -> Result<st_data::DataStatus, String> {
    host.data_rollback()?;
    host.data_status()
}

/// 删除外移数据副本（仅当当前数据不在外移目录时允许）。
/// 场景：回滚到 ST 目录后，外移副本残留在应用数据目录，提供手动清理入口。
#[tauri::command]
async fn st_data_remove_external(host: Host<'_>) -> Result<st_data::DataStatus, String> {
    let status = host.data_status()?;
    if status.external {
        return Err("当前数据正在使用外移目录，不能删除".into());
    }
    if !status.external_data_exists {
        return Err("外移副本不存在".into());
    }
    let root = st_data::external_root(&host.config_dir);
    let cfg = st_data::external_config_path(&host.config_dir);
    tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
        std::fs::remove_dir_all(&root).map_err(|e| format!("删除数据副本失败：{e}"))?;
        if cfg.is_file() {
            std::fs::remove_file(&cfg).map_err(|e| format!("删除配置副本失败：{e}"))?;
        }
        Ok(())
    })
    .await
    .map_err(|e| format!("任务异常：{e}"))??;
    host.data_status()
}

/// 导出/导入共用的现场检查：返回（数据目录, config 路径, ST 是否在跑）。
/// resolve 失败（未绑定 ST）不算错误 —— 数据位置是独立绑定的，导出导入照常可用。
fn data_bag_context(host: &plugin::PluginHost) -> Result<(PathBuf, PathBuf, bool), String> {
    match host.resolve() {
        Ok(r) => {
            let running = st_data::is_st_running(r.st_port);
            if running {
                return Err("SillyTavern 正在运行，请先关闭应用再执行导入/导出".into());
            }
            let (root, cfg) = host.data_paths(&r.st_dir);
            Ok((root, cfg, false))
        }
        // ST 未就绪：数据位置仍是自动绑定值，可安全导入导出
        Err(_) => {
            let root = st_data::external_root(&host.config_dir);
            let cfg = st_data::external_config_path(&host.config_dir);
            Ok((root, cfg, false))
        }
    }
}

/// 导出数据袋（data 目录 + config.yaml）到指定 zip 路径。
/// 路径由前端用系统对话框选好再传进来。
#[tauri::command]
async fn st_data_export(host: Host<'_>, dest: String) -> Result<st_data::DataBagResult, String> {
    let (root, cfg, _) = data_bag_context(&host)?;
    let dest = PathBuf::from(dest.trim());
    if dest.extension().map(|e| e != "zip").unwrap_or(true) {
        return Err("导出目标必须是 .zip 文件".into());
    }
    let cfg_ref = cfg.is_file().then_some(cfg);
    tauri::async_runtime::spawn_blocking(move || st_data::export_bag(&root, cfg_ref.as_deref(), &dest))
        .await
        .map_err(|e| format!("导出任务异常：{e}"))?
}

/* ================================================================== *
 * 会话 jsonl 行级增量操作（S5 消息编辑/删除/swipe）
 * ================================================================== */

/// 单行操作：index 为 jsonl 行号（0 = 元数据头行）
#[derive(serde::Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum ChatLineOp {
    /// 整行替换（消息编辑 / swipe 切换）
    Replace { index: usize, json: String },
    /// 删除该行
    Delete { index: usize },
    /// 插入到 index 之前（index = lines.len() 时为追加）
    Insert { index: usize, json: String },
}

/// 解析会话 jsonl 路径：<data_root>/default-user/chats/<avatar>/<file>.jsonl
fn chat_jsonl_path(host: &plugin::PluginHost, avatar: &str, file: &str) -> Result<PathBuf, String> {
    let safe = |s: &str| {
        !s.is_empty()
            && !s.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|', '\0'])
            && s != "."
            && s != ".."
    };
    if !safe(avatar) || !safe(file) {
        return Err("非法的会话路径参数".into());
    }
    let r = host.resolve().map_err(|e| format!("组件未就绪：{e}"))?;
    let (root, _) = host.data_paths(&r.st_dir);
    let name = if file.ends_with(".jsonl") {
        file.to_string()
    } else {
        format!("{file}.jsonl")
    };
    Ok(root
        .join("default-user")
        .join("chats")
        .join(avatar)
        .join(name))
}

/// 解析角色卡文件路径：<data_root>/default-user/characters/<avatar>（导出 PNG 用）
fn character_png_path(host: &plugin::PluginHost, avatar: &str) -> Result<PathBuf, String> {
    let safe = |s: &str| {
        !s.is_empty()
            && !s.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|', '\0'])
            && s != "."
            && s != ".."
    };
    if !safe(avatar) {
        return Err("非法的角色卡路径参数".into());
    }
    let r = host.resolve().map_err(|e| format!("组件未就绪：{e}"))?;
    let (root, _) = host.data_paths(&r.st_dir);
    Ok(root.join("default-user").join("characters").join(avatar))
}

/// 导出角色卡原文件（PNG/JSON/CHARX，保存对话框选位置）；字节级复制，不做任何转换
#[tauri::command]
async fn character_export_png(
    host: Host<'_>,
    avatar: String,
    dest: String,
) -> Result<(), String> {
    let src = character_png_path(&host, &avatar)?;
    if !src.is_file() {
        return Err(format!("角色卡文件不存在：{}", src.display()));
    }
    let dest = PathBuf::from(dest.trim());
    tauri::async_runtime::spawn_blocking(move || {
        std::fs::copy(&src, &dest)
            .map(|_| ())
            .map_err(|e| format!("导出失败：{e}"))
    })
    .await
    .map_err(|e| format!("任务异常：{e}"))?
}

/// 行级增量改写会话文件：除目标行外不解析、不序列化——swipes/extra 等
/// 扩展字段天然保真，也避免整文件经前后端往返。
/// ST 后端对 jsonl 无长驻缓存（每请求重读），运行时改写安全；
/// 前端串行调用（store 内 await），与 saveCurrent 全量保存互不穿插。
#[tauri::command]
async fn chat_mutate_lines(
    host: Host<'_>,
    avatar: String,
    file: String,
    ops: Vec<ChatLineOp>,
) -> Result<(), String> {
    let path = chat_jsonl_path(&*host, &avatar, &file)?;
    tauri::async_runtime::spawn_blocking(move || {
        let raw = std::fs::read_to_string(&path).map_err(|e| format!("读取会话失败：{e}"))?;
        let mut lines: Vec<String> = raw.lines().map(|s| s.to_string()).collect();
        for op in ops {
            match op {
                ChatLineOp::Replace { index, json } => {
                    if index >= lines.len() {
                        return Err(format!("行号越界：{index}（共 {} 行）", lines.len()));
                    }
                    serde_json::from_str::<serde_json::Value>(&json)
                        .map_err(|e| format!("第 {index} 行不是合法 JSON：{e}"))?;
                    lines[index] = json;
                }
                ChatLineOp::Delete { index } => {
                    if index >= lines.len() {
                        return Err(format!("行号越界：{index}（共 {} 行）", lines.len()));
                    }
                    lines.remove(index);
                }
                ChatLineOp::Insert { index, json } => {
                    serde_json::from_str::<serde_json::Value>(&json)
                        .map_err(|e| format!("插入内容不是合法 JSON：{e}"))?;
                    let index = index.min(lines.len());
                    lines.insert(index, json);
                }
            }
        }
        // 原子写回：临时文件 + rename，防中途崩溃损坏会话
        let tmp = path.with_extension("jsonl.tmp");
        std::fs::write(&tmp, format!("{}\n", lines.join("\n")))
            .map_err(|e| format!("写入临时文件失败：{e}"))?;
        std::fs::rename(&tmp, &path).map_err(|e| format!("替换会话文件失败：{e}"))?;
        Ok::<(), String>(())
    })
    .await
    .map_err(|e| format!("任务异常：{e}"))?
}

/// 导出会话原版 jsonl 到指定路径（与 ST 文件互通）
#[tauri::command]
async fn chat_export_file(
    host: Host<'_>,
    avatar: String,
    file: String,
    dest: String,
) -> Result<(), String> {
    let src = chat_jsonl_path(&*host, &avatar, &file)?;
    if !src.is_file() {
        return Err(format!("会话文件不存在：{}", src.display()));
    }
    let dest = PathBuf::from(dest.trim());
    tauri::async_runtime::spawn_blocking(move || {
        std::fs::copy(&src, &dest)
            .map(|_| ())
            .map_err(|e| format!("导出失败：{e}"))
    })
    .await
    .map_err(|e| format!("任务异常：{e}"))?
}

/// 从数据袋 zip 导入（覆盖现有数据；导入前自动备份为 data.bak-<时间戳>）。
#[tauri::command]
async fn st_data_import(host: Host<'_>, src: String) -> Result<st_data::DataBagResult, String> {
    let (root, cfg, _) = data_bag_context(&host)?;
    let src = PathBuf::from(src.trim());
    if !src.is_file() {
        return Err(format!("导入文件不存在：{}", src.display()));
    }
    let cfg_ref = cfg.is_file().then_some(cfg);
    tauri::async_runtime::spawn_blocking(move || st_data::import_bag(&src, &root, cfg_ref.as_deref()))
        .await
        .map_err(|e| format!("导入任务异常：{e}"))?
}

/* ================================================================== *
 * 数据区诊断模式（S2）
 * ================================================================== */

/// `st-chat.exe --data-plan <configDir>`     只读预览迁移计划
/// `st-chat.exe --migrate-data <configDir>`  执行迁移
/// `st-chat.exe --data-rollback <configDir>` 回滚到 ST 目录内
///
/// 走的是**与设置页完全相同的宿主逻辑**，用于无 GUI 环境验证与用户取证。
fn cli_data(mode: &str, config_dir: &Path) -> i32 {
    let _ = std::fs::create_dir_all(config_dir);
    let host = plugin::PluginHost::new(config_dir);

    let result: Result<(String, &str), String> = match mode {
        "plan" => host
            .data_plan()
            .and_then(|p| serde_json::to_string_pretty(&p).map_err(|e| e.to_string()))
            .map(|j| (j, "data-plan.json")),
        "migrate" => host
            .data_migrate(&mut |ev| {
                if !ev.message.is_empty() {
                    let _ = writeln!(std::io::stdout(), "[{}] {}", ev.phase, ev.message);
                }
            })
            .and_then(|r| serde_json::to_string_pretty(&r).map_err(|e| e.to_string()))
            .map(|j| (j, "data-migrate.json")),
        "rollback" => host
            .data_rollback()
            .and_then(|_| {
                host.data_status()
                    .and_then(|s| serde_json::to_string_pretty(&s).map_err(|e| e.to_string()))
            })
            .map(|j| (j, "data-rollback.json")),
        other => Err(format!("未知模式：{other}")),
    };

    match result {
        Ok((json, file)) => {
            let out = config_dir.join(file);
            let _ = std::fs::write(&out, &json);
            let _ = writeln!(std::io::stdout(), "{json}");
            let _ = writeln!(
                std::io::stdout(),
                "written: {}",
                st_data::display_path(&out)
            );
            0
        }
        Err(e) => {
            let _ = writeln!(std::io::stdout(), "failed: {e}");
            let _ = writeln!(std::io::stderr(), "failed: {e}");
            1
        }
    }
}

/* ================================================================== *
 * 组件安装诊断模式（S4）
 * ================================================================== */

/// `st-chat.exe --install-component <configDir> <component> <version>`
///
/// 装到 `<configDir>/plugins/<component>/<version>/`。
/// **不切换绑定** —— 装完由用户在插件页确认，避免"装失败却已经改了绑定"。
fn cli_install(config_dir: &Path, component: &str, version: &str) -> i32 {
    let _ = std::fs::create_dir_all(config_dir);
    let host = plugin::PluginHost::new(config_dir);

    let Some(m) = host.manifest(component).cloned() else {
        let _ = writeln!(std::io::stdout(), "failed: 未知组件 {component}");
        return 1;
    };

    let platform = installer::platform_key();
    let spec = match installer::choose_spec(&m, version, &platform) {
        Ok(s) => s,
        Err(e) => {
            let _ = writeln!(std::io::stdout(), "failed: {e}");
            return 1;
        }
    };

    // 装 ST 需要 node 来跑 npm install（node 本身不需要）
    let node_exe = host
        .reports()
        .iter()
        .find(|r| r.id == "node")
        .and_then(|r| r.path.clone())
        .map(PathBuf::from);

    let _ = writeln!(
        std::io::stdout(),
        "组件 {} {} | 平台 {} | 需要 npm: {} | 预打包: {} | node: {}",
        component,
        version,
        platform,
        spec.npm_install,
        spec.prebuilt,
        node_exe
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "（不需要）".into())
    );

    // 代理：环境变量 ST_CHAT_PROXY > app-settings.json > 直连
    let proxy = app_settings::effective_proxy(config_dir);
    let settings = app_settings::load(config_dir);
    // npm 是否跟随代理由开关决定；不跟随时传 None，installer 会显式清空代理环境变量
    let npm_proxy = if settings.proxy_for_npm {
        proxy.clone()
    } else {
        None
    };
    let _ = writeln!(
        std::io::stdout(),
        "代理：{}　npm 跟随：{}",
        proxy.as_deref().unwrap_or("（直连）"),
        settings.proxy_for_npm
    );

    let rt = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(r) => r,
        Err(e) => {
            let _ = writeln!(std::io::stdout(), "failed: 创建 runtime 失败 {e}");
            return 1;
        }
    };

    let client = match downloader::build_client(proxy.as_deref()) {
        Ok(c) => c,
        Err(e) => {
            let _ = writeln!(std::io::stdout(), "failed: {e}");
            return 1;
        }
    };
    // ⚠ 大资产装到 LOCALAPPDATA，不装 Roaming（实测 Roaming 下 npm install 慢 30 倍，见 installer::install_root）
    let root = installer::install_root(config_dir);
    let cache = installer::component_root(&root, "_cache");

    let _ = writeln!(
        std::io::stdout(),
        "安装根目录: {}",
        st_data::display_path(&root)
    );

    let mut last_phase = String::new();
    let result = rt.block_on(installer::install(
        &client,
        &root,
        node_exe.as_deref(),
        &spec,
        &cache,
        npm_proxy.as_deref(),
        |p| {
            // 进度每 200ms 一次，全打会刷屏：只在阶段切换或带说明时输出
            if p.phase != last_phase || !p.message.is_empty() {
                last_phase = p.phase.clone();
                let _ = writeln!(
                    std::io::stdout(),
                    "[{}] {:.0}% {}",
                    p.phase,
                    p.percent,
                    p.message
                );
            }
        },
    ));

    match result {
        Ok(o) => {
            let json = serde_json::to_string_pretty(&o).unwrap_or_default();
            let _ = writeln!(std::io::stdout(), "{json}");
            let out = config_dir.join(format!("install-{}-{}.json", o.component, o.version));
            let _ = std::fs::write(&out, &json);
            let _ = writeln!(
                std::io::stdout(),
                "\n✅ 安装完成：{}（{} 个文件 / {:.1} MB / {:.1}s）",
                st_data::display_path(Path::new(&o.dir)),
                o.files,
                o.bytes as f64 / 1048576.0,
                o.elapsed_ms as f64 / 1000.0
            );
            let _ = writeln!(std::io::stdout(), "written: {}", st_data::display_path(&out));
            0
        }
        Err(e) => {
            let _ = writeln!(std::io::stdout(), "\n❌ 安装失败：{e}");
            let _ = writeln!(std::io::stderr(), "failed: {e}");
            1
        }
    }
}

/* ================================================================== *
 * 入口
 * ================================================================== */

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 诊断模式先于 Tauri 启动，不创建窗口
    let args: Vec<String> = std::env::args().collect();
    let flag_dir = |flag: &str| -> Option<String> {
        args.iter()
            .position(|a| a == flag)
            .map(|i| args.get(i + 1).cloned().unwrap_or_else(|| ".".to_string()))
    };

    if let Some(dir) = flag_dir("--dump-plugins") {
        std::process::exit(dump_plugins_report(Path::new(&dir)));
    }
    if let Some(dir) = flag_dir("--data-plan") {
        std::process::exit(cli_data("plan", Path::new(&dir)));
    }
    if let Some(dir) = flag_dir("--migrate-data") {
        std::process::exit(cli_data("migrate", Path::new(&dir)));
    }
    if let Some(dir) = flag_dir("--data-rollback") {
        std::process::exit(cli_data("rollback", Path::new(&dir)));
    }
    // --install-component <configDir> <component> <version>
    if let Some(i) = args.iter().position(|a| a == "--install-component") {
        let dir = args.get(i + 1).cloned().unwrap_or_else(|| ".".into());
        let comp = args.get(i + 2).cloned().unwrap_or_default();
        let ver = args.get(i + 3).cloned().unwrap_or_default();
        if comp.is_empty() || ver.is_empty() {
            let _ = writeln!(
                std::io::stdout(),
                "用法：--install-component <配置目录> <组件> <版本>"
            );
            std::process::exit(2);
        }
        std::process::exit(cli_install(Path::new(&dir), &comp, &ver));
    }

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            st_relay_base,
            st_status,
            st_retry_start,
            plugins_stack,
            plugins_rescan,
            plugin_set_binding,
            plugin_set_enabled,
            plugin_set_port,
            st_data_status,
            st_data_plan,
            st_data_migrate,
            st_data_rollback,
            st_data_remove_external,
            st_pause_for_migration,
            st_data_export,
            st_data_import,
            chat_mutate_lines,
            chat_export_file,
            character_export_png,
            component_versions,
            component_install,
            component_use,
            component_remove,
            component_install_state,
            get_app_settings,
            set_app_settings
        ])
        .setup(|app| {
            let handle = app.handle().clone();

            // 0) 插件宿主
            let config_dir = app.path().app_config_dir()?;
            let _ = std::fs::create_dir_all(&config_dir);
            let host = Arc::new(plugin::PluginHost::new(&config_dir));
            app.manage(host.clone());

            // 1) 托管 SillyTavern（解析失败不阻断启动：前端插件页会显示原因与修复入口）
            let st_port = match host.resolve() {
                Ok(mut res) => {
                    println!(
                        "[setup] 插件解析：st_dir={} node={} port={} data_root={}",
                        res.st_dir.display(),
                        res.node.display(),
                        res.st_port,
                        res.data_root.display()
                    );

                    // 1.5) 首次挂载自动同步：ST 目录里已有数据、外移目标还是空的 → 复制过去。
                    // 复用 st_data 迁移引擎（含五条安全铁律）。此刻 ST 尚未拉起，
                    // 不违反「运行中禁止迁移」。目标非空 = 已经同步过，直接跳过。
                    let src_data = st_data::default_data_root(&res.st_dir);
                    let dst_data = st_data::external_data_root(&config_dir);
                    let dst_occupied = std::fs::read_dir(&dst_data)
                        .map(|mut d| d.next().is_some())
                        .unwrap_or(false);
                    if src_data.is_dir() && !dst_occupied {
                        let st_dir = res.st_dir.clone();
                        let cfg = config_dir.clone();
                        let port = res.st_port;
                        match st_data::execute(&st_dir, &cfg, port, &mut |p| {
                            println!("[setup] 数据同步[{}] {}", p.phase, p.message);
                        }) {
                            Ok(r) => println!("[setup] 数据区已同步：{}", r.message),
                            Err(e) => {
                                // 同步没成、目标又是空的 → 不能带着空目录启动（用户会以为数据丢了）
                                eprintln!("[setup] 数据自动同步未完成，回退到 ST 默认位置：{e}");
                                host.clear_data_binding();
                                let (dr, cp) = host.data_paths(&res.st_dir);
                                res.data_root = dr;
                                res.config_path = cp;
                            }
                        }
                    }

                    match st_sidecar::ensure_running(&handle, &res, &config_dir) {
                        Ok(p) => p,
                        Err(e) => {
                            eprintln!("[setup] SillyTavern 托管未启动: {e}");
                            res.st_port
                        }
                    }
                }
                Err(e) => {
                    eprintln!("[setup] 插件解析失败: {e}");
                    // 必须把错误态推给启动屏：否则 SidecarState 未注册，
                    // 前端永远停在「正在启动 SillyTavern…」，用户无从得知要修绑定
                    st_sidecar::mark_unresolved(
                        &handle,
                        st_sidecar::DEFAULT_ST_PORT,
                        &e,
                    );
                    st_sidecar::DEFAULT_ST_PORT
                }
            };

            // 2) 启动中继（启动时生成一次性鉴权 token，与基地址一并交给前端）
            let relay_ep = tauri::async_runtime::block_on(relay::start(st_port))
                .map_err(|e| format!("HTTP 中继启动失败: {e}"))?;
            app.manage(RelayAuth {
                port: relay_ep.port,
                token: relay_ep.token,
            });

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|handle, event| {
        // 4) 退出清理（只杀自己拉起的 ST）
        if let tauri::RunEvent::Exit = event {
            st_sidecar::stop_sillytavern(handle);
        }
    });
}
