import { defineStore, acceptHMRUpdate } from 'pinia'

/**
 * 外观偏好：字号档位 / 字体族 / 语言。
 * 与 theme store 同一模式：状态 + apply() 直接落到 document，
 * 偏好持久化 localStorage（key: app.appearance）。
 *
 * 字号实现说明：全站组件字号都是显式 px（紧凑排版），rem 缩放不生效，
 * 所以用 html 的 CSS `zoom`（WebView2/Chromium 标准属性）整体等比缩放。
 */
export const useAppearanceStore = defineStore('appearance', {
  state: () => ({
    /** 字号档位：sm 0.93 / md 1.0 / lg 1.07 / xl 1.13 */
    fontScale: 'md' as FontScale,
    /** 字体族 CSS 值；'' = 系统默认（走 base.css 的默认栈） */
    fontFamily: '',
    /** 语言偏好（i18n 落地前仅存储，界面暂只有中文） */
    lang: 'zh-CN' as Lang,
  }),

  getters: {
    /** zoom 值（保留两位避免浮点误差） */
    zoom(): number {
      return SCALE_ZOOM[this.fontScale] ?? 1
    },
  },

  actions: {
    setFontScale(v: FontScale) {
      this.fontScale = v
      this.apply()
    },
    setFontFamily(v: string) {
      this.fontFamily = v
      this.apply()
    },
    setLang(v: Lang) {
      this.lang = v
      this.persist()
    },
    apply() {
      const root = document.documentElement
      root.style.zoom = String(this.zoom)
      root.style.setProperty('--app-font-family', this.fontFamily || 'var(--font-default)')
      this.persist()
    },
    persist() {
      localStorage.setItem(
        'app.appearance',
        JSON.stringify({ fontScale: this.fontScale, fontFamily: this.fontFamily, lang: this.lang }),
      )
    },
    /** 启动时恢复偏好并应用 */
    init() {
      try {
        const raw = localStorage.getItem('app.appearance')
        if (raw) Object.assign(this, JSON.parse(raw))
      } catch {
        /* 坏数据按默认值走 */
      }
      this.apply()
    },
  },
})

export type FontScale = 'sm' | 'md' | 'lg' | 'xl'
export type Lang = 'zh-CN' | 'en-US'

export const SCALE_ZOOM: Record<FontScale, number> = {
  sm: 0.93,
  md: 1.0,
  lg: 1.07,
  xl: 1.13,
}

/* 开发期热更新接管 */
if (import.meta.hot) {
  import.meta.hot.accept(acceptHMRUpdate(useAppearanceStore, import.meta.hot))
}
