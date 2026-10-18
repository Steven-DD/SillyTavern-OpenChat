/**
 * 架构适应度自检（评审整改 T22 · A9：模块依赖方向禁则）
 *
 * 方案 §4.2 模块划分的依赖方向（api → compat/providers/…；禁止反向）：
 *   backend/compat/*    不得依赖 backend::api（格式内核无 HTTP 面）
 *   backend/providers/* 不得反向依赖 api 之外的兄弟族 handler
 *   providers* / compat 不得 `use crate::api`（T22 强化 · 显式 use 断言）
 * Rust 层用源码正则静态检查（与应用的 tsc 无关，node 直跑）。
 *
 * 用法：npm run selfcheck（或 node scripts/selfcheck-arch-boundaries.mjs）
 */
import { readFileSync, readdirSync } from 'node:fs'
import { join } from 'node:path'

// ⚠ run-selfchecks bundle 后 import.meta.url 不可靠 —— 仓库根取 process.cwd()
const root = process.cwd()
const backendDir = join(root, 'src-tauri', 'src', 'backend')
// compat 家族与 backend 平级（src-tauri/src/compat）—— 不在 backend 扫描根内，
// 必须显式并入文件集，否则规则 1/3 对 compat 空转。
const compatDir = join(root, 'src-tauri', 'src', 'compat')

let fail = 0
const bad = (msg) => { fail++; console.error(`✗ ${msg}`) }

const rustFiles = (dir) => {
  let names = []
  try {
    names = readdirSync(dir)
  } catch {
    return [] // 目录不存在（兼容早期布局）
  }
  const out = []
  for (const name of names) {
    if (name.endsWith('.rs')) out.push(join(dir, name))
  }
  return out
}

const files = [...rustFiles(backendDir), ...rustFiles(compatDir)]

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

/* 规则 3（T22 强化 · 显式 grep 断言）：providers* 与 compat 模块禁
   `use crate::api`（T22 验收原文："CI 中 compat/ 与 providers/ 不得出现
   use crate::api"；use crate::backend::api / super::api 同义形态一并禁）。
   数据只能经 mod.rs 聚合的服务接口或本族模块；api 模块只允许被 handler
   方向（api.rs → 各族 handler）引用。整文件断言（含 #[cfg(test)]）：
   测试若要触 api 面属依赖方向坏味道，直接红。 */
const useApiRe = /(?:^|\r?\n)\s*use\s+(?:crate::(?:backend::)?api|super::api)\b/
for (const f of files) {
  const base = f.split(/[\\/]/).pop() ?? f
  const isCompat = base.includes('compat')
  const isProviders = base.startsWith('providers')
  if (!isCompat && !isProviders) continue
  const src = readFileSync(f, 'utf-8')
  if (useApiRe.test(src)) {
    bad(`providers*/compat 出现 use crate::api（${base}）—— T22 显式断言：api 面不得被反向 import`)
  }
}

if (fail) {
  console.error(`\n${fail} 处依赖方向违规`)
  process.exit(1)
}
console.log('✅ arch-boundary：依赖方向禁则全过（§4.2 适应度函数）')
process.exit(0)
