import { createApp } from 'vue'

import { createPinia } from 'pinia'
import App from './App.vue'
import router from './router'
import { i18n } from './i18n'
import { useThemeStore } from '@/stores/theme'
import { useAppearanceStore } from '@/stores/appearance'
import { initDesktopBridge } from '@/services/tauri/bridge'
import { installGlobalErrorHook, log } from '@/services/log'
// 原子类（UnoCSS presetUno**不含** reset —— 不要引 @unocss/reset/tailwind.css：
// 该样式会将 html 的 line-height 设为 1.5，影响紧凑布局）
import 'virtual:uno.css'
import './styles/tokens.css'
import './styles/base.css'

const app = createApp(App)
app.use(createPinia())
app.use(router)
app.use(i18n)

// 全局错误捕获（Vue errorHandler + window error/unhandledrejection → 日志）
installGlobalErrorHook(app)

// 移动端安全区域避让（针对 Android WebView env() 为 0 时的保底策略）
if (typeof navigator !== 'undefined' && /Android/i.test(navigator.userAgent)) {
  const root = document.documentElement
  const computedSat = getComputedStyle(root).getPropertyValue('--sat').trim()
  if (!computedSat || computedSat === '0px' || computedSat === 'env(safe-area-inset-top, 0px)') {
    root.style.setProperty('--sat', '40px')
  }
  const computedSab = getComputedStyle(root).getPropertyValue('--sab').trim()
  if (!computedSab || computedSab === '0px' || computedSab === 'env(safe-area-inset-bottom, 0px)') {
    root.style.setProperty('--sab', '24px')
  }
}

// 主题在挂载前应用，避免闪烁
useThemeStore().apply()
// 外观偏好（字号/字体/语言）同样在挂载前应用，避免二次跳动
useAppearanceStore().init()

// 桌面壳：取本地后端地址后再挂载，保证首个请求就打到正确基地址。
// 浏览器开发环境下此函数直接返回（不加载 @tauri-apps/api）。
await initDesktopBridge()

app.mount('#app')
log.op('app.start', '应用启动', { href: location.hash || '/' })
