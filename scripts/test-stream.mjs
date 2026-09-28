/**
 * SSE 流式生成测试：复刻前端 client.ts 的 stStream 解析逻辑，
 * 验证「ST → Google → SSE 增量透传 → 增量回调」全链路。
 *
 * 用法：node scripts/test-stream.mjs ["提示词"] [模型]
 */
// dev server 已固定绑 127.0.0.1（见 vite.config.ts server.host），这里保持一致
const BASE = process.env.ST_BASE ?? 'http://127.0.0.1:1420'
const prompt = process.argv[2] ?? '数出 1 到 5，用逗号分隔。'
const model = process.argv[3] ?? 'gemini-3.8-flash'

async function getCsrf() {
  const res = await fetch(`${BASE}/csrf-token`)
  const setCookies = res.headers.getSetCookie?.() ?? []
  return {
    token: (await res.json()).token,
    cookie: setCookies.map((c) => c.split(';')[0]).join('; '),
  }
}

const { token, cookie } = await getCsrf()
const t0 = Date.now()

const res = await fetch(`${BASE}/api/backends/chat-completions/generate`, {
  method: 'POST',
  headers: {
    'Content-Type': 'application/json',
    'X-CSRF-Token': token,
    Cookie: cookie,
  },
  body: JSON.stringify({
    chat_completion_source: 'makersuite',
    model,
    messages: [{ role: 'user', content: prompt }],
    max_tokens: 300,
    temperature: 0.7,
    stream: true,
  }),
})

console.log(`[stream] HTTP ${res.status} 模型=${model}`)
if (!res.ok) {
  console.log('[stream] 错误响应:', (await res.text()).slice(0, 400))
  process.exit(1)
}

const reader = res.body.getReader()
const decoder = new TextDecoder()
let buf = ''
let chunks = 0
let out = ''
let firstAt = 0

for (;;) {
  const { done, value } = await reader.read()
  if (done) break
  buf += decoder.decode(value, { stream: true })

  // SSE 事件以空行分隔：兼容 \n\n 与 \r\n\r\n（Google 用后者）
  for (;;) {
    const sepMatch = /\r?\n\r?\n/.exec(buf)
    if (!sepMatch) break
    const raw = buf.slice(0, sepMatch.index)
    buf = buf.slice(sepMatch.index + sepMatch[0].length)
    for (const line of raw.split(/\r?\n/)) {
      if (!line.startsWith('data:')) {
        if (process.env.DEBUG) console.log('[raw-nondata]', line.slice(0, 200))
        continue
      }
      const payload = line.slice(5).trim()
      if (process.env.DEBUG) console.log('[raw-data]', payload.slice(0, 300))
      if (!payload || payload === '[DONE]') continue
      try {
        const chunk = JSON.parse(payload)
        if (chunk?.error) {
          console.log('[stream] ❌ Google 错误:', JSON.stringify(chunk.error).slice(0, 200))
          continue
        }
        let delta = chunk?.choices?.[0]?.delta?.content ?? chunk?.choices?.[0]?.text ?? ''
        // Google 原生格式（makersuite 原样透传）
        if (!delta && chunk?.candidates) {
          delta = (chunk.candidates[0]?.content?.parts ?? [])
            .map((p) => p.text ?? '')
            .join('')
        }
        if (delta) {
          if (!firstAt) firstAt = Date.now() - t0
          chunks++
          out += delta
          process.stdout.write(delta)
        }
      } catch {
        /* 忽略非 JSON 行 */
      }
    }
  }
}

console.log()
console.log('----------------------------------------')
console.log(`[stream] ✅ 增量片段 ${chunks} 个，首字 ${firstAt}ms，总计 ${Date.now() - t0}ms`)
console.log(`[stream] 拼接结果: ${out.trim().slice(0, 200)}`)
