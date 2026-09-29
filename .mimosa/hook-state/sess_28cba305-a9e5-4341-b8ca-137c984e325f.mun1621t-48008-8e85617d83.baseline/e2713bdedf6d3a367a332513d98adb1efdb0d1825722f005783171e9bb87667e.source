import { defineStore, acceptHMRUpdate } from 'pinia'

export type ThemeMode = 'light' | 'dark' | 'dusk'

/**
 * 主题状态 + 皮肤通道。
 * M1 内置三主题；后续皮肤包 = theme.json（仅覆盖语义 token）
 * 经 loadSkin 应用到 :root CSS 变量，与本 store 的 apply 同一通道。
 */
export const useThemeStore = defineStore('theme', {
  state: () => ({
    mode: 'light' as ThemeMode,
  }),
  actions: {
    setMode(mode: ThemeMode) {
      this.mode = mode
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
