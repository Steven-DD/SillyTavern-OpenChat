/**
 * Prompt 组装引擎（M3 · 自研核心）
 *
 * ── 为什么必须自研 ──
 * ST 的 prompt 组装（prompt manager / 世界书扫描 / 深度插入）跑在它的**浏览器前端**，
 * 后端只做转发。我们把 ST 当纯后端用 → 组装必须自己实现。
 * 《ST_API能力盘点 v1.0》第六节 P0 项。
 *
 * ── 与 ST 的差异（已知、有意为之）──
 * 已实现 ST 默认管线的核心：系统提示（角色描述/性格/场景/对话示例）+ 对话历史
 *   + 世界书扫描注入 + 作者注释（position/depth/interval/role）+ 人设注入。
 * 暂未实现：prompt 条目自定义排序/深度（Prompt Manager）、群聊。
 *
 * ── 实测约束（决定本文件的写法，勿随意改）──
 * 1. 不传 `use_sysprompt`（保持 falsy）：ST 的 convertGooglePrompt 只在它为真时
 *    才把 system 放进 systemInstruction，否则把 system 折成**首个 user 轮**。
 *    Google 侧对「首轮为 model」有 prefill 校验会报错 —— 我们的历史以开场白（assistant）
 *    开头，所以必须靠 system 顶到第一轮，保证 contents[0].role === 'user'。
 *    见 sillytavern/src/prompt-converters.js:435-451 / 462-467
 * 2. messages 里**不能带 name 字段**：convertGooglePrompt:489-510 会把 name 拼成
 *    `name: ` 前缀塞进正文（那是给 example_user/example_assistant 用的）。
 * 3. ST 会自动合并相邻同角色消息（:599-619），我们不必自己合并。
 */
import type { StChatMessage, StCharacter } from './types'
import { isChatHeader, parseSendDate, type AuthorsNote } from './chatdoc'
import { emptyContext, getGlobalVarStore, runMacros, type MacroContext, type MacroVariables } from './macros'

export interface PromptMessage {
  role: 'system' | 'user' | 'assistant'
  content: string
}

export interface BuildPromptOptions {
  character: StCharacter
  /** 会话消息（含元数据头与开场白，顺序为旧→新） */
  history: StChatMessage[]
  /** 用户名（ST settings.username），用于 {{user}} 宏 */
  userName: string
  /** 上下文上限（token），默认 8192（对应 ST max_context） */
  maxContext?: number
  /** 为模型回复预留的 token */
  reserveForReply?: number
  /** 是否附带对话示例（few-shot）。示例通常很长，紧张时可关 */
  includeExamples?: boolean
  /** 生效人设（会话锁定或默认人设）；null = 无注入 */
  persona?: PersonaInjection | null
  /** 世界书注入（checkWorldInfo 的结果；不传 = 不注入） */
  worldInfo?: WiPlacement
  /** 作者注释（chat_metadata.note_*；null/不传 = 不注入） */
  authorsNote?: AuthorsNote | null
  /** 宏引擎附加上下文（变量存储/模型等；不传用默认值） */
  macro?: {
    model?: string
    maxResponse?: number
    swipeId?: number
    /**
     * 会话级变量存储（chat_metadata.variables 的 store）。
     * P1 统一：调用方必须传 store 本体而非存储键 —— 此前经 sessionKey 走
     * localStorage 副本，与 STscript/正则通路的 chat_metadata 变量互不可见。
     */
    chatVars?: MacroVariables
  }
  /** Prompt Manager 配置（prompt_order + 三槽内容；不传走传统固定装配） */
  pm?: PmConfig
  /** 总结记忆注入（B3 M-6）：position 0 = 系统提示顶部；1 = @Depth */
  memory?: { content: string; position: number; depth: number; role: number }
  /** 向量检索注入（B3 M-7）：@Depth system 块 */
  vectors?: { content: string; depth: number }
}

/** 世界书注入材料（worldinfo.ts checkWorldInfo 的产出） */
export interface WiPlacement {
  /** ↑Char：角色定义前的整体文本 */
  before: string
  /** ↓Char：角色定义后的整体文本 */
  after: string
  /** @Depth：按深度插进聊天消息流 */
  depthEntries?: { depth: number; role: 'system' | 'user' | 'assistant'; content: string }[]
}

export interface PromptResult {
  messages: PromptMessage[]
  systemPrompt: string
  /** 估算输入 token */
  inputTokens: number
  /** 因超上下文被裁掉的历史条数 */
  trimmed: number
  /** 对话示例因上下文紧张被丢弃 */
  examplesDropped: boolean
  /** {{banned}} 宏收集的禁词（Text Completion 透传用，通常为空） */
  bannedWords: string[]
}

/* ------------------------------------------------------------------ *
 * 宏替换
 * ------------------------------------------------------------------ */

/**
 * ST 角色卡宏替换：
 * - {{char}} / {{user}}（官方），<BOT> / <USER>（旧卡兼容）
 * - {{time}} / {{date}}：生成时刻的时间/日期
 * - {{roll}} / {{roll:dN}}：掷骰（默认 1-100）
 * - {{random:a,b,c}}：每次随机取一项；{{pick:a,b,c}}：按文本稳定取一项（近似 ST 的确定性 pick）
 */
export function substituteMacros(
  text: string,
  charName: string,
  userName: string,
): string {
  if (!text) return ''
  let out = text
    .replace(/\{\{char\}\}/gi, charName)
    .replace(/\{\{user\}\}/gi, userName)
    .replace(/<BOT>/gi, charName)
    .replace(/<USER>/gi, userName)

  const now = new Date()
  const p2 = (n: number) => String(n).padStart(2, '0')
  out = out
    .replace(/\{\{time\}\}/gi, `${p2(now.getHours())}:${p2(now.getMinutes())}`)
    .replace(/\{\{date\}\}/gi, `${now.getFullYear()}-${p2(now.getMonth() + 1)}-${p2(now.getDate())}`)

  out = out.replace(/\{\{\s*roll\s*(?::\s*d?(\d+)\s*)?\}\}/gi, (_, n) => {
    const max = Math.max(1, Math.min(1000, Number(n) || 100))
    return String(1 + Math.floor(Math.random() * max))
  })

  out = out.replace(/\{\{\s*(random|pick)\s*:\s*([^{}]+)\}\}/gi, (_w, kind: string, list: string) => {
    const items = String(list)
      .split(',')
      .map((s) => s.trim())
      .filter(Boolean)
    if (!items.length) return ''
    if (kind.toLowerCase() === 'random') return items[Math.floor(Math.random() * items.length)]
    // pick：按整段文本 hash 稳定选取，同一文本多次组装结果一致
    let h = 0
    for (let i = 0; i < text.length; i++) h = (h * 31 + text.charCodeAt(i)) >>> 0
    return items[h % items.length]
  })

  return out
}

/* ------------------------------------------------------------------ *
 * Token 估算
 * ------------------------------------------------------------------ */

/**
 * 粗略 token 估算（用于上下文裁剪，不做精确计数）：
 * - CJK 字符 ≈ 1 token/字
 * - 其它（拉丁字母/数字/符号）≈ 4 字符/token
 * 精度要求不高，宁可略高估（多裁一点比超限报错好）。
 */
export function estimateTokens(text: string): number {
  let cjk = 0
  let other = 0
  for (const ch of text) {
    if (/[\u2e80-\u9fff\uf900-\ufaff\uff00-\uffef\u3000-\u303f]/.test(ch)) cjk++
    else other++
  }
  return Math.ceil(cjk + other / 4)
}

/* ------------------------------------------------------------------ *
 * 系统提示组装（唯一注入点 —— 未来世界书/作者注释都挂这里）
 * ------------------------------------------------------------------ */

/**
 * 人设注入参数（对齐 ST 的 persona_description_positions）
 * - 0 IN_PROMPT：拼进系统提示
 * - 2 TOP_AN / 3 BOTTOM_AN：折进系统提示首/尾（ST 中相对作者注释块的上/下位；
 *   App 侧未按 AN 块相对定位，属已知近似）
 * - 4 AT_DEPTH：作为独立消息按深度插入历史
 * - 9 NONE：不注入
 */
export interface PersonaInjection {
  name: string
  description: string
  position: number
  depth: number
  /** 0 system / 1 user / 2 assistant（仅 AT_DEPTH 用） */
  role: number
}

/** Prompt Manager 配置（数据来自 settings.json prompts/prompt_order，见 services/st/promptmanager.ts） */
export interface PmConfig {
  order: { identifier: string; enabled: boolean }[]
  main?: string
  nsfw?: string
  jailbreak?: string
}

/** 系统提示装配结果：pre = 历史前的 system 块；post = chatHistory 之后的块（逐条 system 消息，对齐 ST post-history 位） */
export interface SystemParts {
  pre: string[]
  post: string[]
}

/**
 * 系统提示装配。
 * - 传 pm → 按 prompt_order 顺序行走（chatHistory 之后的块进 post，对齐 ST post-history 位）
 * - 不传 pm → 传统固定装配（完全向后兼容）
 */
export function buildSystemParts(
  character: StCharacter,
  userName: string,
  includeExamples = true,
  persona?: PersonaInjection | null,
  worldInfo?: WiPlacement,
  authorsNote?: AuthorsNote | null,
  macroCtx?: MacroContext,
  pm?: PmConfig,
): SystemParts {
  const char = character.name
  const sub = (t?: string) =>
    macroCtx
      ? runMacros(String(t ?? '').trim(), macroCtx)
      : substituteMacros(String(t ?? '').trim(), char, userName)

  const mainText =
    pm?.main?.trim() ||
    `Write ${char}'s next reply in a fictional roleplay between ${char} and ${userName}. ` +
      `Stay in character, write in third person past tense, and never write ${userName}'s lines or actions.`

  // 各标识 → 块内容（宏已展开；空块由调用方跳过）
  const blockFor = (id: string): string => {
    switch (id) {
      case 'main':
        return mainText
      case 'worldInfoBefore':
        return sub(worldInfo?.before)
      case 'personaDescription':
        return persona && persona.position !== 4 && persona.position !== 9 ? sub(persona.description) : ''
      case 'charDescription':
        return sub(character.description)
      case 'charPersonality':
        return sub(character.personality)
      case 'scenario':
        return sub(character.scenario)
      case 'nsfw':
        return sub(pm?.nsfw)
      case 'jailbreak':
        return sub(pm?.jailbreak)
      case 'worldInfoAfter':
        return sub(worldInfo?.after)
      case 'dialogueExamples':
        return includeExamples ? sub(character.mes_example) : ''
      default:
        return '' // enhanceDefinitions 等无 App 对应块 → 跳过
    }
  }

  /* ---- PM 有序装配 ---- */
  if (pm?.order?.length) {
    const pre: string[] = []
    const post: string[] = []
    let seenHistory = false
    for (const item of pm.order) {
      if (!item.enabled) continue
      if (item.identifier === 'chatHistory') {
        seenHistory = true // chatHistory 之后条目 → post-history（对齐 ST）
        continue
      }
      const content = blockFor(item.identifier)
      if (!content.trim()) continue
      ;(seenHistory ? post : pre).push(content)
    }
    return { pre, post }
  }

  /* ---- 传统固定装配（与历史版本一致） ---- */
  const blocks: string[] = []

  blocks.push(mainText)

  if (worldInfo?.before?.trim()) blocks.push(worldInfo.before.trim())

  const pDesc = persona && persona.position !== 4 && persona.position !== 9
    ? sub(persona.description)
    : ''
  if (pDesc && persona && persona.position === 2) blocks.push(`### About ${userName}
${pDesc}`)

  if (authorsNote?.prompt.trim() && authorsNote.position === 2) {
    blocks.push(sub(authorsNote.prompt))
  }

  const description = sub(character.description)
  if (description) blocks.push(description)

  const personality = sub(character.personality)
  if (personality) blocks.push(`### Personality\n${personality}`)

  const scenario = sub(character.scenario)
  if (scenario) blocks.push(`### Scenario\n${scenario}`)

  if (worldInfo?.after?.trim()) blocks.push(worldInfo.after.trim())

  if (authorsNote?.prompt.trim() && authorsNote.position === 0) {
    blocks.push(sub(authorsNote.prompt))
  }

  if (pDesc && persona && persona.position === 0) blocks.push(`### About ${userName}
${pDesc}`)

  if (includeExamples) {
    const examples = sub(character.mes_example)
    if (examples) {
      blocks.push(`### Example dialogue\n${examples}`)
    }
  }

  return { pre: blocks, post: [] }
}

/** 兼容包装：仅返回历史前系统提示（post 部分由 buildPrompt 处理） */
export function buildSystemPrompt(
  character: StCharacter,
  userName: string,
  includeExamples = true,
  persona?: PersonaInjection | null,
  worldInfo?: WiPlacement,
  authorsNote?: AuthorsNote | null,
  macroCtx?: MacroContext,
  pm?: PmConfig,
): string {
  return buildSystemParts(
    character,
    userName,
    includeExamples,
    persona,
    worldInfo,
    authorsNote,
    macroCtx,
    pm,
  ).pre.join('\n\n')
}

/* ------------------------------------------------------------------ *
 * 主组装
 * ------------------------------------------------------------------ */

/** 历史消息 → OpenAI 风格 messages（不含 system） */
function historyToMessages(
  history: StChatMessage[],
  charName: string,
  userName: string,
  macroCtx?: MacroContext,
): PromptMessage[] {
  const out: PromptMessage[] = []
  for (const m of history) {
    if (!m || isChatHeader(m)) continue
    // is_system（隐藏消息）不进 prompt（对齐 ST script.js:1851 usableMessages 过滤）
    if (m.is_system === true) continue
    const raw = typeof m.mes === 'string' ? m.mes : ''
    // 按消息顺序展开宏（setvar 等副作用与 ST 的文档顺序一致）
    const content = (
      macroCtx ? runMacros(raw, macroCtx) : substituteMacros(raw, charName, userName)
    ).trim()
    if (!content) continue
    // 填充宏上下文历史（{{lastMessage}}/{{input}} 等按已见消息取值）
    if (macroCtx) {
      macroCtx.history.push({ role: m.is_user ? 'user' : 'assistant', content })
      if (m.is_user) {
        const ts = parseSendDate(m.send_date as string | number | undefined)
        if (ts !== undefined) macroCtx.lastUserSendDate = ts
      }
    }
    out.push({ role: m.is_user ? 'user' : 'assistant', content })
  }
  return out
}

/**
 * 组装完整 prompt。
 *
 * 裁剪策略：
 *   1. 系统提示 + 示例若已吃掉预算 70% 以上 → 先丢对话示例（最长的可裁剪块）
 *   2. 仍超预算 → 从**最旧**的历史开始丢，直到装入 budget
 *   3. 至少保留最后 1 条历史（否则模型看不到本轮输入）
 */
export function buildPrompt(opts: BuildPromptOptions): PromptResult {
  const {
    character,
    history,
    userName,
    maxContext = 8192,
    includeExamples = true,
    persona = null,
    worldInfo,
    authorsNote,
  } = opts

  const reserve =
    opts.reserveForReply ?? Math.min(1024, Math.max(256, Math.floor(maxContext * 0.25)))
  const budget = Math.max(512, maxContext - reserve)

  // 宏引擎上下文（变量存储：有会话键 → localStorage 持久；无 → 瞬态）
  const macroCtx: MacroContext = emptyContext({
    charName: character.name,
    userName,
    persona: persona?.description ?? '',
    charDescription: String(character.description ?? ''),
    charPersonality: String(character.personality ?? ''),
    charScenario: String(character.scenario ?? ''),
    charCreatorNotes: String(character.creatorcomment ?? ''),
    charFirstMessage: String(character.first_mes ?? ''),
    charVersion:
      typeof character.character_version === 'string'
        ? character.character_version
        : String(
            (character.data as { character_version?: string } | undefined)?.character_version ?? '',
          ),
    mesExamples: String(character.mes_example ?? ''),
    model: opts.macro?.model ?? '',
    maxContext,
    maxResponse: opts.macro?.maxResponse ?? 2048,
    swipeId: opts.macro?.swipeId ?? 0,
    chatVars: opts.macro?.chatVars ?? emptyContext().chatVars,
    globalVars: getGlobalVarStore(),
  })

  const historyMsgs = historyToMessages(history, character.name, userName, macroCtx)

  // 作者注释频率门（对齐 ST authors-note.js:346-361）：每 interval 条消息插一次，
  // interval = 1 恒成立；消息数尚未攒到 interval 时不插
  const anActive = (() => {
    if (!authorsNote?.prompt.trim()) return false
    const interval = Math.max(1, Math.floor(authorsNote.interval))
    return historyMsgs.length > 0 && historyMsgs.length % interval === 0
  })()
  const anForSystem = anActive && authorsNote && authorsNote.position !== 1 ? authorsNote : null

  // 先用完整系统提示试；过宽则降级为不含示例的版本
  const parts = buildSystemParts(
    character,
    userName,
    includeExamples,
    persona,
    worldInfo,
    anForSystem,
    macroCtx,
    opts.pm,
  )
  let sys = parts.pre.join('\n\n')
  // 总结记忆（position 0 = 系统提示顶部）
  if (opts.memory && opts.memory.content.trim() && opts.memory.position === 0) {
    sys = `${opts.memory.content.trim()}\n\n${sys}`
  }
  let examplesDropped = false
  const hasExamples = includeExamples && !!String(character.mes_example ?? '').trim()
  if (hasExamples && estimateTokens(sys) > budget * 0.7) {
    const lean = buildSystemParts(
      character,
      userName,
      false,
      persona,
      worldInfo,
      anForSystem,
      macroCtx,
      opts.pm,
    ).pre.join('\n\n')
    if (estimateTokens(lean) < estimateTokens(sys)) {
      sys = lean
      examplesDropped = true
    }
  }

  const sysTokens = estimateTokens(sys)
  const totalOf = (h: PromptMessage[]) =>
    sysTokens + h.reduce((n, m) => n + estimateTokens(m.content), 0)

  let kept = historyMsgs
  let trimmed = 0
  while (kept.length > 1 && totalOf(kept) > budget) {
    kept = kept.slice(1)
    trimmed++
  }

  let messages: PromptMessage[] = [
    { role: 'system', content: sys },
    ...kept,
    // Prompt Manager：chatHistory 之后的块 → post-history system 消息（对齐 ST）
    ...parts.post.map((content) => ({ role: 'system' as const, content })),
  ]

  // AT_DEPTH：人设描述作为独立消息，从末尾往前数 depth 条插入（与 ST 一致）
  if (persona && persona.position === 4 && persona.description.trim()) {
    const roleMap = ['system', 'user', 'assistant'] as const
    const role = roleMap[persona.role] ?? 'system'
    const depth = Math.max(0, Math.min(persona.depth, messages.length - 1))
    const at = Math.max(1, messages.length - depth)
    messages = [
      ...messages.slice(0, at),
      { role, content: runMacros(persona.description.trim(), macroCtx) },
      ...messages.slice(at),
    ]
  }

  // 世界书 @Depth 条目：同样从末尾往前数 depth 条插入（同 ST 的深度注入位）
  for (const d of worldInfo?.depthEntries ?? []) {
    if (!d.content.trim()) continue
    const depth = Math.max(0, Math.min(d.depth, messages.length - 1))
    const at = Math.max(1, messages.length - depth)
    messages = [...messages.slice(0, at), { role: d.role, content: d.content }, ...messages.slice(at)]
  }

  // 作者注释「聊天内 @Depth」（position 1，ST 默认位；深度/角色可配）
  if (anActive && authorsNote && authorsNote.position === 1) {
    const roleMap = ['system', 'user', 'assistant'] as const
    const role = roleMap[authorsNote.role] ?? 'system'
    const depth = Math.max(0, Math.min(authorsNote.depth, messages.length - 1))
    const at = Math.max(1, messages.length - depth)
    messages = [
      ...messages.slice(0, at),
      { role, content: runMacros(authorsNote.prompt, macroCtx).trim() },
      ...messages.slice(at),
    ]
  }

  // 总结记忆「聊天内 @Depth」（position 1 = IN_CHAT）
  if (opts.memory && opts.memory.content.trim() && opts.memory.position === 1) {
    const roleMap = ['system', 'user', 'assistant'] as const
    const role = roleMap[opts.memory.role] ?? 'system'
    const depth = Math.max(0, Math.min(opts.memory.depth, messages.length - 1))
    const at = Math.max(1, messages.length - depth)
    messages = [
      ...messages.slice(0, at),
      { role, content: opts.memory.content.trim() },
      ...messages.slice(at),
    ]
  }

  // 总结记忆「作者注释槽位」（position 2 = IN_NOTE）：按记忆 depth 插入聊天内（AN 槽位语义）
  if (opts.memory && opts.memory.content.trim() && opts.memory.position === 2) {
    const roleMap = ['system', 'user', 'assistant'] as const
    const role = roleMap[opts.memory.role] ?? 'system'
    const depth = Math.max(0, Math.min(opts.memory.depth, messages.length - 1))
    const at = Math.max(1, messages.length - depth)
    messages = [
      ...messages.slice(0, at),
      { role, content: opts.memory.content.trim() },
      ...messages.slice(at),
    ]
  }

  // 总结记忆「发送前追加」（position 3 = IN_API）：prompt 末尾追加 system 块
  if (opts.memory && opts.memory.content.trim() && opts.memory.position === 3) {
    messages.push({ role: 'system', content: opts.memory.content.trim() })
  }

  // 向量检索注入（@Depth system 块）
  if (opts.vectors && opts.vectors.content.trim()) {
    const depth = Math.max(0, Math.min(opts.vectors.depth, messages.length - 1))
    const at = Math.max(1, messages.length - depth)
    messages = [
      ...messages.slice(0, at),
      { role: 'system', content: opts.vectors.content.trim() },
      ...messages.slice(at),
    ]
  }

  return {
    messages,
    systemPrompt: sys,
    inputTokens: totalOf(kept),
    trimmed,
    examplesDropped,
    bannedWords: macroCtx.bannedWords,
  }
}

/* ------------------------------------------------------------------ *
 * 生成类型变体（Continue / Impersonate，对齐 ST 的生成类型分支）
 * ------------------------------------------------------------------ */

/** ST oai_settings.continue_nudge_prompt 默认文案（openai.js:111） */
export const DEFAULT_CONTINUE_NUDGE =
  '[Continue your last message without repeating its original content.]'

/**
 * M-3 续写（CC 通道，对齐 ST continue_prefill=false 默认行为，openai.js:906-921/1088-1091）：
 * 半截 assistant 消息保持为最后一条，continue_nudge system 提示插在其前——
 * 模型从半截内容自然续写（不再是旧实现的 [Continue] 用户轮）。
 */
export function withContinue(messages: PromptMessage[], nudge?: string): PromptMessage[] {
  const partial = messages[messages.length - 1]
  if (!partial || partial.role !== 'assistant') {
    return [...messages, { role: 'user', content: '[Continue]' }]
  }
  return [
    ...messages.slice(0, -1),
    { role: 'system', content: nudge || DEFAULT_CONTINUE_NUDGE },
    partial,
  ]
}

/** ST settings.impersonation_prompt 默认文案（openai.js:105） */
export const DEFAULT_IMPERSONATE_PROMPT =
  "[Write your next reply from the point of view of {{user}}, using the chat history so far as a guideline for the writing style of {{user}}. Don't write as {{char}} or system. Don't describe actions of {{char}}.]"

/** M-4 代写：以用户身份生成下一条消息（支持 ST settings.impersonation_prompt 模板 + 宏替换） */
export function withImpersonate(
  messages: PromptMessage[],
  userName: string,
  template?: string,
  macroCtx?: MacroContext,
): PromptMessage[] {
  const tpl = template?.trim()
  if (tpl) {
    const content = macroCtx ? runMacros(tpl, macroCtx) : tpl
    return [...messages, { role: 'system', content }]
  }
  // 无模板回落 ST 默认 impersonation prompt（system 轮，P1-7 对齐）
  return [...messages, { role: 'system', content: DEFAULT_IMPERSONATE_PROMPT.replace(/{{user}}/gi, userName) }]
}
