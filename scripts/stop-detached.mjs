#!/usr/bin/env node
/**
 * 停止由 launch-detached.mjs 启动的服务（按端口定位进程）。
 * 用法: node stop-detached.mjs
 */
import { execSync } from 'node:child_process'

const PORTS = [8000, 1420]

function listeningPids(port) {
  try {
    const out = execSync('netstat -ano', { encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] })
    return [
      ...new Set(
        out
          .split(/\r?\n/)
          .filter((l) => new RegExp(`:${port}\\s`).test(l) && /LISTEN/i.test(l))
          .map((l) => Number(l.trim().split(/\s+/).pop()))
          .filter((n) => Number.isInteger(n) && n > 0),
      ),
    ]
  } catch {
    return []
  }
}

let killed = 0
for (const port of PORTS) {
  const pids = listeningPids(port)
  if (!pids.length) {
    console.log(`[stop] :${port} 未在监听`)
    continue
  }
  for (const pid of pids) {
    try {
      process.kill(pid)
      console.log(`[stop] :${port} 已停止 pid=${pid}`)
      killed++
    } catch (e) {
      console.log(`[stop] :${port} pid=${pid} 停止失败: ${e.message}`)
    }
  }
}
console.log(killed ? '[stop] 完成' : '[stop] 无需操作')
