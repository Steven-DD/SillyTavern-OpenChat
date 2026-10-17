/**
 * 架构适应度自检（评审整改 T22 · A9：模块依赖方向禁则）
 *
 * 方案 §4.2 模块划分的依赖方向（api → compat/providers/…；禁止反向）：
 *   backend/compat/*    不得依赖 backend::api（格式内核无 HTTP 面）
 *   backend/providers/* 不得反向依赖 api 之外的兄弟族 handler
 * Rust 层用源码正则静态检查（与应用的 tsc 无关，node 直跑）。
 *
 * 用法：npm run selfcheck（或 node scripts/selfcheck-arch-boundaries.mjs）
 */
import { readFileSync, readdirSync } from 'node:fs'
import { join } from 'node:path'

// ⚠ run-selfchecks bundle 后 import.meta.url 不可靠 —— 仓库根取 process.cwd()
const root = process.cwd()
const backendDir = join(root, 'src-tauri', 'src', 'backend')

let fail = 0
const bad = (msg) => { fail++; console.error(`✗ ${msg}`) }

const rustFiles = (dir) => {
  const out = []
  for (const name of readdirSync(dir)) {
    if (name.endsWith('.rs')) out.push(join(dir, name))
  }
  return out
}

const files = rustFiles(backendDir)

/* 规则 1：compat 是格式内核 —— 全源树禁 `crate::api` / `super::api` / `crate::backend::api` 依赖 */
for (const f of files) {
  if (!f.includes('compat')) continue
  const src = readFileSync(f, 'utf-8')
  if (/crate::(backend::)?api\b|super::api\b/.test(src)) {
    bad(`compat 模块依赖 api（${f.split(/[\\/]/).pop()}）—— 禁则：compat 不含 HTTP 语义`)
  }
}

/* 规则 2（实现面）：providers 之间不得横向依赖兄弟族数据服务。
   ⚠ tests 模块例外（构造 fixture 需要全量 BackendState —— 与 handler
   依赖方向无关）；只用 `#[cfg(test)]` 标记后的实现代码做检查。 */
const bannedInProviders = ['characters', 'chats', 'settings', 'assets', 'extensions', 'extras', 'worldinfo_groups', 'tokenizers_secrets']
for (const f of files) {
  if (!f.includes('providers')) continue
  const src = readFileSync(f, 'utf-8')
  const implPart = src.split('#[cfg(test)]')[0]
  for (const b of bannedInProviders) {
    // permit providers_text（本族内部共享件）
    if (b === 'providers_text') continue
    if (new RegExp(`super::${b}\\s*::`).test(implPart)) {
      bad(`providers 横向依赖兄弟族 ${b}（实现面，${f.split(/[\\/]/).pop()}）—— 禁则：走 mod.rs 聚合的数据服务接口`)
    }
  }
}

if (fail) {
  console.error(`\n${fail} 处依赖方向违规`)
  process.exit(1)
}
console.log('✅ arch-boundary：依赖方向禁则全过（§4.2 适应度函数）')
process.exit(0)
