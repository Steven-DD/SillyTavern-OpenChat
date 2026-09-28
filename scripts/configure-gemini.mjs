/**
 * Gemini 接入配置器
 *
 * 作用：把 GEMINI_API_KEY 写入 SillyTavern 的加密 secret 存储（键名 api_key_makersuite），
 *      使 ST 可通过原生 Google AI Studio 通道调用 Gemini，无需 OpenAI 兼容层。
 *
 * key 来源优先级：
 *   1. 进程环境变量 GEMINI_API_KEY
 *   2. Windows 用户环境变量（注册表 HKCU\Environment，WorkBuddy 进程未继承时回退到此）
 *
 * 用法：node scripts/configure-gemini.mjs
 */
import { execSync } from 'node:child_process'

const BASE = process.env.ST_BASE ?? 'http://127.0.0.1:8000'
const SECRET_KEY = 'api_key_makersuite'

/** 从注册表读取用户级环境变量（兜底：WorkBuddy 进程环境可能没继承到） */
function readKeyFromRegistry() {
  try {
    const out = execSync('reg query "HKCU\\Environment" /v GEMINI_API_KEY', {
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'ignore'],
    })
    const m = out.match(/GEMINI_API_KEY\s+REG_SZ\s+(\S+)/)
    return m?.[1]
  } catch {
    return undefined
  }
}

function resolveKey() {
  const fromEnv = process.env.GEMINI_API_KEY
  if (fromEnv) return { key: fromEnv, src: 'process.env' }
  const fromReg = readKeyFromRegistry()
  if (fromReg) return { key: fromReg, src: 'HKCU\\Environment' }
  return { key: undefined, src: 'none' }
}

/** 取 CSRF token + 配套 Cookie（ST 要求双提交校验） */
async function getCsrf() {
  const res = await fetch(`${BASE}/csrf-token`)
  if (!res.ok) throw new Error(`/csrf-token HTTP ${res.status}`)
  const setCookies = res.headers.getSetCookie?.() ?? []
  const cookie = setCookies.map((c) => c.split(';')[0]).join('; ')
  const data = await res.json()
  if (!data.token) throw new Error('csrf-token 响应缺少 token 字段')
  return { token: data.token, cookie }
}

async function stPost(path, body) {
  const { token, cookie } = await getCsrf()
  return fetch(`${BASE}${path}`, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      'X-CSRF-Token': token,
      Cookie: cookie,
    },
    body: JSON.stringify(body),
  })
}

const { key, src } = resolveKey()
if (!key) {
  console.error('[gemini] ❌ 未找到 GEMINI_API_KEY（进程环境与 HKCU\\Environment 均无）')
  process.exit(1)
}
console.log(`[gemini] key 来源: ${src}，长度 ${key.length}，前缀 ${key.slice(0, 7)}…`)

// 1) 写 secret
const writeRes = await stPost('/api/secrets/write', {
  key: SECRET_KEY,
  value: key,
  label: 'Gemini API Key (Google AI Studio)',
})
const writeBody = await writeRes.text()
console.log(`[gemini] 写入 secret 键名 ${SECRET_KEY} → HTTP ${writeRes.status} ${writeBody}`)

// 2) 读回 secret 状态确认（/read 返回 state，不泄明文）
const readRes = await stPost('/api/secrets/read', {})
if (readRes.ok) {
  const state = await readRes.json()
  const has = Array.isArray(state) ? state.includes(SECRET_KEY) : SECRET_KEY in state
  console.log(`[gemini] secret 校验: ${has ? '✅ 已存在' : '⚠️ 未在 state 中找到'}（state 条目 ${Array.isArray(state) ? state.length : Object.keys(state).length}）`)
} else {
  console.log(`[gemini] secret 校验: 读取失败 HTTP ${readRes.status}`)
}

if (writeRes.ok) {
  console.log('[gemini] ✅ 配置完成：ST 可用 chat_completion_source="makersuite" + model="gemini-3.8-flash"')
} else {
  process.exitCode = 1
}
