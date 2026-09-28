/**
 * Prompt Manager 数据层（对齐 ST openai.js / PromptManager.js）
 *
 * settings.json 两个键：
 * - prompts: {identifier, name, system_prompt, role, content, marker?}[]
 * - prompt_order: [{character_id, order: [{identifier, enabled}]}]
 *   全局默认条目集的 character_id = 100001（ST dummyId，openai.js:698）
 *
 * App 使用 100001 全局序；三槽 main/nsfw/jailbreak 内容可编辑；
 * 其余标识映射到 App 的注入能力（世界书/人设/卡字段/示例），不支持的跳过。
 * 保存遵循 settings 三铁律：读全量 → 合并 → 写全量 → 比对一致不写。
 */
import { getSettings, saveSettingsFull } from './data'

export interface PmEntry {
  identifier: string
  name?: string
  system_prompt?: boolean
  role?: 'system' | 'user' | 'assistant'
  content?: string
  marker?: boolean
  injection_position?: number
  injection_depth?: number
  [k: string]: unknown
}

export interface PmOrderItem {
  character_id: number
  order: { identifier: string; enabled: boolean }[]
}

/** App 支持的标识 → 中文名（chatHistory 由历史管线处理，不走系统提示） */
export const PM_IDENTIFIERS: Record<string, string> = {
  main: '主提示词',
  worldInfoBefore: '世界书（角色前）',
  personaDescription: '用户人设',
  charDescription: '角色描述',
  charPersonality: '角色性格',
  scenario: '场景',
  nsfw: 'NSFW',
  worldInfoAfter: '世界书（角色后）',
  dialogueExamples: '对话示例',
  jailbreak: '越狱',
  chatHistory: '聊天历史',
}

/** ST 默认顺序（PromptManager.js:2087-2136，App 支持子集） */
export const DEFAULT_ORDER: { identifier: string; enabled: boolean }[] = [
  { identifier: 'main', enabled: true },
  { identifier: 'worldInfoBefore', enabled: true },
  { identifier: 'personaDescription', enabled: true },
  { identifier: 'charDescription', enabled: true },
  { identifier: 'charPersonality', enabled: true },
  { identifier: 'scenario', enabled: true },
  { identifier: 'nsfw', enabled: true },
  { identifier: 'worldInfoAfter', enabled: true },
  { identifier: 'dialogueExamples', enabled: true },
  { identifier: 'chatHistory', enabled: true },
  { identifier: 'jailbreak', enabled: true },
]

const DUMMY_ID = 100001

export interface PmData {
  /** 三槽内容（缺省 ''） */
  main: string
  nsfw: string
  jailbreak: string
  /** 生效顺序（100001；缺省 DEFAULT_ORDER） */
  order: { identifier: string; enabled: boolean }[]
  /** 原始 prompts 数组（保存时合并回写） */
  rawPrompts: PmEntry[]
  /** 是否来自真实 ST 数据（false = settings 里没有，用默认） */
  loaded: boolean
}

let cache: PmData | null = null

export function invalidatePmCache(): void {
  cache = null
}

function emptySlots(): { main: string; nsfw: string; jailbreak: string } {
  return { main: '', nsfw: '', jailbreak: '' }
}

function findOrder(list: PmOrderItem[] | undefined): { identifier: string; enabled: boolean }[] | null {
  if (!Array.isArray(list)) return null
  const hit = list.find((o) => o.character_id === DUMMY_ID)
  return hit && Array.isArray(hit.order) ? hit.order : null
}

export async function loadPm(force = false): Promise<PmData> {
  if (cache && !force) return cache
  const settings = await getSettings()
  const rawPrompts = (Array.isArray(settings.prompts) ? settings.prompts : []) as PmEntry[]
  const order =
    findOrder(settings.prompt_order as PmOrderItem[] | undefined) ??
    DEFAULT_ORDER.map((x) => ({ ...x }))
  const byId = new Map(rawPrompts.map((p) => [p.identifier, p]))
  const slot = (id: string): string => s(byId.get(id)?.content)
  cache = {
    ...emptySlots(),
    main: slot('main'),
    nsfw: slot('nsfw'),
    jailbreak: slot('jailbreak'),
    order,
    rawPrompts,
    loaded: rawPrompts.length > 0,
  }
  return cache
}

const s = (v: unknown): string => (typeof v === 'string' ? v : '')

/** 三槽 + 顺序写回 settings.json（铁律：读全量 → 合并 → 比对 → 写全量） */
export async function savePm(
  slots: { main?: string; nsfw?: string; jailbreak?: string },
  order: { identifier: string; enabled: boolean }[],
): Promise<void> {
  const settings = await getSettings()
  const prompts = (Array.isArray(settings.prompts) ? settings.prompts : []) as PmEntry[]
  const merged = [...prompts]
  const upsert = (identifier: string, content: string): void => {
    const i = merged.findIndex((p) => p.identifier === identifier)
    if (i >= 0) merged[i] = { ...merged[i]!, content }
    else
      merged.push({
        identifier,
        name: identifier,
        system_prompt: true,
        role: 'system',
        content,
      })
  }
  if (slots.main !== undefined) upsert('main', slots.main)
  if (slots.nsfw !== undefined) upsert('nsfw', slots.nsfw)
  if (slots.jailbreak !== undefined) upsert('jailbreak', slots.jailbreak)

  const orderList = (Array.isArray(settings.prompt_order)
    ? (settings.prompt_order as PmOrderItem[])
    : []
  ).filter((o) => o.character_id !== DUMMY_ID)
  orderList.push({ character_id: DUMMY_ID, order: order.map((x) => ({ ...x })) })

  // 比对一致就不写（避免无谓 autosave 备份）
  const before = JSON.stringify({ p: settings.prompts, o: settings.prompt_order })
  const after = JSON.stringify({ p: merged, o: orderList })
  if (before === after) {
    cache = {
      main: slots.main ?? cache?.main ?? '',
      nsfw: slots.nsfw ?? cache?.nsfw ?? '',
      jailbreak: slots.jailbreak ?? cache?.jailbreak ?? '',
      order,
      rawPrompts: merged,
      loaded: true,
    }
    return
  }
  settings.prompts = merged
  settings.prompt_order = orderList
  await saveSettingsFull(settings)
  invalidatePmCache()
}
