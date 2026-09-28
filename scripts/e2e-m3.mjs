#!/usr/bin/env node
/**
 * M3 端到端验证（真实 HTTP + 真实模型调用）
 *
 * 覆盖：
 *  A. Prompt 引擎自检（把 TS 模块用 esbuild 打包后直接跑，测的是真代码不是复刻）
 *     - 宏替换 / 系统提示组装 / 历史角色映射 / 上下文裁剪 / 开场白种子
 *  B. ST 侧文件链路
 *     - 建会话（写入开场白）→ 落盘 → 读回校验 → 检查磁盘 jsonl 真实存在
 *     - 用「角色卡注入的 prompt」真实流式生成 → 追加回复 → 再落盘 → 读回校验
 *
 * 用法：node app/scripts/e2e-m3.mjs
 * 产物：logs/e2e-m3.txt（人读报告）、logs/e2e-m3.json（结构化）
 */
import { mkdirSync, writeFileSync, readdirSync, existsSync, statSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'

const __dirname = path.dirname(fileURLToPath(import.meta.url))
const APP = path.resolve(__dirname, '..')
const ROOT = path.resolve(APP, '..')
const LOGS = path.join(ROOT, 'logs')
mkdirSync(LOGS, { recursive: true })

const BASE = process.env.ST_BASE ?? 'http://127.0.0.1:8000'
const CHAT_DIR = path.join(ROOT, 'sillytavern', 'data', 'default-user', 'chats')

const lines = []
const report = { base: BASE, at: new Date().toISOString(), A: {}, B: {} }
let failures = 0

function log(s = '') {
  lines.push(s)
  console.log(s)
}
function check(name, ok, detail = '') {
  if (!ok) failures++
  log(`${ok ? '  ✅' : '  ❌'} ${name}${detail ? `  — ${detail}` : ''}`)
  return ok
}

/* ================================================================
 * A. Prompt 引擎自检（esbuild 打包 TS 后运行真代码）
 * ================================================================ */
async function bundlePromptEngine() {
  const esbuild = await import('esbuild')
  // 打包产物放 logs 下，不污染 app 目录（避免被 tsc/vite 扫描）
  const out = path.join(LOGS, '.e2e-tmp')
  mkdirSync(out, { recursive: true })
  const entry = path.join(out, 'entry.ts')
  // 用绝对路径 import，避免临时目录位置变化导致解析失败
  const mods = ['chatdoc', 'prompt', 'types']
    .map((m) => `export * from '${path.join(APP, 'src', 'services', 'st', m).replace(/\\/g, '/')}'`)
    .join('\n')
  writeFileSync(entry, mods, 'utf8')
  const outfile = path.join(out, 'bundle.mjs')
  await esbuild.build({
    entryPoints: [entry],
    bundle: true,
    format: 'esm',
    platform: 'node',
    target: 'node20',
    outfile,
    logLevel: 'silent',
  })
  return import(pathToFileURL(outfile).href)
}

async function runPromptSelfTest() {
  log('')
  log('===== A. Prompt 引擎自检（真代码，esbuild 打包后运行）=====')
  const st = await bundlePromptEngine()

  // A1 宏替换
  const s1 = st.substituteMacros('{{char}} 看着 {{user}}，<BOT> 笑了。<USER> 沉默。', 'Seraphina', '元宝')
  check('A1 宏替换 {{char}}/{{user}}/<BOT>/<USER>', s1 === 'Seraphina 看着 元宝，Seraphina 笑了。元宝 沉默。', s1)
  report.A.macro = s1

  // A2 token 估算：CJK 逐字，拉丁 4 字符 1 token
  const cjk = st.estimateTokens('你好世界')
  const latin = st.estimateTokens('abcdefgh')
  check('A2 token 估算（4 中文=4，8 拉丁=2）', cjk === 4 && latin === 2, `cjk=${cjk} latin=${latin}`)
  report.A.tokens = { cjk, latin }

  // A3 系统提示组装 + 空字段跳过
  const card = {
    name: 'Seraphina',
    description: '{{char}} 是森林守护者。',
    personality: '温和',
    scenario: '幽暗森林',
    first_mes: '*她抬起头* 你终于来了，{{user}}。',
    mes_example: '<START>\n{{user}}: 你好\n{{char}}: 你好呀。',
    avatar: 'default_Seraphina.png',
  }
  const sys = st.buildSystemPrompt(card, '元宝', true)
  const okSys =
    sys.includes('Seraphina 是森林守护者') &&
    !sys.includes('{{char}}') &&
    sys.includes('### Personality') &&
    sys.includes('### Scenario') &&
    sys.includes('### Example dialogue') &&
    sys.includes('元宝')
  check('A3 系统提示组装（含宏替换/四段块/无残留宏）', okSys)
  report.A.systemPrompt = sys

  const sysLean = st.buildSystemPrompt(card, '元宝', false)
  check('A4 关闭示例时不含 Example dialogue', !sysLean.includes('Example dialogue'))

  // A5 开场白种子 = 元数据头 + 第一条 assistant
  const seed = st.createChatSeed(card, '元宝', 'Seraphina', card.avatar)
  const okSeed =
    seed.length === 2 &&
    !!seed[0].chat_metadata &&
    seed[0].mes === undefined &&
    seed[1].is_user === false &&
    seed[1].mes === card.first_mes
  check('A5 开场白种子（头 + first_mes）', okSeed, `len=${seed.length}`)

  // A6 历史 → messages 的角色映射 + 头部过滤
  const history = [
    seed[0],
    seed[1],
    { name: '元宝', is_user: true, mes: '我来找你了' },
    { name: 'Seraphina', is_user: false, mes: '欢迎。' },
  ]
  const built = st.buildPrompt({ character: card, history, userName: '元宝', maxContext: 8192 })
  const roles = built.messages.map((m) => m.role).join(',')
  const okMap =
    built.messages[0].role === 'system' &&
    roles === 'system,assistant,user,assistant' &&
    built.messages[1].content.includes('你终于来了')
  check('A6 历史角色映射（过滤元数据头）', okMap, roles)
  report.A.roles = roles
  report.A.inputTokens = built.inputTokens

  // A7 上下文裁剪：极小窗口应裁掉旧历史但保留最后一条
  const longHist = [seed[0]]
  for (let i = 0; i < 30; i++) {
    longHist.push({ name: '元宝', is_user: true, mes: `第${i}轮问题：` + '填充'.repeat(120) })
    longHist.push({ name: 'Seraphina', is_user: false, mes: `第${i}轮回答：` + '填充'.repeat(120) })
  }
  longHist.push({ name: '元宝', is_user: true, mes: '最后一句' })
  const tight = st.buildPrompt({
    character: card,
    history: longHist,
    userName: '元宝',
    maxContext: 2048,
  })
  const last = tight.messages[tight.messages.length - 1]
  check(
    'A7 上下文裁剪（丢弃旧历史 + 保留末条 + 计入裁剪数）',
    tight.trimmed > 0 && last.content === '最后一句' && tight.inputTokens <= 2048,
    `trimmed=${tight.trimmed} inputTokens=${tight.inputTokens} keep=${tight.messages.length}条`,
  )
  report.A.trim = { trimmed: tight.trimmed, inputTokens: tight.inputTokens, kept: tight.messages.length }

  return { st, card }
}

/* ================================================================
 * B. ST 侧文件 + 生成链路
 * ================================================================ */
let cookie = ''
let csrf = ''

async function init() {
  const res = await fetch(`${BASE}/csrf-token`)
  cookie = (res.headers.getSetCookie?.() ?? []).map((c) => c.split(';')[0]).join('; ')
  csrf = (await res.json()).token
}

function headers(extra = {}) {
  return {
    'Content-Type': 'application/json',
    'X-CSRF-Token': csrf,
    ...(cookie ? { Cookie: cookie } : {}),
    ...extra,
  }
}

async function post(p, body = {}) {
  const res = await fetch(`${BASE}${p}`, {
    method: 'POST',
    headers: headers(),
    body: JSON.stringify(body),
  })
  const text = await res.text()
  let json = null
  try {
    json = JSON.parse(text)
  } catch {
    /* 非 JSON */
  }
  return { status: res.status, json, text }
}

/** SSE 流式生成，返回 { text, model } */
async function streamGenerate(messages, model, maxTokens = 512) {
  const res = await fetch(`${BASE}/api/backends/chat-completions/generate`, {
    method: 'POST',
    headers: headers(),
    body: JSON.stringify({
      messages,
      model,
      temperature: 0.9,
      max_tokens: maxTokens,
      stream: true,
      chat_completion_source: 'makersuite',
    }),
  })
  if (!res.ok) {
    let d = `HTTP ${res.status}`
    try {
      const j = await res.json()
      d = j?.error?.message ?? j?.message ?? d
    } catch {
      /* ignore */
    }
    throw new Error(d)
  }
  const reader = res.body.getReader()
  const dec = new TextDecoder()
  let buf = ''
  let text = ''
  for (;;) {
    const { done, value } = await reader.read()
    if (done) break
    buf += dec.decode(value, { stream: true })
    for (;;) {
      const m = /\r?\n\r?\n/.exec(buf)
      if (!m) break
      const raw = buf.slice(0, m.index)
      buf = buf.slice(m.index + m[0].length)
      for (const line of raw.split(/\r?\n/)) {
        if (!line.startsWith('data:')) continue
        const p = line.slice(5).trim()
        if (!p || p === '[DONE]') continue
        let chunk
        try {
          chunk = JSON.parse(p)
        } catch {
          continue
        }
        if (chunk.error) throw new Error(chunk.error.message ?? 'stream error')
        let d = chunk.choices?.[0]?.delta?.content ?? ''
        if (!d && chunk.candidates) {
          d = (chunk.candidates[0]?.content?.parts ?? []).map((x) => x.text ?? '').join('')
        }
        if (d) text += d
      }
    }
  }
  return text
}

const RETRY = [2000, 6000]

/** Google 的 429 会在正文里给出建议等待秒数：`Please retry in 18.89s` */
function retryHintMs(msg) {
  const m = /retry in ([\d.]+)s/i.exec(msg)
  if (!m) return 0
  const s = Number(m[1])
  return Number.isFinite(s) && s > 0 ? Math.ceil((s + 2) * 1000) : 0
}

/** 免费层「每日」配额耗尽 —— 等多久都没用，不要再烧请求 */
function isQuotaExhausted(msg) {
  return /exceeded your current quota/i.test(msg)
}

/** 配额耗尽时抛这个，让上层标记为「跳过」而非「失败」 */
class QuotaBlockedError extends Error {
  constructor(msg) {
    super(msg)
    this.name = 'QuotaBlockedError'
    this.hint = retryHintMs(msg)
  }
}

async function generateWithRetry(messages, primary, fallback, maxTokens) {
  const models = fallback && fallback !== primary ? [primary, fallback] : [primary]
  let lastErr
  const tried = []
  for (const model of models) {
    for (let i = 0; i <= RETRY.length; i++) {
      try {
        const text = await streamGenerate(messages, model, maxTokens)
        return { text, model, tried }
      } catch (e) {
        lastErr = e
        const msg = e instanceof Error ? e.message : String(e)
        // 每日配额耗尽：立即停止，不再消耗（等待提示值会越来越大，实测无效）
        if (isQuotaExhausted(msg)) {
          tried.push(`${model}×${i + 1}`)
          throw new QuotaBlockedError(msg)
        }
        const hint = retryHintMs(msg)
        tried.push(`${model}×${i + 1}`)
        log(
          `     · ${model} 第${i + 1}次失败：${msg.slice(0, 90)}${hint ? `（等待 ${hint / 1000}s）` : ''}`,
        )
        if (!/503|UNAVAILABLE|high demand|overloaded|429|rate limit|temporarily/i.test(msg)) throw e
        if (i < RETRY.length) await new Promise((r) => setTimeout(r, Math.max(hint, RETRY[i])))
      }
    }
  }
  throw lastErr
}

async function runHttpE2E(st, card) {
  log('')
  log('===== B. ST 侧文件 + 生成链路（真实 HTTP）=====')
  await init()
  log(`  ST = ${BASE}`)

  // B1 后端连通
  const probe = await post('/api/settings/get')
  check('B1 ST 后端连通 /api/settings/get', probe.status === 200 && !!probe.json)

  // B2 取角色
  const all = await post('/api/characters/all')
  const character = Array.isArray(all.json) ? all.json.find((c) => c.name === 'Seraphina') : null
  check('B2 角色列表可读', !!character, character?.avatar)
  if (!character) throw new Error('找不到 Seraphina，无法继续')

  const userName = (() => {
    try {
      const s = typeof probe.json.settings === 'string' ? JSON.parse(probe.json.settings) : probe.json.settings
      return s?.username ?? 'User'
    } catch {
      return 'User'
    }
  })()

  // B3 建会话并落盘（复用真实引擎的种子构造）
  const fileId = `${character.name} - E2E ${Date.now()}`
  const realCard = {
    ...character,
    // 取列表里的真实卡字段（列表已含 description/first_mes）
  }
  const seed = st.createChatSeed(realCard, userName, character.name, character.avatar)
  // 先探一次：ST 的 /api/chats/get 在角色 chats 目录缺失时会创建它（chats.js:599-605），
  // 否则紧随其后的 /api/chats/save 会因父目录不存在而失败。
  await post('/api/chats/get', { avatar_url: character.avatar, file_name: fileId })
  const save1 = await post('/api/chats/save', {
    avatar_url: character.avatar,
    file_name: fileId,
    chat: seed,
  })
  check('B3 建会话写入 ST (/api/chats/save)', save1.status === 200 && save1.json?.ok === true, JSON.stringify(save1.json))

  // B4 磁盘上真有这个文件
  const dir = path.join(CHAT_DIR, character.avatar.replace('.png', ''))
  const onDisk = existsSync(dir) ? readdirSync(dir).filter((f) => f.endsWith('.jsonl')) : []
  const hit = onDisk.find((f) => f.startsWith(fileId.slice(0, 20)))
  check(
    'B4 磁盘 jsonl 真实存在',
    !!hit,
    hit ? `${path.join(dir, hit).replace(ROOT + path.sep, '')} (${statSync(path.join(dir, hit)).size}B)` : `目录内 ${onDisk.length} 个文件`,
  )
  report.B.file = hit ?? null

  // B5 读回校验
  const got = await post('/api/chats/get', { avatar_url: character.avatar, file_name: fileId })
  const arr = Array.isArray(got.json) ? got.json : []
  check(
    'B5 读回会话（首行元数据头 + 开场白）',
    arr.length === 2 && !!arr[0].chat_metadata && arr[1].mes === character.first_mes,
    `len=${arr.length}`,
  )

  // B6 组装 prompt（角色卡注入）
  const userText = '用一句话描述你自己，并说明你在哪里。'
  const history = [...arr, { name: userName, is_user: true, mes: userText }]
  const built = st.buildPrompt({
    character: realCard,
    history,
    userName,
    maxContext: 8192,
    includeExamples: true,
  })
  check(
    'B6 prompt 组装（system 注入角色卡 + 末尾为本轮输入）',
    built.messages[0].role === 'system' &&
      built.messages[0].content.includes(character.name) &&
      built.messages[built.messages.length - 1].content === userText,
    `messages=${built.messages.length} inputTokens=${built.inputTokens} trimmed=${built.trimmed}`,
  )
  log(`     系统提示前 160 字：${built.systemPrompt.slice(0, 160).replace(/\n/g, ' ⏎ ')}…`)
  report.B.prompt = {
    messages: built.messages.length,
    inputTokens: built.inputTokens,
    trimmed: built.trimmed,
    systemHead: built.systemPrompt.slice(0, 200),
  }

  // B7-B9 真实生成：受上游配额约束，配额耗尽时标记为「跳过」而非失败
  log('  · 真实流式生成中（免费层配额有限，遇 503/限流会退避重试）…')
  const t0 = Date.now()
  let gen
  try {
    gen = await generateWithRetry(built.messages, 'gemini-3.8-flash', 'gemini-flash-latest', 512)
  } catch (e) {
    if (e instanceof QuotaBlockedError) {
      const wait = e.hint ? `，Google 建议等待 ${Math.round(e.hint / 1000)}s` : ''
      log(`  ⏭ B7 跳过：上游免费层配额耗尽（非代码问题）${wait}`)
      log(`     请求已真实到达 Google 并返回配额错误 → ST 通路 + 角色卡 prompt 送达链路成立`)
      report.B.skipped = { reason: 'quota-exhausted', message: e.message.slice(0, 300) }
      report.B.promptDelivered = true
      return { fileId, skipped: true }
    }
    throw e
  }
  const ms = Date.now() - t0
  check(
    'B7 真实流式生成返回内容',
    gen.text.trim().length > 0,
    `模型=${gen.model} 全文 ${ms}ms 长度=${gen.text.length}`,
  )
  log(`     回复：${gen.text.trim().slice(0, 200).replace(/\n/g, ' ⏎ ')}`)
  report.B.generate = { model: gen.model, ms, chars: gen.text.length, text: gen.text }

  // B8 回复落盘 + 读回
  const finalDoc = [
    ...arr,
    { name: userName, is_user: true, send_date: new Date().toISOString(), mes: userText, extra: {} },
    { name: character.name, is_user: false, send_date: new Date().toISOString(), mes: gen.text, extra: {} },
  ]
  const save2 = await post('/api/chats/save', {
    avatar_url: character.avatar,
    file_name: fileId,
    chat: finalDoc,
  })
  const got2 = await post('/api/chats/get', { avatar_url: character.avatar, file_name: fileId })
  const arr2 = Array.isArray(got2.json) ? got2.json : []
  check(
    'B8 回复落盘并读回（4 行：头+开场白+用户+回复）',
    save2.json?.ok === true && arr2.length === 4 && arr2[3].mes === gen.text,
    `len=${arr2.length}`,
  )

  // B9 角色卡是否真的影响了输出（弱断言：提到名字或设定关键词）
  const influenced =
    gen.text.toLowerCase().includes(character.name.toLowerCase()) ||
    /forest|guardian|seraphina|森林|守护/i.test(gen.text)
  check('B9 角色卡对输出产生可见影响（含角色名/设定关键词）', influenced)

  report.B.chat = { fileId, avatar: character.avatar, messages: arr2.length }
  return { fileId, skipped: false }
}

/* ================================================================ */
let skipped = null
try {
  const { st, card } = await runPromptSelfTest()
  skipped = await runHttpE2E(st, card)
} catch (e) {
  failures++
  log('')
  log(`💥 致命错误：${e instanceof Error ? e.message : String(e)}`)
  if (e instanceof Error && e.stack) log(e.stack.split('\n').slice(0, 6).join('\n'))
}

log('')
if (skipped?.skipped) {
  log('📌 B7-B9 因上游免费层配额耗尽未执行（非代码问题）—— 配额恢复后重跑本脚本即可补齐')
}
log(failures === 0 ? '🎉 已执行项全部通过' : `⚠️ 存在 ${failures} 项失败`)
report.failures = failures
report.skipped = skipped?.skipped ? 'quota-exhausted' : null

writeFileSync(path.join(LOGS, 'e2e-m3.txt'), lines.join('\n'), 'utf8')
writeFileSync(path.join(LOGS, 'e2e-m3.json'), JSON.stringify(report, null, 2), 'utf8')
console.log(`\n报告：logs/e2e-m3.txt`)
process.exit(failures === 0 ? 0 : 1)
