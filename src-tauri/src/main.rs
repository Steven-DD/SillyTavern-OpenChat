//! 全局：MSVC linker 本地化 stdout 警告抑制（见 lib.rs 同注）
#![allow(linker_messages)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    st_chat_lib::run()
}
