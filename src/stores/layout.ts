/**
 * 布局状态（响应式适配）
 *
 * ── 为什么要它 ──
 * 桌面是三栏（左导航 + 中列表 + 右内容）。窗口一窄（手机 / 分屏）三栏就放不下，
 * 必须切换成微信手机版的形态：**单栏 + 列表↔内容二选一 + 导航变底部 Tab**。
 * 纯 CSS 做不到「二选一」（需要知道用户是否选中了某项），所以状态放这里。
 *
 * ── 两种模式 ──
 * - wide（≥ BREAKPOINT）：三栏全显示
 * - narrow（< BREAKPOINT）：单栏，由 `detail` 决定显示列表还是内容
 */
import { defineStore } from 'pinia'

/** 三栏所需的最小宽度（与 uno.config.ts 的 lg 断点一致） */
export const BREAKPOINT = 900

export const useLayoutStore = defineStore('layout', {
  state: () => ({
    /** 当前布局模式 */
    mode: 'wide' as 'wide' | 'narrow',
    /** narrow 模式下是否处于「内容页」（true = 显示右栏，false = 显示中栏列表） */
    detail: false,
  }),

  getters: {
    isNarrow(state): boolean {
      return state.mode === 'narrow'
    },
  },

  actions: {
    /** 进入内容页（中栏点选某项时调用；wide 模式下无副作用） */
    showDetail(): void {
      this.detail = true
    },

    /** 回到列表（内容页的返回按钮） */
    showList(): void {
      this.detail = false
    },

    /**
     * 监听窗口宽度更新模式。
     * 用 matchMedia 而不是 resize 事件：语义直接、无需自己算宽度、也不用手写防抖。
     */
    watchWidth(): () => void {
      const mq = window.matchMedia(`(min-width: ${BREAKPOINT}px)`)
      const apply = () => {
        this.mode = mq.matches ? 'wide' : 'narrow'
        // 回到宽屏时复位，避免下次变窄直接落在内容页
        if (this.mode === 'wide') this.detail = false
      }
      apply()
      mq.addEventListener('change', apply)
      return () => mq.removeEventListener('change', apply)
    },
  },
})
