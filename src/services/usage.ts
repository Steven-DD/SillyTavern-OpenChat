/**
 * Token 用量与费用估算（状态条「用量胶囊」数据源）
 *
 * 三个层次：
 *  1. 输出 tokens：优先取接口实测（SSE 尾帧 usage），拿不到按字数估算（CJK ≈1 tok/字，其它 ≈4 字符/tok）
 *  2. 输入 tokens：buildPrompt 的本地估算（无法从 ST 拿到真实 prompt 时只能估算）
 *  3. 费用：内置价格表按模型 id 子串匹配（USD / 1M tokens）；无价格数据的模型不显示费用
 *
 * 会话累计：localStorage（本 App 壳侧统计，不进 ST 文件），键 = sessionKey。
 * 价格为公开牌价的近似值，仅作参考，标「≈」；新模型出现时在此补一行即可。
 */

export interface UsagePair {
  input: number
  output: number
}

export interface TokenPrice {
  /** 模型 id 子串匹配（小写包含） */
  match: string
  /** 展示名 */
  label: string
  /** USD / 1M 输入 tokens */
  inPrice: number
  /** USD / 1M 输出 tokens */
  outPrice: number
}

/** 常用模型价格表（公开牌价近似值，2025；USD / 1M tokens） */
const PRICES: TokenPrice[] = [
  { match: 'deepseek-chat', label: 'DeepSeek V3', inPrice: 0.27, outPrice: 1.1 },
  { match: 'deepseek-reasoner', label: 'DeepSeek R1', inPrice: 0.55, outPrice: 2.19 },
  { match: 'deepseek', label: 'DeepSeek', inPrice: 0.27, outPrice: 1.1 },
  { match: 'gemini-2.5-flash-lite', label: 'Gemini 2.5 Flash-Lite', inPrice: 0.1, outPrice: 0.4 },
  { match: 'gemini-2.5-flash', label: 'Gemini 2.5 Flash', inPrice: 0.3, outPrice: 2.5 },
  { match: 'gemini-2.5-pro', label: 'Gemini 2.5 Pro', inPrice: 1.25, outPrice: 10 },
  { match: 'gemini-2.0-flash', label: 'Gemini 2.0 Flash', inPrice: 0.1, outPrice: 0.4 },
  { match: 'gemini-1.5-flash', label: 'Gemini 1.5 Flash', inPrice: 0.075, outPrice: 0.3 },
  { match: 'gemini-1.5-pro', label: 'Gemini 1.5 Pro', inPrice: 1.25, outPrice: 5 },
  { match: 'gpt-4o-mini', label: 'GPT-4o mini', inPrice: 0.15, outPrice: 0.6 },
  { match: 'gpt-4o', label: 'GPT-4o', inPrice: 2.5, outPrice: 10 },
  { match: 'gpt-4.1-mini', label: 'GPT-4.1 mini', inPrice: 0.4, outPrice: 1.6 },
  { match: 'gpt-4.1', label: 'GPT-4.1', inPrice: 2, outPrice: 8 },
  { match: 'claude-3-5-haiku', label: 'Claude 3.5 Haiku', inPrice: 0.8, outPrice: 4 },
  { match: 'claude-haiku', label: 'Claude Haiku', inPrice: 1, outPrice: 5 },
  { match: 'claude-sonnet', label: 'Claude Sonnet', inPrice: 3, outPrice: 15 },
  { match: 'claude-opus', label: 'Claude Opus', inPrice: 15, outPrice: 75 },
  { match: 'claude', label: 'Claude', inPrice: 3, outPrice: 15 },
]

/** 查模型价格（小写子串匹配，先专门后宽泛——表序即优先级）；无数据返回 null */
export function priceOf(model: string): TokenPrice | null {
  const m = model.toLowerCase()
  return PRICES.find((p) => m.includes(p.match)) ?? null
}

/** 本地 token 估算：CJK 字符 ≈1 tok/字，其余 ≈4 字符/tok */
export function estimateTokens(text: string): number {
  if (!text) return 0
  let cjk = 0
  for (const ch of text) if (/[\u4e00-\u9fff\u3040-\u30ff\uac00-\ud7af]/.test(ch)) cjk++
  return Math.ceil(cjk + (text.length - cjk) / 4)
}

/** 估算一轮费用（USD）；模型无价格数据返回 null */
export function calcCost(model: string, usage: UsagePair): number | null {
  const p = priceOf(model)
  if (!p) return null
  return (usage.input * p.inPrice + usage.output * p.outPrice) / 1_000_000
}

/** 费用展示格式：金额很小，保留有效数字（$0.0082 / $0.0621 / $1.24） */
export function fmtCost(usd: number | null): string {
  if (usd == null) return ''
  if (usd >= 1) return `$${usd.toFixed(2)}`
  if (usd >= 0.01) return `$${usd.toFixed(3)}`
  return `$${usd.toFixed(4)}`
}

/** tokens 展示：1 位小数缩写（2134 → 2.1k） */
export function fmtTok(n: number): string {
  if (n >= 10_000) return `${(n / 1000).toFixed(0)}k`
  if (n >= 1000) return `${(n / 1000).toFixed(1)}k`
  return String(n)
}

/* ---------------- 会话累计（localStorage，键 = sessionKey） ---------------- */

const STATS_KEY = 'app.tokstats'

export interface SessionUsage {
  input: number
  output: number
  /** 累计费用（USD）；模型无价格时不累计 */
  cost: number
  /** 生成轮数 */
  turns: number
}

function loadAll(): Record<string, SessionUsage> {
  try {
    const v = JSON.parse(localStorage.getItem(STATS_KEY) ?? '{}')
    return v && typeof v === 'object' ? (v as Record<string, SessionUsage>) : {}
  } catch {
    return {}
  }
}

export function getSessionUsage(key: string): SessionUsage {
  const s = loadAll()[key]
  if (!s) return { input: 0, output: 0, cost: 0, turns: 0 }
  return {
    input: Number(s.input) || 0,
    output: Number(s.output) || 0,
    cost: Number(s.cost) || 0,
    turns: Number(s.turns) || 0,
  }
}

/** 累加一轮用量并落盘 */
export function addSessionUsage(key: string, usage: UsagePair, cost: number | null): void {
  if (!key) return
  const all = loadAll()
  const s = all[key] ?? { input: 0, output: 0, cost: 0, turns: 0 }
  s.input += usage.input
  s.output += usage.output
  if (cost != null) s.cost += cost
  s.turns += 1
  all[key] = s
  localStorage.setItem(STATS_KEY, JSON.stringify(all))
}

/** 会话改名时迁移累计统计（不覆盖已有目标键） */
export function moveSessionUsage(oldKey: string, newKey: string): void {
  if (!oldKey || !newKey || oldKey === newKey) return
  const all = loadAll()
  if (!all[oldKey]) return
  if (!all[newKey]) {
    all[newKey] = all[oldKey]
    delete all[oldKey]
    localStorage.setItem(STATS_KEY, JSON.stringify(all))
  }
}
