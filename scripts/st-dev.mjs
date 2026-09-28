/**
 * ST 开发启动器：拉起本地 SillyTavern（纯后端模式）并等待 API 就绪。
 * 用法：npm run st
 * 前置：仓库根目录同级存在 sillytavern\（git clone release 分支）
 */
import { spawn } from 'node:child_process'
import { existsSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const ST_PORT = process.env.ST_PORT ?? '8000'
const here = dirname(fileURLToPath(import.meta.url))
const stDir = join(here, '..', '..', 'sillytavern')
const serverJs = join(stDir, 'server.js')

if (!existsSync(serverJs)) {
  console.error(`[st-dev] 未找到 ${serverJs}`)
  console.error('[st-dev] 请先在仓库根目录执行: git clone --depth 1 --branch release https://github.com/SillyTavern/SillyTavern.git')
  process.exit(1)
}

const st = spawn(process.execPath, [serverJs], {
  cwd: stDir,
  env: { ...process.env, PORT: ST_PORT },
  stdio: ['ignore', 'pipe', 'pipe'],
})

st.stdout.on('data', (d) => process.stdout.write(`[st] ${d}`))
st.stderr.on('data', (d) => process.stderr.write(`[st:err] ${d}`))
st.on('exit', (code) => {
  console.error(`[st-dev] SillyTavern 退出（code=${code}）`)
  process.exit(code ?? 1)
})

// 轮询就绪：/api/settings 返回 200 即通路可用
const base = `http://127.0.0.1:${ST_PORT}`
const t0 = Date.now()
const timer = setInterval(async () => {
  try {
    const res = await fetch(`${base}/api/settings`)
    if (res.ok) {
      clearInterval(timer)
      console.log(`[st-dev] ✅ SillyTavern 就绪：${base}（耗时 ${Math.round((Date.now() - t0) / 1000)}s）`)
      console.log('[st-dev] 前端连接地址 http://127.0.0.1:8000 · Ctrl+C 结束')
    }
  } catch {
    /* 未就绪，继续等待 */
  }
  if (Date.now() - t0 > 60_000) {
    clearInterval(timer)
    console.error('[st-dev] 等待超时（60s），请检查 [st] 日志')
  }
}, 1000)

for (const sig of ['SIGINT', 'SIGTERM']) {
  process.on(sig, () => {
    st.kill()
    process.exit(0)
  })
}
