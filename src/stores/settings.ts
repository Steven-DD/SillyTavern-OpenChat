/**
 * 生成参数设置 + 配置映射
 *
 * 单一事实来源：生成层 api.ts 不再自己读 localStorage，
 * 由本 store 显式传参（source/model/temperature/maxTokens），避免两处配置打架。
 *
 * 持久化：localStorage（本 App 的配置就是本 App 的）
 *
 * **映射**：参数变更后，把「服务端也需要知道的项」自动同步进 ST 的 settings.json，
 * 用户只改本 App 的配置即可（详见 `services/st/mapping.ts`）。
 * 同步是 debounce 的，且**写之前先比对，一致就不写** ——
 * 因为 ST 每次 save 都会触发它自己的 autosave 备份，无谓写入会堆一堆备份文件。
 */
import { defineStore, acceptHMRUpdate } from 'pinia'
import { applyMapping, inspectMapping, type MappingDiff } from '@/services/st/mapping'

export type GenSource = string

/** 映射状态：unknown（未检测）| synced | pending（有差异待同步）| error */
export type MappingState = 'unknown' | 'synced' | 'pending' | 'error'

export interface GenSettings {
  /** 生成类型：chat = Chat Completion（messages 结构）；text = Text Completion（单字符串 prompt） */
  genType: 'chat' | 'text'
  /** 生成通道：ST 的 chat_completion_source（makersuite = ST 原生 Gemini 等） */
  source: GenSource
  /** Text Completion 后端（ST textgen_types 值，见 services/st/textgen.ts） */
  textBackend: string
  /** Text Completion 服务器地址（本地类后端必填） */
  textServer: string
  /** 主模型 */
  model: string
  /** 备用模型：主模型高峰过载时自动切换（空字符串 = 不启用回退） */
  fallbackModel: string
  temperature: number
  /** 单次回复最大 token */
  maxTokens: number
  /** 上下文窗口（token），用于 prompt 裁剪 */
  maxContext: number
  /** 是否把角色卡的对话示例注入 prompt */
  includeExamples: boolean
  /** Top P（核采样） */
  topP: number
  /** Top K（0 = 关闭） */
  topK: number
  /** Top A（激活阈值，0 = 关闭） */
  topA: number
  /** Min P（0 = 关闭） */
  minP: number
  /** 频率惩罚 */
  frequencyPenalty: number
  /** 存在惩罚 */
  presencePenalty: number
  /** 重复惩罚 */
  repetitionPenalty: number
}

const KEY = 'app.gen'
/** 从 ST 拉回的模型列表（独立 key：不属于用户手填参数，reset 不应清掉） */
const MODELS_KEY = 'app.gen.models'

function loadModelOptions(): string[] {
  try {
    const raw = localStorage.getItem(MODELS_KEY)
    const arr = raw ? (JSON.parse(raw) as unknown) : []
    return Array.isArray(arr) ? arr.filter((s): s is string => typeof s === 'string') : []
  } catch {
    return []
  }
}

export const DEFAULTS: GenSettings = {
  genType: 'chat',
  /** 默认生成通道（服务端默认 openai） */
  source: 'openai',
  textBackend: 'koboldcpp',
  textServer: '',
  /** 模型不给默认值：必须由用户连接后从实拉列表里选 */
  model: '',
  fallbackModel: '',
  /** 以下为默认采样参数 */
  temperature: 1,
  maxTokens: 300,
  maxContext: 4095,
  includeExamples: true,
  topP: 1,
  topK: 0,
  topA: 0,
  minP: 0,
  frequencyPenalty: 0,
  presencePenalty: 0,
  repetitionPenalty: 1,
}

/** 旧版本的瞎编默认值（曾被打进用户 localStorage），加载时清除，避免下拉里凭空出现 */
const LEGACY_MODEL_DEFAULTS = ['gemini-3.8-flash', 'gemini-flash-latest']

/** 常用模型候选（实测 /v1beta/models 存在，可自由填其它 id） */
function load(): GenSettings {
  try {
    const raw = localStorage.getItem(KEY)
    if (!raw) return { ...DEFAULTS }
    const merged = { ...DEFAULTS, ...(JSON.parse(raw) as Partial<GenSettings>) }
    // 清掉旧版持久化的瞎编模型默认值（模型必须来自实拉列表）
    if (LEGACY_MODEL_DEFAULTS.includes(merged.model)) merged.model = ''
    if (LEGACY_MODEL_DEFAULTS.includes(merged.fallbackModel)) merged.fallbackModel = ''
    return merged
  } catch {
    return { ...DEFAULTS }
  }
}

export const useSettingsStore = defineStore('settings', {
  state: () => ({
    ...load(),
    /** 从 ST 实时拉取的模型列表（主/备模型的下拉候选，与手填预设合并去重） */
    modelOptions: loadModelOptions(),
    /** ST 侧设置的用户名（用于 {{user}} 宏，也是映射的输入之一） */
    stUserName: 'User',
    /** 是否已从 ST 读过设置 */
    stLoaded: false,
    stError: '',

    /* ---- 映射 ---- */
    mapState: 'unknown' as MappingState,
    mapEntries: [] as MappingDiff[],
    mapError: '',
    /** 写入进行中（防重入） */
    syncing: false,
    /** 同步进行中又有新变更 → 完成后补跑一次（保证最后一次变更不丢） */
    syncAgain: false,
  }),

  getters: {
    /** 处于未同步状态的项数 */
    mapPending(state): number {
      return state.mapEntries.filter((e) => !e.inSync).length
    },
    mapSummary(state): string {
      if (state.mapState === 'unknown') return '尚未检测'
      if (state.mapState === 'error') return '同步失败'
      if (state.mapState === 'synced')
        return `已与 SillyTavern 一致（${state.mapEntries.length} 项）`
      return `${state.mapEntries.filter((e) => !e.inSync).length} 项待同步`
    },
  },

  actions: {
    persist() {
      const s: GenSettings = {
        genType: this.genType,
        source: this.source,
        textBackend: this.textBackend,
        textServer: this.textServer,
        model: this.model,
        fallbackModel: this.fallbackModel,
        temperature: this.temperature,
        maxTokens: this.maxTokens,
        maxContext: this.maxContext,
        includeExamples: this.includeExamples,
        topP: this.topP,
        topK: this.topK,
        topA: this.topA,
        minP: this.minP,
        frequencyPenalty: this.frequencyPenalty,
        presencePenalty: this.presencePenalty,
        repetitionPenalty: this.repetitionPenalty,
      }
      localStorage.setItem(KEY, JSON.stringify(s))
      // 参数改了 → 延迟同步到 ST（用户零感知）
      // 参数变更 → 立即同步（用户要求：不走定时器）
      void this.syncNow()
    },

    update(patch: Partial<GenSettings>) {
      Object.assign(this, patch)
      this.persist()
    },

    /** 保存「从 ST 拉取的模型列表」（持久化，reset 不清） */
    setModelOptions(list: string[]) {
      this.modelOptions = list
      localStorage.setItem(MODELS_KEY, JSON.stringify(list))
    },

    reset() {
      Object.assign(this, DEFAULTS)
      this.persist()
    },

    /* ---------------- 映射 ---------------- */

    /** 映射入参（App 侧的值都从这里取，保持 mapping.ts 与 store 解耦） */
    mappingInput() {
      return {
        source: this.source,
        model: this.model,
        temperature: this.temperature,
        maxTokens: this.maxTokens,
        maxContext: this.maxContext,
        topP: this.topP,
        topK: this.topK,
        topA: this.topA,
        minP: this.minP,
        frequencyPenalty: this.frequencyPenalty,
        presencePenalty: this.presencePenalty,
        repetitionPenalty: this.repetitionPenalty,
      }
    },

    /** 只读对比：看哪些键尚未同步（不写入） */
    async inspectMappingNow() {
      try {
        this.mapEntries = await inspectMapping(this.mappingInput())
        this.mapState = this.mapEntries.every((e) => e.inSync) ? 'synced' : 'pending'
        this.mapError = ''
      } catch (e) {
        this.mapState = 'error'
        this.mapError = e instanceof Error ? e.message : String(e)
      }
    },

    /** 真正写入 ST（读全量 → 合并 → 写全量 → 读回校验）。参数变更后**立即**调用，不走定时器。 */
    async syncNow(force = false) {
      // 并发保护：正在同步时挂起一次重跑，保证最后一次变更不会被丢
      if (this.syncing) {
        this.syncAgain = true
        return
      }
      this.syncing = true
      this.mapError = ''
      try {
        // 先比对：全部一致就不写。ST 每次 save 都会做一次 autosave 备份，
        // 无谓写入只会堆垃圾。
        const before = await inspectMapping(this.mappingInput())
        this.mapEntries = before
        if (!force && before.every((e) => e.inSync)) {
          this.mapState = 'synced'
          return
        }
        const rep = await applyMapping(this.mappingInput())
        this.mapEntries = rep.entries
        this.mapState = rep.ok ? 'synced' : 'pending'
        if (!rep.ok) this.mapError = rep.message
      } catch (e) {
        this.mapState = 'error'
        this.mapError = e instanceof Error ? e.message : String(e)
      } finally {
        this.syncing = false
        if (this.syncAgain) {
          this.syncAgain = false
          void this.syncNow(force)
        }
      }
    },
  },
})

/* 开发期热更新接管：改本 store 后无需手动刷新页面（Pinia 官方推荐写法） */
if (import.meta.hot) {
  import.meta.hot.accept(acceptHMRUpdate(useSettingsStore, import.meta.hot))
}
