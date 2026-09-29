/**
 * 会话状态
 *
 * 一条「会话」= ST 侧的一个 <角色>/<会话名>.jsonl 文件。
 * 发消息流程：push 用户消息 → 组装 prompt（角色卡注入）→ SSE 流式 → 落盘回 ST。
 */
import { defineStore, acceptHMRUpdate } from 'pinia'
import { generate, probeSt } from '@/services/st/api'
import {
  deleteChat,
  getChat,
  getSettings,
  listCharacterChats,
  recentChats,
  renameChat,
  saveChat,
} from '@/services/st/data'
import {
  applyAuthorsNote,
  applyChatGenOverride,
  applySummary,
  applyTimedEffects,
  chatAuthorsNote,
  chatGenOverride,
  chatSummary,
  chatTimedEffects,
  chatVariablesOf,
  extractScriptInjects,
  createChatSeed,
  defaultChatName,
  isChatHeader,
  stTimestamp,
  toDisplayMessages,
  toStMessages,
  type AuthorsNote,
  type ChatGenOverride,
  type DisplayMessage,
  type TimedWorldInfo,
} from '@/services/st/chatdoc'
import { buildPrompt, buildSystemPrompt, withContinue, withImpersonate, type ContextTemplate, type PromptMessage, type WiPlacement } from '@/services/st/prompt'
import { loadPm } from '@/services/st/promptmanager'
import { enqueueChatWrite } from '@/services/st/writequeue'
import {
  createGroup as apiCreateGroup,
  deleteGroup,
  editGroup,
  fileToDataUrl,
  getGroupChat,
  groupChatInfo,
  listGroups,
  saveGroupChat,
  uploadGroupAvatarDataUrl,
  type GroupChatLine,
  type StGroup,
} from '@/services/st/groups'
import {
  groupSessionKey,
  pickSpeakers,
  type GroupMemberMeta,
} from '@/services/st/group-orchestrate'
import { chatPersonaId, resolvePersona, setChatPersona } from '@/services/st/persona'
import { checkWorldInfo, collectActiveEntries, type WiGlobals } from '@/services/st/worldinfo'
import type { StCharacter, StChatMessage, StChatSummary } from '@/services/st/types'
import { useCharacterStore } from './character'
import { usePersonaStore } from './persona'
import { useSettingsStore } from './settings'
import { chatMutateLines, type ChatLineOp } from '@/services/tauri/plugins'
import {
  addSessionUsage,
  calcCost,
  estimateTokens,
  getSessionUsage,
  moveSessionUsage,
  type SessionUsage,
} from '@/services/usage'
import { countTokensRemote } from '@/services/st/tokenizer'
import { generateText, messagesToPrompt } from '@/services/st/textgen'
import { runRegex, activeRegexScripts, loadRegexScripts, REGEX_PLACEMENT } from '@/services/st/regex'
import { loadQuickReplies } from '@/services/st/extensions'
import { loadTranslateSettings, translateText } from '@/services/st/translate'
import { speakText, stopSpeech } from '@/services/st/speech'
import {
  generateSummaryPrompt,
  loadMemoryAppEnabled,
  loadMemorySettings,
  summaryInjectionBlock,
} from '@/services/st/memory'
import {
  hashMessage,
  insertVectorItems,
  listVectorHashes,
  loadVectorSettings,
  loadVectorsAppEnabled,
  queryVectors,
  vectorsCollectionId,
  type VectorItem,
} from '@/services/st/vectors'
import { emptyContext, getGlobalVarStore, getStringHash, runMacros, type MacroContext, type MacroVariables } from '@/services/st/macros'
import { runScript, type ScriptContext } from '@/services/st/stscript'
import { formatInstructChat, loadInstruct, type InstructSettings } from '@/services/st/instruct'
import { loadAutoContinue, saveAutoContinue, type AutoContinueSettings } from '@/services/st/data'

/** 会话唯一键：头像 + 会话文件名（均不含扩展名语义，file_id 已去扩展名） */
export function sessionKey(avatar: string, fileId: string): string {
  return `${avatar}::${fileId}`
}

/** 新建会话配置（弹窗 → newChatWithConfig） */
export interface NewChatOptions {
  /** 会话文件名（ST jsonl 名，不含扩展名） */
  fileId: string
  /** 开场白文本（空 = 用角色卡首条消息） */
  greetingText?: string
  /** 锁定人设 id（空 = 跟随默认人设） */
  personaId?: string
  /** 会话级生成参数覆盖（不传 = 跟随全局设置） */
  genOverride?: ChatGenOverride
}

/* ---- 本地持久化（置顶 / 显示别名，均为壳侧数据，不进 ST） ---- */
const PIN_KEY = 'app.chat.pins'
const ALIAS_KEY = 'app.chat.aliases'
/** 会话配置摘要（人设锁/自定义参数，ST recent 接口不带 metadata，本地记账） */
const SESS_META_KEY = 'app.chat.sessmeta'

function loadPins(): string[] {
  try {
    const v = JSON.parse(localStorage.getItem(PIN_KEY) ?? '[]')
    return Array.isArray(v) ? v.filter((x) => typeof x === 'string') : []
  } catch {
    return []
  }
}
function loadAliases(): Record<string, string> {
  try {
    const v = JSON.parse(localStorage.getItem(ALIAS_KEY) ?? '{}')
    return v && typeof v === 'object' ? (v as Record<string, string>) : {}
  } catch {
    return {}
  }
}
interface SessMetaEntry {
  personaId?: string
  gen?: boolean
}
function loadSessMeta(): Record<string, SessMetaEntry> {
  try {
    const v = JSON.parse(localStorage.getItem(SESS_META_KEY) ?? '{}')
    return v && typeof v === 'object' ? (v as Record<string, SessMetaEntry>) : {}
  } catch {
    return {}
  }
}

export interface SessionItem {
  key: string
  avatar: string
  fileId: string
  /** 角色显示名（本地字典查不到时回落文件名） */
  characterName: string
  /** 列表显示名：用户重命名过的会话用别名，否则用角色名 */
  displayName: string
  /** 最后一条消息时间（毫秒） */
  lastMes?: number
  /** 最后一条消息预览 */
  preview: string
  messageCount: number
  /** 本会话锁定的人设 id（本地摘要，无 = 未锁定） */
  personaId?: string
  /** 群聊会话（avatar 为空，displayName = 群名） */
  isGroup?: boolean
  /** 本会话是否有自定义生成参数（本地摘要） */
  genCustom?: boolean
}

/** 组装 prompt 的结果摘要（给状态条显示 token 用量） */
export interface PromptInfo {
  inputTokens: number
  trimmed: number
  examplesDropped: boolean
  /** 本次生成激活的世界书条目数 */
  wiActivated: number
  /** 实际成功使用的模型 */
  usedModel: string
}

export const useChatStore = defineStore('chat', {
  state: () => ({
    // ── 会话列表 ──
    sessions: [] as SessionItem[],
    sessionsLoading: false,
    /** 置顶的会话 key 列表（本地持久化，ST 侧无此概念） */
    pinnedKeys: loadPins(),
    /** 会话显示别名（重命名用，键为 sessionKey，本地持久化） */
    aliases: loadAliases(),
    /** 会话配置摘要（人设锁/自定义参数，本地持久化） */
    sessMeta: loadSessMeta(),

    // ── 当前会话 ──
    currentAvatar: '',
    currentFile: '',
    /** 会话锁定的人设 id（来自 chat_metadata.persona，null = 跟随默认人设） */
    lockedPersonaId: null as string | null,
    /** 会话级生成参数覆盖（来自 chat_metadata.app_gen，null = 跟随全局设置） */
    genOverride: null as ChatGenOverride | null,
    /** 作者注释（来自 chat_metadata.note_*，null = 未设置） */
    authorsNote: null as AuthorsNote | null,
    messages: [] as DisplayMessage[],
    loadingChat: false,
    saving: false,

    // ── 生成 ──
    streaming: false,
    /** 当前生成的中断控制器（streaming 期间非空，「停止生成」触发 abort） */
    abortCtl: null as AbortController | null,
    promptInfo: null as PromptInfo | null,
    /** 最近一轮生成的 token 用量与费用（null = 本会话还没生成过） */
    lastUsage: null as {
      input: number
      output: number
      costUsd: number | null
      /** 输入是否为接口实测（false = 本地估算） */
      measured: boolean
      model: string
    } | null,
    /** 本会话累计用量（localStorage，键 = sessionKey） */
    sessionUsage: { input: 0, output: 0, cost: 0, turns: 0 } as SessionUsage,

    // ── 连接 / 错误 ──
    conn: 'unknown' as 'unknown' | 'online' | 'offline',
    error: '',
    /** 上一次生成的错误（用于气泡与状态条） */
    lastError: '',

    /** ST 侧用户名（{{user}} 宏），从 /api/settings/get 读 */
    userName: 'User',
    /** ST 代写指令模板（settings.impersonation_prompt） */
    impersonationPrompt: '',
    /** 当前会话是否已有 ST 对应文件（未落盘 = 全新会话） */
    dirty: false,

    /** 新建会话配置弹窗开关（中栏「＋ 新建会话」与聊天区空态共用） */
    newChatOpen: false,

    /** 消息翻译结果（键 = 消息下标；显示/隐藏由气泡端切换，不删除缓存） */
    translations: {} as Record<number, string>,
    /** 正在翻译的消息下标（键 = 消息下标，防止重复请求） */
    translating: {} as Record<number, boolean>,
    /** 正在朗读的消息下标（-1 = 无） */
    speakingIndex: -1,
    /** 快捷回复按钮（extension_settings.stchat_quick_replies） */
    quickReplies: [] as { label: string; message: string; enabled: boolean; mode?: 'send' | 'insert' }[],

    /** 总结记忆（chat_metadata.summary） */
    summary: '',
    /** 世界书 timedEffects 计数表（chat_metadata.timedEffects） */
    timedState: {} as TimedWorldInfo,
    /** 宏变量（chat_metadata.variables） */
    chatVariables: {} as Record<string, unknown>,
    /** {{pick}} 重置种子（chat_metadata.pick_reroll_seed；/reroll-pick 写入） */
    pickRerollSeed: null as number | null,
    /** chat_metadata.script_injects（/inject 持久化结果；openSession 捕获） */
    scriptInjects: {} as Record<string, { value?: string; position?: number; depth?: number; role?: number }>,
    /** 群聊会话的 chat_metadata（落盘时随 header 写回，避免清空 ST 侧元数据） */
    groupMetadata: {} as Record<string, unknown>,
    /** chat_variables 防抖落盘计时器 */
    chatVarFlushTimer: null as ReturnType<typeof setTimeout> | null,

    /* ---- 群聊---- */
    /** 当前打开的群（null = 单角色会话） */
    group: null as StGroup | null,
    /** 群聊文件 id（group_chats/<id>.jsonl） */
    groupChatId: '',
    /** 群聊行（header 之外的消息行，落盘即整文件） */
    groupLines: [] as GroupChatLine[],
    /** 群列表缓存（loadSessions / 联系人页共用） */
    groupsList: [] as StGroup[],
    /** 上一发言成员 avatar（SWAP 轮换用） */
    lastSpeaker: '',
    /** STscript /echo 输出（状态条短暂展示） */
    scriptEcho: '',
    /** 群聊自动发言（ST is_group_automode_enabled，间隔 = group.auto_mode_delay 秒） */
    groupAutoMode: false,
    groupAutoModeTimer: null as ReturnType<typeof setInterval> | null,
    /** 会话列表延迟刷新定时器（每条消息落盘后全量拉 recentChats 太重，合并之） */
    sessionsRefreshTimer: null as ReturnType<typeof setTimeout> | null,
    /** ST Instruct 模板设置（power_user.instruct，undefined = 未加载） */
    instructData: undefined as InstructSettings | undefined,
    /** auto-continue：回复 token 数低于目标时自动续写 */
    autoContinueEnabled: false,
    autoContinueTarget: 400,
    /** 上一次生成是否被用户中断（auto-continue 据此跳过续写） */
    lastGenAborted: false,
  }),

  getters: {
    /**
     * Text Completion 的 prompt/stop 组装：Instruct 启用时按 ST 模板拼接，
     * 否则退回默认「名: 内容」级别拼接。opts 透传 continue/impersonate 修饰。
     */
    instructPromptOf(state): (
      messages: PromptMessage[],
      charName: string,
      opts?: { isContinue?: boolean; isImpersonate?: boolean },
    ) => { prompt: string; stop?: string[] } {
      return (messages, charName, opts) => {
        const ins = state.instructData
        if (ins?.enabled) {
          const r = formatInstructChat(messages, ins, state.userName, charName, opts)
          return { prompt: r.text, stop: r.stop }
        }
        return { prompt: messagesToPrompt(messages, charName, state.userName) }
      }
    },
    /**
     * 当前会话对应的角色卡。
     * 优先命中当前会话头像；否则回落角色 store 的选中项。
     */
    currentCharacter(state): StCharacter | null {
      const chars = useCharacterStore()
      const avatar = state.currentAvatar || chars.currentAvatar
      if (!avatar) return null
      return chars.list.find((c) => c.avatar === avatar) ?? null
    },
    sessionKeyOf(state): string {
      if (state.group) return groupSessionKey(state.group.id, state.groupChatId)
      return state.currentAvatar && state.currentFile
        ? sessionKey(state.currentAvatar, state.currentFile)
        : ''
    },
    /** 群成员元表数组（信息面板展示用） */
    groupMemberMetaList(state): GroupMemberMeta[] {
      if (!state.group) return []
      const chars = useCharacterStore()
      return state.group.members.map((a) => {
        const c = chars.list.find((x) => x.avatar === a)
        return {
          avatar: a,
          name: c?.name ?? a.replace(/\.png$/i, ''),
          talkativeness: Number((c as { talkativeness?: number } | undefined)?.talkativeness ?? 0.5),
        }
      })
    },
  },

  actions: {
    async refreshConn() {
      this.conn = (await probeSt()) ? 'online' : 'offline'
    },

    /** 读 ST 全局设置里的用户名（{{user}} 宏用） */
    async loadStUserName() {
      const s = useSettingsStore()
      try {
        const st = await getSettings()
        if (st.username) {
          this.userName = String(st.username)
          s.stUserName = this.userName
        }
        if (typeof st.max_context === 'number' && st.max_context > 0) s.maxContext = st.max_context
        // Instruct 模板（Text Completion 拼接用）
        this.instructData = await loadInstruct()
        // auto-continue 设置
        const ac = await loadAutoContinue().catch(() => null)
        if (ac) {
          this.autoContinueEnabled = ac.enabled
          this.autoContinueTarget = ac.targetLength
        }
        // ST 代写指令模板
        const ip = (st as Record<string, unknown>).impersonation_prompt
        if (typeof ip === 'string' && ip.trim()) this.impersonationPrompt = ip
      } catch {
        /* 读不到就用默认值，不阻塞聊天 */
      }
    },

    /* ---------------- 会话列表 ---------------- */

    async loadSessions() {
      this.sessionsLoading = true
      try {
        const chars = useCharacterStore()
        const raw: StChatSummary[] = await recentChats(60, true)
        this.sessions = raw
          .filter((r) => r.file_id && (r.avatar || r.group))
          .map((r) => {
            const avatar = String(r.avatar ?? '')
            const name =
              chars.list.find((c) => c.avatar === avatar)?.name ??
              avatar.replace(/\.png$/i, '')
            const key = sessionKey(avatar, r.file_id)
            const meta = this.sessMeta[key]
            return {
              key,
              avatar,
              fileId: r.file_id,
              characterName: name,
              displayName: this.aliases[key] ?? name,
              lastMes: typeof r.last_mes === 'number' ? r.last_mes : undefined,
              preview: String(r.mes ?? '').replace(/\s+/g, ' ').slice(0, 60),
              messageCount: r.chat_items ?? 0,
              personaId: meta?.personaId,
              genCustom: meta?.gen,
            }
          })

        // 群组会话（ST recent 返回 {file_id: chatId, group: groupId} 条目）
        try {
          this.groupsList = await listGroups()
          const groupById = new Map(this.groupsList.map((g) => [g.id, g]))
          for (const r of raw) {
            const groupId = String((r as Record<string, unknown>).group ?? '')
            if (!groupId) continue
            const g = groupById.get(groupId)
            const key = groupSessionKey(groupId, r.file_id)
            const meta = this.sessMeta[key]
            this.sessions.push({
              key,
              avatar: '',
              fileId: r.file_id,
              characterName: g?.name ?? groupId,
              displayName: this.aliases[key] ?? g?.name ?? groupId,
              lastMes: typeof r.last_mes === 'number' ? r.last_mes : undefined,
              preview: String(r.mes ?? '').replace(/\s+/g, ' ').slice(0, 60),
              messageCount: r.chat_items ?? 0,
              personaId: meta?.personaId,
              genCustom: meta?.gen,
              isGroup: true,
            })
          }
        } catch {
          /* 群组列表读取失败不阻断单角色会话列表 */
        }
        this.sortSessions()
      } catch (e) {
        this.error = e instanceof Error ? e.message : String(e)
      } finally {
        this.sessionsLoading = false
      }
    },

    /**
     * 把当前会话的配置摘要写入本地映射（会话列表徽标用）。
     * ST 的 recent 接口不返回 chat_metadata，逐个读文件太重，
     * 故在打开/修改配置时本地记账；两项都无则清掉条目。
     */
    syncSessMeta() {
      if (!this.currentAvatar || !this.currentFile) return
      const key = sessionKey(this.currentAvatar, this.currentFile)
      const personaId = this.lockedPersonaId ?? undefined
      const gen = !!this.genOverride
      if (!personaId && !gen) delete this.sessMeta[key]
      else this.sessMeta[key] = { personaId, gen }
      localStorage.setItem(SESS_META_KEY, JSON.stringify(this.sessMeta))
    },

    /** 列表排序：置顶优先，其余按最后消息时间倒序 */
    sortSessions() {      const pins = new Set(this.pinnedKeys)
      this.sessions.sort((a, b) => {
        const pa = pins.has(a.key) ? 1 : 0
        const pb = pins.has(b.key) ? 1 : 0
        if (pa !== pb) return pb - pa
        return (b.lastMes ?? 0) - (a.lastMes ?? 0)
      })
    },

    isPinned(key: string): boolean {
      return this.pinnedKeys.includes(key)
    },

    /** 读会话的显示别名（无则 null） */
    aliasOf(key: string): string | null {
      return this.aliases[key] ?? null
    },

    /** 置顶 / 取消置顶（本地即时重排，不打 ST） */
    togglePin(avatar: string, fileId: string) {
      const key = sessionKey(avatar, fileId)
      const i = this.pinnedKeys.indexOf(key)
      if (i >= 0) this.pinnedKeys.splice(i, 1)
      else this.pinnedKeys.unshift(key)
      localStorage.setItem(PIN_KEY, JSON.stringify(this.pinnedKeys))
      this.sortSessions()
    },

    /* ---------------- 打开 / 新建会话 ---------------- */

    /** 打开一个已存在的会话 */
    /**
     * 锁定/解除锁定本会话的人设（写 chat_metadata.persona）。
     * id = null 表示解除锁定，回到「跟随默认人设」。
     */
    async lockPersona(id: string | null) {
      if (!this.currentAvatar || !this.currentFile) return
      const prev = this.lockedPersonaId
      this.lockedPersonaId = id
      try {
        await setChatPersona(this.currentAvatar, this.currentFile, id)
        this.syncSessMeta()
      } catch (e) {
        this.lockedPersonaId = prev // 失败回滚，避免 UI 与文件不一致
        this.error = e instanceof Error ? e.message : String(e)
      }
    },

    /**
     * 设置会话级生成参数覆盖（写 chat_metadata.app_gen，与新建会话弹窗同一落点）。
     * null = 清除覆盖，回到「跟随全局设置」。生成中禁改。
     */
    async setGenOverride(override: ChatGenOverride | null) {
      if (!this.currentAvatar || !this.currentFile || this.streaming) return
      const prev = this.genOverride
      this.genOverride = override
      try {
        const avatar = this.currentAvatar
        const file = this.currentFile
        await enqueueChatWrite(async () => {
          const raw = await getChat(avatar, file)
          if (!raw.length) throw new Error('会话为空，无法写入参数')
          applyChatGenOverride(raw, override)
          await saveChat(avatar, file, raw)
        })
        this.syncSessMeta()
      } catch (e) {
        this.genOverride = prev // 失败回滚
        this.error = e instanceof Error ? e.message : String(e)
      }
    },

    /**
     * 设置作者注释（写 chat_metadata.note_*，
     * 两边打开同一会话看到的是同一份注释）。null = 清除。生成中禁改。
     */
    async setAuthorsNote(note: AuthorsNote | null) {
      if (!this.currentAvatar || !this.currentFile || this.streaming) return
      const prev = this.authorsNote
      this.authorsNote = note && note.prompt.trim() ? note : null
      try {
        const avatar = this.currentAvatar
        const file = this.currentFile
        await enqueueChatWrite(async () => {
          const raw = await getChat(avatar, file)
          if (!raw.length) throw new Error('会话为空，无法写入作者注释')
          applyAuthorsNote(raw, this.authorsNote)
          await saveChat(avatar, file, raw)
        })
      } catch (e) {
        this.authorsNote = prev // 失败回滚
        this.error = e instanceof Error ? e.message : String(e)
      }
    },

    async openSession(avatar: string, fileId: string) {
      if (this.streaming) return
      // 切会话前把 400ms 防抖窗口内的变量变更落盘（否则旧会话丢改动）
      await this.flushChatVariables()
      this.loadingChat = true
      this.error = ''
      try {
        // 离开群聊态（两套会话互斥）
        this.group = null
        this.groupChatId = ''
        this.groupLines = []
        if (this.groupAutoModeTimer) {
          clearInterval(this.groupAutoModeTimer)
          this.groupAutoModeTimer = null
        }
        this.groupAutoMode = false
        const raw = await getChat(avatar, fileId)
        const chars = useCharacterStore()
        const charName = chars.list.find((c) => c.avatar === avatar)?.name ?? avatar.replace(/\.png$/i, '')
        this.currentAvatar = avatar
        this.currentFile = fileId
        this.messages = toDisplayMessages(raw, { userFallback: this.userName, charFallback: charName })
        this.lockedPersonaId = chatPersonaId(raw)
        this.genOverride = chatGenOverride(raw)
        this.authorsNote = chatAuthorsNote(raw)
        this.summary = chatSummary(raw)
        this.timedState = chatTimedEffects(raw)
        this.chatVariables = chatVariablesOf(raw)
        // {{pick}} 种子（与网页端同字段；无则视为未重置）
        this.pickRerollSeed = Number((raw[0] as { chat_metadata?: { pick_reroll_seed?: unknown } } | undefined)?.chat_metadata?.pick_reroll_seed) || null
        this.scriptInjects = extractScriptInjects(raw[0])
        this.dirty = false
        this.promptInfo = null
        this.lastUsage = null
        this.sessionUsage = getSessionUsage(sessionKey(avatar, fileId))
        this.syncSessMeta()
        // 同步联系人面板的选中态（内含详情加载）
        chars.select(avatar)
      } catch (e) {
        this.error = e instanceof Error ? e.message : String(e)
      } finally {
        this.loadingChat = false
      }
    },

    /** 与指定角色新建会话（写入开场白，立即落盘 ST） */
    async newChat(character: StCharacter) {
      if (this.streaming) return
      const fileId = defaultChatName(character.name)
      const seed = createChatSeed(character, this.userName, character.name, character.avatar)
      try {
        await saveChat(character.avatar, fileId, seed)
      } catch (e) {
        // 落盘失败不阻断本地会话（后端离线时仍可先聊，联网后再存）
        this.error = e instanceof Error ? e.message : String(e)
      }
      this.currentAvatar = character.avatar
      this.currentFile = fileId
      this.messages = toDisplayMessages(seed, {
        userFallback: this.userName,
        charFallback: character.name,
      })
      this.dirty = true
      this.genOverride = null
      this.lastUsage = null
      this.sessionUsage = { input: 0, output: 0, cost: 0, turns: 0 }
      this.chatVariables = {}
      this.syncSessMeta()
      await this.loadSessions()
    },

    /**
     * 按配置新建会话（新建会话配置弹窗的落点）。
     * 成功返回 true；同名会话已存在时拒绝（saveChat 是整体覆盖，绝不能毁掉旧会话）。
     */
    async newChatWithConfig(character: StCharacter, opts: NewChatOptions): Promise<boolean> {
      if (this.streaming) return false
      // 文件名净化（与 renameSession 同规则）
      const fileId = opts.fileId.replace(/[\\/:*?"<>|]/g, '').trim()
      if (!fileId) return false
      if (this.sessions.some((x) => x.avatar === character.avatar && x.fileId === fileId)) {
        this.error = `已存在同名会话「${fileId}」`
        return false
      }
      const seed = createChatSeed(character, this.userName, character.name, character.avatar)
      // 开场白：配置选中的文本非空且种子有消息 → 替换首条角色消息
      const greeting = opts.greetingText?.trim()
      if (greeting && seed.length > 1) {
        seed[1] = { ...seed[1], mes: greeting }
      }
      // 会话元数据：人设锁定 + 生成参数覆盖（chat_metadata 自由结构）
      const override = opts.genOverride
      if (override) applyChatGenOverride(seed, override)
      if (opts.personaId) {
        const head = seed[0] as StChatMessage & { chat_metadata: Record<string, unknown> }
        head.chat_metadata = { ...(head.chat_metadata ?? {}), persona: opts.personaId }
      }
      try {
        await saveChat(character.avatar, fileId, seed)
      } catch (e) {
        this.error = e instanceof Error ? e.message : String(e)
      }
      this.currentAvatar = character.avatar
      this.currentFile = fileId
      this.messages = toDisplayMessages(seed, {
        userFallback: this.userName,
        charFallback: character.name,
      })
      this.lockedPersonaId = opts.personaId ?? null
      this.genOverride = override ?? null
      this.dirty = true
      this.lastUsage = null
      this.sessionUsage = { input: 0, output: 0, cost: 0, turns: 0 }
      this.syncSessMeta()
      await this.loadSessions()
      return true
    },

    /**
     * 打开与角色的最近会话；没有则新建。
     * 供「联系人 → 发消息」使用。
     */
    async openWithCharacter(character: StCharacter) {
      try {
        const list = await listCharacterChats(character.avatar, { metadata: false })
        if (list.length) {
          // 取 mtime 最新的一个
          const newest = [...list].sort((a, b) => (b.last_mes ?? 0) - (a.last_mes ?? 0))[0]
          await this.openSession(character.avatar, newest.file_id)
          return
        }
      } catch {
        /* 无会话目录会返回 {error:true}，按「无会话」处理 */
      }
      await this.newChat(character)
    },

    /* ---------------- 生成 ---------------- */

    /** 把界面消息还原为 ST 会话消息（排除错误/空/发送中的占位） */
    buildHistory(charName: string): StChatMessage[] {
      const usable = this.messages.filter(
        (m) => !m.error && !m.pending && m.content.trim().length > 0,
      )
      const st = toStMessages(usable, charName)
      // 正则脚本（提示词通路，placement 按消息角色，深度 = 距末尾条数）
      const scripts = activeRegexScripts()
      if (scripts.length) {
        const total = st.length
        for (let i = 0; i < total; i++) {
          const msg = st[i]!
          const placement = msg.is_user ? REGEX_PLACEMENT.USER_INPUT : REGEX_PLACEMENT.AI_OUTPUT
          const depth = total - 1 - i
          msg.mes = runRegex(msg.mes ?? '', scripts, {
            placement,
            isPrompt: true,
            depth,
            macroCtx: this.macroCtxFor(),
          })
        }
      }
      return st
    },

    /** 宏上下文（正则 substituteRegex 等复用） */
    macroCtxFor(): MacroContext {
      const s = useSettingsStore()
      return emptyContext({
        charName: this.currentCharacter?.name ?? '',
        userName: this.userName,
        model: s.model,
        maxContext: s.maxContext,
        maxResponse: s.maxTokens,
        chatVars: this.chatVarStore(),
        globalVars: getGlobalVarStore(),
        // {{pick}} 种子：会话哈希（与网页端同为 chatId 的字符串 hash）+ reroll 种子
        chatIdHash: getStringHash(this.group ? this.groupChatId : this.currentFile || ''),
        pickRerollSeed: this.pickRerollSeed,
      })
    },

    /**
     * 组装当前会话的完整 prompt（人设 + 世界书 + AN + 宏上下文）。
     * generateReply / continueReply / impersonate 共用。
     */
    async assemblePrompt(
      char: StCharacter,
      s: ReturnType<typeof useSettingsStore>,
    ): Promise<{ built: ReturnType<typeof buildPrompt>; wiActivated: number }> {
      // Prompt Manager 数据（prompt_order + 三槽）
      const pm = await loadPm()
      // 生效人设：会话锁定优先 → 默认人设 → 列表第一个
      const pStore = usePersonaStore()
      const effective = resolvePersona(
        {
          personas: pStore.personas,
          defaultId: pStore.defaultId,
          userName: pStore.userName,
          depth: pStore.depth,
          role: pStore.role,
        },
        this.lockedPersonaId,
      )

      // 世界书：收集生效条目（角色绑定书 + 全局勾选书）→ 扫描最近消息 → 激活注入
      // timedEffects 状态随扫描推进，有计数时写回 chat_metadata.timedEffects
      let wi: WiPlacement | undefined
      let wiActivated = 0
      try {
        const wiEntries = await collectActiveEntries(char.avatar)
        if (wiEntries.length) {
          const texts = this.messages
            .filter((m) => !m.error && !m.pending && m.content.trim())
            .map((m) => m.content)
          // 全局 WI 参数以服务端 world_info_settings 为基底（跨端一致）；
          // 本地递归开关（世界书页）显式设置时优先
          const wiGlobals: Partial<WiGlobals> = { recursive: false }
          try {
            const w = ((await getSettings()).world_info_settings ?? {}) as Record<string, unknown>
            const num = (v: unknown): number | undefined =>
              Number.isFinite(Number(v)) ? Number(v) : undefined
            wiGlobals.scanDepth = num(w.world_info_depth)
            wiGlobals.budgetPercent = num(w.world_info_budget)
            wiGlobals.budgetCap = num(w.world_info_budget_cap)
            wiGlobals.minActivations = num(w.world_info_min_activations)
            wiGlobals.minActivationsDepthMax = num(w.world_info_min_activations_depth_max)
          } catch {
            /* 服务端设置读不到 → 全用默认值 */
          }
          const localRecursive = localStorage.getItem('app.wi.recursive')
          wiGlobals.recursive =
            localRecursive === '1' ? true : localRecursive === '0' ? false : undefined
          // 修剪/补写前的记录数（修剪可能清空 → 仍需写回以删除 timedWorldInfo 键）
          const hadTimed =
            Object.keys(this.timedState.sticky ?? {}).length +
              Object.keys(this.timedState.cooldown ?? {}).length >
            0
          const res = checkWorldInfo(
            texts,
            wiEntries,
            s.maxContext,
            wiGlobals,
            // turn 基准：服务端按含 header 的 chat.length 计数，App 的
            // messages 不含 header，故 +1 补上 header 计数
            { state: this.timedState, turn: this.messages.length + 1 },
          )
          wiActivated = res.activated.length
          if (res.before || res.after || res.depthEntries.length) {
            wi = { before: res.before, after: res.after, depthEntries: res.depthEntries }
          }
          // timedWorldInfo 有变化（修剪过期 / 新增记录）→ 持久化
          const hasTimed =
            Object.keys(this.timedState.sticky ?? {}).length +
              Object.keys(this.timedState.cooldown ?? {}).length >
            0
          if ((hasTimed || hadTimed) && this.currentAvatar && this.currentFile) {
            const avatar = this.currentAvatar
            const file = this.currentFile
            await enqueueChatWrite(async () => {
              const raw = await getChat(avatar, file)
              if (raw.length) {
                applyTimedEffects(raw, this.timedState)
                await saveChat(avatar, file, raw)
              }
            })
          }        }
      } catch {
        wiActivated = 0 // 世界书失败不阻断生成
      }

      // 总结记忆 / 向量检索注入块
      let memory: { content: string; position: number; depth: number; role: number } | undefined
      let vectors: { content: string; depth: number } | undefined
      try {
        const memCfg = await loadMemorySettings()
        if ((await loadMemoryAppEnabled()) && this.summary.trim()) {
          memory = {
            content: summaryInjectionBlock(this.summary, char.name, this.userName),
            position: memCfg.position,
            depth: memCfg.depth,
            role: memCfg.role,
          }
        }
      } catch {
        /* 记忆设置读取失败不阻断 */
      }
      try {
        if ((await loadVectorsAppEnabled()) && this.currentAvatar && this.currentFile) {
          const vecCfg = await loadVectorSettings()
          const lastUser = [...this.messages].reverse().find((m) => m.role === 'user' && !m.error && !m.pending)
          if (lastUser) {
            const col = vectorsCollectionId(this.currentAvatar, this.currentFile)
            const metas = await queryVectors(col, lastUser.content, vecCfg.top_k, vecCfg.source)
            // metadata.hash 对应消息文本 hash → 稳定映射回消息
            const byHash = new Map(this.messages.map((m) => [hashMessage(m.content), m]))
            const texts = metas
              .map((meta) => {
                const msg = meta.hash !== undefined ? byHash.get(Number(meta.hash)) : undefined
                return msg && !msg.pending && !msg.error ? `${msg.name}: ${msg.content}` : ''
              })
              .filter((t) => t.trim())
            if (texts.length) {
              vectors = { content: vecCfg.template.replace(/\{\{text\}\}/g, texts.join('\n---\n')), depth: vecCfg.depth }
            }
          }
        }
      } catch {
        /* 向量检索失败不阻断 */
      }

      // 深度注入：角色卡 depth_prompt + /inject 持久化（script_injects）
      const injections: { content: string; depth: number; role: number }[] = []
      let systemPrefix = ''
      let systemSuffix = ''
      const cardDp = (
        (char.data as { extensions?: { depth_prompt?: { prompt?: unknown; depth?: unknown; role?: unknown } } } | undefined)
          ?.extensions?.depth_prompt
      )
      if (typeof cardDp?.prompt === 'string' && cardDp.prompt.trim()) {
        injections.push({
          content: cardDp.prompt.trim(),
          depth: Number(cardDp.depth) || 4,
          role: Number(cardDp.role) || 0,
        })
      }
      for (const inj of Object.values(this.scriptInjects ?? {})) {
        const text = String(inj?.value ?? '')
        if (!text.trim()) continue
        const role = Number(inj.role) || 0
        const depth = Number(inj.depth) || 4
        if (inj.position === 2) injections.push({ content: text.trim(), depth, role })
        else if (inj.position === 0) systemPrefix += (systemPrefix ? '\n' : '') + text.trim()
        else systemSuffix += (systemSuffix ? '\n' : '') + text.trim()
      }

      // Text Completion 上下文模板（power_user.context；story_string 等）
      let context: ContextTemplate | null = null
      if (s.genType === 'text') {
        try {
          const pu = ((await getSettings()).power_user ?? {}) as Record<string, unknown>
          const c = (pu.context ?? {}) as Record<string, unknown>
          const str = (v: unknown): string => (typeof v === 'string' ? v : '')
          if (str(c.story_string).trim()) {
            context = {
              storyString: str(c.story_string),
              chatStart: str(c.chat_start),
              exampleSeparator: str(c.example_separator),
              position: Number(c.story_string_position) || 0,
              depth: Number(c.story_string_depth) || 1,
              role: Number(c.story_string_role) || 0,
            }
          }
        } catch {
          /* 服务端设置读不到 → 走传统装配 */
        }
      }

      const built = buildPrompt({
        character: char,
        history: this.buildHistory(char.name),
        userName: this.userName,
        maxContext: s.maxContext,
        includeExamples: s.includeExamples,
        persona: effective
          ? {
              name: effective.name,
              description: effective.description,
              position: effective.position,
              depth: pStore.depth,
              role: pStore.role,
            }
          : null,
        worldInfo: wi,
        authorsNote: this.authorsNote,
        memory,
        vectors,
        // 深度注入（角色卡 depth_prompt + /inject 持久化）与系统提示前后缀
        injections,
        systemPrefix,
        systemSuffix,
        context,
        macro: {
          model: s.model,
          maxResponse: this.genOverride?.maxTokens ?? s.maxTokens,
          swipeId: this.messages[this.messages.length - 1]?.swipeId ?? 0,
          // prompt 组装和 STscript/正则共用同一份 chat_metadata.variables 存储
          chatVars: this.chatVarStore(),
          // {{pick}} 种子
          chatIdHash: getStringHash(this.group ? this.groupChatId : this.currentFile || ''),
          pickRerollSeed: this.pickRerollSeed,
        },
        pm: {
          order: pm.order,
          main: pm.main,
          nsfw: pm.nsfw,
          jailbreak: pm.jailbreak,
        },
      })
      return { built, wiActivated }
    },

    /** CC 通道统一采样参数（会话级覆盖优先） */
    ccSampler(s: ReturnType<typeof useSettingsStore>) {
      return {
        topP: this.genOverride?.topP ?? s.topP,
        topK: s.topK,
        topA: s.topA,
        minP: s.minP,
        frequencyPenalty: s.frequencyPenalty,
        presencePenalty: s.presencePenalty,
        repetitionPenalty: s.repetitionPenalty,
      }
    },

    /** textgen 通道统一采样参数 */
    textSampler(s: ReturnType<typeof useSettingsStore>) {
      return {
        temperature: this.genOverride?.temperature ?? s.temperature,
        topP: this.genOverride?.topP ?? s.topP,
        topK: s.topK,
        minP: s.minP,
        repetitionPenalty: s.repetitionPenalty,
        frequencyPenalty: s.frequencyPenalty,
        presencePenalty: s.presencePenalty,
      }
    },

    /**
     * 生成一条回复并追加到消息列表。
     * 调用前消息列表的**最后一条**应为本轮用户输入。
     */
    async generateReply() {
      const char = this.currentCharacter
      if (!char) {
        this.lastError = '未选择角色卡'
        return
      }
      const s = useSettingsStore()

      // 助手占位气泡（流式写入目标）
      const reply: DisplayMessage = {
        role: 'assistant',
        content: '',
        name: char.name,
        sendDate: Date.now(),
        pending: true,
      }
      this.messages.push(reply)
      this.streaming = true
      this.lastError = ''
      this.lastGenAborted = false

      // 中断控制器：stopGeneration() 触发 abort，fetch 立即断开
      const ctl = new AbortController()
      this.abortCtl = ctl

      try {
        const { built, wiActivated } = await this.assemblePrompt(char, s)

        // 接口实测用量（SSE 尾帧，部分通道不回传 → 回落本地估算）
        let measured: { prompt: number; completion: number } | null = null
        const used =
          s.genType === 'text'
            ? // Text Completion：Instruct 启用时按 Instruct 模板拼接，否则默认级别
              await (async () => {
                const tp = this.instructPromptOf(built.messages, char.name)
                await generateText(
                  {
                    prompt: tp.prompt,
                    stop: tp.stop,
                    model: s.model,
                    backend: s.textBackend,
                    server: s.textServer || undefined,
                    maxTokens: this.genOverride?.maxTokens ?? s.maxTokens,
                    sampler: {
                      temperature: this.genOverride?.temperature ?? s.temperature,
                      topP: this.genOverride?.topP ?? s.topP,
                      topK: s.topK,
                      minP: s.minP,
                      repetitionPenalty: s.repetitionPenalty,
                      frequencyPenalty: s.frequencyPenalty,
                      presencePenalty: s.presencePenalty,
                    },
                    bannedWords: built.bannedWords,
                    signal: ctl.signal,
                  },
                  (delta) => {
                    reply.content += delta
                  },
                  (r) => {
                    reply.reasoning = (reply.reasoning ?? '') + r
                  },
                )
                return s.model
              })()
            : await generate(
          {
            messages: built.messages,
            model: s.model,
            source: s.source,
            // Horde：补全式通道走 Instruct 拼接
            ...(s.source === 'horde' || s.source === 'novel'
              ? { ...this.instructPromptOf(built.messages, char.name), maxContext: s.maxContext }
              : {}),
            // 会话级覆盖优先（chat_metadata.app_gen），无覆盖跟随全局设置
            temperature: this.genOverride?.temperature ?? s.temperature,
            maxTokens: this.genOverride?.maxTokens ?? s.maxTokens,
            sampler: {
              topP: this.genOverride?.topP ?? s.topP,
              topK: s.topK,
              topA: s.topA,
              minP: s.minP,
              frequencyPenalty: s.frequencyPenalty,
              presencePenalty: s.presencePenalty,
              repetitionPenalty: s.repetitionPenalty,
            },
            onUsage: (u) => {
              measured = u
            },
            signal: ctl.signal,
          },
          (delta) => {
            reply.content += delta
          },
          (r) => {
            // 推理内容增量（extra.reasoning 镜像字段）
            reply.reasoning = (reply.reasoning ?? '') + r
          },
                )

        this.promptInfo = {
          inputTokens: built.inputTokens,
          trimmed: built.trimmed,
          examplesDropped: built.examplesDropped,
          wiActivated,
          usedModel: used,
        }

        // 用量与费用：输入优先 SSE 尾帧实测，缺则服务端精确计数，再缺才本地粗估
        const m = measured as { prompt: number; completion: number } | null
        const input =
          m && m.prompt > 0
            ? m.prompt
            : (await countTokensRemote(built.systemPrompt)) ?? built.inputTokens
        const output =
          m && m.completion > 0
            ? m.completion
            : (await countTokensRemote(reply.content + (reply.reasoning ?? ''), used)) ??
              estimateTokens(reply.content + (reply.reasoning ?? ''))
        const costUsd = calcCost(used, { input, output })
        this.lastUsage = {
          input,
          output,
          costUsd,
          measured: !!(m && m.prompt > 0),
          model: used,
        }
        const uKey = this.sessionKeyOf
        if (uKey) {
          addSessionUsage(uKey, { input, output }, costUsd)
          this.sessionUsage = getSessionUsage(uKey)
        }
        reply.pending = false

        if (!reply.content.trim()) {
          reply.error = true
          reply.content = '（模型返回空内容）'
          this.lastError = '模型返回空内容'
        }

        // 生成后置：向量索引 + 自动摘要（静默执行，失败不打断聊天）
        try {
          await this.indexSessionVectors()
          const memCfg = await loadMemorySettings()
          if (this.messages.length % Math.max(1, memCfg.interval) === 0) {
            if (await loadMemoryAppEnabled()) await this.updateSummary()
          }
        } catch {
          /* 后置任务失败不提示气泡错误 */
        }
      } catch (e) {
        reply.pending = false
        if (ctl.signal.aborted) {
          // 主动停止：不算错误。已有内容 → 保留为正常消息；
          // 空内容 → 移除占位气泡
          this.lastGenAborted = true
          this.lastError = ''
          if (!reply.content.trim() && !(reply.reasoning ?? '').trim()) {
            const i = this.messages.indexOf(reply)
            if (i >= 0) this.messages.splice(i, 1)
          }
        } else {
          // 非 abort 错误（断网/上游中断）：**保留已生成内容**——生成几百字后断线
          // 不应把输出整段替换成错误文本。
          // 错误信息放 lastError（UI 有展示）；气泡仅在完全没有产出时才标记为错误。
          this.lastError = e instanceof Error ? e.message : String(e)
          this.conn = 'unknown'
          if (!reply.content.trim() && !(reply.reasoning ?? '').trim()) {
            reply.error = true
            reply.content = this.lastError
          }
        }
      } finally {
        this.streaming = false
        this.abortCtl = null
        this.dirty = true
        void this.refreshConn()
      }
    },

    /** 停止生成：中断当前 SSE 流，已生成部分保留落盘 */
    stopGeneration() {
      this.abortCtl?.abort()
    },

    /** 设置 auto-continue（写回 ST power_user.auto_continue，两端同一配置） */
    async setAutoContinue(v: AutoContinueSettings) {
      this.autoContinueEnabled = v.enabled
      this.autoContinueTarget = v.targetLength
      try {
        await saveAutoContinue(v)
      } catch (e) {
        this.error = e instanceof Error ? e.message : String(e)
      }
    },

    /**
     * auto-continue：
     * - 用服务端真实 tokenizer 计数（失败回落本地估算）
     * - 新增 chunk 需 >5 字符（USABLE_LENGTH）才继续
     * - 无轮数上限（ST 靠目标长度自然收敛），无增量即退出
     * - CC 通道支持 allow_chat_completions 门槛（localStorage app.autoContinue.allowCC，默认开）
     */
    async autoContinueReply(targetTokens: number): Promise<void> {
      const allowCC = localStorage.getItem('app.autoContinue.allowCC') !== '0'
      for (;;) {
        const last = this.messages[this.messages.length - 1]
        if (!last || last.role !== 'assistant' || last.error || this.streaming) return
        const s = useSettingsStore()
        if (s.genType !== 'text' && !allowCC) return
        const tokenCount =
          (await countTokensRemote(last.content, s.genType === 'text' ? undefined : s.model)) ??
          estimateTokens(last.content)
        if (tokenCount >= targetTokens) return
        const before = last.content
        await this.continueReply()
        const after = this.messages[this.messages.length - 1]
        if (!after || after.error) return
        // ST USABLE_LENGTH = 5：本轮新增内容过短 → 停止
        if (after.content.trim().length - before.trim().length <= 5) return
      }
    },

    /** 续写末条 AI 回复：增量追加到现有内容 + 同步当前 swipe */
    async continueReply() {
      if (this.streaming) return
      const char = this.currentCharacter
      const last = this.messages[this.messages.length - 1]
      if (!char || !last || last.role !== 'assistant' || last.pending || last.error || !last.content.trim()) {
        return
      }
      const s = useSettingsStore()
      this.streaming = true
      this.lastError = ''
      const ctl = new AbortController()
      this.abortCtl = ctl
      try {
        const { built } = await this.assemblePrompt(char, s)
        // CC：continue_nudge system 轮 + 半截 assistant 消息收尾（ST continue_prefill=false 默认）
        const messages = withContinue(built.messages)
        // CC continue_postfix：半截内容不带结尾空格时补一个空格，
        // 续写首个增量回填时补上，保证拼接文本连续
        let needsPostfix = s.genType !== 'text' && !/\s$/.test(last.content)
        const syncSwipe = () => {
          if (last.swipes && last.swipeId !== undefined && last.swipes[last.swipeId] !== undefined) {
            last.swipes[last.swipeId] = last.content
          }
        }
        if (s.genType === 'text') {
          // Text：prefill 语义——prompt 止于半截内容，不追加 [Continue] 轮
          const tp = this.instructPromptOf(built.messages, char.name, { isContinue: true })
          await generateText(
            {
              prompt: tp.prompt,
              stop: tp.stop,
              model: s.model,
              backend: s.textBackend,
              server: s.textServer || undefined,
              maxTokens: this.genOverride?.maxTokens ?? s.maxTokens,
              sampler: this.textSampler(s),
              bannedWords: built.bannedWords,
              signal: ctl.signal,
            },
            (d) => {
              last.content += d
              syncSwipe()
            },
          )
        } else {
          await generate(
            {
              messages,
              model: s.model,
              source: s.source,
              // Horde：补全式通道走 Instruct 拼接；continue 同样 prefill 化
              ...(s.source === 'horde' || s.source === 'novel'
                ? {
                    ...this.instructPromptOf(built.messages, char.name, { isContinue: true }),
                    maxContext: s.maxContext,
                  }
                : {}),
              temperature: this.genOverride?.temperature ?? s.temperature,
              maxTokens: this.genOverride?.maxTokens ?? s.maxTokens,
              sampler: this.ccSampler(s),
              signal: ctl.signal,
            },
            (d) => {
              last.content += (needsPostfix ? ' ' : '') + d
              needsPostfix = false
              syncSwipe()
            },
            (r) => {
              last.reasoning = (last.reasoning ?? '') + r
            },
          )
        }
        this.dirty = true
        // 行级落盘（增量失败回落全量）
        const opLine = last.lineIndex
        if (opLine !== undefined && this.currentAvatar && this.currentFile) {
          try {
            await chatMutateLines(this.currentAvatar, this.currentFile, [
              { op: 'replace', index: opLine, json: this.currentMessageJson(last) },
            ])
            this.dirty = false
          } catch {
            await this.saveCurrent()
          }
        } else {
          await this.saveCurrent()
        }
      } catch (e) {
        if (!ctl.signal.aborted) {
          this.lastError = e instanceof Error ? e.message : String(e)
        }
      } finally {
        this.streaming = false
        this.abortCtl = null
        void this.refreshConn()
      }
    },

    /** 代写：以用户身份生成一条消息文本，返回给输入框草稿 */
    async impersonate(): Promise<string> {
      if (this.streaming) return ''
      const char = this.currentCharacter
      if (!char) return ''
      const s = useSettingsStore()
      this.streaming = true
      this.lastError = ''
      const ctl = new AbortController()
      this.abortCtl = ctl
      try {
        const { built } = await this.assemblePrompt(char, s)
        const messages = withImpersonate(
          built.messages,
          this.userName,
          this.impersonationPrompt || undefined,
          this.macroCtxFor(),
        )
        let out = ''
        const onDelta = (d: string) => {
          out += d
        }
        if (s.genType === 'text') {
          // Impersonate：不补 output 前缀（ST 代写不添加 assistant 前缀）
          const tp = this.instructPromptOf(messages, char.name, { isImpersonate: true })
          await generateText(
            {
              prompt: tp.prompt,
              stop: tp.stop,
              model: s.model,
              backend: s.textBackend,
              server: s.textServer || undefined,
              maxTokens: s.maxTokens,
              sampler: this.textSampler(s),
              bannedWords: built.bannedWords,
              signal: ctl.signal,
            },
            onDelta,
          )
        } else {
          await generate(
            {
              messages,
              model: s.model,
              source: s.source,
              // Horde：补全式通道走 Instruct 拼接；impersonate 不补 output 前缀
              ...(s.source === 'horde' || s.source === 'novel'
                ? {
                    ...this.instructPromptOf(messages, char.name, { isImpersonate: true }),
                    maxContext: s.maxContext,
                  }
                : {}),
              temperature: s.temperature,
              maxTokens: s.maxTokens,
              sampler: this.ccSampler(s),
              signal: ctl.signal,
            },
            onDelta,
          )
        }
        return out.trim()
      } catch (e) {
        if (!ctl.signal.aborted) {
          this.lastError = e instanceof Error ? e.message : String(e)
        }
        return ''
      } finally {
        this.streaming = false
        this.abortCtl = null
        void this.refreshConn()
      }
    },

    /** 翻译消息（缓存命中直接返回；显示切换由 MessageBubble 端处理） */
    async translateMessage(index: number) {
      const m = this.messages[index]
      if (!m || this.translating[index] || this.translations[index]) return
      this.translating = { ...this.translating, [index]: true }
      try {
        const s = await loadTranslateSettings()
        const t = await translateText(m.content, s.target_language, s.provider)
        this.translations = { ...this.translations, [index]: t || '（翻译结果为空）' }
      } catch (e) {
        this.lastError = e instanceof Error ? e.message : String(e)
      } finally {
        const next = { ...this.translating }
        delete next[index]
        this.translating = next
      }
    },

    /** 朗读消息（再次点同一条停止） */
    async speakMessage(index: number) {
      const m = this.messages[index]
      if (!m) return
      if (this.speakingIndex === index) {
        stopSpeech() // releaseCurrent 会触发 onEnded 复位指示
        return
      }
      this.speakingIndex = index
      try {
        // 指示在真正播完/被打断时才复位（此前 play() 一 resolve 就复位，朗读期间无高亮）
        await speakText(m.content, () => {
          if (this.speakingIndex === index) this.speakingIndex = -1
        })
      } catch (e) {
        this.lastError = e instanceof Error ? e.message : String(e)
        if (this.speakingIndex === index) this.speakingIndex = -1
      }
    },

    /** 加载扩展数据（正则脚本缓存 + 快捷回复按钮） */
    async loadExtensionData() {
      try {
        await loadRegexScripts(true)
      } catch {
        /* 正则加载失败不阻断聊天 */
      }
      try {
        this.quickReplies = (await loadQuickReplies(true)).filter(
          (q) => q.enabled && q.message.trim(),
        )
      } catch {
        this.quickReplies = []
      }
    },

    /** 向量索引：把未入库的消息送 ST 服务端嵌入（静默） */
    async indexSessionVectors() {
      try {
        if (!(await loadVectorsAppEnabled()) || !this.currentAvatar || !this.currentFile) return
        const cfg = await loadVectorSettings(true)
        const col = vectorsCollectionId(this.currentAvatar, this.currentFile)
        const known = new Set(await listVectorHashes(col, cfg.source))
        const items: VectorItem[] = []
        this.messages.forEach((m, index) => {
          if (m.error || m.pending || !m.content.trim()) return
          const h = hashMessage(m.content)
          if (!known.has(h)) items.push({ hash: h, text: m.content, index })
        })
        await insertVectorItems(col, cfg.source, items)
      } catch {
        /* 索引失败静默（下次生成重试） */
      }
    },

    /** 更新总结记忆：主通道静默生成 → 写 chat_metadata.summary */
    async updateSummary() {
      if (this.streaming) return
      const char = this.currentCharacter
      if (!char || !this.currentAvatar || !this.currentFile) return
      const s = useSettingsStore()
      const cfg = await loadMemorySettings()
      const texts = this.messages
        .filter((m) => !m.error && !m.pending && m.content.trim())
        .map((m) => `${m.name}: ${m.content}`)
      const prompt = generateSummaryPrompt(this.summary, texts, char.name, this.userName, cfg)
      this.streaming = true
      const ctl = new AbortController()
      this.abortCtl = ctl
      try {
        let out = ''
        if (s.genType === 'text') {
          await generateText(
            {
              prompt,
              model: s.model,
              backend: s.textBackend,
              server: s.textServer || undefined,
              maxTokens: 300,
              sampler: { temperature: 0.3 },
              signal: ctl.signal,
            },
            (d) => {
              out += d
            },
          )
        } else {
          await generate(
            {
              messages: [{ role: 'user', content: prompt }],
              model: s.model,
              source: s.source,
              temperature: 0.3,
              maxTokens: 300,
              signal: ctl.signal,
            },
            (d) => {
              out += d
            },
          )
        }
        out = out.trim()
        if (out) {
          this.summary = out
          const avatar = this.currentAvatar
          const file = this.currentFile
          await enqueueChatWrite(async () => {
            const raw = await getChat(avatar, file)
            if (raw.length) {
              applySummary(raw, out)
              await saveChat(avatar, file, raw)
            }
          })
        }
      } catch (e) {
        if (!ctl.signal.aborted) {
          this.lastError = e instanceof Error ? e.message : String(e)
        }
      } finally {
        this.streaming = false
        this.abortCtl = null
      }
    },

    /** 发送用户消息 */
    /* ---------------- STscript ---------------- */

    /** /genraw：raw 直出（最小系统上下文，不套角色卡，不落盘） */
    async genraw(prompt: string): Promise<string> {
      if (this.streaming) return ''
      const s = useSettingsStore()
      this.streaming = true
      this.lastError = ''
      const ctl = new AbortController()
      this.abortCtl = ctl
      try {
        const messages: PromptMessage[] = [
          { role: 'system', content: 'You are a helpful assistant. Write the requested content directly.' },
          { role: 'user', content: prompt },
        ]
        let out = ''
        if (s.genType === 'text') {
          await generateText(
            {
              prompt: messagesToPrompt(messages, '', this.userName),
              model: s.model,
              backend: s.textBackend,
              server: s.textServer || undefined,
              maxTokens: s.maxTokens,
              sampler: this.textSampler(s),
              signal: ctl.signal,
            },
            (d) => {
              out += d
            },
          )
        } else {
          await generate(
            {
              messages,
              model: s.model,
              source: s.source,
              temperature: s.temperature,
              maxTokens: s.maxTokens,
              sampler: this.ccSampler(s),
              signal: ctl.signal,
            },
            (d) => {
              out += d
            },
          )
        }
        return out.trim()
      } catch (e) {
        if (!ctl.signal.aborted) {
          this.lastError = e instanceof Error ? e.message : String(e)
        }
        return ''
      } finally {
        this.streaming = false
        this.abortCtl = null
      }
    },

    /** 输入 `/` 开头 → 按 STscript 执行（变量走宏存储，{{pipe}} 管道） */
    /** /reroll-pick：重置 {{pick}} 稳定种子（写 chat_metadata.pick_reroll_seed，跨端同字段） */
    async rerollPick(): Promise<void> {
      this.pickRerollSeed = Date.now()
      try {
        if (this.group) {
          this.groupMetadata.pick_reroll_seed = this.pickRerollSeed
          await this.saveGroupLines()
          return
        }
        const avatar = this.currentAvatar
        const file = this.currentFile
        if (!avatar || !file) return
        await enqueueChatWrite(async () => {
          const raw = await getChat(avatar, file)
          if (!raw.length) return
          const head = raw[0] as Record<string, unknown>
          const meta = (head.chat_metadata ?? {}) as Record<string, unknown>
          meta.pick_reroll_seed = this.pickRerollSeed
          head.chat_metadata = meta
          await saveChat(avatar, file, raw)
        })
      } catch (e) {
        this.error = e instanceof Error ? e.message : String(e)
      }
    },

    async runUserScript(script: string): Promise<void> {
      // /reroll-pick 不走脚本引擎（App 直接实现种子重置）
      if (script.trim() === '/reroll-pick') {
        await this.rerollPick()
        return
      }
      const ctx: ScriptContext = {
        vars: this.chatVarStore(),
        expand: (text) => runMacros(text, this.macroCtxFor()),
        send: (text) => this.send(text),
        genraw: (prompt) => this.genraw(prompt),
        echo: (text) => {
          this.scriptEcho = text
          window.setTimeout(() => {
            if (this.scriptEcho === text) this.scriptEcho = ''
          }, 4000)
        },
        sys: (text) => this.addNarrator(text),
      }
      try {
        await runScript(script, ctx)
      } catch (e) {
        this.lastError = `脚本错误：${e instanceof Error ? e.message : String(e)}`
      }
    },

    /* ---------------- 群聊 ---------------- */

    /** 成员元表（avatar → 名字/talkativeness） */
    groupMemberMeta(): Record<string, GroupMemberMeta> {
      const chars = useCharacterStore()
      const map: Record<string, GroupMemberMeta> = {}
      for (const a of this.group?.members ?? []) {
        const c = chars.list.find((x) => x.avatar === a)
        map[a] = {
          avatar: a,
          name: c?.name ?? a.replace(/\.png$/i, ''),
          talkativeness: Number((c as { talkativeness?: number } | undefined)?.talkativeness ?? 0.5),
        }
      }
      return map
    },

    /** 打开群聊会话（group_chats/<chatId>.jsonl） */
    async openGroupSession(groupId: string, chatId: string) {
      if (this.streaming) return
      // 切会话前把防抖窗口内的变量变更落盘（与 openSession 同理）
      await this.flushChatVariables()
      this.loadingChat = true
      this.error = ''
      try {
        if (!this.groupsList.length) this.groupsList = await listGroups()
        const g = this.groupsList.find((x) => x.id === groupId) ?? null
        const raw = await getGroupChat(chatId)
        // 剥离 header 行；
        // 过滤而非仅 shift：兼容历史上误存的多重 header；此后 groupLines = 纯消息行，
        // lineIndex 直接对应 groupLines 下标，saveGroupLines 统一前置 header，不再重复
        const lines = raw.filter((l) => !isChatHeader(l as StChatMessage))
        // 群聊 chat_metadata（timedWorldInfo/variables/integrity 等），落盘时随 header 写回
        try {
          this.groupMetadata = await groupChatInfo(chatId)
        } catch {
          this.groupMetadata = {}
        }
        this.pickRerollSeed = Number(this.groupMetadata.pick_reroll_seed) || null
        this.scriptInjects = extractScriptInjects({ chat_metadata: this.groupMetadata })
        const gv = this.groupMetadata.variables
        this.chatVariables =
          gv && typeof gv === 'object' ? { ...(gv as Record<string, unknown>) } : {}
        this.group = g
        this.groupChatId = chatId
        this.groupLines = lines
        // 若自动模式保持开启（ST 为全局开关），按新群 delay 重启计时器
        if (this.groupAutoMode && g) {
          await this.toggleGroupAutoMode(true)
        }
        // 离开单角色会话态
        this.currentAvatar = ''
        this.currentFile = ''
        this.lockedPersonaId = null
        this.genOverride = null
        this.authorsNote = null
        this.summary = ''
        this.timedState = {}
        this.messages = toDisplayMessages(lines, {
          userFallback: this.userName,
          charFallback: g?.name ?? '群聊',
        })
        // 恢复 SWAP 轮换游标：最后一位 AI 发言者
        const meta = this.groupMemberMeta()
        const nameToAvatar = new Map(Object.values(meta).map((m) => [m.name, m.avatar]))
        const lastAi = [...this.messages].reverse().find((m) => m.role === 'assistant')
        this.lastSpeaker = (lastAi && nameToAvatar.get(lastAi.name)) || ''
        this.dirty = false
        this.promptInfo = null
        this.lastUsage = null
      } catch (e) {
        this.error = e instanceof Error ? e.message : String(e)
      } finally {
        this.loadingChat = false
      }
    },

    /** 群成员 prompt：成员系统提示 + 群聊指令 + 历史（他人发言标名，避免抢戏） */
    buildGroupMessages(char: StCharacter, speakerName: string): PromptMessage[] {
      const sys = buildSystemPrompt(char, this.userName)
      const directive =
        `[Group chat] You are ${speakerName} in a group conversation with ${this.userName} ` +
        `and other participants. Write only ${speakerName}'s next reply, staying in character. ` +
        `Never write other participants' lines.`
      const out: PromptMessage[] = [{ role: 'system', content: `${sys}\n\n${directive}` }]
      for (const m of this.messages) {
        if (m.pending || m.error || !m.content.trim()) continue
        if (m.role === 'user') out.push({ role: 'user', content: m.content })
        else out.push({ role: 'assistant', content: `${m.name}: ${m.content}` })
      }
      out.push({ role: 'user', content: `[Continue as ${speakerName}]` })
      return out
    },

    /** 整文件落盘群聊（header + 消息行，header 带回 groupMetadata，保全 ST 侧 chat_metadata） */
    async saveGroupLines() {
      if (!this.group || !this.groupChatId) return
      try {
        const header: GroupChatLine = {
          chat_metadata: this.groupMetadata,
          user_name: 'unused',
          character_name: 'unused',
        }
        const chatId = this.groupChatId
        await enqueueChatWrite(() =>
          saveGroupChat(chatId, [header, ...this.groupLines]),
        )
      } catch (e) {
        this.error = e instanceof Error ? e.message : String(e)
      }
    },

    /** 群聊发送：用户行入列 → 按编排逐成员生成 → 落盘 */    async sendGroup(text: string) {
      const t = text.trim()
      if (!t || this.streaming || !this.group) return
      this.groupLines.push({
        name: this.userName,
        is_user: true,
        is_system: false,
        send_date: stTimestamp(),
        mes: t,
      })
      this.messages.push({
        role: 'user',
        content: t,
        name: this.userName,
        sendDate: Date.now(),
        lineIndex: this.groupLines.length - 1,
      })
      // 先落盘用户行：MANUAL 等策略下本轮可能无人发言
      await this.saveGroupLines()
      await this.runGroupTurn({ isUserInput: true, activationText: t })
    },

    /** 生成一轮群聊发言（按激活策略编排发言成员，可中断） */
    async runGroupTurn(opts: { isUserInput?: boolean; activationText?: string } = {}) {
      const g = this.group
      if (!g || this.streaming) return
      const s = useSettingsStore()
      const chars = useCharacterStore()
      this.streaming = true
      this.lastError = ''
      const ctl = new AbortController()
      this.abortCtl = ctl
      try {
        const metaMap = this.groupMemberMeta()
        // POOLED 用：自上一条用户消息以来已发言的成员 avatar
        const spokenSinceUser: string[] = []
        for (let i = this.messages.length - 1; i >= 0; i--) {
          const m = this.messages[i]!
          if (m.role === 'user') break
          if (m.isSystem || !m.content.trim()) continue
          const hit = Object.values(metaMap).find((x) => x.name === m.name)
          if (hit) spokenSinceUser.push(hit.avatar)
        }
        const lastAi = [...this.messages].reverse().find((m) => m.role === 'assistant')
        const plan = pickSpeakers({
          activationStrategy: Number(g.activation_strategy ?? 0),
          members: g.members,
          disabledMembers: g.disabled_members,
          meta: metaMap,
          isUserInput: opts.isUserInput ?? false,
          activationText: opts.activationText ?? '',
          lastSpeakerName: lastAi?.name,
          lastSpeakerAvatar: this.lastSpeaker || undefined,
          spokenSinceUser,
          allowSelfResponses: g.allow_self_responses === true,
        })
        for (const sp of plan) {
          const member = metaMap[sp.avatar] ?? { avatar: sp.avatar, name: sp.avatar }
          const char = chars.list.find((c) => c.avatar === sp.avatar)
          if (!char) continue
          const reply: DisplayMessage = {
            role: 'assistant',
            content: '',
            name: member.name,
            sendDate: Date.now(),
            pending: true,
          }
          this.messages.push(reply)
          try {
            const messages = this.buildGroupMessages(char, member.name)
            if (s.genType === 'text') {
              const tp = this.instructPromptOf(messages, member.name)
              await generateText(
                {
                  prompt: tp.prompt,
                  stop: tp.stop,
                  model: s.model,
                  backend: s.textBackend,
                  server: s.textServer || undefined,
                  maxTokens: this.genOverride?.maxTokens ?? s.maxTokens,
                  sampler: this.textSampler(s),
                  signal: ctl.signal,
                },
                (d) => {
                  reply.content += d
                },
              )
            } else {
              await generate(
                {
                  messages,
                  model: s.model,
                  source: s.source,
                  // Horde：补全式通道走 Instruct 拼接
                  ...(s.source === 'horde' || s.source === 'novel'
                    ? { ...this.instructPromptOf(messages, member.name), maxContext: s.maxContext }
                    : {}),
                  temperature: this.genOverride?.temperature ?? s.temperature,
                  maxTokens: this.genOverride?.maxTokens ?? s.maxTokens,
                  sampler: this.ccSampler(s),
                  signal: ctl.signal,
                },
                (d) => {
                  reply.content += d
                },
                (r) => {
                  reply.reasoning = (reply.reasoning ?? '') + r
                },
              )
            }
            reply.pending = false
            if (!reply.content.trim()) {
              reply.error = true
              reply.content = '（模型返回空内容）'
            }
            this.lastSpeaker = sp.avatar
            if (!reply.error) {
              this.groupLines.push({
                name: member.name,
                is_user: false,
                is_system: false,
                send_date: stTimestamp(),
                mes: reply.content,
              })
              reply.lineIndex = this.groupLines.length - 1
              await this.saveGroupLines()
            }
          } catch (e) {
            reply.pending = false
            if (ctl.signal.aborted) {
              // 主动停止：有内容保留并落盘，无内容移除占位；中断即结束本轮
              if (reply.content.trim()) {
                this.groupLines.push({
                  name: member.name,
                  is_user: false,
                  is_system: false,
                  send_date: stTimestamp(),
                  mes: reply.content,
                })
                reply.lineIndex = this.groupLines.length - 1
                await this.saveGroupLines()
              } else {
                const i = this.messages.indexOf(reply)
                if (i >= 0) this.messages.splice(i, 1)
              }
            } else {
              // 非 abort 错误：保留已生成部分，仅无产出时才标记错误气泡
              this.lastError = e instanceof Error ? e.message : String(e)
              if (!reply.content.trim() && !(reply.reasoning ?? '').trim()) {
                reply.error = true
                reply.content = this.lastError
              }
            }
            break
          }
        }
      } finally {
        this.streaming = false
        this.abortCtl = null
        void this.refreshConn()
      }
    },

    /** 切换群聊生成模式（写 ST 群文件） */
    async setGroupGenerationMode(mode: number) {
      const g = this.group
      if (!g || this.streaming) return
      const prev = g.generation_mode
      g.generation_mode = mode
      try {
        await editGroup({ id: g.id, generation_mode: mode })
      } catch (e) {
        g.generation_mode = prev
        this.error = e instanceof Error ? e.message : String(e)
      }
    },

    /** 切换群聊激活策略（0 NATURAL / 1 LIST / 2 MANUAL / 3 POOLED，写 ST 群文件） */
    async setGroupActivationStrategy(strategy: number) {
      const g = this.group
      if (!g || this.streaming) return
      const prev = g.activation_strategy
      g.activation_strategy = strategy
      try {
        await editGroup({ id: g.id, activation_strategy: strategy })
      } catch (e) {
        g.activation_strategy = prev
        this.error = e instanceof Error ? e.message : String(e)
      }
    },

    /** 群头像上传（缩略图落 ST 服务器 + avatar_url 写回群文件） */
    async uploadGroupAvatar(file: File): Promise<boolean> {
      const g = this.group
      if (!g) return false
      try {
        const dataUrl = await fileToDataUrl(file)
        const path = await uploadGroupAvatarDataUrl(g.id, dataUrl)
        g.avatar_url = path
        this.groupsList = await listGroups()
        return true
      } catch (e) {
        this.error = e instanceof Error ? e.message : String(e)
        return false
      }
    },

    /** 群聊自动发言开关 */
    async toggleGroupAutoMode(on: boolean) {
      const g = this.group
      if (!g) return
      this.groupAutoMode = on
      if (this.groupAutoModeTimer) {
        clearInterval(this.groupAutoModeTimer)
        this.groupAutoModeTimer = null
      }
      if (!on) return
      const delayMs = Math.max(1, Number(g.auto_mode_delay ?? 5)) * 1000
      this.groupAutoModeTimer = setInterval(() => {
        void this.groupAutoModeTick()
      }, delayMs)
    },

    async groupAutoModeTick() {
      const g = this.group
      if (!this.groupAutoMode || !g || this.streaming || !this.groupLines.length) return
      const last = this.messages[this.messages.length - 1]
      const activationText = last?.content ?? ''
      await this.runGroupTurn({ isUserInput: false, activationText })
    },

    /** 新建群组（默认成员 + 单聊天文件） */
    async createGroup(name: string, members: string[]): Promise<boolean> {
      try {
        await apiCreateGroup({
          name,
          members,
          avatar_url: 'default',
          allow_self_responses: false,
          hideMutedSprites: false,
          activation_strategy: 0,
          generation_mode: 0,
          disabled_members: [],
          fav: false,
          chat_id: '',
          chats: [],
        })
        this.groupsList = await listGroups()
        return true
      } catch (e) {
        this.error = e instanceof Error ? e.message : String(e)
        return false
      }
    },

    /** 删除群组（可选连带聊天记录） */
    async removeGroup(id: string, deleteChats = true) {
      // 生成中删群会让 runGroupTurn 继续对内存推送"幽灵回复"且不再落盘
      if (this.streaming && this.group?.id === id) {
        this.lastError = '群聊正在生成回复，请先停止生成再删除'
        return
      }
      // 删除当前群时停掉自动发言定时器（此前定时器残留，下个群会被自动接上）
      if (this.group?.id === id && this.groupAutoMode) {
        await this.toggleGroupAutoMode(false)
      }
      try {
        await deleteGroup(id, deleteChats)
        if (this.group?.id === id) {
          this.group = null
          this.groupChatId = ''
          this.groupLines = []
          this.messages = []
        }
        this.groupsList = await listGroups()
        await this.loadSessions()
      } catch (e) {
        this.error = e instanceof Error ? e.message : String(e)
      }
    },

    /** 重命名群组 */
    async renameGroup(id: string, name: string) {
      try {
        await editGroup({ id, name })
        const g = this.groupsList.find((x) => x.id === id)
        if (g) g.name = name
        if (this.group?.id === id) this.group = g ?? this.group
        await this.loadSessions()
      } catch (e) {
        this.error = e instanceof Error ? e.message : String(e)
      }
    },

    /** 群聊重新生成：删除本轮 AI 发言后按激活策略重跑 */
    async regenerateGroup() {
      if (this.streaming || !this.group) return
      // 移除末条 AI 消息与其行，然后按当前 lastSpeaker 重跑
      const last = this.messages[this.messages.length - 1]
      if (last?.role === 'assistant') {
        const lineIdx = this.groupLineIndexOf(last)
        if (lineIdx >= 0) {
          this.groupLines.splice(lineIdx, 1)
          for (const x of this.messages) {
            if (x.lineIndex !== undefined && x.lineIndex > lineIdx) x.lineIndex -= 1
          }
        }
        this.messages.pop()
      }
      // ST：activationText = 最后一条消息正文（此处应为用户消息或上一条 AI 消息）
      const activationText = this.messages[this.messages.length - 1]?.content ?? ''
      await this.saveGroupLines()
      await this.runGroupTurn({ isUserInput: false, activationText })
    },

    async send(text: string) {
      const t = text.trim()
      if (!t || this.streaming) return
      // `/` 开头 → STscript（单聊/群聊通用）
      if (t.startsWith('/')) {
        await this.runUserScript(t)
        return
      }
      // 群聊会话走群聊通路（成员编排 + 群聊文件落盘）
      if (this.group) {
        await this.sendGroup(t)
        return
      }
      if (!this.currentFile) {
        this.lastError = '请先选择或新建一个会话'
        return
      }
      if (!this.currentCharacter) {
        this.lastError = '未选择角色卡'
        return
      }
      this.messages.push({
        role: 'user',
        content: t,
        name: this.userName,
        sendDate: Date.now(),
      })
      // 先落盘用户行：
      // 生成期间崩溃/断电不丢整轮输入。失败回落全量保存（saveCurrent 现已保全 header）。
      await this.persistUserLine()
      await this.generateReply()
      // M-19 auto-continue：回复过短时自动续写（generateReply 的 streaming 状态已复位）
      if (this.autoContinueEnabled && !this.lastGenAborted) {
        await this.autoContinueReply(this.autoContinueTarget)
      }
      await this.saveCurrent()
    },

    /**
     * 把末条用户消息行级追加落盘（send 用：生成开始前输入先保存，崩溃不丢）。
     * insert 允许追加到文件末尾（Rust 侧 index.min(len)）；行级不可用时回落全量保存。
     */
    async persistUserLine(): Promise<void> {
      const m = this.messages[this.messages.length - 1]
      if (!m || m.role !== 'user' || !this.currentAvatar || !this.currentFile) return
      // 已知最大行号 +1 = 追加位置（error 气泡等占位消息不入文件，无行号）
      const at = Math.max(-1, ...this.messages.map((x) => x.lineIndex ?? -1)) + 1
      try {
        const avatar = this.currentAvatar
        const file = this.currentFile
        await enqueueChatWrite(() =>
          chatMutateLines(avatar, file, [
            { op: 'insert', index: at, json: this.currentMessageJson(m) },
          ]),
        )
        m.lineIndex = at
        this.dirty = false
      } catch {
        await this.saveCurrent()
      }
    },

    /** 重新生成：末条为成功 AI 回复时追加为 swipe 备选（保留旧回复），否则截断重跑 */
    async regenerate() {
      if (this.streaming) return
      // 群聊会话走群聊重生成通路（删除本轮 AI 发言后按激活策略重跑）
      if (this.group) {
        await this.regenerateGroup()
        return
      }
      const last = this.messages[this.messages.length - 1]
      if (last && last.role === 'assistant' && !last.error && last.content.trim()) {
        await this.regenerateAsSwipe(last)
        return
      }
      while (this.messages.length && this.messages[this.messages.length - 1].role === 'assistant') {
        this.messages.pop()
      }
      if (!this.messages.length) return
      await this.generateReply()
      await this.saveCurrent()
    },

    /** swipe 追加式重生成：旧回复保留为备选，新回复成为当前显示 */
    async regenerateAsSwipe(last: DisplayMessage) {
      // 群聊无单聊 swipe 语义（生成路径依赖 currentCharacter，单聊专用）——
      // UI 已隐藏入口，这里兜底防脚本/误触把用户消息误当生成结果
      if (this.group) {
        this.lastError = '群聊会话请使用「重新生成本轮」'
        return
      }
      last.swipes ??= [last.content]
      if (!last.swipes.includes(last.content)) last.swipes.push(last.content)
      this.messages.pop() // 从历史视图移除，重新生成不含它
      await this.generateReply()
      const fresh = this.messages[this.messages.length - 1]
      // fresh === last：生成什么都没产出（如停止生成时占位气泡已被移除），走恢复路径
      if (fresh === last || !fresh || fresh.error || fresh.pending || !fresh.content.trim()) {
        // 生成失败：恢复旧回复显示
        if (fresh && fresh !== last) this.messages.splice(this.messages.length - 1, 1, last)
        else if (!this.messages.includes(last)) this.messages.push(last)
        if (fresh?.error) this.lastError = fresh.content
        return
      }
      last.swipes.push(fresh.content)
      last.swipeId = last.swipes.length - 1
      last.content = fresh.content
      last.sendDate = fresh.sendDate
      if (fresh.reasoning) last.reasoning = fresh.reasoning
      this.messages.splice(this.messages.length - 1, 1, last)

      const opLine = last.lineIndex
      if (opLine !== undefined && this.currentAvatar && this.currentFile) {
        try {
          await chatMutateLines(this.currentAvatar, this.currentFile, [
            { op: 'replace', index: opLine, json: this.currentMessageJson(last) },
          ])
          this.dirty = false
          return
        } catch (e) {
          this.error = e instanceof Error ? e.message : String(e)
        }
      }
      await this.saveCurrent()
    },

    /** 切换末条 AI 消息的 swipe 备选（左右方向）。
     *  末条回复即使从未生成过备选（无 swipes 字段）也按 [当前内容] 处理 ——
     * swipe 条恒显 1/1，末尾右滑 = overswipe 生成新备选。 */
    async switchSwipe(index: number, dir: -1 | 1) {
      if (this.streaming) return
      if (this.group) return // 群聊无 swipe 语义（UI 已隐藏，store 层兜底）
      const m = this.messages[index]
      if (!m || m.role !== 'assistant' || m.pending || m.error) return
      const swipes = m.swipes?.length ? m.swipes : [m.content]
      const next = (m.swipeId ?? 0) + dir
      if (next < 0) return
      if (next >= swipes.length) {
        // overswipe：已是最后一条备选仍右滑 → 追加生成新备选（ST 的 OVERSWIPE 行为）
        if (dir === 1 && m.role === 'assistant' && !m.pending && !m.error) {
          await this.regenerateAsSwipe(m)
        }
        return
      }
      await this.switchSwipeTo(index, next)
    },

    /** 切换到指定序号的备选（swipe picker 用） */
    async switchSwipeTo(index: number, target: number) {
      if (this.streaming) return
      if (this.group) return // 群聊无 swipe 语义（UI 已隐藏，store 层兜底）
      const m = this.messages[index]
      if (!m?.swipes?.length) return
      const next = Math.max(0, Math.min(target, m.swipes.length - 1))
      if (next === (m.swipeId ?? 0)) return
      m.swipeId = next
      m.content = m.swipes[next] ?? ''
      const opLine = m.lineIndex
      if (opLine !== undefined && this.currentAvatar && this.currentFile) {
        try {
          await chatMutateLines(this.currentAvatar, this.currentFile, [
            { op: 'replace', index: opLine, json: this.currentMessageJson(m) },
          ])
          return
        } catch (e) {
          this.error = e instanceof Error ? e.message : String(e)
        }
      }
      await this.saveCurrent()
    },

    /** 从某条用户消息重发：删除其后所有消息，再重新生成 */
    async resendFrom(index: number) {
      if (this.streaming) return
      const m = this.messages[index]
      if (!m || m.role !== 'user') return
      const tail = this.messages.slice(index + 1)
      if (tail.length) {
        const ops: ChatLineOp[] = []
        let allKnown = true
        for (let k = tail.length - 1; k >= 0; k--) {
          const li = tail[k].lineIndex
          if (li === undefined) {
            allKnown = false
            break
          }
          ops.push({ op: 'delete', index: li })
        }
        this.messages.splice(index + 1)
        // 译文按消息下标存储：被移除的尾部消息译文一并丢弃（同 removeMessage）
        for (const k of Object.keys(this.translations)) {
          if (Number(k) > index) delete this.translations[Number(k)]
        }
        if (allKnown && this.currentAvatar && this.currentFile) {
          try {
            await chatMutateLines(this.currentAvatar, this.currentFile, ops)
          } catch (e) {
            this.error = e instanceof Error ? e.message : String(e)
            await this.saveCurrent()
          }
        } else {
          await this.saveCurrent()
        }
      }
      await this.generateReply()
      await this.saveCurrent()
    },

    /** 从某条消息分支：复制前缀为新会话文件并打开（原会话不动） */
    async branchFrom(index: number) {
      if (this.streaming) return
      const avatar = this.currentAvatar
      const file = this.currentFile
      const charName = this.currentCharacter?.name ?? ''
      if (!avatar || !file) return
      const prefix = this.messages
        .slice(0, index + 1)
        .filter((x) => x.content.trim() && !x.error && !x.pending)
      if (!prefix.length) return
      const newFile = `${file} - branch ${stTimestamp()}`
      // 保留原会话 chat_metadata（副本此前丢失人设锁/AN/摘要/变量）
      const chatMetadata = await this.readChatMetadata(avatar, file)
      const header: StChatMessage = {
        chat_metadata: chatMetadata,
        user_name: this.userName,
        character_name: charName,
      }
      try {
        await saveChat(avatar, newFile, [header, ...toStMessages(prefix, charName)])
        await this.loadSessions()
        await this.openSession(avatar, newFile)
      } catch (e) {
        this.error = e instanceof Error ? e.message : String(e)
      }
    },

    /** 给消息添加书签（checkpoint）：复制当前会话为独立文件并回写 bookmark_link */
    async bookmarkMessage(index: number) {
      if (this.streaming) return
      const m = this.messages[index]
      const avatar = this.currentAvatar
      const file = this.currentFile
      const charName = this.currentCharacter?.name ?? ''
      if (!m || !avatar || !file) return
      const all = this.messages.filter((x) => x.content.trim() && !x.error && !x.pending)
      if (!all.length) return
      const newFile = `${file} - ${stTimestamp()}`
      // 保留原会话 chat_metadata（书签副本此前丢失人设锁/AN/摘要/变量）
      const chatMetadata = await this.readChatMetadata(avatar, file)
      const header: StChatMessage = {
        chat_metadata: chatMetadata,
        user_name: this.userName,
        character_name: charName,
      }
      try {
        await saveChat(avatar, newFile, [header, ...toStMessages(all, charName)])
        m.extra = { ...m.extra, bookmark_link: newFile }
        m.bookmarkLink = newFile
        await this.loadSessions()
        const opLine = m.lineIndex
        if (opLine !== undefined) {
          try {
            await chatMutateLines(avatar, file, [
              { op: 'replace', index: opLine, json: this.currentMessageJson(m) },
            ])
          } catch {
            await this.saveCurrent() // 增量失败（如非桌面环境）回落全量保存
          }
        } else {
          await this.saveCurrent()
        }
      } catch (e) {
        this.error = e instanceof Error ? e.message : String(e)
      }
    },

    /** 删除某条消息（行级增量落盘，未知行号回落全量保存，群聊走 groupLines + saveGroupLines） */
    async removeMessage(index: number) {
      if (this.streaming) return
      const m = this.messages[index]
      if (!m) return
      if (this.group) {
        const li = this.groupLineIndexOf(m)
        if (li >= 0) {
          this.groupLines.splice(li, 1)
          for (const x of this.messages) {
            if (x.lineIndex !== undefined && x.lineIndex > li) x.lineIndex -= 1
          }
          m.lineIndex = undefined
        }
        this.messages.splice(index, 1)
        this.reindexTranslationsAfterRemoval(index)
        await this.saveGroupLines()
        return
      }
      const opLine = m.lineIndex
      this.messages.splice(index, 1)
      this.reindexTranslationsAfterRemoval(index)
      if (opLine !== undefined) this.shiftLineIndexes(opLine, -1)
      if (opLine !== undefined && this.currentAvatar && this.currentFile) {
        try {
          await chatMutateLines(this.currentAvatar, this.currentFile, [
            { op: 'delete', index: opLine },
          ])
          return
        } catch (e) {
          this.error = e instanceof Error ? e.message : String(e)
        }
      }
      await this.saveCurrent()
    },

    /** 批量删除消息（多选模式）：indices 为消息下标（任意顺序）；单聊行级、群聊定位删除 */
    async removeMessages(indices: number[]): Promise<void> {
      if (this.streaming || !indices.length) return
      const targets = [...new Set(indices)].sort((a, b) => b - a) // 降序：删除不影响未处理下标
      if (this.group) {
        const deletedLines: number[] = []
        for (const i of targets) {
          const m = this.messages[i]
          if (!m) continue
          const li = this.groupLineIndexOf(m)
          if (li >= 0) deletedLines.push(li)
        }
        for (const i of targets) {
          this.messages.splice(i, 1)
          this.reindexTranslationsAfterRemoval(i)
        }
        // 群聊行号 = groupLines 下标：降序删除后按删除位置校正剩余行号
        deletedLines.sort((a, b) => b - a)
        for (const li of deletedLines) this.groupLines.splice(li, 1)
        const asc = [...deletedLines].sort((a, b) => a - b)
        for (const m of this.messages) {
          if (m.lineIndex === undefined) continue
          m.lineIndex -= asc.filter((d) => d < m.lineIndex!).length
        }
        await this.saveGroupLines()
        return
      }
      // 单聊：收集文件行号（含未知行号 → 全量保存兜底）
      const delIdx: number[] = []
      let allKnown = true
      for (const i of targets) {
        const m = this.messages[i]
        if (!m) continue
        if (m.lineIndex === undefined) { allKnown = false; continue }
        delIdx.push(m.lineIndex)
      }
      for (const i of targets) {
        this.messages.splice(i, 1)
        this.reindexTranslationsAfterRemoval(i)
      }
      if (delIdx.length && allKnown && this.currentAvatar && this.currentFile) {
        delIdx.sort((a, b) => b - a)
        const ops: ChatLineOp[] = delIdx.map((index) => ({ op: 'delete' as const, index }))
        try {
          const avatar = this.currentAvatar
          const file = this.currentFile
          await enqueueChatWrite(() => chatMutateLines(avatar, file, ops))
          const asc = [...delIdx].sort((a, b) => a - b)
          for (const m of this.messages) {
            if (m.lineIndex === undefined) continue
            m.lineIndex -= asc.filter((d) => d < m.lineIndex!).length
          }
          this.dirty = false
          return
        } catch (e) {
          this.error = e instanceof Error ? e.message : String(e)
        }
      }
      await this.saveCurrent()
    },

    /** 切换消息隐藏（is_system）：隐藏的消息不进 prompt，UI 半透明显示 */
    async toggleHideMessage(index: number) {
      if (this.streaming) return
      const m = this.messages[index]
      if (!m || m.pending) return
      m.isSystem = !m.isSystem
      if (this.group) {
        const li = this.groupLineIndexOf(m)
        if (li >= 0) {
          this.groupLines[li].is_system = m.isSystem
          await this.saveGroupLines()
        }
        return
      }
      const opLine = m.lineIndex
      if (opLine !== undefined && this.currentAvatar && this.currentFile) {
        try {
          await chatMutateLines(this.currentAvatar, this.currentFile, [
            { op: 'replace', index: opLine, json: this.currentMessageJson(m) },
          ])
          return
        } catch (e) {
          this.error = e instanceof Error ? e.message : String(e)
        }
      }
      await this.saveCurrent()
    },

    /** 插入旁白（/sys）：is_system 消息，居中灰显、不进 prompt */
    async addNarrator(text: string) {
      const t = text.trim()
      if (!t || this.streaming) return
      const msg: DisplayMessage = {
        role: 'assistant',
        content: t,
        name: '旁白',
        sendDate: Date.now(),
        isSystem: true,
      }
      if (this.group) {
        // 群聊：旁白 = is_system 行，追加到 groupLines 尾部
        const line: GroupChatLine = {
          name: '旁白',
          is_user: false,
          is_system: true,
          send_date: stTimestamp(),
          mes: t,
        }
        msg.lineIndex = this.groupLines.length
        this.groupLines.push(line)
        this.messages.push(msg)
        await this.saveGroupLines()
        return
      }
      // 行级落盘：追加到会话文件尾部。行号按「已知最大行号 +1」推算（error 气泡等
      // 占位消息不入文件也无行号，此前 messages.length+1 会算出越界/错位行号）
      // 用 insert（Rust 侧允许追加到末尾）
      const at = this.currentFile && this.currentAvatar
        ? Math.max(-1, ...this.messages.map((x) => x.lineIndex ?? -1)) + 1
        : -1
      if (at >= 0) msg.lineIndex = at
      this.messages.push(msg)
      if (at >= 0) {
        try {
          await chatMutateLines(this.currentAvatar, this.currentFile, [
            { op: 'insert', index: at, json: this.currentMessageJson(msg) },
          ])
          return
        } catch (e) {
          this.error = e instanceof Error ? e.message : String(e)
        }
      }
      await this.saveCurrent()
    },    async editMessage(index: number, content: string) {
      if (this.streaming) return
      const m = this.messages[index]
      if (!m || m.pending || m.content === content) return
      if (this.group) {
        const li = this.groupLineIndexOf(m)
        if (li >= 0) {
          const line = this.groupLines[li]
          line.mes = content
          if (m.swipes && m.swipeId !== undefined && m.swipes[m.swipeId] !== undefined) {
            m.swipes[m.swipeId] = content
            const swipes = Array.isArray(line.swipes) ? (line.swipes as string[]) : undefined
            if (swipes && m.swipeId < swipes.length) swipes[m.swipeId] = content
          }
          await this.saveGroupLines()
        }
        m.content = content
        return
      }
      m.content = content
      if (m.swipes && m.swipeId !== undefined && m.swipes[m.swipeId] !== undefined) {
        m.swipes[m.swipeId] = content
      }
      const opLine = m.lineIndex
      if (opLine !== undefined && this.currentAvatar && this.currentFile) {
        try {
          await chatMutateLines(this.currentAvatar, this.currentFile, [
            { op: 'replace', index: opLine, json: this.currentMessageJson(m) },
          ])
          return
        } catch (e) {
          this.error = e instanceof Error ? e.message : String(e)
        }
      }
      await this.saveCurrent()
    },

    /** 显示消息 → groupLines 下标：优先 lineIndex（需行内容吻合），否则按正文回溯定位 */
    groupLineIndexOf(m: DisplayMessage): number {
      if (
        m.lineIndex !== undefined &&
        this.groupLines[m.lineIndex] &&
        this.groupLines[m.lineIndex].mes === m.content
      ) {
        return m.lineIndex
      }
      return this.groupLines.map((l) => l.mes).lastIndexOf(m.content)
    },

    /** 删行后维护本地行号：> opLine 的全部偏移 delta */
    shiftLineIndexes(opLine: number, delta: number) {
      for (const m of this.messages) {
        if (m.lineIndex !== undefined && m.lineIndex > opLine) m.lineIndex += delta
      }
    },

    /** 单条消息 → ST jsonl 行 JSON（保留 extra/swipes） */
    currentMessageJson(m: DisplayMessage): string {
      const charName = this.currentCharacter?.name ?? m.name
      return JSON.stringify(toStMessages([m], charName)[0])
    },

    /* ---------------- 持久化 ---------------- */

    /**
     * 宏变量存储。
     * 变更经防抖写回会话文件。
     */
    chatVarStore(): MacroVariables {
      const self = this
      return {
        get: (name) => self.chatVariables[name],
        set: (name, value) => {
          self.chatVariables[name] = value
          self.scheduleChatVarFlush()
        },
        has: (name) => name in self.chatVariables,
        delete: (name) => {
          delete self.chatVariables[name]
          self.scheduleChatVarFlush()
        },
      }
    },

    /** 防抖触发变量落盘（setvar 可能高频触发，整文件保存按批合并） */
    scheduleChatVarFlush(): void {
      if (this.chatVarFlushTimer) return
      this.chatVarFlushTimer = setTimeout(() => {
        this.chatVarFlushTimer = null
        void this.persistChatVariables()
      }, 400)
    },

    /** 有未落盘的变量变更时立即写回（切会话/关窗前调用，防防抖窗口丢改动） */
    async flushChatVariables(): Promise<void> {
      if (this.chatVarFlushTimer) {
        clearTimeout(this.chatVarFlushTimer)
        this.chatVarFlushTimer = null
        await this.persistChatVariables()
      }
    },

    /** 读会话文件 header 的 chat_metadata（书签/分支副本用，读不到返回空对象） */
    async readChatMetadata(
      avatar: string,
      file: string,
    ): Promise<Record<string, unknown>> {
      try {
        const raw = await getChat(avatar, file)
        const head = raw[0] as Record<string, unknown> | undefined
        const meta = head?.chat_metadata
        return meta && typeof meta === 'object' ? { ...(meta as Record<string, unknown>) } : {}
      } catch {
        return {}
      }
    },

    /** 会话列表延迟刷新（send 每轮全量拉 recentChats 太重，500ms 合并） */
    scheduleSessionListRefresh(): void {
      if (this.sessionsRefreshTimer) return
      this.sessionsRefreshTimer = setTimeout(() => {
        this.sessionsRefreshTimer = null
        void this.loadSessions()
      }, 500)
    },

    /** 删除消息后重排 translations 键（按消息下标存储，删中间消息后残留译文会贴错气泡） */
    reindexTranslationsAfterRemoval(removedIndex: number): void {
      if (!Object.keys(this.translations).length) return
      const next: Record<number, string> = {}
      for (const [k, v] of Object.entries(this.translations)) {
        const i = Number(k)
        if (i === removedIndex) continue
        next[i > removedIndex ? i - 1 : i] = v
      }
      this.translations = next
    },

    /** 把 chat_metadata.variables 写回当前会话文件（单聊走 getChat+saveChat，群聊随 header） */
    async persistChatVariables(): Promise<void> {
      try {
        if (this.group) {
          this.groupMetadata.variables = { ...this.chatVariables }
          await this.saveGroupLines()
          return
        }
        if (!this.currentAvatar || !this.currentFile) return
        const avatar = this.currentAvatar
        const file = this.currentFile
        await enqueueChatWrite(async () => {
          const raw = await getChat(avatar, file)
          if (!raw.length) return
          const head = raw[0] as Record<string, unknown>
          const meta = (head.chat_metadata ?? {}) as Record<string, unknown>
          if (Object.keys(this.chatVariables).length) meta.variables = { ...this.chatVariables }
          else delete meta.variables
          head.chat_metadata = meta
          await saveChat(avatar, file, raw)
        })
      } catch {
        /* 变量落盘失败不阻断（下次变更会重试） */
      }
    },

    /** 把当前会话写回 ST */
    async saveCurrent() {
      if (!this.currentAvatar || !this.currentFile) return
      if (!this.messages.some((m) => m.content.trim())) return
      this.saving = true
      try {
        const avatar = this.currentAvatar
        const file = this.currentFile
        // 整文件写入入队串行：防止与变量/摘要/参数覆盖等读-改-写任务交错（后写者胜回滚）
        await enqueueChatWrite(async () => {
          // chat_metadata 承载人设锁/作者注释/摘要/宏变量/timedWorldInfo 等，
          // 全量保存必须**保全旧 header**（此前硬编码 {} 会导致每条消息落盘即清空全部
          // 会话元数据，重开会话即暴露）。user_name/character_name 用当前值
          // 覆盖以跟进改名；读不到旧档时才退回空 metadata 兜底。
          let header: StChatMessage = {
            chat_metadata: {},
            user_name: this.userName,
            character_name: this.currentCharacter?.name ?? '',
          }
          try {
            const raw = await getChat(avatar, file)
            if (raw.length && raw[0] && typeof raw[0] === 'object') {
              header = {
                ...raw[0],
                user_name: this.userName,
                character_name: this.currentCharacter?.name ?? '',
              }
            }
          } catch {
            /* 旧档读取失败（如首存前被删）按空 metadata 兜底，行为同旧版 */
          }
          const kept = this.messages.filter(
            (m) => m.content.trim() && !m.error && !m.pending,
          )
          const body = toStMessages(kept, this.currentCharacter?.name ?? '')
          await saveChat(avatar, file, [header, ...body])
          // 写回成功：文件结构 = 元数据头 + kept 顺序 → 重排本地行号，行级增量才能
          const keptSet = new Set(kept)
          let li = 1
          for (const m of this.messages) {
            m.lineIndex = keptSet.has(m) ? li++ : undefined
          }
          this.dirty = false
        })
        // 延迟合并刷新（每条消息全量拉 recentChats 太重）
        this.scheduleSessionListRefresh()
      } catch (e) {
        this.error = e instanceof Error ? e.message : String(e)
      } finally {
        this.saving = false
      }
    },

    /**
     * 重命名会话（改 ST 侧文件名 + 本地显示别名）。
     * 置顶键 / 别名键随 fileId 迁移；若重命名的是当前会话，currentFile 跟随。
     */
    async renameSession(avatar: string, fileId: string, newName: string): Promise<boolean> {
      // 文件名净化：去掉文件系统非法字符
      const name = newName.replace(/[\\/:*?"<>|]/g, '').trim()
      if (!name || name === fileId) return false
      const oldKey = sessionKey(avatar, fileId)
      const newKey = sessionKey(avatar, name)
      try {
        await renameChat(avatar, `${fileId}.jsonl`, `${name}.jsonl`)
        // 用量累计统计随键迁移（壳侧 localStorage）
        moveSessionUsage(oldKey, newKey)
        // 别名 = 用户选的名字（改名后列表立即可见）；旧键迁移
        delete this.aliases[oldKey]
        this.aliases[newKey] = name
        localStorage.setItem(ALIAS_KEY, JSON.stringify(this.aliases))
        // 置顶键迁移
        const pi = this.pinnedKeys.indexOf(oldKey)
        if (pi >= 0) {
          this.pinnedKeys[pi] = newKey
          localStorage.setItem(PIN_KEY, JSON.stringify(this.pinnedKeys))
        }
        if (this.currentAvatar === avatar && this.currentFile === fileId) {
          this.currentFile = name
        }
        await this.loadSessions()
        return true
      } catch (e) {
        this.error = e instanceof Error ? e.message : String(e)
        return false
      }
    },

    /** 删除一个会话文件（成功返回 true，正在流式生成时拒绝删除） */
    async deleteSession(avatar: string, fileId: string): Promise<boolean> {
      if (this.streaming) return false
      try {
        await deleteChat(avatar, `${fileId}.jsonl`)
        delete this.sessMeta[sessionKey(avatar, fileId)]
        localStorage.setItem(SESS_META_KEY, JSON.stringify(this.sessMeta))
        if (this.currentAvatar === avatar && this.currentFile === fileId) {
          // 删除的正是当前打开的会话 → 清空主面板，避免残留已删内容
          this.currentAvatar = ''
          this.currentFile = ''
          this.lockedPersonaId = null
          this.genOverride = null
          this.messages = []
          this.dirty = false
          this.promptInfo = null
        }
        await this.loadSessions()
        return true
      } catch (e) {
        this.error = e instanceof Error ? e.message : String(e)
        return false
      }
    },
  },
})

/* 开发期热更新接管：改本 store 后无需手动刷新页面（Pinia 官方推荐写法） */
if (import.meta.hot) {
  import.meta.hot.accept(acceptHMRUpdate(useChatStore, import.meta.hot))
}
