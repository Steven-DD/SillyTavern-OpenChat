/**
 * SillyTavern 数据层（M3 · 角色卡 / 会话 / 设置 / 静态资源）
 *
 * 与 api.ts（生成层）分离：这里只做数据读写，不涉及 prompt 与流式。
 * 所有字段名以 2026-09-22 实测为准（见 types.ts 头部说明）。
 */
import { stBase, stPost, stPostForm, stPostJson, stPostVoid } from './client'
import type {
  StCharacter,
  StCharacterBook,
  StChatMessage,
  StChatSummary,
  StSettingsLite,
  StThumbnailType,
  StWorldBook,
  StWorldEntry,
  StWorldListItem,
} from './types'

/* ------------------------------------------------------------------ *
 * 角色卡（= 联系人）
 * ------------------------------------------------------------------ */

/** 角色卡列表。注意：description/first_mes 等字段可能很长（KB 级） */
export async function listCharacters(): Promise<StCharacter[]> {
  const data = await stPostJson<StCharacter[] | { error?: boolean }>('/api/characters/all')
  return Array.isArray(data) ? data.filter((c) => c?.name) : []
}

/** 单个角色卡（深层，含完整 data 字段） */
export async function getCharacter(avatarUrl: string): Promise<StCharacter> {
  return stPostJson<StCharacter>('/api/characters/get', { avatar_url: avatarUrl })
}

/**
 * 保存角色卡修改（JSON 即可，该路由未挂 multer）。
 * 实测入参名由 charaFormatData 决定（characters.js:565-593）：
 *   ch_name / description / personality / scenario / first_mes / mes_example
 *   / creator_notes / tags / talkativeness / fav / chat / create_date / json_data
 * 注意：
 * - `json_data` 必须原样回传，否则 V2/V3 卡里的其它外键字段会丢；
 * - `chat` 与 `create_date` 也要回传，否则 ST 会把「最近会话」和创建时间重置。
 */
export interface CharacterEditPayload {
  avatar_url: string
  ch_name: string
  description: string
  personality: string
  scenario: string
  first_mes: string
  mes_example: string
  creator_notes: string
  tags: string[]
  talkativeness: string | number
  fav: string | boolean
  chat: string
  create_date: string
  json_data?: string
  /** 绑定的世界书名（写 data.extensions.world）；不传 = ST 会把绑定抹成空 */
  world?: string
  /** 高级字段：作者署名（v2 spec creator） */
  creator?: string
  /** 高级字段：卡片版本号（v2 spec character_version） */
  character_version?: string
  /** 高级字段：备选开场白（v2 spec alternate_greetings，数组） */
  alternate_greetings?: string[]
}

export async function editCharacter(payload: CharacterEditPayload): Promise<void> {
  // ⚠ characters/edit 成功时是 sendStatus(200) → 纯文本 "OK"，不能用 stPostJson
  await stPostVoid('/api/characters/edit', payload)
}

/** 新建空白卡入参（其余字段 ST 侧 charaFormatData 会填默认值） */
export interface CharacterCreatePayload {
  ch_name: string
  description?: string
  personality?: string
  scenario?: string
  first_mes?: string
  mes_example?: string
  creator_notes?: string
  tags?: string[]
  /** 可选头像图片（不传 = ST 默认头像） */
  avatar?: File | null
}

/**
 * 新建角色卡（POST /api/characters/create，multipart）。
 * 全局 multer `single('avatar')`（server-main.js:269）→ 文件字段名固定 avatar，
 * 文本字段走 FormData（该路由必须经 multer，JSON body 拿不到）。
 * 返回头像文件名（'名字.png'）。
 */
export async function createCharacter(p: CharacterCreatePayload): Promise<string> {
  const fd = new FormData()
  fd.set('ch_name', p.ch_name)
  if (p.description) fd.set('description', p.description)
  if (p.personality) fd.set('personality', p.personality)
  if (p.scenario) fd.set('scenario', p.scenario)
  if (p.first_mes) fd.set('first_mes', p.first_mes)
  if (p.mes_example) fd.set('mes_example', p.mes_example)
  if (p.creator_notes) fd.set('creator_notes', p.creator_notes)
  if (p.tags?.length) fd.set('tags', JSON.stringify(p.tags))
  if (p.avatar) fd.set('avatar', p.avatar)
  const res = await stPostForm('/api/characters/create', fd)
  if (!res.ok) throw new Error(`新建角色卡失败（HTTP ${res.status}）`)
  // ⚠ 该端点返回纯文本（response.send(avatarName)），不是 JSON
  const name = (await res.text()).trim()
  if (!name) throw new Error('新建角色卡失败（ST 未返回文件名）')
  return name.endsWith('.png') ? name : `${name}.png`
}

/**
 * 导入角色卡文件（POST /api/characters/import，multipart）。
 * 支持 json / png / charx / yaml / byaf（characters.js:1570 的格式表）。
 * 返回头像文件名（⚠ ST 返回的 file_name 可能不带 .png，这里统一补全）；
 * v2 JSON 的内嵌 character_book 会被 ST 原样保留在卡里。
 */
export async function importCharacterFile(file: File): Promise<string> {
  const ext = (file.name.split('.').pop() ?? 'json').toLowerCase()
  let payload: Blob = file
  if (ext === 'json') {
    // ⚠ ST 的 importFromJson 直接 JSON.parse，不接受 UTF-8 BOM（U+FEFF）——
    // 带BOM 的卡实测返回 {error:true}（characters.js:887）。读文本剥 BOM 后再上传。
    const text = (await file.text()).replace(/^\uFEFF/, '')
    payload = new File([text], file.name, { type: 'application/json' })
  }
  const fd = new FormData()
  fd.set('avatar', payload)
  fd.set('file_type', ext)
  const res = await stPostForm('/api/characters/import', fd)
  if (!res.ok) throw new Error(`导入角色卡失败（HTTP ${res.status}）`)
  const data = (await res.json()) as { file_name?: string; error?: boolean }
  if (!data.file_name) throw new Error('导入角色卡失败（文件格式无法识别或与现有卡重名）')
  return data.file_name.endsWith('.png') ? data.file_name : `${data.file_name}.png`
}

/** 删除角色卡（/api/characters/delete）。delete_chats=true 连带清掉该卡全部会话。 */
export async function deleteCharacter(avatarUrl: string): Promise<void> {
  const res = await stPost('/api/characters/delete', {
    avatar_url: avatarUrl,
    delete_chats: true,
  })
  if (!res.ok) throw new Error(`删除角色卡失败（HTTP ${res.status}）`)
}

/**
 * 把世界书绑定到角色卡（写 data.extensions.world，ST 的角色↔世界书关联字段）。
 * 走 /api/characters/edit：必须带全字段（同 saveCard），另传 world，
 * charaFormatData 会把书回嵌进 data.character_book 并写 extensions.world。
 */
export async function setCharacterWorld(avatarUrl: string, world: string): Promise<void> {
  const c = await getCharacter(avatarUrl)
  await stPostVoid('/api/characters/edit', {
    avatar_url: avatarUrl,
    ch_name: c.name,
    description: c.description ?? '',
    personality: c.personality ?? '',
    scenario: c.scenario ?? '',
    first_mes: c.first_mes ?? '',
    mes_example: c.mes_example ?? '',
    creator_notes: String(c.creatorcomment ?? ''),
    tags: c.tags ?? [],
    talkativeness: c.talkativeness ?? 0.5,
    fav: String(c.fav ?? false),
    chat: c.chat,
    create_date: c.create_date,
    json_data: c.json_data,
    world,
  })
  // ⚠ 同 editCharacter：返回纯文本 "OK"，不能用 stPostJson
}

/** 给角色卡追加标签（读全字段 → tags 合并 → 整卡回写；批量打标签循环调用） */
export async function appendCharacterTags(avatarUrl: string, addTags: string[]): Promise<void> {
  const c = await getCharacter(avatarUrl)
  const cur = new Set((c.tags ?? []).map((t) => String(t)))
  const merged = [...cur, ...addTags.filter((t) => t && !cur.has(t))]
  await stPostVoid('/api/characters/edit', {
    avatar_url: avatarUrl,
    ch_name: c.name,
    description: c.description ?? '',
    personality: c.personality ?? '',
    scenario: c.scenario ?? '',
    first_mes: c.first_mes ?? '',
    mes_example: c.mes_example ?? '',
    creator_notes: String(c.creatorcomment ?? ''),
    tags: merged,
    talkativeness: c.talkativeness ?? 0.5,
    fav: String(c.fav ?? false),
    chat: c.chat,
    create_date: c.create_date,
    json_data: c.json_data,
  })
}

/**
 * 重命名角色卡（POST /api/characters/rename {avatar_url, new_name}，characters.js:1053）。
 * 服务端同步迁移聊天目录/群组成员引用；返回新 avatar 文件名（getPngName(newName).png）。
 */
export async function renameCharacter(avatarUrl: string, newName: string): Promise<string> {
  const res = await stPost('/api/characters/rename', {
    avatar_url: avatarUrl,
    new_name: newName,
  })
  if (!res.ok) throw new Error(`重命名失败（HTTP ${res.status}；可能与现有卡重名）`)
  return `${newName.replace(/[\\/:*?"<>|]/g, '')}.png`
}

/* ------------------------------------------------------------------ *
 * 世界书（World Info）
 * ------------------------------------------------------------------ */

/** 世界书列表（/api/worldinfo/list，无入参） */
export async function listWorlds(): Promise<StWorldListItem[]> {
  const data = await stPostJson<StWorldListItem[] | null>('/api/worldinfo/list')
  return Array.isArray(data) ? data : []
}

/** 读取世界书（/api/worldinfo/get {name}） */
export async function getWorld(name: string): Promise<StWorldBook> {
  return stPostJson<StWorldBook>('/api/worldinfo/get', { name })
}

/** 保存世界书（/api/worldinfo/edit {name, data}）。ST 无 create 端点，新建 = 直接 edit 空 entries */
export async function saveWorld(name: string, data: StWorldBook): Promise<void> {
  await stPostJson('/api/worldinfo/edit', { name, data })
}

/** 新建空世界书（复刻 ST 前端 createWorldInfo：edit 一个空 entries） */
export async function createWorld(name: string): Promise<string> {
  await saveWorld(name, { entries: {} })
  return name
}

/**
 * 导入世界书文件（POST /api/worldinfo/import，multipart）。
 * 文件需为顶层含 entries 的 JSON（ST 原生世界书导出格式）。
 * 返回世界书名。
 */
export async function importWorldFile(file: File): Promise<string> {
  const fd = new FormData()
  fd.set('avatar', file)
  const res = await stPostForm('/api/worldinfo/import', fd)
  const text = await res.text()
  if (!res.ok) throw new Error(`导入世界书失败（HTTP ${res.status}）`)
  let data: { name?: string } = {}
  try {
    data = JSON.parse(text) as { name?: string }
  } catch {
    /* 400 时 body 是纯文本错误信息 */
  }
  if (!data.name) throw new Error(text || '导入世界书失败（不是有效的世界书文件）')
  return data.name
}

/** 删除世界书（/api/worldinfo/delete {name}）。成功返回纯文本 "OK"，不能用 stPostJson */
export async function deleteWorld(name: string): Promise<void> {
  await stPostVoid('/api/worldinfo/delete', { name })
}

/* ------------------------------------------------------------------ *
 * v2 内嵌世界书 → ST 世界书 转换
 * ------------------------------------------------------------------ */

/* ST 常量（public/scripts/world-info.js:33/96/855 与 script.js:494） */
const WI_POSITION_AFTER = 1
const WI_LOGIC_AND_ANY = 0
const ROLE_SYSTEM = 0
const DEFAULT_DEPTH = 4
const DEFAULT_WEIGHT = 100

/**
 * chara_card_v2 的 character_book → ST 世界书格式。
 * 复刻 ST 前端 convertCharacterBook（world-info.js:5617），字段默认值保持一致。
 */
export function convertCharacterBook(book: StCharacterBook): StWorldBook {
  const result: StWorldBook = { entries: {}, originalData: book }
  const list = Array.isArray(book.entries) ? book.entries : []
  list.forEach((entry, index) => {
    const id = entry.id ?? index
    // spec 的 extensions 里可以带 ST 专属字段；林晚棠这类纯 spec 卡全走默认值
    const ext = (entry.extensions ?? {}) as Record<string, unknown>
    const e = ext as {
      position?: number
      exclude_recursion?: boolean
      prevent_recursion?: boolean
      delay_until_recursion?: boolean
      display_index?: number
      probability?: number
      useProbability?: boolean
      depth?: number
      selectiveLogic?: number
      group?: string
      group_override?: boolean
      group_weight?: number
      scan_depth?: number | null
      case_sensitive?: boolean | null
      match_whole_words?: boolean | null
      use_group_scoring?: boolean | null
      automation_id?: string
      role?: number
      vectorized?: boolean
      sticky?: number | null
      cooldown?: number | null
      delay?: number | null
      triggers?: string[]
      ignore_budget?: boolean
    }
    result.entries[String(id)] = {
      uid: id,
      key: entry.keys ?? [],
      keysecondary: entry.secondary_keys ?? [],
      comment: entry.comment ?? '',
      content: entry.content ?? '',
      constant: entry.constant ?? false,
      selective: entry.selective ?? false,
      order: entry.insertion_order ?? 100,
      position: e.position ?? (entry.position === 'after_char' ? WI_POSITION_AFTER : 0),
      excludeRecursion: e.exclude_recursion ?? false,
      preventRecursion: e.prevent_recursion ?? false,
      // v2 spec 的 delay_until_recursion 是 boolean（true = 延迟 1 步）；ST 1.19 内部为 number
      delayUntilRecursion:
        typeof e.delay_until_recursion === 'number'
          ? e.delay_until_recursion
          : e.delay_until_recursion
            ? 1
            : 0,
      disable: !entry.enabled,
      addMemo: !!entry.comment,
      displayIndex: e.display_index ?? index,
      probability: e.probability ?? 100,
      useProbability: e.useProbability ?? true,
      depth: e.depth ?? DEFAULT_DEPTH,
      selectiveLogic: e.selectiveLogic ?? WI_LOGIC_AND_ANY,
      group: e.group ?? '',
      groupOverride: e.group_override ?? false,
      groupWeight: e.group_weight ?? DEFAULT_WEIGHT,
      scanDepth: e.scan_depth ?? null,
      caseSensitive: e.case_sensitive ?? null,
      matchWholeWords: e.match_whole_words ?? null,
      useGroupScoring: e.use_group_scoring ?? null,
      automationId: e.automation_id ?? '',
      role: e.role ?? ROLE_SYSTEM,
      vectorized: e.vectorized ?? false,
      sticky: e.sticky ?? null,
      cooldown: e.cooldown ?? null,
      delay: e.delay ?? null,
      matchPersonaDescription: false,
      matchCharacterDescription: false,
      matchCharacterPersonality: false,
      matchCharacterDepthPrompt: false,
      matchScenario: false,
      matchCreatorNotes: false,
      extensions: entry.extensions ?? {},
      triggers: e.triggers || [],
      ignoreBudget: e.ignore_budget ?? false,
    } satisfies StWorldEntry
  })
  return result
}

/* ------------------------------------------------------------------ *
 * 会话
 * ------------------------------------------------------------------ */

/**
 * 某角色下的会话文件列表。
 * ST 行为：该角色还没有 chats 目录时返回 `{error:true}`（不是错误，是没有会话），
 * 目录存在但为空时返回 `[]`。这里统一归一化为数组。
 */
export async function listCharacterChats(
  avatarUrl: string,
  opts: { metadata?: boolean; simple?: boolean } = {},
): Promise<StChatSummary[]> {
  const data = await stPostJson<StChatSummary[] | { error?: boolean }>(
    '/api/characters/chats',
    { avatar_url: avatarUrl, metadata: !!opts.metadata, simple: !!opts.simple },
  )
  if (Array.isArray(data)) return data
  return []
}

/** 跨角色最近会话（按 mtime 倒序）—— 会话列表数据源 */
export async function recentChats(max = 50, metadata = true): Promise<StChatSummary[]> {
  const data = await stPostJson<StChatSummary[] | { error?: boolean }>('/api/chats/recent', {
    max,
    metadata,
  })
  return Array.isArray(data) ? data : []
}

/** 会话搜索结果（ST /api/chats/search 返回；注意主键是 file_name 而非 recent 的 file_id） */
export interface StChatSearchHit {
  file_name: string
  file_size?: string
  message_count?: number
  last_mes?: number | string
  preview_message?: string
}

/**
 * 会话全文搜索（ST /api/chats/search）：query 按空格分词，匹配消息正文。
 * avatar_url 指定角色目录（ST 侧中间件要求必填）。
 */
export async function searchChats(query: string, avatarUrl: string): Promise<StChatSearchHit[]> {
  const data = await stPostJson<StChatSearchHit[] | { error?: boolean }>('/api/chats/search', {
    query,
    avatar_url: avatarUrl,
  })
  return Array.isArray(data) ? data : []
}

/**
 * 读取会话消息。
 * 返回 JSONL 解析后的数组，首项是元数据头（{chat_metadata,user_name,character_name}，无 mes）。
 */
export async function getChat(avatarUrl: string, fileName: string): Promise<StChatMessage[]> {
  const data = await stPostJson<StChatMessage[] | Record<string, never>>('/api/chats/get', {
    avatar_url: avatarUrl,
    file_name: fileName,
  })
  return Array.isArray(data) ? data : []
}

export interface SaveChatResult {
  ok?: boolean
}

/**
 * 保存会话（消息数组整体写回）。
 * ST 带文件完整性校验：若文件被外部改动会返回 400 {error:'integrity'}，
 * 此时以 force=true 重试（我们的场景是单一写入方，冲突即覆盖）。
 */
export async function saveChat(
  avatarUrl: string,
  fileName: string,
  chat: StChatMessage[],
  force = false,
): Promise<void> {
  try {
    await stPostJson<SaveChatResult>('/api/chats/save', {
      avatar_url: avatarUrl,
      file_name: fileName,
      chat,
      force,
    })
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e)
    if (!force && /integrity|400/.test(msg)) {
      await stPostJson<SaveChatResult>('/api/chats/save', {
        avatar_url: avatarUrl,
        file_name: fileName,
        chat,
        force: true,
      })
      return
    }
    throw e
  }
}

/** 删除会话文件（chatfile 需含 .jsonl） */
export async function deleteChat(avatarUrl: string, chatFile: string): Promise<void> {
  await stPostJson('/api/chats/delete', { avatar_url: avatarUrl, chatfile: chatFile })
}

/** 重命名会话文件 */
export async function renameChat(
  avatarUrl: string,
  originalFile: string,
  renamedFile: string,
): Promise<void> {
  await stPostJson('/api/chats/rename', {
    avatar_url: avatarUrl,
    original_file: originalFile,
    renamed_file: renamedFile,
  })
}

/* ------------------------------------------------------------------ *
 * 设置
 * ------------------------------------------------------------------ */

/** ST 全局设置。后端把 settings 存成 JSON 字符串，这里解开 */
export async function getSettings(): Promise<StSettingsLite> {
  const data = await stPostJson<{ settings?: string | StSettingsLite }>('/api/settings/get')
  const raw = data?.settings
  if (typeof raw === 'string') {
    try {
      return JSON.parse(raw) as StSettingsLite
    } catch {
      return {}
    }
  }
  return (raw ?? {}) as StSettingsLite
}

/**
 * 保存完整 settings.json（body 即 settings 本体，不包 {settings:...} —— 三条铁律之一）。
 * 供 extension_settings 类扩展数据写入（regex/翻译/快捷回复等）；调用方自行保证
 * 「读全量 → 合并 → 写全量」。
 */
export async function saveSettingsFull(settings: unknown): Promise<void> {
  await stPostVoid('/api/settings/save', settings as Record<string, unknown>)
}

/* ---------------- auto-continue（M-19，power_user.auto_continue） ---------------- */

export interface AutoContinueSettings {
  enabled: boolean
  /** 目标回复 token 数，低于则自动续写 */
  targetLength: number
}

export async function loadAutoContinue(): Promise<AutoContinueSettings> {
  const s = await getSettings()
  const pu = (s as Record<string, unknown>).power_user as Record<string, unknown> | undefined
  const ac = (pu?.auto_continue ?? {}) as Record<string, unknown>
  return {
    enabled: ac.enabled === true,
    targetLength: typeof ac.target_length === 'number' ? ac.target_length : 400,
  }
}

/** 写回 ST 的 auto_continue 设置（读改写 power_user，其余字段原样透传） */
export async function saveAutoContinue(v: AutoContinueSettings): Promise<void> {
  const s = (await getSettings()) as Record<string, unknown>
  const pu = (s.power_user as Record<string, unknown> | undefined) ?? {}
  s.power_user = {
    ...pu,
    auto_continue: {
      ...(pu.auto_continue as Record<string, unknown> | undefined),
      enabled: v.enabled,
      target_length: Math.max(0, Math.floor(v.targetLength)),
      allow_chat_completions: true,
    },
  }
  await saveSettingsFull(s)
}

/**
 * 写回 ST 全局设置。
 *
 * ⚠⚠ **这个接口有两个坑，用错会毁掉用户配置** ⚠⚠
 *
 * ST 服务端实现是 `writeFileAtomicSync(path, JSON.stringify(request.body))`
 * （`sillytavern/src/endpoints/settings.js:209`）—— **整体覆盖，传什么写什么**：
 *   1. **绝不能传 `{ settings: ... }` 包装对象** —— 它会把包装对象本身写成配置
 *      （settings.json 会变成 `{"settings":"{...}"}`，彻底损坏）
 *   2. **绝不能只传部分字段** —— 没传的键会被**直接抹掉**
 *
 * 因此本函数只负责「把一个完整对象发出去」，
 * **合并逻辑必须在调用方完成**（见 `mapping.ts` 的读-改-写流程）。
 */
export async function saveSettings(settings: StSettingsLite): Promise<void> {
  await stPostJson('/api/settings/save', settings)
}

/* ------------------------------------------------------------------ *
 * 静态资源
 * ------------------------------------------------------------------ */

/**
 * 角色头像缩略图 URL。
 * ⚠ 必须拼上 stBase()：开发环境 stBase() 为空 → 相对路径走 Vite 代理；
 *   生产环境 stBase() = Tauri 中继地址 → 绝对路径。若写成纯相对路径，
 *   生产环境会去请求 Tauri 应用自身的源，必然 404。
 * 实测：/thumbnail?type=avatar&file=x.png → 200（约 13KB，原图 550KB）
 */
export function thumbnailUrl(type: StThumbnailType, file: string): string {
  return `${stBase()}/thumbnail?type=${type}&file=${encodeURIComponent(file)}`
}

/** 角色头像缩略图（便捷封装） */
export function avatarThumb(avatar: string): string {
  // 空头像别拼出 `?file=` 这种必失败的 URL，交给 Avatar 组件走占位
  if (!avatar) return ''
  return thumbnailUrl('avatar', avatar)
}

/** 角色原图 URL（缩略图不可用时兜底） */
export function avatarOriginal(avatar: string): string {
  return `${stBase()}/characters/${encodeURIComponent(avatar)}`
}
