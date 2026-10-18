//! manifest 闸门（批18 · task-6 工作项 4）
#![allow(linker_messages)]
//! 契约单一生成源：docs/agents/endpoint-manifest.md。本测试解析其契约表
//!（方法/路径），与**路由表快照**（硬编码，注释段注明与
//! src-tauri/src/backend/api.rs 的同步责任）逐条对照：
//! - 已路由 → 计入已覆盖（照快照逐条断言匹配规则生效）；
//! - 缺席（含 elevenlabs，由 dataapi-fix 同批落地）→ 打印「跳过清单」，不红；
//! - 红线 = manifest 必到位族（native 已实装面）在快照中缺席，或解析器
//!   自身空转（manifest 行数 / 快照规则数为零）—— 门必须真有牙。
//!
//! 快照同步责任（维护注释）：本文件的 `ROUTE_SNAPSHOT` 是
//! `src-tauri/src/backend/api.rs` route() match 臂的字面抄录（运行时刻口径
//! 11 条 exact GET/POST + 3 静态 GET 前缀 + POST 前缀两条：/api/tokenizers/*/encode
//! 与 /api/translate/<provider>）。**api.rs 路由增删时必须同步更新此处**；
//! 闸门另以 REQUIRED_SET 断言必到位族，snapshot 与 api.rs 的字面一致性由
//! 本注释 + code review 承担（snapshot 无法机械证明 api.rs 现状，登记为
//! 已知限制）。
//!
//! 另附 IPC 面闸门：IPC_COMMANDS（Tauri 命令名集合）与 src-tauri/src/lib.rs
//! 的 invoke_handler 注册面逐名对照（读源码文本），防两侧漂移。

/// 契约基线为**本地维护**文件（`docs/` 按仓库约定不入库）：
/// CI / 新克隆缺席时返回 None，闸门跳过并打印提示，而不是编译期失败。
fn manifest_text() -> Option<String> {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("docs")
        .join("agents")
        .join("endpoint-manifest.md");
    std::fs::read_to_string(&p).ok()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Rule {
    Exact(&'static str),
    Prefix(&'static str),
    /// POST /api/tokenizers/<…>/encode（api.rs 结尾/前缀双条件臂）
    TokenizersEncode,
    /// POST /api/translate/<provider>（api.rs 前缀臂）
    TranslatePrefix,
}

use Rule::{Exact, Prefix};

/// 快照：运行时刻 api.rs 的分发面（同步责任见文首）
const ROUTE_SNAPSHOT: &[(&str, Rule)] = &[
    // infra（#76/#77）
    ("GET", Exact("/csrf-token")),
    ("POST", Exact("/api/ping")),
    // settings（#28/#29）
    ("POST", Exact("/api/settings/get")),
    ("POST", Exact("/api/settings/save")),
    // characters（#1-#8）
    ("POST", Exact("/api/characters/all")),
    ("POST", Exact("/api/characters/get")),
    ("POST", Exact("/api/characters/chats")),
    ("POST", Exact("/api/characters/delete")),
    ("POST", Exact("/api/characters/rename")),
    ("POST", Exact("/api/characters/create")),
    ("POST", Exact("/api/characters/edit")),
    ("POST", Exact("/api/characters/import")),
    ("POST", Exact("/api/characters/export")),
    // chats（#9-#18）
    ("POST", Exact("/api/chats/save")),
    ("POST", Exact("/api/chats/get")),
    ("POST", Exact("/api/chats/rename")),
    ("POST", Exact("/api/chats/delete")),
    ("POST", Exact("/api/chats/search")),
    ("POST", Exact("/api/chats/recent")),
    ("POST", Exact("/api/chats/group/get")),
    ("POST", Exact("/api/chats/group/info")),
    ("POST", Exact("/api/chats/group/delete")),
    ("POST", Exact("/api/chats/group/save")),
    // worldinfo（#19-#23#26）
    ("POST", Exact("/api/worldinfo/list")),
    ("POST", Exact("/api/worldinfo/get")),
    ("POST", Exact("/api/worldinfo/edit")),
    ("POST", Exact("/api/worldinfo/delete")),
    ("POST", Exact("/api/worldinfo/import")),
    // groups（#19 族）
    ("POST", Exact("/api/groups/all")),
    ("POST", Exact("/api/groups/create")),
    ("POST", Exact("/api/groups/edit")),
    ("POST", Exact("/api/groups/delete")),
    // 资产/静态（#30-#33/#35-#40）
    ("GET", Exact("/thumbnail")),
    ("POST", Exact("/api/avatars/get")),
    ("POST", Exact("/api/avatars/upload")),
    ("POST", Exact("/api/avatars/delete")),
    ("POST", Exact("/api/backgrounds/all")),
    ("POST", Exact("/api/images/upload")),
    ("GET", Exact("/api/sprites/get")),
    ("GET", Prefix("/characters/")),
    ("GET", Prefix("/backgrounds/")),
    ("GET", Prefix("/user-images/")),
    // tokenizers + secrets（#50-#53）
    ("POST", Exact("/api/tokenizers/openai/count")),
    ("POST", Rule::TokenizersEncode),
    ("POST", Exact("/api/secrets/write")),
    ("POST", Exact("/api/secrets/read")),
    ("POST", Exact("/api/secrets/view")),
    // extras / data-maid / speech / vector（#54-#64）+ #66 classify（R4 本地实装）
    ("POST", Exact("/api/data-maid/report")),
    ("POST", Exact("/api/data-maid/finalize")),
    ("POST", Exact("/api/data-maid/delete")),
    ("GET", Exact("/api/data-maid/view")),
    ("POST", Exact("/api/extra/classify")),
    ("POST", Exact("/api/translate/onering")),
    ("POST", Rule::TranslatePrefix),
    ("POST", Exact("/api/speech/synthesize")),
    ("POST", Exact("/api/speech/elevenlabs/synthesize")),
    ("POST", Exact("/api/openai/generate-voice")),
    ("POST", Exact("/api/vector/list")),
    ("POST", Exact("/api/vector/insert")),
    ("POST", Exact("/api/vector/query")),
    ("POST", Exact("/api/vector/delete")),
    // providers 生成通道（#41/#42/#48/#49）
    ("POST", Exact("/api/backends/chat-completions/status")),
    ("POST", Exact("/api/backends/chat-completions/generate")),
    ("POST", Exact("/api/backends/text-completions/status")),
    ("POST", Exact("/api/backends/text-completions/generate")),
    // horde（#44-#47）
    ("POST", Exact("/api/horde/generate-text")),
    ("POST", Exact("/api/horde/task-status")),
    ("POST", Exact("/api/horde/cancel-task")),
    ("POST", Exact("/api/horde/text-models")),
    // novelai（#43）
    ("POST", Exact("/api/novelai/generate")),
    // extensions 族（#67-#75）
    ("GET", Exact("/api/extensions/discover")),
    ("POST", Exact("/api/quick-replies/save")),
    ("POST", Exact("/api/extensions/install")),
    ("POST", Exact("/api/extensions/update")),
    ("POST", Exact("/api/extensions/version")),
    ("POST", Exact("/api/extensions/branches")),
    ("POST", Exact("/api/extensions/switch")),
    ("POST", Exact("/api/extensions/move")),
    ("POST", Exact("/api/extensions/delete")),
];

fn route_match(method: &str, path: &str) -> bool {
    let path = path.split('?').next().unwrap_or(path);
    ROUTE_SNAPSHOT.iter().any(|(m, rule)| {
        *m == method
            && match rule {
                Rule::Exact(p) => *p == path,
                Rule::Prefix(p) => path.starts_with(p),
                Rule::TokenizersEncode => {
                    path.starts_with("/api/tokenizers/") && path.ends_with("/encode")
                }
                Rule::TranslatePrefix => path == "/api/translate/onering" || path.starts_with("/api/translate/"),
            }
    })
}

/// 解析 manifest 契约表：`| # | 方法 | 路径 | …` 行 → (序号, method, path)
/// （cell 内 `\|` 转义先还原；非 GET/POST 行（Tauri IPC 命令、说明性表格）自动跳过）
fn parse_manifest_rows(text: &str) -> Vec<(usize, String, String)> {
    let mut rows = Vec::new();
    for line in text.lines() {
        if !line.trim_start().starts_with('|') {
            continue;
        }
        let cleaned = line.replace("\\|", "¦"); // \| 是表内竖线转义（如 /thumbnail?...avatar¦persona¦bg）
        let cells: Vec<&str> = cleaned.split('|').collect();
        // cells: ["", "#", 方法, 路径, ...]
        if cells.len() < 4 {
            continue;
        }
        let idx: usize = match cells[1].trim().parse() {
            Ok(n) => n,
            Err(_) => continue, // 表头/分隔行/杂项表（原语表/§9 说明行）
        };
        // 契约表路径/方法列带 ` 反引号 —— 剥离后再判
        let method = cells[2].trim().trim_matches('`').trim();
        let path = cells[3].trim().trim_matches('`').trim();
        if (method == "GET" || method == "POST") && path.starts_with('/') {
            rows.push((idx, method.to_string(), path.to_string()));
        }
    }
    rows
}

/// 必到位族（native 已实装面；不在表内 → 红）。以 manifest 注解的
/// 已实装 families 为准 —— 不含：#34 /scripts/extensions（ST 静态映射，
/// 未迁移）。#66 extra/classify 已本地实装（R4 词典近似，backend/classify.rs）；
/// #57 elevenlabs 已落地（批17）。
const REQUIRED_SET: &[(&str, &str)] = &[
    ("POST", "/api/characters/all"),
    ("POST", "/api/characters/get"),
    ("POST", "/api/characters/chats"),
    ("POST", "/api/characters/delete"),
    ("POST", "/api/characters/rename"),
    ("POST", "/api/characters/create"),
    ("POST", "/api/characters/edit"),
    ("POST", "/api/characters/import"),
    ("POST", "/api/characters/export"),
    ("POST", "/api/chats/get"),
    ("POST", "/api/chats/save"),
    ("POST", "/api/chats/delete"),
    ("POST", "/api/chats/rename"),
    ("POST", "/api/chats/recent"),
    ("POST", "/api/chats/search"),
    ("POST", "/api/chats/group/get"),
    ("POST", "/api/chats/group/save"),
    ("POST", "/api/chats/group/delete"),
    ("POST", "/api/chats/group/info"),
    ("POST", "/api/groups/all"),
    ("POST", "/api/groups/create"),
    ("POST", "/api/groups/edit"),
    ("POST", "/api/groups/delete"),
    ("POST", "/api/worldinfo/list"),
    ("POST", "/api/worldinfo/get"),
    ("POST", "/api/worldinfo/edit"),
    ("POST", "/api/worldinfo/import"),
    ("POST", "/api/worldinfo/delete"),
    ("POST", "/api/settings/get"),
    ("POST", "/api/settings/save"),
    ("GET", "/thumbnail"),
    ("GET", "/characters/fixture"),
    ("GET", "/backgrounds/fixture"),
    ("GET", "/user-images/fixture"),
    ("POST", "/api/avatars/get"),
    ("POST", "/api/avatars/upload"),
    ("POST", "/api/avatars/delete"),
    ("POST", "/api/backgrounds/all"),
    ("POST", "/api/images/upload"),
    ("GET", "/api/sprites/get"),
    ("POST", "/api/backends/chat-completions/generate"),
    ("POST", "/api/backends/chat-completions/status"),
    ("POST", "/api/novelai/generate"),
    ("POST", "/api/horde/generate-text"),
    ("POST", "/api/horde/task-status"),
    ("POST", "/api/horde/cancel-task"),
    ("POST", "/api/horde/text-models"),
    ("POST", "/api/backends/text-completions/generate"),
    ("POST", "/api/backends/text-completions/status"),
    ("POST", "/api/secrets/write"),
    ("POST", "/api/tokenizers/openai/count"),
    ("POST", "/api/tokenizers/llama3/encode"),
    ("POST", "/api/tokenizers/nerdstash_v2/encode"),
    ("POST", "/api/translate/onering"),
    ("POST", "/api/speech/synthesize"),
    ("POST", "/api/speech/elevenlabs/synthesize"),
    ("POST", "/api/openai/generate-voice"),
    ("POST", "/api/vector/list"),
    ("POST", "/api/vector/insert"),
    ("POST", "/api/vector/query"),
    ("POST", "/api/vector/delete"),
    ("POST", "/api/data-maid/report"),
    ("POST", "/api/data-maid/finalize"),
    ("POST", "/api/data-maid/delete"),
    ("GET", "/api/data-maid/view"),
    ("POST", "/api/extra/classify"),
    ("POST", "/api/quick-replies/save"),
    ("GET", "/api/extensions/discover"),
    ("POST", "/api/extensions/install"),
    ("POST", "/api/extensions/update"),
    ("POST", "/api/extensions/version"),
    ("POST", "/api/extensions/branches"),
    ("POST", "/api/extensions/switch"),
    ("POST", "/api/extensions/move"),
    ("POST", "/api/extensions/delete"),
    ("GET", "/csrf-token"),
    ("POST", "/api/ping"),
];

/// Tauri IPC 命令集合（与 src-tauri/src/lib.rs 的 invoke_handler 注册面
/// 逐名对照；manifest §8 #78-#81 + 基础设施 backend_base）
const IPC_COMMANDS: &[&str] = &[
    "backend_base",
    "chat_mutate_lines",
    "chat_export_file",
    "character_export_png",
    "st_data_export",
    "st_data_import",
];

/// 读 lib.rs 源码文本，抽出 invoke_handler(tauri::generate_handler![...]) 内的
/// 命令名集合。文件缺席（非本仓布局）→ None，闸门跳过而非误红。
fn ipc_commands_in_lib_rs() -> Option<Vec<String>> {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("lib.rs");
    let text = std::fs::read_to_string(p).ok()?;
    let block_start = text.find("invoke_handler(tauri::generate_handler![")?;
    let rest = &text[block_start..];
    let block_end = rest.find("])")?;
    let inner = rest[..block_end].split("generate_handler![").nth(1)?;
    Some(
        inner
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect(),
    )
}

/* ------------------------------------------------------------------ *
 * 闸门本体
 * ------------------------------------------------------------------ */

#[test]
fn manifest_gate_routes_parsed_and_covered() {
    // 快照自检（红线 3）不依赖契约文件，先跑，保证门在无基线时也有牙
    assert!(ROUTE_SNAPSHOT.len() >= 70, "路由快照规则异常稀少");
    let mut uniq: Vec<(&str, &Rule)> = ROUTE_SNAPSHOT.iter().map(|(m, r)| (*m, r)).collect();
    let before = uniq.len();
    uniq.sort_by(|a, b| (a.0, format!("{:?}", a.1)).cmp(&(b.0, format!("{:?}", b.1))));
    uniq.dedup_by(|a, b| a.0 == b.0 && format!("{:?}", a.1) == format!("{:?}", b.1));
    assert_eq!(uniq.len(), before, "快照存在重复规则（同步抄写错误）");

    // 必到位族（红线 1）同样只依赖快照
    let missing: Vec<&(&str, &str)> = REQUIRED_SET
        .iter()
        .filter(|(m, p)| !route_match(m, p))
        .collect();
    assert!(
        missing.is_empty(),
        "必到位端点从路由快照缺席（红线）：{:?}",
        missing.iter().map(|(m, p)| format!("{m} {p}")).collect::<Vec<_>>()
    );

    let Some(text) = manifest_text() else {
        eprintln!(
            "[manifest_gate] 跳过契约对照：docs/agents/endpoint-manifest.md 不在本地\
             （docs/ 不入库，CI/新克隆必然缺席）；快照红线已执行"
        );
        return;
    };
    let rows = parse_manifest_rows(&text);
    assert!(
        rows.len() >= 70,
        "manifest 契约行解析过少（解析 {}/预期 77）：解析器空转或契约文件损坏",
        rows.len()
    );

    let mut covered: Vec<String> = Vec::new();
    let mut skipped: Vec<(usize, String)> = Vec::new();
    for (idx, m, p) in &rows {
        if route_match(m, p) {
            covered.push(format!("#{idx} {m} {p}"));
        } else {
            skipped.push((*idx, format!("{m} {p}")));
        }
    }

    // 跳过清单（不红）：缺席端点在此列出，落地方承诺后由本表对照回收
    let report_skip: Vec<String> = skipped
        .iter()
        .map(|(i, e)| format!("  - #{i} {e}（未路由 → 跳过清单）"))
        .collect();
    eprintln!(
        "[manifest_gate] 已路由 {}/{}；跳过清单 {} 项：\n{}",
        covered.len(),
        rows.len(),
        skipped.len(),
        report_skip.join("\n")
    );

    // 红线 2：跳过清单应只含「已知未迁移面」，若出现正文级覆盖缺口（如 #1-#29）
    // 说明路由退化 —— 凡是 manifest 数据 CRUD/生成通道族的关键端点都不允许在 skip 里
    let critical_hint = [
        "/api/settings/",
        "/api/chats/",
        "/api/characters/",
        "/api/groups/",
        "/api/worldinfo/",
        "/api/backends/",
        "/api/secrets/",
        "/api/speech/synthesize",
    ];
    for (idx, entry) in &skipped {
        for h in critical_hint {
            assert!(
                !entry.contains(h),
                "manifest #{idx} {entry} 属关键族却在跳过清单（路由退化红线）"
            );
        }
    }

    eprintln!(
        "[manifest_gate] 必到位族 {} 条全部命中快照；快照 receipt 规则 {} 条",
        REQUIRED_SET.len(),
        ROUTE_SNAPSHOT.len()
    );
}

/// 比较 lib.rs 实注册面与闸门清单：None = 一致，Some(差异描述) = 漂移。
/// 抽为纯函数，便于负向用例（R18 门禁②）在无文件注入下证明「门有牙」。
fn ipc_set_diff(found: &[String], want: &[&str]) -> Option<String> {
    let mut got = found.to_vec();
    got.sort();
    let mut w: Vec<String> = want.iter().map(|s| s.to_string()).collect();
    w.sort();
    if got == w {
        None
    } else {
        Some(format!("lib.rs invoke_handler={got:?} vs 闸门清单={w:?}"))
    }
}

/// IPC 面闸门：闸门清单 IPC_COMMANDS 与 lib.rs 的 invoke_handler 注册面一致。
/// lib.rs 不在本地时跳过（不误红）。
#[test]
fn ipc_commands_match_lib_rs_invoke_handler() {
    let Some(found) = ipc_commands_in_lib_rs() else {
        eprintln!("[manifest_gate] 跳过 IPC 对照：src/lib.rs 不在本地");
        return;
    };
    assert!(found.len() >= 5, "IPC 注册面异常稀少：{found:?}");
    if let Some(diff) = ipc_set_diff(&found, IPC_COMMANDS) {
        panic!("IPC 注册面与闸门清单不一致（lib.rs invoke_handler 已漂移）：{diff}");
    }
}

/// 负向用例（R18 门禁②）：合成漂移清单必须被判为不一致 —— 证明 IPC 断言真有牙。
/// 纯内存合成，不依赖 lib.rs / manifest 在场（CI/新克隆同样执行）。
#[test]
fn ipc_gate_flags_synthetic_drift() {
    let real: Vec<String> = IPC_COMMANDS.iter().map(|s| s.to_string()).collect();
    // 正向：与闸门清单自身一致 → 不得误报
    assert!(ipc_set_diff(&real, IPC_COMMANDS).is_none(), "同集合被误判为漂移");
    // 负向 1：注册面多出一条幽灵命令（lib.rs 侧漂移）→ 必须报红
    let mut extra = real.clone();
    extra.push("ghost_command".to_string());
    assert!(ipc_set_diff(&extra, IPC_COMMANDS).is_some(), "幽灵命令未被检出");
    // 负向 2：注册面少一条（漏注册）→ 必须报红
    let mut dropped = real.clone();
    dropped.pop();
    assert!(ipc_set_diff(&dropped, IPC_COMMANDS).is_some(), "缺失命令未被检出");
}

/// manifest 解析器口径单测：`\|` 转义还原、方法过滤（IPC 命令行不误入）、
/// 序号列非数字的说明性行不误入。契约基线不在本地时跳过（docs/ 不入库）。
#[test]
fn manifest_parser_boundary_rows() {
    let Some(text) = manifest_text() else {
        eprintln!("[manifest_gate] 跳过解析器边界单测：契约基线不在本地（docs/ 不入库）");
        return;
    };
    let rows = parse_manifest_rows(&text);
    // /thumbnail 行（含 `avatar\|persona\|bg` 转义）应还原为单行并切出 /thumbnail
    let t = rows.iter().find(|(i, _, _)| *i == 30).expect("缺 #30 行");
    assert_eq!(t.1, "GET");
    assert!(
        t.2.starts_with("/thumbnail?"),
        "#30 路径解析应为 /thumbnail?…（含转义还原），实际 {t:?}"
    );
    // Tauri IPC（#78-81）不是 HTTP 行：不在解析结果
    assert!(!rows.iter().any(|(i, _, _)| (78..=81).contains(i)));
    // 明确移出域（§9 说明表）不入契约表：解析结果只含 GET/POST，且两类都有
    assert!(rows.iter().any(|(_, m, _)| m == "GET"));
    assert!(rows.iter().any(|(_, m, _)| m == "POST"));
    assert!(rows.iter().any(|(i, _, _)| *i == 77), "ping 行应解析到");
    assert!(rows.iter().any(|(i, _, _)| *i == 76), "csrf-token 行应解析到");
}
