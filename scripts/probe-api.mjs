#!/usr/bin/env node
/**
 * ST API 字段探针（M3 前置）
 *
 * 目的：直接打真实接口，拿到端点响应结构与字段名，
 *      避免按记忆猜字段（CSRF 双提交 + Cookie 必须手动维护，node 无 cookie jar）。
 *
 * 用法：
 *   node app/scripts/probe-api.mjs               # 探默认端点
 *   node app/scripts/probe-api.mjs --raw         # 额外打印完整 JSON（截断）
 *
 * 输出：
 *   logs/api-probe.json   结构化结果
 *   logs/api-probe.txt    人读摘要
 */
import { writeFileSync, mkdirSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const __dirname = path.dirname(fileURLToPath(import.meta.url))
const ROOT = path.resolve(__dirname, '..', '..')
const LOGS = path.join(ROOT, 'logs')
mkdirSync(LOGS, { recursive: true })

const BASE = process.env.ST_BASE ?? 'http://127.0.0.1:8000'
const RAW = process.argv.includes('--raw')

let cookie = ''
let csrf = ''

async function init() {
  const res = await fetch(`${BASE}/csrf-token`)
  const setCookies = res.headers.getSetCookie?.() ?? []
  cookie = setCookies.map((c) => c.split(';')[0]).join('; ')
  const data = await res.json()
  csrf = data.token
}

async function post(p, body = {}) {
  const res = await fetch(`${BASE}${p}`, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      'X-CSRF-Token': csrf,
      ...(cookie ? { Cookie: cookie } : {}),
    },
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

/** 取对象数组首项的字段清单（含类型），用于确认字段名 */
function shape(v, depth = 0, maxDepth = 2) {
  if (v === null) return 'null'
  if (Array.isArray(v)) {
    if (v.length === 0) return 'array(0)'
    return depth >= maxDepth
      ? `array(${v.length}) of ?`
      : `array(${v.length}) of ${shape(v[0], depth + 1, maxDepth)}`
  }
  if (typeof v === 'object') {
    if (depth >= maxDepth) return 'object{...}'
    const keys = Object.keys(v)
    if (keys.length === 0) return 'object{}'
    return (
      '{\n' +
      keys
        .map((k) => {
          let val = v[k]
          if (typeof val === 'string' && val.length > 80) val = val.slice(0, 80) + `…(len ${v[k].length})`
          return `    ${k}: ${JSON.stringify(val)}`
        })
        .join('\n') +
      '\n  }'
    )
  }
  return typeof v
}

const out = { base: BASE, at: new Date().toISOString(), probes: {} }
const lines = []

function record(name, req, res, note = '') {
  out.probes[name] = { request: req, status: res.status, note, data: res.json }
  lines.push(`\n${'='.repeat(70)}`)
  lines.push(`### ${name}   [HTTP ${res.status}]`)
  if (note) lines.push(`# ${note}`)
  lines.push(`REQ  ${JSON.stringify(req)}`)
  if (res.json === null) {
    lines.push(`RESP (非 JSON) ${res.text.slice(0, 300)}`)
  } else {
    lines.push(`SHAPE ${shape(res.json)}`)
    if (RAW) {
      const s = JSON.stringify(res.json)
      lines.push(`RAW  ${s.length > 3000 ? s.slice(0, 3000) + '…' : s}`)
    }
  }
  console.log(lines.slice(-6).join('\n'))
}

await init()
lines.push(`ST base: ${BASE}`)
lines.push(`csrf: ${csrf ? csrf.slice(0, 16) + '…' : '(none)'}`)
lines.push(`cookie: ${cookie ? cookie.slice(0, 40) + '…' : '(none)'}`)

// 1) 角色列表
{
  const req = {}
  const res = await post('/api/characters/all', req)
  record('POST /api/characters/all', req, res, '角色卡（= 联系人）列表')
}

// 2) 全局设置（确认生成通道与模型）
{
  const req = {}
  const res = await post('/api/settings/get', req)
  const s = res.json?.settings
  let parsed = null
  try {
    parsed = typeof s === 'string' ? JSON.parse(s) : s
  } catch {
    /* ignore */
  }
  const pick = parsed
    ? {
        main_api: parsed.main_api,
        chat_completion_source: parsed.chat_completion_source,
        preset_settings: parsed.preset_settings,
        username: parsed.username,
        firstRun: parsed.firstRun,
        amount_gen: parsed.amount_gen,
        max_context: parsed.max_context,
        api_server: parsed.api_server,
      }
    : null
  out.probes['POST /api/settings/get'] = { request: req, status: res.status, note: '关键设置字段', data: pick }
  lines.push(`\n${'='.repeat(70)}`)
  lines.push(`### POST /api/settings/get   [HTTP ${res.status}]`)
  lines.push(`KEY FIELDS ${JSON.stringify(pick, null, 2)}`)
}

// 3) 最近会话（跨角色，= 微信会话列表）
{
  const req = { max: 10, metadata: true }
  const res = await post('/api/chats/recent', req)
  record('POST /api/chats/recent', req, res, '跨角色最近会话（会话列表数据源）')
}

// 4) 选中角色的会话列表
{
  const all = out.probes['POST /api/characters/all']?.data
  const first = Array.isArray(all) ? all[0] : null
  if (first?.avatar) {
    const req = { avatar_url: first.avatar }
    const res = await post('/api/characters/chats', req)
    record(`POST /api/characters/chats  (${first.name})`, req, res, '该角色下的会话文件列表')
  } else {
    lines.push('\n[skip] /api/characters/chats —— 无角色可测')
  }
}

writeFileSync(path.join(LOGS, 'api-probe.json'), JSON.stringify(out, null, 2), 'utf8')
writeFileSync(path.join(LOGS, 'api-probe.txt'), lines.join('\n'), 'utf8')
console.log(`\n写出：logs/api-probe.json / logs/api-probe.txt`)
