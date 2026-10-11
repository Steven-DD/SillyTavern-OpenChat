<script setup lang="ts">
/**
 * `<thinking>` 自定义标签渲染（风月思维链"折叠可见"形态）。
 *
 * markstream 解析器在 customHtmlTags 中注册 'thinking' 后，会把完整的
 * <thinking>...</thinking> 块收拢为一个自定义节点（流式中未闭合的半截标签被
 * 抑制，不会闪现），再把节点交给本组件渲染。
 *
 * props.node 形如 { type:'thinking', tag:'thinking', content, attrs, loading? }，
 * content 为标签内部原始 Markdown 文本，用嵌套 MarkdownRender 渲染。
 */
import { computed, ref } from 'vue'
import { MarkdownRender } from 'markstream-vue'

const props = defineProps<{
  node: {
    tag?: string
    content?: string
    attrs?: Record<string, string> | null
    loading?: boolean
  }
}>()

const open = ref(false)
const title = computed(() => props.node.attrs?.title || '思维链')
</script>

<template>
  <div class="thinking" :class="{ open }">
    <button class="thinking-head" type="button" @click="open = !open">
      <span class="thinking-icon">✦</span>
      <span class="thinking-title">{{ title }}</span>
      <span class="thinking-arrow" :class="{ open }">▾</span>
    </button>
    <div v-show="open" class="thinking-body">
      <MarkdownRender :content="node.content ?? ''" mode="chat" final />
    </div>
  </div>
</template>

<style scoped>
.thinking {
  margin: 6px 0;
  border: 1px solid var(--c-border);
  border-radius: var(--radius-md);
  background: var(--c-panel-2, var(--c-panel));
  overflow: hidden;
}
.thinking-head {
  display: flex;
  align-items: center;
  gap: 6px;
  width: 100%;
  padding: 6px 10px;
  font-size: 11px;
  color: var(--c-text-2);
  text-align: left;
}
.thinking-head:hover {
  color: var(--c-text);
}
.thinking-icon {
  color: var(--p-400, var(--c-text-3));
}
.thinking-title {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.thinking-arrow {
  transition: transform 0.15s;
}
.thinking-arrow.open {
  transform: rotate(180deg);
}
.thinking-body {
  max-height: 260px;
  overflow-y: auto;
  padding: 2px 12px 8px;
  font-size: 12px;
  line-height: 1.7;
  color: var(--c-text-2);
  border-top: 1px dashed var(--c-border);
}
.thinking-body :deep(p) {
  margin: 6px 0;
}
</style>
