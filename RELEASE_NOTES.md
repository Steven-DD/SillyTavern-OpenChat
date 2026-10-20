# SillyTavern-OpenChat v0.2.0

SillyTavern-OpenChat 是自包含 Rust 本地后端的跨平台桌面与移动聊天客户端。
本版本正式开启 **Android 移动端支持**，并带来 **MCP 工具生态**、**WebDAV 云端同步**、**Live2D/VRM 动态视口**、**会话树状分支图**、**Lua 沙箱**及**多语言国际化**等重大特性。

---

## 📦 安装与下载

### 1. Android 移动端安装包
本版本通过 ABI 架构精简分包，单个安装包由 300MB+ 大幅缩减至 **~32MB**：
- **`SillyTavern-OpenChat_0.2.0_arm64-v8a.apk`**：适用于绝大多数主流 Android 手机与平板（推荐）。
- **`SillyTavern-OpenChat_0.2.0_x86_64.apk`**：适用于 Android 模拟器、Chromebook 及 x86 设备。

### 2. Windows 桌面安装包
- **`SillyTavern-OpenChat_0.2.0_x64-setup.exe`**：NSIS 当前用户安装程序，双击即用，无需配置 Node.js 运行时。

---

## 🚀 本版本重点特性

### 📱 Android 移动端全量支持
- **原生适配**：基于 Tauri v2 Mobile 架构与 Android 15 API 35 规范适配，内置 Rust 服务与自包含前端无缝协作。
- **架构拆分与极度轻量化**：通过启用 ABI 分包与 Release 编译优化（`opt-level = "s"`, 符号剥离与 Proguard），安装包体积降低 **89%**（仅 ~32MB）。
- **全自动化端到端测试覆盖**：在 Android 15 环境下完成底部五大模块导航、SillyTavern v3 角色创建、会话新建与数据持久化全流程自动化冒烟与 UI 渲染验证。

### 🧩 MCP (Model Context Protocol) 工具生态
- 支持作为 MCP 客户端连接外部工具服务，实现模型自主工具调用与多轮工具回传。
- 内置工具调用解析与异常容错保护机制。

### ☁️ WebDAV 云端增量同步
- 支持多设备（PC/Android）间通过 WebDAV 进行会话、角色与配置的增量同步。
- 具备网络连通性探测、加密凭据存储与 JSONL 会话合并防丢失机制。

### 🌳 树状会话分支图 (Branching Tree)
- 提供类 Git 的会话分叉与时间线视图，支持会话节点快速回溯、分叉切换与多结局探索。

### 🎭 Live2D 与 VRM 3D 动态模型视口
- 支持 Live2D 与 VRM 3D 模型加载与动态舞台交互。
- 集成 6 种核心情绪动作映射与 Web Audio RMS 实时音频口型同步。

### 🛡️ 安全 Lua 脚本沙箱
- 兼容 RisuAI 脚本生态与生命周期钩子（`onPrompt` / `onMessage`）。
- 严格限制宿主系统 API 调用，内建指令数配额（硬上限 250,000）以彻底杜绝死循环攻击。

### 🎨 文生图与语音输入 (STT)
- 支持聊天文本内 `{draw::...}` 宏解析与文生图 API 调度。
- 支持基于浏览器的实时语音识别输入。

### 🌐 多语言国际化 (i18n)
- 完整支持 **简体中文 (zh-CN)** 与 **English (en-US)** 界面无缝切换，多语言键集合 100% 结构对称。

---

## 🛠️ 质量与门禁
- **前端自检**：全量 29 项自检（Selfcheck）100% PASS。
- **Rust 后端**：全量 228 项单元测试与集成测试 100% PASS。
- **移动端**：Android CLI 自动化测试 0 Crash、0 ANR，全生命周期稳定。

---

## 升级与数据兼容性
- 完整兼容 SillyTavern 角色卡格式（V2 / V3 / CharX）与会话 JSONL 格式。
- 本地历史数据无缝升级，密钥自动安全迁移至系统凭据库。
