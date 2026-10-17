#!/usr/bin/env node
/**
 * T23 干净机器验收环境矩阵（脚本化断言 · 验收 D6）
 *
 * 固化的两条硬性断言（`发布冒烟清单_v0.1.0-alpha` §9 的人工项替代）：
 *   D6-1 安装包/构建产物**不含 node.exe 与 ST 目录**（SillyTavern/ST 源码树）
 *   D6-2 运行期**无 node 子进程**（源码面静态断言：不 spawn node）+
 *        运行中的 App 进程探测（可选 —— --process 面需要桌面进程在场）
 *
 * 用法：
 *   node scripts/verify-clean-machine.mjs [--artifacts <dir1> <dir2> ...]
 *   CI：npm run selfcheck 不含本脚本（构建产物必须先存在；bundle 环境跑不了）
 *   人工/打包后：手动跑或接入打包流水线。
 *
 * 断言样本（可加）：构建目录默认取 src-tauri/target/release + dist/ 存在者。
 *
 * 退出码：任一断言失败 = 1（干净机器验收不通过）。
 */
import { readdirSync, statSync, existsSync, createReadStream } from 'node:fs'
import { spawnSync } from 'node:child_process'
import { join, dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const here = dirname(fileURLToPath(import.meta.url))
const repo = resolve(here, '..')

let fail = 0
const ok = (c, n) => { if (!c) { fail++; console.error(`✗ ${n}`) } else { console.log(`✔ ${n}`) } }

const args = process.argv.slice(2)
let artifactDirs = []
for (let i = 0; i < args.length; i++) {
  if (args[i] === '--artifacts') {
    artifactDirs = args.slice(i + 1).map((d) => resolve(d))
    break
  }
}
if (!artifactDirs.length) {
  const candidates = [
    join(repo, 'src-tauri', 'target', 'release'),
    join(repo, 'dist'),
  ]
  artifactDirs = candidates.filter((d) => existsSync(d))
}
if (!artifactDirs.length) {
  console.error('未找到构建产物目录（src-tauri/target/release 或 dist）——请先 tauri build 或传 --artifacts')
  process.exit(1)
}

/* ---------------- D6-1：产物不含 node.exe / ST 目录 ---------------- */

const DEFAULT_NODE_NAMES = new Set(['node.exe', 'node', 'node.cmd', 'npm.cmd', 'pnpm.cmd'])
const ST_DIR_MARKERS = ['sillytavern', 'st-1.19.0', 'public/scripts']

function* walk(dir, depth = 0) {
  if (depth > 4) return // 打包目录深度兜底（防符号链接/巨树）
  for (const e of readdirSync(dir)) {
    const p = join(dir, e)
    let m
    try { m = statSync(p) } catch { continue }
    if (m.isDirectory()) yield* walk(p, depth + 1)
    else yield p
  }
}

let nodeExeFound = []
let stDirFound = []
for (const base of artifactDirs) {
  for (const p of walk(base)) {
    const name = p.split(/[\\/]/).pop().toLowerCase()
    const lowerPath = p.toLowerCase()
    if (DEFAULT_NODE_NAMES.has(name)) nodeExeFound.push(p)
    // ST 源码树目录名（作为 zip / 目录出现在产物里 = 打包错把 ST 带走）
    if (ST_DIR_MARKERS.some((m2) => lowerPath.includes(m2))) stDirFound.push(p)
  }
}
ok(nodeExeFound.length === 0, `D6-1a 产物不含 node 可执行文件（found=${nodeExeFound.length}）`)
if (nodeExeFound.length) {
  for (const p of nodeExeFound.slice(0, 5)) console.error(`    ${p}`)
}
ok(stDirFound.length === 0, `D6-1b 产物不含 SillyTavern 源码树标记路径（found=${stDirFound.length}）`)
if (stDirFound.length) {
  for (const p of stDirFound.slice(0, 5)) console.error(`    ${p}`)
}

// cargo 打包命令面：tauri.conf/resources 与 bundle 配置不得把 node/ST 加入打包
{
  const confPath = join(repo, 'src-tauri', 'tauri.conf.json')
  if (existsSync(confPath)) {
    const raw = (await import('node:fs')).readFileSync(confPath, 'utf8')
    ok(!/"node\.exe"?/i.test(raw.replace(/\/\/.*$/gm, '')), 'D6-1c tauri.conf.json 无显式 node.exe 资源引用')
    ok(!/[\\/]sillytavern[\\/]/i.test(raw), 'D6-1d tauri.conf.json 无 SillyTavern 目录资源项')
  }
}

/* ---------------- D6-2：运行期无 node 子进程 ---------------- */

// 2a) 静态：Rust/JS 源码不 spawn node（node 侧的构建/安装脚本不受此限 ——
//     只断言 src-tauri 运行时源码树）
{
  const srcDir = join(repo, 'src-tauri', 'src')
  const bad = []
  for (const p of walk(srcDir, 0)) {
    if (!p.endsWith('.rs')) continue
    const text = (await import('node:fs')).readFileSync(p, 'utf8')
    // spawn node 字面（Command::new("node") / "node.exe"）
    if (/Command::new\(\s*"(node|node\.exe)"/i.test(text)) {
      bad.push(`${p}: Command::new("node")`)
    }
    if (/spawn\s*\(\s*"(?:.*[\\/])?node(?:\.exe)?"?/i.test(text)) {
      bad.push(`${p}: spawn("node...")`)
    }
  }
  ok(bad.length === 0, `D6-2a src-tauri 源码树无 node 子进程调用（found=${bad.length}）`)
  for (const b of bad.slice(0, 5)) console.error(`    ${b}`)
}

// 2b) 动态：检测运行中的 st-chat 进程是否有 node 子进程（--process 传入才做）
{
  const wantProcess = args.includes('--process')
  if (wantProcess) {
    const out = spawnSync('powershell', ['-NoProfile', '-Command', `
      $app = Get-CimInstance Win32_Process -Filter "name like 'st-chat%' or name like 'stchat%'"
      if (-not $app) { exit 2 }
      $ids = ($app | ForEach-Object { $_.ProcessId }) -join ','
      $children = Get-CimInstance Win32_Process -Filter "ParentProcessId=$($app[0].ProcessId)"
      $nodeKids = $children | Where-Object { $"$($_.Name)" -imatch '^node(\\.exe)?$' }
      if ($nodeKids) { $nodeKids | ForEach-Object { Write-Output "node-child: $($_.Name) $($_.ProcessId)" } ; exit 1 }
      exit 0
    `], { encoding: 'utf8' })
    if (out.status === 2) {
      console.log('— 运行中的 st-chat 进程未找到（动态面跳过；先启动 App 再跑 --process）')
    } else {
      ok(out.status === 0, 'D6-2b 运行中的 App 无 node 子进程')
      if (out.stdout && out.stdout.trim()) {
        for (const line of out.stdout.trim().split('\n')) console.error(`    ${line}`)
      }
    }
  } else {
    console.log('— 动态面（--process）未启用：静态断言已覆盖源码面，动态面由干净机器冒烟时跑 --process')
  }
}

console.log(fail ? `\n${fail} 项断言失败 —— 干净机器验收不通过` : '\n全部断言通过（T23 干净机器矩阵）')
process.exit(fail ? 1 : 0)
