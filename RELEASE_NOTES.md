# ST Chat v0.1.0-alpha

自包含 Rust 后端的桌面聊天客户端首个公开测试版。数据以 SillyTavern 兼容格式保存，
可与 ST 生态互通。

## 安装

下载 `ST Chat_0.1.0_x64-setup.exe`（NSIS 安装包），双击安装即可。
启动即拉起内置本地后端，**无需下载或安装 Node / SillyTavern**。

## 本版本包含

- **聊天主链路**：流式生成 / 停止（保留已生成部分）/ 重新生成 / Swipe 备选 /
  消息编辑删除重发 / Continue 续写 / Impersonate 代写 / auto-continue / 消息隐藏与旁白
- **生成源**：Chat Completion 24 源 + Text Completion 15 后端 + NovelAI + Horde；
  采样参数与 settings.json 同步
- **角色与世界观**：角色卡导入导出（PNG/JSON/charx）、世界书全字段编辑器与完整注入
  管线（递归 / sticky-cooldown / min_activations / 分组打分）、Persona、Prompt Manager
- **群聊**：四种激活策略、自动发言、群头像
- **扩展体系**：宏（55+）、STscript 子集、正则脚本、快捷回复、Summarize/Vectors、
  表情立绘（含自动切换）、消息翻译、TTS 多供应商
- **安全**：本地环回后端 + 一次性 token 鉴权；密钥类存 OS keychain

## 已知限制

- 当前仅提供 Windows 安装包（多端能力随 Tauri 2 后续启用）
- TTS 覆盖 5 家常用供应商，其余供应商按需补齐
- 界面暂为中文单语言（i18n 排期中）

## 反馈

问题请到 [Issues](https://github.com/Steven-DD/SillyTavern-OpenChat/issues) 提交。
