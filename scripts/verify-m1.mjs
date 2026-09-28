#!/usr/bin/env node
/**
 * M1 配置映射验证
 *
 * 1) 用 esbuild 把 `mapping-core.ts` 打包后跑**真代码**（不是复刻）的纯逻辑断言
 * 2) 打真实 ST，验证「读全量 → 合并 → 写全量 → 读回校验」的端到端行为
 *    —— 核心断言：**写入不会抹掉用户的其它设置键**（ST 的 save 是整体覆盖，这是最大风险）
 * 3) 测完把 ST 设置**恢复原状**
 *
 * 用法：node scripts/verify-m1.mjs [baseUrl]    默认 http://127.0.0.1:8000
 */
import * as esbuild from 'esbuild'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const HERE = path.dirname(fileURLToPath(import.meta.url))
const APP = path.resolve(HERE, '..')
const BASE = process.argv[2] ?? process.env.ST_BASE ?? 'http://127.0.0.1:8000'

let pass = 0
let fail = 0
function ok(name, cond, detail = '') {
  const tail = detail ? `  — ${detail}` : ''
  if (cond) {
    pass++
    console.log(`  ✅ ${name}${tail}`)
  } else {
    fail++
    console.log(`  ❌ ${name}${tail}`)
  }
}

/* ---------------- 用真代码：打包 mapping-core.ts ---------------- */

console.log('=== 0) 载入待测真代码（esbuild 打包 mapping-core.ts）===')
const built = await esbuild.build({
  entryPoints: [path.join(APP, 'src/services/st/mapping-core.ts')],
  bundle: true,
  format: 'esm',
  write: false,
  logLevel: 'silent',
})
const code = built.outputFiles[0].text
const core = await import('data:text/javascript;base64,' + Buffer.from(code).toString('base64'))
const { buildEntries, getPath, setPath, diffMapping, mergeInto } = core
ok('模块载入', typeof mergeInto === 'function', `导出 ${Object.keys(core).length} 项`)

const CFG = {
  source: 'makersuite',
  model: 'gemini-3-pro',
  temperature: 0.9,
  maxTokens: 2048,
  maxContext: 8192,
  topP: 0.95,
  topK: 40,
  topA: 0,
  minP: 0.05,
  frequencyPenalty: 0.1,
  presencePenalty: 0.2,
  repetitionPenalty: 1.05,
}

/* ---------------- 纯逻辑断言 ---------------- */

console.log('\n=== 1) 纯逻辑（映射表 / 嵌套读写 / 合并）===')
const entries = buildEntries(CFG)
ok(
  '映射表条目数（model+source+温度+7采样+上限x2+上下文x2）',
  entries.length === 14,
  `${entries.length} 条: ${entries.map((e) => e.stPath).join(', ')}`,
)
ok(
  '模型按通道写入正确键（makersuite → google_model）',
  entries.some((e) => e.stPath === 'oai_settings.google_model' && e.appValue === 'gemini-3-pro'),
)

const nested = { a: 1 }
setPath(nested, 'x.y.z', 'v')
ok('setPath 逐层创建中间对象', getPath(nested, 'x.y.z') === 'v', JSON.stringify(nested))

const broken = { x: 'not-an-object' }
setPath(broken, 'x.y.z', 1)
ok('setPath 覆盖非对象中间层', getPath(broken, 'x.y.z') === 1, JSON.stringify(broken))

const snapshot = {
  username: 'User',
  max_context: 4096,
  amount_gen: 100,
  oai_settings: {
    temp_openai: 1,
    top_p_openai: 1,
    top_k_openai: 0,
    top_a_openai: 0.5,
    min_p_openai: 0,
    freq_pen_openai: 0,
    pres_pen_openai: 0,
    repetition_penalty_openai: 1,
    openai_max_tokens: 300,
    openai_max_context: 4095,
    openai_model: 'gpt-4-turbo',
    google_model: 'gemini-2.5-flash',
    chat_completion_source: 'openai',
  },
  power_user: { tokenizer: 99, personas: {} },
  world_info_settings: { world_info_depth: 2 },
  swipes: true,
}
const snapshotBefore = JSON.stringify(snapshot)
const merged = mergeInto(snapshot, CFG)

ok('mergeInto 不修改入参（无副作用）', JSON.stringify(snapshot) === snapshotBefore)
ok('合并后写入温度', getPath(merged, 'oai_settings.temp_openai') === 0.9)
ok('合并后写入模型（google_model）', getPath(merged, 'oai_settings.google_model') === 'gemini-3-pro')
ok('合并后写入通道', getPath(merged, 'oai_settings.chat_completion_source') === 'makersuite')
ok('合并后写入 top_p', getPath(merged, 'oai_settings.top_p_openai') === 0.95)
ok('合并后写入 openai_max_tokens', getPath(merged, 'oai_settings.openai_max_tokens') === 2048)
ok('合并后写入 openai_max_context', getPath(merged, 'oai_settings.openai_max_context') === 8192)
ok('合并后写入回复上限', merged.amount_gen === 2048)
ok('合并后写入上下文窗口', merged.max_context === 8192)
ok(
  '合并后**保留**未映射的键（防抹除）',
  merged.username === 'User' &&
    getPath(merged, 'oai_settings.openai_model') === 'gpt-4-turbo' &&
    getPath(merged, 'power_user.tokenizer') === 99 &&
    getPath(merged, 'world_info_settings.world_info_depth') === 2 &&
    merged.swipes === true,
)
ok(
  '合并后顶层键数不变',
  Object.keys(merged).length === Object.keys(snapshot).length,
  `${Object.keys(snapshot).length} → ${Object.keys(merged).length}`,
)
ok(
  '合并后嵌套对象键数不变',
  Object.keys(merged.oai_settings).length === Object.keys(snapshot.oai_settings).length,
)

const diffs = diffMapping(snapshot, CFG)
ok('diffMapping 正确识别不一致', diffs.every((d) => !d.inSync), diffs.map((d) => `${d.app}=${d.stValue}`).join(', '))
ok('diffMapping 对已一致的值判为一致', diffMapping(merged, CFG).every((d) => d.inSync))

/* ---------------- 真机端到端 ---------------- */

console.log(`\n=== 2) 真机端到端（${BASE}）===`)

let cookie = ''
let csrf = ''
async function req(p, body, method = 'POST') {
  const headers = { 'Content-Type': 'application/json' }
  if (csrf) headers['X-CSRF-Token'] = csrf
  if (cookie) headers['Cookie'] = cookie
  const r = await fetch(BASE + p, {
    method,
    headers,
    body: method === 'GET' ? undefined : JSON.stringify(body ?? {}),
  })
  const sc = r.headers.getSetCookie?.() ?? []
  if (sc.length) cookie = sc.map((s) => s.split(';')[0]).join('; ')
  const text = await r.text()
  let json = null
  try {
    json = JSON.parse(text)
  } catch {
    /* 上游可能返回非 JSON（如 429 结构化错误） */
  }
  return { status: r.status, json, text }
}

async function readSettings() {
  const r = await req('/api/settings/get', {})
  if (r.status !== 200 || !r.json?.settings) throw new Error(`settings/get 失败 HTTP ${r.status}`)
  const raw = r.json.settings
  return typeof raw === 'string' ? JSON.parse(raw) : raw
}

async function writeSettings(obj) {
  return req('/api/settings/save', obj)
}

const csrfRes = await req('/csrf-token', null, 'GET')
csrf = csrfRes.json?.token ?? ''
ok('取到 CSRF token', !!csrf, csrf ? `token=${csrf.slice(0, 12)}…` : `空（HTTP ${csrfRes.status}）`)

let original = null
try {
  original = await readSettings()
  ok('读取真实 settings', !!original && typeof original === 'object', `${Object.keys(original ?? {}).length} 个顶层键`)
} catch (e) {
  console.log(`  ❌ 读取失败：${e.message}`)
  fail++
}

if (original) {
  const originalJson = JSON.stringify(original)
  const topKeys = Object.keys(original).length

  // --- 真写：用真代码合并，再整份写回 ---
  const toWrite = mergeInto(original, CFG)
  const w = await writeSettings(toWrite)
  ok('写入 ST 成功', w.status === 200, `HTTP ${w.status}`)

  const after = await readSettings()
  ok('读回后温度已生效', getPath(after, 'oai_settings.temp_openai') === 0.9, `= ${getPath(after, 'oai_settings.temp_openai')}`)
  ok('读回后回复上限已生效', after.amount_gen === 2048, `= ${after.amount_gen}`)
  ok('读回后上下文已生效', after.max_context === 8192, `= ${after.max_context}`)
  ok('读回后模型已生效（google_model）', getPath(after, 'oai_settings.google_model') === 'gemini-3-pro', `= ${getPath(after, 'oai_settings.google_model')}`)
  ok('读回后通道已生效', getPath(after, 'oai_settings.chat_completion_source') === 'makersuite', `= ${getPath(after, 'oai_settings.chat_completion_source')}`)

  // 🔴 最关键的断言：其它键一个都不能少
  ok(
    '写入未抹掉任何顶层键',
    Object.keys(after).length === topKeys,
    `${topKeys} → ${Object.keys(after).length}`,
  )
  const lostTop = Object.keys(original).filter((k) => !(k in after))
  ok('无顶层键丢失', lostTop.length === 0, lostTop.join(', ') || '无')

  const lostNested = []
  for (const k of Object.keys(original)) {
    const a = original[k]
    const b = after[k]
    if (a && typeof a === 'object' && !Array.isArray(a) && b && typeof b === 'object') {
      for (const kk of Object.keys(a)) if (!(kk in b)) lostNested.push(`${k}.${kk}`)
    }
  }
  ok('无嵌套键丢失（power_user / oai_settings 等）', lostNested.length === 0, lostNested.slice(0, 6).join(', ') || '无')

  ok(
    '未映射项的值原样保留',
    after.username === original.username && getPath(after, 'power_user.tokenizer') === getPath(original, 'power_user.tokenizer'),
    `username=${after.username}, tokenizer=${getPath(after, 'power_user.tokenizer')}`,
  )

  // --- 恢复原状 ---
  const rw = await writeSettings(original)
  const restored = await readSettings()
  ok('已恢复原设置', rw.status === 200)
  ok(
    '恢复后与原始快照逐字节等价',
    JSON.stringify(sortKeys(restored)) === JSON.stringify(sortKeys(JSON.parse(originalJson))),
    `${Object.keys(restored).length} 个顶层键`,
  )

  console.log(`\n  说明：本次测试真写过 ST 的 settings.json，已还原（${topKeys} 个顶层键）`)
}

function sortKeys(o) {
  if (Array.isArray(o)) return o.map(sortKeys)
  if (o && typeof o === 'object') {
    return Object.keys(o)
      .sort()
      .reduce((acc, k) => {
        acc[k] = sortKeys(o[k])
        return acc
      }, {})
  }
  return o
}

console.log(`\n${fail === 0 ? '🎉 全部通过' : `⚠️ ${fail} 项失败`}（通过 ${pass}）`)
process.exit(fail === 0 ? 0 : 1)
