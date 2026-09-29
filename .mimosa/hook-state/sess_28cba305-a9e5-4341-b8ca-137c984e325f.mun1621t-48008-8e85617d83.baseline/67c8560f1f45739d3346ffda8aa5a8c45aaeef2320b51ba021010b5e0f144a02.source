//! SillyTavern Sidecar 进程管理
//!
//! 职责（《技术选型与项目架构 v1.0》）：
//! 1. 以子进程方式拉起 SillyTavern（纯后端 + API 模式，不暴露其 Web UI）
//! 2. 轮询 `/csrf-token` 判断就绪，通过事件 `st-status` 推给前端
//! 3. 应用退出时终止 ST 进程（含整棵进程树）
//!
//! ── 与插件宿主的分工 ──
//! 「ST 在哪、用哪个 node、用哪个端口」**全部由 `plugin::PluginHost` 解析**（见其 `resolve()`）。
//! 本模块只负责「拿着解析结果把进程管起来」——单一事实来源，
//! 避免出现两套路径解析互相打架。
//!
//! ── 重要：只清理「自己拉起的」ST ──
//! 如果端口上已经有 ST 在跑（比如用户手动双击了 `start-detached.bat`），
//! 本模块只标记为 external，**绝不接管也绝不在退出时杀掉它**。

use serde::Serialize;
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};
use tokio::process::{Child, Command};

use crate::app_settings;
use crate::plugin::ResolvedStack;

pub const DEFAULT_ST_PORT: u16 = 8000;
const EVENT_STATUS: &str = "st-status";
/// Windows `CREATE_NO_WINDOW`：不弹控制台窗口
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/* ------------------------------------------------------------------ *
 * 状态
 * ------------------------------------------------------------------ */

#[derive(Debug, Clone, Serialize)]
pub struct StatusDto {
    /// starting | ready | error | stopped
    pub state: String,
    pub message: String,
    pub port: u16,
    pub pid: Option<u32>,
    pub st_dir: String,
    pub node: String,
    /// true = 本应用拉起的（退出时会清理）；false = 外部已在运行
    pub managed: bool,
}

pub struct SidecarState {
    pub port: u16,
    pub st_dir: PathBuf,
    pub node: String,
    /// 可变的 managed 标志（P1）：解析失败→重试成功后确实 spawn 了子进程，
    /// 必须把 mark_unresolved 留下的 false 纠正为 true，否则退出时不清理成孤儿
    managed: AtomicBool,
    child: Mutex<Option<Child>>,
    /// 最近一次状态（供前端同步查询）
    last: Mutex<(String, String)>,
    /// 子进程已退出（watcher 置位）——健康轮询看到即退出，不再傻等 90s 超时
    child_gone: AtomicBool,
}

impl SidecarState {
    fn new(port: u16, st_dir: PathBuf, node: String, managed: bool, child: Option<Child>) -> Self {
        Self {
            port,
            st_dir,
            node,
            managed: AtomicBool::new(managed),
            child: Mutex::new(child),
            last: Mutex::new(("starting".into(), "正在启动 SillyTavern…".into())),
            child_gone: AtomicBool::new(false),
        }
    }

    pub fn managed(&self) -> bool {
        self.managed.load(Ordering::SeqCst)
    }

    pub fn set_managed(&self, v: bool) {
        self.managed.store(v, Ordering::SeqCst);
    }

    fn dto(&self, state: &str, message: &str) -> StatusDto {
        StatusDto {
            state: state.to_string(),
            message: message.to_string(),
            port: self.port,
            pid: self
                .child
                .lock()
                .ok()
                .and_then(|g| g.as_ref().and_then(|c| c.id())),
            st_dir: self.st_dir.to_string_lossy().into_owned(),
            node: self.node.clone(),
            managed: self.managed(),
        }
    }

    /// 当前状态快照
    pub fn report(&self) -> StatusDto {
        let (state, message) = self
            .last
            .lock()
            .map(|g| g.clone())
            .unwrap_or_else(|_| ("unknown".into(), String::new()));
        self.dto(&state, &message)
    }

    fn emit(&self, app: &AppHandle, state: &str, message: &str) {
        if let Ok(mut g) = self.last.lock() {
            *g = (state.to_string(), message.to_string());
        }
        let _ = app.emit(EVENT_STATUS, self.dto(state, message));
    }
}

/* ------------------------------------------------------------------ *
 * 端口探测
 * ------------------------------------------------------------------ */

fn is_listening(port: u16) -> bool {
    let addr = format!("127.0.0.1:{port}");
    match addr.to_socket_addrs() {
        Ok(mut it) => match it.next() {
            Some(sa) => TcpStream::connect_timeout(&sa, Duration::from_millis(400)).is_ok(),
            None => false,
        },
        Err(_) => false,
    }
}

/// 端口占用者身份探测（P1）：TCP 可连 ≠ 是 ST。
/// 之前的实现把任何恰好监听该端口的程序都当成「外部 SillyTavern 已就绪」，
/// 中继随之把前端流量转发给陌生服务，且真正的 ST 永远不会被拉起。
/// 这里对 `/csrf-token` 发一个极简 HTTP 请求 —— ST 恒回 200 + `{"token": ...}`。
fn looks_like_sillytavern(port: u16) -> bool {
    use std::io::{Read, Write};
    let Ok(mut it) = format!("127.0.0.1:{port}").to_socket_addrs() else {
        return false;
    };
    let Some(sa) = it.next() else { return false };
    let Ok(mut s) = TcpStream::connect_timeout(&sa, Duration::from_millis(800)) else {
        return false;
    };
    let _ = s.set_read_timeout(Some(Duration::from_millis(1500)));
    let _ = s.set_write_timeout(Some(Duration::from_millis(1500)));
    let req =
        format!("GET /csrf-token HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n");
    if s.write_all(req.as_bytes()).is_err() {
        return false;
    }
    let mut buf = Vec::new();
    // 超时/提前断开时 read_to_end 返回 Err，但已读到的字节仍保留在 buf 里 —— 照常判定
    let _ = s.read_to_end(&mut buf);
    let text = String::from_utf8_lossy(&buf);
    text.contains(" 200 ") && text.contains("\"token\"")
}

/* ------------------------------------------------------------------ *
 * 启动 / 停止
 * ------------------------------------------------------------------ */

/// 确保 SillyTavern 在跑（**幂等**：首次启动与前端「重试」共用同一入口）。
/// `res` 由插件宿主解析得出（node 路径、ST 目录、端口、数据目录）。
/// `config_dir` 用于存放 PID 账本（`st.pid`）——退出异常时靠它找出孤儿进程。
pub fn ensure_running(
    app: &AppHandle,
    res: &ResolvedStack,
    config_dir: &Path,
) -> Result<u16, String> {
    let port = res.st_port;

    // 先给上次崩溃/强杀留下的孤儿收尸，再做端口探测。
    // 顺序很重要：如果先判 is_listening() 就会命中孤儿，把它当成「用户手动起的外部实例」，
    // 于是永远不再清理 —— 孤儿就此永久驻留。重复调用时无账本，零开销。
    reap_stale_sillytavern(port, config_dir);

    // state 注册（幂等）：首次创建；重试时复用已存在的
    if app.try_state::<SidecarState>().is_none() {
        let st = SidecarState::new(
            port,
            res.st_dir.clone(),
            res.node.to_string_lossy().into_owned(),
            true,
            None,
        );
        let _ = app.manage(st);
    }
    let state = app.state::<SidecarState>();

    // 端口已被占用 → 确认身份后按外部实例对待（只管用不管杀）。
    // （此时 child 恒为 None，退出清理自然是空操作 —— 不会误杀外部进程）
    if is_listening(port) {
        if !looks_like_sillytavern(port) {
            return Err(format!(
                "端口 {port} 已被其它程序占用（未响应 SillyTavern 探测）。\
                 请停用占用该端口的程序，或到插件页更换 ST 端口后重试"
            ));
        }
        println!("[sidecar] 端口 {port} 已有 SillyTavern 在运行，接管为外部实例（退出时不清理）");
        state.emit(app, "ready", "已连接外部启动的 SillyTavern");
        return Ok(port);
    }

    launch_and_watch(app, &state, res, config_dir)?;
    Ok(port)
}

/// 把「本 App 设置的 HTTP 代理」注入 ST 的 `config.yaml`（`requestProxy` 段）。
/// 为什么每次启动前都写：ST 不读环境变量代理，出站只认 config.yaml 的 requestProxy；
/// 而该文件会被 ST 自行重写 —— 所以我们在**每次 spawn 之前**重打补丁，确定性生效。
/// ST 网页端同样的能力叫「请求代理」，这里等价搬到 App 侧。
///
/// 实现为行级替换（YAML 结构固定，无新增依赖）：
/// 在 `requestProxy:` 段内把 `enabled` / `url` 两行替换为我们的值；段外不动。
fn inject_request_proxy(config_yaml: &Path, proxy: &str) {
    let Ok(text) = std::fs::read_to_string(config_yaml) else {
        return; // config 还没生成（首次启动前 ST 会自己建）→ 跳过
    };
    let mut out = String::with_capacity(text.len() + 64);
    let mut in_section = false;
    let mut changed = false;
    for line in text.lines() {
        if !line.starts_with(' ') && !line.starts_with('#') && !line.trim().is_empty() {
            in_section = line.trim() == "requestProxy:";
        }
        let new_line = if in_section && line.trim_start().starts_with("enabled:") {
            changed = true;
            let indent = &line[..line.len() - line.trim_start().len()];
            format!("{indent}enabled: true")
        } else if in_section && line.trim_start().starts_with("url:") {
            changed = true;
            let indent = &line[..line.len() - line.trim_start().len()];
            format!("{indent}url: {proxy}")
        } else {
            line.to_string()
        };
        out.push_str(&new_line);
        out.push('\n');
    }
    if changed {
        // 原子写：config.yaml 损坏会让 ST 起不来
        if let Err(e) = crate::st_data::atomic_write(config_yaml, out.as_bytes()) {
            eprintln!("[sidecar] 写入 requestProxy 失败: {e}");
        } else {
            println!("[sidecar] 已为 ST 注入出站代理（requestProxy → {proxy}）");
        }
    }
}

/// 拉起 ST 子进程 + 健康检查轮询（供 `ensure_running` 首次启动与重试共用）。
fn launch_and_watch(
    app: &AppHandle,
    st: &SidecarState,
    res: &ResolvedStack,
    config_dir: &Path,
) -> Result<(), String> {
    let port = res.st_port;
    let node = res.node.to_string_lossy().into_owned();

    let log_path = app
        .path()
        .app_log_dir()
        .ok()
        .map(|d| {
            let _ = std::fs::create_dir_all(&d);
            d.join("sillytavern.log")
        })
        .and_then(|p| std::fs::File::create(p).ok());

    let mut cmd = Command::new(&res.node);
    cmd.arg("server.js")
        // 端口显式传入：CLI 优先于 config.yaml，否则配置里的端口不会真生效
        .arg("--port")
        .arg(port.to_string())
        // 仅本地回环：ST 只当后端使，不把它的 Web UI 暴露到局域网
        // （对应《技术选型与项目架构 v1.0》「纯后端不暴露」的约束）
        .arg("--listen=false")
        // ⚠ 参数名必须是**驼峰连写**的 `browserLaunchEnabled`！
        // 之前写的 `--browserLaunch.enabled=false` 是 yargs 不认识的键，会被直接忽略，
        // 于是取值链掉到 config.yaml 的 `browserLaunch.enabled: true`（ST 默认）
        // → 每次启动都自动弹浏览器。见 command-line.js:311 的 `??` 优先级链：
        //   cliArguments.browserLaunchEnabled ?? cliArguments.autorun ?? getConfigValue('browserLaunch.enabled')
        .arg("--browserLaunchEnabled=false");

    // 数据区外移（S2）：**无条件传** —— res.data_root 已经是 data_paths() 的裁决结果，
    // 没绑定时它就等于 ST 默认位置，传了也等价。
    // ⚠ 之前「≠ 默认才传」的写法在绑定失效回退时会漏传（两者恰好相等），
    //   于是 ST 用默认值把数据写进代码目录 —— 这正是 S2 要防的事故。
    cmd.arg("--dataRoot")
        .arg(res.data_root.to_string_lossy().to_string());
    if res.config_path != res.st_dir.join("config.yaml") {
        // ⚠ ST 只在 global 模式下创建 configPath 的父目录（command-line.js:283），
        // 非 global 模式不会 —— 所以父目录必须由我们先建好，
        // 否则首次以外移配置启动会因写不了 config.yaml 而失败。
        if let Some(parent) = res.config_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("创建配置目录失败 {}: {e}", parent.display()))?;
        }
        cmd.arg("--configPath")
            .arg(res.config_path.to_string_lossy().to_string());
    }

    cmd.current_dir(&res.st_dir).stdin(Stdio::null());

    // 出站代理：ST 出站（拉模型/调上游 API）只认 config.yaml 的 requestProxy。
    // 用户在「设置 → 下载与安装」里填了 HTTP 代理 → 每次启动前注入（ST 会重写该文件，所以要重打补丁）。
    if let Some(proxy) = app_settings::effective_proxy(config_dir) {
        inject_request_proxy(&res.config_path, &proxy);
    }

    match log_path {
        Some(f) => {
            let f2 = f
                .try_clone()
                .map(Stdio::from)
                .unwrap_or_else(|_| Stdio::null());
            cmd.stdout(Stdio::from(f)).stderr(f2);
        }
        None => {
            cmd.stdout(Stdio::null()).stderr(Stdio::null());
        }
    }
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);

    let child = cmd
        .spawn()
        .map_err(|e| format!("拉起 SillyTavern 失败（node={node}）: {e}"))?;

    // P1：确实 spawn 成功了 —— 把 mark_unresolved 留下的 managed=false 纠正回来，
    // 否则「解析失败→修复→重试」路径拉起的 ST 退出时永远不被清理（孤儿进程）
    st.set_managed(true);

    // 记账：万一 App 崩溃/被强杀，下次启动能凭这条记录找到它并收尸
    if let Some(pid) = child.id() {
        write_pid_file(config_dir, pid);
    }

    if let Ok(mut g) = st.child.lock() {
        *g = Some(child);
    }
    st.emit(app, "starting", "正在启动 SillyTavern…");

    // 退出观察者（P1）：node 秒退（端口冲突/语法错误）时立刻报错，
    // 不再让用户对着「正在启动」干等 90 秒健康检查超时
    let watch_handle = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_millis(500)).await;
            let Some(s) = watch_handle.try_state::<SidecarState>() else {
                return; // app 关闭
            };
            if s.child_gone.load(Ordering::SeqCst) {
                return;
            }
            let polled = {
                let Ok(mut g) = s.child.lock() else { return };
                match g.as_mut() {
                    // child 已被 stop_sillytavern 取走 → 正常退出路径，静默结束
                    None => return,
                    Some(c) => c.try_wait(),
                }
            };
            if let Ok(Some(status)) = polled {
                s.child_gone.store(true, Ordering::SeqCst);
                let msg = format!(
                    "SillyTavern 进程已退出（{status}）。详见日志 sillytavern.log"
                );
                s.emit(&watch_handle, "error", &msg);
                eprintln!("[sidecar] {msg}");
                return;
            }
        }
    });

    // 健康检查：轮询 /csrf-token（子进程先退出的场合由 watcher 报错，这里提前收工）
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let url = format!("http://127.0.0.1:{port}/csrf-token");
        let client = match reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(3))
            .build()
        {
            Ok(c) => c,
            Err(_) => return,
        };
        let deadline = Instant::now() + Duration::from_secs(90);
        loop {
            if let Some(s) = handle.try_state::<SidecarState>() {
                if s.child_gone.load(Ordering::SeqCst) {
                    return; // 子进程已退出，错误由 watcher 上报
                }
            }
            if let Ok(r) = client.get(&url).send().await {
                if r.status().is_success() {
                    if let Some(s) = handle.try_state::<SidecarState>() {
                        s.emit(&handle, "ready", "SillyTavern 已就绪");
                    }
                    println!("[sidecar] SillyTavern 就绪（端口 {port}）");
                    return;
                }
            }
            if Instant::now() >= deadline {
                if let Some(s) = handle.try_state::<SidecarState>() {
                    s.emit(&handle, "error", "SillyTavern 启动超时（90s）");
                }
                eprintln!("[sidecar] SillyTavern 启动超时");
                return;
            }
            tokio::time::sleep(Duration::from_millis(600)).await;
        }
    });

    Ok(())
}

/// 应用退出时调用：只清理自己拉起的 ST（含进程树）
pub fn stop_sillytavern(app: &AppHandle) {
    let Some(state) = app.try_state::<SidecarState>() else {
        return;
    };
    if !state.managed() {
        println!("[sidecar] 外部实例，退出时不做清理");
        return;
    }
    let Ok(mut guard) = state.child.lock() else {
        return;
    };
    let Some(mut child) = guard.take() else {
        return;
    };
    if let Some(pid) = child.id() {
        println!("[sidecar] 正在停止 SillyTavern（pid={pid}）…");
        kill_tree(pid);
    }
    let _ = child.start_kill();
    // 干净退出 → 销账，下次启动不必再查
    if let Ok(dir) = app.path().app_config_dir() {
        clear_pid_file(&dir);
    }
}

/// 把当前托管状态推到前端（供迁移流程在暂停 ST 后更新状态条）
pub fn mark_state(app: &AppHandle, state: &str, message: &str) {
    if let Some(s) = app.try_state::<SidecarState>() {
        s.emit(app, state, message);
    }
}

/// 插件解析失败（如「未找到 SillyTavern」）时，把错误态推给启动屏。
///
/// 没有它，setup 阶段解析失败只会打日志：SidecarState 从未注册，
/// 前端 `st_status` 拿到 null → 启动屏永远停在「正在启动 SillyTavern…」，
/// 用户连错误都看不到，只能瞎等或瞎点。
/// st_dir / node 此刻未知，先留空；用户修好绑定后 `st_retry_start`
/// 会走 `ensure_running` 复用已注册的 State（幂等）。
pub fn mark_unresolved(app: &AppHandle, port: u16, message: &str) {
    if app.try_state::<SidecarState>().is_none() {
        let _ = app.manage(SidecarState::new(
            port,
            PathBuf::new(),
            String::new(),
            false,
            None,
        ));
    }
    if let Some(s) = app.try_state::<SidecarState>() {
        s.emit(app, "error", message);
    }
}

/// **数据迁移前置：让 SillyTavern 停下。**
///
/// 运行中迁移会把"正在写入的数据"落空 —— 这是迁移唯一的真实数据风险，
/// 所以宁可拒绝也不能带着 ST 搬。
///   - 本应用托管 → 停掉（含进程树）并等端口真正释放
///   - 外部实例 → 拒绝（用户自己起的进程，我们无权杀）
///   - 未运行 → 直接放行
///
/// 幂等：已停止时重复调用安全。
pub fn prepare_for_migration(app: &AppHandle) -> Result<Option<String>, String> {
    let Some(state) = app.try_state::<SidecarState>() else {
        return Ok(None);
    };

    if !state.managed() {
        if is_listening(state.port) {
            return Err(format!(
                "SillyTavern 由外部启动（端口 {}），本应用无权停止它，\
                 因此无法在运行中迁移。请先手动关闭它，再回到这里重试。",
                state.port
            ));
        }
        return Ok(None);
    }

    let port = state.port;
    // 已经停过（child 已取走）→ 只需确认端口释放
    stop_sillytavern(app);
    for _ in 0..30 {
        if !is_listening(port) {
            mark_state(app, "stopped", "已为数据迁移暂停，迁移完成后请重启应用");
            return Ok(Some(format!("已暂停本应用托管的 SillyTavern（端口 {port}）")));
        }
        std::thread::sleep(Duration::from_millis(200));
    }

    Err(format!(
        "已发出停止指令但端口 {port} 仍被占用，请稍后重试或检查是否有其它 SillyTavern 进程"
    ))
}

/* ------------------------------------------------------------------ *
 * 残留清理（PID 记账）
 * ------------------------------------------------------------------ */

// ── 为什么需要它 ──
// App 被强杀或崩溃时 `stop_sillytavern()` 不会执行，我们拉起的 ST 就成了孤儿。
// 由于 ST 端口固定，下次启动会命中 `is_listening()` → 标记为 external
// →「退出时不清理」→ 孤儿从此永久驻留，后续所有会话都连到它身上，**再也 resets 不掉**。
//
// 对策：把每次拉起的 ST pid 记进 `st.pid`，启动时先查账。
// **只杀自己记过的进程** —— 用户手动 `node server.js` 起的实例不在账本里，不受影响。

const ST_PID_FILE: &str = "st.pid";

fn st_pid_file(config_dir: &Path) -> PathBuf {
    config_dir.join(ST_PID_FILE)
}

fn write_pid_file(config_dir: &Path, pid: u32) {
    if let Err(e) = std::fs::write(st_pid_file(config_dir), pid.to_string()) {
        eprintln!("[sidecar] 写入 {ST_PID_FILE} 失败: {e}");
    }
}

fn clear_pid_file(config_dir: &Path) {
    let _ = std::fs::remove_file(st_pid_file(config_dir));
}

/// 谁在监听该端口（返回 pid）——用来确认进程身份，避免误杀。
fn port_owner_pid(port: u16) -> Option<u32> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let out = std::process::Command::new("netstat")
            .args(["-ano", "-p", "TCP"])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .ok()?;
        parse_listening_pid(&String::from_utf8_lossy(&out.stdout), port)
    }
    #[cfg(not(windows))]
    {
        let _ = port;
        None
    }
}

/// 从 `netstat -ano -p TCP` 的输出里找出 LISTENING 于 `port` 的 pid。
///
/// 抽成纯函数是为了能单测 —— 这里最怕的不是查不到，而是**查错**：
/// 一旦把 `0.0.0.0:8000` 这种"别人连出去的"当成监听者，就会去杀无辜进程。
fn parse_listening_pid(text: &str, port: u16) -> Option<u32> {
    let suffix = format!(":{port}");
    for line in text.lines() {
        if !line.contains("LISTENING") {
            continue;
        }
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() < 5 {
            continue;
        }
        if cols[1].ends_with(&suffix) {
            return cols[4].parse::<u32>().ok();
        }
    }
    None
}

/// 该 pid 是不是 node（ST 跑在 node 上）——二次确认，防 pid 复用后误杀。
fn is_node_process(pid: u32) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        std::process::Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).contains("node.exe"))
            .unwrap_or(false)
    }
    #[cfg(not(windows))]
    {
        let _ = pid;
        false
    }
}

/// 启动前查账：把上次没来得及清理的本应用 ST 送走。
/// **三重确认后才动手**：① 账本里有这条记录 ② 它正占着 ST 端口 ③ 它确实是 node 进程。
pub fn reap_stale_sillytavern(port: u16, config_dir: &Path) {
    let Ok(txt) = std::fs::read_to_string(st_pid_file(config_dir)) else {
        return; // 没有账本 = 首次启动或上次干净退出，零开销
    };
    let Ok(pid) = txt.trim().parse::<u32>() else {
        clear_pid_file(config_dir);
        return;
    };
    if port_owner_pid(port) != Some(pid) || !is_node_process(pid) {
        clear_pid_file(config_dir);
        return;
    }
    println!("[sidecar] 发现上次残留的 SillyTavern（pid={pid}），先收尸再启动");
    kill_tree(pid);
    clear_pid_file(config_dir);
    // 等端口腾出来，最长 2.5s（通常几百毫秒）。干净启动时不会走到这里。
    for _ in 0..25 {
        if !is_listening(port) {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// 终止整棵进程树：node 可能派生子进程，只 kill 父进程会留孤儿
fn kill_tree(pid: u32) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .creation_flags(CREATE_NO_WINDOW)
            .output();
    }
    #[cfg(not(windows))]
    {
        let _ = std::process::Command::new("kill")
            .args(["-TERM", &pid.to_string()])
            .output();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 真实 `netstat -ano -p TCP` 输出片段（含表头、IPv6、以及会误导人的 ESTABLISHED / TIME_WAIT）
    const NETSTAT_SAMPLE: &str = "\
Proto  Local Address          Foreign Address        State           PID
  TCP    0.0.0.0:135            0.0.0.0:0              LISTENING       1234
  TCP    127.0.0.1:8000         0.0.0.0:0              LISTENING       44044
  TCP    192.168.1.5:54321      104.18.2.1:443         ESTABLISHED     9999
  TCP    127.0.0.1:54322        127.0.0.1:8000         ESTABLISHED     8888
  TCP    127.0.0.1:8000         127.0.0.1:54322        ESTABLISHED     44044
  TCP    127.0.0.1:54323        127.0.0.1:8000         TIME_WAIT       0
  TCP    [::1]:8001             [::]:0                 LISTENING       7777
";

    #[test]
    fn picks_the_pid_that_listens_on_the_port() {
        assert_eq!(parse_listening_pid(NETSTAT_SAMPLE, 8000), Some(44044));
        assert_eq!(parse_listening_pid(NETSTAT_SAMPLE, 8001), Some(7777));
        assert_eq!(parse_listening_pid(NETSTAT_SAMPLE, 135), Some(1234));
    }

    /// 关键的安全用例：同一个 pid 也出现在 ESTABLISHED 行里（连出去的那个），
    /// 但只要没有 LISTENING 就绝不能命中 —— 否则会去杀一个无辜进程。
    #[test]
    fn ignores_non_listening_rows() {
        // 54321 那行是 ESTABLISHED 到 443，不该被认为是"监听 443"
        assert_eq!(parse_listening_pid(NETSTAT_SAMPLE, 443), None);
        // 没人监听 4999
        assert_eq!(parse_listening_pid(NETSTAT_SAMPLE, 4999), None);
    }

    /// 端口后缀匹配必须是**精确的后缀**，不能 `8000` 命中 `18000`（字符串 ends_with 的经典坑）
    #[test]
    fn does_not_match_port_by_loose_substring() {
        let tricky = "  TCP    127.0.0.1:18000        0.0.0.0:0              LISTENING       5150\n";
        assert_eq!(parse_listening_pid(tricky, 8000), None);
        assert_eq!(parse_listening_pid(tricky, 18000), Some(5150));
    }

    #[test]
    fn tolerates_garbage_input() {
        assert_eq!(parse_listening_pid("", 8000), None);
        assert_eq!(parse_listening_pid("完全没有 netstat 结构的一行", 8000), None);
        assert_eq!(
            parse_listening_pid("  TCP    127.0.0.1:8000  LISTENING\n", 8000),
            None // 列数不足
        );
    }
}
