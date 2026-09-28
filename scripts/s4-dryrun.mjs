#!/usr/bin/env node
/**
 * S4 安装流程 dry-run
 *
 * 验证「从零装 ST」的完整链路是否真的走得通 —— 这是 S4 能否落地的前提。
 *
 * 背景（已实测）：ST 的 GitHub release **没有任何 zip 资产**（assets=0），
 * 所以「安装 ST」只能走 **源码快照 + npm install**，而源码包不含 node_modules。
 *
 * 本脚本只做前半段：多源下载 → 解压 → 结构校验。
 * npm install 单独验证（耗时数分钟，见 --with-npm）。
 *
 * 用法：node scripts/s4-dryrun.mjs [版本] [--with-npm]
 */
import { execFileSync } from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

const args = process.argv.slice(2)
const ST_VER = args.find((a) => !a.startsWith('--')) ?? '1.19.0'
const WITH_NPM = args.includes('--with-npm')

const WORK = path.join(os.tmpdir(), 's4-dryrun')
const ZIP = path.join(WORK, `SillyTavern-${ST_VER}.zip`)

/** 候选源：镜像优先（国内直连 GitHub 很慢，实测 codeload 会超时） */
const SOURCES = [
  {
    id: 'gh-proxy',
    url: `https://gh-proxy.com/https://github.com/SillyTavern/SillyTavern/archive/refs/tags/${ST_VER}.zip`,
  },
  {
    id: 'ghfast',
    url: `https://ghfast.top/https://github.com/SillyTavern/SillyTavern/archive/refs/tags/${ST_VER}.zip`,
  },
  {
    id: 'github-direct',
    url: `https://codeload.github.com/SillyTavern/SillyTavern/zip/refs/tags/${ST_VER}`,
  },
]

const MB = 1048576
const fmt = (b) => (b / MB).toFixed(1) + ' MB'

console.log(`=== S4 安装 dry-run（ST ${ST_VER}）===`)
console.log(`工作目录：${WORK}\n`)

fs.mkdirSync(WORK, { recursive: true })

/* ---------------- 1) 多源下载 ---------------- */

console.log('【1】下载源码快照（按顺序试源，60s 超时）')
let picked = null
for (const s of SOURCES) {
  const t0 = Date.now()
  try {
    const ctrl = new AbortController()
    const timer = setTimeout(() => ctrl.abort(), 60000)
    const r = await fetch(s.url, { signal: ctrl.signal })
    clearTimeout(timer)
    if (!r.ok) {
      console.log(`  ✗ ${s.id.padEnd(14)} HTTP ${r.status}`)
      continue
    }
    const buf = Buffer.from(await r.arrayBuffer())
    const secs = (Date.now() - t0) / 1000
    // ZIP 魔数校验：防止把 HTML 错误页当成包
    const isZip = buf[0] === 0x50 && buf[1] === 0x4b
    if (!isZip) {
      console.log(`  ✗ ${s.id.padEnd(14)} 不是 ZIP（首字节 ${buf.subarray(0, 2).toString('hex')}）`)
      continue
    }
    fs.writeFileSync(ZIP, buf)
    console.log(
      `  ✅ ${s.id.padEnd(14)} ${fmt(buf.length).padEnd(10)} ${secs.toFixed(1)}s  ${(buf.length / MB / secs).toFixed(2)} MB/s`,
    )
    picked = s
    break
  } catch (e) {
    const why = e.name === 'AbortError' ? '超时 60s' : e.message.slice(0, 60)
    console.log(`  ✗ ${s.id.padEnd(14)} ${why}`)
  }
}

if (!picked) {
  console.log('\n❌ 所有源都失败 —— S4 无法落地，需要先解决下载通道')
  process.exit(1)
}

/* ---------------- 2) 解压 ---------------- */

console.log('\n【2】解压（Windows 自带 bsdtar 支持 zip）')
const TAR = 'C:\\Windows\\System32\\tar.exe'
const extractDir = path.join(WORK, 'extract')
fs.rmSync(extractDir, { recursive: true, force: true })
fs.mkdirSync(extractDir, { recursive: true })
try {
  execFileSync(TAR, ['-xf', ZIP, '-C', extractDir], { stdio: 'pipe' })
  console.log('  ✅ 解压完成')
} catch (e) {
  console.log('  ❌ 解压失败：', (e.stderr?.toString() ?? e.message).slice(0, 200))
  process.exit(1)
}

/* ---------------- 3) 结构校验 ---------------- */

const top = fs.readdirSync(extractDir)
const root = top.length === 1 && fs.statSync(path.join(extractDir, top[0])).isDirectory()
  ? path.join(extractDir, top[0])
  : extractDir
console.log(`\n【3】结构校验（顶层目录 ${path.basename(root)}）`)

const must = ['server.js', 'package.json', 'src/command-line.js', 'public/index.html', 'default/config.yaml']
let okCount = 0
for (const f of must) {
  const exists = fs.existsSync(path.join(root, f))
  if (exists) okCount++
  console.log(`  ${exists ? '✅' : '❌'} ${f}`)
}

function countFiles(dir) {
  let n = 0
  const walk = (d) => {
    for (const e of fs.readdirSync(d, { withFileTypes: true })) {
      if (e.isDirectory()) walk(path.join(d, e.name))
      else n++
    }
  }
  walk(dir)
  return n
}
const total = countFiles(root)
console.log(`  文件总数：${total}`)
console.log(`  node_modules：${fs.existsSync(path.join(root, 'node_modules')) ? '存在' : '不存在（符合预期，需 npm install）'}`)

const pkg = JSON.parse(fs.readFileSync(path.join(root, 'package.json'), 'utf8'))
console.log(`  上游版本：${pkg.version}　engines.node：${pkg.engines?.node ?? '未声明'}`)
console.log(`  生产依赖数：${Object.keys(pkg.dependencies ?? {}).length}`)

console.log(
  `\n${okCount === must.length ? '🎉 下载与解压链路可用' : '⚠️ 关键文件缺失，需检查'}`,
)

/* ---------------- 4) 可选：npm install ---------------- */

if (WITH_NPM) {
  console.log('\n【4】npm install --omit=dev（耗时较长，用于验证依赖可装性）')
  const t0 = Date.now()
  try {
    execFileSync(
      process.platform === 'win32' ? 'npm.cmd' : 'npm',
      ['install', '--omit=dev', '--no-audit', '--no-fund'],
      { cwd: root, stdio: 'inherit' },
    )
    const secs = ((Date.now() - t0) / 1000).toFixed(0)
    const nm = path.join(root, 'node_modules')
    const deps = fs.existsSync(nm) ? fs.readdirSync(nm).length : 0
    console.log(`  ✅ 依赖安装完成：${secs}s，node_modules 顶层 ${deps} 项`)
  } catch (e) {
    console.log('  ❌ npm install 失败：', (e.message ?? '').slice(0, 300))
    process.exit(1)
  }
} else {
  console.log('\n（未加 --with-npm，跳过依赖安装）')
}

console.log(`\n产物目录：${root}`)
