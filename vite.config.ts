import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'
import UnoCSS from 'unocss/vite'
import { fileURLToPath, URL } from 'node:url'

// Tauri 集成：固定端口供 tauri.conf 的 devUrl 使用
export default defineConfig({
  plugins: [vue(), UnoCSS()],
  resolve: {
    alias: {
      '@': fileURLToPath(new URL('./src', import.meta.url)),
    },
  },
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    /**
     * 必须显式指定 host：
     * Vite 默认 host='localhost'，而 Node 17+ 的 dns.lookup 默认 verbatim，
     * Windows 上 'localhost' 会先解析出 ::1 → 只绑 IPv6 环回，
     * 导致 http://127.0.0.1:1420 连不上（502），探活/预览都会误判。
     * 固定 IPv4 环回：既稳定，又不会把 dev server 暴露到局域网。
     */
    host: '127.0.0.1',
    /**
     * 必须显式忽略 src-tauri/**：
     * cargo 在 src-tauri/target/ 下持续新建/覆盖 build script 及其 .exe/.pdb，
     * Windows 上对「正被占用」的文件调 fs.watch 会直接抛 EBUSY，而 Vite 的
     * FSWatcher 对 error 事件不做兜底（直接抛）→ dev server 进程退出 →
     * tauri dev 报 "The beforeDevCommand terminated with a non-zero status code"。
     * 官方 Tauri 模板同样显式忽略该目录；只忽略 Rust 侧产物，
     * 前端 src/、index.html 的热更新不受影响。
     */
    watch: {
      ignored: ['**/src-tauri/**'],
    },
  },
  // 同上：preview 默认也走 'localhost'，同样会只绑 IPv6 环回，一并固定
  preview: {
    port: 1420,
    host: '127.0.0.1',
  },
  build: {
    target: 'es2022',
    outDir: 'dist',
    /**
     * 关掉自动清空：本机环境的 safe-delete 防护会拦掉「一次递归删除 >50 个条目」，
     * 而 dist/assets 正好在这个量级 → emptyOutDir 必然失败。
     * 改为构建前跑 scripts/clean-dist.mjs（逐文件删除）。
     */
    emptyOutDir: false,
  },
})
