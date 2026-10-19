//! 全局：MSVC linker 本地化 stdout 警告抑制（仓库测试文件公约）
#![allow(linker_messages)]
//! T10 黄金样本 roundtrip 测试（批18 · task-6 工作项 2）
//!
//! 起临时 data_root 的 native backend（backend::start 随机端口 + 256-bit token，
//! 鉴权/客户端形态抄 src-tauri/src/backend/mod.rs 既有测试），对三层
//! 数据面做「fixture → save → 落盘/读取 → 与 fixture 对照」：
//!
//! 1. settings：save（整体覆盖）→ get → 格式串解析 → 与 fixture 逐键零丢失
//!    （200 未知键 + 中文 + 嵌套结构），另验落盘 settings.json 同等保真；
//! 2. chats：JSONL save → get 行级一致 + 落盘文件逐行回读一致（extra/swipes
//!    与未知字段全保真）；
//! 3. worldinfo：edit roundtrip（嵌套 entries / timedEffects / 未知字段保真）。
//!
//! 对照口径：反序列化后的 serde_json::Value 深相等（键序无关 —— 落盘为
//! 4 空格 pretty，且 serde_json 收敛键序，字节级比对不成立亦不必要；
//! 「零丢失」以全键集合 + 每键值相等为准）。不出网。

use sillytavern_openchat_lib::backend;
use std::path::PathBuf;

const SETTINGS_FIXTURE: &str = include_str!("fixtures/settings_golden.json");
const CHAT_FIXTURE: &str = include_str!("fixtures/chat_golden.jsonl");
const WORLDINFO_FIXTURE: &str = include_str!("fixtures/worldinfo_golden.json");
const CHARACTER_FIXTURE: &str = include_str!("fixtures/character_v2_golden.json");
const SSE_FIXTURE: &str = include_str!("fixtures/sse_frames_golden.txt");

/* ------------------------------------------------------------------ *
 * 共享启动件（与 manifest_gate.rs 各自内联同款，属测试 crate 独立
 * 编译单元 —— 不经 src/ 的任何改动）
 * ------------------------------------------------------------------ */

struct BackendEnv {
    base: String,
    token: String,
    root: PathBuf,
    /// 临时数据根守卫：用例结束自动删树（_tmp 最后 drop）
    _tmp: tempfile::TempDir,
}

async fn spawn_backend(label: &str) -> BackendEnv {
    let _tmp = tempfile::Builder::new()
        .prefix(&format!("stoc-golden-{label}-"))
        .tempdir()
        .expect("创建临时目录失败");
    let root = _tmp.path().to_path_buf();
    let ep = backend::start(root.clone()).await.expect("backend 启动失败");
    BackendEnv {
        base: format!("http://127.0.0.1:{}", ep.port),
        token: ep.token,
        root,
        _tmp,
    }
}

fn fresh_client() -> reqwest::Client {
    reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("reqwest client 构建失败")
}

/* ------------------------------------------------------------------ *
 * 1. settings：save → get 逐键零丢失 + 落盘保真（manifest #28/#29）
 * ------------------------------------------------------------------ */

#[tokio::test]
async fn settings_roundtrip_save_get_zero_loss() {
    let env = spawn_backend("settings").await;
    let c = fresh_client();
    let auth = ("x-relay-auth", env.token.as_str());
    let fixture: serde_json::Value = serde_json::from_str(SETTINGS_FIXTURE).unwrap();

    // save：body 即 settings 本体（整体覆盖）
    let r = c
        .post(format!("{}/api/settings/save", env.base))
        .header(auth.0, auth.1)
        .json(&fixture)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 200, "settings/save 应成功");
    let v: serde_json::Value = r.json().await.unwrap();
    assert_eq!(v["result"], "ok", "save 响应: {v}");

    // get：{settings: <文件原文字符串>}
    let r = c
        .post(format!("{}/api/settings/get", env.base))
        .header(auth.0, auth.1)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 200);
    let v: serde_json::Value = r.json().await.unwrap();
    let raw = v["settings"].as_str().expect("settings 应为原文字符串");
    let served: serde_json::Value = serde_json::from_str(raw).expect("get 返回应为合法 JSON");

    // 逐键零丢失：全集相等 + 每键值相等（深相等即等价于二者的同时成立）
    assert_eq!(
        served, fixture,
        "settings get 全量与 fixture 逐键零丢失，实测差异见上方 Debug 输出"
    );
    // 显式点名断言（评审 A3 的「读-改-写」链路原子面中 App 真正消费的键）
    assert_eq!(served["username"], "夜航船长", "中文值保真");
    assert!(served["extension_settings"]["stoc_quick_replies"].is_array());
    for i in 0..200 {
        assert!(
            !served[format!("unknown_key_{i}")].is_null(),
            "未知键 unknown_key_{i} 丢失"
        );
    }

    // 落盘文件回读一致：data_root/default-user/settings.json 与 fixture 同等保真
    let disk = env
        .root
        .join("default-user")
        .join("settings.json");
    let disk_text = std::fs::read_to_string(&disk).expect("settings.json 应已落盘");
    let disk_v: serde_json::Value = serde_json::from_str(&disk_text).unwrap();
    assert_eq!(disk_v, fixture, "落盘 settings.json 与 fixture 保真");
}

/* ------------------------------------------------------------------ *
 * 2. chats JSONL：save → get 行级一致 + 落盘逐行回读一致（manifest #9/#10）
 * ------------------------------------------------------------------ */

fn fixture_chat_lines() -> Vec<serde_json::Value> {
    CHAT_FIXTURE
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).expect("fixture chat 行应为合法 JSON"))
        .collect()
}

#[tokio::test]
async fn chats_jsonl_line_fidelity_on_disk_and_get() {
    let env = spawn_backend("chats").await;
    let c = fresh_client();
    let auth = ("x-relay-auth", env.token.as_str());
    let fixture_lines = fixture_chat_lines();
    assert_eq!(fixture_lines.len(), 4, "黄金样本应为 1 元数据头 + 3 消息行");

    let avatar = "fixture-巡夜卡";
    let file = "fixture-chat";

    // save（manifest #10 入参形态：chat 数组 + integrity 首行）
    let r = c
        .post(format!("{}/api/chats/save", env.base))
        .header(auth.0, auth.1)
        .header("x-csrf-token", auth.1)
        .json(&serde_json::json!({
            "avatar_url": avatar,
            "file_name": file,
            "chat": fixture_lines,
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 200, "chats/save 应成功");
    let v: serde_json::Value = r.json().await.unwrap();
    assert_eq!(v["ok"], true, "save 响应: {v}");

    // get（manifest #9）：解析数组，逐行深相等
    let r = c
        .post(format!("{}/api/chats/get", env.base))
        .header(auth.0, auth.1)
        .json(&serde_json::json!({ "avatar_url": avatar, "file_name": file }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 200);
    let got: Vec<serde_json::Value> = r
        .json()
        .await
        .expect("chats/get 应返回 JSONL 解析数组");
    assert_eq!(got.len(), fixture_lines.len(), "行数应完全一致");
    for (i, (a, b)) in got.iter().zip(fixture_lines.iter()).enumerate() {
        assert_eq!(a, b, "chats/get 第 {i} 行与 fixture 不一致");
    }

    // 落盘文件回读一致（default-user/chats/<avatar>/<file>.jsonl）
    let disk = env
        .root
        .join("default-user")
        .join("chats")
        .join(avatar)
        .join(format!("{file}.jsonl"));
    let disk_text = std::fs::read_to_string(&disk).expect("会话文件应已落盘");
    let disk_lines: Vec<serde_json::Value> = disk_text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).expect("落盘行应为合法 JSON"))
        .collect();
    assert_eq!(disk_lines.len(), fixture_lines.len());
    for (i, (a, b)) in disk_lines.iter().zip(fixture_lines.iter()).enumerate() {
        assert_eq!(a, b, "落盘第 {i} 行与 fixture 不一致");
    }

    // 保真点抽验：swipes / extra 嵌套 / 中文 / 未知顶层键
    let third = &got[2];
    assert_eq!(third["swipes"].as_array().unwrap().len(), 2, "swipes 保真");
    assert_eq!(third["swipe_id"], 1);
    assert!(
        third["extra"]["embed_probe"]["nested"].is_array(),
        "extra 嵌套保真"
    );
    assert_eq!(got[0]["chat_metadata"]["integrity"], "stchat-golden-slug-fixture");
    assert_eq!(
        got[3]["extra"]["stchat_unknown"][1]["对象"],
        serde_json::Value::Null,
        "null 保真"
    );
}

/* ------------------------------------------------------------------ *
 * 3. worldinfo：edit roundtrip（嵌套 entries / timedEffects / 未知字段保真）
 * ------------------------------------------------------------------ */

#[tokio::test]
async fn worldinfo_edit_roundtrip_unknown_field_fidelity() {
    let env = spawn_backend("worldinfo").await;
    let c = fresh_client();
    let auth = ("x-relay-auth", env.token.as_str());
    let fixture: serde_json::Value = serde_json::from_str(WORLDINFO_FIXTURE).unwrap();

    // edit（manifest #25：{name, data}；新建即 edit）
    let r = c
        .post(format!("{}/api/worldinfo/edit", env.base))
        .header(auth.0, auth.1)
        .json(&serde_json::json!({
            "name": "fixture-world-中文书",
            "data": fixture,
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 200, "worldinfo/edit 应成功");

    // get（manifest #24）→ 全量深相等（未知字段零丢失）
    let r = c
        .post(format!("{}/api/worldinfo/get", env.base))
        .header(auth.0, auth.1)
        .json(&serde_json::json!({ "name": "fixture-world-中文书" }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 200);
    let got: serde_json::Value = r.json().await.unwrap();
    assert_eq!(got, fixture, "worldinfo roundtrip 与 fixture 保真");

    // 落盘文件回读一致（default-user/worlds/<name>.json，wi_get 读的就是它）
    let disk = env
        .root
        .join("default-user")
        .join("worlds")
        .join("fixture-world-中文书.json");
    let disk_v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&disk).unwrap()).unwrap();
    assert_eq!(disk_v, fixture, "worldinfo 落盘与 fixture 保真");

    // 保真点抽验：嵌套 timedEffects / 未知字段 / 中文
    assert_eq!(got["entries"]["0"]["timedEffects"]["sticky"]["enabled"], true);
    assert_eq!(got["entries"]["0"]["timedEffects"]["sticky"]["duration"], 5);
    assert_eq!(
        got["entries"]["1"]["stchat_unknown_verify"]["typed"][2]["k"],
        "键值二"
    );
    assert_eq!(got["entries"]["1"]["disable"], true);
    assert_eq!(got["stchat_top_unknown"]["顶层未知"]["语言"], "中文");

    // list（manifest #23）应见到该书；顺带验证 CJK 文件名不丢行
    let r = c
        .post(format!("{}/api/worldinfo/list", env.base))
        .header(auth.0, auth.1)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 200);
    let list: Vec<serde_json::Value> = r.json().await.unwrap();
    assert!(
        list.iter()
            .any(|it| it["file_id"] == "fixture-world-中文书" || it["name"] == "fixture-world-中文书"),
        "worldinfo/list 应含黄金样本（返回 {list:?}）"
    );
}

/* ------------------------------------------------------------------ *
 * 4. 黄金样本语料自检（fixture 入库面：保证样本自身即「黄金态」，
 *    防止样本腐化后 roundtrip 对着坏样本变绿）
 * ------------------------------------------------------------------ */

#[test]
fn golden_corpus_self_check() {
    // settings：200+ 未知键 + 中文
    let settings: serde_json::Value = serde_json::from_str(SETTINGS_FIXTURE).unwrap();
    for i in 0..200 {
        assert!(
            settings.get(format!("unknown_key_{i}")).is_some(),
            "settings fixture 缺 unknown_key_{i}"
        );
    }
    assert_eq!(settings["username"], "夜航船长");
    assert!(
        settings["naive_nested_probe"]["深层"]["更深"][1]["保留"].is_null(),
        "嵌套 null 探针缺失"
    );

    // 角色卡 v2：spec 头 + data 全字段 + 两层未知键 + character_book 嵌套
    let card: serde_json::Value = serde_json::from_str(CHARACTER_FIXTURE).unwrap();
    assert_eq!(card["spec"], "chara_card_v2");
    assert_eq!(card["spec_version"], "2.0");
    for k in [
        "name",
        "description",
        "personality",
        "scenario",
        "first_mes",
        "mes_example",
        "creator_notes",
        "system_prompt",
        "post_history_instructions",
        "alternate_greetings",
        "tags",
        "creator",
        "character_version",
        "extensions",
    ] {
        assert!(card["data"].get(k).is_some(), "v2 data 缺字段 {k}");
    }
    assert_eq!(card["data"]["alternate_greetings"].as_array().unwrap().len(), 2);
    assert_eq!(card["alternate_greetings"].as_array().unwrap().len(), 3, "顶层 legacy 面三条开场");
    assert!(card.get("unknown_stchat_key").is_some(), "顶层未知键缺失");
    book_entries_nonempty(&card);
    assert!(
        card["description"].as_str().unwrap().contains('，'),
        "中文标点探针缺失"
    );

    // 会话 JSONL：首行元数据头 + 3 行含 extra/swipes
    let lines = fixture_chat_lines();
    assert!(lines[0]["chat_metadata"].is_object());
    assert!(
        lines[0].get("mes").is_none(),
        "首行是元数据头，不得含 mes"
    );
    for (i, l) in lines[1..4].iter().enumerate() {
        assert!(l["extra"].is_object(), "消息行 {i} 缺 extra");
    }
    assert!(lines[2].get("swipes").is_some(), "swipes 探针缺失");
    // 黄金样本 mes 含中文文本与中文标点（。/？）
    let mes_probe = lines.iter().any(|l| {
        let m = l["mes"].as_str().unwrap_or("");
        m.contains('。') || m.contains('？')
    });
    assert!(mes_probe, "消息文本中文探针缺失");

    // 世界书：嵌套 entries / timed effects / 未知字段
    let wi: serde_json::Value = serde_json::from_str(WORLDINFO_FIXTURE).unwrap();
    assert_eq!(wi["entries"].as_object().unwrap().len(), 2);
    for (_, e) in wi["entries"].as_object().unwrap() {
        assert!(e.get("timedEffects").is_some(), "timedEffects 缺失");
        assert!(e.get("stchat_unknown_verify").is_some(), "未知字段缺失");
        assert!(e.get("key").is_some());
    }

    // SSE 帧序列：含内容帧 / usage 帧 / 错误帧 / [DONE]
    let data_frames: Vec<&str> = SSE_FIXTURE
        .lines()
        .filter(|l| l.starts_with("data:"))
        .collect();
    assert!(data_frames.len() >= 4);
    assert!(
        data_frames.iter().any(|l| l.contains("\"error\"")),
        "SSE 黄金样本应含一帧错误帧"
    );
    assert!(data_frames.iter().any(|l| l.contains("[DONE]")), "缺 [DONE]");
    assert!(
        SSE_FIXTURE.contains("\"usage\""),
        "usage 探针帧缺失（SSE 用量帧）"
    );
}

fn book_entries_nonempty(card: &serde_json::Value) {
    let entries = card
        .pointer("/data/extensions/character_book/entries")
        .and_then(|e| e.as_array())
        .expect("character_book entries 缺失");
    assert!(!entries.is_empty());
    assert!(entries[0]["extensions"].get("entry_ext_unknown").is_some(), "书内未知字段缺失");
}
