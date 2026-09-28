/**
 * 世界书扫描与注入（移植自 ST public/scripts/world-info.js 核心逻辑，AGPL 合规）
 *
 * 与 ST 1.19.0 对齐的行为：
 * - 激活来源：角色绑定世界书（data.extensions.world）+ 全局勾选（settings.world_info.globalSelect）
 * - 排序：角色书优先（character_first 策略），组内按 order 降序（order 大 = 优先）
 * - 匹配：正则关键词（/pattern/flags 写法）优先；普通关键词默认大小写不敏感 + 子串包含
 *   （ST 1.19 默认 match_whole_words=false；条目可单独覆盖 matchWholeWords/caseSensitive/scanDepth）
 * - 常驻条目（constant）直接激活；停用（disable）跳过
 * - selective 二级关键词：selectiveLogic 0=AND_ANY 1=NOT_ALL 2=NOT_ANY 3=AND_ALL
 * - 概率：useProbability && probability<100 时掷骰
 * - token 预算：budget = budget% × maxContext（book 可用 token_budget 覆盖；budgetCap 全局上限）；
 *   ignoreBudget 条目绕过预算（不计入占用）
 * - inclusion group：group（逗号分隔可属多组）内多条激活时只留一条 —— groupOverride 优先（order 大者），
 *   否则按 groupWeight 掷骰（world-info.js filterByInclusionGroups）
 * - 注入位置：0=↑Char（角色定义前）1=↓Char（角色定义后）2/3=作者注释上下（并入 after）4=@Depth 按深度插聊天流
 *   5/6=EM 仅 Text Completion，App 不支持 → 按 after 处理
 * - 递归扫描：g.recursive 开启时启用（默认关，与 ST 默认一致）；timedEffects
 *   （sticky/cooldown 计数 + delay 消息进度）已实现，状态经调用方持久化到 chat_metadata.timedEffects；
 *   min_activations（默认 0）、useGroupScoring（需匹配度打分）未实现
 */
import { getCharacter, getSettings, getWorld } from './data'
import { estimateTokens } from './prompt'
import type { StWorldEntry } from './types'

/** 位置枚举（world-info.js:855 world_info_position） */
export const WI_POSITION = {
  before: 0,
  after: 1,
  ANTop: 2,
  ANBottom: 3,
  atDepth: 4,
  EMTop: 5,
  EMBottom: 6,
} as const

/** 二级关键词逻辑（world-info.js:33 world_info_logic） */
const WI_LOGIC = { AND_ANY: 0, NOT_ALL: 1, NOT_ANY: 2, AND_ALL: 3 } as const

/** @Depth 默认插入深度（world-info.js:96 DEFAULT_DEPTH） */
const DEFAULT_DEPTH = 4

/** 全局默认（对齐 world-info.js:69-81 的初始值） */
export interface WiGlobals {
  /** 扫描深度：扫描最近 N 条消息 */
  scanDepth: number
  /** token 预算：maxContext 的百分比 */
  budgetPercent: number
  /** 预算硬上限（token），0 = 不设上限 */
  budgetCap: number
  caseSensitive: boolean
  matchWholeWords: boolean
  /** 递归扫描（ST 默认关；开启后新激活条目的内容并入扫描再匹配，最多 2 轮） */
  recursive: boolean
}
const DEFAULT_GLOBALS: WiGlobals = {
  scanDepth: 2,
  budgetPercent: 25,
  budgetCap: 0,
  caseSensitive: false,
  matchWholeWords: false,
  recursive: false,
}

/**
 * timedWorldInfo 单条记录（与 ST WorldInfoTimedEffects #getEntryTimedEffect 同构）：
 * 区间 [start, end) 按消息数推进；protected 的记录在 chat 未推进时不被清除。
 */
export interface WiTimedEffect {
  hash: number
  start: number
  end: number
  protected?: boolean
}

/**
 * chat_metadata.timedWorldInfo 持久化结构（与 ST 网页端互通，world-info.js:559-577）：
 * 键 = `${world}.${uid}`，分 sticky / cooldown 两个桶。
 */
export interface WiTimedWorldInfo {
  sticky?: Record<string, WiTimedEffect>
  cooldown?: Record<string, WiTimedEffect>
}

/** timedEffects 输入（调用方构造：持久化结构 + 当前消息数，turn 语义 = ST 的 chat.length） */
export interface WiTimedInput {
  state: WiTimedWorldInfo
  /** 当前会话消息数（ST 用 chat.length 推进/过期 timedWorldInfo） */
  turn: number
}

/** 带来源标签的条目（用于排序与激活统计）。role 沿用 ST 的数字枚举（0 system/1 user/2 assistant） */
export type WiEntry = StWorldEntry & {
  /** 来源世界书名 */
  world: string
}

export interface WiDepthEntry {
  depth: number
  role: 'system' | 'user' | 'assistant'
  content: string
}

export interface WiScanResult {
  /** ↑Char：插到角色定义前（换行连接的整体文本） */
  before: string
  /** ↓Char：插到角色定义后 */
  after: string
  /** @Depth：按深度插到聊天消息流 */
  depthEntries: WiDepthEntry[]
  /** 本次激活的条目（给状态条/UI 展示） */
  activated: { world: string; uid: number; comment: string }[]
  /** 激活内容消耗的估算 token */
  usedTokens: number
}

/* ---------------- 缓存（会话期间复用；写操作后调 invalidateWiCache） ---------------- */

const books = new Map<string, WiEntry[]>()
let settingsCache: { globalSelect: string[] } | null = null
const charWorldCache = new Map<string, string | null>()

/** 清空世界书缓存（世界书增删改、角色卡换绑后调用） */
export function invalidateWiCache(): void {
  books.clear()
  settingsCache = null
  charWorldCache.clear()
}

async function loadGlobalSelect(): Promise<string[]> {
  if (!settingsCache) {
    try {
      const s = await getSettings()
      const wi = s.world_info as { globalSelect?: unknown } | undefined
      const sel = wi?.globalSelect
      settingsCache = { globalSelect: Array.isArray(sel) ? sel.filter((x): x is string => typeof x === 'string') : [] }
    } catch {
      settingsCache = { globalSelect: [] }
    }
  }
  return settingsCache.globalSelect
}

async function loadCharWorld(avatar: string): Promise<string | null> {
  if (!charWorldCache.has(avatar)) {
    try {
      const c = await getCharacter(avatar)
      const ext = c.data?.extensions as { world?: unknown } | undefined
      const w = ext?.world
      charWorldCache.set(avatar, typeof w === 'string' && w ? w : null)
    } catch {
      charWorldCache.set(avatar, null)
    }
  }
  return charWorldCache.get(avatar) ?? null
}

async function loadBook(name: string): Promise<WiEntry[]> {
  const hit = books.get(name)
  if (hit) return hit
  try {
    const book = await getWorld(name)
    const entries: WiEntry[] = Object.values(book.entries ?? {})
      .filter((e) => e && typeof e === 'object')
      .map((e) => ({ ...e, world: name }))
    books.set(name, entries)
    return entries
  } catch {
    books.set(name, [])
    return []
  }
}

/**
 * 收集当前角色生效的世界书条目：角色绑定书优先（组内 order 降序），全局书随后。
 * 对齐 ST getSortedEntries 的 character_first 策略。
 */
export async function collectActiveEntries(avatar: string): Promise<WiEntry[]> {
  const [globalNames, charWorld] = await Promise.all([loadGlobalSelect(), loadCharWorld(avatar)])
  const names = [...new Set([charWorld, ...globalNames].filter((x): x is string => !!x))]
  const charEntries: WiEntry[] = []
  const globalEntries: WiEntry[] = []
  for (const n of names) {
    const list = await loadBook(n)
    if (n === charWorld) charEntries.push(...list)
    else globalEntries.push(...list)
  }
  const byOrder = (a: WiEntry, b: WiEntry) => (b.order ?? 0) - (a.order ?? 0)
  return [...charEntries.sort(byOrder), ...globalEntries.sort(byOrder)]
}

/* ---------------- 扫描匹配（移植 matchKeys / checkWorldInfo 判定段） ---------------- */

/** ST parseRegexFromString：`/pattern/flags` 写法 → RegExp，否则 null */
function parseRegexFromString(str: string): RegExp | null {
  const m = /^\/(.+)\/([gimsuy]*)$/s.exec(str.trim())
  if (!m) return null
  try {
    return new RegExp(m[1], m[2].replace('g', ''))
  } catch {
    return null
  }
}

function escapeRegex(s: string): string {
  return s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
}

/** ST WorldInfoBuffer.matchKeys */
function matchKeys(haystack: string, needle: string, entry: WiEntry, g: WiGlobals): boolean {
  const re = parseRegexFromString(needle)
  if (re) return re.test(haystack)

  const caseSensitive = entry.caseSensitive ?? g.caseSensitive
  const h = caseSensitive ? haystack : haystack.toLowerCase()
  const n = caseSensitive ? needle : needle.toLowerCase()
  const wholeWords = entry.matchWholeWords ?? g.matchWholeWords

  if (wholeWords) {
    if (n.includes(' ')) return h.includes(n) // 多词：整串包含
    // 单词：非文字字符边界（\W 含 CJK，中文关键词可正常命中）
    return new RegExp(`(?:^|\\W)(${escapeRegex(n)})(?:$|\\W)`).test(h)
  }
  return h.includes(n)
}

/** selective 二级关键词判定（ST matchSecondaryKeys） */
function matchSecondary(entry: WiEntry, text: string, g: WiGlobals): boolean {
  const logic = entry.selectiveLogic ?? WI_LOGIC.AND_ANY
  let hasAny = false
  let hasAll = true
  for (const k of entry.keysecondary ?? []) {
    const hit = !!k.trim() && matchKeys(text, k.trim(), entry, g)
    if (hit) hasAny = true
    else hasAll = false
    if (logic === WI_LOGIC.AND_ANY && hit) return true
    if (logic === WI_LOGIC.NOT_ALL && !hit) return true
  }
  if (logic === WI_LOGIC.NOT_ANY && !hasAny) return true
  if (logic === WI_LOGIC.AND_ALL && hasAll) return true
  return false
}

/**
 * 世界书扫描主入口（对齐 ST checkWorldInfo 的非递归主路径 + 递归/timedEffects）。
 *
 * @param historyTexts 聊天消息正文数组（旧→新）
 * @param entries      collectActiveEntries 的结果
 * @param maxContext   上下文窗口（预算基数）
 * @param overrides    书级/全局覆盖（scanDepth 按条目 → 书 → 全局取值；recursive 递归开关）
 * @param timed        timedEffects 状态（sticky/cooldown 计数 + 消息进度 turn；可选）
 */
export function checkWorldInfo(
  historyTexts: string[],
  entries: WiEntry[],
  maxContext: number,
  overrides: Partial<WiGlobals> = {},
  timed?: WiTimedInput,
): WiScanResult {
  const g: WiGlobals = { ...DEFAULT_GLOBALS, ...overrides }
  const empty: WiScanResult = { before: '', after: '', depthEntries: [], activated: [], usedTokens: 0 }
  if (!entries.length) return empty

  // 扫描文本按深度切片（条目可用 scanDepth 覆盖全局），\x01 连接防跨边界正则误匹配（同 ST）
  const sliceAt = (depth: number): string =>
    ['\x01', ...historyTexts.slice(-depth).map((t) => t.trim()), ''].join('\n\x01')
  const sliceCache = new Map<number, string>()
  const scanTextOf = (entry: WiEntry): string => {
    const d = Math.max(1, entry.scanDepth ?? g.scanDepth)
    let t = sliceCache.get(d)
    if (t === undefined) {
      t = sliceAt(d)
      sliceCache.set(d, t)
    }
    return t
  }

  // 预算：% × maxContext，cap 上限；条目所在书可写 token_budget 覆盖百分比
  let budget = Math.round((g.budgetPercent * maxContext) / 100) || 1
  if (g.budgetCap > 0 && budget > g.budgetCap) budget = g.budgetCap

  const activated: WiEntry[] = []
  let used = 0
  let overflowed = false

  // timedWorldInfo（对齐 ST WorldInfoTimedEffects，world-info.js:479-793）：
  // 记录 = 区间 {hash,start,end,protected}，随消息数（turn）推进与过期。
  // sticky 区间内 → 强制激活；cooldown 区间内 → 跳过；sticky 到期那一刻立起 protected cooldown。
  const tState: WiTimedWorldInfo = timed?.state ?? {}
  const turn = timed?.turn ?? 0
  const entryKey = (e: WiEntry): string => `${e.world}.${e.uid}`
  // ST 的 entry.hash = getStringHash(JSON.stringify(entry))；App 侧匹配走 world.uid 键，
  // hash 仅为结构保真（网页端读取记录时按 key 定位，hash 仅用于其内部 find）
  const hashOf = (key: string): number => {
    let h = 5381
    for (let i = 0; i < key.length; i++) h = ((h << 5) + h + key.charCodeAt(i)) | 0
    return h
  }
  // 本轮生效缓冲（对齐 #buffer）
  const stickyActive = new Set<string>()
  const cooldownActive = new Set<string>()

  // checkTimedEffects（对齐 #checkTimedEffectOfType:619-660）：非 dry run 语义，直接增删记录
  const pruneTimedEffects = (type: 'sticky' | 'cooldown'): void => {
    const bucket = tState[type]
    if (!bucket) return
    for (const key of Object.keys(bucket)) {
      const eff = bucket[key]!
      const entry = entries.find((x) => entryKey(x) === key)
      // chat 未推进（消息被删/重新生成）→ 未保护记录清除（:626-630）
      if (turn <= eff.start && !eff.protected) {
        delete bucket[key]
        continue
      }
      // 条目缺失（可能来自其他角色的书）：区间过了才清（:632-639）
      if (!entry) {
        if (turn >= eff.end) delete bucket[key]
        continue
      }
      // 条目已不配置该效果（:641-646）
      const cfg = Number(type === 'sticky' ? entry.sticky : entry.cooldown)
      if (!(cfg > 0)) {
        delete bucket[key]
        continue
      }
      // 区间结束 → 删除并触发 onEnded（:648-655）
      if (turn >= eff.end) {
        delete bucket[key]
        if (type === 'sticky' && Number(entry.cooldown) > 0) {
          // sticky 结束那一刻立起 cooldown（protected，:518-528），本轮立即跳过。
          // ⚠ 记录必须写进 **cooldown 桶**（ST :525 写 timedWorldInfo.cooldown）——
          //   此前误写回 sticky 桶，条目被 sticky 无限续期强制激活、冷却永不生效
          const cdBucket = (tState.cooldown ??= {})
          cdBucket[key] = { hash: hashOf(key), start: turn, end: turn + Number(entry.cooldown), protected: true }
          cooldownActive.add(key)
        }
        continue
      }
      ;(type === 'sticky' ? stickyActive : cooldownActive).add(key)
    }
  }
  pruneTimedEffects('sticky')
  pruneTimedEffects('cooldown')

  /** 返回 'force'（强制激活）| 'pass'（允许匹配）| 'skip'（跳过） */
  const timedCheck = (entry: WiEntry): 'force' | 'pass' | 'skip' => {
    // delay：消息数未到延迟阈值 → 跳过（#checkDelayEffect:666-677）
    if ((entry.delay ?? 0) > turn) return 'skip'
    const key = entryKey(entry)
    if (stickyActive.has(key)) return 'force'
    if (cooldownActive.has(key)) return 'skip'
    return 'pass'
  }

  const tryActivate = (entry: WiEntry, hit: boolean): boolean => {
    if (!hit) return false
    // 概率掷骰（useProbability && <100）
    if (entry.useProbability && (entry.probability ?? 100) < 100) {
      if (Math.random() * 100 > (entry.probability ?? 100)) return false
    }
    const content = entry.content ?? ''
    if (!content.trim()) return false
    if (!entry.ignoreBudget) {
      if (used + estimateTokens(content) + 1 >= budget) {
        if (!overflowed) overflowed = true
        return false
      }
      used += estimateTokens(content) + 1
    }
    activated.push(entry)
    return true
  }

  for (const entry of entries) {
    if (entry.disable) continue
    const hasKeys = Array.isArray(entry.key) && entry.key.some((k) => k.trim())
    if (!entry.constant && !hasKeys) continue
    const gate = timedCheck(entry)
    if (gate === 'skip') continue

    let hit = entry.constant || gate === 'force'
    if (!hit && hasKeys) {
      const text = scanTextOf(entry)
      const primaryKey = (entry.key ?? []).find((k) => k.trim() && matchKeys(text, k.trim(), entry, g))
      if (primaryKey) {
        const hasSecondary =
          entry.selective && Array.isArray(entry.keysecondary) && entry.keysecondary.some((k) => k.trim())
        hit = !hasSecondary || matchSecondary(entry, text, g)
      }
    }

    tryActivate(entry, hit)
  }
  if (!activated.length) return empty

  // 递归扫描（默认关，g.recursive 开启时）：新激活条目的内容并入扫描再匹配，最多 2 轮
  if (g.recursive) {
    const activatedSet = new Set<WiEntry>(activated)
    let extraText = activated.map((e) => e.content ?? '').join('\x01')
    for (let pass = 0; pass < 2; pass++) {
      let added = false
      for (const entry of entries) {
        if (entry.disable || activatedSet.has(entry)) continue
        const hasKeys = Array.isArray(entry.key) && entry.key.some((k) => k.trim())
        if (!hasKeys) continue
        const text = `${scanTextOf(entry)}\x01${extraText}`
        const primaryKey = (entry.key ?? []).find((k) => k.trim() && matchKeys(text, k.trim(), entry, g))
        if (!primaryKey) continue
        const hasSecondary =
          entry.selective && Array.isArray(entry.keysecondary) && entry.keysecondary.some((k) => k.trim())
        const hit = !hasSecondary || matchSecondary(entry, text, g)
        if (tryActivate(entry, hit)) {
          activatedSet.add(entry)
          extraText += `\x01${entry.content ?? ''}`
          added = true
        }
      }
      if (!added) break
    }
  }

  // setTimedEffects（对齐 world-info.js:730-736）：为激活条目补 sticky/cooldown 记录
  // （已有记录不刷新、不延长；含被 inclusion group 淘汰的条目，与 ST allActivatedEntries 一致）
  const ensureTimedEffect = (entry: WiEntry, type: 'sticky' | 'cooldown'): void => {
    const n = Number(type === 'sticky' ? entry.sticky : entry.cooldown)
    if (!(n > 0)) return
    const bucket = (tState[type] ??= {})
    const key = entryKey(entry)
    if (!bucket[key]) bucket[key] = { hash: hashOf(key), start: turn, end: turn + n, protected: false }
  }
  for (const e of activated) {
    ensureTimedEffect(e, 'sticky')
    ensureTimedEffect(e, 'cooldown')
  }

  if (!activated.length) return empty

  // inclusion group：同组只留一条（group 支持逗号分隔多组；先按组名处理，条目移除后跳过后续组）。
  // winner：有 groupOverride 的条目中 order 最大者优先，否则按 groupWeight 掷骰（同 ST）
  {
    const groups = new Map<string, WiEntry[]>()
    for (const e of activated) {
      for (const g of String(e.group ?? '').split(/,\s*/)) {
        if (!g.trim()) continue
        const arr = groups.get(g)
        if (arr) arr.push(e)
        else groups.set(g, [e])
      }
    }
    const removed = new Set<WiEntry>()
    for (const members of groups.values()) {
      const alive = members.filter((m) => !removed.has(m))
      if (alive.length <= 1) continue
      const prios = alive.filter((m) => m.groupOverride)
      let winner: WiEntry
      if (prios.length) {
        winner = prios.reduce((a, b) => ((b.order ?? 0) > (a.order ?? 0) ? b : a))
      } else {
        const total = alive.reduce((n, m) => n + (m.groupWeight ?? 100), 0)
        let roll = Math.random() * total
        winner = alive[alive.length - 1]!
        for (const m of alive) {
          roll -= m.groupWeight ?? 100
          if (roll <= 0) {
            winner = m
            break
          }
        }
      }
      for (const m of alive) if (m !== winner) removed.add(m)
    }
    if (removed.size) {
      for (const m of removed) {
        const i = activated.indexOf(m)
        if (i >= 0) activated.splice(i, 1)
      }
    }
  }
  if (!activated.length) return empty

  // 分桶：order 降序遍历 + unshift → 最终 order 升序排列（同 ST 组装段）
  const beforeArr: string[] = []
  const afterArr: string[] = []
  const depthMap = new Map<string, WiDepthEntry>()
  for (const e of [...activated].sort((a, b) => (b.order ?? 0) - (a.order ?? 0))) {
    const content = e.content ?? ''
    const pos = e.position ?? WI_POSITION.before
    if (pos === WI_POSITION.before) beforeArr.unshift(content)
    else if (pos === WI_POSITION.after || pos === WI_POSITION.ANTop || pos === WI_POSITION.ANBottom || pos === WI_POSITION.EMTop || pos === WI_POSITION.EMBottom)
      afterArr.unshift(content)
    else if (pos === WI_POSITION.atDepth) {
      const d = e.depth ?? DEFAULT_DEPTH
      const roleMap = ['system', 'user', 'assistant'] as const
      const role = roleMap[e.role ?? 0] ?? 'system'
      const key = `${d}:${role}`
      const exist = depthMap.get(key)
      if (exist) exist.content = `${content}\n${exist.content}`
      else depthMap.set(key, { depth: d, role, content })
    }
  }

  return {
    before: beforeArr.join('\n'),
    after: afterArr.join('\n'),
    depthEntries: [...depthMap.values()],
    activated: activated.map((e) => ({ world: e.world, uid: e.uid, comment: e.comment ?? '' })),
    usedTokens: used,
  }
}
