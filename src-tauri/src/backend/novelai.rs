//! backend/novelai —— NovelAI 文本生成（Phase 2 批5 · manifest #43）。
//!
//! ST `src/endpoints/novelai.js /generate`（:167-298）逐行核对：
//! - 缺 key → 400（裸 sendStatus）
//! - 服务端拼模型默认：badWordsList / logitBiasExp / repPenaltyWhitelist
//!   按模型 hash 选择（clio/kayra、erato、hypebot）
//! - stop_sequences 由 App 前端经 /api/tokenizers/* 编码为 id 后传；
//!   服务端做 default 追加 + 空数组剔除
//! - theme_textadventure prefix → eos_token_id（clio/kayra=49405、erato=29）
//! - URL：kayra/erato → text.novelai.net，其余 api.novelai.net；
//!   streaming → /ai/generate-stream 透传，否则 /ai/generate
//! - 非 ok → 500 {error:{message}}；连接失败 → 200 {error:true}

use super::providers::{json_resp, read_secret};
use super::BackendState;
use axum::http::{HeaderValue, StatusCode};
use axum::response::Response;
use serde_json::{json, Map, Value};

const API_NOVELAI: &str = "https://api.novelai.net";
const TEXT_NOVELAI: &str = "https://text.novelai.net";

// 禁词默认表（novelai.js:20-47）
const BAD_WORDS_CLIO_KAYRA: &[&[i64]] = &[
    &[3],
    &[49356],
    &[1431],
    &[31715],
    &[34387],
    &[20765],
    &[30702],
    &[10691],
    &[49333],
    &[1266],
    &[19438],
    &[43145],
    &[26523],
    &[41471],
    &[2936],
    &[85, 85],
    &[49332],
    &[7286],
    &[1115],
    &[24],
];
const BAD_WORDS_ERATO: &[&[i64]] = &[
    &[16067],
    &[933, 11144],
    &[25106, 11144],
    &[58, 106901, 16073, 33710, 25, 109933],
    &[933, 58, 11144],
    &[128030],
    &[58, 30591, 33503, 17663, 100204, 25, 11144],
];
const BAD_WORDS_HYPEBOT: &[&[i64]] = &[
    &[58],
    &[60],
    &[90],
    &[92],
    &[685],
    &[1391],
    &[1782],
    &[2361],
    &[3693],
    &[4083],
    &[4357],
    &[4895],
    &[5512],
    &[5974],
    &[7131],
    &[8183],
    &[8351],
    &[8762],
    &[8964],
    &[8973],
    &[9063],
    &[11208],
    &[11709],
    &[11907],
    &[11919],
    &[12878],
    &[12962],
    &[13018],
    &[13412],
    &[14631],
    &[14692],
    &[14980],
    &[15090],
    &[15437],
    &[16151],
    &[16410],
    &[16589],
    &[17241],
    &[17414],
    &[17635],
    &[17816],
    &[17912],
    &[18083],
    &[18161],
    &[18477],
    &[19629],
    &[19779],
    &[19953],
    &[20520],
    &[20598],
    &[20662],
    &[20740],
    &[21476],
    &[21737],
    &[22133],
    &[22241],
    &[22345],
    &[22935],
    &[23330],
    &[23785],
    &[23834],
    &[23884],
    &[25295],
    &[25597],
    &[25719],
    &[25787],
    &[25915],
    &[26076],
    &[26358],
    &[26398],
    &[26894],
    &[26933],
    &[27007],
    &[27422],
    &[28013],
    &[29164],
    &[29225],
    &[29342],
    &[29565],
    &[29795],
    &[30072],
    &[30109],
    &[30138],
    &[30866],
    &[31161],
    &[31478],
    &[32092],
    &[32239],
    &[32509],
    &[33116],
    &[33250],
    &[33761],
    &[34171],
    &[34758],
    &[34949],
    &[35944],
    &[36338],
    &[36463],
    &[36563],
    &[36786],
    &[36796],
    &[36937],
    &[37250],
    &[37913],
    &[37981],
    &[38165],
    &[38381],
    &[38430],
    &[38892],
    &[39850],
    &[39893],
    &[41832],
    &[41888],
    &[42535],
    &[42669],
    &[42785],
    &[42924],
    &[43839],
    &[44438],
    &[44587],
    &[44926],
    &[45144],
    &[45297],
    &[46110],
    &[46570],
    &[46581],
    &[46956],
    &[47175],
    &[47182],
    &[47527],
    &[47715],
    &[48600],
    &[48683],
    &[48688],
    &[48874],
    &[48999],
    &[49074],
    &[49082],
    &[49146],
    &[49946],
    &[10221],
    &[4841],
    &[1427],
    &[2602, 834],
    &[29343],
    &[37405],
    &[35780],
    &[2602],
    &[50256],
];

fn bad_words_of(model: &str) -> Vec<Value> {
    let rows: &[&[i64]] = if model.contains("hypebot") {
        BAD_WORDS_HYPEBOT
    } else if model.contains("clio") || model.contains("kayra") {
        BAD_WORDS_CLIO_KAYRA
    } else if model.contains("erato") {
        BAD_WORDS_ERATO
    } else {
        &[]
    };
    rows.iter()
        .map(|ids| Value::Array(ids.iter().map(|i| json!(i)).collect()))
        .collect()
}

fn logit_bias_of(model: &str) -> Vec<Value> {
    // novelai.js:65-73：dinkus/asterism 抑制
    let rows: &[Value] = if model.contains("erato") {
        &[
            json!({ "sequence": [12488], "bias": -0.08, "ensure_sequence_finish": false, "generate_once": false }),
            json!({ "sequence": [128041], "bias": -0.08, "ensure_sequence_finish": false, "generate_once": false }),
        ]
    } else if model.contains("clio") || model.contains("kayra") {
        &[
            json!({ "sequence": [23], "bias": -0.08, "ensure_sequence_finish": false, "generate_once": false }),
            json!({ "sequence": [21], "bias": -0.08, "ensure_sequence_finish": false, "generate_once": false }),
        ]
    } else {
        &[]
    };
    rows.to_vec()
}

/// clio/kayra 白名单（novelai.js:50-55）
const REP_PEN_CLIO_KAYRA: &[&[i64]] = &[&[
    49256, 49264, 49231, 49230, 49287, 85, 49255, 49399, 49262, 336, 333, 432, 363, 468, 492, 745,
    401, 426, 623, 794, 1096, 2919, 2072, 7379, 1259, 2110, 620, 526, 487, 16562, 603, 805, 761,
    2681, 942, 8917, 653, 3513, 506, 5301, 562, 5010, 614, 10942, 539, 2976, 462, 5189, 567, 2032,
    123, 124, 125, 126, 127, 128, 129, 130, 131, 132, 588, 803, 1040, 49209, 4, 5, 6, 7, 8, 9, 10,
    11, 12,
]];

/// erato 白名单（novelai.js:57-62）
const REP_PEN_ERATO: &[&[i64]] = &[&[
    6, 1, 11, 13, 25, 198, 12, 9, 8, 279, 264, 459, 323, 477, 539, 912, 374, 574, 1051, 1550, 1587,
    4536, 5828, 15058, 3287, 3250, 1461, 1077, 813, 11074, 872, 1202, 1436, 7846, 1288, 13434,
    1053, 8434, 617, 9167, 1047, 19117, 706, 12775, 649, 4250, 527, 7784, 690, 2834, 15, 16, 17,
    18, 19, 20, 21, 22, 23, 24, 1210, 1359, 608, 220, 596, 956, 3077, 44886, 4265, 3358, 2351,
    2846, 311, 389, 315, 304, 520, 505, 430,
]];

fn rep_pen_of(model: &str) -> Option<Value> {
    if model.contains("clio") || model.contains("kayra") {
        return Some(Value::Array(
            REP_PEN_CLIO_KAYRA
                .iter()
                .flat_map(|ids| ids.iter().map(|i| json!(i)).collect::<Vec<_>>())
                .collect(),
        ));
    }
    if model.contains("erato") {
        return Some(Value::Array(
            REP_PEN_ERATO
                .iter()
                .flat_map(|ids| ids.iter().map(|i| json!(i)).collect::<Vec<_>>())
                .collect(),
        ));
    }
    None
}

/// POST /api/novelai/generate（manifest #43）
pub async fn generate(st: &BackendState, body: &Value, origin: Option<HeaderValue>) -> Response {
    let api_key = match read_secret(st, "api_key_novel") {
        Some(k) => k,
        // ST 字面：裸 400（sendStatus 无 JSON 体）
        None => {
            use axum::response::IntoResponse;
            return super::apply_cors_resp(StatusCode::BAD_REQUEST.into_response(), origin);
        }
    };
    let model = body
        .get("model")
        .and_then(|m| m.as_str())
        .unwrap_or("")
        .to_string();
    let model = model.as_str();

    // App 请求体是平铺（input/model/use_string/…）→ ST 服务端重组进 parameters
    let mut parameters = Map::new();
    let default_on =
        |k: &str, dflt: Value, body: &Value| -> Value { body.get(k).cloned().unwrap_or(dflt) };
    parameters.insert(
        "use_string".into(),
        default_on("use_string", json!(true), body),
    );
    for k in [
        "temperature",
        "max_length",
        "min_length",
        "tail_free_sampling",
        "repetition_penalty",
        "repetition_penalty_range",
        "repetition_penalty_slope",
        "repetition_penalty_frequency",
        "repetition_penalty_presence",
        "top_a",
        "top_p",
        "top_k",
        "typical_p",
        "mirostat_lr",
        "mirostat_tau",
        "phrase_rep_pen",
        "stop_sequences",
        "generate_until_sentence",
        "use_cache",
        "return_full_text",
        "prefix",
        "order",
        "num_logprobs",
        "min_p",
        "math1_temp",
        "math1_quad",
        "math1_quad_entropy_scale",
    ] {
        if let Some(v) = body.get(k) {
            parameters.insert(k.to_string(), v.clone());
        }
    }
    // repetition_penalty_whitelist（模型默认）
    if let Some(whitelist) = rep_pen_of(model) {
        parameters.insert("repetition_penalty_whitelist".into(), whitelist);
    }
    // bad_words_ids：模型默认 + App 传入追加 + 空数组剔除
    let mut bad_words = bad_words_of(model);
    if let Some(list) = body.get("bad_words_ids").and_then(|b| b.as_array()) {
        for bw in list {
            let is_int_array = bw
                .as_array()
                .map(|a| a.iter().all(|x| x.is_i64() || x.is_u64()))
                .unwrap_or(false);
            if is_int_array {
                bad_words.push(bw.clone());
            }
        }
    }
    bad_words.retain(|bw| bw.as_array().map(|a| !a.is_empty()).unwrap_or(true));
    parameters.insert(
        "bad_words_ids".into(),
        if bad_words.is_empty() {
            Value::Null
        } else {
            Value::Array(bad_words)
        },
    );
    parameters.insert("logit_bias_exp".into(), {
        let mut list = logit_bias_of(model);
        if let Some(extra) = body.get("logit_bias_exp").and_then(|e| e.as_array()) {
            list.extend(extra.clone());
        }
        Value::Array(list)
    });

    let mut data = Map::new();
    data.insert(
        "input".into(),
        body.get("input").cloned().unwrap_or(Value::Null),
    );
    data.insert(
        "model".into(),
        body.get("model").cloned().unwrap_or(Value::Null),
    );
    data.insert("parameters".into(), Value::Object(parameters));
    // theme_textadventure → eos
    if body.get("prefix").and_then(|p| p.as_str()) == Some("theme_textadventure") {
        let eos = if model.contains("clio") || model.contains("kayra") {
            49405
        } else if model.contains("erato") {
            29
        } else {
            -1
        };
        if eos >= 0 {
            data["parameters"]
                .as_object_mut()
                .unwrap()
                .insert("eos_token_id".into(), json!(eos));
        }
    }

    let base = if model.contains("kayra") || model.contains("erato") {
        TEXT_NOVELAI
    } else {
        API_NOVELAI
    };
    let streaming = body
        .get("streaming")
        .and_then(|s| s.as_bool())
        .unwrap_or(false);
    let url = if streaming {
        format!("{base}/ai/generate-stream")
    } else {
        format!("{base}/ai/generate")
    };

    let resp_res =
        super::providers::upstream_post(&url, &api_key, Vec::new(), &Value::Object(data)).await;
    let Ok(resp) = resp_res else {
        return json_resp(StatusCode::OK, json!({ "error": true }), origin);
    };
    let status_ok = resp.status().is_success();
    if streaming {
        return super::providers::passthrough(resp, origin).await;
    }
    let text = resp.text().await.unwrap_or_default();
    if !status_ok {
        let message = serde_json::from_str::<Value>(&text)
            .ok()
            .and_then(|v| v.get("message").and_then(|m| m.as_str()).map(String::from))
            .unwrap_or(text.clone());
        return json_resp(
            StatusCode::INTERNAL_SERVER_ERROR,
            json!({ "error": { "message": message } }),
            origin,
        );
    }
    let v = serde_json::from_str::<Value>(&text).unwrap_or(json!({}));
    json_resp(StatusCode::OK, v, origin)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bad_words_by_model() {
        assert_eq!(bad_words_of("clio-v2").len(), 20);
        assert_eq!(bad_words_of("erato-v4").len(), 7);
        assert_eq!(bad_words_of("hypebot-3").len(), BAD_WORDS_HYPEBOT.len());
        assert_eq!(bad_words_of("other").len(), 0);
    }

    #[test]
    fn rep_pen_by_model() {
        assert!(rep_pen_of("kayra-v4").is_some());
        assert!(rep_pen_of("erato-x").is_some());
        assert!(rep_pen_of("mystery").is_none());
    }
}
