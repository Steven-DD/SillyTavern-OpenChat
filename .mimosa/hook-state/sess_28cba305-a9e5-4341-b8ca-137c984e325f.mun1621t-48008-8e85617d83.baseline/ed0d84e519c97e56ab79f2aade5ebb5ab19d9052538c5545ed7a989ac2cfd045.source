/**
 * 配置映射 · 纯逻辑层（无 IO，无依赖）
 *
 * 与 `mapping.ts` 拆开的理由：这一层是**映射算法的本体**（映射表，嵌套读写，差异比较）
 * 必须能被真代码直接测试；IO 放另一侧，测试时用 node 的真实 HTTP 验证传输链路。
 * 混在一个文件里的话，测纯逻辑就得连网络一起拉起。
 */

/** 这条映射影响什么（UI 据此分组说明） */
export type MappingRole = 'identity' | 'generation' | 'memory'

export interface MappingEntry {
  /** 本 App 侧的字段名（人类可读） */
  app: string
  /** 本 App 侧的值 */
  appValue: unknown
  /** ST settings.json 里的点分路径 */
  stPath: string
  role: MappingRole
  /** 为什么映射它 / 有什么效果 */
  note: string
}

export interface MappingDiff extends MappingEntry {
  stValue: unknown
  inSync: boolean
}

/** 供映射的 App 侧配置（由调用方从各 store 取，保持本模块纯粹） */
export interface AppConfigForMapping {
  /** 当前生成通道（决定 model 写进哪个 `<source>_model` 键） */
  source: string
  /** 主模型（空 = 用户尚未选择 → 不映射，保留 ST 侧原值） */
  model: string
  temperature: number
  maxTokens: number
  maxContext: number
  topP: number
  topK: number
  topA: number
  minP: number
  frequencyPenalty: number
  presencePenalty: number
  repetitionPenalty: number
}

const ROLE_LABEL: Record<MappingRole, string> = {
  identity: '身份',
  generation: '生成',
  memory: '记忆',
}

export function roleLabel(r: MappingRole): string {
  return ROLE_LABEL[r]
}

/**
 * 各通道的模型在 `oai_settings` 里的键名 —— 键名全部来自实测 settings.json。
 * 注意两个历史命名例外：makersuite → `google_model`；custom → `custom_model`。
 */
const SOURCE_MODEL_KEY: Record<string, string> = {
  openai: 'openai_model',
  claude: 'claude_model',
  openrouter: 'openrouter_model',
  makersuite: 'google_model',
  vertexai: 'vertexai_model',
  mistralai: 'mistralai_model',
  custom: 'custom_model',
  cohere: 'cohere_model',
  perplexity: 'perplexity_model',
  groq: 'groq_model',
  chutes: 'chutes_model',
  electronhub: 'electronhub_model',
  nanogpt: 'nanogpt_model',
  deepseek: 'deepseek_model',
  aimlapi: 'aimlapi_model',
  xai: 'xai_model',
  pollinations: 'pollinations_model',
  moonshot: 'moonshot_model',
  fireworks: 'fireworks_model',
  cometapi: 'cometapi_model',
  ai21: 'ai21_model',
  zai: 'zai_model',
  siliconflow: 'siliconflow_model',
  minimax: 'minimax_model',
  workers_ai: 'workers_ai_model',
  azure_openai: 'azure_openai_model',
}

/** 当前通道的模型在 ST settings 里的点分路径（未知通道返回空串） */
export function modelStPath(source: string): string {
  const key = SOURCE_MODEL_KEY[source]
  return key ? `oai_settings.${key}` : ''
}

/**
 * 映射表 —— 键名全部来自实测（读真实 settings.json 得到），不是猜的。
 *
 * 覆盖本 App 拥有主导权的**全部**配置：通道、模型（按通道写入对应键）
 * 采样参数全套、生成上限（OpenAI 通道与全局各一份，两处都同步保持一致）。
 *
 * 刻意不收 `username`：本 App 的用户名是**从 ST 读来**的（`{{user}}` 宏取值）
 * 我们没有改它的入口 —— 放进来只会是一条永远「一致」的空转映射。
 * 等「用户角色（Persona）」功能上线、App 真正拥有这个值的主导权再加。
 */
export function buildEntries(cfg: AppConfigForMapping): MappingEntry[] {
  const entries: MappingEntry[] = [
    {
      app: '生成通道',
      appValue: cfg.source,
      stPath: 'oai_settings.chat_completion_source',
      role: 'generation',
      note: 'ST 侧 Chat Completion 来源（与 App 当前选中的供应商一致）',
    },
    {
      app: '温度',
      appValue: cfg.temperature,
      stPath: 'oai_settings.temp_openai',
      role: 'generation',
      note: 'ST 侧默认温度（本 App 每次请求仍显式传参）',
    },
    {
      app: 'Top P',
      appValue: cfg.topP,
      stPath: 'oai_settings.top_p_openai',
      role: 'generation',
      note: '核采样',
    },
    {
      app: 'Top K',
      appValue: cfg.topK,
      stPath: 'oai_settings.top_k_openai',
      role: 'generation',
      note: '0 = 关闭',
    },
    {
      app: 'Top A',
      appValue: cfg.topA,
      stPath: 'oai_settings.top_a_openai',
      role: 'generation',
      note: '0 = 关闭',
    },
    {
      app: 'Min P',
      appValue: cfg.minP,
      stPath: 'oai_settings.min_p_openai',
      role: 'generation',
      note: '0 = 关闭',
    },
    {
      app: '频率惩罚',
      appValue: cfg.frequencyPenalty,
      stPath: 'oai_settings.freq_pen_openai',
      role: 'generation',
      note: '',
    },
    {
      app: '存在惩罚',
      appValue: cfg.presencePenalty,
      stPath: 'oai_settings.pres_pen_openai',
      role: 'generation',
      note: '',
    },
    {
      app: '重复惩罚',
      appValue: cfg.repetitionPenalty,
      stPath: 'oai_settings.repetition_penalty_openai',
      role: 'generation',
      note: '',
    },
    {
      app: '单次回复上限',
      appValue: cfg.maxTokens,
      stPath: 'oai_settings.openai_max_tokens',
      role: 'generation',
      note: 'OpenAI 兼容通道的生成长度',
    },
    {
      app: '单次回复上限（全局）',
      appValue: cfg.maxTokens,
      stPath: 'amount_gen',
      role: 'generation',
      note: 'ST 侧全局默认生成长度（与上一项保持一致）',
    },
    {
      app: '上下文窗口',
      appValue: cfg.maxContext,
      stPath: 'oai_settings.openai_max_context',
      role: 'generation',
      note: 'OpenAI 兼容通道的上下文上限',
    },
    {
      app: '上下文窗口（全局）',
      appValue: cfg.maxContext,
      stPath: 'max_context',
      role: 'generation',
      note: 'ST 侧全局上下文上限（影响它自己的 prompt 裁剪）',
    },
  ]

  // 模型：按当前通道写入对应的 `<source>_model` 键；
  // 用户尚未选择（空串）时不映射 —— 写空串会清掉 ST 侧的原有默认。
  const modelPath = modelStPath(cfg.source)
  if (modelPath && cfg.model) {
    entries.unshift({
      app: '主模型',
      appValue: cfg.model,
      stPath: modelPath,
      role: 'generation',
      note: `写入 ${modelPath}（随通道切换自动换目标键）`,
    })
  }

  return entries
}

/* ------------------------------------------------------------------ *
 * 嵌套读写
 * ------------------------------------------------------------------ */

export type Dict = Record<string, unknown>

export function getPath(obj: Dict, path: string): unknown {
  return path.split('.').reduce<unknown>((acc, k) => {
    if (acc !== null && typeof acc === 'object') return (acc as Dict)[k]
    return undefined
  }, obj)
}

/** 逐层创建缺失的中间对象，保证嵌套写入不会因父级缺失而丢 */
export function setPath(obj: Dict, path: string, value: unknown): void {
  const parts = path.split('.')
  let cur: Dict = obj
  for (let i = 0; i < parts.length - 1; i++) {
    const k = parts[i]
    const next = cur[k]
    if (next === null || typeof next !== 'object') cur[k] = {}
    cur = cur[k] as Dict
  }
  cur[parts[parts.length - 1]] = value
}

export function same(a: unknown, b: unknown): boolean {
  if (a === b) return true
  if (typeof a === 'number' && typeof b === 'number') return Math.abs(a - b) < 1e-9
  if (a === null || a === undefined || b === null || b === undefined) return false
  return String(a) === String(b)
}

/* ------------------------------------------------------------------ *
 * 差异比较
 * ------------------------------------------------------------------ */

export function diffMapping(settings: unknown, cfg: AppConfigForMapping): MappingDiff[] {
  const dict = (settings ?? {}) as Dict
  return buildEntries(cfg).map((e) => {
    const stValue = getPath(dict, e.stPath)
    return { ...e, stValue, inSync: same(stValue, e.appValue) }
  })
}

/**
 * 把 App 配置合并进一份 settings 快照（**不改入参**，返回新对象）。
 *
 * 这是 `applyMapping` 的核心：ST 的 `/api/settings/save` 是整体覆盖，
 * 所以必须拿着「读到的全量」合并后再整份写回，否则会抹掉用户的其它设置。
 */
export function mergeInto(settings: unknown, cfg: AppConfigForMapping): Dict {
  const merged = JSON.parse(JSON.stringify(settings ?? {})) as Dict
  for (const e of buildEntries(cfg)) setPath(merged, e.stPath, e.appValue)
  return merged
}
