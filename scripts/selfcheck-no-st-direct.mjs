#!/usr/bin/env node
/**
 * R18 门禁①：无原 ST 直连（ST 默认端口 / 主机字面禁则）
 *
 * 扫描面：src 全树（*.ts / *.vue）与 src-tauri/src 全树（*.rs）
 * 禁则（命中即红）：
 *   a) ST 默认端口字面 `:8000`（含 127.0.0.1:8000 / localhost:8000 / 裸 :8000）
 *   b) 以 sillytavern 为**主机名**的 http(s) URL（含子域 sillytavern.* / *.sillytavern）
 * 不误伤（显式边界，见 docs/agents/regression-gates.md）：
 *   - 本机随机端口的 127.0.0.1 / localhost（如 http://127.0.0.1:1420、:5001）
 *   - provider 官方域名（api.openai.com、github.com/.../SillyTavern-OpenChat 等，非 ST 主机）
 *   - 纯注释里的 "ST" 字样（无 URL、无 :8000 即不命中）
 *   - 数据面 fixture `port: 8000`（冒号后有空格，不是 host:port 字面；非直连语义）
 *
 * 用法：
 *   node scripts/selfcheck-no-st-direct.mjs
 *   node scripts/selfcheck-no-st-direct.mjs --extra-file <path>   # 负向用例注入（R18 证据）
 *
 * 退出码：命中任一禁则 = 1。
 */
import { readFileSync, readdirSync, statSync, existsSync } from 'node:fs'
import { join, resolve, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'

/* run-selfchecks 会 esbuild bundle 后执行（import.meta.url 不可靠）——仓库根优先取 cwd */
function repoRoot() {
  const cwd = process.cwd()
  if (existsSync(join(cwd, 'src-tauri', 'src'))) return cwd
  const up = resolve(dirname(fileURLToPath(import.meta.url)), '..')
  if (existsSync(join(up, 'src-tauri', 'src'))) return up
  return cwd
}
const root = repoRoot()

/** ST 默认端口字面：`:8000`（不吞 :80000 这类更长数字） */
const ST_PORT_RE = /:8000(?![0-9])/

/** 从一行里找出以 sillytavern 为 host 的 http(s) URL（路径段里出现 SillyTavern 不算，如 github.com/...） */
function stHostUrl(line) {
  for (const m of line.matchAll(/https?:\/\/[^\s"'\`<>()\[\]{}]+/gi)) {
    let authority = m[0].replace(/^https?:\/\//i, '').split(/[/?#]/)[0]
    authority = authority.split('@').pop() ?? authority
    const host = authority.split(':')[0].toLowerCase()
    if (host === 'sillytavern' || host.startsWith('sillytavern.') || host.endsWith('.sillytavern')) {
      return m[0]
    }
  }
  return null
}

function* walk(dir, depth = 0) {
  if (depth > 8) return
  let names = []
  try {
    names = readdirSync(dir)
  } catch {
    return
  }
  for (const n of names) {
    const p = join(dir, n)
    let st
    try {
      st = statSync(p)
    } catch {
      continue
    }
    if (st.isDirectory()) yield* walk(p, depth + 1)
    else yield p
  }
}

const scanSets = [
  { base: join(root, 'src'), exts: ['.ts', '.vue'] },
  { base: join(root, 'src-tauri', 'src'), exts: ['.rs'] },
]
const targets = []
for (const s of scanSets) {
  for (const p of walk(s.base)) {
    if (s.exts.some((e) => p.endsWith(e))) targets.push(p)
  }
}

/* 负向用例注入：--extra-file <path>（扫描面之外的样本文件） */
const argv = process.argv.slice(2)
const extra = []
for (let i = 0; i < argv.length; i++) {
  if (argv[i] === '--extra-file' && argv[i + 1]) extra.push(resolve(argv[++i]))
}

const rel = (p) => (p.startsWith(root) ? p.slice(root.length + 1) : p).replace(/\\/g, '/')
let fail = 0
const bad = (p, lineNo, kind, sample) => {
  fail++
  console.error(`✗ ${rel(p)}:${lineNo}: ${kind} → ${sample.trim().slice(0, 160)}`)
}

for (const p of [...targets, ...extra]) {
  let text
  try {
    text = readFileSync(p, 'utf-8')
  } catch (e) {
    fail++
    console.error(`✗ 无法读取文件 ${p}: ${e instanceof Error ? e.message : e}`)
    continue
  }
  text.split(/\r?\n/).forEach((line, idx) => {
    const pm = ST_PORT_RE.exec(line)
    if (pm) bad(p, idx + 1, 'ST 默认端口字面 ":8000"', line)
    const u = stHostUrl(line)
    if (u) bad(p, idx + 1, 'sillytavern 主机 URL', u)
  })
}

if (fail) {
  console.error(`\n${fail} 处 ST 直连字面（R18 门禁①）`)
  process.exit(1)
}
console.log(`✅ no-st-direct：扫描 ${targets.length} 文件（src + src-tauri/src）+ 注入 ${extra.length}，无 ST 默认端口/主机字面`)
process.exit(0)
