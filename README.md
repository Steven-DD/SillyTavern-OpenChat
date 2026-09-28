<div align="center">

# SillyTavern-OpenChat

**ST Chat · SillyTavern 的多端客户端**

内部托管原版 SillyTavern 服务端，数据与 ST 网页端完全互通：
同一份角色卡、会话、世界书，双端随时切换使用。

![Status](https://img.shields.io/badge/Status-开发中-orange)
![License](https://img.shields.io/badge/License-AGPL_v3-blue)
![Tauri](https://img.shields.io/badge/Tauri-2-FFC131?logo=tauri&logoColor=black)
![Vue](https://img.shields.io/badge/Vue-3-4FC08D?logo=vuedotjs&logoColor=white)

</div>

> [!WARNING]
> **本项目正在积极开发中，尚未发布第一个正式版本。**
> 功能、接口与数据结构仍可能随时大幅调整，请勿用于生产环境；
> 欢迎试用并到 [Issues](https://github.com/Steven-DD/SillyTavern-OpenChat/issues) 反馈问题。

## 数据互通

App 沿用 SillyTavern 标准数据目录结构（角色卡、JSONL 会话、世界书、`settings.json`、
群聊文件、`chat_metadata` 等），**不做私有格式**：

- 在 ST 网页端和 App 之间切换，看到的是同一份角色与聊天记录；
- 会话级状态（人设锁、作者注释、摘要、宏变量、timedWorldInfo）直接读写
  `chat_metadata`，与网页端互通；
- 组件安装器把 Node / SillyTavern / Git 安装在版本化目录中，数据区独立于安装目录，
  升级 / 重装不丢数据。

## 快速开始

### 环境要求

- 目标形态为**多端应用**（Tauri 2，桌面 + 移动）；当前以 **Windows 10 / 11**
  为主要开发与打包平台（NSIS 安装包）
- [Node.js](https://nodejs.org/) ≥ 20（构建前端用；运行时的 Node 由应用托管）
- [Rust](https://rustup.rs/) stable（MSVC 工具链）

### 构建运行

```bash
git clone git@github.com:Steven-DD/SillyTavern-OpenChat.git
cd SillyTavern-OpenChat
npm install

npm run tauri dev     # 开发运行：桌面壳 + 前端热更新（首次启动引导安装 ST 组件）
npm run tauri build   # 产出 NSIS 安装包（src-tauri/target/release/bundle/nsis/）
```

### 仅调试前端（浏览器）

```bash
# 在仓库目录的上一级准备一份 SillyTavern（供 scripts/st-dev.mjs 拉起）
git clone --depth 1 --branch release https://github.com/SillyTavern/SillyTavern.git ../SillyTavern

npm run st     # 拉起 ST 服务端（127.0.0.1:8000，纯后端模式）
npm run dev    # 浏览器打开 http://127.0.0.1:1420，经 Vite 代理访问 ST
```

## 测试

```bash
npm run typecheck                                # vue-tsc 类型检查
npm run build                                    # 类型检查 + 生产构建
cargo test --manifest-path src-tauri/Cargo.toml  # Rust 单测 + 中继/插件宿主 e2e
npm run relay-test                               # 中继 e2e（含对真实 ST 的验证）
```

`scripts/selfcheck-*.mjs` 为各功能模块的回归自检脚本（宏、正则、群聊编排、
Instruct、textgen 等）。

## 安全设计

- **网络面**：ST 与中继均只绑定本机环回；中继对每个请求校验一次性随机 token，
  本机其它进程与浏览器网页无法借道访问 ST（含密钥接口）。
- **密钥**：所有上游 API Key 经 ST `/api/secrets` 由服务端加密托管，前端不落盘。
- **渲染**：AI 消息 Markdown 渲染走 sanitize 白名单，角色卡字段按纯文本处理。

## 许可证

本项目以 [AGPL-3.0](./LICENSE) 协议开源 —— 与上游
[SillyTavern](https://github.com/SillyTavern/SillyTavern) 同协议，感谢 ST 项目的优秀工作。
