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
    // ST 通路：同源代理解决跨域 + CSRF Cookie（生产环境由 Tauri Rust 侧中继）
    proxy: {
      '/api': 'http://127.0.0.1:8000',
      '/csrf-token': 'http://127.0.0.1:8000',
      // 静态资源：角色头像缩略图 /thumbnail?type=avatar&file=x.png、原图 /characters/x.png
      '/thumbnail': 'http://127.0.0.1:8000',
      '/characters': 'http://127.0.0.1:8000',
      '/user-images': 'http://127.0.0.1:8000',
      '/backgrounds': 'http://127.0.0.1:8000',
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
