#!/usr/bin/env node
/**
 * S4 下载源可用性探测
 *
 * 用途：S4 下载器要按「官方优先 → 镜像兜底」排源，但**实际能不能下到**必须实测，
 * 不能按印象写死。本脚本对每个候选源测三件事：
 *   1. 可达性（HTTP 状态）
 *   2. 体积（content-length，用于 UI 预估与磁盘预检）
 *   3. **是否支持 Range 请求** —— 这决定能不能做断点续传
 *
 * 用法：node scripts/probe-sources.mjs [--proxy http://127.0.0.1:7890]
 *
 * 注：node 原生 fetch 不读 HTTP_PROXY，带代理时走 curl（脚本内部切换）。
 */
import { execFileSync } from 'node:child_process'

const argv = process.argv.slice(2)
const proxyIdx = argv.indexOf('--proxy')
const PROXY = proxyIdx >= 0 ? argv[proxyIdx + 1] : ''

const NODE_VER = '22.22.2'
const ST_VER = '1.19.0'

const TARGETS = [
  {
    id: 'node-official',
    group: 'Node 运行时',
    note: '官方源，最权威',
    url: `https://nodejs.org/dist/v${NODE_VER}/node-v${NODE_VER}-win-x64.zip`,
  },
  {
    id: 'node-npmmirror',
    group: 'Node 运行时',
    note: '国内镜像（阿里），兜底',
    url: `https://cdn.npmmirror.com/binaries/node/v${NODE_VER}/node-v${NODE_VER}-win-x64.zip`,
  },
  {
    id: 'st-zipball',
    group: 'SillyTavern 源码',
    note: 'GitHub 源码快照（不含 node_modules，装完需 npm install）',
    url: `https://api.github.com/repos/SillyTavern/SillyTavern/zipball/${ST_VER}`,
  },
  {
    id: 'st-ghproxy',
    group: 'SillyTavern 源码',
    note: 'GitHub 加速镜像（兜底）',
    url: `https://gh-proxy.com/https://api.github.com/repos/SillyTavern/SillyTavern/zipball/${ST_VER}`,
  },
  {
    id: 'npm-npmmirror',
    group: 'npm registry',
    note: 'ST 依赖安装用（npm install 的源）',
    url: 'https://registry.npmmirror.com/express',
  },
  {
    id: 'npm-official',
    group: 'npm registry',
    note: '官方 registry',
    url: 'https://registry.npmjs.org/express',
  },
]

function proxyArgs() {
  return PROXY ? ['-x', PROXY] : ['--noproxy', '*']
}

/** HEAD 请求：header 由 -I 直接输出到 stdout */
function head(url) {
  const args = ['-s', '--max-time', '30', '-I', ...proxyArgs(), url]
  try {
    return execFileSync('curl', args, { encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] })
  } catch (e) {
    return e.stdout?.toString() ?? ''
  }
}

/** 带自定义头的 GET（只取 body 到 /dev/null，header 打到 stdout） */
function headWith(url, headers = []) {
  const args = ['-s', '--max-time', '30', '-D', '-', '-o', '/dev/null', ...headers, ...proxyArgs(), url]
  try {
    return execFileSync('curl', args, { encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] })
  } catch (e) {
    return e.stdout?.toString() ?? ''
  }
}

function parseHeaders(raw) {
  const blocks = raw.split(/\r?\n\r?\n/).filter(Boolean)
  const last = blocks[blocks.length - 1] ?? ''
  const lines = last.split(/\r?\n/)
  const status = Number((lines[0]?.match(/HTTP\/[\d.]+ (\d+)/) ?? [])[1] ?? 0)
  const get = (name) => {
    const l = lines.find((x) => x.toLowerCase().startsWith(name + ':'))
    return l ? l.slice(name.length + 1).trim() : ''
  }
  return { status, len: Number(get('content-length') || 0), acceptRanges: get('accept-ranges') }
}

console.log(`=== S4 下载源探测${PROXY ? `（经代理 ${PROXY}）` : '（直连）'} ===\n`)

let group = ''
const rows = []
for (const t of TARGETS) {
  if (t.group !== group) {
    group = t.group
    console.log(`【${group}】`)
  }

  // 1) 基础可达性
  const base = parseHeaders(head(t.url))

  // 2) Range 支持（断点续传的前提）：请求前 1024 字节
  const ranged = parseHeaders(headWith(t.url, ['-H', 'Range: bytes=0-1023']))
  const rangeOk = ranged.status === 206

  const sizeMB = base.len > 0 ? (base.len / 1048576).toFixed(1) + ' MB' : '—'
  const mark = base.status === 200 || base.status === 302 ? '✅' : '❌'

  console.log(`  ${mark} ${t.id.padEnd(18)} HTTP ${String(base.status).padEnd(4)} ${sizeMB.padEnd(10)} Range:${rangeOk ? '支持' : '不支持'}  — ${t.note}`)
  rows.push({ ...t, status: base.status, len: base.len, rangeOk })
}

console.log('\n=== 结论与建议 ===')
const nodeOk = rows.filter((r) => r.group === 'Node 运行时' && r.status === 200)
const stOk = rows.filter((r) => r.group === 'SillyTavern 源码' && (r.status === 200 || r.status === 302))
const npmOk = rows.filter((r) => r.group === 'npm registry' && r.status === 200)

console.log(`  Node 源可用 ${nodeOk.length} 个：${nodeOk.map((r) => r.id).join(', ') || '无'}`)
console.log(`  ST   源可用 ${stOk.length} 个：${stOk.map((r) => r.id).join(', ') || '无'}`)
console.log(`  npm  源可用 ${npmOk.length} 个：${npmOk.map((r) => r.id).join(', ') || '无'}`)

const anyRange = rows.some((r) => r.rangeOk)
console.log(`  断点续传：${anyRange ? '部分源支持 Range，下载器应实现续传' : '⚠️ 所有源都不支持 Range —— 只能整包重下'}`)

console.log('\n  ⚠ 重要：ST 的 release 无 zip 资产（assets=0），')
console.log('    因此「安装 ST」必须走 源码快照 + npm install，npm registry 的可达性是硬前提。')
