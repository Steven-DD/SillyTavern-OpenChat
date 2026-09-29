/**
 * 右键菜单（全局单例）
 *
 * 任意列表/元素通过 `ctx.show(e, items, onPick)` 唤起，
 * 由挂载在 App 根部的 <CtxMenu/> 渲染。同一时刻最多一个菜单。
 * 浏览器默认菜单由 App.vue 的全局 contextmenu 监听统一屏蔽（输入框除外）。
 */
import { defineStore, acceptHMRUpdate } from 'pinia'

export interface CtxMenuItem {
  id: string
  label: string
  /** 危险操作（红色） */
  danger?: boolean
  disabled?: boolean
}

export const useCtxMenuStore = defineStore('ctxmenu', {
  state: () => ({
    open: false,
    x: 0,
    y: 0,
    items: [] as CtxMenuItem[],
    onPick: null as ((id: string) => void) | null,
  }),

  actions: {
    /** 在鼠标位置唤起菜单（内部 preventDefault，屏蔽浏览器默认菜单） */
    show(e: MouseEvent, items: CtxMenuItem[], onPick: (id: string) => void) {
      e.preventDefault()
      this.open = true
      this.x = e.clientX
      this.y = e.clientY
      this.items = items
      this.onPick = onPick
    },

    close() {
      this.open = false
      this.onPick = null
    },

    pick(id: string) {
      const cb = this.onPick
      this.close() // 先关再回调，回调里打开确认态/输入态不会被菜单遮挡
      cb?.(id)
    },
  },
})

if (import.meta.hot) {
  import.meta.hot.accept(acceptHMRUpdate(useCtxMenuStore, import.meta.hot))
}
