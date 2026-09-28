<script setup lang="ts">
/**
 * 中栏容器（标题 + 工具栏插槽 + 列表区）
 * 宽度与边框走 uno.config 的 `pane-mid` shortcut（窄屏铺满，宽屏 230px）
 * 这样断点只在一处定义，不会与 AppShell 走偏。
 */
defineProps<{
  title: string
  subtitle?: string
}>()
</script>

<template>
  <!--
    宽度与边框由 AppShell 的 .pane-mid 包裹层统一管（这里只负责填充与排版）。
    ⚠ 不要用 lg:(...) 变体组语法 —— 需要额外的 transformerVariantGroup，没装就是死类名（踩过）。
  -->
  <aside class="w-full flex-1 min-h-0 flex flex-col bg-canvas">
    <div class="px-14px pt-14px pb-8px flex-none">
      <h3 class="text-14px font-600">{{ title }}</h3>
      <div v-if="subtitle" class="text-11px color-ink-3 mt-2px">{{ subtitle }}</div>
      <!-- 供各入口放入搜索框 / 工具栏，保持「左固定+中列表+右内容」一致性 -->
      <slot name="head" />
    </div>
    <div class="flex-1 overflow-y-auto px-6px">
      <slot />
    </div>
  </aside>
</template>
