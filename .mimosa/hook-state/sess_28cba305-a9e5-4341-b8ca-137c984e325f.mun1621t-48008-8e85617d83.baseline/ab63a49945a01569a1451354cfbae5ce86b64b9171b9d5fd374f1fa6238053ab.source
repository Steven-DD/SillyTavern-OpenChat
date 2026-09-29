/**
 * 群聊发言编排（纯函数）
 *
 * 核心事实（以 ST 源码为准）：
 * - 发言人数**只由 activation_strategy 决定**（generateGroupWrapper:1002-1031）；
 *   generation_mode（SWAP/APPEND/APPEND_DISABLED）只影响成员卡片合并（:439-441, :501）
 *   与发言编排无关。
 * - 策略枚举（:122-127）：NATURAL:0 / LIST:1 / MANUAL:2 / POOLED:3。
 *   - NATURAL(0)：@mentions 优先（activateNaturalOrder:1254-1268）+ talkativeness 掷骰
 *     （乱序，roll ≤ talkativeness 激活，:1271-1291）；无人命中随机兜底 1 人（:1293-1306）；
 *     非 用户输入 触发时禁上一位发言者连说（:1246，除非 allow_self_responses）。
 *   - LIST(1)：启用成员按表序全部发言（activateListOrder:1180-1188）。
 *   - MANUAL(2)：**仅非用户输入**时 shuffle 启用成员取 1 人（:1029-1030）；
 *     用户输入触发时不激活任何人（ST 只保存用户消息，不生成回复）。
 *   - POOLED(3)：从"自上一条用户消息以来未发言"的成员随机挑 1 人（activatePooledOrder:
 *     1197-1231）；全部已发言则随机 1 人（排除上一位发言者，若成员 >1）。
 * - 所有策略都只考虑启用成员（disabled_members 排除，:1003）。
 */

/** 策略枚举 */
export const GROUP_ACTIVATION_STRATEGY = {
  NATURAL: 0,
  LIST: 1,
  MANUAL: 2,
  POOLED: 3,
} as const

export const GROUP_GENERATION_MODE = {
  SWAP: 0,
  APPEND: 1,
  APPEND_DISABLED: 2,
} as const

/** 成员元数据（avatar = 成员键，talkativeness 0~1，缺省 0.5 = ST talkativeness_default） */
export interface GroupMemberMeta {
  avatar: string
  name: string
  talkativeness?: number
}

export interface GroupPlanInput {
  activationStrategy: number
  members: string[]
  disabledMembers?: string[]
  /** 成员元表（缺 talkativeness 用默认值） */
  meta?: Record<string, GroupMemberMeta>
  /** 是否由用户输入触发（false = auto 模式 / 重新生成等无输入触发） */
  isUserInput?: boolean
  /** 触发文本：用户输入正文，或 auto 模式下最后一条消息正文（@mentions 匹配用） */
  activationText?: string
  /** 上一位 AI 发言者的名字（NATURAL 防连说按名匹配，allowSelfResponses 时不生效） */
  lastSpeakerName?: string
  /** 上一位 AI 发言者的 avatar（POOLED 排除用） */
  lastSpeakerAvatar?: string
  /** 自上一条用户消息以来已发言的成员 avatar 列表（POOLED 用） */
  spokenSinceUser?: string[]
  allowSelfResponses?: boolean
  /** 随机源（测试注入用，缺省 Math.random） */
  random?: () => number
}

export interface SpeakerPlan {
  avatar: string
  /** 是否掷骰兜底选中（UI 可提示） */
  forced?: boolean
}

const DEFAULT_TALKATIVENESS = 0.5

function shuffled<T>(arr: T[], random: () => number): T[] {
  const out = [...arr]
  for (let i = out.length - 1; i > 0; i--) {
    const j = Math.floor(random() * (i + 1))
    ;[out[i], out[j]] = [out[j]!, out[i]!]
  }
  return out
}

/** extractAllWords（\b\w+\b 小写词） */
function extractAllWords(value: string | undefined): string[] {
  if (!value) return []
  return [...value.matchAll(/\b\w+\b/gim)].map((m) => m[0]!.toLowerCase())
}

/** 启用成员 */
function enabledMembers(input: GroupPlanInput): string[] {
  const disabled = new Set(input.disabledMembers ?? [])
  return input.members.filter((a) => !disabled.has(a))
}

/** NATURAL(0)：mentions + talkativeness 掷骰（activateNaturalOrder:1242-1316） */
function naturalPick(input: GroupPlanInput, random: () => number): SpeakerPlan[] {
  const members = enabledMembers(input)
  // 防连说：仅非用户输入触发时禁上一位发言者（:1246）；allow_self_responses 豁免
  const bannedName =
    !input.isUserInput && input.lastSpeakerName && !input.allowSelfResponses
      ? input.lastSpeakerName
      : undefined
  const nameOf = (a: string) => input.meta?.[a]?.name ?? a.replace(/\.png$/i, '')

  const activated: string[] = []
  // @mentions 优先（:1254-1268）：触发文本的每个词命中成员名即激活
  for (const word of extractAllWords(input.activationText)) {
    for (const avatar of members) {
      const name = nameOf(avatar)
      if (name === bannedName) continue
      if (extractAllWords(name).includes(word)) {
        activated.push(avatar)
        break
      }
    }
  }

  // talkativeness 掷骰（乱序，:1271-1291）
  const chatty: string[] = []
  for (const avatar of shuffled(members, random)) {
    const name = nameOf(avatar)
    if (name === bannedName) continue
    const t = Number(input.meta?.[avatar]?.talkativeness ?? DEFAULT_TALKATIVENESS)
    const talk = Number.isNaN(t) ? DEFAULT_TALKATIVENESS : t
    if (talk >= random()) activated.push(avatar)
    if (talk > 0) chatty.push(avatar)
  }

  // 无人命中随机兜底（:1293-1306）
  if (!activated.length) {
    const pool = chatty.length ? chatty : members
    for (let retries = 0; !activated.length && retries < pool.length; retries++) {
      activated.push(pool[Math.floor(random() * pool.length)]!)
    }
  }

  // 去重（:1309）
  return [...new Set(activated)].map((avatar) => ({
    avatar,
    // ST 无此概念；保留旧 UI 提示语义：无法从掷骰/mentions 判定是否兜底，统一不标
  }))
}

/** MANUAL(2)：仅非用户输入时随机 1 人（:1029-1030）；用户输入不激活任何人 */
function manualPick(input: GroupPlanInput, random: () => number): SpeakerPlan[] {
  if (input.isUserInput) return []
  const members = enabledMembers(input)
  if (!members.length) return []
  return [{ avatar: shuffled(members, random)[0]! }]
}

/** POOLED(3)：优先未发言者，否则随机排除上一位发言者（activatePooledOrder:1197-1231） */
function pooledPick(input: GroupPlanInput, random: () => number): SpeakerPlan[] {
  const members = enabledMembers(input)
  if (!members.length) return []
  const spoken = new Set(input.spokenSinceUser ?? [])
  const haveNotSpoken = members.filter((a) => !spoken.has(a))

  let pick: string | undefined
  if (haveNotSpoken.length) {
    pick = haveNotSpoken[Math.floor(random() * haveNotSpoken.length)]!
  }
  if (!pick) {
    // 成员 >1 时排除上一位发言者（:1224-1226）
    const pool =
      members.length > 1 && input.lastSpeakerAvatar
        ? members.filter((a) => a !== input.lastSpeakerAvatar)
        : members
    pick = pool[Math.floor(random() * pool.length)]!
  }
  return pick ? [{ avatar: pick }] : []
}

/**
 * 计算本轮要生成回复的成员序列（按发言顺序）。
 * 发言人数只由 activation_strategy 决定。
 */
export function pickSpeakers(input: GroupPlanInput): SpeakerPlan[] {
  const random = input.random ?? Math.random
  switch (input.activationStrategy) {
    case GROUP_ACTIVATION_STRATEGY.LIST:
      // LIST：启用成员按表序全部发言（去重，activateListOrder）
      return [...new Set(enabledMembers(input))].map((avatar) => ({ avatar }))
    case GROUP_ACTIVATION_STRATEGY.MANUAL:
      return manualPick(input, random)
    case GROUP_ACTIVATION_STRATEGY.POOLED:
      return pooledPick(input, random)
    case GROUP_ACTIVATION_STRATEGY.NATURAL:
    default:
      return naturalPick(input, random)
  }
}

/** 群聊会话的 sessionKey 解析（`group::<groupId>::<chatId>`） */
export function parseGroupSessionKey(key: string): { groupId: string; chatId: string } | null {
  const m = /^group::(.+)::(.+)$/.exec(key)
  return m ? { groupId: m[1]!, chatId: m[2]! } : null
}

export function groupSessionKey(groupId: string, chatId: string): string {
  return `group::${groupId}::${chatId}`
}
