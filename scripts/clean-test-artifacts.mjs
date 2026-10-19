#!/usr/bin/env node
/**
 * 测试残留收尾清理（自检 + cargo test 在 %TEMP% 下的夹具）
 *
 * 背景：Rust 单测把数据根建在系统临时目录下，命名为 `stoc-<case>-<pid>`，
 * 例如 stoc-chars-all-48020 / stoc-vault-sync-51956 / stoc-ext-localrepo-50048 /
 * stoc-backend-test-auth；另有 stoc-clippy.log、stoc-test-all.log 这类测试日志。
 * 跑一轮 cargo test 就留约 81 个目录，且**测试本身不会清**（20 处直接
 * `std::env::temp_dir().join(...)`，没有 TempDir 守卫）。
 * 原来清这类东西的 scripts/clean-test-artifacts.mjs 在批20 随 ST 资产一起删除，
 * 此后无人清理 —— 本脚本补上收尾。
 *
 * 清理范围（严格白名单，只动系统临时目录的**直接子项**）：
 *   - 名称匹配 /^stoc-[A-Za-z0-9._-]+$/i 的目录或文件
 *   - 明确排除 stoc-runtime（应用真实数据目录名，防误伤；TEMP 下本不应存在）
 *
 * 安全闸：
 *   ① 只从 os.tmpdir() 的 readdir 结果里取目标（路径由本脚本拼出，天然在临时目录内）；
 *   ② 不跟随符号链接/junction（只 lstat，整棵树的删除交给 rmSync）；
 *   ③ 检测到在跑的测试/编译进程（cargo / rustc / backend-host / 应用与测试二进制）时
 *      整体跳过 —— 否则会清掉正在执行的测试的数据根（--force 可强制）。
 *
 * 用法：
 *   node scripts/clean-test-artifacts.mjs [--dry-run] [--quiet] [--force]
 *   npm run clean:test    # 手动清
 *   npm run selfcheck     # 自检收尾自动调用（自检失败也会执行）
 *   npm run test:rust     # cargo test 后自动调用
 */
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import { pathToFileURL } from 'node:url'

const NAME_RE = /^stoc-[A-Za-z0-9._-]+$/i
/** 永不删除的名字：应用真实数据目录名（TEMP 下不应存在，属纵深防御） */
const NEVER_DELETE = new Set(['stoc-runtime'])

/** 在跑就不该清：编译进程 + 后端宿主 + 应用/测试二进制（镜像名小写前缀匹配） */
const BUSY_IMAGE_PREFIXES = ['cargo', 'rustc', 'backend-host', 'sillytavern-openchat', 'sillytavern_openchat']

/** 探测在跑的测试/编译进程；命令不可用等异常一律按“探不到”处理（不阻断清理） */
function detectBusyTestProcs() {
  let raw = ''
  try {
    if (process.platform === 'win32') {
      raw = spawnSync('tasklist', ['/FO', 'CSV', '/NH'], { encoding: 'utf8', windowsHide: true }).stdout || ''
    } else {
      raw = spawnSync('ps', ['-A', '-o', 'comm='], { encoding: 'utf8' }).stdout || ''
    }
  } catch {
    return []
  }
  const names =
    process.platform === 'win32'
      ? raw.split(/\r?\n/).map((l) => (l.split(',')[0] || '').replace(/^"|"$/g, '').trim().toLowerCase())
      : raw.split(/\r?\n/).map((l) => l.trim().toLowerCase())
  return [...new Set(names.filter((n) => n && BUSY_IMAGE_PREFIXES.some((p) => n.startsWith(p))))]
}

/** 统计一棵树的文件数与字节数（不跟随符号链接/junction） */
function statTree(root) {
  let files = 0
  let bytes = 0
  const stack = [root]
  while (stack.length) {
    const cur = stack.pop()
    let st
    try {
      st = fs.lstatSync(cur)
    } catch {
      continue
    }
    if (st.isSymbolicLink()) continue
    if (st.isDirectory()) {
      let ents = []
      try {
        ents = fs.readdirSync(cur)
      } catch {
        continue
      }
      for (const e of ents) stack.push(path.join(cur, e))
    } else {
      files++
      bytes += st.size
    }
  }
  return { files, bytes }
}

/**
 * 清理测试残留。返回 { targets, removed, failed, files, bytes, tmp }。
 * 单项删除失败只告警不抛（不因残留把构建/自检带红）。
 * 安全闸不过时返回 { error }，由调用方决定如何处理。
 */
export function cleanTestArtifacts({ dryRun = false, quiet = false, force = false } = {}) {
  const log = (...a) => {
    if (!quiet) console.log(...a)
  }
  const tmp = os.tmpdir()
  if (!tmp || tmp.length < 4 || !fs.existsSync(tmp)) {
    return { error: `临时目录不可用：${tmp || '(空)'}`, tmp }
  }
  // 安全闸③：有测试/编译在跑就整体跳过（在跑的测试的数据根正躺在这里）
  const busy = force ? [] : detectBusyTestProcs()
  if (busy.length) {
    return { tmp, busy, targets: 0, removed: 0, failed: 0, files: 0, bytes: 0 }
  }

  const targets = fs
    .readdirSync(tmp, { withFileTypes: true })
    .filter((e) => NAME_RE.test(e.name) && !NEVER_DELETE.has(e.name.toLowerCase()))
    .map((e) => path.join(tmp, e.name))

  let removed = 0
  let failed = 0
  let files = 0
  let bytes = 0
  for (const p of targets) {
    const s = statTree(p)
    if (dryRun) {
      log(`  待删  ${path.basename(p)}（${s.files} 文件 / ${(s.bytes / 1024).toFixed(1)} KB）`)
      files += s.files
      bytes += s.bytes
      continue
    }
    try {
      fs.rmSync(p, { recursive: true, force: true, maxRetries: 3, retryDelay: 100 })
    } catch (e) {
      failed++
      console.error(`  ⚠ 删除失败 ${path.basename(p)}：${e instanceof Error ? e.message : e}`)
      continue
    }
    if (fs.existsSync(p)) {
      failed++
      console.error(`  ⚠ 仍有残留 ${p}`)
    } else {
      removed++
      files += s.files
      bytes += s.bytes
    }
  }

  return { tmp, targets: targets.length, removed, failed, files, bytes, dryRun }
}

const isCli = process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href
if (isCli) {
  const dryRun = process.argv.includes('--dry-run')
  const quiet = process.argv.includes('--quiet')
  const force = process.argv.includes('--force')
  const r = cleanTestArtifacts({ dryRun, quiet, force })
  if (r.error) {
    console.error(`✗ ${r.error}`)
    process.exit(1)
  }
  if (r.busy && r.busy.length) {
    console.log('=== 测试残留清理：跳过 ===')
    console.log(`  检测到在跑的测试/编译进程：${r.busy.join(', ')}（确认无风险时用 --force 强制清理）`)
    process.exit(0)
  }
  if (!quiet) {
    console.log(`=== 测试残留清理（${r.tmp}，${dryRun ? 'dry-run' : '执行'}）===`)
    console.log(
      dryRun
        ? `  命中 ${r.targets} 项（${r.files} 文件 / ${(r.bytes / 1024).toFixed(1)} KB）—— 未删除`
        : `  清理 ${r.removed}/${r.targets} 项（${r.files} 文件 / ${(r.bytes / 1024).toFixed(1)} KB）${r.failed ? `，${r.failed} 项失败` : ''}`,
    )
  }
  process.exit(0)
}
