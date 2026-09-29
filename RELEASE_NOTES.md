# ST Chat v0.1.0-alpha

SillyTavern 的多端客户端首个公开测试版。内部托管原版 SillyTavern 服务端，
数据与 ST 网页端完全互通。

## 安装

下载 `ST Chat_0.1.0_x64-setup.exe`（NSIS 安装包），双击安装即可。
首次启动会进入引导页，自动下载并托管 SillyTavern 运行组件（Node / ST / Git）。

## 本版本包含

- **聊天主链路**：流式生成 / 停止（保留已生成部分）/ 重新生成 / Swipe 备选 /
  消息编辑删除重发 / Continue 续写 / Impersonate 代写 / auto-continue / 消息隐藏与旁白
- **生成源**：Chat Completion 24 源 + Text Completion 15 后端 + NovelAI + Horde；
  采样参数与 ST settings.json 双向同步
- **角色与世界观**：角色卡导入导出（PNG/JSON/charx）、世界书全字段编辑器与完整注入
  管线（递归 / sticky-cooldown / min_activations / 分组打分）、Persona、Prompt Manager
- **群聊**：与 ST 数据互通，四种激活策略、自动发言、群头像
- **扩展体系**：宏（55+）、STscript 子集、正则脚本、快捷回复、Summarize/Vectors、
  表情立绘（含自动切换）、消息翻译、TTS 多供应商
- **安全**：本地环回中继 + 一次性 token 鉴权；API 密钥由服务端加密托管

## 已知限制

- 当前仅提供 Windows 安装包（多端能力随 Tauri 2 后续启用）
- TTS 覆盖 5 家常用供应商，其余供应商按需补齐
- 界面暂为中文单语言（i18n 排期中）

## 反馈

问题请到 [Issues](https://github.com/Steven-DD/SillyTavern-OpenChat/issues) 提交。
