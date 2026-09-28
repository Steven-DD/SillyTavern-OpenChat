/**
 * 端到端生成测试：走 ST 原生 Gemini 通道（makersuite）发一条对话请求。
 * 用法：node scripts/test-generate.mjs ["自定义提示词"]
 */
const BASE = process.env.ST_BASE ?? 'http://127.0.0.1:8000'
const prompt = process.argv[2] ?? '用一句话介绍你自己，并说明你的模型名称。'
const model = process.argv[3] ?? process.env.MODEL ?? 'gemini-3.8-flash'

async function getCsrf() {
  const res = await fetch(`${BASE}/csrf-token`)
  if (!res.ok) throw new Error(`/csrf-token HTTP ${res.status}`)
  const setCookies = res.headers.getSetCookie?.() ?? []
  return {
    token: (await res.json()).token,
    cookie: setCookies.map((c) => c.split(';')[0]).join('; '),
  }
}

const { token, cookie } = await getCsrf()

const body = {
  chat_completion_source: 'makersuite',
  model,
  messages: [
    { role: 'system', content: 'You are a helpful assistant. Reply in Chinese.' },
    { role: 'user', content: prompt },
  ],
  max_tokens: 300,
  temperature: 0.7,
  stream: false,
}

console.log(`[test] 请求 ${BASE}/api/backends/chat-completions/generate`)
console.log(`[test] source=makersuite model=${model}`)

const t0 = Date.now()
const res = await fetch(`${BASE}/api/backends/chat-completions/generate`, {
  method: 'POST',
  headers: {
    'Content-Type': 'application/json',
    'X-CSRF-Token': token,
    Cookie: cookie,
  },
  body: JSON.stringify(body),
})

const text = await res.text()
console.log(`[test] HTTP ${res.status}（${Date.now() - t0}ms）`)

try {
  const data = JSON.parse(text)
  if (data.error) {
    console.log('[test] ❌ ST 返回错误:', JSON.stringify(data.error).slice(0, 500))
  } else {
    const content = data?.choices?.[0]?.message?.content ?? data?.choices?.[0]?.text ?? JSON.stringify(data)
    console.log('[test] ✅ 模型回复:')
    console.log('----------------------------------------')
    console.log(typeof content === 'string' ? content.slice(0, 800) : content)
    console.log('----------------------------------------')
    if (data.usage) console.log('[test] token 用量:', JSON.stringify(data.usage))
  }
} catch {
  console.log('[test] 原始响应:', text.slice(0, 800))
}
