/**
 * Text Completion 生成通道（对齐 ST textgen 15 后端）
 *
 * ST 服务器只做代理转发（src/endpoints/backends/text-completions.js）：
 * - 请求体：prompt 为**单字符串**（非 messages），api_type = textgen_types 值
 * - 流式：透传上游 SSE，主流格式 `choices[].text`；llamacpp 原生 `content`；ollama 被 ST
 *   包装为 `choices[].text` + `thinking`；结束 `data: [DONE]`
 * - 托管类后端 URL 固定 + 密钥（api_key_{id} 或独立 key）；本地类由用户填 server URL
 * - 采样：通用 OpenAI 风格字段 + 后端白名单由 ST 服务端裁剪（generic/ooba 等整体转发）
 */
import { stPost, stPostJson } from './client'

export interface TextGenBackendDef {
  /** ST textgen_types 值（api_type） */
  id: string
  label: string
  /** 固定 API 服务器（托管类）；空 = 用户自填（本地类） */
  fixedServer?: string
  /** secrets key（托管类需要密钥） */
  secretKey?: string
  /** 模型列表端点（默认 {server}/v1/models） */
  modelsPath?: string
}

/** 全部 15 后端（textgen-settings.js:31-47 + SERVER_INPUTS:122-141） */
export const TEXTGEN_BACKENDS: TextGenBackendDef[] = [
  { id: 'koboldcpp', label: 'KoboldCpp', secretKey: 'api_key_koboldcpp' },
  { id: 'ooba', label: 'Text Generation WebUI (ooba)', secretKey: 'api_key_ooba' },
  { id: 'llamacpp', label: 'llama.cpp server' },
  { id: 'ollama', label: 'Ollama' },
  { id: 'vllm', label: 'vLLM', secretKey: 'api_key_vllm' },
  { id: 'aphrodite', label: 'Aphrodite', secretKey: 'api_key_aphrodite' },
  { id: 'tabby', label: 'TabbyAPI', secretKey: 'api_key_tabby' },
  { id: 'generic', label: '自定义（OpenAI 兼容）', secretKey: 'api_key_generic' },
  { id: 'togetherai', label: 'Together.ai', fixedServer: 'https://api.together.xyz', secretKey: 'api_key_togetherai' },
  { id: 'infermaticai', label: 'Infermatic', fixedServer: 'https://api.infermatic.ai', secretKey: 'api_key_infermaticai' },
  { id: 'dreamgen', label: 'DreamGen', fixedServer: 'https://api.dreamgen.com', secretKey: 'api_key_dreamgen' },
  { id: 'openrouter', label: 'OpenRouter (Text)', fixedServer: 'https://openrouter.ai/api', secretKey: 'api_key_openrouter' },
  { id: 'featherless', label: 'Featherless', fixedServer: 'https://api.featherless.ai', secretKey: 'api_key_featherless' },
  { id: 'mancer', label: 'Mancer', fixedServer: 'https://neuro.mancer.tech/webui/api', secretKey: 'api_key_mancer' },
  { id: 'huggingface', label: 'HuggingFace', fixedServer: 'https://router.huggingface.co', secretKey: 'api_key_huggingface' },
]

/** OpenRouter Text 走 chat/completions 端点（text-completions.js:324 特例） */
function completionPath(apiType: string): string {
  switch (apiType) {
    case 'dreamgen': return '/api/openai/v1/completions'
    case 'mancer': return '/oai/v1/completions'
    case 'llamacpp': return '/completion'
    case 'ollama': return '/api/generate'
    case 'openrouter': return '/v1/chat/completions'
    default: return '/v1/completions'
  }
}

export interface TextGenSampler {
  temperature?: number
  topP?: number
  topK?: number
  minP?: number
  typicalP?: number
  repetitionPenalty?: number
  frequencyPenalty?: number
  presencePenalty?: number
  seed?: number
}

/** 采样参数 → 各后端字段（llamacpp 用别名 repeat_penalty/n_predict，ST nonAphroditeParams） */
function buildSampler(apiType: string, s: TextGenSampler): Record<string, unknown> {
  const body: Record<string, unknown> = {}
  if (s.temperature !== undefined) body.temperature = s.temperature
  if (s.topP !== undefined) body.top_p = s.topP
  if (s.topK) body.top_k = s.topK
  if (s.minP) body.min_p = s.minP
  if (s.typicalP) body.typical_p = s.typicalP
  if (s.seed) body.seed = s.seed
  if (apiType === 'llamacpp') {
    if (s.repetitionPenalty !== undefined && s.repetitionPenalty !== 1) body.repeat_penalty = s.repetitionPenalty
    if (s.frequencyPenalty) body.frequency_penalty = s.frequencyPenalty
    if (s.presencePenalty) body.presence_penalty = s.presencePenalty
  } else {
    if (s.repetitionPenalty !== undefined && s.repetitionPenalty !== 1) body.repetition_penalty = s.repetitionPenalty
    if (s.frequencyPenalty) body.frequency_penalty = s.frequencyPenalty
    if (s.presencePenalty) body.presence_penalty = s.presencePenalty
  }
  return body
}

/** OpenAI 风格 messages → 单字符串 prompt（instruct 关闭时的默认拼接级别） */
export function messagesToPrompt(
  messages: { role: 'system' | 'user' | 'assistant'; content: string }[],
  charName: string,
  userName: string,
): string {
  const parts: string[] = []
  let lastRole = ''
  for (const m of messages) {
    if (m.role === 'system') {
      parts.push(m.content)
      continue
    }
    const who = m.role === 'user' ? userName : charName
    if (lastRole === m.role) parts.push(`${m.content}`)
    else parts.push(`${who}: ${m.content}`)
    lastRole = m.role
  }
  // 以角色名前缀结尾，诱导模型接续回复
  parts.push(`${charName}:`)
  return parts.join('\n\n')
}

export interface TextGenOptions {
  prompt: string
  model: string
  backend: string
  /** 空 = 用后端 fixedServer（仅托管类） */
  server?: string
  maxTokens?: number
  stream?: boolean
  stop?: string[]
  sampler?: TextGenSampler
  bannedWords?: string[]
  signal?: AbortSignal
}

export function buildBody(o: TextGenOptions): Record<string, unknown> {
  const def = TEXTGEN_BACKENDS.find((b) => b.id === o.backend)
  const server = (o.server || def?.fixedServer || '').replace(/\/+$/, '')
  if (!server) throw new Error(`后端 ${o.backend} 需要填写 API 服务器地址`)
  // ST 后端按 api_type 在 server 后拼接端点路径（text-completions.js:296-324）
  const path = completionPath(o.backend)
  const body: Record<string, unknown> = {
    prompt: o.prompt,
    api_type: o.backend,
    api_server: server + path,
    model: o.model,
    stream: o.stream ?? true,
    ...buildSampler(o.backend, o.sampler ?? {}),
  }
  const maxTokens = o.maxTokens ?? 512
  if (o.backend === 'llamacpp' || o.backend === 'ollama') body.n_predict = maxTokens
  else body.max_tokens = maxTokens
  if (o.stop?.length && o.backend !== 'ollama') {
    body.stop = o.backend === 'koboldcpp' ? o.stop : o.stop.slice(0, 4)
  }
  // {{banned}} 宏收集的禁词（openai 兼容后端支持 ban_param_json；其余后端忽略）
  if (o.bannedWords?.length && ['generic', 'ooba', 'aphrodite', 'vllm', 'tabby'].includes(o.backend)) {
    body.ban_param_json = o.bannedWords.map((w) => [{ string: w, enabled: true }])
  }
  if (o.backend === 'ollama') {
    body.raw = true
    body.options = {
      temperature: o.sampler?.temperature,
      top_p: o.sampler?.topP,
      top_k: o.sampler?.topK,
      num_predict: maxTokens,
    }
  }
  if (o.backend === 'koboldcpp') body.trim_stop = true
  return body
}

/**
 * Text Completion 生成（SSE 流式）。
 * 解析三种上游格式：OpenAI completions `choices[].text`、llamacpp `content`、
 * ollama（ST 已包装为 choices[].text + thinking）。
 */
export async function generateText(
  o: TextGenOptions,
  onDelta: (text: string) => void,
  onThinking?: (text: string) => void,
): Promise<void> {
  const res = await stPost('/api/backends/text-completions/generate', buildBody(o), {
    signal: o.signal,
  })
  if (!res.ok) {
    let detail = `HTTP ${res.status}`
    try {
      const err = (await res.json()) as { message?: string; error?: { message?: string } }
      if (err.error?.message) detail = err.error.message
      else if (err.message) detail = err.message
    } catch {
      /* 保留状态码 */
    }
    throw new Error(`生成请求失败（${detail}）`)
  }
  if (!res.body) throw new Error('响应内容为空')

  const reader = res.body.getReader()
  const decoder = new TextDecoder()
  let buf = ''
  try {
    for (;;) {
      const { done, value } = await reader.read()
      if (done) break
      buf += decoder.decode(value, { stream: true })
      for (;;) {
        const sep = /\r?\n\r?\n/.exec(buf)
        if (!sep) break
        const raw = buf.slice(0, sep.index)
        buf = buf.slice(sep.index + sep[0].length)
        for (const line of raw.split(/\r?\n/)) {
          if (!line.startsWith('data:')) continue
          const payload = line.slice(5).trim()
          if (!payload || payload === '[DONE]') continue
          try {
            const chunk = JSON.parse(payload) as {
              choices?: { text?: string; thinking?: string }[]
              content?: string
              response?: string
              error?: { message?: string }
            }
            if (chunk.error) throw new Error(`ST 返回错误（${chunk.error.message}）`)
            const t = chunk.choices?.[0]?.text ?? chunk.content ?? chunk.response ?? ''
            const th = chunk.choices?.[0]?.thinking
            if (th && onThinking) onThinking(th)
            if (t) onDelta(t)
          } catch (e) {
            if (e instanceof SyntaxError) continue
            throw e
          }
        }
      }
    }
  } finally {
    // 异常路径也释放底层连接（此前 reader 悬挂到 GC 才回收）
    reader.cancel().catch(() => {})
  }
}

/** 拉取模型列表（{server}/v1/models；托管类服务器固定；密钥由 ST 服务端 secrets 提供） */
export async function fetchTextModels(backend: string, server?: string): Promise<string[]> {
  const def = TEXTGEN_BACKENDS.find((b) => b.id === backend)
  const base = (server || def?.fixedServer || '').replace(/\/+$/, '')
  if (!base) throw new Error('请先填写 API 服务器地址')
  const path = def?.modelsPath ?? '/v1/models'
  const data = await stPostJson<{ data?: { id?: string }[]; models?: { name?: string }[] }>(
    '/api/backends/text-completions/status',
    { api_type: backend, api_server: base + path },
  )
  const ids = (data?.data ?? data?.models ?? [])
    .map((m) => (('id' in m ? m.id : (m as { name?: string }).name) ?? '').trim())
    .filter((s): s is string => !!s)
  if (!ids.length) throw new Error('后端未返回任何模型（请检查服务器地址/密钥）')
  return ids
}
