import { createApp } from 'vue'
import { createPinia } from 'pinia'
import App from './App.vue'
import router from './router'
import { useThemeStore } from '@/stores/theme'
import { useAppearanceStore } from '@/stores/appearance'
import { initDesktopBridge } from '@/services/tauri/bridge'
// 原子类（UnoCSS presetUno**不含** reset —— 不要引 @unocss/reset/tailwind.css：
// 它会给 html 设 line-height:1.5，把我们的紧凑排版整体撑高）
import 'virtual:uno.css'
import './styles/tokens.css'
import './styles/base.css'

const app = createApp(App)
app.use(createPinia())
app.use(router)

// 主题在挂载前应用，避免闪烁
useThemeStore().apply()
// 外观偏好（字号/字体/语言）同样在挂载前应用，避免二次跳动
useAppearanceStore().init()

// 桌面壳：取中继地址 + 订阅 ST 状态后再挂载，保证首个请求就打到正确基地址。
// 浏览器开发环境下此函数直接返回（不加载 @tauri-apps/api）。
await initDesktopBridge()

app.mount('#app')
