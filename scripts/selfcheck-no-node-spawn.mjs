#!/usr/bin/env node
/**
 * R18 门禁④：无 node 子进程（静态 Rust 源码面）
 *
 * 从 scripts/verify-clean-machine.mjs 的 D6-2a 静态扫描抽出为独立 selfcheck ——
 * 不依赖构建产物即可在本地/CI 跑（verify-clean-machine 保留产物面 D6-1 与动态面 D6-2b）。
 *
 * 扫描面：src-tauri/src 全树（*.rs）。命中形态（任一即红）：
 *   ① 字面：Command::new("node") / "…/node.exe" / std|tokio::process::Command::new('node')
 *   ② 变量间接：let/const/static X = "…/node" → Command::new(X) / Command::new(&X)
 *   ③ 兜底：Command::new(<标识符>) 且标识符名含 node
 *   ④ spawn 族字面：spawn("node…") / spawnSync / exec / execFile…
 *
 * 用法：
 *   node scripts/selfcheck-no-node-spawn.mjs
 *   node scripts/selfcheck-no-node-spawn.mjs --extra-file <path>   # 负向用例注入（R18 证据）
 *
 * 退出码：命中任一形态 = 1。
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

const srcDir = join(root, 'src-tauri', 'src')
const targets = []
for (const p of walk(srcDir)) if (p.endsWith('.rs')) targets.push(p)

/* 负向用例注入：--extra-file <path> */
const argv = process.argv.slice(2)
const extra = []
for (let i = 0; i < argv.length; i++) {
  if (argv[i] === '--extra-file' && argv[i + 1]) extra.push(resolve(argv[++i]))
}

const rel = (p) => (p.startsWith(root) ? p.slice(root.length + 1) : p).replace(/\\/g, '/')
let fail = 0

/** 从一段源码文本提取「拉起 node 子进程」的命中：字面 / 全限定 / 变量间接形态 */
function scanNodeSpawns(text) {
  const hits = []
  const isNodeBin = (s) => /^node(\.exe)?$/i.test(String(s).split(/[\\/]/).pop() ?? '')
  // ① 字面：Command::new("node") / std::process::Command::new("…/node.exe") / tokio::process::Command::new('node')
  for (const m of text.matchAll(/(?:(?:std|tokio)::process::)?Command::new\(\s*(["'])((?:[^"'\\]|\\.)*)\1\s*\)/gi)) {
    if (isNodeBin(m[2])) hits.push(`Command::new("${m[2]}")`)
  }
  // ② 变量间接：let/const/static X = "…/node"（可包 String::from/PathBuf::from）→ Command::new(X) / Command::new(&X)
  const nodeIdents = new Set()
  for (const m of text.matchAll(/\b(?:let|const|static)\s+(?:mut\s+)?([A-Za-z_]\w*)\s*(?::\s*[^=;\n]+)?=\s*(?:[A-Za-z_:]+::[A-Za-z_:]+\s*\(\s*)?(["'])((?:[^"'\\]|\\.)*)\2/g)) {
    if (isNodeBin(m[3])) nodeIdents.add(m[1])
  }
  for (const id of nodeIdents) {
    if (new RegExp(`(?:(?:std|tokio)::process::)?Command::new\\(\\s*&?\\s*${id}\\b`, 'i').test(text)) {
      hits.push(`Command::new(${id})  // ${id} 绑定到 node 字面`)
    }
  }
  // ③ 兜底：Command::new(<标识符>) 且变量名含 node（覆盖路径经函数拼接后传入的形态）
  for (const m of text.matchAll(/(?:(?:std|tokio)::process::)?Command::new\(\s*&?\s*([A-Za-z_]\w*)\s*[,)]/gi)) {
    if (/node/i.test(m[1])) hits.push(`Command::new(${m[1]})  // 变量名含 node`)
  }
  // ④ 保留既有 spawn 面：spawn("node…") / spawnSync / exec / execFile 等
  for (const m of text.matchAll(/\b(?:spawn|spawnSync|exec|execSync|execFile|execFileSync)\s*\(\s*["'][^"']*?(?:[\\/])?node(?:\.exe)?["']/gi)) {
    hits.push(`${m[0].split('(')[0]}("node…")`)
  }
  return [...new Set(hits)]
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
  for (const h of scanNodeSpawns(text)) {
    fail++
    console.error(`✗ ${rel(p)}: ${h}`)
  }
}

if (fail) {
  console.error(`\n${fail} 处 node 子进程调用（R18 门禁④）`)
  process.exit(1)
}
console.log(`✅ no-node-spawn：扫描 ${targets.length} 个 Rust 源文件 + 注入 ${extra.length}，无 node 子进程调用`)
process.exit(0)
