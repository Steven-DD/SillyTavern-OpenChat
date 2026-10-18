# SillyTavern-OpenChat v0.2.0-alpha

自包含 Rust 后端的桌面聊天客户端。本版本完成「去 ST 后端化」收口：不再依赖 Node / SillyTavern
运行时，数据仍以 SillyTavern 兼容格式保存，可与 ST 生态互通。

## 安装

下载 SillyTavern-OpenChat_0.2.0_x64-setup.exe（NSIS，当前用户安装），双击安装即可。
启动即拉起内置本地后端，**无需下载或安装 Node / SillyTavern**。

## 本版本重点

- **应用更名与标识统一**：ST Chat → SillyTavern-OpenChat；安装包名、窗口标题、可执行文件名、
  数据目录（stoc-runtime）、配置命名空间（stoc_*）、日志与凭据命名全部统一。
- **后端完全自包含**：端点族全量落地（settings / characters / chats / worldinfo / groups /
  扩展 git 安装 / Chat Completion 26 源 / Text Completion 15 后端 / NovelAI / Horde /
  tokenizers / translate / TTS），原有反向中继已移除。
- **密钥策略收紧**：以系统 keychain 为权威，secrets.json 仅保留结构（正常路径不再落明文）；
  仅当 keychain 不可用或单条超长时才回退明文。首次启动会把历史明文回填到 keychain 并清除磁盘明文。
- **情感分类本地实装**：/api/extra/classify（GoEmotions 28 类）不再依赖 ST 的 extra 模块。
- **settings.json 落盘缩进对齐 ST** 的 4 空格，跨端读写更一致。
- **数据袋导出加固**：排除任意层级的 secrets.json / cookie-secret.txt 及其备份副本；
  包含 API 密钥导出时必须输入确认词。
- **Edge TTS 明确降级**：该能力属 ST 端插件、本期不迁移，界面给出明确提示而非静默失败。
- **回归门禁新增 5 条**：无 ST 直连 / 无 node 子进程 / 契约出口登记 / 未实装端点禁引 /
  IPC 契约一致；并移除了已失效的「双跑」验收框架。

## 升级说明（自 v0.1.0-alpha 或更早）

- 应用标识由 io.github.stevendd.stchat 变为 io.github.stevendd.sillytavernopenchat，属**新应用身份**：
  安装本版本后请卸载旧版（旧版不会被自动更新到本版本）。
- 首次启动会自动把旧数据目录（旧标识目录下的 st-runtime）**整体迁移**到新数据根 stoc-runtime；
  若迁移失败会保留原目录并写日志，可手动搬运，不会静默丢数据。
- 旧设置键（stchat_*）自动迁移到 stoc_*；旧版保存的密钥会被回填到系统凭据库并清掉磁盘明文。

## 已知限制

- 当前仅提供 Windows 安装包（多端能力随 Tauri 2 后续启用）。
- App 内自动更新依赖发布侧签名密钥：本 Release 未附带 latest.json 与 .sig，如需启用请配置
  仓库 Secrets 的 TAURI_SIGNING_PRIVATE_KEY / PASSWORD。
- 向量集合 id 已更名为 stoc-*，旧集合不做自动迁移，需重新索引会话。
- 界面暂为中文单语言（i18n 排期中）。

## 反馈

问题请到 [Issues](https://github.com/Steven-DD/SillyTavern-OpenChat/issues) 提交。
