<script setup lang="ts">
/**
 * 启动屏（Boot Gate 的展示层）
 *
 * ST 不再开网页，用户无从得知后端是否就绪 —— 所以在 ST 就绪前用本屏挡住主界面，
 * 并把 sidecar 的实时状态亮出来。
 *
 * 「未找到 SillyTavern」不摆按钮等用户点：提示后自动进入插件页
 * （那里可下载安装 / 手动指定目录）。其余错误（如启动超时）才保留重试入口。
 */
import { computed, onUnmounted, ref, watch } from 'vue'
import { sidecar } from '@/services/tauri/bridge'

const emit = defineEmits<{ (e: 'force'): void }>()

const retrying = ref(false)

const message = computed(() => sidecar.status?.message || '正在启动 SillyTavern…')

/** ST 本机未找到（探测失败）——自动进入，不打断用户 */
const stMissing = computed(
  () =>
    sidecar.status?.state === 'error' &&
    (sidecar.status?.message ?? '').includes('未找到'),
)

let autoTimer: ReturnType<typeof setTimeout> | null = null
watch(
  stMissing,
  (v) => {
    if (v && autoTimer === null) {
      autoTimer = setTimeout(() => emit('force'), 1600)
    }
  },
  { immediate: true },
)
onUnmounted(() => {
  if (autoTimer !== null) clearTimeout(autoTimer)
})

async function retry(): Promise<void> {
  retrying.value = true
  try {
    const { invoke } = await import('@tauri-apps/api/core')
    await invoke('st_retry_start')
  } catch (e) {
    console.warn('[boot] 重试失败:', e)
  } finally {
    retrying.value = false
  }
}
</script>

<template>
  <div class="boot" role="status" aria-live="polite">
    <div class="card">
      <div class="mark" aria-hidden="true">
        <span class="spin-ring"></span>
        <span class="letter">S</span>
      </div>

      <h1 class="title">ST Chat</h1>
      <p class="msg">{{ message }}</p>

      <div class="detail">
        <span>端口 {{ sidecar.status?.port ?? '—' }}</span>
        <span v-if="sidecar.status?.pid">pid {{ sidecar.status.pid }}</span>
      </div>

      <template v-if="sidecar.status?.state === 'error'">
        <!-- 未找到：主动提示 + 自动进入插件页（下载安装 / 手动指定） -->
        <p v-if="stMissing" class="hint">
          即将进入插件页：可在此下载安装 SillyTavern，或在详情里手动指定本机已有的目录。
        </p>
        <!-- 其余错误（如启动超时）：保留重试逃生门 -->
        <template v-else>
          <div class="actions">
            <button class="btn primary" :disabled="retrying" @click="retry">
              {{ retrying ? '正在重试…' : '重试启动' }}
            </button>
          </div>
          <p class="hint">正在等待 SillyTavern 就绪失败，可稍后重试。</p>
        </template>
      </template>
      <p v-else class="hint">首次启动可能需要十几秒，正在等待 SillyTavern 就绪…</p>
    </div>
  </div>
</template>

<style scoped>
.boot {
  position: fixed;
  inset: 0;
  z-index: 9999;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--c-bg);
}

.card {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 12px;
  padding: 40px 48px;
  border-radius: 16px;
  background: var(--c-panel);
  box-shadow: var(--c-shadow);
  min-width: 320px;
  text-align: center;
}

.mark {
  position: relative;
  width: 56px;
  height: 56px;
  margin-bottom: 4px;
}

.spin-ring {
  position: absolute;
  inset: 0;
  border-radius: 50%;
  border: 3px solid var(--c-border);
  border-top-color: var(--p-500);
  animation: spin 0.9s linear infinite;
}

.letter {
  position: absolute;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 22px;
  font-weight: 700;
  color: var(--p-500);
}

@keyframes spin {
  to {
    transform: rotate(360deg);
  }
}

.title {
  margin: 0;
  font-size: 17px;
  font-weight: 600;
  color: var(--c-text);
}

.msg {
  margin: 0;
  font-size: 13px;
  color: var(--c-text-2);
}

.detail {
  display: flex;
  gap: 12px;
  font-size: 12px;
  color: var(--c-text-3);
}

.actions {
  display: flex;
  gap: 10px;
  margin-top: 6px;
}

.btn {
  padding: 7px 18px;
  border: 1px solid var(--c-border);
  border-radius: 8px;
  background: transparent;
  font-size: 13px;
  color: var(--c-text);
  cursor: pointer;
}

.btn:hover {
  background: var(--c-panel);
}

.btn.primary {
  background: var(--p-500);
  border-color: var(--p-500);
  color: var(--c-on-primary);
}

.hint {
  margin: 2px 0 0;
  max-width: 300px;
  font-size: 12px;
  line-height: 1.5;
  color: var(--c-text-3);
}
</style>
