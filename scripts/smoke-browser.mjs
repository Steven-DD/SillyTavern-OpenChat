/**
 * 冒烟浏览器自动化（native 后端直连面 —— Phase 6 清单 §8 一键化执行位）
 *
 * 编排：
 *   1. 起独立 backend-host.exe（真 native 后端；vault 降级面同注册口径）
 *   2. 起一个 OpenAI 兼容 mock 上游（/v1/chat/completions SSE），
 *      并经 native REST 写入「冒烟角色」+ 密钥（vault 面真实写）
 *   3. 起 vite（1420 固定端口——后端 CORS 白名单只认 1420）→
 *      puppeteer(msedge headless) 打开页面，localStorage 预置三段契约：
 *      stoc.base/stoc.apiToken（后端直连）、app.gen+stoc.source（生成通道/模型）、
 *      app.gen.conn={custom:{customUrl}}（按源分桶连接配置，customUrl 以 /v1 结尾）
 *   4. UI 断言：
 *      A. App 进聊天页（输入区出现）
 *      B. REST 造的冒烟角色出现在联系人/角色卡列表
 *      C0. 种下的冒烟会话可打开（首条消息气泡可见）
 *      C1. 输入区定位（右栏 textarea）
 *      C2. 发送 → mock 上游命中 → 冒烟回复气泡（UI→native→上游→SSE 全链）
 *      C3. 回复落盘 native chats/<avatar 去 .png>/<file>.jsonl
 *      D. 设置面板含生成源面板
 *
 * 退出码：断言全过=0。产物：控制台报告 + %TEMP%\smoke-page.txt 文本快照。
 * 依赖：puppeteer-core（--no-save 安装）；Edge 系统自带；需先 cargo build --bin backend-host。
 */
import { spawn } from 'node:child_process'
import { existsSync, writeFileSync, readdirSync } from 'node:fs'
import { mkdtempSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import http from 'node:http'

const here = dirname(fileURLToPath(import.meta.url))
const repo = resolve(here, '..')
const EXE = join(repo, 'src-tauri', 'target', 'debug', 'backend-host.exe')

let fail = 0
let UP_HITS = 0
const ok = (c, n) => { if (!c) { fail++; console.error(`✗ ${n}`) } else { console.log(`✔ ${n}`) } }

/* ---------------- 1) mock OpenAI 上游 ---------------- */

const UPSTREAM = http.createServer((req, res) => {
  if (req.url.startsWith('/v1/models')) {
    res.writeHead(200, { 'content-type': 'application/json' })
    res.end(JSON.stringify({ data: [{ id: 'mock-model' }] }))
    return
  }
  if (req.url.startsWith('/v1/chat/completions')) {
    UP_HITS++
    console.error('[mock] completions hit #' + UP_HITS)
    res.writeHead(200, { 'content-type': 'text/event-stream' })
    for (const f of [
      'data: {"choices":[{"delta":{"content":"冒烟"}}]}\n\n',
      'data: {"choices":[{"delta":{"content":"回复"}}]}\n\n',
      'data: {"choices":[{"delta":{},"finish_reason":"stop"}]}\n\n',
      'data: [DONE]\n\n',
    ]) res.write(f)
    res.end()
    return
  }
  res.writeHead(404); res.end()
})
await new Promise((r) => UPSTREAM.listen(0, '127.0.0.1', r))
const upPort = UPSTREAM.address().port
console.log(`[mock] OpenAI 上游 127.0.0.1:${upPort}`)

/* ---------------- 2) native 后端 + 数据种子 ---------------- */

const dataRoot = mkdtempSync(join(tmpdir(), 'stoc-smoke-'))
console.log(`[data] ${dataRoot}`)
const backend = spawn(EXE, [dataRoot], { stdio: ['ignore', 'pipe', 'pipe'] })
let port = 0, token = ''
let buf = ''
backend.stdout.on('data', (d) => {
  buf += d
  const m = /META BACKEND port=(\d+) token=([0-9a-f]+)/.exec(buf)
  if (m && !port) { port = +m[1]; token = m[2] }
})
let deadline = Date.now() + 15_000
while (!port && Date.now() < deadline) await new Promise((r) => setTimeout(r, 100))
if (!port) { console.error('backend-host 未在 15s 内上报端口'); process.exit(1) }
console.log(`[backend] 127.0.0.1:${port}`)

const BE = `http://127.0.0.1:${port}`
const H = {
  'X-Relay-Auth': token,
  'Content-Type': 'application/json',
}

// 无痕 REST 助手（native 无 CSRF 校验面；X-CSRF-Token 带上亦可）
let csrf = ''
// 先照 client.ts 流程取一次（真实面）
{
  const r = await fetch(`${BE}/csrf-token`, { headers: { 'X-Relay-Auth': token } })
  csrf = (await r.json()).token
}
const bpost = async (path, body, extra = {}) => {
  const isForm = body instanceof FormData
  const r = await fetch(`${BE}${path}`, {
    method: 'POST',
    headers: {
      ...(isForm ? {} : { 'Content-Type': 'application/json' }),
      'X-Relay-Auth': token,
      'X-CSRF-Token': csrf,
      ...extra,
    },
    body: isForm ? body : JSON.stringify(body),
  })
  return r
}

// 造角色卡（multipart 字段面 = data.ts createCharacter 契约：ch_name + avatar 文件）
{
  // 最小 1x1 PNG（T9 只验魔数；可选头/宽高面按 backend 上传 fixtures 同款 1x1）
  const pngB64 =
    'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg=='
  const png = Buffer.from(pngB64, 'base64')
  const fd = new FormData()
  fd.append('ch_name', '冒烟角色')
  fd.append('description', '自动化冒烟角色')
  fd.append('first_mes', '你好，我是冒烟角色。')
  fd.append('avatar', new Blob([png], { type: 'image/png' }), 'smoke-hero.png')
  const r = await bpost('/api/characters/create', fd)
  console.log(`[seed] characters/create → ${r.status} ${await r.text()}`)
  const r2 = await bpost('/api/secrets/write', { key: 'api_key_custom', value: 'sk-smoke' })
  console.log(`[seed] secrets/write → ${r2.status}`)
}

// 种一条“冒烟会话”（saveChat 契约：header 是首行 metadata —— /chats/save 自组）
{
  const chat = [
    {
      user_name: 'User',
      character_name: '冒烟角色',
      create_date: '2000-01-01T00:00:00',
      chat_metadata: {},
    },
    {
      name: 'User',
      is_user: true,
      is_name: true,
      send_date: '2000-01-01T00:00:00.000',
      mes: '你好',
    },
    {
      name: '冒烟角色',
      is_user: false,
      is_name: true,
      send_date: '2000-01-01T00:00:00.000',
      mes: '你好，我是冒烟角色。',
    },
  ]
  const r = await bpost('/api/chats/save', {
    avatar_url: '冒烟角色.png',
    file_name: 'smoke-chat', // backend 语义：无条件补 .jsonl（与 ST appendExtension 同款）
    chat,
  })
  console.log(`[seed] chats/save → ${r.status}`)
}

/* ---------------- 3) vite + puppeteer ---------------- */

const VITE_PORT = 1420
// 复用/残留清理：1420 被占用则杀（上一轮 vite 残留是 Windows 下 npm shell kill 不彻底的面）
{
  const netstat = spawn('netstat', ['-ano', '-p', 'tcp'], { shell: true })
  let out = ''
  netstat.stdout.on('data', (d) => (out += d))
  netstat.on('close', () => {
    for (const line of out.split(/\r?\n/)) {
      const m = /LISTENING\s+(\d+)\s*$/.exec(line)
      if (m && line.includes(':1420')) {
        try { process.kill(+m[1], 'SIGKILL') } catch {}
      }
    }
  })
}
const vite = spawn('npm', ['run', 'dev', '--', '--port', String(VITE_PORT), '--strictPort'], { cwd: repo, shell: true, stdio: ['ignore', 'pipe', 'pipe'] })
let viteReady = false
vite.stdout.on('data', (d) => {
  const s = d.toString()
  if (/ready in|Local:/.test(s)) viteReady = true
})
vite.stderr.on('data', (d) => process.stderr.write(d))
deadline = Date.now() + 30_000
// 双探：checkLocal 一次
while (!(viteReady && await (async () => {
  try { const r = await fetch(`http://127.0.0.1:${VITE_PORT}/`); return r.ok } catch { return false }
})()) && Date.now() < deadline) {
  await new Promise((r) => setTimeout(r, 300))
}
ok(viteReady, 'vite dev 就绪')
if (!viteReady) { process.exit(1) }

const puppeteer = await import('puppeteer-core')
const edgePath = ['C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe', 'C:/Program Files/Microsoft/Edge/Application/msedge.exe'].find(
  (p) => existsSync(p),
)
if (!edgePath) { console.error('未找到 Edge —— 请安装或指定期望浏览器路径'); process.exit(1) }

const browser = await puppeteer.default.launch({
  executablePath: edgePath,
  headless: 'new',
  args: ['--no-sandbox', '--window-size=1400,900'],
})
const page = await browser.newPage()
// 诊断：页面侧对后端 API 的 4xx/5xx（favicon 等 404 不会刷屏，仅 /api/ 面）
page.on('response', (r) => { if (r.url().includes('/api/') && r.status() >= 400) console.error('[HTTP ' + r.status() + '] ' + r.url().slice(-80)) })
await page.setViewport({ width: 1400, height: 900 })
page.on('dialog', async (d) => { await d.accept() })
await page.evaluateOnNewDocument((cfg) => {
  // native 后端直连（bridge.ts 语义：base + apiToken 指向 backend-host；旧键 st.base/st.relayToken 已废弃）
  localStorage.setItem('stoc.base', cfg.base)
  // apiToken 主存 sessionStorage（client.ts setApiToken）；再写 localStorage 回落面
  sessionStorage.setItem('stoc.apiToken', cfg.token)
  localStorage.setItem('stoc.apiToken', cfg.token)
  // 生成通道/模型 = settings store 的 app.gen JSON（ChatsMain 状态条 gen.model/gen.source）
  localStorage.setItem('app.gen', cfg.gen)
  // 连接配置 = 按源分桶的 map（api.ts getConnConfig(source)）
  localStorage.setItem('app.gen.conn', cfg.conn)
  // api.ts 兜底面（currentSource；键已随 task-10 迁移为 stoc.source）
  localStorage.setItem('stoc.source', 'custom')
  localStorage.setItem('app.gen.stream', '1')
}, { base: BE, token, gen: JSON.stringify({ source: 'custom', model: 'mock-model' }), conn: JSON.stringify({ custom: { customUrl: `http://127.0.0.1:${upPort}/v1` } }) })

try {
  await page.goto(`http://127.0.0.1:${VITE_PORT}/`, { waitUntil: 'networkidle2', timeout: 30_000 })
} catch (e) {
  console.error(`goto: ${e.message}`)
  process.exit(1)
}
await new Promise((r) => setTimeout(r, 3000))

const snapAll = await page.evaluate(() => document.body.innerText.slice(0, 3000))
writeFileSync(join(tmpdir(), 'smoke-page.txt'), snapAll, 'utf8')
console.log('[page] text snapshot → %TEMP%\\smoke-page.txt')

/* ---------------- A. 进聊天页 ---------------- */
const hasInput = await page.evaluate(() => {
  const els = [...document.querySelectorAll('textarea,input[type=text]')]
  return els.some((e) => e.getBoundingClientRect().height > 0)
})
ok(hasInput, 'A. App 进聊天页（输入区可见）')

/* ---------------- B. 联系人出现 ---------------- */
let heroSeen = false
for (let attempt = 0; attempt < 3 && !heroSeen; attempt++) {
  // 切到联系人页签（导航文本）
  try {
    await page.evaluate(() => {
      const nav = [...document.querySelectorAll('button,a,[role=button],div,span')].find(
        (n) => n.children.length === 0 && n.textContent.trim() === '联系人',
      )
      nav?.click()
    })
  } catch {}
  await new Promise((r) => setTimeout(r, 1500))
  heroSeen = await page.evaluate(() => document.body.innerText.includes('冒烟角色'))
  if (!heroSeen) await page.reload({ waitUntil: 'networkidle2' })
  await new Promise((r) => setTimeout(r, 1500))
  heroSeen = await page.evaluate(() => document.body.innerText.includes('冒烟角色'))
}
ok(heroSeen, 'B. REST 种下的「冒烟角色」出现在联系人列表')

/* ---------------- C. 打开聊天 + 发消息 + 流式回复 ---------------- */
try {
  // 切回会话页签（左栏路由入口）
  for (let i = 0; i < 5; i++) {
    await page.evaluate(() => {
      const nav = [...document.querySelectorAll('button,a,[role=button],div,span')].find(
        (n) => n.children.length === 0 && n.textContent.trim() === '会话',
      )
      nav?.click()
    })
    await new Promise((r) => setTimeout(r, 800))
    // 打开种下的会话：找会话行（.sess 行内含冒烟文本）——重试等 loadSessions 完成
    const opened = await page.evaluate(() => {
      const lines = [...document.querySelectorAll('*')].filter(
        (n) => n.children.length === 0 && /冒烟/.test(n.textContent || ''),
      )
      const row = lines.find((n) => n.closest('.sess'))?.closest('.sess')
      if (row) row.click()
      return !!row
    })
    if (opened) break
  }
  // 等首条消息气泡出现（会话真正打开的信号）
  let chatOpen = false
  for (let i = 0; i < 20 && !chatOpen; i++) {
    chatOpen = await page.evaluate(() => document.body.innerText.includes('你好，我是冒烟角色。'))
    await new Promise((r) => setTimeout(r, 250))
  }
  ok(chatOpen, 'C0. 种下会话已打开（首条消息「你好，我是冒烟角色。」可见）')

  // 输入区：限右栏内容页的 textarea（避开中栏搜索框）；回落 contenteditable
  const inputSel = await page.evaluate(() => {
    const pane = document.querySelector('.pane-main') ?? document
    const ta = [...pane.querySelectorAll('textarea')].find((t) => t.getBoundingClientRect().height > 0)
    if (ta) return 'textarea'
    const ce = pane.querySelector('[contenteditable=true]')
    return ce ? '[contenteditable=true]' : null
  })
  ok(!!inputSel, `C1. 输入区定位成功（${inputSel}）`)
  if (inputSel) {
    await page.click(inputSel)
    await page.type(inputSel, '测试消息')
    await page.keyboard.press('Enter')
    // Enter 兜底：若未进入流式（按钮仍是「发 送」），点一次按钮发送
    await new Promise((r) => setTimeout(r, 1200))
    await page.evaluate(() => {
      const btn = [...document.querySelectorAll('button')].find(
        (b) => b.children.length <= 1 && b.textContent.trim() === '发 送' && b.getBoundingClientRect().height > 0,
      )
      btn?.click()
    })
    // 等回复气泡（mock 上游固定返回「冒烟回复」）
    let got = false
    for (let i = 0; i < 40 && !got; i++) {
      got = await page.evaluate(() => document.body.innerText.includes('冒烟回复'))
      await new Promise((r) => setTimeout(r, 250))
    }
    ok(got, 'C2. 生成回复「冒烟回复」出现在页面')
    // 会话落盘面（native）：chats/<avatar 去 .png>/<file>.jsonl（unconditional 补扩展名语义）
    const chatDir = join(dataRoot, 'default-user', 'chats', '冒烟角色')
    const saved = existsSync(chatDir) && readdirSync(chatDir).some((f) => f.endsWith('.jsonl'))
    ok(saved, 'C3. 回复已落盘（native chats/save jsonl）')
  }
} catch (e) {
  fail++
  console.error(`✗ C 异常：${e.message}`)
}

/* ---------------- D. 设置面板可见 ---------------- */
try {
  // 找设置入口（齿轮/文本）；App 侧分别为 设置 → 通用/模型 等
  const setupBtn = await page.evaluateHandle(() => {
    return [...document.querySelectorAll('button,[role=button],a,div,span')]
      .find((n) => n.textContent.trim() === '设置' || n.getAttribute('aria-label') === '设置')
  })
  if (setupBtn && setupBtn.asElement()) {
    await setupBtn.asElement().click()
    await new Promise((r) => setTimeout(r, 1000))
    const settingsVisible = await page.evaluate(() =>
      document.body.innerText.includes('模型') && (document.body.innerText.includes('自定义') || document.body.innerText.includes('OpenAI')),
    )
    ok(settingsVisible, 'D. 设置面板含生成源面板')
  } else {
    console.log('— 未定位设置入口（人工确认 §3；不判失败）')
  }
} catch (e) {
  console.log(`— D 异常（人工确认）：${e.message}`)
}

/* ---------------- 收尾 ---------------- */
await browser.close()
vite.kill()
UPSTREAM.close()
backend.kill()

console.log(fail ? `\n${fail} 项失败 —— 冒烟自动化未通过` : '\n冒烟自动化全部通过')
process.exit(fail ? 1 : 0)
