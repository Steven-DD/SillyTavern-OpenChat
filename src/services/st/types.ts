/**
 * SillyTavern 数据模型（M3 · 字段名全部来自 2026-09-22 真实接口实测）
 *
 * 实测来源：app/scripts/probe-api.mjs → logs/api-probe.txt
 * 端点对照：
 *   POST /api/characters/all      角色卡列表
 *   POST /api/characters/get      单个角色卡（浅层/深层）
 *   POST /api/characters/chats    某角色的会话文件列表
 *   POST /api/chats/recent        跨角色最近会话（会话列表数据源）
 *   POST /api/chats/get           读会话（JSONL → 消息数组，首行为元数据头）
 *   POST /api/chats/save          写会话（消息数组整体写回）
 *   POST /api/settings/get|save   全局设置
 */

/** 角色卡（ST 「联系人」）。spec = chara_card_v2 / chara_card_v3 */
export interface StCharacter {
  name: string
  description: string
  personality: string
  scenario: string
  /** 开场白 */
  first_mes: string
  /** 对话示例（few-shot，含 <START> 与 {{user}}/{{char}} 宏） */
  mes_example: string
  /** 头像文件名，如 default_Seraphina.png */
  avatar: string
  /** 最近一次会话名（无扩展名） */
  chat: string
  create_date: string
  talkativeness: string | number
  fav: boolean
  creatorcomment?: string
  spec?: string
  spec_version?: string
  tags?: string[]
  /** 毫秒时间戳 */
  date_added?: number
  chat_size?: number
  date_last_chat?: number
  /** v2/v3 卡原始数据（嵌套一层，字段与顶层同名） */
  data?: Partial<StCharacter> & Record<string, unknown>
  json_data?: string
  [k: string]: unknown
}

/* ------------------------------------------------------------------ *
 * 世界书（World Info / Lorebook）
 * ------------------------------------------------------------------ */

/**
 * 世界书条目 —— ST 内部格式（/api/worldinfo/get 返回的 entries 值）。
 * 与 v2 spec 的 character_book entry 字段不同（key/keysecondary/order/数字 position），
 * 转换关系见 data.ts 的 convertCharacterBook（复刻 ST world-info.js:5617）。
 */
export interface StWorldEntry {
  uid: number
  /** 主关键词（触发词） */
  key: string[]
  /** 次级关键词（selective 时配合主关键词） */
  keysecondary: string[]
  /** 条目标题 / 备注 */
  comment: string
  /** 注入 prompt 的内容 */
  content: string
  /** 常驻条目（不依赖关键词，始终注入） */
  constant: boolean
  /** 启用次级关键词 */
  selective: boolean
  /** 插入顺序（同 ST v2 的 insertion_order） */
  order: number
  /** 注入位置：0=角色定义前 1=角色定义后 2=ANTop */
  position: number
  /** 是否停用 */
  disable?: boolean
  addMemo?: boolean
  displayIndex?: number
  probability?: number
  useProbability?: boolean
  depth?: number
  selectiveLogic?: number
  caseSensitive?: boolean | null
  scanDepth?: number | null
  matchWholeWords?: boolean | null
  role?: number
  group?: string
  vectorized?: boolean
  /** 不参与 token 预算扣减（world-info.js:4095） */
  ignoreBudget?: boolean
  /** 不被其它条目递归扫描激活 */
  excludeRecursion?: boolean
  /** 其内容不再触发递归扫描 */
  preventRecursion?: boolean
  /** 递归扫描延迟 N 步后才可激活 */
  delayUntilRecursion?: number
  /** 扫描来源扩展：把人设/角色卡各字段纳入关键词扫描 */
  matchPersonaDescription?: boolean
  matchCharacterDescription?: boolean
  matchCharacterPersonality?: boolean
  matchCharacterDepthPrompt?: boolean
  matchScenario?: boolean
  matchCreatorNotes?: boolean
  /** inclusion group（同组按 groupWeight 掷骰只激活一个） */
  groupOverride?: boolean
  groupWeight?: number
  useGroupScoring?: boolean | null
  /** EM 位置（5/6）时的 outlet 名称（position 7） */
  outletName?: string
  /** 外部自动化 id（ST 扩展用，App 透传） */
  automationId?: string
  /** timedEffects：激活后保持 N 步 / 冷却 N 步 / 延迟 N 步（world-info.js:4118-4120） */
  sticky?: number | null
  cooldown?: number | null
  delay?: number | null
  extensions?: Record<string, unknown>
  triggers?: string[]
  [k: string]: unknown
}

/** 世界书文件（/api/worldinfo/get 与 /edit 的 data 结构） */
export interface StWorldBook {
  /** uid → 条目。ST 的 world 文件就是这一个 map（可能带 originalData 等附加键） */
  entries: Record<string, StWorldEntry>
  /** 导入内嵌书时保留的 v2 原始数据（ST 导入行为） */
  originalData?: unknown
  name?: string
  description?: string
  scan_depth?: number
  token_budget?: number
  recursive_scanning?: boolean
  [k: string]: unknown
}

/** /api/worldinfo/list 返回项 */
export interface StWorldListItem {
  /** 不含扩展名的文件名 —— get/delete 用这个（也是 name） */
  file_id: string
  /** 显示名（world 文件里的 name 字段，缺省 = file_id） */
  name: string
  extensions?: Record<string, unknown>
}

/* ------------------------------------------------------------------ *
 * v2 角色卡内嵌世界书（character_book，spec 格式）
 * ------------------------------------------------------------------ */

/** chara_card_v2 的 character_book.entries[]（林晚棠卡即此格式） */
export interface StCharacterBookEntry {
  keys: string[]
  content: string
  enabled: boolean
  insertion_order: number
  case_sensitive?: boolean
  name?: string
  priority?: number
  comment?: string
  selective?: boolean
  secondary_keys?: string[]
  constant?: boolean
  /** spec 只有两个值；数字 position 走 extensions */
  position?: 'before_char' | 'after_char'
  extensions?: Record<string, unknown>
  id?: number
}

/** chara_card_v2 的 data.character_book */
export interface StCharacterBook {
  name?: string
  description?: string
  scan_depth?: number
  token_budget?: number
  recursive_scanning?: boolean
  extensions?: Record<string, unknown>
  entries: StCharacterBookEntry[]
}

/** 会话文件摘要（/api/characters/chats 与 /api/chats/recent 的返回项） */
export interface StChatSummary {  /** 含扩展名的文件名，如 "Seraphina - 2023-5-12 @21h 32m 29s 224ms.jsonl" */
  file_name: string
  /** 不含扩展名的会话名 —— 调用 get/save 时传这个 */
  file_id: string
  file_size?: string
  /** 消息条数 */
  chat_items?: number
  /** 最后一条消息时间（毫秒时间戳） */
  last_mes?: number
  /** 最后一条消息预览文本 */
  mes?: string
  /** 归属角色头像文件名（recent 接口带） */
  avatar?: string
  /** 群聊 id（群聊才有） */
  group?: string
  metadata?: Record<string, unknown>
  [k: string]: unknown
}

/**
 * 会话消息（JSONL 单行）。
 * 首行是元数据头 { chat_metadata, user_name, character_name }，无 mes 字段。
 */
export interface StChatMessage {
  name?: string
  is_user?: boolean
  /** ISO 字符串；旧数据可能是毫秒时间戳 */
  send_date?: string | number
  mes?: string
  extra?: Record<string, unknown>
  is_system?: boolean
  /** swipe 备选回复（ST 格式：与 mes 平级；当前显示 mes = swipes[swipe_id]） */
  swipes?: unknown[]
  swipe_id?: number
  // 仅首行（元数据头）
  chat_metadata?: Record<string, unknown>
  user_name?: string
  character_name?: string
}

/** ST 全局设置的关键子集 */
export interface StSettingsLite {
  main_api?: string
  chat_completion_source?: string
  preset_settings?: string
  username?: string
  first_run?: boolean
  amount_gen?: number
  max_context?: number
  api_server?: string
  [k: string]: unknown
}

/** 缩略图类型（对应 ST /thumbnail?type=…） */
export type StThumbnailType = 'avatar' | 'persona' | 'bg'
