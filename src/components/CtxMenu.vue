<script setup lang="ts">
/**
 * 全局右键菜单 —— 唯一渲染实例（挂载在 App.vue）
 *
 * 关闭时机：点击菜单外任意处 / 滚动 / 窗口缩放失焦 / Esc。
 * 菜单项自身点击不走外点关闭（closest 判断豁免），保证 pick 回调可达。
 */
import { computed, onMounted, onUnmounted } from 'vue'
import { useCtxMenuStore } from '@/stores/ctxmenu'

const ctx = useCtxMenuStore()

/** 估算尺寸用于防溢出（项高约 34px，宽 148px）。
 * ⚠ Vue 3 的 style 绑定不会像 React 那样自动补 px —— 数字会被当成无单位非法值丢弃，
 * 菜单会掉到文档流末尾（视口外），表现为「右键菜单不显示」。必须手动带单位。 */
const MENU_W = 148
const pos = computed(() => {
  const h = ctx.items.length * 34 + 10
  return {
    left: `${Math.max(4, Math.min(ctx.x, window.innerWidth - MENU_W - 8))}px`,
    top: `${Math.max(4, Math.min(ctx.y, window.innerHeight - h - 8))}px`,
  }
})

/** 外点关闭（捕获阶段）；点在菜单内则豁免 */
function insideMenu(e: MouseEvent): boolean {
  return !!(e.target as Element | null)?.closest?.('.ctx-menu')
}
function onWinClick(e: MouseEvent) {
  if (!ctx.open || insideMenu(e)) return
  ctx.close()
}
/** 右键不触发 click，必须单独监听 —— 否则右键别处时旧菜单残留 */
function onWinCtx(e: MouseEvent) {
  if (!ctx.open || insideMenu(e)) return
  ctx.close()
}
function onKey(e: KeyboardEvent) {
  if (ctx.open && e.key === 'Escape') ctx.close()
}
/** 滚动 / 缩放 / 失焦一律关闭，避免菜单悬空错位 */
function onDismiss() {
  if (ctx.open) ctx.close()
}

onMounted(() => {
  window.addEventListener('click', onWinClick, true)
  window.addEventListener('contextmenu', onWinCtx, true)
  window.addEventListener('keydown', onKey)
  window.addEventListener('scroll', onDismiss, true)
  window.addEventListener('resize', onDismiss)
  window.addEventListener('blur', onDismiss)
})
onUnmounted(() => {
  window.removeEventListener('click', onWinClick, true)
  window.removeEventListener('contextmenu', onWinCtx, true)
  window.removeEventListener('keydown', onKey)
  window.removeEventListener('scroll', onDismiss, true)
  window.removeEventListener('resize', onDismiss)
  window.removeEventListener('blur', onDismiss)
})
</script>

<template>
  <Teleport to="body">
    <div v-if="ctx.open" class="ctx-menu" :style="pos" @contextmenu.prevent>
      <button
        v-for="it in ctx.items"
        :key="it.id"
        class="ctx-item"
        :class="{ danger: it.danger }"
        :disabled="it.disabled"
        @click="ctx.pick(it.id)"
      >
        {{ it.label }}
      </button>
    </div>
  </Teleport>
</template>

<style scoped>
.ctx-menu {
  position: fixed;
  z-index: 1000;
  min-width: 148px;
  padding: 5px;
  background: var(--c-bg);
  border: 1px solid var(--c-border);
  border-radius: var(--radius-md);
  box-shadow: var(--c-shadow-strong);
  user-select: none;
}
.ctx-item {
  display: block;
  width: 100%;
  padding: 7px 10px;
  font-size: 12px;
  text-align: left;
  border-radius: var(--radius-sm);
  color: var(--c-text);
}
.ctx-item:hover:not(:disabled) {
  background: var(--c-panel);
}
.ctx-item.danger {
  color: var(--s-error);
}
.ctx-item.danger:hover:not(:disabled) {
  background: var(--s-error-bg);
}
.ctx-item:disabled {
  color: var(--c-text-3);
  cursor: not-allowed;
}
</style>
