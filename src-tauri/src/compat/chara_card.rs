//! compat/chara_card —— 角色卡 Spec V1/V2/V3 归一化（评审 T3 · manifest #2/#8）
//!
//! ST 契约（characters.js:406-425 processCharacter + :450-461 getCharaCardV2
//! + :504-557 readFromV2 + :565-657 charaFormatData，已逐行核对）：
//!
//! 响应字段（前端 data.ts 消费）：
//! - `json_data`：PNG 内**原文字符串**（v2 或 v3 原始 JSON，未归一化）
//! - `avatar`：文件名（`xxx.png`）
//! - `date_added` / `date_last_chat`：**毫秒时间戳**（ctimeMs / mtimeMs）
//! - `create_date`：**ISO 字符串**（缺失时 new Date().toISOString()）
//! - `chat_size`：chats 目录字节和；`data_size`：data 字段字符和
//! - 顶层扁平字段（name/description/personality/...）：v2 从 data.* 提升（readFromV2）
//!
//! 保存路径（/edit 与 /rename）：charaFormatData —— 保留 json_data 里的外键
//! （spec/spec_version/data 未知键），写回 spec=chara_card_v2。

use serde_json::{Map, Value};

#[derive(Debug)]
pub struct CardError(pub String);

impl std::fmt::Display for CardError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl std::error::Error for CardError {}

/// readFromV2 的字段映射（characters.js:513-552）：顶层 ← data.*
const FLAT_FIELDS: [(&str, &str); 8] = [
    ("name", "name"),
    ("description", "description"),
    ("personality", "personality"),
    ("scenario", "scenario"),
    ("first_mes", "first_mes"),
    ("mes_example", "mes_example"),
    ("talkativeness", "extensions.talkativeness"),
    ("fav", "extensions.fav"),
];

fn get_path<'a>(v: &'a Value, path: &str) -> Option<&'a Value> {
    let mut cur = v;
    for seg in path.split('.') {
        cur = cur.get(seg)?;
    }
    Some(cur)
}

/// 等价 getCharaCardV2（characters.js:450-461）：
/// - `spec` 缺失 → V1 卡：转 V2（convertToV2 简化为“包一层 data”）
/// - 有 spec → readFromV2（顶层字段从 data.* 提升，缺省补 talkativeness/fav）
pub fn to_v2_response(mut card: Value) -> Value {
    let has_spec = card.get("spec").is_some();
    if !has_spec {
        // V1：把已知字段收进 data；未知键留在顶层（保留外键）
        let mut data = Map::new();
        for (flat, v2path) in FLAT_FIELDS {
            let v2path_first = v2path.split('.').next().unwrap();
            if let Some(v) = card.get(flat) {
                data.insert(v2path_first.into(), v.clone());
            }
        }
        card.as_object_mut()
            .map(|o| o.insert("data".into(), Value::Object(data)));
        if card.get("spec").is_none() {
            if let Some(o) = card.as_object_mut() {
                o.insert("spec".into(), "chara_card_v2".into());
                o.insert("spec_version".into(), "2.0".into());
            }
        }
        return card;
    }

    // readFromV2：顶层字段从 data.* 提升
    if let Some(obj) = card.as_object_mut() {
        obj.remove("json_data"); // 防递归保存（ST 同）
        for (flat, v2path) in FLAT_FIELDS {
            match get_path(obj.get("data").unwrap_or(&Value::Null), v2path) {
                Some(v) => {
                    obj.insert(flat.into(), v.clone());
                }
                None => {
                    // 缺省补值（characters.js:532-546）
                    if v2path == "extensions.talkativeness" {
                        obj.insert(flat.into(), serde_json::json!(0.5));
                    } else if v2path == "extensions.fav" {
                        obj.insert(flat.into(), serde_json::json!(false));
                    }
                }
            }
        }
    }
    card
}

/// 保存路径（/edit 的 charaFormatData 精简版）：保留外键，写回 spec=chara_card_v2。
/// 输入是前端 edit 表单（扁平字段 + 可选 world/extensions）；输出是写进 PNG 的 JSON。
pub fn format_for_save(body: &Value) -> Value {
    // ST charaFormatData：以 tryParse(json_data) 为基底，保留顶层与 data 内的未知外键。
    // 卡片内嵌扩展键（regex_scripts / TavernHelper_scripts / 风月 detailed_intro 等）
    // 与 data.character_book 全靠这一步存活 —— 基底恒为空壳时，每次 /edit 都会静默丢掉
    // 它们（前端 saveCard / setCharacterWorld 正是为此专门回传 json_data）。
    let mut char: Map<String, Value> = body
        .get("json_data")
        .and_then(|v| v.as_str())
        .and_then(|s| serde_json::from_str::<Value>(s).ok())
        .and_then(|v| match v {
            Value::Object(o) => Some(o),
            _ => None,
        })
        .unwrap_or_default();
    char.remove("json_data"); // 防递归保存（与 to_v2_response 同款守卫）

    // 保留未知外键（tryParse(json_data) 的语义）
    for k in ["spec", "spec_version"] {
        if let Some(v) = body.get(k) {
            char.insert(k.into(), v.clone());
        }
    }

    let s = |v: Option<&Value>| -> Value {
        match v {
            Some(Value::String(s)) if !s.is_empty() => Value::String(s.clone()),
            _ => Value::String(String::new()),
        }
    };
    let arr_or_empty = |v: Option<&Value>| -> Value {
        match v {
            Some(Value::Array(a)) => Value::Array(a.clone()),
            Some(Value::String(st)) => Value::Array(
                st.split(',')
                    .map(|x| x.trim())
                    .filter(|x| !x.is_empty())
                    .map(|x| Value::String(x.into()))
                    .collect(),
            ),
            _ => Value::Array(Vec::new()),
        }
    };

    char.insert("name".into(), s(body.get("ch_name")));
    char.insert("description".into(), s(body.get("description")));
    char.insert("personality".into(), s(body.get("personality")));
    char.insert("scenario".into(), s(body.get("scenario")));
    char.insert("first_mes".into(), s(body.get("first_mes")));
    char.insert("mes_example".into(), s(body.get("mes_example")));
    char.insert("creatorcomment".into(), s(body.get("creator_notes")));
    char.insert("avatar".into(), "none".into());
    char.insert(
        "talkativeness".into(),
        match body.get("talkativeness") {
            Some(Value::Number(n)) => Value::Number(n.clone()),
            _ => serde_json::json!(0.5),
        },
    );
    char.insert(
        "fav".into(),
        match body.get("fav") {
            Some(Value::Bool(b)) => Value::Bool(*b),
            Some(Value::String(st)) => Value::Bool(st == "true"),
            _ => Value::Bool(false),
        },
    );
    char.insert("tags".into(), arr_or_empty(body.get("tags")));
    char.insert("spec".into(), "chara_card_v2".into());
    char.insert("spec_version".into(), "2.0".into());

    // data 镜像（charaFormatData 的 Spec V2 段）：在基底 data 上覆盖已知字段，
    // 未知键（character_book 等）原样保留
    let mut data = char
        .get("data")
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default();
    data.insert("name".into(), s(body.get("ch_name")));
    data.insert("description".into(), s(body.get("description")));
    data.insert("personality".into(), s(body.get("personality")));
    data.insert("scenario".into(), s(body.get("scenario")));
    data.insert("first_mes".into(), s(body.get("first_mes")));
    data.insert("mes_example".into(), s(body.get("mes_example")));
    data.insert("creator_notes".into(), s(body.get("creator_notes")));
    data.insert("system_prompt".into(), s(body.get("system_prompt")));
    data.insert(
        "post_history_instructions".into(),
        s(body.get("post_history_instructions")),
    );
    data.insert("tags".into(), arr_or_empty(body.get("tags")));
    data.insert("creator".into(), s(body.get("creator")));
    data.insert("character_version".into(), s(body.get("character_version")));
    data.insert(
        "alternate_greetings".into(),
        match body.get("alternate_greetings") {
            Some(Value::Array(a)) => Value::Array(a.clone()),
            Some(Value::String(st)) => Value::Array(vec![Value::String(st.clone())]),
            _ => Value::Array(Vec::new()),
        },
    );
    // extensions 合并而非整体替换：卡内嵌正则（regex_scripts）与脚本库
    // （TavernHelper_scripts）等扩展键必须存活，只覆盖下面四个已知键
    let mut ext = data
        .get("extensions")
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default();
    ext.insert("talkativeness".into(), char["talkativeness"].clone());
    ext.insert("fav".into(), char["fav"].clone());
    ext.insert("world".into(), s(body.get("world")));
    // depth_prompt（characters.js:619-626）
    let depth = match body.get("depth_prompt_depth") {
        Some(Value::Number(n)) => Value::Number(n.clone()),
        _ => serde_json::json!(4),
    };
    let role = match body.get("depth_prompt_role") {
        Some(Value::String(r)) => Value::String(r.clone()),
        _ => Value::String("system".into()),
    };
    ext.insert(
        "depth_prompt".into(),
        serde_json::json!({
            "prompt": s(body.get("depth_prompt_prompt")),
            "depth": depth,
            "role": role,
        }),
    );
    data.insert("extensions".into(), Value::Object(ext));
    char.insert("data".into(), Value::Object(data));

    // chat / create_date 原样透传（ST 同：edit 不清空、rename 改 name）
    for k in ["chat", "create_date"] {
        if let Some(v) = body.get(k) {
            char.insert(k.into(), v.clone());
        }
    }
    Value::Object(char)
}

/// calculateDataSize（characters.js:361-363）：data 各字段字符长度和
pub fn calculate_data_size(data: &Value) -> u64 {
    match data {
        Value::Object(o) => o
            .values()
            .map(|v| match v {
                Value::String(s) => s.len() as u64,
                _ => v.to_string().len() as u64,
            })
            .sum(),
        _ => 0,
    }
}

/// 前端 edit 入参的合法性（characters.js:1108-1112）
pub fn valid_edit_name(body: &Value) -> bool {
    match body.get("ch_name") {
        Some(Value::String(s)) => !s.is_empty() && s != ".",
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v2_card_flattens_fields() {
        let card = serde_json::json!({
            "spec": "chara_card_v2",
            "name": "old-name",
            "data": {
                "name": "林晚棠",
                "description": "desc",
                "extensions": { "talkativeness": 0.8, "world": "myworld" },
            },
            "json_data": "should-be-unset",
        });
        let out = to_v2_response(card);
        assert_eq!(out["name"], "林晚棠");
        assert_eq!(out["description"], "desc");
        assert_eq!(out["talkativeness"], 0.8);
        assert!(out.get("json_data").is_none(), "json_data 必须被 unset");
    }

    #[test]
    fn v2_missing_extensions_get_defaults() {
        let card = serde_json::json!({ "spec": "chara_card_v2", "data": { "name": "x" } });
        let out = to_v2_response(card);
        assert_eq!(out["talkativeness"], 0.5, "缺省 talkativeness=0.5");
        assert_eq!(out["fav"], false, "缺省 fav=false");
    }

    #[test]
    fn v1_card_wrapped_to_v2() {
        let card = serde_json::json!({ "name": "v1char", "description": "d", "unknown_key": 1 });
        let out = to_v2_response(card);
        assert_eq!(out["spec"], "chara_card_v2");
        assert_eq!(out["data"]["name"], "v1char");
        assert_eq!(out["unknown_key"], 1, "未知外键保留");
    }

    #[test]
    fn format_for_save_keeps_foreign_keys() {
        let body = serde_json::json!({
            "ch_name": "新卡",
            "description": "d",
            "tags": "a, b ,c",
            "fav": "true",
            "unknown_field": "keep",
        });
        let out = format_for_save(&body);
        assert_eq!(out["spec"], "chara_card_v2");
        assert_eq!(out["data"]["name"], "新卡");
        assert_eq!(
            out["data"]["tags"],
            serde_json::json!(["a", "b", "c"]),
            "字符串 tags 拆数组"
        );
        assert_eq!(out["fav"], true, "fav='true' → bool");
        assert_eq!(out["data"]["extensions"]["depth_prompt"]["depth"], 4);
        assert_eq!(out["chat"], Value::Null, "chat 不透传时不存在");
        assert!(
            out.get("unknown_field").is_none(),
            "表单里的未知字段不是卡外键 —— 外键只能经 json_data 基底保留（见下条用例）"
        );
    }

    #[test]
    fn format_for_save_preserves_embedded_extensions_from_json_data() {
        // 回归（2026-10-19 渲染故障根因）：卡片内嵌正则/脚本库在每次 /edit 被静默丢弃。
        // 前端 saveCard / setCharacterWorld 专门回传 json_data 让 charaFormatData 以它为
        // 基底保留未知外键，而实现此前把 json_data 当空气（基底恒为空壳）。
        // 丢掉的正是「[占位标记] → HTML 应用」那条正则，表现为卡片前端界面渲染成裸文本。
        let json_data = serde_json::json!({
            "spec": "chara_card_v2",
            "spec_version": "2.0",
            "name": "写卡助手",
            "foreign_top": { "keep": true },
            "data": {
                "name": "写卡助手",
                "character_book": { "name": "书", "entries": [{ "content": "条目" }] },
                "extensions": {
                    "talkativeness": 0.5,
                    "fav": false,
                    "world": "旧世界",
                    "regex_scripts": [
                        {
                            "scriptName": "萧谴写卡小助手",
                            "findRegex": "\\[萧谴写卡小助手\\]",
                            "replaceString": "<!DOCTYPE html>",
                            "markdownOnly": true,
                        }
                    ],
                    "TavernHelper_scripts": [],
                    "detailed_intro": "风月扩展键"
                }
            }
        })
        .to_string();
        let body = serde_json::json!({
            "ch_name": "写卡助手",
            "description": "改过的描述",
            "json_data": json_data,
            "world": "新世界",
        });

        let out = format_for_save(&body);
        assert_eq!(out["description"], "改过的描述", "表单字段覆盖基底");
        let ext = &out["data"]["extensions"];
        assert_eq!(
            ext["regex_scripts"][0]["findRegex"], "\\[萧谴写卡小助手\\]",
            "卡内嵌正则在 /edit 后必须保留（否则卡片前端界面无法渲染）"
        );
        assert!(ext["TavernHelper_scripts"].is_array(), "脚本库键保留");
        assert_eq!(ext["detailed_intro"], "风月扩展键", "其它未知扩展键保留");
        assert_eq!(ext["world"], "新世界", "表单 world 覆盖基底旧值");
        assert_eq!(
            out["data"]["character_book"]["entries"][0]["content"], "条目",
            "内嵌世界书保留（前端注释依赖 charaFormatData 回嵌 character_book）"
        );
        assert_eq!(out["foreign_top"]["keep"], true, "顶层未知外键保留");
        assert!(
            out.get("json_data").is_none(),
            "json_data 本身不得写回卡内（防递归）"
        );
    }

    #[test]
    fn data_size_counts_chars() {
        let data = serde_json::json!({ "a": "abc", "b": 12 });
        assert_eq!(calculate_data_size(&data), 5);
    }

    #[test]
    fn edit_name_validation() {
        assert!(valid_edit_name(&serde_json::json!({ "ch_name": "x" })));
        assert!(!valid_edit_name(&serde_json::json!({ "ch_name": "" })));
        assert!(!valid_edit_name(&serde_json::json!({ "ch_name": "." })));
        assert!(!valid_edit_name(&serde_json::json!({})));
    }
}
