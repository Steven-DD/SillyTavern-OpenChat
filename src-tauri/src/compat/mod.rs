//! compat —— ST 格式兼容内核（评审 T3 · 方案 §4.2 compat/）
//!
//! 依赖方向（T22 适应度函数目标）：api → compat → 共用件；
//! compat **不依赖 api**（格式内核不含 HTTP 语义）。
//! Phase 1 覆盖：png（tEXt chunk 编解码）+ chara_card（V1/V2/V3 归一化）。
//! 后续批次：charx（zip）、byaf、jsonl、worldinfo、settings 映射。

pub mod chara_card;
pub mod charx;
pub mod png;
