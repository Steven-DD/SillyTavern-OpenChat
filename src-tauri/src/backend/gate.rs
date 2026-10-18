//! backend/gate —— ST ↔ native backend 互斥闸门（评审整改 T6）
//!
//! 方案第九章只写「禁止 ST 与新后端同时运行」，未定义由谁执行、外部实例怎么办。
//! 评审 A4/RB1：双写同一数据目录是真实数据损坏风险 —— settings 整体覆盖语义下
//! 双发必然互相抹字段，JSONL 双写直接损坏。
//!
//! 本闸门是 backend mode 切换的**唯一通道**，复用 st_sidecar::prepare_for_migration
//! 的三分支语义（托管实例停掉并等端口释放 / 外部实例拒绝 / 未运行放行），
//! 但以**纯函数**形式实现（不依赖 Tauri AppHandle），供 backend 启动路径与
//! 将来的 mode 开关命令共用；Tauri 侧的 prepare_for_migration 仍是 UI 入口，
//! 两者判定口径一致（端口探测 + PID 账本）。
//!
//! ── 回滚前置校验（RB1/快检）──
//! native 写出的文件 ST 是否仍可读，切回 `st` 前必须抽样快检：
//! characters PNG 头 / characters charx zip 魔数（PK\x03\x04）/ chats JSONL
//! 首末行可解析 / worlds JSON 可解析 / settings.json JSON 可解析。
//! 不通过 → 拒绝回滚并输出清单（不静默放行）。

use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// 闸门拒绝原因（handler 层映射为可读错误）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateError(pub String);

impl std::fmt::Display for GateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl std::error::Error for GateError {}

/// 端口是否有人在听（500ms 连接超时，与 st_sidecar::is_listening 同口径）
pub fn is_listening(port: u16) -> bool {
    match format!("127.0.0.1:{port}").to_socket_addrs() {
        Ok(mut it) => it
            .next()
            .map(|sa| TcpStream::connect_timeout(&sa, Duration::from_millis(500)).is_ok())
            .unwrap_or(false),
        Err(_) => false,
    }
}

/// 外部 ST 活性判定：指定端口有监听即认为在跑（外部实例我们无法区分归属）。
/// `managed_check` 由调用方传入：true = 本应用托管的实例（可停）；false = 外部。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StPresence {
    /// 无实例在跑
    Absent,
    /// 本应用托管的实例在跑（可由壳层停止）
    Managed(u16),
    /// 外部实例在跑（拒绝切换，要求用户手动关闭）
    External(u16),
}

impl StPresence {
    pub fn probe(port: u16, managed: bool) -> Self {
        if !is_listening(port) {
            Self::Absent
        } else if managed {
            Self::Managed(port)
        } else {
            Self::External(port)
        }
    }
}

/// 切换前置：ST 必须不在跑。
/// - Absent → 放行
/// - Managed → 调用方先经 `st_sidecar::prepare_for_migration` 停止后重试
///   （本函数不持有 AppHandle，无法替壳层停 —— 语义上等价于闸门只认「已停」）
/// - External → 拒绝并给出可读指引
pub fn require_st_stopped(presence: StPresence) -> Result<(), GateError> {
    match presence {
        StPresence::Absent => Ok(()),
        StPresence::Managed(port) => Err(GateError(format!(
            "本应用托管的 SillyTavern 仍在运行（端口 {port}）：\
             请先经「数据迁移/模式切换」流程停止它，再切换 backend mode"
        ))),
        StPresence::External(port) => Err(GateError(format!(
            "检测到外部启动的 SillyTavern（端口 {port}），本应用无权停止它。\
             请先手动关闭它，再切换 backend mode"
        ))),
    }
}

/* ------------------------------------------------------------------ *
 * 回滚前置校验（抽样快检）
 * ------------------------------------------------------------------ */

/// 快检结果：通过 / 不通过（附失败清单）
pub struct RollbackCheck {
    pub ok: bool,
    pub failures: Vec<String>,
}

/// 单行 JSON 校验（worlds 用整文件，chats 用首末行 —— 本函数保留给
/// 单行场景的后续接线；当前 chats 走 first_and_last_json_ok）
#[allow(dead_code)]
fn first_json_line_ok(p: &Path) -> Result<(), String> {
    let text = std::fs::read_to_string(p).map_err(|e| format!("读取失败 {}: {e}", p.display()))?;
    let first = text.lines().next().unwrap_or("");
    serde_json::from_str::<serde_json::Value>(first)
        .map(|_| ())
        .map_err(|e| format!("首行非合法 JSON {}: {e}", p.display()))
}

fn png_magic_ok(p: &Path) -> Result<(), String> {
    use std::io::Read;
    let mut f = std::fs::File::open(p).map_err(|e| format!("打开失败 {}: {e}", p.display()))?;
    let mut head = [0u8; 8];
    f.read_exact(&mut head)
        .map_err(|e| format!("读取失败 {}: {e}", p.display()))?;
    if head == [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A] {
        Ok(())
    } else {
        Err(format!("PNG 魔数不符 {}", p.display()))
    }
}

/// charx 是 zip 容器（首 4 字节 PK\x03\x04）；样本头损坏说明写出中断或被改写
fn zip_magic_ok(p: &Path) -> Result<(), String> {
    use std::io::Read;
    let mut f = std::fs::File::open(p).map_err(|e| format!("打开失败 {}: {e}", p.display()))?;
    let mut head = [0u8; 4];
    f.read_exact(&mut head)
        .map_err(|e| format!("读取失败 {}: {e}", p.display()))?;
    if head == [b'P', b'K', 0x03, 0x04] {
        Ok(())
    } else {
        Err(format!("charx zip 魔数不符 {}", p.display()))
    }
}

/// 回滚前置快检：native 写出的数据目录 ST 是否仍可读。
/// 抽样策略：settings.json 整份可解析 / characters 最多 3 张 PNG 魔数 + 最多
/// 3 个 charx zip 魔数 / chats 每角色最多 1 个 JSONL 首末行 / worlds 全部
/// JSON 解析（世界书通常少）。发现任何失败即拒绝回滚。
pub fn pre_rollback_check(data_root: &Path) -> RollbackCheck {
    let mut failures = Vec::new();
    let du = data_root.join("default-user");

    // settings.json：整份 JSON 可解析（单文件，无需抽样）
    let settings_path = du.join("settings.json");
    if settings_path.is_file() {
        if let Err(e) = std::fs::read_to_string(&settings_path)
            .map_err(|e| format!("读取失败 {}: {e}", settings_path.display()))
            .and_then(|t| {
                serde_json::from_str::<serde_json::Value>(&t)
                    .map(|_| ())
                    .map_err(|e| format!("JSON 解析失败 {}: {e}", settings_path.display()))
            })
        {
            failures.push(e);
        }
    }

    // characters：PNG 魔数（抽样 3）+ charx zip 魔数（抽样 3）
    let chars_dir = du.join("characters");
    if chars_dir.is_dir() {
        let mut png_n = 0;
        let mut charx_n = 0;
        for entry in std::fs::read_dir(&chars_dir).into_iter().flatten() {
            let p = match entry {
                Ok(e) => e.path(),
                Err(e) => {
                    failures.push(format!("遍历 characters 失败：{e}"));
                    break;
                }
            };
            match p.extension().and_then(|e| e.to_str()) {
                Some("png") if png_n < 3 => {
                    if let Err(e) = png_magic_ok(&p) {
                        failures.push(e);
                    }
                    png_n += 1;
                }
                Some("charx") if charx_n < 3 => {
                    if let Err(e) = zip_magic_ok(&p) {
                        failures.push(e);
                    }
                    charx_n += 1;
                }
                _ => {}
            }
            if png_n >= 3 && charx_n >= 3 {
                break;
            }
        }
    }

    // chats：每角色目录抽 1 个 JSONL 的首末行可解析
    let chats_dir = du.join("chats");
    if chats_dir.is_dir() {
        for entry in std::fs::read_dir(&chats_dir).into_iter().flatten() {
            let dir = match entry {
                Ok(e) => e.path(),
                Err(e) => {
                    failures.push(format!("遍历 chats 失败：{e}"));
                    break;
                }
            };
            if !dir.is_dir() {
                continue;
            }
            let jsonl = std::fs::read_dir(&dir)
                .into_iter()
                .flatten()
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .find(|p| p.extension().and_then(|x| x.to_str()) == Some("jsonl"));
            if let Some(p) = jsonl {
                if let Err(e) = first_and_last_json_ok(&p) {
                    failures.push(e);
                }
            }
        }
    }

    // worlds：全部 JSON 可解析
    let worlds_dir = du.join("worlds");
    if worlds_dir.is_dir() {
        for entry in std::fs::read_dir(&worlds_dir)
            .into_iter()
            .flatten()
            .flatten()
        {
            let p = entry.path();
            if p.extension().and_then(|x| x.to_str()) == Some("json") {
                if let Err(e) = std::fs::read_to_string(&p)
                    .map_err(|e| format!("读取失败 {}: {e}", p.display()))
                    .and_then(|t| {
                        serde_json::from_str::<serde_json::Value>(&t)
                            .map(|_| ())
                            .map_err(|e| format!("JSON 解析失败 {}: {e}", p.display()))
                    })
                {
                    failures.push(e);
                }
            }
        }
    }

    RollbackCheck {
        ok: failures.is_empty(),
        failures,
    }
}

fn first_and_last_json_ok(p: &Path) -> Result<(), String> {
    let text = std::fs::read_to_string(p).map_err(|e| format!("读取失败 {}: {e}", p.display()))?;
    let mut lines = text.lines().filter(|l| !l.trim().is_empty());
    let first = lines.next().unwrap_or("");
    let last = lines.last().unwrap_or(first);
    for (label, line) in [("首行", first), ("末行", last)] {
        serde_json::from_str::<serde_json::Value>(line)
            .map(|_| ())
            .map_err(|e| format!("JSONL {label}非合法 JSON {}: {e}", p.display()))?;
    }
    Ok(())
}

/// 数据根目录类型的便利别名（避免调用方反复 import）
pub type DataRoot = PathBuf;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gate_semantics() {
        assert!(require_st_stopped(StPresence::Absent).is_ok());
        let e = require_st_stopped(StPresence::External(8000)).unwrap_err();
        assert!(e.0.contains("外部"), "{e}");
        assert!(e.0.contains("手动关闭"), "{e}");
        let e = require_st_stopped(StPresence::Managed(8000)).unwrap_err();
        assert!(e.0.contains("托管"), "{e}");
    }

    #[test]
    fn is_listening_off_port_is_absent() {
        // 未监听端口 → Absent（不依赖外部服务）
        assert_eq!(StPresence::probe(1, false), StPresence::Absent);
        assert_eq!(StPresence::probe(1, true), StPresence::Absent);
    }

    #[test]
    fn rollback_check_passes_wellformed() {
        let tmp = std::env::temp_dir().join(format!("stchat-gate-ok-{}", std::process::id()));
        let du = tmp.join("default-user");
        std::fs::create_dir_all(du.join("characters")).unwrap();
        std::fs::create_dir_all(du.join("chats/char1")).unwrap();
        std::fs::create_dir_all(du.join("worlds")).unwrap();
        std::fs::write(
            du.join("characters/a.png"),
            [0x89u8, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0x00],
        )
        .unwrap();
        // charx：zip 魔数 PK\x03\x04 开头即视为可读（快检不做完整解包）
        std::fs::write(du.join("characters/b.charx"), [b'P', b'K', 0x03, 0x04, 0xFF])
            .unwrap();
        std::fs::write(du.join("settings.json"), "{\"username\":\"老板\"}").unwrap();
        std::fs::write(
            du.join("chats/char1/x.jsonl"),
            "{\"chat_metadata\":{}}\n{\"mes\":\"hi\"}\n",
        )
        .unwrap();
        std::fs::write(du.join("worlds/w.json"), "{\"entries\":{}}").unwrap();

        let r = pre_rollback_check(&tmp);
        assert!(r.ok, "应通过：{:?}", r.failures);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn rollback_check_fails_on_corrupt_jsonl() {
        let tmp = std::env::temp_dir().join(format!("stchat-gate-bad-{}", std::process::id()));
        let du = tmp.join("default-user");
        std::fs::create_dir_all(du.join("chats/char1")).unwrap();
        std::fs::write(
            du.join("chats/char1/x.jsonl"),
            "not-json\n{\"mes\":\"hi\"}\n",
        )
        .unwrap();

        let r = pre_rollback_check(&tmp);
        assert!(!r.ok, "损坏 JSONL 必须拒绝回滚");
        assert!(
            r.failures.iter().any(|f| f.contains("首行")),
            "{:?}",
            r.failures
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn rollback_check_fails_on_corrupt_png() {
        let tmp = std::env::temp_dir().join(format!("stchat-gate-png-{}", std::process::id()));
        let du = tmp.join("default-user");
        std::fs::create_dir_all(du.join("characters")).unwrap();
        std::fs::write(du.join("characters/a.png"), b"NOTPNG!!").unwrap();

        let r = pre_rollback_check(&tmp);
        assert!(!r.ok, "损坏 PNG 必须拒绝回滚");
        assert!(
            r.failures.iter().any(|f| f.contains("PNG 魔数")),
            "{:?}",
            r.failures
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn rollback_check_fails_on_corrupt_charx() {
        let tmp = std::env::temp_dir().join(format!("stchat-gate-charx-{}", std::process::id()));
        let du = tmp.join("default-user");
        std::fs::create_dir_all(du.join("characters")).unwrap();
        std::fs::write(du.join("characters/a.charx"), b"NOTZIP!").unwrap();

        let r = pre_rollback_check(&tmp);
        assert!(!r.ok, "损坏 charx 必须拒绝回滚");
        assert!(
            r.failures.iter().any(|f| f.contains("charx zip 魔数")),
            "{:?}",
            r.failures
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn rollback_check_fails_on_corrupt_settings() {
        let tmp = std::env::temp_dir().join(format!("stchat-gate-setj-{}", std::process::id()));
        let du = tmp.join("default-user");
        std::fs::create_dir_all(&du).unwrap();
        std::fs::write(du.join("settings.json"), "{not-json").unwrap();

        let r = pre_rollback_check(&tmp);
        assert!(!r.ok, "不可解析的 settings.json 必须拒绝回滚");
        assert!(
            r.failures.iter().any(|f| f.contains("JSON 解析失败")),
            "{:?}",
            r.failures
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
