import { defineStore, acceptHMRUpdate } from 'pinia'

export type ThemeMode = 'dark' | 'light' | 'cyber-tokyo' | 'nordic-fog' | 'dusk'

const STORAGE_KEY = 'app.theme'

function getInitialMode(): ThemeMode {
  try {
    const saved = localStorage.getItem(STORAGE_KEY) as ThemeMode | null
    if (saved && ['dark', 'light', 'cyber-tokyo', 'nordic-fog', 'dusk'].includes(saved)) {
      return saved
    }
  } catch {
    /* ignore */
  }
  return 'dark'
}

/**
 * 主题状态 + 皮肤通道。
 * 内置五套高级主题：
 *  - dark: 曜石幻夜 (Obsidian Zinc · 推荐默认)
 *  - light: 象牙纸墨 (Ivory Editorial · 柔和浅色)
 *  - cyber-tokyo: 赛博极夜 (Tokyo Cyber · 霓虹莓红与荧光青)
 *  - nordic-fog: 暮霭冷杉 (Nordic Forest · 治愈苔绿)
 *  - dusk: 暮光 (Rose Dusk · 玫瑰绯红)
 */
export const useThemeStore = defineStore('theme', {
  state: () => ({
    mode: getInitialMode() as ThemeMode,
  }),
  actions: {
    setMode(mode: ThemeMode) {
      this.mode = mode
      try {
        localStorage.setItem(STORAGE_KEY, mode)
      } catch {
        /* ignore */
      }
      this.apply()
    },
    apply() {
      document.documentElement.setAttribute('data-theme', this.mode)
    },
  },
})

/* 开发期热更新接管：改本 store 后无需手动刷新页面（Pinia 官方推荐写法） */
if (import.meta.hot) {
  import.meta.hot.accept(acceptHMRUpdate(useThemeStore, import.meta.hot))
}
