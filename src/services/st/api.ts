import { stPost, stPostJson, stStream } from './client'

/** 探测 ST 后端是否在线（连通性检查，供状态条使用）。ST 1.19: POST /api/settings/get */
export async function probeSt(): Promise<boolean> {
  try {
    await stPostJson('/api/settings/get')
    return true
  } catch {
    return false
  }
}

/**
 * 生成通道来源（与 ST 网页端「API 连接」面板的 Chat Completion Source 对齐；
 * UI 不同、字段与端点一致）。secretKey 均符合 ST 的 `api_key_<source>` 命名。
 */
export interface ChatSourceDef {
  /** ST 的 chat_completion_source 值 */
  id: string
  label: string
  /** ST secrets 的 key（writeSecret 用） */
  secretKey: string
  /** 支持反代端点（reverse_proxy 字段） */
  reverseProxy?: boolean
  /** 需要自定义端点 URL（custom 源） */
  customUrl?: boolean
  /** 密钥可选（可匿名） */
  keyOptional?: boolean
  /** 非空 = 该源暂不支持（显示原因并禁用） */
  disabled?: string
}

export const CHAT_SOURCES: ChatSourceDef[] = [
  { id: 'makersuite', label: 'Google AI Studio（Gemini）', secretKey: 'api_key_makersuite' },
  { id: 'openai', label: 'OpenAI', secretKey: 'api_key_openai', reverseProxy: true },
  { id: 'claude', label: 'Anthropic Claude', secretKey: 'api_key_claude' },
  { id: 'openrouter', label: 'OpenRouter', secretKey: 'api_key_openrouter' },
  { id: 'novel', label: 'NovelAI（补全）', secretKey: 'api_key_novel' },
  { id: 'horde', label: 'Horde（分布式众包）', secretKey: 'api_key_horde' },
  { id: 'custom', label: '自定义（OpenAI 兼容）', secretKey: 'api_key_custom', customUrl: true },
  { id: 'deepseek', label: 'DeepSeek 深度求索', secretKey: 'api_key_deepseek', reverseProxy: true },
  { id: 'moonshot', label: 'Moonshot 月之暗面', secretKey: 'api_key_moonshot', reverseProxy: true },
  { id: 'zai', label: '智谱 Z.ai', secretKey: 'api_key_zai' },
  { id: 'siliconflow', label: '硅基流动 SiliconFlow', secretKey: 'api_key_siliconflow' },
  { id: 'minimax', label: 'MiniMax', secretKey: 'api_key_minimax' },
  { id: 'xai', label: 'xAI（Grok）', secretKey: 'api_key_xai', reverseProxy: true },
  { id: 'mistralai', label: 'Mistral AI', secretKey: 'api_key_mistralai', reverseProxy: true },
  { id: 'groq', label: 'Groq', secretKey: 'api_key_groq' },
  { id: 'cohere', label: 'Cohere', secretKey: 'api_key_cohere' },
  { id: 'chutes', label: 'Chutes', secretKey: 'api_key_chutes' },
  { id: 'electronhub', label: 'ElectronHub', secretKey: 'api_key_electronhub' },
  { id: 'nanogpt', label: 'NanoGPT', secretKey: 'api_key_nanogpt' },
  { id: 'aimlapi', label: 'AI/ML API', secretKey: 'api_key_aimlapi' },
  { id: 'fireworks', label: 'Fireworks AI', secretKey: 'api_key_fireworks' },
  { id: 'perplexity', label: 'Perplexity', secretKey: 'api_key_perplexity' },
  { id: 'ai21', label: 'AI21', secretKey: 'api_key_ai21' },
  // cometapi：ST 1.19 后端硬禁用（chat-completions.js:2543 直接 throw），无法支持
  { id: 'cometapi', label: 'CometAPI', secretKey: 'api_key_cometapi', disabled: 'ST 1.19 后端已禁用该源（不可用）' },
  { id: 'pollinations', label: 'Pollinations', secretKey: 'api_key_pollinations' },
  { id: 'vertexai', label: 'Google Vertex AI（Express）', secretKey: 'api_key_vertexai' },
  { id: 'azure_openai', label: 'Azure OpenAI', secretKey: 'api_key_azure_openai' },
  { id: 'workers_ai', label: 'Cloudflare Workers AI', secretKey: 'api_key_workers_ai' },
]

export type ChatCompletionSource = string

const DEFAULT_SOURCE = 'makersuite'
const DEFAULT_MODEL = 'gemini-3.8-flash'

/** 按源读取连接附加配置（reverse proxy / custom URL / 源专属字段），持久化于 localStorage */
export interface ConnConfig {
  reverseProxy?: string
  customUrl?: string
  /** vertexai：认证模式（express = API key，full = 服务账号 JSON secret）与区域 */
  vertexaiAuthMode?: 'express' | 'full'
  vertexaiRegion?: string
  vertexaiProjectId?: string
  /** azure_openai 三件套 */
  azureBaseUrl?: string
  azureDeploymentName?: string
  azureApiVersion?: string
  /** workers_ai 账号 ID */
  workersAiAccountId?: string
  /** horde：仅使用受信任 worker（trusted_workers，horde.js:216） */
  trustedWorkersOnly?: boolean
}

const CONN_KEY = 'app.gen.conn'

export function getConnConfig(source: string): ConnConfig {
  try {
    const all = JSON.parse(localStorage.getItem(CONN_KEY) ?? '{}') as Record<string, ConnConfig>
    return all[source] ?? {}
  } catch {
    return {}
  }
}

export function setConnConfig(source: string, patch: ConnConfig): void {
  let all: Record<string, ConnConfig> = {}
  try {
    all = JSON.parse(localStorage.getItem(CONN_KEY) ?? '{}') as Record<string, ConnConfig>
  } catch {
    /* 忽坏档 */
  }
  all[source] = { ...all[source], ...patch }
  localStorage.setItem(CONN_KEY, JSON.stringify(all))
}

/** 当前生成通道 */
export function currentSource(): string {
  return localStorage.getItem('st.source') ?? DEFAULT_SOURCE
}

/** 当前模型 id */
export function currentModel(): string {
  return localStorage.getItem('st.model') ?? DEFAULT_MODEL
}

export interface GenerateOptions {
  /** 消息列表（OpenAI 格式） */
  messages: { role: 'system' | 'user' | 'assistant'; content: string }[]
  /** 模型 id，缺省取 currentModel() */
  model?: string
  /** 生成通道，缺省取 currentSource() */
  source?: ChatCompletionSource
  temperature?: number
  maxTokens?: number
  /** 采样参数（top_p / top_k / 惩罚等），缺省不透传 */
  sampler?: SamplerParams
  /** 预拼接的 Text Completion prompt（Horde 等补全式通道用；缺省回落 messagesToPrompt） */
  prompt?: string
  /** 停止序列（Horde stop_sequence；Instruct 模板的 output sequence 等） */
  stop?: string[]
  /** 上下文长度上限（Horde max_context_length；缺省 2048） */
  maxContext?: number
  /** token 用量回调（SSE 尾帧实测值；接口不给则不回调，由调用方估算） */
  onUsage?: (usage: { prompt: number; completion: number }) => void
  /** 中断信号（「停止生成」用；中断后已生成的增量内容保留在调用方） */
  signal?: AbortSignal
}

/** 备用模型：主模型高峰过载（Google 侧 503 UNAVAILABLE）时自动切换 */
export function currentFallbackModel(): string {
  return localStorage.getItem('st.modelFallback') ?? 'gemini-flash-latest'
}

const RETRY_DELAYS = [1500, 6000]

/**
 * Horde 文本生成（对齐 ST horde.js generateHorde：Text Completion + Instruct 拼接）。
 * - 提交：POST /api/horde/generate-text → {id}；payload 含 models / trusted_workers
 *   （horde.js:213-219），prompt 优先用调用方传入的 Instruct 拼接结果
 * - 轮询：POST /api/horde/task-status {taskId} → {done, faulted, generations:[{text}], queue_position}
 *   每 3s 一次，最长 300s
 * - 中断：AbortSignal 触发时调 cancel-task 后抛 AbortError（调用方按停止处理）
 */
async function hordeGenerate(
  opts: GenerateOptions,
  onDelta: (text: string) => void,
): Promise<void> {
  const { messagesToPrompt } = await import('./textgen')
  const prompt =
    opts.prompt ?? messagesToPrompt(opts.messages, 'Assistant', 'User')
  // models 必传（服务端原样透传，缺失时 aihorde 分配不可控或拒绝；horde.js:213-219）
  const models = [opts.model ?? currentModel()].filter(Boolean)
  const conn = getConnConfig('horde')
  const submit = await stPostJson<{ id?: string; error?: unknown }>('/api/horde/generate-text', {
    prompt,
    params: {
      n: 1,
      max_length: opts.maxTokens ?? 512,
      max_context_length: opts.maxContext ?? 2048,
      temperature: opts.temperature ?? 1,
      top_p: opts.sampler?.topP,
      top_k: opts.sampler?.topK,
      rep_pen: opts.sampler?.repetitionPenalty,
      // Instruct stop 序列（AI Horde 文本参数 stop_sequence）
      ...(opts.stop?.length ? { stop_sequence: opts.stop } : {}),
      // frmt* 系列：与 ST generateHorde 一致全部显式关闭（horde.js:207-211）
      frmtadsnsp: false,
      frmtrmblln: false,
      frmtrmspch: false,
      frmttriminc: false,
    },
    trusted_workers: conn.trustedWorkersOnly === true,
    models,
  })
  const taskId = submit?.id
  if (!taskId) {
    throw new Error(`Horde 提交失败：${JSON.stringify(submit) || '空响应'}`)
  }

  const sleepAbort = async (ms: number, signal?: AbortSignal): Promise<void> => {
    await new Promise<void>((resolve, reject) => {
      const t = setTimeout(resolve, ms)
      signal?.addEventListener(
        'abort',
        () => {
          clearTimeout(t)
          reject(new DOMException('Aborted', 'AbortError'))
        },
        { once: true },
      )
    })
  }

  const deadline = Date.now() + 300_000
  try {
    while (Date.now() < deadline) {
      await sleepAbort(3000, opts.signal)
      const st = await stPostJson<{
        done?: boolean
        faulted?: boolean
        queue_position?: number
        wait_time?: number
        generations?: { text?: string }[]
      }>('/api/horde/task-status', { taskId })
      if (st.done) {
        const text = st.generations?.[0]?.text
        if (text) onDelta(text)
        return
      }
      if (st.faulted) throw new Error('Horde 任务失败（faulted，稍后重试）')
    }
    throw new Error('Horde 任务超时（300s）；可到 Horde 网页端查看队列状态')
  } catch (e) {
    if (opts.signal?.aborted) {
      // 用户停止：尽力取消远端任务
      try {
        await stPostJson('/api/horde/cancel-task', { taskId })
      } catch {
        /* 取消失败不掩盖中断 */
      }
    }
    throw e
  }
}

/**
 * NAI 模型 → tokenizer 编码端点（nai-settings.js getTokenizerTypeForModel：
 * erato 用 llama3，其余用 nerdstash_v2）
 */
function naiTokenizerEndpoint(model: string): string {
  return model.includes('erato')
    ? '/api/tokenizers/llama3/encode'
    : '/api/tokenizers/nerdstash_v2/encode'
}

/** 文本 → token id 序列（ST tokenizer 服务端编码；失败返回 null） */
async function encodeText(text: string, model: string): Promise<number[] | null> {
  try {
    const r = await stPostJson<{ ids?: unknown }>(naiTokenizerEndpoint(model), { text })
    return Array.isArray(r.ids) ? (r.ids as number[]) : null
  } catch {
    return null
  }
}

/**
 * NovelAI 文本生成（P1-5 对齐 ST nai-settings.js getNovelGenerationData 全集）：
 * - NAI 专属采样参数（tfs/typical_p/slope/freq/presence/phrase_rep_pen/mirostat/math1/order）
 *   直接读 ST settings.nai_settings（与网页端同一份数据源）
 * - stop_sequences / bad_words_ids：按模型选 tokenizer 服务端编码为 token id
 *   （{{banned}} 宏产物 bannedWords → bad_words_ids）
 * - erato 模型补 <|startoftext|> 前缀（nai-settings.js:567-569）
 * - 非流式：等完整 {output} 一次性回调（NAI SSE 是 token 流格式，v1 取非流式）
 */
async function naiGenerate(
  model: string,
  opts: GenerateOptions,
  onDelta: (text: string) => void,
): Promise<void> {
  const { messagesToPrompt } = await import('./textgen')
  const { getSettings } = await import('./data')
  const settings = await getSettings()
  const nai = ((settings as Record<string, unknown>).nai_settings ?? {}) as Record<string, unknown>
  const num = (key: string, dflt: number): number => {
    const v = Number(nai[key])
    return Number.isFinite(v) ? v : dflt
  }
  const s = opts.sampler ?? {}
  const input =
    opts.prompt ?? messagesToPrompt(opts.messages, 'Assistant', 'User')
  // erato：补 NAI 特殊前缀（nai-settings.js:567-569）
  const finalInput = model.includes('erato')
    ? `<|startoftext|><|reserved_special_token81|>${input}`
    : input

  // stop 序列 / 禁词 → token ids（NAI API 只收 id 数组）
  const stopStrings = opts.stop ?? []
  const [stopIds, badIds] = await Promise.all([
    Promise.all(stopStrings.map((t) => encodeText(t, model))),
    (typeof nai.banned_tokens === 'string' && nai.banned_tokens.trim()
      ? Promise.all(
          nai.banned_tokens
            .split('\n')
            .map((t) => t.trim())
            .filter(Boolean)
            .map((t) => encodeText(t, model)),
        )
      : Promise.resolve([])
    ).then((arr) => arr.filter((x): x is number[] => Array.isArray(x) && x.length > 0)),
  ])
  const stopSequences = stopIds.every((x) => Array.isArray(x)) ? stopIds : undefined
  const badWordsIds = badIds.length ? badIds : undefined

  const body: Record<string, unknown> = {
    input: finalInput,
    model,
    use_string: true,
    temperature: opts.temperature ?? num('temperature', 1.5),
    max_length: opts.maxTokens ?? 512,
    min_length: num('min_length', 1),
    tail_free_sampling: num('tail_free_sampling', 0.975),
    repetition_penalty: s.repetitionPenalty ?? num('repetition_penalty', 2.25),
    repetition_penalty_range: num('repetition_penalty_range', 2048),
    repetition_penalty_slope: num('repetition_penalty_slope', 0.09),
    repetition_penalty_frequency: num('repetition_penalty_frequency', 0),
    repetition_penalty_presence: num('repetition_penalty_presence', 0.005),
    top_a: s.topA ?? num('top_a', 0.08),
    top_p: s.topP ?? num('top_p', 0.75),
    top_k: s.topK ?? num('top_k', 10),
    min_p: s.minP ?? num('min_p', 0),
    typical_p: num('typical_p', 0.975),
    mirostat_lr: num('mirostat_lr', 0),
    mirostat_tau: num('mirostat_tau', 0),
    math1_temp: num('math1_temp', 1),
    math1_quad: num('math1_quad', 0),
    math1_quad_entropy_scale: num('math1_quad_entropy_scale', 0),
    phrase_rep_pen: (nai.phrase_rep_pen as string) || 'off',
    // stop/bad_words 为空时不传：ST 服务端会补模型默认禁词与 logit bias（novelai.js:183-206）
    ...(stopSequences?.length ? { stop_sequences: stopSequences } : {}),
    ...(badWordsIds?.length ? { bad_words_ids: badWordsIds } : {}),
    generate_until_sentence: true,
    use_cache: false,
    return_full_text: false,
    prefix: (nai.prefix as string) || '',
    order: nai.order ?? [1, 5, 0, 2, 3, 4],
  }
  const res = (await (await stPost('/api/novelai/generate', body, { signal: opts.signal })).json()) as {
    output?: string
    message?: string
    error?: unknown
  }
  if (res && typeof res.output === 'string' && res.output) {
    onDelta(res.output)
    return
  }
  const detail = typeof res?.message === 'string' ? res.message : JSON.stringify(res)
  throw new Error(
    /400/i.test(detail) || /key/i.test(detail)
      ? 'NovelAI 未配置 Access Token：请到 ST 网页端「API 连接」填入密钥'
      : `NovelAI 生成失败：${detail || '空响应'}`,
  )
}

/** Google 侧临时性故障（过载/限流）——值得重试 */
function isRetryable(msg: string): boolean {
  return /503|UNAVAILABLE|high demand|overloaded|rate limit|quota|429|temporarily|exceeded/i.test(msg)
}

/**
 * Google 的 429 正文会给出建议等待秒数：`Please retry in 18.89s`。
 * 免费层配额是按分钟计的，退避不到这个时长重试必然再次 429（实测踩过）。
 * 上限 25s，避免极端情况下卡太久。
 */
function retryHintMs(msg: string): number {
  const m = /retry in ([\d.]+)s/i.exec(msg)
  const s = m ? Number(m[1]) : NaN
  if (!Number.isFinite(s) || s <= 0) return 0
  return Math.min(25000, Math.ceil((s + 2) * 1000))
}

function sleep(ms: number): Promise<void> {
  return new Promise((r) => setTimeout(r, ms))
}

/** 采样参数（与 ST 网页端 Chat Completion 采样面板对齐；ST 会按源取用、忽略不支持的） */
export interface SamplerParams {
  topP?: number
  topK?: number
  topA?: number
  minP?: number
  frequencyPenalty?: number
  presencePenalty?: number
  repetitionPenalty?: number
}

function buildBody(
  model: string,
  source: ChatCompletionSource,
  opts: GenerateOptions,
): Record<string, unknown> {
  const body: Record<string, unknown> = {
    // ⚠ 不要给消息加 name 字段：ST 的 convertGooglePrompt 会把 name 拼成 `name: ` 前缀塞进正文
    messages: opts.messages,
    model,
    temperature: opts.temperature ?? 0.9,
    max_tokens: opts.maxTokens ?? 2048,
    stream: true,
    chat_completion_source: source,
    // ⚠ 刻意不传 use_sysprompt：ST 只在它为真时才把 system 放进 systemInstruction。
    // 我们的历史以开场白（assistant）开头，若 system 走 systemInstruction，
    // Google 侧 contents 首轮会变成 model，触发 prefill 校验报错。
    // 保持 falsy → system 被折成首个 user 轮，contents[0].role === 'user'，安全。
    // 回归验证见 app/scripts/test-prompt.mjs
  }

  // 采样参数：仅在显式提供时透传（0 是合法值，不能用 ?? 吞掉）
  const s = opts.sampler
  if (s) {
    if (s.topP !== undefined) body.top_p = s.topP
    if (s.topK !== undefined) body.top_k = s.topK
    if (s.topA !== undefined) body.top_a = s.topA
    if (s.minP !== undefined) body.min_p = s.minP
    if (s.frequencyPenalty !== undefined) body.frequency_penalty = s.frequencyPenalty
    if (s.presencePenalty !== undefined) body.presence_penalty = s.presencePenalty
    if (s.repetitionPenalty !== undefined) body.repetition_penalty = s.repetitionPenalty
  }

  // 按源附加端点字段（密钥一律由 ST secrets 服务端托管）
  const conn = getConnConfig(source)
  const def = CHAT_SOURCES.find((d) => d.id === source)
  if (def?.reverseProxy && conn.reverseProxy) body.reverse_proxy = conn.reverseProxy
  if (source === 'vertexai') {
    body.vertexai_auth_mode = conn.vertexaiAuthMode ?? 'express'
    body.vertexai_region = conn.vertexaiRegion ?? 'us-central1'
    if (conn.vertexaiProjectId) body.vertexai_express_project_id = conn.vertexaiProjectId
  }
  if (source === 'azure_openai') {
    if (conn.azureBaseUrl) body.azure_base_url = conn.azureBaseUrl
    if (conn.azureDeploymentName) body.azure_deployment_name = conn.azureDeploymentName
    body.azure_api_version = conn.azureApiVersion ?? '2024-02-15-preview'
  }
  if (source === 'workers_ai' && conn.workersAiAccountId) {
    body.workers_ai_account_id = conn.workersAiAccountId
  }
  if (source === 'custom') {
    body.custom_url =
      conn.customUrl ?? 'https://generativelanguage.googleapis.com/v1beta/openai/'
    body.custom_include_headers = true
    body.custom_include_body = true
    // ⚠ 不传 custom_api_key：ST 的 generate/status 对 custom 源**只从服务端 secret 读密钥**
    //   （chat-completions.js: readSecret(dirs, SECRET_KEYS.CUSTOM)），前端传了也是无效参数。
  }
  return body
}

/* ------------------------------------------------------------------ *
 * 密钥与模型列表（ST 网页版「API 连接」面板的 App 侧等价物）
 * ------------------------------------------------------------------ */

/** 把密钥写入 ST secrets（服务端加密存储；本 App 不保存明文） */
export function writeSecret(key: string, value: string): Promise<unknown> {
  return stPostJson('/api/secrets/write', { key, value })
}

/**
 * 从 ST 拉取指定通道的可用模型列表（`/api/backends/chat-completions/status`）。
 *
 * 顺带就是密钥有效性的试金石：
 * - 未配密钥 → ST 返回 400（直接抛错）
 * - 密钥无效 → 上游报错由 ST 透传
 * - 上游临时故障 → ST 返回 { error: true, bypass: true }（软失败，也抛错提示稍后再试）
 */
export async function fetchModels(source: ChatCompletionSource): Promise<string[]> {
  // Horde：模型列表走独立端点（horde.js /text-models 透传 aihorde）
  if (source === 'horde') {
    const r = await stPostJson<{ models?: unknown } | unknown[]>('/api/horde/text-models', {})
    const list = Array.isArray(r) ? r : ((r as { models?: unknown })?.models ?? [])
    const ids = (list as { name?: string; value?: string }[])
      .map((m) => String(m?.name ?? m?.value ?? '').trim())
      .filter(Boolean)
    if (!ids.length) throw new Error('Horde 未返回任何模型')
    return ids
  }
  const def = CHAT_SOURCES.find((d) => d.id === source)
  const body: Record<string, unknown> = { chat_completion_source: source }
  const conn = getConnConfig(source)
  if (def?.reverseProxy && conn.reverseProxy) body.reverse_proxy = conn.reverseProxy
  if (source === 'vertexai') {
    body.vertexai_auth_mode = conn.vertexaiAuthMode ?? 'express'
    body.vertexai_region = conn.vertexaiRegion ?? 'us-central1'
    if (conn.vertexaiProjectId) body.vertexai_express_project_id = conn.vertexaiProjectId
  }
  if (source === 'azure_openai') {
    if (conn.azureBaseUrl) body.azure_base_url = conn.azureBaseUrl
    if (conn.azureDeploymentName) body.azure_deployment_name = conn.azureDeploymentName
    body.azure_api_version = conn.azureApiVersion ?? '2024-02-15-preview'
  }
  if (source === 'workers_ai' && conn.workersAiAccountId) {
    body.workers_ai_account_id = conn.workersAiAccountId
  }
  if (source === 'custom') {
    body.custom_url =
      conn.customUrl ?? 'https://generativelanguage.googleapis.com/v1beta/openai/'
    body.custom_include_headers = true
    body.custom_include_body = true
  }
  const r = await stPostJson<{ data?: { id?: string }[]; error?: boolean }>(
    '/api/backends/chat-completions/status',
    body,
  )
  if (r?.error) {
    throw new Error('拉取失败：请先保存该通道的密钥（或密钥/端点无效、上游临时故障）')
  }
  const ids = (r?.data ?? [])
    .map((m) => m.id?.trim())
    .filter((s): s is string => !!s)
  if (!ids.length) throw new Error('ST 未返回任何模型（请检查密钥或端点配置）')
  return ids
}

/**
 * Chat Completions 生成（SSE 流式），带**重试 + 模型回退**：
 *   1. 主模型失败且为 Google 侧临时故障 → 退避重试 2 次
 *   2. 主模型仍失败 → 切换备用模型再试 1 次
 * 一旦已吐出内容则不再重试（避免内容重复）。
 *
 * @returns 实际成功使用的模型 id
 */
export async function generate(
  opts: GenerateOptions,
  onDelta: (text: string) => void,
  onReasoning?: (text: string) => void,
): Promise<string> {
  const source = opts.source ?? currentSource()
  const primary = opts.model ?? currentModel()

  // NovelAI：独立端点（novelai.js /generate；非流式 {output}，与 chat-completions 代理无关）
  if (source === 'novel') {
    await naiGenerate(primary, opts, onDelta)
    return primary
  }

  // Horde：任务提交 + 轮询（horde.js generate-text/task-status/cancel-task）
  if (source === 'horde') {
    await hordeGenerate(opts, onDelta)
    return primary
  }

  if (!primary) {
    throw new Error('尚未选择模型：请到「设置 → 模型」连接并拉取模型列表后选择')
  }
  const fallback = opts.model ? '' : currentFallbackModel()
  const candidates = fallback && fallback !== primary ? [primary, fallback] : [primary]

  let lastErr: unknown = new Error('生成失败')

  for (const model of candidates) {
    for (let attempt = 0; attempt <= RETRY_DELAYS.length; attempt++) {
      let emitted = false
      try {
        await stStream(
          '/api/backends/chat-completions/generate',
          buildBody(model, source, opts),
          (d) => {
            emitted = true
            onDelta(d)
          },
          onReasoning,
          opts.onUsage,
          { signal: opts.signal },
        )
        return model
      } catch (e) {
        lastErr = e
        const msg = e instanceof Error ? e.message : String(e)
        // 用户主动停止：不重试、不回退，直接抛给调用方处理
        if (opts.signal?.aborted) throw e
        // 已产出内容，重试会导致重复输出 —— 直接结束
        if (emitted || !isRetryable(msg)) throw e
        if (attempt < RETRY_DELAYS.length) {
          // 配额/限流类错误按 Google 建议的等待时长退避，否则用固定退避
          await sleep(Math.max(retryHintMs(msg), RETRY_DELAYS[attempt]))
        }
      }
    }
  }

  throw lastErr
}
