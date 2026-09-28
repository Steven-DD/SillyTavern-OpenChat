#!/usr/bin/env node
/**
 * 一键跑全部 selfcheck（P2 工程件；此前各脚本要按头部注释手工 esbuild bundle）
 *
 * 大部分 selfcheck 的 import 链里有无扩展名导入，纯 node 跑不了 →
 * 统一用 esbuild 打包成临时 ESM 文件再执行（esbuild 随 vite 依赖树存在，无需新装）。
 * 任一脚本失败即退出码非 0，CI 可直接用。
 *
 * 用法：npm run selfcheck
 */
import { build } from 'esbuild'
import { spawnSync } from 'node:child_process'
import { mkdirSync, readdirSync, rmSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const here = dirname(fileURLToPath(import.meta.url))
const outDir = join(here, '..', 'node_modules', '.selfcheck-tmp')

const scripts = readdirSync(here)
  .filter((f) => /^selfcheck-.+\.mjs$/.test(f) && f !== 'run-selfchecks.mjs')
  .sort()

mkdirSync(outDir, { recursive: true })
let failed = 0
for (const s of scripts) {
  const outfile = join(outDir, s)
  try {
    await build({
      entryPoints: [join(here, s)],
      outfile,
      bundle: true,
      format: 'esm',
      platform: 'node',
      // 依赖树里的包不打进 bundle，运行时仍从 node_modules 解析
      packages: 'external',
      logLevel: 'silent',
    })
    const r = spawnSync(process.execPath, [outfile], { stdio: 'inherit' })
    if (r.status !== 0) {
      console.error(`✗ ${s}（退出码 ${r.status}）`)
      failed++
    }
  } catch (e) {
    console.error(`✗ ${s}（打包失败: ${e instanceof Error ? e.message : e}）`)
    failed++
  }
}

rmSync(outDir, { recursive: true, force: true })
if (failed) {
  console.error(`\n${failed} 个自检失败`)
  process.exit(1)
}
console.log(`\n✅ 全部 ${scripts.length} 个 selfcheck 通过`)
