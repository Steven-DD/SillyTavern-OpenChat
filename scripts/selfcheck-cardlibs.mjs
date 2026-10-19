/**
 * 帧内预置库（方案批1+批2）一致性自检
 *
 * 后端表分两类条目，因此这里按两类分别断言：
 *   · 可注入集（inject: true, css/js） ↔ 前端 CARD_LIB_FILES（同名同序）
 *   · 可服务集（全部，含仅供 CSS 相对引用的 webfonts/主题图片） ↔ 磁盘文件（递归）
 * 另验：Rust 条目的 include_bytes 路径必须等于其 URL 名（名字即相对路径，子目录才安全）、
 * 以及 buildCardLibTags 的产出形状（顺序 / 基地址 / YAML 别名 / 空基地址不注入）。
 *
 * 背景：上游酒馆助手给每个前端帧注入一套第三方库，卡作者按这些全局量写卡。
 * 我们走本地后端只读路由而非宿主 blob（帧是不透明源，读不到宿主 blob）。
 */
import { readFileSync, readdirSync, statSync } from 'node:fs'
import { join } from 'node:path'

const { CARD_LIB_FILES, buildCardLibTags } = await import('../src/services/st/embed.ts')

// ⚠ 基准用 cwd 而非 import.meta.url：本文件会被 run-selfchecks 用 esbuild 打包到
// node_modules/.selfcheck-tmp/ 下执行，import.meta.url 那时指向打包产物，路径会错。
// run-selfchecks 恒以仓库根为 cwd 启动子进程。
const repo = process.cwd()
const RUST = join(repo, 'src-tauri', 'src', 'backend', 'cardlibs.rs')
const DIR = join(repo, 'src-tauri', 'assets', 'card-libs')

let fail = 0
const ok = (c, n) => {
  if (!c) {
    fail++
    console.error(`✗ ${n}`)
  }
}
const eq = (a, e, n) => {
  if (JSON.stringify(a) !== JSON.stringify(e)) {
    fail++
    console.error(`✗ ${n}\n  期望 ${JSON.stringify(e)}\n  实得 ${JSON.stringify(a)}`)
  }
}

/* ---- 解析 Rust 表（只取含 include_bytes 的 Asset 块） ---- */
const rs = readFileSync(RUST, 'utf8')
const entries = [...rs.matchAll(/Asset\s*\{([\s\S]*?)\n\s*\},/g)]
  .map((m) => {
    const b = m[1]
    return {
      name: /name:\s*"([^"]+)"/.exec(b)?.[1],
      inject: /inject:\s*(true|false)/.exec(b)?.[1] === 'true',
      path: /include_bytes!\(\s*"(?:\.\.\/)+assets\/card-libs\/([^"]+)"\s*\)/.exec(b)?.[1],
    }
  })
  .filter((e) => e.name && e.path)

ok(entries.length > 0, 'Rust ASSETS 解析出条目')
for (const e of entries) {
  eq(e.path, e.name, `${e.name} 的 include_bytes 路径应等于其 URL 名（名字即相对路径）`)
}
const rustInject = entries.filter((e) => e.inject).map((e) => e.name)
const rustAll = entries.map((e) => e.name)

/* ---- 可注入集 ↔ 前端清单（同名同序） ---- */
eq(rustInject, [...CARD_LIB_FILES], 'Rust injectable() 与前端 CARD_LIB_FILES 同名同序')

/* ---- 可服务集 ↔ 磁盘（递归；批2 有子目录） ---- */
function walk(dir, base = '') {
  const out = []
  for (const ent of readdirSync(dir, { withFileTypes: true })) {
    const rel = base ? `${base}/${ent.name}` : ent.name
    if (ent.isDirectory()) out.push(...walk(join(dir, ent.name), rel))
    else if (/\.(js|css|woff2|png)$/.test(ent.name)) out.push(rel)
  }
  return out
}
const disk = walk(DIR).sort()
eq(disk, [...rustAll].sort(), '磁盘 served 文件与 Rust 可服务集一致')

for (const name of CARD_LIB_FILES) {
  const p = join(DIR, ...name.split('/'))
  ok(statSync(p).size > 1024, `${name} 存在且非空（>1KB）`)
  ok(/\d+\.\d+\.\d+/.test(name), `${name} 文件名应含版本号`)
}

/* ---- 标签产出形状 ---- */
const base = 'http://127.0.0.1:12345'
const tags = buildCardLibTags(base)
const lines = tags.split('\n')
eq(lines.length, CARD_LIB_FILES.length + 1, '标签行数 = 可注入库数 + 1（YAML 别名一行）')
CARD_LIB_FILES.forEach((f) => {
  // ⚠ 按内容定位而非索引：js-yaml 之后多插了一行 YAML 别名，索引会错位
  const idx = lines.findIndex((l) => l.includes(`/card-libs/${f}`))
  ok(idx >= 0, `${f} 有对应标签行`)
  if (idx < 0) return
  if (f.endsWith('.css')) {
    ok(lines[idx].startsWith('<link rel="stylesheet"'), `${f} 应用 <link>`)
  } else {
    ok(
      lines[idx].startsWith('<script src='),
      `${f} 应用 <script src=（parser 阻塞，保证卡内脚本前就绪）`,
    )
  }
})
const order = CARD_LIB_FILES.map((f) => lines.findIndex((l) => l.includes(`/card-libs/${f}`)))
eq(order, [...order].sort((a, b) => a - b), '标签顺序与清单一致（依赖顺序）')
const yamlIdx = lines.findIndex((l) => l.includes('/card-libs/js-yaml-'))
ok(
  lines[yamlIdx + 1] === '<script>window.YAML=window.YAML||window.jsyaml;</script>',
  'YAML 别名紧跟 js-yaml（上游以 YAML 称该库）',
)
eq(buildCardLibTags(''), '', '基地址未就绪时不注入任何标签（不退化成相对路径）')
eq(buildCardLibTags(`${base}/`), tags, '基地址尾斜杠归一')

console.log(
  fail
    ? `${fail} 失败`
    : `预置库清单一致：可注入 ${CARD_LIB_FILES.length} 个 / 可服务 ${rustAll.length} 个 · 标签形状正确`,
)
process.exit(fail ? 1 : 0)
