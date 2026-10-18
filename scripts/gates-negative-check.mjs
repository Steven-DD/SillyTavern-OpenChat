#!/usr/bin/env node
/**
 * R18 五条回归门禁 · 负向用例自动化（注入违规样本必须报红，撤除后恢复绿）
 *
 * ①③④⑤ 走 JS selfcheck 的 --extra-file 注入面（样本文件落在系统临时目录，
 * 不改受保护源码树）；② 走 node 侧镜像断言 —— 权威门是
 * cargo test --test manifest_gate 的 IPC 用例，此处镜像证明其前提（两侧清单一致）
 * 且合成漂移可被同一比较逻辑检出。
 *
 * 用法：node scripts/gates-negative-check.mjs
 * 退出码：任一期望不满足 = 1。
 */
import { readFileSync, writeFileSync, mkdtempSync, rmSync, existsSync } from 'node:fs'
import { spawnSync } from 'node:child_process'
import { join, resolve, dirname } from 'node:path'
import { tmpdir } from 'node:os'
import { fileURLToPath } from 'node:url'

function repoRoot() {
  const cwd = process.cwd()
  if (existsSync(join(cwd, 'src-tauri', 'src'))) return cwd
  const up = resolve(dirname(fileURLToPath(import.meta.url)), '..')
  if (existsSync(join(up, 'src-tauri', 'src'))) return up
  return cwd
}
const root = repoRoot()

const rows = []
let fail = 0
function record(id, name, ok, detail) {
  rows.push({ id: id, name: name, ok: ok, detail: detail })
  if (!ok) fail++
}
function run(script, args) {
  return spawnSync(process.execPath, [join(root, script)].concat(args || []), { cwd: root, encoding: 'utf-8' })
}

const tmp = mkdtempSync(join(tmpdir(), 'r18-gates-neg-'))
try {
  const cases = [
    {
      id: '①', name: '无 ST 直连',
      script: 'scripts/selfcheck-no-st-direct.mjs', file: 'gate1-neg.ts',
      content: "const B='http://127.0.0.1:8000'\n", expect: 'ST 默认端口字面',
    },
    {
      id: '③', name: 'manifest ↔ 前端出口',
      script: 'scripts/selfcheck-contract.mjs', file: 'gate3-neg.ts',
      content: "const x = stGet('/api/ghost/endpoint')\n", expect: '未登记在 endpoint-manifest.md',
    },
    {
      id: '④', name: '无 node 子进程',
      script: 'scripts/selfcheck-no-node-spawn.mjs', file: 'gate4-neg.rs',
      content: 'fn f() {\n    let NODE = "/usr/bin/node";\n    let _ = std::process::Command::new(NODE);\n}\n',
      expect: 'Command::new(NODE)',
    },
    {
      id: '⑤', name: '未实装端点禁引',
      script: 'scripts/selfcheck-contract.mjs', file: 'gate5-neg.ts',
      content: "const x = stGet('/scripts/extensions/third-party/x/manifest.json')\n", expect: '命中未实装端点',
    },
  ]
  for (const c of cases) {
    const sample = join(tmp, c.file)
    writeFileSync(sample, c.content, 'utf-8')
    const neg = run(c.script, ['--extra-file', sample])
    const negOut = (neg.stdout || '') + (neg.stderr || '')
    const red = neg.status === 1 && negOut.indexOf(c.expect) >= 0
    const clean = run(c.script, [])
    const green = clean.status === 0
    record(c.id, c.name, red && green,
      '红 exit=' + neg.status + (red ? '（命中：' + c.expect + '）' : '（未命中断言）') +
      ' / 撤除后绿 exit=' + clean.status + ' | node ' + c.script + ' --extra-file ' + c.file)
  }

  /* ② node 侧镜像：manifest_gate.rs 的 IPC_COMMANDS ↔ lib.rs invoke_handler */
  const gateSrc = readFileSync(join(root, 'src-tauri', 'tests', 'manifest_gate.rs'), 'utf-8')
  const wantBlock = gateSrc.match(/const IPC_COMMANDS: &\[&str\] = &\[([\s\S]*?)\];/)
  const want = wantBlock ? (wantBlock[1].match(/"([^"]+)"/g) || []).map(function (s) { return s.slice(1, -1) }) : []
  const libSrc = readFileSync(join(root, 'src-tauri', 'src', 'lib.rs'), 'utf-8')
  const ihBlock = libSrc.match(/invoke_handler\(tauri::generate_handler!\[([\s\S]*?)\]\)/)
  const got = ihBlock ? ihBlock[1].split(',').map(function (s) { return s.trim() }).filter(Boolean) : []
  const sameSet = function (a, b) {
    if (a.length !== b.length) return false
    return a.slice().sort().join('|') === b.slice().sort().join('|')
  }
  const green2 = want.length >= 5 && sameSet(got, want)
  const red2 = !sameSet(got, want.concat(['ghost_command']))
  record('②', 'manifest ↔ invoke_handler', green2 && red2,
    '实注册=' + got.length + ' / 闸门清单=' + want.length + '，一致=' + green2 + '，合成漂移可检出=' + red2 +
    ' | 权威门：cargo test --manifest-path src-tauri/Cargo.toml --locked --test manifest_gate')

  console.log('R18 五条门禁负向用例（注入报红 → 撤除恢复绿）')
  for (const r of rows) console.log('  ' + (r.ok ? 'PASS' : 'FAIL') + '  ' + r.id + ' ' + r.name + '  ' + r.detail)
  console.log(fail ? '\n' + fail + ' 条负向用例未达预期' : '\n全部负向用例符合预期（' + rows.length + '/5 门禁）')
} finally {
  rmSync(tmp, { recursive: true, force: true })
}
process.exit(fail ? 1 : 0)
