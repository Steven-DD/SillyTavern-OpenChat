//! 插件宿主（「万物皆可插件」的落地骨架）
//!
//! ── 设计要点 ──
//! * **管理面统一**：一份清单 + 一套生命周期 + 一个插件页，`tier` 是唯一的等级开关
//!   （`required` = 必要插件，`optional` = 可选插件）。
//! * **执行面分派**：`kind` 标识执行模型（process / resource / frontend / st-extension），
//!   宿主只统一「发现—探测—绑定—启停—依赖求解」，具体怎么执行由各模型自己决定。
//! * **宿主是 Rust**：所以能引导 node 这类插件而无需先有 JS 运行时
//!   （VS Code 宿主是 Node，只能把内置插件硬编码进二进制 —— 我们不学它）。
//!
//! ── 绑定模型（本片重点）──
//! 同一个插件可以有三种满足方式，`source` 决定用哪种：
//!   `auto`    宿主探测（系统 PATH → 常见安装位置 → 目录候选）——**默认，复用用户已有环境，零下载**
//!   `manual`  用户指定路径（优先于 auto）
//!   `managed` 宿主安装到插件目录（下载安装属后续能力，当前如实标注未提供）
//!
//! **auto 解析结果不落盘**，每次启动重新探测 —— 这样用户升级/换路径后能自愈，
//! 也满足「每次启动 revalidate」的要求。只有 `manual` / `managed` 需要持久化。
//!
//! ── 必要插件的铁律（吸取 VS Code 内置插件「禁用后仍有残留注册」的教训）──
//! 1. 必要插件也走同一套生命周期（**不硬编码**）
//! 2. 不能「卸载到缺失」——UI 只给修复 / 换绑定
//! 3. 停用要能预告影响（`required_by` 字段供前端做警告）
//! 4. 依赖级联校验：`requires` 对所有绑定来源一视同仁
//! 5. 降级态可观测：`capabilities` 直接按「用户能做什么」报告，而不是列插件状态

pub mod builtin;
pub mod probe;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use probe::ProbeOutcome;

use crate::st_data;

/* ================================================================== *
 * 清单（不可变规格）
 * ================================================================== */

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionFrom {
    pub file: String,
    pub json_path: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProbeSpec {
    /// 可执行文件名（process 型走 PATH 查找）
    #[serde(default)]
    pub commands: Vec<String>,
    #[serde(default)]
    pub version_args: Vec<String>,
    #[serde(default)]
    pub version_pattern: Option<String>,
    /// 最低版本要求（等价于 `>=`）
    #[serde(default)]
    pub min_version: Option<String>,
    /// 常见安装位置（支持 %VAR% 展开）
    #[serde(default)]
    pub search_paths: Vec<String>,
    /// 目录型插件的必需文件（非空即视为目录型）
    #[serde(default)]
    pub dir_markers: Vec<String>,
    /// 目录名匹配模式（如 `SillyTavern*`，大小写不敏感、支持尾部 `*`）。
    /// 用于「安装根目录邻近 + 用户常见位置」的兜底扫描——只扫固定浅层，
    /// 不做递归遍历（隐私考量：不翻用户文件，只认名字）。
    #[serde(default)]
    pub dir_name_patterns: Vec<String>,
    /// 目录型插件从哪里读版本
    #[serde(default)]
    pub version_from: Option<VersionFrom>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EntrySpec {
    #[serde(default)]
    pub exec_names: Vec<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default)]
    pub health: Option<String>,
    #[serde(default)]
    pub health_timeout_ms: Option<u64>,
    /// `default` = 用插件目录内的 data（当前行为）；后续可切外部目录
    #[serde(default)]
    pub data_root_mode: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    /// required | optional —— 唯一的等级开关
    pub tier: String,
    /// process | resource | frontend | st-extension —— 执行模型
    pub kind: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub provides: Vec<String>,
    /// 依赖：插件 id → 版本约束
    #[serde(default)]
    pub requires: HashMap<String, String>,
    #[serde(default)]
    pub probe: ProbeSpec,
    #[serde(default)]
    pub entry: EntrySpec,
    /// 下载源（后续能力的规格，当前只透传给前端展示）
    #[serde(default)]
    pub install: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilitySpec {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub requires: Vec<String>,
    #[serde(default)]
    pub hint: String,
}

/* ================================================================== *
 * 绑定与状态（可变，落盘）
 * ================================================================== */

fn default_source() -> String {
    "auto".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Binding {
    /// auto | manual | managed
    #[serde(default = "default_source")]
    pub source: String,
    #[serde(default)]
    pub path: Option<String>,
}

impl Default for Binding {
    fn default() -> Self {
        Self {
            source: default_source(),
            path: None,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PluginState {
    #[serde(default)]
    pub bindings: HashMap<String, Binding>,
    #[serde(default)]
    pub disabled: Vec<String>,
    /// 覆盖清单里的默认端口
    #[serde(default)]
    pub port_override: Option<u16>,
    /// ST 数据根（null = 用 ST 默认的 <st_dir>/data；设置后由 `--dataRoot` 传入）
    #[serde(default)]
    pub data_root: Option<String>,
    /// ST 配置文件（null = 用 ST 默认的 <st_dir>/config.yaml）
    #[serde(default)]
    pub config_path: Option<String>,
    /// auto 探测的**结果缓存**（插件 id → 上次命中路径）。
    /// 不是绑定：每次使用前仍验 marker，失效自动回退全链重扫并更新。
    /// 升级/挪目录后自愈能力不受影响 —— 这是它与 managed 绑定的本质区别。
    #[serde(default)]
    pub auto_cache: HashMap<String, String>,
    #[serde(rename = "_readme", default = "readme")]
    pub readme: String,
}

fn readme() -> String {
    "插件状态：bindings 绑定（auto=自动探测 / manual=指定路径 / managed=宿主安装）、\
     disabled=已停用插件、port_override=覆盖 ST 端口、\
     data_root/config_path=ST 数据与配置位置（首次运行自动绑定到应用数据目录 st-runtime，\
     配置由 ST 首启自动生成）、auto_cache=自动探测结果缓存（仅加速，失效自动重扫）。\
     改动后重启应用生效；\
     也可用环境变量 ST_CHAT_ST_DIR / ST_CHAT_NODE / ST_CHAT_ST_PORT / ST_CHAT_DATA_DIR 覆盖。"
        .to_string()
}

/* ================================================================== *
 * 报告（给前端）
 * ================================================================== */

#[derive(Debug, Clone, Serialize)]
pub struct DepStatus {
    pub id: String,
    pub ok: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PluginReport {
    pub id: String,
    pub name: String,
    pub tier: String,
    pub kind: String,
    pub description: String,
    pub enabled: bool,
    pub source: String,
    pub path: Option<String>,
    pub version: Option<String>,
    pub min_version: Option<String>,
    /// ready | missing | mismatch | disabled
    pub status: String,
    pub message: String,
    /// 依赖本插件的能力名（停用时用来预告影响）
    pub required_by: Vec<String>,
    pub dependencies: Vec<DepStatus>,
    /// 清单里是否带下载源
    pub downloadable: bool,
    /// 宿主安装（managed）是否已支持 —— 当前为 false，UI 据此如实标注
    pub managed_supported: bool,
    pub provides: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CapabilityReport {
    pub id: String,
    pub name: String,
    pub ok: bool,
    pub missing: Vec<String>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct StackReport {
    pub plugins: Vec<PluginReport>,
    pub capabilities: Vec<CapabilityReport>,
    /// 已解析的执行路径（供诊断展示与 sidecar 使用）
    pub node_path: Option<String>,
    pub st_dir: Option<String>,
    pub data_root: Option<String>,
    pub st_port: u16,
    pub git_path: Option<String>,
    /// 数据区现状（是否已外移、体积、关键数据清单）
    pub data: Option<crate::st_data::DataStatus>,
}

/// sidecar 真正需要的东西
#[derive(Debug, Clone)]
pub struct ResolvedStack {
    pub node: PathBuf,
    pub st_dir: PathBuf,
    pub data_root: PathBuf,
    /// ST 配置文件路径（外移后指向用户数据目录）
    pub config_path: PathBuf,
    pub st_port: u16,
}

/* ================================================================== *
 * 宿主
 * ================================================================== */

pub struct PluginHost {
    manifests: Vec<Manifest>,
    capabilities: Vec<CapabilitySpec>,
    state_path: PathBuf,
    state: Mutex<PluginState>,
    /// 应用配置目录（数据区外移的目标根，见 `st_data`）
    pub config_dir: PathBuf,
}

/// 宿主安装（managed）自 S4 起可用：安装到版本化目录后绑定版本目录
/// （node 绑到其中的可执行文件，ST 绑到目录本身）。
const MANAGED_SUPPORTED: bool = true;

impl PluginHost {
    pub fn new(config_dir: &Path) -> Self {
        let (manifests, capabilities) = builtin::load(config_dir);
        let state_path = config_dir.join("plugins.json");
        // 首次运行（文件不存在）也落一份：`_readme` 里写了绑定语义与环境变量，
        // 用户想手工调路径或排查问题时至少知道文件在哪。
        let fresh = !state_path.exists();
        let state = Self::load_state(&state_path, config_dir);

        let host = Self {
            manifests,
            capabilities,
            state_path,
            state: Mutex::new(state),
            config_dir: config_dir.to_path_buf(),
        };

        if fresh {
            host.save();
        }
        host.autobind_data_root_if_needed();

        println!(
            "[plugin] 载入 {} 个插件清单（{} 个必要 / {} 个可选）",
            host.manifests.len(),
            host.manifests.iter().filter(|m| m.tier == "required").count(),
            host.manifests.iter().filter(|m| m.tier != "required").count()
        );
        host
    }

    /// 数据目录默认化（老板定的形态：**不等用户迁移，直接生成一份默认配置**）。
    ///
    /// 首次运行（未绑定过数据位置且无环境变量覆盖）→ 直接绑到应用数据目录
    /// `<app_config_dir>/st-runtime`，ST 首启会在那里自动生成 config.yaml 与默认数据
    /// （已实测：`config.yaml not found → Creating a new one`）。
    /// 用户从此不需要理解「迁移」这个概念；设置页只展示位置/大小 + 导入导出。
    fn autobind_data_root_if_needed(&self) {
        if std::env::var("ST_CHAT_DATA_DIR")
            .ok()
            .is_some_and(|v| !v.trim().is_empty())
        {
            return; // 环境变量优先级最高，保持覆盖语义
        }
        let already = self
            .state
            .lock()
            .ok()
            .and_then(|s| s.data_root.clone())
            .is_some();
        if already {
            return;
        }
        // ⚠ 口径统一：绑 `st-runtime\data`，与迁移引擎/状态展示一致（原来绑的是 st-runtime，差一层）
        let root = st_data::external_data_root(&self.config_dir);
        let cfg = st_data::external_config_path(&self.config_dir);

        // ⚠ 必须先把目录建出来：data_paths() 只在 is_dir() 为真时才采纳绑定。
        // 目录不存在 → 静默回退 ST 默认 → sidecar 不传 --dataRoot → 数据写进代码目录
        // （升级 ST 时会被一起带走）。这个自杀式循环就是从这里掐断的。
        if let Err(e) = std::fs::create_dir_all(&root) {
            eprintln!(
                "[plugin] 创建数据目录失败 {}：{e}",
                st_data::display_path(&root)
            );
            return;
        }

        if let Ok(mut s) = self.state.lock() {
            s.data_root = Some(root.to_string_lossy().into_owned());
            s.config_path = Some(cfg.to_string_lossy().into_owned());
            drop(s);
            self.save();
            println!(
                "[plugin] 数据目录默认绑定到应用数据目录（自动生成默认配置）：{}",
                st_data::display_path(&root)
            );
        }
    }

    /// 清除数据区绑定，回退到 ST 默认位置。
    ///
    /// 只在「自动同步失败」时用：此时外移目录是空的，若仍按绑定启动，
    /// ST 会以空数据起来 —— 用户会以为数据丢了（最危险的失败模式）。
    /// 清掉绑定后 ST 回到 `<ST目录>/data`，用户原有的数据还在那儿。
    pub fn clear_data_binding(&self) {
        if let Ok(mut s) = self.state.lock() {
            s.data_root = None;
            s.config_path = None;
            drop(s);
            self.save();
            println!("[plugin] 已清除数据区绑定，回退到 ST 默认位置");
        }
    }

    fn load_state(path: &Path, config_dir: &Path) -> PluginState {
        if let Ok(text) = std::fs::read_to_string(path) {
            match serde_json::from_str::<PluginState>(&text) {
                Ok(s) => return s,
                Err(e) => eprintln!("[plugin] plugins.json 解析失败，改用默认: {e}"),
            }
        }
        Self::migrate_legacy(config_dir)
    }

    /// 从旧版 `sidecar.json` 迁移绑定，避免升级后突然「找不到 ST」
    fn migrate_legacy(config_dir: &Path) -> PluginState {
        let mut st = PluginState {
            readme: readme(),
            ..Default::default()
        };
        let old = config_dir.join("sidecar.json");
        let Ok(text) = std::fs::read_to_string(&old) else {
            return st;
        };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
            return st;
        };

        if let Some(p) = v.get("node_path").and_then(|x| x.as_str()) {
            if !p.is_empty() && !p.eq_ignore_ascii_case("node") && Path::new(p).is_file() {
                st.bindings.insert(
                    "node".into(),
                    Binding {
                        source: "manual".into(),
                        path: Some(p.to_string()),
                    },
                );
            }
        }
        if let Some(p) = v.get("st_dir").and_then(|x| x.as_str()) {
            if !p.is_empty() && Path::new(p).join("server.js").is_file() {
                st.bindings.insert(
                    "sillytavern".into(),
                    Binding {
                        source: "manual".into(),
                        path: Some(p.to_string()),
                    },
                );
            }
        }
        if let Some(p) = v.get("st_port").and_then(|x| x.as_u64()) {
            st.port_override = Some(p as u16);
        }
        if !st.bindings.is_empty() || st.port_override.is_some() {
            println!(
                "[plugin] 已从 sidecar.json 迁移 {} 项绑定 + 端口 {:?}",
                st.bindings.len(),
                st.port_override
            );
        }
        st
    }

    pub fn save(&self) {
        let snapshot = match self.state.lock() {
            Ok(s) => s.clone(),
            Err(_) => return,
        };
        if let Some(dir) = self.state_path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        match serde_json::to_string_pretty(&snapshot) {
            Ok(s) => {
                // 原子写：写一半崩溃会让 plugins.json 截断、绑定信息回默认态
                if let Err(e) = st_data::atomic_write(&self.state_path, s.as_bytes()) {
                    eprintln!("[plugin] 写入 {} 失败: {e}", self.state_path.display());
                }
            }
            Err(e) => eprintln!("[plugin] 序列化状态失败: {e}"),
        }
    }

    pub fn manifest(&self, id: &str) -> Option<&Manifest> {
        self.manifests.iter().find(|m| m.id == id)
    }

    fn binding_of(&self, id: &str) -> Binding {
        let env_override = match id {
            "node" => std::env::var("ST_CHAT_NODE").ok(),
            "sillytavern" => std::env::var("ST_CHAT_ST_DIR").ok(),
            _ => None,
        };
        if let Some(p) = env_override.filter(|p| !p.trim().is_empty()) {
            return Binding {
                source: "manual".into(),
                path: Some(p),
            };
        }
        self.state
            .lock()
            .ok()
            .and_then(|s| s.bindings.get(id).cloned())
            .unwrap_or_default()
    }

    /// auto 缓存读写。`path=None` 表示清除。只在内容实际变化时落盘，
    /// 避免 reports() 每次调用都重写 plugins.json。
    fn set_auto_cache(&self, id: &str, path: Option<String>) {
        let changed = {
            let Ok(mut s) = self.state.lock() else {
                return;
            };
            let old = s.auto_cache.get(id).cloned();
            let new = path.filter(|p| !p.is_empty());
            if old == new {
                false
            } else {
                match new {
                    Some(p) => {
                        s.auto_cache.insert(id.to_string(), p);
                    }
                    None => {
                        s.auto_cache.remove(id);
                    }
                }
                true
            }
        };
        if changed {
            self.save();
            println!("[plugin] auto 探测缓存已更新：{id}");
        }
    }

    fn probe_of(&self, m: &Manifest) -> ProbeOutcome {
        let b = self.binding_of(&m.id);
        match b.source.as_str() {
            // manual 与 managed 走同一条探测链路：都拿一个显式路径去校验
            // （managed 的路径 = 宿主安装的版本目录，node 则是其可执行文件）
            "manual" | "managed" => {
                let p = b.path.as_ref().map(PathBuf::from);
                probe::probe(m, p.as_deref(), None, None)
            }
            // auto：缓存路径优先快查（省重复扫描），失效回退全链（含 install_root 兜底）
            _ => {
                let root = crate::installer::install_root(&self.config_dir);
                let cached = self
                    .state
                    .lock()
                    .ok()
                    .and_then(|s| s.auto_cache.get(&m.id).cloned())
                    .map(PathBuf::from);
                let out = probe::probe(m, None, Some(&root), cached.as_deref());
                // 缓存维护：命中且满足 → 记下（变了才写）；彻底未找到 → 清掉，
                // 让下次直接跑全链（缓存留着也只会白 stat 一次）
                let cached_str = cached.as_ref().map(|pb| pb.to_string_lossy().into_owned());
                if out.found && out.satisfies {
                    let p = out.path.clone();
                    if p != cached_str {
                        self.set_auto_cache(&m.id, p);
                    }
                } else if !out.found && cached.is_some() {
                    self.set_auto_cache(&m.id, None);
                }
                out
            }
        }
    }

    /* ---------------- 报告 ---------------- */

    pub fn reports(&self) -> Vec<PluginReport> {
        let state = self.state.lock().map(|s| s.clone()).unwrap_or_default();
        let probes: HashMap<String, ProbeOutcome> = self
            .manifests
            .iter()
            .map(|m| (m.id.clone(), self.probe_of(m)))
            .collect();

        self.manifests
            .iter()
            .map(|m| {
                let p = probes
                    .get(&m.id)
                    .cloned()
                    .unwrap_or_else(|| ProbeOutcome::missing("未探测"));
                let enabled = !state.disabled.contains(&m.id);
                let b = self.binding_of(&m.id);
                let status = if !enabled {
                    "disabled"
                } else if !p.found {
                    "missing"
                } else if !p.satisfies {
                    "mismatch"
                } else {
                    "ready"
                };

                // 依赖校验：约束对所有绑定来源一视同仁
                let mut dependencies = Vec::new();
                let mut dep_keys: Vec<&String> = m.requires.keys().collect();
                dep_keys.sort();
                for dep_id in dep_keys {
                    let constraint = &m.requires[dep_id];
                    let dp = probes.get(dep_id.as_str());
                    let (ok, message) = match dp {
                        None => (false, format!("未知插件 {dep_id}")),
                        Some(o) if !o.found => (false, format!("{dep_id} 未就绪")),
                        Some(o) if !o.satisfies => (false, format!("{dep_id} 不满足 {constraint}")),
                        Some(o) => match o.version.as_ref() {
                            Some(v) if !probe::satisfies(v, constraint) => (
                                false,
                                format!("{dep_id} {v} 不满足要求 {constraint}"),
                            ),
                            Some(v) => (true, format!("{dep_id} {v}")),
                            None => (true, format!("{dep_id} 已就绪")),
                        },
                    };
                    dependencies.push(DepStatus {
                        id: dep_id.clone(),
                        ok,
                        message,
                    });
                }

                let required_by: Vec<String> = self
                    .capabilities
                    .iter()
                    .filter(|c| c.requires.contains(&m.id))
                    .map(|c| c.name.clone())
                    .collect();

                PluginReport {
                    id: m.id.clone(),
                    name: m.name.clone(),
                    tier: m.tier.clone(),
                    kind: m.kind.clone(),
                    description: m.description.clone(),
                    enabled,
                    source: b.source.clone(),
                    path: p.path.clone(),
                    version: p.version.clone(),
                    min_version: m.probe.min_version.clone(),
                    status: status.to_string(),
                    message: p.message.clone(),
                    required_by,
                    dependencies,
                    downloadable: m.install.is_some(),
                    managed_supported: MANAGED_SUPPORTED,
                    provides: m.provides.clone(),
                }
            })
            .collect()
    }

    /// 按「能力」聚合 —— 用户关心能不能聊天，不关心 node 装没装
    ///
    /// 单独抽出 `_of(&reps)` 是为了让 `stack()` 复用同一份探测结果：
    /// 每次探测都要起子进程（约几十毫秒），不能在同一个请求里跑两遍。
    fn capabilities_of(&self, reps: &[PluginReport]) -> Vec<CapabilityReport> {
        let map: HashMap<&str, &PluginReport> =
            reps.iter().map(|r| (r.id.as_str(), r)).collect();

        self.capabilities
            .iter()
            .map(|c| {
                let mut missing: Vec<String> = c
                    .requires
                    .iter()
                    .filter(|id| {
                        map.get(id.as_str())
                            .map(|r| r.status != "ready")
                            .unwrap_or(true)
                    })
                    .cloned()
                    .collect();
                missing.sort();
                let ok = missing.is_empty();
                let message = if ok {
                    "可用".to_string()
                } else if c.hint.is_empty() {
                    format!("缺少插件：{}", missing.join("、"))
                } else {
                    format!("{}（缺少 {}）", c.hint, missing.join("、"))
                };
                CapabilityReport {
                    id: c.id.clone(),
                    name: c.name.clone(),
                    ok,
                    missing,
                    message,
                }
            })
            .collect()
    }

    pub fn capabilities(&self) -> Vec<CapabilityReport> {
        let reps = self.reports();
        self.capabilities_of(&reps)
    }

    pub fn stack(&self) -> StackReport {
        let reps = self.reports();
        let caps = self.capabilities_of(&reps);
        let get = |id: &str| reps.iter().find(|r| r.id == id);
        let st_dir = get("sillytavern").and_then(|r| r.path.clone());
        let port = self
            .state
            .lock()
            .ok()
            .and_then(|s| s.port_override)
            .or_else(|| self.manifest("sillytavern").and_then(|m| m.entry.port))
            .unwrap_or(8000);
        // 数据路径走与 sidecar 完全相同的裁决点 —— 否则会出现「界面显示 A、实际跑 B」
        let (data_root, data) = match st_dir.as_ref().map(PathBuf::from) {
            Some(p) => {
                let (root, cfg) = self.data_paths(&p);
                let s = st_data::status(&p, &self.config_dir, &root, &cfg, port);
                (Some(st_data::display_path(&root)), Some(s))
            }
            None => (None, None),
        };

        StackReport {
            node_path: get("node").and_then(|r| r.path.clone()),
            git_path: get("git").and_then(|r| r.path.clone()),
            st_dir,
            data_root,
            st_port: port,
            capabilities: caps,
            plugins: reps,
            data,
        }
    }

    /* ---------------- sidecar 用 ---------------- */

    pub fn resolve(&self) -> Result<ResolvedStack, String> {
        let st_m = self
            .manifest("sillytavern")
            .ok_or_else(|| "缺少 sillytavern 插件清单".to_string())?;
        let st_p = self.probe_of(st_m);
        let st_dir = st_p
            .path
            .clone()
            .map(PathBuf::from)
            .filter(|_| st_p.found && st_p.satisfies)
            // 探测 message 本身已是完整结论（如「未找到 SillyTavern」），直接透传
            .ok_or_else(|| st_p.message.clone())?;

        // node 未就绪时回落到 PATH 上的 node —— 保持改造前的行为，不打断现有可用环境；
        // 真实性由 capabilities/status 报告负责，不在这里假装满足。
        let node = self
            .manifest("node")
            .map(|m| self.probe_of(m))
            .and_then(|p| p.path)
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("node"));

        let st_port = self
            .state
            .lock()
            .ok()
            .and_then(|s| s.port_override)
            .or_else(|| std::env::var("ST_CHAT_ST_PORT").ok().and_then(|v| v.parse().ok()))
            .or(st_m.entry.port)
            .unwrap_or(8000);

        // 数据路径：由宿主统一裁决（默认 = ST 目录内；外移后 = 应用数据目录）
        let (data_root, config_path) = self.data_paths(&st_dir);

        Ok(ResolvedStack {
            node,
            st_dir,
            data_root,
            config_path,
            st_port,
        })
    }

    /// **数据与配置路径的唯一裁决点。**
    ///
    /// 规则（按优先级）：
    ///   1. 环境变量 `ST_CHAT_DATA_DIR` —— 排障/临时覆盖用
    ///   2. `plugins.json` 的 `data_root` 绑定（迁移后由应用写入）
    ///   3. ST 默认位置 `<st_dir>/data`
    ///
    /// ⚠ **绑定失效必须回退，不能"指向空目录"**：
    /// 如果绑定的目录不存在（用户手工改了配置、或误删了数据目录），
    /// 直接把它当成 dataRoot 会让 ST 以空数据启动 —— 用户会以为数据丢了。
    /// 这里主动回退到默认位置并打日志，把"看不到数据"变成"能看到但位置不对"。
    pub fn data_paths(&self, st_dir: &Path) -> (PathBuf, PathBuf) {
        let (bound_root, bound_cfg) = self
            .state
            .lock()
            .map(|s| (s.data_root.clone(), s.config_path.clone()))
            .unwrap_or((None, None));

        let env_root = std::env::var("ST_CHAT_DATA_DIR")
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty());

        let data_root = match env_root.or(bound_root) {
            Some(p) => {
                let pb = PathBuf::from(&p);
                if pb.is_dir() {
                    pb
                } else {
                    eprintln!(
                        "[plugin] 数据目录绑定失效（{p} 不存在），已回退到 ST 默认位置；\
                         数据可能仍在原位置，请在「设置 → 数据目录」确认"
                    );
                    st_data::default_data_root(st_dir)
                }
            }
            None => st_data::default_data_root(st_dir),
        };

        // configPath 与 dataRoot 独立：外移后两者都应指向用户数据目录。
        // 若只绑了 dataRoot 没绑 configPath，仍用 ST 目录内的（保持可预测，不擅自推断）。
        let config_path = match bound_cfg {
            Some(p) if !p.trim().is_empty() => PathBuf::from(p),
            _ => st_data::default_config_path(st_dir),
        };

        (data_root, config_path)
    }

    /* ---------------- 数据区（S2） ---------------- */

    /// 数据区现状（设置页数据源）
    pub fn data_status(&self) -> Result<st_data::DataStatus, String> {
        let st_dir = self
            .manifest("sillytavern")
            .map(|m| self.probe_of(m))
            .and_then(|p| p.path)
            .map(PathBuf::from)
            .ok_or_else(|| "SillyTavern 未就绪，无法读取数据目录".to_string())?;
        let (root, cfg) = self.data_paths(&st_dir);
        let port = self
            .state
            .lock()
            .ok()
            .and_then(|s| s.port_override)
            .or_else(|| std::env::var("ST_CHAT_ST_PORT").ok().and_then(|v| v.parse().ok()))
            .or_else(|| self.manifest("sillytavern").and_then(|m| m.entry.port))
            .unwrap_or(8000);
        Ok(st_data::status(&st_dir, &self.config_dir, &root, &cfg, port))
    }

    /// 迁移计划（只读预览）
    pub fn data_plan(&self) -> Result<st_data::MigrationPlan, String> {
        let (st_dir, port) = self.st_dir_and_port()?;
        Ok(st_data::plan(&st_dir, &self.config_dir, port))
    }

    /// 执行迁移。**不删源目录**；成功后由本方法切换绑定。
    pub fn data_migrate(
        &self,
        on_progress: &mut dyn FnMut(st_data::MigrationProgress),
    ) -> Result<st_data::MigrationResult, String> {
        let (st_dir, port) = self.st_dir_and_port()?;
        let res = st_data::execute(&st_dir, &self.config_dir, port, on_progress)?;

        // 校验已通过才切换绑定 —— 顺序不能反
        let mut s = self.state.lock().map_err(|_| "状态锁失效".to_string())?;
        s.data_root = Some(res.target.clone());
        s.config_path = Some(res.config_target.clone());
        drop(s);
        self.save();
        Ok(res)
    }

    /// 回滚到 ST 目录内的默认位置。
    /// 因为迁移从不删除源数据，回滚只是改绑定，**零数据风险**。
    pub fn data_rollback(&self) -> Result<(), String> {
        let (st_dir, _) = self.st_dir_and_port()?;
        if !st_data::default_data_root(&st_dir).is_dir() {
            return Err(format!(
                "原位置已不存在，无法回滚：{}",
                st_data::default_data_root(&st_dir).display()
            ));
        }
        let mut s = self.state.lock().map_err(|_| "状态锁失效".to_string())?;
        s.data_root = None;
        s.config_path = None;
        drop(s);
        self.save();
        Ok(())
    }

    fn st_dir_and_port(&self) -> Result<(PathBuf, u16), String> {
        let m = self
            .manifest("sillytavern")
            .ok_or_else(|| "缺少 sillytavern 插件清单".to_string())?;
        let p = self.probe_of(m);
        let st_dir = p
            .path
            .map(PathBuf::from)
            .filter(|_| p.found && p.satisfies)
            .ok_or_else(|| p.message.clone())?;
        let port = self
            .state
            .lock()
            .ok()
            .and_then(|s| s.port_override)
            .or_else(|| std::env::var("ST_CHAT_ST_PORT").ok().and_then(|v| v.parse().ok()))
            .or(m.entry.port)
            .unwrap_or(8000);
        Ok((st_dir, port))
    }

    /* ---------------- 变更 ---------------- */

    pub fn set_binding(&self, id: &str, source: &str, path: Option<String>) -> Result<(), String> {
        if self.manifest(id).is_none() {
            return Err(format!("未知插件：{id}"));
        }
        match source {
            "auto" => {}
            "manual" => {
                let p = path
                    .as_ref()
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| "手动绑定必须提供路径".to_string())?;
                if !Path::new(p).exists() {
                    return Err(format!("路径不存在：{p}"));
                }
            }
            "managed" => {
                if !MANAGED_SUPPORTED {
                    return Err("宿主安装（managed）尚未提供，请先用「自动探测」或「指定路径」".into());
                }
                // managed 绑定必须带版本目录路径（node 绑其中的可执行文件）
                let p = path
                    .as_ref()
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| "宿主安装绑定必须提供版本目录路径".to_string())?;
                if !Path::new(p).exists() {
                    return Err(format!("版本目录不存在：{p}"));
                }
            }
            other => return Err(format!("未知绑定来源：{other}")),
        }

        let mut s = self.state.lock().map_err(|_| "状态锁失效".to_string())?;
        s.bindings.insert(
            id.to_string(),
            Binding {
                source: source.to_string(),
                path: path.filter(|p| !p.trim().is_empty()),
            },
        );
        drop(s);
        self.save();
        Ok(())
    }

    /// 显式绑定的原始路径（manual/managed）；auto 探测不落盘，返回 None。
    /// 供宿主安装区判断「哪个版本是当前绑定」。
    pub fn binding_path_of(&self, id: &str) -> Option<String> {
        let b = self.binding_of(id);
        if b.source == "manual" || b.source == "managed" {
            b.path
        } else {
            None
        }
    }

    pub fn set_enabled(&self, id: &str, enabled: bool) -> Result<(), String> {
        let m = self.manifest(id).ok_or_else(|| format!("未知插件：{id}"))?;
        // 系统插件（tier=required）是 App 运行前提，任何入口都不得停用
        if !enabled && m.tier == "required" {
            return Err(format!("「{}」是系统插件，不可停用", m.name));
        }
        let mut s = self.state.lock().map_err(|_| "状态锁失效".to_string())?;
        s.disabled.retain(|x| x != id);
        if !enabled {
            s.disabled.push(id.to_string());
        }
        drop(s);
        self.save();
        Ok(())
    }

    pub fn set_port(&self, port: Option<u16>) -> Result<(), String> {
        if let Some(p) = port {
            if p < 1024 {
                return Err("端口需 ≥ 1024".into());
            }
        }
        let mut s = self.state.lock().map_err(|_| "状态锁失效".to_string())?;
        s.port_override = port;
        drop(s);
        self.save();
        Ok(())
    }

    /// 强制重新探测并返回完整快照（探测本来就是每次实时执行，
    /// 这个入口只是给 UI 的「重新检测」按钮一个明确的语义）
    pub fn rescan(&self) -> StackReport {
        self.stack()
    }
}
