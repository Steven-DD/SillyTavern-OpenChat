<div align="center">

# SillyTavern-OpenChat

**ST Chat · 自包含 Rust 后端的桌面聊天客户端**

内置 Rust 本地 HTTP 后端，数据以 SillyTavern 兼容格式保存，
同一份角色卡、会话、世界书可与 ST 生态互通。

![Status](https://img.shields.io/badge/Status-开发中-orange)
![License](https://img.shields.io/badge/License-AGPL_v3-blue)
![Tauri](https://img.shields.io/badge/Tauri-2-FFC131?logo=tauri&logoColor=black)
![Vue](https://img.shields.io/badge/Vue-3-4FC08D?logo=vuedotjs&logoColor=white)

</div>

> [!WARNING]
> **本项目正在积极开发中，尚未发布第一个正式版本。**
> 功能、接口与数据结构仍可能随时大幅调整，请勿用于生产环境；
> 欢迎试用并到 [Issues](https://github.com/Steven-DD/SillyTavern-OpenChat/issues) 反馈问题。

## 架构

App 内置 Rust 本地 HTTP 后端（`src-tauri/src/backend/`），**不再依赖外部 Node / SillyTavern
进程**：启动即拉起自包含后端，数据落盘到应用数据目录。

## 数据互通

数据沿用 SillyTavern 标准目录结构（角色卡、JSONL 会话、世界书、`settings.json`、
群聊文件、`chat_metadata` 等），**不做私有格式**：

- 会话级状态（人设锁、作者注释、摘要、宏变量、timedWorldInfo）直接读写
  `chat_metadata`，与网页端互通；
- 数据目录独立于程序目录，升级 / 重装不丢数据；
- 设置 → 数据 提供数据袋导出 / 导入做备份与迁移。

## 快速开始

### 环境要求

- 目标形态为**多端应用**（Tauri 2，桌面 + 移动）；当前以 **Windows 10 / 11**
  为主要开发与打包平台（NSIS 安装包）
- [Node.js](https://nodejs.org/) ≥ 20（仅构建前端用；运行时不需要 Node）
- [Rust](https://rustup.rs/) stable（MSVC 工具链）

### 构建运行

```bash
git clone git@github.com:Steven-DD/SillyTavern-OpenChat.git
cd SillyTavern-OpenChat
npm install

npm run tauri dev     # 开发运行：桌面壳 + 前端热更新
npm run tauri build   # 产出 NSIS 安装包（src-tauri/target/release/bundle/nsis/）
```

## 测试

```bash
npm run typecheck                                # vue-tsc 类型检查
npm run build                                    # 类型检查 + 生产构建
npm run selfcheck                                # 前端各模块回归自检
cargo test --manifest-path src-tauri/Cargo.toml  # Rust 单测 + 契约/黄金样本测试
```

## 排障与日志

前后端日志统一走 `tauri-plugin-log`，落在系统日志目录（Windows：
`%LOCALAPPDATA%\<app-id>\logs\`，单文件 2MB 滚动保留一份），按时间线可直接
串联「前端操作 → 后端请求 → 上游出站」：

- 每条请求一行：`← 200 12ms POST /api/chats/get origin=… bytes=…`（`stchat::api`）
- 数据落盘：`stchat::fs`；上游出站：`stchat::provider`；密钥：`stchat::vault`；
  启动：`stchat::boot`；扩展/翻译/TTS：`stchat::ext`
- 默认级别 `Info`；排障时设环境变量 `ST_CHAT_LOG=debug`（或 `trace`）后重启应用
- 日志**不含请求体**，且 token/密钥一律脱敏为 `***`（安全规格 S13）

## 安全设计

- **网络面**：本地后端只绑定 127.0.0.1 随机端口；每个请求校验一次性随机 token，
  本机其它进程与浏览器网页无法借道访问（含密钥接口）。
- **密钥**：密钥类存 OS keychain（vault），前端不落盘。
- **渲染**：AI 消息 Markdown 渲染走 sanitize 白名单，角色卡字段按纯文本处理。

## 许可证

本项目以 [AGPL-3.0](./LICENSE) 协议开源。数据格式与部分协议行为参考
[SillyTavern](https://github.com/SillyTavern/SillyTavern)，感谢 ST 项目的优秀工作。
