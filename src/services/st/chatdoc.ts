/**
 * 会话文档辅助（ST JSONL 结构 ↔ 界面模型）
 *
 * ST 的会话文件是 JSONL：首行是元数据头（无 mes 字段），其后每行一条消息。
 * 实测消息形状：{ name, is_user, send_date, mes, extra }
 * 元数据头形状：{ chat_metadata, user_name, character_name }
 */
import type { StCharacter, StChatMessage } from './types'

/** 是否为元数据头（有 chat_metadata 且无正文） */
export function isChatHeader(m: StChatMessage): boolean {
  return !!m.chat_metadata && m.mes === undefined
}

/** 界面用消息模型 */
export interface DisplayMessage {
  role: 'user' | 'assistant'
  content: string
  /** 显示名（角色名 / 用户名） */
  name: string
  /** 毫秒时间戳 */
  sendDate?: number
  /** 生成失败时的错误文本 */
  error?: boolean
  /** 发送中（流式占位） */
  pending?: boolean
  /**
   * 原始扩展字段（ST 侧放 swipes/reasoning/bookmark_link 等）。
   * 写回时原样保留——避免全量保存抹掉 ST 侧扩展数据。
   */
  extra?: Record<string, unknown>
  /** 对应 jsonl 行号（0 = 元数据头）；行级增量落盘用，未知为 undefined */
  lineIndex?: number
  /** 推理/思考内容（读自 extra.reasoning） */
  reasoning?: string
  /** swipe 备选回复；存在时 content === swipes[swipeId] */
  swipes?: string[]
  swipeId?: number
  /** 书签（checkpoint）链接：该消息关联的会话文件名 */
  bookmarkLink?: string
  /** 隐藏（is_system）：不进 prompt，UI 半透明显示 */
  isSystem?: boolean
}

/**
 * send_date 兼容解析：
 * - 数字：< 1e12 视为秒级，否则毫秒
 * - 字符串：ISO 或可被 Date.parse 识别的格式
 */
export function parseSendDate(v?: string | number): number | undefined {
  if (v === undefined || v === null || v === '') return undefined
  if (typeof v === 'number') {
    if (!Number.isFinite(v) || v <= 0) return undefined
    return v < 1e12 ? v * 1000 : v
  }
  const n = Number(v)
  if (Number.isFinite(n) && n > 1e6) return n < 1e12 ? n * 1000 : n
  // humanized 格式回退解析（`September 28, 2026 3:04pm` —— 非标准格式，
  // 部分 JS 引擎的 Date.parse 不认，显式解析保证双向兼容）
  const hm = /^([A-Za-z]+) (\d{1,2}), (\d{4}) (\d{1,2}):(\d{2})(am|pm)$/i.exec(v.trim())
  if (hm) {
    const months = [
      'january', 'february', 'march', 'april', 'may', 'june',
      'july', 'august', 'september', 'october', 'november', 'december',
    ]
    const mi = months.indexOf(hm[1]!.toLowerCase())
    if (mi >= 0) {
      let h = Number(hm[4]) % 12
      if (hm[6]!.toLowerCase() === 'pm') h += 12
      return new Date(Number(hm[3]), mi, Number(hm[2]), h, Number(hm[5])).getTime()
    }
  }
  const t = Date.parse(v)
  return Number.isNaN(t) ? undefined : t
}

/**
 * 把 ST 消息数组转成界面模型。
 * 过滤：元数据头、空正文。is_system 行保留（但不进 prompt）。
 */
export function toDisplayMessages(
  raw: StChatMessage[],
  opts: { userFallback?: string; charFallback?: string } = {},
): DisplayMessage[] {
  const userFallback = opts.userFallback ?? '我'
  const charFallback = opts.charFallback ?? '角色'
  const out: DisplayMessage[] = []

  for (let i = 0; i < raw.length; i++) {
    const m = raw[i]
    if (!m || isChatHeader(m)) continue
    const content = typeof m.mes === 'string' ? m.mes : ''
    if (!content.trim()) continue
    const extra =
      m.extra && typeof m.extra === 'object' ? (m.extra as Record<string, unknown>) : {}
    const swipes = Array.isArray(m.swipes)
      ? m.swipes.filter((s): s is string => typeof s === 'string')
      : undefined
    out.push({
      role: m.is_user ? 'user' : 'assistant',
      content,
      name: m.name || (m.is_user ? userFallback : charFallback),
      sendDate: parseSendDate(m.send_date),
      extra,
      lineIndex: i,
      reasoning: typeof extra.reasoning === 'string' ? extra.reasoning : undefined,
      swipes,
      swipeId: typeof m.swipe_id === 'number' ? m.swipe_id : undefined,
      bookmarkLink: typeof extra.bookmark_link === 'string' ? extra.bookmark_link : undefined,
      isSystem: m.is_system === true,
    })
  }
  return out
}

/** 界面模型 → ST 消息（写回用）。保留 extra/swipes 等扩展字段，避免覆盖 ST 侧数据 */
export function toStMessages(
  msgs: DisplayMessage[],
  characterName: string,
): StChatMessage[] {
  return msgs.map((m) => {
    const base: StChatMessage = {
      // is_system 旁白行保留自身名字（此前被改写成角色名）
      name: m.role === 'user' || m.isSystem ? m.name : characterName,
      is_user: m.role === 'user',
      // ST 网页端为 humanized 格式（parseSendDate 双向兼容）
      send_date: humanizedSendDate(new Date(m.sendDate ?? Date.now())),
      mes: m.content,
      extra: m.extra ?? {},
    }
    if (m.isSystem) base.is_system = true
    // reasoning 是 extra.reasoning 的镜像字段：有值时写回
    if (m.reasoning) base.extra = { ...base.extra, reasoning: m.reasoning }
    if (m.swipes && m.swipes.length) {
      base.swipes = m.swipes
      base.swipe_id = m.swipeId ?? 0
    }
    return base
  })
}

/** ST 网页端的 send_date humanized 格式：`September 28, 2026 3:04pm` */
export function humanizedSendDate(d = new Date()): string {
  const months = [
    'January', 'February', 'March', 'April', 'May', 'June',
    'July', 'August', 'September', 'October', 'November', 'December',
  ]
  const h24 = d.getHours()
  const h = h24 % 12 === 0 ? 12 : h24 % 12
  const min = String(d.getMinutes()).padStart(2, '0')
  return `${months[d.getMonth()]} ${d.getDate()}, ${d.getFullYear()} ${h}:${min}${h24 >= 12 ? 'pm' : 'am'}`
}

/** ST 会话时间戳格式：`2023-5-12 @21h 32m 29s 224ms` */
export function stTimestamp(d = new Date()): string {
  const p = (n: number) => String(n)
  return (
    `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ` +
    `@${p(d.getHours())}h ${p(d.getMinutes())}m ${p(d.getSeconds())}s ${p(d.getMilliseconds())}ms`
  )
}

/** ST 新建会话的默认文件名：`<角色名> - <时间戳>` */
export function defaultChatName(characterName: string, d = new Date()): string {
  return `${characterName} - ${stTimestamp(d)}`
}

/**
 * 新建会话的初始文档：
 * 元数据头 + 开场白（first_mes）作为第一条角色消息。
 * 这样在 ST 侧打开同一会话也能看到开场白。
 */
export function createChatSeed(
  character: StCharacter,
  userName: string,
  charName?: string,
  cardFileName?: string,
): StChatMessage[] {
  const name = charName ?? character.name
  const header: StChatMessage = {
    chat_metadata: cardFileName ? { integrity: cardFileName } : {},
    user_name: userName,
    character_name: name,
  }
  const messages: StChatMessage[] = []
  const first = String(character.first_mes ?? '').trim()
  if (first) {
    messages.push({
      name,
      is_user: false,
      send_date: new Date().toISOString(),
      mes: first,
      extra: {},
    })
  }
  return [header, ...messages]
}

/* ---------------- 会话级生成参数覆盖（chat_metadata.app_gen） ---------------- */

function asDict(v: unknown): Record<string, unknown> {
  return v && typeof v === 'object' ? (v as Record<string, unknown>) : {}
}

/**
 * 会话级生成参数覆盖。ST 的 chat_metadata 是自由结构，人设锁定（persona）之外，
 * 本 App 用 app_gen 键存本会话专属的采样参数（生成时优先于全局设置）。
 */
export interface ChatGenOverride {
  temperature?: number
  topP?: number
  maxTokens?: number
}

/** 读会话的生成参数覆盖（无则 null） */
export function chatGenOverride(chat: unknown[]): ChatGenOverride | null {
  if (!chat.length) return null
  const head = asDict(chat[0])
  const meta = asDict(head.chat_metadata)
  const g = meta.app_gen
  if (!g || typeof g !== 'object') return null
  const o = g as Record<string, unknown>
  const num = (v: unknown): number | undefined =>
    typeof v === 'number' && Number.isFinite(v) ? v : undefined
  const out: ChatGenOverride = {
    temperature: num(o.temperature),
    topP: num(o.topP),
    maxTokens: num(o.maxTokens),
  }
  return out.temperature !== undefined || out.topP !== undefined || out.maxTokens !== undefined
    ? out
    : null
}

/** 写会话的生成参数覆盖（传入 null = 清除覆盖，回到跟随全局） */
export function applyChatGenOverride(
  chat: unknown[],
  override: ChatGenOverride | null,
): void {
  if (!chat.length) return
  const head = asDict(chat[0])
  const meta = asDict(head.chat_metadata)
  if (override) meta.app_gen = override
  else delete meta.app_gen
  head.chat_metadata = meta
  chat[0] = head as (typeof chat)[number]
}

/* ---------------- 作者注释（chat_metadata.note_*） ---------------- */

/**
 * 作者注释（Author's Note）。同一会话在 ST 网页端打开时可直接看到/编辑同一份注释。
 */
export interface AuthorsNote {
  /** 注释正文（{{char}}/{{user}} 宏在组装时替换） */
  prompt: string
  /** 每 N 条消息插入一次；1 = 每条都插（ST DEFAULT_INTERVAL = 1） */
  interval: number
  /** @Depth 插入深度（仅 position = 1 生效，ST DEFAULT_DEPTH = 4） */
  depth: number
  /** 注入位置：0 角色定义后（scenario）/ 1 聊天内 @Depth（默认）/ 2 角色定义前（before） */
  position: number
  /** @Depth 时的角色：0 system / 1 user / 2 assistant */
  role: number
}

/** ST 默认值 */
export const AN_DEFAULTS: Omit<AuthorsNote, 'prompt'> = {
  interval: 1,
  depth: 4,
  position: 1,
  role: 0,
}

/** 读会话的作者注释（未设置或正文为空 → null） */
export function chatAuthorsNote(chat: unknown[]): AuthorsNote | null {
  if (!chat.length) return null
  const meta = asDict(asDict(chat[0]).chat_metadata)
  const prompt = typeof meta.note_prompt === 'string' ? meta.note_prompt : ''
  if (!prompt.trim()) return null
  const num = (v: unknown, fallback: number, min: number): number =>
    typeof v === 'number' && Number.isFinite(v) ? Math.max(min, v) : fallback
  const pick = (v: unknown, fallback: number, max: number): number =>
    typeof v === 'number' && Number.isInteger(v) && v >= 0 && v <= max ? v : fallback
  return {
    prompt,
    interval: num(meta.note_interval, AN_DEFAULTS.interval, 1),
    depth: num(meta.note_depth, AN_DEFAULTS.depth, 0),
    position: pick(meta.note_position, AN_DEFAULTS.position, 2),
    role: pick(meta.note_role, AN_DEFAULTS.role, 2),
  }
}

/** 写会话的作者注释（传入 null = 清除全部 note_* 键，回到 ST 默认行为） */
export function applyAuthorsNote(chat: unknown[], note: AuthorsNote | null): void {
  if (!chat.length) return
  const head = asDict(chat[0])
  const meta = asDict(head.chat_metadata)
  const keys = ['note_prompt', 'note_interval', 'note_depth', 'note_position', 'note_role']
  if (note) {
    meta.note_prompt = note.prompt
    meta.note_interval = note.interval
    meta.note_depth = note.depth
    meta.note_position = note.position
    meta.note_role = note.role
  } else {
    for (const k of keys) delete meta[k]
  }
  head.chat_metadata = meta
  chat[0] = head as (typeof chat)[number]
}

/* ---------------- 总结记忆 / timedEffects（chat_metadata） ---------------- */

/** 读会话摘要（chat_metadata.summary） */
export function chatSummary(chat: unknown[]): string {
  if (!chat.length) return ''
  const meta = asDict(asDict(chat[0]).chat_metadata)
  return typeof meta.summary === 'string' ? meta.summary : ''
}

/** 写会话摘要（空串 = 删除键） */
export function applySummary(chat: unknown[], summary: string): void {
  if (!chat.length) return
  const head = asDict(chat[0])
  const meta = asDict(head.chat_metadata)
  if (summary.trim()) meta.summary = summary
  else delete meta.summary
  head.chat_metadata = meta
  chat[0] = head as (typeof chat)[number]
}

export interface TimedWorldInfoEffect {
  hash: number
  start: number
  end: number
  protected?: boolean
}

export interface TimedWorldInfo {
  sticky?: Record<string, TimedWorldInfoEffect>
  cooldown?: Record<string, TimedWorldInfoEffect>
}

/** 读宏变量（chat_metadata.variables） */
export function chatVariablesOf(chat: unknown[]): Record<string, unknown> {
  if (!chat.length) return {}
  const meta = asDict(asDict(chat[0]).chat_metadata)
  const v = meta.variables
  return v && typeof v === 'object' ? (v as Record<string, unknown>) : {}
}

/** 读 timedWorldInfo */
export function chatTimedEffects(chat: unknown[]): TimedWorldInfo {
  if (!chat.length) return {}
  const meta = asDict(asDict(chat[0]).chat_metadata)
  const raw = meta.timedWorldInfo
  if (!raw || typeof raw !== 'object') return {}
  const out: TimedWorldInfo = {}
  for (const type of ['sticky', 'cooldown'] as const) {
    const bucket = asDict((raw as Record<string, unknown>)[type])
    const cleaned: Record<string, TimedWorldInfoEffect> = {}
    for (const [k, v] of Object.entries(bucket)) {
      if (v && typeof v === 'object') {
        const o = v as Record<string, unknown>
        if (typeof o.start === 'number' && typeof o.end === 'number') {
          cleaned[k] = {
            hash: typeof o.hash === 'number' ? o.hash : 0,
            start: o.start,
            end: o.end,
            protected: o.protected === true,
          }
        }
      }
    }
    if (Object.keys(cleaned).length) out[type] = cleaned
  }
  return out
}

/** 写 timedWorldInfo（空结构 = 删除键） */
export function applyTimedEffects(chat: unknown[], state: TimedWorldInfo): void {
  if (!chat.length) return
  const head = asDict(chat[0])
  const meta = asDict(head.chat_metadata)
  if (Object.keys(state).length) meta.timedWorldInfo = state
  else delete meta.timedWorldInfo
  head.chat_metadata = meta
  chat[0] = head as (typeof chat)[number]
}
