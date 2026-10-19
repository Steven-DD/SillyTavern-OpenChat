/**
 * 帧内预置库（方案批1）一致性自检
 *
 * 三向对齐，防任何一侧单方面漂移：
 *   ① 前端清单 src/services/st/embed.ts::CARD_LIB_FILES
 *   ② 后端内嵌表 src-tauri/src/backend/cardlibs.rs::ASSETS（include_bytes 的文件名）
 *   ③ 磁盘 src-tauri/assets/card-libs/ 下的 served 文件
 * 外加 buildCardLibTags 的产出形状（顺序 / 基地址 / YAML 别名 / 空基地址不注入）。
 *
 * 背景：上游酒馆助手给每个前端帧注入一套第三方库，卡作者按这些全局量写卡。
 * 我们走本地后端只读路由而非宿主 blob（帧是不透明源，读不到宿主 blob），
 * 因此这条链路的每一环都必须可核对。
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

/* ① ↔ ② 同名同序 */
const rustNames = [...readFileSync(RUST, 'utf8').matchAll(
  /include_bytes!\("\.\.\/\.\.\/assets\/card-libs\/([^"]+)"\)/g,
)].map((m) => m[1])
eq(rustNames, [...CARD_LIB_FILES], 'Rust ASSETS 与前端 CARD_LIB_FILES 同名同序')

/* ① ↔ ③ 磁盘文件集合一致且非空 */
const disk = readdirSync(DIR)
  .filter((f) => /\.(js|css)$/.test(f))
  .sort()
eq(disk, [...CARD_LIB_FILES].sort(), '磁盘 served 文件与前端清单一致')
for (const f of CARD_LIB_FILES) {
  const p = join(DIR, f)
  ok(statSync(p).size > 1024, `${f} 存在且非空（>1KB）`)
  // 文件名内嵌版本是「可长缓存」的前提
  ok(/\d+\.\d+\.\d+/.test(f), `${f} 文件名应含版本号`)
}

/* 标签产出形状 */
const base = 'http://127.0.0.1:12345'
const tags = buildCardLibTags(base)
const lines = tags.split('\n')
eq(lines.length, CARD_LIB_FILES.length + 1, '标签行数 = 库数 + 1（YAML 别名一行）')
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
// 顺序：清单内相对顺序必须与 CARD_LIB_FILES 一致（依赖顺序 jQuery 先于 Vue 等）
const order = CARD_LIB_FILES.map((f) => lines.findIndex((l) => l.includes(`/card-libs/${f}`)))
eq(order, [...order].sort((a, b) => a - b), '标签顺序与清单一致（依赖顺序）')
const yamlIdx = lines.findIndex((l) => l.includes('/card-libs/js-yaml-'))
ok(
  lines[yamlIdx + 1] === '<script>window.YAML=window.YAML||window.jsyaml;</script>',
  'YAML 别名紧跟 js-yaml（上游以 YAML 称该库）',
)
eq(buildCardLibTags(''), '', '基地址未就绪时不注入任何标签（不退化成相对路径）')
eq(buildCardLibTags(`${base}/`), tags, '基地址尾斜杠归一')
eq(buildCardLibTags(`${base}///`), tags, '基地址多尾斜杠归一')

console.log(fail ? `${fail} 失败` : `预置库清单三向一致（${CARD_LIB_FILES.length} 个）· 标签产出形状正确`)
process.exit(fail ? 1 : 0)
