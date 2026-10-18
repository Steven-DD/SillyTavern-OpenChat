/**
 * 契约基线自检（评审整改 T10/T21 + R5/R8：无外部依赖、失败即红）
 *
 * 覆盖四条门禁：
 * 1. manifest 完整性 —— docs/agents/endpoint-manifest.md 必须存在，且头部有结构化
 *    契约版本字段 `contract_version: ST 1.19.0`（R5：断字段，不断散文）；
 * 2. 契约漂移检测（R18 门禁③）—— src/services/st/ 中所有 stGet/stPost 系/stStream/stGetText
 *    的路径字面量必须出现在 manifest 中（动态拼接的模板串按前缀比对）；
 * 3. 静态前缀对齐（R8）—— manifest 的「静态前缀登记」必须与
 *    src-tauri/src/backend/mod.rs 的 EXEMPT_PREFIXES 集合完全相同（双向断言）；
 * 4. 未实装端点禁引（R18 门禁⑤）—— manifest 标注「未迁移」/「移出契约」的路径
 *    不得被 src/services/st 的前端出口命中。
 *
 * CI/新克隆：docs/ 按仓库约定不入库（.gitignore），manifest 必然缺席 →
 * 与 src-tauri/tests/manifest_gate.rs 同口径跳过契约对照（打印提示、不误红）。
 *
 * 用法：npm run selfcheck（由 run-selfchecks.mjs 统一打包执行）
 *       node scripts/selfcheck-contract.mjs --extra-file <path>   # 负向用例注入（R18 证据）
 */
import { readFileSync, readdirSync } from 'node:fs'
import { join, resolve } from 'node:path'

// ⚠ run-selfchecks.mjs 会把本脚本 esbuild bundle 到 node_modules/.selfcheck-tmp 再执行，
// import.meta.url 已不可靠 —— 仓库根一律取 process.cwd()（selfcheck 约定在仓库根跑）。
const root = process.cwd()
let fail = 0
const bad = (msg) => { fail++; console.error(`✗ ${msg}`) }

/* 负向用例注入（R18 证据）：--extra-file <path> 追加进前端出口扫描面 */
const extraFiles = []
{
  const argv = process.argv.slice(2)
  for (let i = 0; i < argv.length; i++) {
    if (argv[i] === '--extra-file' && argv[i + 1]) extraFiles.push(resolve(argv[++i]))
  }
}

/* ── 1. manifest 存在 + 结构化契约版本字段（R5）── */
const manifestPath = join(root, 'docs', 'agents', 'endpoint-manifest.md')
let manifest = ''
try {
  manifest = readFileSync(manifestPath, 'utf-8')
} catch {
  // CI/新克隆：docs/ 不入库（.gitignore），manifest 必然缺席 —— 与 manifest_gate.rs 同口径
  // 跳过契约对照（打印醒目提示、不误红）；本地具备 docs/agents/endpoint-manifest.md 时全量执行。
  console.log(`— 契约基线缺席（docs/ 不入库，CI/新克隆常态）：${manifestPath}`)
  console.log('  跳过 R18 门禁③⑤与 R8 静态前缀对照；本地须先具备 docs/agents/endpoint-manifest.md')
  process.exit(0)
}
const contractVersion = manifest.match(/^contract_version:[ \t]*(.+?)[ \t]*$/m)
if (!contractVersion) {
  bad('manifest 缺结构化契约版本字段 contract_version（应在文件头部，形态：contract_version: ST 1.19.0）')
} else if (contractVersion[1] !== 'ST 1.19.0') {
  bad(`manifest 契约版本字段值错误：contract_version=${contractVersion[1]}（期望 ST 1.19.0）`)
}
if (!/chat_mutate_lines/.test(manifest)) bad('manifest 缺 Tauri IPC 契约（chat_mutate_lines）')
if (!/csrf-token/.test(manifest)) bad('manifest 缺 /csrf-token 基础设施契约')
if (!/CHAT_COMPLETION_SOURCES.*26|26 个/.test(manifest)) bad('manifest 缺 provider 双口径记录（26 源）')

/* 从 manifest 抽出已登记路径（表格列里的 /api/... 与静态前缀）。
   R5：不再放行 scripts 前缀（/scripts/extensions/ 已随 R6 死代码删除，不属本契约基线）。 */
const registered = new Set()
for (const m of manifest.matchAll(/`(\/(?:api|csrf-token|thumbnail|characters|backgrounds|user-images)[^`\s?]*)(\?[^`]*)?`/g)) {
  registered.add(m[1])
}
if (registered.size < 60) bad(`manifest 登记路径过少（${registered.size}），疑似解析失真`)

/* ── 1b. R8：manifest 静态前缀登记 ↔ 后端静态豁免集合（双向）── */
const backendModPath = join(root, 'src-tauri', 'src', 'backend', 'mod.rs')
let exemptPrefixes = []
try {
  const modSrc = readFileSync(backendModPath, 'utf-8')
  const arr = modSrc.match(/const EXEMPT_PREFIXES:\s*\[&str;\s*\d+\]\s*=\s*\[([\s\S]*?)\]/)
  if (arr) exemptPrefixes = [...arr[1].matchAll(/"([^"]+)"/g)].map((x) => x[1])
} catch {
  /* 落到下面的空集合报错 */
}
if (!exemptPrefixes.length) {
  bad('无法从 src-tauri/src/backend/mod.rs 解析静态豁免前缀 EXEMPT_PREFIXES（R8 断言失效）')
} else {
  const declared = new Set()
  const marker = manifest.indexOf('R8 静态前缀登记')
  if (marker < 0) {
    bad('manifest 缺「R8 静态前缀登记」块（静态前缀必须与后端静态豁免集合对齐）')
  } else {
    for (const line of manifest.slice(marker).split('\n').slice(1)) {
      if (!line.trim()) continue
      if (!/^\s*-\s+`/.test(line)) break
      const mm = line.match(/`(\/[^`]+)`/)
      if (mm) declared.add(mm[1])
    }
    if (!declared.size) bad('「R8 静态前缀登记」块为空（应为 /thumbnail、/characters、/backgrounds、/user-images）')
  }
  for (const p of declared) {
    if (!exemptPrefixes.includes(p)) bad(`manifest 登记的静态前缀 ${p} 不在后端静态豁免集合 [${exemptPrefixes.join(', ')}]（R8 漂移）`)
  }
  for (const p of exemptPrefixes) {
    if (!declared.has(p)) bad(`后端静态豁免前缀 ${p} 未登记在 manifest 的「R8 静态前缀登记」块（R8 漂移）`)
  }
}

/* ── 2. 前端出口全量扫描（R18 门禁③）── */
const svcDir = join(root, 'src', 'services', 'st')
const scanTargets = readdirSync(svcDir)
  .filter((f) => f.endsWith('.ts') && f !== 'client.ts' && f !== 'types.ts')
  .map((f) => ({ label: f, path: join(svcDir, f) }))
for (const p of extraFiles) scanTargets.push({ label: `[injected] ${p}`, path: p })

/** 收集全部前端出口命中（label/raw/path），供门禁③（登记）与门禁⑤（未实装禁引）共用 */
const exportHits = []

/* 例外：动态 provider 路径按前缀登记（translate 的 provider）。
   R5：/scripts/extensions/ 白名单已移除（对应死代码 getExtensionManifest 已随 R6 删除）。 */
const dynamicPrefixes = ['/api/translate/']

const callRe = /(?:stGet|stGetText|stPost|stPostJson|stPostVoid|stPostForm|stStream)\s*<?[^(\n]*\(\s*[\n\s]*(`[^`]+`|'[^']+'|"[^"]+")/g

for (const t of scanTargets) {
  const src = readFileSync(t.path, 'utf-8')
  for (const m of src.matchAll(callRe)) {
    const raw = m[1].slice(1, -1)
    // 模板串：取 ${ 前的静态前缀（保留尾斜杠，如 /api/translate/）；
    // 带固定 query 键的模板串（如 /api/sprites/get?name=）取 ? 前的路径段
    const preQuery = raw.split('?')[0]
    const path = preQuery.includes('${') ? preQuery.split('${')[0] : preQuery
    if (!path.startsWith('/')) continue
    exportHits.push({ label: t.label, raw, path })
    if (registered.has(path) || registered.has(path.replace(/\/$/, ''))) continue
    if (dynamicPrefixes.some((p) => path.startsWith(p)) &&
        (manifest.includes(path) || manifest.includes(path.replace(/\/$/, '')))) continue
    bad(`${t.label}: 出口 "${raw}" 未登记在 endpoint-manifest.md（契约漂移；新增出口必须同步回写 manifest）`)
  }
}

/* ── 3. 写端点语义：settings/save 整体覆盖（R1：双跑已取消，仅保留该语义断言）── */
if (!/整体覆盖/.test(manifest) || !/settings\/save/.test(manifest)) {
  bad('manifest 缺 settings/save 整体覆盖语义标注（写端点禁局部写）')
}

/* ── 4. R18 门禁⑤：未实装 / 移出契约端点不得被前端出口引用 ── */
const notMigrated = new Map()
// 4a. 契约表内标注「未迁移」的行（#34 /scripts/extensions/...）
for (const line of manifest.split('\n')) {
  if (!line.trimStart().startsWith('|') || !line.includes('未迁移')) continue
  for (const cell of line.split('|')) {
    for (const m of cell.matchAll(/`(\/[^`]+)`/g)) notMigrated.set(m[1], '契约表标注「未迁移」')
  }
}
// 4b. §9「明确移出 ST 契约的项」表内登记的路径（edge-tts、/proxy 等）
const sec9 = manifest.indexOf('明确移出 ST 契约的项')
if (sec9 >= 0) {
  const seg = manifest.slice(sec9)
  const nextH2 = seg.indexOf('\n## ', 3)
  const block = nextH2 >= 0 ? seg.slice(0, nextH2) : seg
  for (const line of block.split('\n')) {
    if (!line.trimStart().startsWith('|')) continue
    for (const m of line.matchAll(/`(\/[^`]+)`/g)) notMigrated.set(m[1], '§9 明确移出 ST 契约')
  }
}
if (!notMigrated.size) {
  bad('未能从 manifest 解析「未迁移 / 移出契约」端点（R18 门禁⑤解析失效）')
} else {
  // 动态段截断为前缀：/scripts/extensions/<name>/manifest.json → /scripts/extensions/
  const prefixes = [...notMigrated.entries()].map(([p, why]) => [p.split(/[<:]/)[0], p, why])
  for (const h of exportHits) {
    for (const [pre, full, why] of prefixes) {
      if (h.path === full || h.path === pre || h.path.startsWith(pre)) {
        bad(`${h.label}: 出口 "${h.raw}" 命中未实装端点 ${full}（${why}；R18 门禁⑤）`)
      }
    }
  }
  console.log(`— 未迁移/移出契约端点 ${notMigrated.size} 条：前端出口零引用`)
}

if (fail) {
  console.error(`\n${fail} 处契约漂移 —— 前端出口与 docs/agents/endpoint-manifest.md 不一致`)
} else {
  console.log('✅ 契约基线一致：契约版本字段 + 前端出口登记 + 静态前缀豁免集合 + 未实装端点禁引 四项对齐')
}
process.exit(fail ? 1 : 0)
