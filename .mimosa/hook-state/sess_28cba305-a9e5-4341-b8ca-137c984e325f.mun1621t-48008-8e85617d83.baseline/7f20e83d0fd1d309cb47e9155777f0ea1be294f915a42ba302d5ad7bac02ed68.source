/**
 * 聊天背景状态：选择来自 ST backgrounds 目录的图片作为聊天页背景。
 * 持久化 localStorage 'app.bg'；应用方式 = 写根节点 CSS 变量 --page-chats-image
 * （base.css：.page[data-page=chats] { --page-bg-image: var(--page-chats-image) }）。
 */
import { defineStore, acceptHMRUpdate } from 'pinia'
import { backgroundUrl, listBackgrounds, type BackgroundItem } from '@/services/st/backgrounds'

const KEY = 'app.bg'

interface BgState {
  name: string
}

function load(): BgState {
  try {
    return { name: JSON.parse(localStorage.getItem(KEY) ?? '""') as string }
  } catch {
    return { name: '' }
  }
}

export const useBackgroundStore = defineStore('background', {
  state: () => ({
    ...load(),
    list: [] as BackgroundItem[],
    loading: false,
    error: '',
  }),

  getters: {
    enabled: (state) => !!state.name,
  },

  actions: {
    persist() {
      localStorage.setItem(KEY, JSON.stringify(this.name))
    },

    /** 把当前选择应用到根节点 CSS 变量 */
    apply() {
      const root = document.documentElement
      if (this.name) {
        root.style.setProperty('--page-chats-image', `url("${backgroundUrl(this.name)}")`)
      } else {
        root.style.removeProperty('--page-chats-image')
      }
    },

    async loadList(force = false) {
      if (this.loading) return
      if (this.list.length && !force) return
      this.loading = true
      this.error = ''
      try {
        this.list = await listBackgrounds()
      } catch (e) {
        this.error = e instanceof Error ? e.message : String(e)
      } finally {
        this.loading = false
      }
    },

    /** 应用指定背景（空串 = 不使用）；持久化并即时生效 */
    async choose(name: string) {
      this.name = name
      this.persist()
      this.apply()
      if (name) await this.loadList()
    },
  },
})

if (import.meta.hot) {
  import.meta.hot.accept(acceptHMRUpdate(useBackgroundStore, import.meta.hot))
}
