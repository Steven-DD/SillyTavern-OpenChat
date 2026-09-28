<script setup lang="ts">
/**
 * 全局错误提示条（各 store 的 `error` 字段此前零消费，落盘/删除/写回失败全部静默）
 *
 * 监听各 store 的 `error`（持久型错误标志，区别于聊天气泡的 lastError）
 * 非空即弹出 toast，点击关闭，6 秒自动消失。同一条错误不重复弹
 * （error 字段被成功操作清空后再次写入才会重新触发）。
 */
import { ref, watch } from 'vue'
import { useChatStore } from '@/stores/chat'
import { useWorldsStore } from '@/stores/worlds'
import { useCharacterStore } from '@/stores/character'

const chat = useChatStore()
const worlds = useWorldsStore()
const chars = useCharacterStore()

const visible = ref(false)
const text = ref('')
let hideTimer: number | undefined
/** 去重：同一批 store 里连续写入的相同内容不重复弹 */
let lastShown = ''

function show(msg: string): void {
  if (!msg || msg === lastShown) return
  lastShown = msg
  text.value = msg
  visible.value = true
  if (hideTimer !== undefined) clearTimeout(hideTimer)
  hideTimer = window.setTimeout(() => {
    visible.value = false
  }, 6000)
}

function dismiss(): void {
  visible.value = false
  if (hideTimer !== undefined) clearTimeout(hideTimer)
}

watch(
  () => [chat.error, worlds.error, chars.error] as const,
  ([a, b, c]) => {
    // 取最近一条非空错误（多个同时置位时展示最后写入的，顺序无感即可）
    show(c || b || a)
  },
)
</script>

<template>
  <Transition name="toast">
    <div
      v-if="visible"
      class="toast"
      role="alert"
      title="点击关闭"
      @click="dismiss"
    >
      <span class="msg">{{ text }}</span>
      <span class="x">✕</span>
    </div>
  </Transition>
</template>

<style scoped>
.toast {
  position: fixed;
  left: 50%;
  bottom: 34px;
  transform: translateX(-50%);
  z-index: 999;
  display: flex;
  align-items: center;
  gap: 10px;
  max-width: min(72vw, 640px);
  padding: 9px 16px;
  border-radius: 8px;
  background: var(--c-danger, #c0392b);
  color: #fff;
  font-size: 13px;
  line-height: 1.5;
  box-shadow: 0 6px 20px rgb(0 0 0 / 25%);
  cursor: pointer;
  user-select: none;
}
.msg {
  flex: 1;
  word-break: break-all;
}
.x {
  flex: none;
  opacity: 0.75;
}
.toast-enter-active,
.toast-leave-active {
  transition: opacity 0.18s ease, transform 0.18s ease;
}
.toast-enter-from,
.toast-leave-to {
  opacity: 0;
  transform: translateX(-50%) translateY(8px);
}
</style>
