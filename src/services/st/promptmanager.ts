/**
 * Prompt Manager 数据层
 *
 * settings.json 两个键：
 * - prompts: {identifier, name, system_prompt, role, content, marker?}[]
 * - prompt_order: [{character_id, order: [{identifier, enabled}]}]
 *   全局默认条目集的 character_id = 100001
 *
 * App 使用 100001 全局序；三槽 main/nsfw/jailbreak 内容可编辑；
 * 其余标识映射到 App 的注入能力（世界书/人设/卡字段/示例），不支持的跳过。
 * 保存遵循 settings 三铁律：读全量 → 合并 → 写全量 → 比对一致不写。
 */
import { getSettings } from './data'
import { mutateSettings } from './settings-writequeue'

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

/** ST 默认顺序 */
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
  /** 生效顺序（缺省 DEFAULT_ORDER） */
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

/** 三槽 + 顺序写回 settings.json（经 settings 写队列：锁内新鲜读 → 合并 → 比对 → 写全量） */
export async function savePm(
  slots: { main?: string; nsfw?: string; jailbreak?: string },
  order: { identifier: string; enabled: boolean }[],
  customBlocks?: PmEntry[],
): Promise<void> {
  await mutateSettings((settings) => {
    const prompts = (Array.isArray(settings.prompts) ? settings.prompts : []) as PmEntry[]
    const merged = [...prompts]
    const upsert = (entry: PmEntry): void => {
      const i = merged.findIndex((p) => p.identifier === entry.identifier)
      if (i >= 0) merged[i] = { ...merged[i]!, ...entry }
      else merged.push(entry)
    }
    if (slots.main !== undefined) {
      upsert({ identifier: 'main', name: 'main', system_prompt: true, role: 'system', content: slots.main })
    }
    if (slots.nsfw !== undefined) {
      upsert({ identifier: 'nsfw', name: 'nsfw', system_prompt: true, role: 'system', content: slots.nsfw })
    }
    if (slots.jailbreak !== undefined) {
      upsert({ identifier: 'jailbreak', name: 'jailbreak', system_prompt: true, role: 'system', content: slots.jailbreak })
    }
    if (customBlocks?.length) {
      for (const cb of customBlocks) {
        upsert(cb)
      }
    }

    const orderList = (Array.isArray(settings.prompt_order)
      ? (settings.prompt_order as PmOrderItem[])
      : []
    ).filter((o) => o.character_id !== DUMMY_ID)
    orderList.push({ character_id: DUMMY_ID, order: order.map((x) => ({ ...x })) })

    // 比对一致就不写（避免无谓 autosave 备份）
    const before = JSON.stringify({ p: settings.prompts, o: settings.prompt_order })
    const after = JSON.stringify({ p: merged, o: orderList })
    if (before === after) {
      return {
        next: settings,
        skip: true,
      }
    }
    return {
      next: { ...settings, prompts: merged, prompt_order: orderList },
    }
  })
  invalidatePmCache()
}

/** 导出当前 Prompt Manager 配置为 ST 兼容 JSON */
export function exportPmPreset(name: string, pm: PmData): void {
  const payload = {
    name,
    prompts: pm.rawPrompts,
    prompt_order: pm.order,
  }
  const str = JSON.stringify(payload, null, 4)
  const blob = new Blob([str], { type: 'application/json' })
  const url = URL.createObjectURL(blob)
  const a = document.createElement('a')
  a.href = url
  a.download = `${name.trim() || 'prompt-preset'}.json`
  document.body.appendChild(a)
  a.click()
  document.body.removeChild(a)
  URL.revokeObjectURL(url)
}

/** 导入 ST Prompt Manager 预设 JSON */
export async function importPmPreset(file: File): Promise<PmData> {
  const text = await file.text()
  const raw = JSON.parse(text) as Record<string, unknown>
  const rawPrompts = (Array.isArray(raw.prompts) ? raw.prompts : []) as PmEntry[]
  let order: { identifier: string; enabled: boolean }[] = []
  if (Array.isArray(raw.prompt_order)) {
    order = raw.prompt_order as { identifier: string; enabled: boolean }[]
  } else {
    order = DEFAULT_ORDER.map((x) => ({ ...x }))
  }

  const findContent = (id: string) => rawPrompts.find((p) => p.identifier === id)?.content ?? ''

  await mutateSettings((settings) => {
    const orderList = (Array.isArray(settings.prompt_order)
      ? (settings.prompt_order as PmOrderItem[])
      : []
    ).filter((o) => o.character_id !== DUMMY_ID)
    orderList.push({ character_id: DUMMY_ID, order: order.map((x) => ({ ...x })) })

    return {
      next: { ...settings, prompts: rawPrompts, prompt_order: orderList },
    }
  })

  invalidatePmCache()
  return {
    main: findContent('main'),
    nsfw: findContent('nsfw'),
    jailbreak: findContent('jailbreak'),
    order,
    rawPrompts,
    loaded: true,
  }
}
