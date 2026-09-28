#!/usr/bin/env node
/**
 * NSIS 静默安装到指定目录
 *
 * ⚠ 为什么需要这个脚本：从 WorkBuddy 的 bash 直接传 `/D=D:\STChat` 会被 MSYS
 * 强制做路径转换（反斜杠 → 正斜杠，连单引号都拦不住），而 NSIS 对正斜杠的
 * `/D=` 处理会失败并回退到默认目录。node 的 spawnSync 直接走 CreateProcess，
 * 参数原样传递，没有这层转换。
 *
 * 用法：node scripts/install-to.mjs <setup.exe> <目标目录>
 * 例：  node scripts/install-to.mjs "C:\...\ST Chat_0.1.0_x64-setup.exe" "D:\STChat"
 */
import { spawnSync } from 'node:child_process'
import fs from 'node:fs'

const [setup, destRaw] = process.argv.slice(2)
if (!setup || !destRaw) {
  console.error('用法: node scripts/install-to.mjs <setup.exe> <目标目录>')
  process.exit(2)
}
// ⚠ WorkBuddy 的 bash shim 会把参数里的 `D:\xxx` 强制改成 `D:/xxx`（单引号都拦不住）。
// NSIS 对正斜杠的 /D= 会失败回退默认目录，所以这里换回来。
const dest = destRaw.replace(/\//g, '\\')
if (!fs.existsSync(setup)) {
  console.error(`安装包不存在: ${setup}`)
  process.exit(2)
}

console.log(`安装包: ${setup}`)
console.log(`目标目录: ${dest}`)
console.log(`参数: ["/S", "/D=${dest}"]`)

const t0 = Date.now()
// ⚠ NSIS 规定：/D= 必须是最后一个参数、不能带引号
const r = spawnSync(setup, ['/S', `/D=${dest}`], { stdio: 'ignore' })
const secs = ((Date.now() - t0) / 1000).toFixed(1)

console.log(`退出码: ${r.status}（${secs}s）`)

const ok = fs.existsSync(dest)
console.log(ok ? `✅ 已安装到 ${dest}` : `❌ 目标目录未出现`)
console.log(
  ok
    ? `  文件数: ${(() => {
        let n = 0
        const walk = (d) => {
          let ents = []
          try {
            ents = fs.readdirSync(d, { withFileTypes: true })
          } catch {
            return
          }
          for (const e of ents) {
            const p = `${d}\\${e.name}`
            if (e.isDirectory()) walk(p)
            else n++
          }
        }
        walk(dest)
        return n
      })()}`
    : '',
)
process.exit(ok ? 0 : 1)
