/**
 * 后台服务启动器 v1
 *
 * 背景：WorkBuddy 在回合/会话结束时回收其子进程树，导致 `npm run st` / `npm run dev`
 * 起的服务被一并杀掉（表现为"任务卡住"、端口 8000/1420 无监听）。
 *
 * 对策：用 detached + unref 启动，令 ST 与 Vite 脱离 agent 进程组独立存活；
 *      日志落盘到 <root>/logs/，PID 记录到 <root>/logs/services.json。
 *
 * 用法：
 *   node scripts/serve-bg.mjs start     # 启动 ST + Vite（已在跑则跳过）
 *   node scripts/serve-bg.mjs start st  # 只启动 ST
 *   node scripts/serve-bg.mjs start web # 只启动 Vite
 *   node scripts/serve-bg.mjs stop      # 停止全部（记录在案的 PID）
 *   node scripts/serve-bg.mjs status    # 查看端口与进程状态
 */
import { spawn } from 'node:child_process'
import { existsSync, mkdirSync, openSync, readFileSync, writeFileSync, rmSync } from 'node:fs'
import { createConnection } from 'node:net'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const here = dirname(fileURLToPath(import.meta.url))
const appDir = join(here, '..')
const rootDir = join(appDir, '..')
const stDir = join(rootDir, 'sillytavern')
const logsDir = join(rootDir, 'logs')
const pidFile = join(logsDir, 'services.json')

const ST_PORT = process.env.ST_PORT ?? '8000'
const WEB_PORT = process.env.WEB_PORT ?? '1420'

mkdirSync(logsDir, { recursive: true })

/** 单地址探活 */
function probeHost(port, host, timeout) {
  return new Promise((resolve) => {
    const sock = createConnection({ port: Number(port), host })
    const done = (ok) => {
      sock.destroy()
      resolve(ok)
    }
    sock.setTimeout(timeout)
    sock.once('connect', () => done(true))
    sock.once('timeout', () => done(false))
    sock.once('error', () => done(false))
  })
}

/**
 * 端口探活：vite 默认绑 localhost（可能是 ::1，IPv6），
 * 仅探 127.0.0.1 会误判为未就绪，故多地址轮流试。
 */
async function probe(port, timeout = 1200) {
  for (const host of ['127.0.0.1', '::1', 'localhost']) {
    if (await probeHost(port, host, timeout)) return true
  }
  return false
}

function readPids() {
  try {
    return JSON.parse(readFileSync(pidFile, 'utf8'))
  } catch {
    return {}
  }
}

function writePids(obj) {
  writeFileSync(pidFile, JSON.stringify(obj, null, 2))
}

/** detached 启动：脱离父进程组，父进程退出后子进程照常运行 */
function launch(name, args, cwd, extraEnv = {}) {
  const out = openSync(join(logsDir, `${name}.log`), 'a')
  const err = openSync(join(logsDir, `${name}.err.log`), 'a')
  const child = spawn(process.execPath, args, {
    cwd,
    env: { ...process.env, ...extraEnv },
    detached: true,
    windowsHide: true,
    stdio: ['ignore', out, err],
  })
  child.unref()
  return child.pid
}

async function startST() {
  if (await probe(ST_PORT)) {
    console.log(`[bg] ST 已在 ${ST_PORT} 运行，跳过`)
    return
  }
  if (!existsSync(join(stDir, 'server.js'))) {
    console.error(`[bg] 未找到 ${join(stDir, 'server.js')}`)
    process.exitCode = 1
    return
  }
  const pid = launch(
    'st',
    ['server.js', '--browserLaunch.enabled=false'],
    stDir,
    { PORT: ST_PORT, NODE_ENV: 'production' },
  )
  const pids = readPids()
  pids.st = pid
  writePids(pids)
  console.log(`[bg] ST 已启动 pid=${pid}，等待就绪…`)

  const t0 = Date.now()
  while (Date.now() - t0 < 90_000) {
    if (await probe(ST_PORT)) {
      console.log(`[bg] ✅ ST 就绪 http://127.0.0.1:${ST_PORT}（${Math.round((Date.now() - t0) / 1000)}s）`)
      return
    }
    await new Promise((r) => setTimeout(r, 1000))
  }
  console.error('[bg] ⚠️ ST 90s 未就绪，请查看 logs/st.log / st.err.log')
  process.exitCode = 1
}

async function startWeb() {
  if (await probe(WEB_PORT)) {
    console.log(`[bg] Vite 已在 ${WEB_PORT} 运行，跳过`)
    return
  }
  const viteBin = join(appDir, 'node_modules', 'vite', 'bin', 'vite.js')
  if (!existsSync(viteBin)) {
    console.error(`[bg] 未找到 vite（先执行 npm install）：${viteBin}`)
    process.exitCode = 1
    return
  }
  const pid = launch('web', [viteBin, '--port', WEB_PORT, '--strictPort'], appDir)
  const pids = readPids()
  pids.web = pid
  writePids(pids)
  console.log(`[bg] Vite 已启动 pid=${pid}，等待就绪…`)

  const t0 = Date.now()
  while (Date.now() - t0 < 45_000) {
    if (await probe(WEB_PORT)) {
      console.log(`[bg] ✅ Vite 就绪 http://localhost:${WEB_PORT}（${Math.round((Date.now() - t0) / 1000)}s）`)
      return
    }
    await new Promise((r) => setTimeout(r, 800))
  }
  console.error('[bg] ⚠️ Vite 45s 未就绪，请查看 logs/web.log / web.err.log')
  process.exitCode = 1
}

function stopAll() {
  const pids = readPids()
  for (const [name, pid] of Object.entries(pids)) {
    try {
      process.kill(pid)
      console.log(`[bg] 已停止 ${name} pid=${pid}`)
    } catch {
      console.log(`[bg] ${name} pid=${pid} 已不存在`)
    }
  }
  rmSync(pidFile, { force: true })
}

async function status() {
  const pids = readPids()
  console.log('=== 端口 ===')
  console.log(`ST   ${ST_PORT}: ${(await probe(ST_PORT)) ? 'LISTENING ✅' : 'DOWN ❌'}`)
  console.log(`Vite ${WEB_PORT}: ${(await probe(WEB_PORT)) ? 'LISTENING ✅' : 'DOWN ❌'}`)
  console.log('=== 记录 PID ===')
  console.log(Object.keys(pids).length ? JSON.stringify(pids) : '(无)')
}

const [cmd = 'status', target = 'all'] = process.argv.slice(2)

if (cmd === 'start') {
  if (target === 'all' || target === 'st') await startST()
  if (target === 'all' || target === 'web') await startWeb()
} else if (cmd === 'stop') {
  stopAll()
} else {
  await status()
}
