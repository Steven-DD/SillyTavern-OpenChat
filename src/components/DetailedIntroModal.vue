<script setup lang="ts">
/**
 * 详细介绍沙箱预览（可复用）：iframe sandbox（allow-scripts）渲染风月卡
 * data.extensions.detailed_intro 的 HTML+JS。opaque origin 隔离——可交互，
 * 无存储/同源/顶层导航权限（对齐指南「详细介绍页」环境约束）。
 *
 * 使用方式（Teleport 自包含，v-if 由调用方控制或用 v-model）：
 *   <DetailedIntroModal v-if="open" :html="intro" :title="char.name" @close="open = false" />
 */
import { onUnmounted } from 'vue'

defineProps<{
  /** 详细介绍 HTML（detailed_intro 原文，整页文档或片段均可） */
  html: string
  /** 遮罩标题（默认「详细介绍」） */
  title?: string
}>()

const emit = defineEmits<{ close: [] }>()

function onKey(e: KeyboardEvent): void {
  if (e.key === 'Escape') emit('close')
}
window.addEventListener('keydown', onKey)
onUnmounted(() => window.removeEventListener('keydown', onKey))
</script>

<template>
  <Teleport to="body">
    <div class="intro-mask" @click.self="emit('close')">
      <div class="intro-dlg" role="dialog" aria-modal="true">
        <header class="intro-head">
          <span class="intro-tt">{{ title ?? '详细介绍' }}</span>
          <button class="intro-x" title="关闭" @click="emit('close')">×</button>
        </header>
        <iframe class="intro-frame" sandbox="allow-scripts" :srcdoc="html" />
      </div>
    </div>
  </Teleport>
</template>

<style scoped>
.intro-mask {
  position: fixed;
  inset: 0;
  z-index: 90;
  display: grid;
  place-items: center;
  padding: 24px;
  background: rgb(0 0 0 / 55%);
}
.intro-dlg {
  display: flex;
  flex-direction: column;
  width: min(860px, 100%);
  height: min(86vh, 900px);
  border-radius: var(--radius-lg);
  background: var(--c-bg);
  border: 1px solid var(--c-border);
  overflow: hidden;
}
.intro-head {
  flex: none;
  display: flex;
  align-items: center;
  padding: 10px 14px;
  border-bottom: 1px solid var(--c-border);
}
.intro-tt {
  font-size: 13px;
  font-weight: 600;
}
.intro-x {
  margin-left: auto;
  width: 24px;
  height: 24px;
  display: grid;
  place-items: center;
  border-radius: var(--radius-sm);
  color: var(--c-text-3);
  font-size: 15px;
}
.intro-x:hover {
  background: var(--c-panel);
  color: var(--c-text);
}
.intro-frame {
  flex: 1;
  width: 100%;
  border: 0;
  background: #fff;
}
</style>
