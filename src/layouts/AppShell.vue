<script setup lang="ts">
/**
 * 应用布局壳 —— 唯一布局容器
 *
 * ── 形态（断点 lg=900px，与 stores/layout.ts 的 BREAKPOINT 一致）──
 * 宽屏：左导航(60) + 中栏列表(230) + 右栏内容（三栏并列 → flex-row）
 * 窄屏：导航变底部 Tab，主体**单栏** —— 列表与内容二选一（v-show 互斥）
 *
 * 布局尺寸全部走 UnoCSS 原子类（`lg:` 前缀 = 宽屏）：
 * 这里只管「谁显示」，不再管「多宽」——宽度定义在 uno.config 的
 * `pane-mid` / `pane-main` / `rail-*` shortcuts 里，保证两处断点不会走偏。
 */
import { onMounted, onUnmounted } from 'vue'
import RailNav from '@/components/RailNav.vue'
import GlobalToast from '@/components/GlobalToast.vue'
import { useLayoutStore } from '@/stores/layout'

const layout = useLayoutStore()

let stop: (() => void) | null = null
onMounted(() => {
  stop = layout.watchWidth()
})
onUnmounted(() => {
  stop?.()
})

/** 切换导航项时回到列表（否则从「设置」切到「会话」会停在旧的内容页） */
function onNavChange(): void {
  layout.showList()
}
</script>

<template>
  <div class="flex h-full flex-col lg:flex-row">
    <RailNav @change="onNavChange" />

    <!--
      宽屏：中栏与右栏并列（lg:flex-row）—— 冻结版三栏骨架的第二、三栏。
      ⚠ 这里曾是纯 flex-col，导致宽屏下中栏叠在右栏上方（UI 全乱的根因之一）。
      窄屏：恢复单栏堆叠，rail 靠 order 沉到底部（见 RailNav 的 order-last）。
    -->
    <div class="flex flex-1 flex-col min-w-0 min-h-0 lg:flex-row">
      <!-- 窄屏内容页：统一返回条 -->
      <div
        v-if="layout.isNarrow && layout.detail"
        class="flex-none flex items-center gap-2 px-10px py-6px border-b border-t-0 border-l-0 border-r-0 border-solid border-line bg-canvas"
      >
        <button class="btn" @click="layout.showList()">
          <span class="inline-flex items-center gap-3px">
            <svg
              viewBox="0 0 24 24"
              width="14"
              height="14"
              fill="none"
              stroke="currentColor"
              stroke-width="2"
              stroke-linecap="round"
              stroke-linejoin="round"
            >
              <path d="M15 18l-6-6 6-6" />
            </svg>
            返回
          </span>
        </button>
      </div>

      <!-- 中栏：窄屏进入内容页后隐藏；窄屏下 flex-1 占满高度，宽屏固定 230px -->
      <div v-show="!layout.isNarrow || !layout.detail" class="pane-mid flex-1 lg:flex-none">
        <router-view name="mid" />
      </div>

      <!-- 右栏：窄屏未选中任何项时隐藏 -->
      <div v-show="!layout.isNarrow || layout.detail" class="pane-main">
        <router-view name="main" />
      </div>
    </div>

    <!-- 全局错误提示（store error 字段的统一出口：落盘/删除/写回失败不再静默） -->
    <GlobalToast />
  </div>
</template>
