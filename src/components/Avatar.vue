<script setup lang="ts">
/**
 * 头像（带兜底）
 *
 * 优先加载 ST 缩略图；文件名为空 / 请求失败时降级为「名称首字母」色块，
 * 避免出现破图或空白（ST 未就绪，头像文件缺失，中继不通都会走到兜底）。
 */
import { computed, ref, watch } from 'vue'
import { avatarThumb } from '@/services/st/data'

const props = defineProps<{
  /** ST 头像文件名；为空直接走占位 */
  avatar?: string
  /** 角色/会话名，用于占位首字母与无障碍标签 */
  name?: string
}>()

const failed = ref(false)
const src = computed(() => (props.avatar ? avatarThumb(props.avatar) : ''))

// 换角色时要重新给一次机会，否则上次的失败态会一直挂着
watch(
  () => props.avatar,
  () => {
    failed.value = false
  },
)

const initial = computed(() => {
  const s = (props.name ?? '').trim()
  if (!s) return '?'
  // 用 Array.from 取首字符，避免 emoji / 生僻字被拆成半个代理对
  return Array.from(s)[0]!.toUpperCase()
})
</script>

<template>
  <img v-if="src && !failed" class="face" :src="src" :alt="name ?? ''" @error="failed = true" />
  <div v-else class="face ph" role="img" :aria-label="name ?? '头像'">{{ initial }}</div>
</template>

<style scoped>
.face {
  display: block;
  object-fit: cover;
  background: var(--c-panel);
}

.ph {
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--c-panel);
  color: var(--c-text-2);
  font-size: 13px;
  font-weight: 500;
  user-select: none;
}
</style>
