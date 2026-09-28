#!/usr/bin/env node
/**
 * 清理 S4 测试产物
 *
 * ⚠ 重要：**从 PowerShell / 直接双击运行，不要从 WorkBuddy 的 bash 跑**。
 * 实测同一份删除代码：
 *   - 从 bash 启动 → 3 万文件 6 分钟没删完
 *   - 从 PowerShell 启动 → 同一批目录秒级完成
 * 原因是 bash 环境对非 %TEMP% 的 AppData 路径有额外的文件系统开销（详见项目记忆）。
 *
 * 用法：node scripts/clean-test-artifacts.mjs [--dry-run]
 */
import fs from 'node:fs'
import path from 'node:path'

const DRY = process.argv.includes('--dry-run')

const appData = process.env.APPDATA ?? ''
const localAppData = process.env.LOCALAPPDATA ?? ''
const temp = process.env.TEMP ?? ''

const TARGETS = [
  [path.join(appData, 'com.datago.st-chat', 'plugins'), 'Roaming/plugins'],
  [path.join(localAppData, 'com.datago.st-chat', 'plugins'), 'Local/plugins'],
  [path.join(temp, 's4-dryrun'), 'TEMP/s4-dryrun'],
  [path.join(temp, 's4test'), 'TEMP/s4test'],
  [path.join(temp, 's4verify'), 'TEMP/s4verify'],
  [path.join(localAppData, 'com.datago.st-chat', '_sp'), 'Local/_sp'],
  [path.join(appData, 'com.datago.st-chat', '_sp'), 'Roaming/_sp'],
  [path.join(localAppData, '_sp'), 'Local根/_sp'],
]

const KEEP = path.join(appData, 'com.datago.st-chat', 'st-runtime')

function countFiles(d) {
  let n = 0
  const stack = [d]
  while (stack.length) {
    const cur = stack.pop()
    let ents = []
    try {
      ents = fs.readdirSync(cur, { withFileTypes: true })
    } catch {
      continue
    }
    for (const e of ents) {
      const p = path.join(cur, e.name)
      if (e.isDirectory()) stack.push(p)
      else n++
    }
  }
  return n
}

function treeSize(d) {
  let b = 0
  const stack = [d]
  while (stack.length) {
    const cur = stack.pop()
    let ents = []
    try {
      ents = fs.readdirSync(cur, { withFileTypes: true })
    } catch {
      continue
    }
    for (const e of ents) {
      const p = path.join(cur, e.name)
      if (e.isDirectory()) stack.push(p)
      else {
        try {
          b += fs.statSync(p).size
        } catch {
          /* 被占用的文件跳过 */
        }
      }
    }
  }
  return b
}

console.log(`=== S4 测试产物清理${DRY ? '（dry-run）' : ''} ===`)
console.log(`⚠ 保留（用户数据）：${KEEP}\n`)

let freed = 0
let totalFiles = 0
for (const [p, label] of TARGETS) {
  if (!p || !fs.existsSync(p)) {
    console.log(`  ${label.padEnd(18)} 不存在`)
    continue
  }
  // 安全闸：绝不能碰到 st-runtime
  if (KEEP.startsWith(p)) {
    console.log(`  ${label.padEnd(18)} ⛔ 跳过（会波及用户数据目录）`)
    continue
  }
  const n = countFiles(p)
  const b = treeSize(p)
  if (DRY) {
    console.log(`  ${label.padEnd(18)} 待删 ${n} 文件 / ${(b / 1048576).toFixed(1)} MB`)
    continue
  }
  const t0 = Date.now()
  try {
    fs.rmSync(p, { recursive: true, force: true })
    const secs = ((Date.now() - t0) / 1000).toFixed(1)
    const ok = !fs.existsSync(p)
    console.log(
      `  ${label.padEnd(18)} ${ok ? '已删' : '⚠ 残留'} ${String(n).padStart(6)} 文件 / ${(b / 1048576).toFixed(1).padStart(7)} MB  ${secs}s`,
    )
    if (ok) {
      freed += b
      totalFiles += n
    }
  } catch (e) {
    console.log(`  ${label.padEnd(18)} ❌ ${e.message.slice(0, 120)}`)
  }
}

console.log(`\n合计释放 ${(freed / 1048576).toFixed(1)} MB / ${totalFiles} 文件`)

// 数据完整性自检：清理绝不能碰到 st-runtime
if (fs.existsSync(KEEP)) {
  const n = countFiles(KEEP)
  const b = treeSize(KEEP)
  console.log(`\n数据目录自检：st-runtime ${n} 文件 / ${(b / 1048576).toFixed(2)} MB`)
  for (const [rel, label] of [
    ['data/default-user/settings.json', 'settings.json'],
    ['data/default-user/secrets.json', 'secrets.json'],
    ['data/cookie-secret.txt', 'cookie-secret.txt'],
    ['config.yaml', 'config.yaml'],
  ]) {
    console.log(`  ${label}: ${fs.existsSync(path.join(KEEP, rel)) ? '在 ✅' : '缺失 ❌'}`)
  }
} else {
  console.log('\n⚠ st-runtime 不存在（数据可能已被移动）')
}
