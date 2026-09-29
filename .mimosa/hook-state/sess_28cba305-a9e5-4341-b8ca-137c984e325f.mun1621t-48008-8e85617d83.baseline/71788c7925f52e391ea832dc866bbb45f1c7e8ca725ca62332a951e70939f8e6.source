/**
 * 用户人设（ST Persona）读写
 *
 * ── 数据在哪 ──
 * 人设的**映射与描述**全在 settings.json 的 `power_user` 段（见实测）：
 *   power_user.personas               = { 头像文件名 → 人设名 }
 *   power_user.persona_descriptions   = { 头像文件名 → { description, position } }
 *   power_user.default_persona        = 默认人设（文件名）
 *   power_user.username               = {{user}} 宏取值
 * 头像文件本身在 `default-user/User Avatars/`，图走 `/thumbnail?type=persona`
 * （Content-Type 由中继嗅探校正，CORP 由中继放开 —— 见 relay.rs）。
 *
 * ── 写入策略 ──
 * 与配置映射同路：**读全量 → 合并 → 写全量**。`/api/settings/save` 是整体覆盖，
 * 只发部分字段会把其余键抹掉。
 *
 * ── 第二版预留 ──
 * Persona 里的 `lorebook` / `characters` / `lockedChat` 三个字段属于第二版
 * （描述关联世界书 / 人设绑定角色 / 会话锁定），数据结构先留好，UI 暂不开放。
 */
import { stBase, stPostForm, stPostJson } from './client'
import { getChat, getSettings, saveChat, saveSettings } from './data'
import { enqueueChatWrite } from './writequeue'
import type { StSettingsLite } from './types'

/** 描述注入位置 */
export const PERSONA_POSITIONS = [
  { v: 0, label: '融进系统提示' },
  { v: 2, label: '角色定义上方' },
  { v: 3, label: '角色定义下方' },
  { v: 4, label: '按深度插入' },
  { v: 9, label: '不注入' },
] as const

/** {{user}} 的用户名默认值（ST 初始化就是空的，需要一个可读兜底） */
export const DEFAULT_USER_NAME = 'User'

/** 按深度插入时的角色身份（0 = system / 1 = user / 2 = assistant） */
export const PERSONA_ROLES = [
  { v: 0, label: 'system' },
  { v: 1, label: 'user' },
  { v: 2, label: 'assistant' },
] as const

export interface Persona {
  /** 头像文件名 = 人设 id（ST 就是以头像文件为键） */
  id: string
  name: string
  description: string
  /** 注入位置枚举值（见 PERSONA_POSITIONS） */
  position: number
  // ===== 第二版预留（数据类型先备好，UI 与写入暂不开放） =====
  /** 关联的世界书名 */
  lorebook?: string
  /** 绑定到的角色 id（多连接） */
  characters?: string[]
  /** 锁定到的会话文件名 */
  lockedChat?: string | null
}

export interface PersonaSnapshot {
  personas: Persona[]
  /** 默认人设 id（null = 未设置） */
  defaultId: string | null
  /** {{user}} 宏取值 */
  userName: string
  /** 按深度插入时的深度（全局） */
  depth: number
  /** 按深度插入时的角色身份（全局） */
  role: number
}

type Dict = Record<string, unknown>

function asDict(v: unknown): Dict {
  return v && typeof v === 'object' && !Array.isArray(v) ? (v as Dict) : {}
}

/** 人设头像 URL（经中继，type=persona） */
export function personaAvatarUrl(id: string): string {
  return `${stBase()}/thumbnail?type=persona&file=${encodeURIComponent(id)}`
}

/** 读取人设快照（settings + 头像文件列表） */
export async function listPersonas(): Promise<PersonaSnapshot> {
  const [settings, avatars] = await Promise.all([
    getSettings(),
    stPostJson<string[]>('/api/avatars/get').catch(() => [] as string[]),
  ])
  const root = settings as unknown as Dict
  const pu = asDict(root.power_user)

  const names = asDict(pu.personas)
  const descs = asDict(pu.persona_descriptions)

  // ST 允许「有头像文件但没建映射」，补成一条
  const ids = new Set<string>([...Object.keys(names), ...avatars])
  const personas: Persona[] = [...ids].map((id) => {
    const d = asDict(descs[id])
    return {
      id,
      name: typeof names[id] === 'string' ? String(names[id]) : id.replace(/\.[^.]+$/, ''),
      description: typeof d.description === 'string' ? String(d.description) : '',
      position: Number.isFinite(Number(d.position)) ? Number(d.position) : 0,
    }
  })
  // 按名字排序（ST 的 persona_sort_order 默认 asc）
  personas.sort((a, b) => a.name.localeCompare(b.name))

  // ST 初始化时 username 就是空串（实测）——{{user}} 宏需要一个可读的值，
  // 空的话按默认 "User" 显示（保存时会写回 ST，两边一致）。
  const rawName = typeof pu.username === 'string' ? pu.username.trim() : ''

  return {
    personas,
    defaultId: typeof pu.default_persona === 'string' ? String(pu.default_persona) : null,
    userName: rawName || DEFAULT_USER_NAME,
    depth: Number(pu.persona_description_depth ?? 2),
    role: Number(pu.persona_description_role ?? 0),
  }
}

/**
 * 写回人设数据。
 * `patch` 只描述要改的部分，本函数负责「读全量 → 合并 → 写全量」。
 */
export async function savePersonas(patch: {
  personas?: Persona[]
  defaultId?: string | null
  userName?: string
  depth?: number
  role?: number
}): Promise<void> {
  const settings = (await getSettings()) as unknown as Dict
  const pu = asDict(settings.power_user)
  const merged: Dict = { ...pu }

  if (patch.personas) {
    const names: Dict = {}
    const descs: Dict = {}
    for (const p of patch.personas) {
      names[p.id] = p.name
      const entry: Dict = { description: p.description, position: p.position }
      // 第二版字段：有值才写，避免空数组污染 ST 侧
      if (p.lorebook) entry.lorebook = p.lorebook
      descs[p.id] = entry
    }
    merged.personas = names
    merged.persona_descriptions = descs
  }
  if (patch.defaultId !== undefined) merged.default_persona = patch.defaultId
  if (patch.userName !== undefined) {
    merged.username = patch.userName
    // 顶层 username 与 power_user.username 是同一语义（{{user}} 宏从顶层读）
    settings.username = patch.userName
  }
  if (patch.depth !== undefined) merged.persona_description_depth = patch.depth
  if (patch.role !== undefined) merged.persona_description_role = patch.role

  settings.power_user = merged
  // settings 是「读到的全量」，整体写回（部分字段会把其余键抹掉）
  await saveSettings(settings as unknown as StSettingsLite)
}

/** 上传头像（multipart）——返回落盘的文件名 */
export async function uploadAvatar(file: File): Promise<string> {
  const form = new FormData()
  form.append('avatar', file, file.name)
  const res = await stPostForm('/api/avatars/upload', form)
  if (!res.ok) {
    const detail = await res.text().catch(() => '')
    throw new Error(`头像上传失败（HTTP ${res.status}）${detail ? `：${detail.slice(0, 120)}` : ''}`)
  }
  const data = (await res.json()) as { path?: string }
  if (!data.path) throw new Error('头像上传返回格式错误')
  return data.path
}

/** 取生效人设：会话锁定优先，其次默认人设，再退列表第一个 */
export function resolvePersona(snap: PersonaSnapshot, lockedId: string | null): Persona | null {
  const byId = (id: string | null | undefined) => snap.personas.find((p) => p.id === id) ?? null
  return byId(lockedId) ?? byId(snap.defaultId) ?? snap.personas[0] ?? null
}

/**
 * 会话锁定人设。
 *
 * ST 的会话文件是 JSONL：**首项是元数据头**，`chat_metadata` 嵌在里面。
 * 所以这里「读整个会话 → 改首项的 chat_metadata.persona → 整份写回」。
 * 传 null = 解除锁定（跟随默认人设）。
 */
export async function setChatPersona(
  avatarUrl: string,
  fileName: string,
  personaId: string | null,
): Promise<void> {
  // 读-改-写入队：与 saveCurrent 等其它会话文件写通路串行（防后写者胜回滚）
  await enqueueChatWrite(async () => {
    const chat = await getChat(avatarUrl, fileName)
    if (!chat.length) throw new Error('会话为空，无法锁定人设')

    const head = asDict(chat[0])
    const meta = asDict(head.chat_metadata)
    if (personaId) meta.persona = personaId
    else delete meta.persona
    head.chat_metadata = meta
    chat[0] = head as (typeof chat)[number]

    await saveChat(avatarUrl, fileName, chat)
  })
}

/** 读会话里锁定的的人设 id（未锁定返回 null） */
export function chatPersonaId(chat: unknown[]): string | null {
  const head = asDict(chat[0])
  const meta = asDict(head.chat_metadata)
  return typeof meta.persona === 'string' && meta.persona ? meta.persona : null
}

/** 删除头像文件（不影响人设映射，调用方需自行同步删映射） */
export async function deleteAvatar(id: string): Promise<void> {
  await stPostJson('/api/avatars/delete', { avatar: id })
}
