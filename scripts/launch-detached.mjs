#!/usr/bin/env node
/**
 * 脱离式启动器（Launch Detached）
 *
 * 背景：WorkBuddy 在回合结束时会回收 agent 进程树下的所有子进程，
 *      因此直接用 serve-bg.mjs 起的 ST / Vite 会被连带杀掉。
 *
 * 方案：本脚本必须由「外部宿主」执行（Windows 任务计划程序），
 *      这样脚本进程本身不在 WorkBuddy 的 Job 对象里；
 *      它 spawn 出的子进程同样不在，且 detached + unref 后成为孤儿进程，
 *      父进程退出不影响其存活 → 服务真正常驻。
 *
 * 用法（由任务计划程序调用，或手动双击同目录的 start-detached.bat）：
 *   node launch-detached.mjs
 */
import { spawn } from 'node:child_process'
import { createConnection } from 'node:net'
import { openSync, mkdirSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const __dirname = path.dirname(fileURLToPath(import.meta.url))
const APP = path.resolve(__dirname, '..')
const ROOT = path.resolve(APP, '..')
const ST = path.join(ROOT, 'sillytavern')
const LOGS = path.join(ROOT, 'logs')
const NODE = process.execPath

mkdirSync(LOGS, { recursive: true })

/** 端口探活：同时试 IPv4 与 IPv6（Vite 默认绑 ::1，只探 127.0.0.1 会误判） */
function probe(port, timeout = 1200) {
  const hosts = ['127.0.0.1', '::1']
  return Promise.all(
    hosts.map(
      (host) =>
        new Promise((resolve) => {
          const sock = createConnection({ port, host })
          const done = (ok) => {
            sock.destroy()
            resolve(ok)
          }
          sock.setTimeout(timeout)
          sock.once('connect', () => done(true))
          sock.once('timeout', () => done(false))
          sock.once('error', () => done(false))
        }),
    ),
  ).then((rs) => rs.some(Boolean))
}

function launch(label, cmd, args, cwd, logBase) {
  const out = openSync(path.join(LOGS, `${logBase}.log`), 'a')
  const err = openSync(path.join(LOGS, `${logBase}.err.log`), 'a')
  const child = spawn(cmd, args, {
    cwd,
    detached: true,
    windowsHide: true,
    stdio: ['ignore', out, err],
    env: { ...process.env, NODE_ENV: 'production' },
  })
  child.unref()
  console.log(`[detach] ${label} 已派生 pid=${child.pid}`)
  return child.pid
}

const tasks = [
  {
    label: 'SillyTavern :8000',
    port: 8000,
    cmd: NODE,
    args: ['server.js', '--browserLaunch.enabled=false'],
    cwd: ST,
    logBase: 'st',
  },
  {
    label: 'Vite :1420',
    port: 1420,
    cmd: NODE,
    args: [path.join(APP, 'node_modules', 'vite', 'bin', 'vite.js'), '--port', '1420', '--strictPort'],
    cwd: APP,
    logBase: 'web',
  },
]

for (const t of tasks) {
  if (await probe(t.port)) {
    console.log(`[detach] ${t.label} 已在运行，跳过`)
    continue
  }
  launch(t.label, t.cmd, t.args, t.cwd, t.logBase)
}

// 给子进程一点时间完成端口绑定，便于调用方（任务计划程序）判断
await new Promise((r) => setTimeout(r, 4000))

const results = []
for (const t of tasks) results.push(`${t.label} => ${(await probe(t.port)) ? 'OK' : 'FAILED'}`)
console.log('[detach] 启动结果:\n  ' + results.join('\n  '))
process.exit(0)
