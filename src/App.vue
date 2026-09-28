<script setup lang="ts">
/**
 * 应用根组件 —— 启动门卫（Boot Gate）
 *
 * ST 已改为纯后端（不开网页），用户在 ST 就绪前看不到任何线索。
 * 所以：桌面环境下，等 sidecar 报 ready 再放行主界面；
 * 加载中/失败由 BootScreen 呈现（含重试）。浏览器开发环境直接放行。
 *
 * `stopped`（数据迁移暂停 ST）只发生在启动之后的用户操作里，
 * 那时主界面已进入、状态条会接管提示，这里不得再拦。
 */
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import AppShell from '@/layouts/AppShell.vue'
import BootScreen from '@/components/BootScreen.vue'
import CtxMenu from '@/components/CtxMenu.vue'
import { sidecar } from '@/services/tauri/bridge'
import { useBackgroundStore } from '@/stores/background'

const router = useRouter()
const forcing = ref(false)
const bg = useBackgroundStore()

/**
 * 全局屏蔽浏览器默认右键菜单（桌面应用质感）。
 * 输入框/可编辑元素保留原生菜单（复制粘贴实用）；
 * 业务右键菜单由各列表自行唤起（CtxMenu），两者互不冲突。
 */
function onGlobalCtx(e: MouseEvent): void {
  const t = e.target as Element | null
  if (t?.closest?.('input, textarea, [contenteditable="true"]')) return
  e.preventDefault()
}
onMounted(() => {
  window.addEventListener('contextmenu', onGlobalCtx)
  bg.apply() // 恢复聊天背景（无选择则移除变量）
})
onUnmounted(() => window.removeEventListener('contextmenu', onGlobalCtx))

const blocked = computed(() => {
  if (forcing.value) return false
  // 浏览器 dev（无 Tauri 壳）：不拦
  if (!sidecar.isDesktop) return false
  if (sidecar.ready) return false
  return sidecar.status?.state !== 'stopped'
})

/**
 * 「仍要进入」：ST 未就绪（典型如「未找到 SillyTavern」）时强行放行，
 * 直达插件页 —— 那里有状态详情、手动指定路径与重新检测，就地修复。
 */
function force(): void {
  forcing.value = true
  if (!sidecar.ready) {
    void router.replace('/plugins')
  }
}
</script>

<template>
  <BootScreen v-if="blocked" @force="force" />
  <AppShell v-else />
  <CtxMenu />
</template>
