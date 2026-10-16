/**
 * 契约基线自检（评审整改 T10/T21：fixtures 驱动、无外部依赖、失败即红）
 *
 * 覆盖两条门禁：
 * 1. manifest 完整性 —— docs/agents/endpoint-manifest.md 必须存在且为合法结构；
 * 2. 契约漂移检测 —— src/services/st/ 中所有 stGet/stPost 系/stStream/stGetText 的
 *    路径字面量必须出现在 manifest 中（动态拼接的模板串按前缀比对）。
 *
 * 用法：npm run selfcheck（由 run-selfchecks.mjs 统一打包执行）
 */
import { readFileSync, readdirSync } from 'node:fs'
import { join } from 'node:path'

// ⚠ run-selfchecks.mjs 会把本脚本 esbuild bundle 到 node_modules/.selfcheck-tmp 再执行，
// import.meta.url 已不可靠 —— 仓库根一律取 process.cwd()（selfcheck 约定在仓库根跑）。
const root = process.cwd()
let fail = 0
const bad = (msg) => { fail++; console.error(`✗ ${msg}`) }

/* ── 1. manifest 存在且可解析 ── */
const manifestPath = join(root, 'docs', 'agents', 'endpoint-manifest.md')
let manifest = ''
try {
  manifest = readFileSync(manifestPath, 'utf-8')
} catch {
  bad(`endpoint-manifest.md 不存在（契约基线丢失）：${manifestPath}`)
  console.log(fail ? `${fail} 失败` : '通过')
  process.exit(1)
}
if (!/ST 1\.19\.0/.test(manifest)) bad('manifest 未标注契约版本 ST 1.19.0')
if (!/chat_mutate_lines/.test(manifest)) bad('manifest 缺 Tauri IPC 契约（chat_mutate_lines）')
if (!/csrf-token/.test(manifest)) bad('manifest 缺 /csrf-token 基础设施契约')
if (!/CHAT_COMPLETION_SOURCES.*26|26 个/.test(manifest)) bad('manifest 缺 provider 双口径记录（26 源）')

/* 从 manifest 抽出已登记路径（表格列里的 /api/... 与静态前缀） */
const registered = new Set()
for (const m of manifest.matchAll(/`(\/(?:api|csrf-token|thumbnail|characters|backgrounds|user-images|scripts)[^`\s?]*)(\?[^`]*)?`/g)) {
  registered.add(m[1])
}
if (registered.size < 60) bad(`manifest 登记路径过少（${registered.size}），疑似解析失真`)

/* ── 2. 前端出口全量扫描 ── */
const svcDir = join(root, 'src', 'services', 'st')
const files = readdirSync(svcDir).filter((f) => f.endsWith('.ts') && f !== 'client.ts' && f !== 'types.ts')

/* 例外：动态 provider/名称 路径按前缀登记（translate 的 provider、extensions 的 name、sprites 的 query） */
const dynamicPrefixes = ['/api/translate/', '/scripts/extensions/']

const callRe = /(?:stGet|stGetText|stPost|stPostJson|stPostVoid|stPostForm|stStream)\s*<?[^(\n]*\(\s*[\n\s]*(`[^`]+`|'[^']+'|"[^"]+")/g

for (const f of files) {
  const src = readFileSync(join(svcDir, f), 'utf-8')
  for (const m of src.matchAll(callRe)) {
    const raw = m[1].slice(1, -1)
    // 模板串：取 ${ 前的静态前缀（保留尾斜杠，如 /api/translate/）；
    // 带固定 query 键的模板串（如 /api/sprites/get?name=）取 ? 前的路径段
    const preQuery = raw.split('?')[0]
    const path = preQuery.includes('${') ? preQuery.split('${')[0] : preQuery
    if (!path.startsWith('/')) continue
    if (registered.has(path) || registered.has(path.replace(/\/$/, ''))) continue
    if (dynamicPrefixes.some((p) => path.startsWith(p)) &&
        (manifest.includes(path) || manifest.includes(path.replace(/\/$/, '')))) continue
    bad(`${f}: 出口 "${raw}" 未登记在 endpoint-manifest.md（契约漂移；新增出口必须同步回写 manifest）`)
  }
}

/* ── 3. 写端点分类表存在（双跑比对框架的输入）── */
if (!/整体覆盖/.test(manifest) || !/settings\/save/.test(manifest)) {
  bad('manifest 缺 settings/save 整体覆盖语义标注（T4 写端点分类的依据）')
}

if (fail) {
  console.error(`\n${fail} 处契约漂移 —— 前端出口与 docs/agents/endpoint-manifest.md 不一致`)
} else {
  console.log('✅ 契约基线一致：前端出口全部登记于 endpoint-manifest.md')
}
process.exit(fail ? 1 : 0)
