/**
 * SillyTavern API 客户端（通路层）
 *
 * ST 后端受 CSRF 保护：POST 需先从 /csrf-token 获取 token（Cookie + X-CSRF-Token 双提交）。
 * 生成接口返回 SSE 流（OpenAI / Google 原生 chunk 透传），统一在这里解析为增量文本。
 *
 * ── 基地址（两种运行形态）──
 * - 开发：留空 → 同源，由 Vite 代理转发到 127.0.0.1:8000
 * - 生产：由 Tauri Rust 中继注入 `http://127.0.0.1:<随机端口>`
 *   （中继代持会话 Cookie，见 src-tauri/src/relay.rs）
 * ⚠ 不能缓存成模块级常量：桥接在模块求值之后才写入基地址，缓存会拿到空值。
 */

/** 读取当前基地址 */
export function stBase(): string {
  return localStorage.getItem('st.base') ?? ''
}

/** 设置基地址（Tauri 启动 / 设置页手动填写时调用） */
export function setStBase(url: string): void {
  const v = url.trim()
  if (v) localStorage.setItem('st.base', v)
  else localStorage.removeItem('st.base')
  // 换基地址必须作废旧令牌，否则新会话会 403
  csrfToken = null
  csrfPending = null
}

/** 中继鉴权 token（桌面壳经中继访问时由 bridge 写入，浏览器开发 / 直连为空） */
export function setRelayToken(t: string): void {
  const v = t.trim()
  if (v) localStorage.setItem('st.relayToken', v)
  else localStorage.removeItem('st.relayToken')
}

/**
 * 中继鉴权头：中继代持着 ST 会话 Cookie（含密钥接口），除静态资源 GET 外
 * 一律要求 `X-Relay-Auth`。未配置 token时不附加，
 * 避免给直连场景引入多余的自定义头。
 */
function relayAuthHeaders(): Record<string, string> {
  const t = localStorage.getItem('st.relayToken') ?? ''
  return t ? { 'X-Relay-Auth': t } : {}
}

let csrfToken: string | null = null
/** 进行中的令牌获取（single-flight：并发请求共享同一次获取） */
let csrfPending: Promise<string> | null = null

async function ensureCsrf(): Promise<string> {
  if (csrfToken) return csrfToken
  // ⚠ 必须单飞：/csrf-token 每次调用都会轮换 session cookie，
  // 启动时多个 store 并发各取一次会互相覆盖 cookie → token 与 cookie
  // 不成对 → 全部 403（重试也会继续互踩）。共享同一次获取即可根治。
  csrfPending ??= (async () => {
    try {
      const res = await fetch(`${stBase()}/csrf-token`, {
        credentials: 'include',
        headers: relayAuthHeaders(),
      })
      if (!res.ok) throw new Error(`令牌获取失败（HTTP ${res.status}）`)
      const data = (await res.json()) as { token?: string }
      if (!data.token) throw new Error('令牌响应格式错误')
      csrfToken = data.token
      return csrfToken
    } catch (e) {
      csrfPending = null // 失败放行，下次重取
      throw e
    }
  })()
  return csrfPending
}

/** GET，返回纯文本（/view 类文本端点用；JSON 端点用 stGet） */
export async function stGetText(path: string): Promise<string> {
  const res = await fetch(`${stBase()}${path}`, {
    credentials: 'include',
    headers: relayAuthHeaders(),
  })
  if (!res.ok) throw new Error(`读取失败（${path}，HTTP ${res.status}）`)
  return res.text()
}

export async function stGet<T>(path: string): Promise<T> {
  const res = await fetch(`${stBase()}${path}`, {
    credentials: 'include',
    headers: relayAuthHeaders(),
  })
  if (!res.ok) throw new Error(`读取失败（${path}，HTTP ${res.status}）`)
  return (await res.json()) as T
}

/**
 * POST 公共逻辑：403 时自动换新 CSRF 令牌重试一次。
 * 场景：ST 重启后旧会话失效，缓存里的令牌全部作废 —— 不重试的话
 * 用户看到的会是莫名其妙的「HTTP 403」（实测：导入卡报「无法导入」即此因）。
 */
async function postWithCsrf(path: string, init: (token: string) => RequestInit): Promise<Response> {
  const send = (i: RequestInit) =>
    fetch(`${stBase()}${path}`, {
      method: 'POST',
      credentials: 'include',
      ...i,
      // 中继鉴权头在前，调用方的 CSRF/Content-Type 头在后可覆盖
      headers: { ...relayAuthHeaders(), ...(i.headers ?? {}) },
    })
  let token = await ensureCsrf()
  let res = await send(init(token))
  if (res.status === 403) {
    csrfToken = null
    csrfPending = null
    token = await ensureCsrf()
    res = await send(init(token))
  }
  return res
}

export async function stPost(
  path: string,
  body: unknown,
  opts?: { signal?: AbortSignal },
): Promise<Response> {
  return postWithCsrf(path, (token) => ({
    headers: {
      'Content-Type': 'application/json',
      'X-CSRF-Token': token,
    },
    body: JSON.stringify(body),
    signal: opts?.signal,
  }))
}

/**
 * POST multipart（头像上传等）。
 * ⚠ 不能手动设 Content-Type —— boundary 必须由浏览器生成；也不能 JSON.stringify。
 */
export async function stPostForm(path: string, form: FormData): Promise<Response> {
  return postWithCsrf(path, (token) => ({
    headers: { 'X-CSRF-Token': token },
    body: form,
  }))
}

/** POST + 解析 JSON（如 /api/settings/get） */
export async function stPostJson<T>(path: string, body: unknown = {}): Promise<T> {
  const res = await stPost(path, body)
  if (!res.ok) throw new Error(`写入失败（${path}，HTTP ${res.status}）`)
  return (await res.json()) as T
}

/**
 * POST，只看状态码、不解析响应体。
 * ST 不少写端点成功时是 `sendStatus(200)` → body 是纯文本 "OK"（不是 JSON）
 * 用 stPostJson 解析会抛「Unexpected token 'O'」——实测：worldinfo/delete、characters/edit。
 */
export async function stPostVoid(path: string, body: unknown): Promise<void> {
  const res = await stPost(path, body)
  if (!res.ok) throw new Error(`操作失败（${path}，HTTP ${res.status}）`)
}

/**
 * SSE 流式请求：逐块回调增量文本。
 *
 * ST 各后端源的流式格式并不统一：
 * - claude / openai 兼容源 → ST 转换为 OpenAI chunk（choices[0].delta.content）
 * - makersuite / vertexai → ST 以 &alt=sse 原样透传 **Google 原生格式**
 *   （candidates[0].content.parts[].text）
 * 因此这里同时解析两种格式；错误对象（error / message）统一抛出。
 */
export async function stStream(
  path: string,
  body: unknown,
  onDelta: (text: string) => void,
  onReasoning?: (text: string) => void,
  onUsage?: (usage: { prompt: number; completion: number }) => void,
  opts?: { signal?: AbortSignal },
): Promise<void> {
  const res = await stPost(path, body, { signal: opts?.signal })
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

      // SSE 事件以空行分隔：兼容 \n\n 与 \r\n\r\n（Google 使用后者）
      for (;;) {
        const sepMatch = /\r?\n\r?\n/.exec(buf)
        if (!sepMatch) break
        const raw = buf.slice(0, sepMatch.index)
        buf = buf.slice(sepMatch.index + sepMatch[0].length)
        for (const line of raw.split(/\r?\n/)) {
          if (!line.startsWith('data:')) continue
          const payload = line.slice(5).trim()
          if (!payload || payload === '[DONE]') continue
          try {
            const chunk = JSON.parse(payload) as {
              // OpenAI 兼容格式（reasoning_content：DeepSeek 等，reasoning：OpenRouter）
              choices?: {
                delta?: { content?: string; reasoning_content?: string; reasoning?: string }
                text?: string
              }[]
              // Google 原生格式（thought === true 的 part 是思考内容）
              candidates?: {
                content?: { parts?: { text?: string; thought?: boolean }[] }
                finishReason?: string
              }[]
              // token 用量（OpenAI 兼容：流式尾帧 usage，Google：usageMetadata 每帧携带）
              usage?: { prompt_tokens?: number; completion_tokens?: number }
              usageMetadata?: { promptTokenCount?: number; candidatesTokenCount?: number }
              // 错误
              error?: { message?: string; status?: string }
              message?: string
            }

            if (chunk.error) {
              throw new Error(`ST 返回错误（${chunk.error.message ?? chunk.error.status}）`)
            }
            if (chunk.message && !chunk.choices && !chunk.candidates) {
              throw new Error(`ST 返回错误（${chunk.message}）`)
            }

            let delta =
              chunk.choices?.[0]?.delta?.content ?? chunk.choices?.[0]?.text ?? ''

            // Google 原生：合并同一 chunk 内的多个 part（thought part 是思考内容，分流）
            if (chunk.candidates) {
              const parts = chunk.candidates[0]?.content?.parts ?? []
              const thought = parts
                .filter((p) => p.thought)
                .map((p) => p.text ?? '')
                .join('')
              if (thought && onReasoning) onReasoning(thought)
              delta = delta || parts.filter((p) => !p.thought).map((p) => p.text ?? '').join('')
            }

            // OpenAI 兼容思考增量
            const reasoningDelta =
              chunk.choices?.[0]?.delta?.reasoning_content ??
              chunk.choices?.[0]?.delta?.reasoning ??
              ''
            if (reasoningDelta && onReasoning) onReasoning(reasoningDelta)

            // token 用量：两种格式都在尾部/每帧携带，重复回调无妨（调用方取最后一次）
            if (onUsage) {
              const u = chunk.usage
              const g = chunk.usageMetadata
              if (u && (typeof u.prompt_tokens === 'number' || typeof u.completion_tokens === 'number')) {
                onUsage({ prompt: u.prompt_tokens ?? 0, completion: u.completion_tokens ?? 0 })
              } else if (g && (typeof g.promptTokenCount === 'number' || typeof g.candidatesTokenCount === 'number')) {
                onUsage({ prompt: g.promptTokenCount ?? 0, completion: g.candidatesTokenCount ?? 0 })
              }
            }

            if (delta) onDelta(delta)
          } catch (e) {
            if (e instanceof SyntaxError) continue // 非 JSON 心跳行，忽略
            throw e
          }
        }
      }
    }
  } finally {
    // 异常路径（chunk.error / 上游中断）也释放底层连接，此前会悬挂到 GC
    reader.cancel().catch(() => {})
  }
}
