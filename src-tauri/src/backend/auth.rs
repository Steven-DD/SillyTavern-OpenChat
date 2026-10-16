//! backend/auth —— token 生成（S1）
//!
//! 与 relay.rs:76-81 同源（getrandom 0.2 + OS CSPRNG + hex 编码 256-bit）。
//! relay 在 native 模式移除后这里是唯一来源。

/// 生成 256-bit 随机 token（hex）。环形回环上的本地 token，熵源用 OS CSPRNG。
pub fn gen_token() -> String {
    let mut buf = [0u8; 32];
    getrandom::getrandom(&mut buf).expect("OS CSPRNG 不可用（getrandom 失败）");
    buf.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn token_is_256bit_hex_and_unique() {
        let a = super::gen_token();
        let b = super::gen_token();
        assert_eq!(a.len(), 64, "256-bit → 64 hex 字符");
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b, "两次生成不应相同");
    }
}
