//! compat/png —— PNG tEXt chunk 编解码（评审 T3 · manifest #4/#5/#8 的底层）
//!
//! ST 契约（character-card-parser.js:15-78，已逐行核对）：
//! - `read`：抽 tEXt chunk → base64 解码；**ccv3（v3）优先于 chara（v2）**
//! - `write`：移除旧 chara/ccv3 chunk → IEND 前插入 base64('chara') + base64('ccv3')
//! - chunk 布局：[length BE][name 4B][data][crc32 BE]，crc 覆盖 name+data
//! - PNG 头 8 字节：89 50 4E 47 0D 0A 1A 0A
//!
//! ⚠ 非 tEXt 的 chunk（IDAT/IHDR 等）**原样保留** —— 角色卡导入导出必须
//! 保持 ST 可读可导入（方案 Phase 1 退出标准）。

use base64::Engine;

#[derive(Debug)]
pub struct PngError(pub String);

impl std::fmt::Display for PngError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl std::error::Error for PngError {}

const PNG_MAGIC: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// 解析出的 chunk（name + data）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chunk {
    pub name: [u8; 4],
    pub data: Vec<u8>,
}

/// 拆分 PNG 为 [magic 后的 chunk 列表]。非 PNG 或结构损坏 → Err。
pub fn extract_chunks(bytes: &[u8]) -> Result<Vec<Chunk>, PngError> {
    if bytes.len() < 8 || bytes[..8] != PNG_MAGIC {
        return Err(PngError("不是 PNG 文件（魔数不符）".into()));
    }
    let mut chunks = Vec::new();
    let mut pos = 8usize;
    while pos < bytes.len() {
        if pos + 8 > bytes.len() {
            return Err(PngError("PNG chunk 头被截断".into()));
        }
        let len = u32::from_be_bytes([bytes[pos], bytes[pos + 1], bytes[pos + 2], bytes[pos + 3]])
            as usize;
        let name: [u8; 4] = bytes[pos + 4..pos + 8].try_into().unwrap();
        let data_start = pos + 8;
        let data_end = data_start + len;
        if data_end + 4 > bytes.len() {
            return Err(PngError("PNG chunk 数据被截断".into()));
        }
        let data = bytes[data_start..data_end].to_vec();
        let crc_stored = u32::from_be_bytes([
            bytes[data_end],
            bytes[data_end + 1],
            bytes[data_end + 2],
            bytes[data_end + 3],
        ]);
        let crc_calc = crc32(&[name.as_slice(), &data].concat());
        if crc_stored != crc_calc {
            return Err(PngError(format!(
                "PNG chunk {} CRC 不符（{crc_stored:#x} != {crc_calc:#x}）",
                String::from_utf8_lossy(&name)
            )));
        }
        chunks.push(Chunk { name, data });
        pos = data_end + 4;
    }
    Ok(chunks)
}

/// chunk 列表 → PNG 字节（对齐 encode.js：BE 长度 + name + data + BE crc）
pub fn encode_chunks(chunks: &[Chunk]) -> Vec<u8> {
    let total: usize = 8 + chunks.iter().map(|c| c.data.len() + 12).sum::<usize>();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(&PNG_MAGIC);
    for c in chunks {
        out.extend_from_slice(&(c.data.len() as u32).to_be_bytes());
        out.extend_from_slice(&c.name);
        out.extend_from_slice(&c.data);
        out.extend_from_slice(&crc32(&[c.name.as_slice(), &c.data].concat()).to_be_bytes());
    }
    out
}

/// 构造 tEXt chunk：keyword\0text（对齐 png-chunk-text encode）
pub fn text_chunk(keyword: &str, text: &str) -> Chunk {
    let mut data = Vec::with_capacity(keyword.len() + 1 + text.len());
    data.extend_from_slice(keyword.as_bytes());
    data.push(0);
    data.extend_from_slice(text.as_bytes());
    Chunk {
        name: *b"tEXt",
        data,
    }
}

/// 解码 tEXt chunk → (keyword, text)（对齐 png-chunk-text decode）
pub fn text_chunk_decode(data: &[u8]) -> Option<(String, String)> {
    let nul = data.iter().position(|&b| b == 0)?;
    let keyword = String::from_utf8_lossy(&data[..nul]).to_string();
    let text = String::from_utf8_lossy(&data[nul + 1..]).to_string();
    Some((keyword, text))
}

/// 读角色卡数据：ccv3 优先，其次 chara（character-card-parser.js:54-78）
pub fn read_chara(png: &[u8]) -> Result<String, PngError> {
    let chunks = extract_chunks(png)?;
    let texts: Vec<(String, String)> = chunks
        .iter()
        .filter(|c| c.name == *b"tEXt")
        .filter_map(|c| text_chunk_decode(&c.data))
        .collect();
    if texts.is_empty() {
        return Err(PngError("PNG 无 tEXt chunk（无角色数据）".into()));
    }
    for kw in ["ccv3", "chara"] {
        if let Some((_, v)) = texts.iter().find(|(k, _)| k.eq_ignore_ascii_case(kw)) {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(v)
                .map_err(|e| PngError(format!("base64 解码失败：{e}")))?;
            return String::from_utf8(bytes).map_err(|e| PngError(format!("UTF-8 解码失败：{e}")));
        }
    }
    Err(PngError("PNG 无 chara/ccv3 chunk".into()))
}

/// 写角色卡数据：移除旧 chara/ccv3 → IEND 前插 chara + ccv3
/// （character-card-parser.js:15-46；ccv3 插入失败不阻断 —— v2 兜底）
pub fn write_chara(png: &[u8], data: &str) -> Result<Vec<u8>, PngError> {
    let mut chunks = extract_chunks(png)?;
    let b64 = base64::engine::general_purpose::STANDARD.encode(data.as_bytes());
    // 移除旧 chara/ccv3
    chunks.retain(|c| {
        if c.name != *b"tEXt" {
            return true;
        }
        match text_chunk_decode(&c.data) {
            Some((kw, _)) => !(kw.eq_ignore_ascii_case("chara") || kw.eq_ignore_ascii_case("ccv3")),
            None => true,
        }
    });
    // 找 IEND 位置（正常 PNG 必有；缺失则追加）
    let iend = chunks
        .iter()
        .position(|c| c.name == *b"IEND")
        .unwrap_or(chunks.len());
    // v3 chunk：spec 改写失败（非 JSON）时跳过（对齐 ST try/catch）
    if let Ok(mut v) = serde_json::from_str::<serde_json::Value>(data) {
        if let Some(obj) = v.as_object_mut() {
            obj.insert("spec".into(), serde_json::json!("chara_card_v3"));
            obj.insert("spec_version".into(), serde_json::json!("3.0"));
            let v3 = serde_json::to_string(&v).unwrap_or_default();
            let b64v3 = base64::engine::general_purpose::STANDARD.encode(v3.as_bytes());
            chunks.insert(iend, text_chunk("ccv3", &b64v3));
        }
    }
    chunks.insert(iend, text_chunk("chara", &b64));
    Ok(encode_chunks(&chunks))
}

/// PNG CRC32（对齐 crc 包：IEEE 多项式 0xEDB88320，初值 0xFFFFFFFF，末位取反）
pub fn crc32(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for i in 0..256u32 {
        let mut c = i;
        for _ in 0..8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
        }
        table[i as usize] = c;
    }
    let mut crc: u32 = 0xFFFF_FFFF;
    for &b in data {
        crc = table[((crc ^ b as u32) & 0xFF) as usize] ^ (crc >> 8);
    }
    !crc
}

/// 生成 1x1 透明 PNG 兜底图（writeCharacterData 读图失败时的 fallback 语义；
/// 这里是“最小合法 PNG”而非 ST 的 DEFAULT_AVATAR_PATH —— 兜底只需结构合法）
pub fn minimal_png() -> Vec<u8> {
    // 1x1 灰度 PNG（crc 正确、ST/浏览器均可解码）
    const IHDR: [u8; 13] = [
        0, 0, 0, 1, // width=1
        0, 0, 0, 1, // height=1
        8, // bit depth
        0, // color type gray
        0, 0, 0, // compression/filter/interlace
    ];
    let idat_data = [0x00u8]; // filter byte for one row
    let chunks = vec![
        Chunk {
            name: *b"IHDR",
            data: IHDR.to_vec(),
        },
        Chunk {
            name: *b"IDAT",
            data: zlib_stored(&idat_data),
        },
        Chunk {
            name: *b"IEND",
            data: Vec::new(),
        },
    ];
    encode_chunks(&chunks)
}

/// zlib 存储块（无压缩，wbits=15 的合法 zlib 流：2 字节头 + stored block + adler32）
fn zlib_stored(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01]; // zlib header: deflate, 32K window
    out.push(0x01); // BFINAL=1, BTYPE=00 (stored)
    out.extend_from_slice(&(data.len() as u16).to_le_bytes());
    out.extend_from_slice(&(!(data.len() as u16)).to_le_bytes());
    out.extend_from_slice(data);
    let adler = adler32(data);
    out.extend_from_slice(&adler.to_be_bytes());
    out
}

fn adler32(data: &[u8]) -> u32 {
    let mut a: u32 = 1;
    let mut b: u32 = 0;
    for &byte in data {
        a = (a + byte as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_matches_known_vector() {
        // 已知向量：crc32("123456789") = 0xCBF43926（IEEE）
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0);
    }

    #[test]
    fn text_chunk_roundtrip() {
        let c = text_chunk("chara", "eyJhIjoxfQ==");
        assert_eq!(c.name, *b"tEXt");
        let (kw, v) = text_chunk_decode(&c.data).unwrap();
        assert_eq!(kw, "chara");
        assert_eq!(v, "eyJhIjoxfQ==");
    }

    #[test]
    fn read_write_roundtrip_and_v3_precedence() {
        // 构造一张带旧 chara 的 PNG → write 后 ccv3 优先读出
        let png = minimal_png();
        let v2 = r#"{"spec":"chara_card_v2","name":"old"}"#;
        let chunks0 = extract_chunks(&png).unwrap();
        let mut with_old = chunks0.clone();
        let iend = with_old.iter().position(|c| c.name == *b"IEND").unwrap();
        let b64 = base64::engine::general_purpose::STANDARD.encode(v2.as_bytes());
        with_old.insert(iend, text_chunk("chara", &b64));
        let png_old = encode_chunks(&with_old);

        // 写新数据（v2 形态输入 → 自动派生 ccv3）
        let new_data = r#"{"name":"林晚棠","data":{"name":"林晚棠"}}"#;
        let out = write_chara(&png_old, new_data).unwrap();

        // read：ccv3 优先 —— spec 被改写为 v3
        let raw = read_chara(&out).unwrap();
        let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(v["spec"], "chara_card_v3");
        assert_eq!(v["spec_version"], "3.0");
        assert_eq!(v["name"], "林晚棠");

        // 非 tEXt chunk 保留（IDAT/IHDR/IEND 结构完整）
        let out_chunks = extract_chunks(&out).unwrap();
        let names: Vec<&[u8; 4]> = out_chunks.iter().map(|c| &c.name).collect();
        assert!(names.contains(&b"IHDR"));
        assert!(names.contains(&b"IDAT"));
        assert!(names.contains(&b"IEND"));
        // 恰好 2 个 chara 类 chunk（chara + ccv3），旧的被移除
        let n = out_chunks
            .iter()
            .filter(|c| {
                c.name == *b"tEXt"
                    && text_chunk_decode(&c.data)
                        .map(|(k, _)| {
                            k.eq_ignore_ascii_case("chara") || k.eq_ignore_ascii_case("ccv3")
                        })
                        .unwrap_or(false)
            })
            .count();
        assert_eq!(n, 2, "chara + ccv3 各一个");
    }

    #[test]
    fn corrupt_png_rejected() {
        assert!(extract_chunks(b"not a png at all").is_err());
        let mut bad = minimal_png();
        let n = bad.len();
        bad[n - 5] ^= 0xFF; // 破坏 IEND 前的 crc 字节
        assert!(extract_chunks(&bad).is_err(), "CRC 不符必须拒绝");
        assert!(read_chara(&bad).is_err());
    }

    #[test]
    fn non_json_input_still_writes_v2_only() {
        // ST 的 try/catch：非 JSON 数据跳过 ccv3（v2 兜底）
        let png = minimal_png();
        let out = write_chara(&png, "not-json").unwrap();
        let raw = read_chara(&out).unwrap();
        assert_eq!(raw, "not-json");
    }
}
