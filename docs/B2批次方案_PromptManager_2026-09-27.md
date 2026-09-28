# B2 批次方案：Prompt Manager（M-5 / P4）（2026-09-27）

> 目标：提示词组装对齐 ST 的 Prompt Manager —— main/nsfw/jailbreak 三槽可编辑 + 条目排序/启停
> （prompt_order），数据存 ST settings.json（`prompts` + `prompt_order`），与 ST 网页端完全互通。

## ST 事实（openai.js / PromptManager.js 实测）

- settings.json 两个键：
  - `prompts`: 条目数组 `{identifier, name, system_prompt, role, content, marker?, injection_position?, injection_depth?}`
  - `prompt_order`: `[{character_id, order: [{identifier, enabled}]}]`；全局默认 character_id = **100001**（dummyId）
- 默认 order（PromptManager.js:2087-2136）：main → worldInfoBefore → personaDescription → charDescription → charPersonality → scenario → enhanceDefinitions(关) → nsfw → worldInfoAfter → dialogueExamples → chatHistory → jailbreak
- 另有 `settings.impersonation_prompt`（代写指令模板，宏 {{user}}/{{char}}）

## 方案（App 侧简化版，数据格式全互通）

### 1. 服务 `services/st/promptmanager.ts`
- `loadPm(force?)`：从 settings 读 `prompts` + `prompt_order`（找 character_id=100001，缺则用默认 order 创建于内存）
- `savePm({main, nsfw, jailbreak}, order)`：读全量 → 合并 prompts 三槽 + prompt_order[100001] → 比对一致不写 → 写全量
- `DEFAULT_ORDER`：对齐 PromptManager.js 默认（App 支持的标识子集）
- 标识 ↔ App 能力映射表（不支持的标识如 enhanceDefinitions 走跳过）：
  main / nsfw / jailbreak / worldInfoBefore / worldInfoAfter / personaDescription /
  charDescription / charPersonality / scenario / dialogueExamples / chatHistory

### 2. prompt.ts
- `BuildPromptOptions.pm?: { order: {identifier, enabled}[] }`
- `buildSystemPrompt` 重构：当传入 pm 时，**按 order 顺序**从「标识 → 内容」映射取块拼接
  （worldInfoBefore→wi.before、personaDescription→人设、charDescription/charPersonality/scenario→卡字段、
  nsfw/jailbreak→三槽内容、main→主指令、dialogueExamples→示例）；
  未传 pm 或 order 缺失 → 走现有固定装配（完全向后兼容）
- `withImpersonate` 支持自定义指令模板（宏替换），缺省用内置文案

### 3. chat.ts
- `assemblePrompt` 里 `const pm = await getPmCached()` 传入 buildPrompt
- `loadStUserName`（已读全量 settings）顺带取 `impersonation_prompt` 存入 state，impersonate 使用

### 4. UI（设置 → 模型 新卡片「提示词管理」）
- 三个文本域：主提示词(main) / NSFW / 越狱(jailbreak)
- 顺序列表：App 支持的标识（中文名）+ 启用勾选 + 上移/下移（操作 prompt_order）
- 保存（写回 ST settings.json）/ 重置为默认顺序
- 提示文案：与 ST 网页端 Prompt Manager 同一份数据

### 5. 自检
- 离线断言：order 行走顺序产出正确块序列、enabled=false 跳过、未传 pm 走旧装配、
  自定义 main 内容生效、enhanceDefinitions 跳过、impersonation 模板宏替换

## 明确不做（B2 范围外）
- 任意条目的增删改（ST 全量 prompt 条目管理器）→ 后续批次
- injection_position/depth 的逐条目注入（现有 AN/世界书 @Depth 已覆盖主要场景）
- per-character prompt_order（App 单角色会话模型，用全局 100001）
