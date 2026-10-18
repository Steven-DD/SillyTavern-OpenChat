<script setup lang="ts">
/**
 * 通用确认弹窗（危险操作二次确认的统一形态）
 *
 * 用法：
 *   <ConfirmDialog :open="!!delTarget" title="删除会话" message="删除后不可恢复"
 *                  :busy="deleting" @confirm="doDelete" @cancel="delTarget = null" />
 *
 * 关闭路径：取消按钮 / 点遮罩 / Esc —— 均只发 cancel；确认按钮发 confirm。
 * Enter 确认不做（避免上一秒刚按下的回车残留在输入法里误触危险操作）。
 * 高风险操作可传 requireText：必须原样输入确认词，确认按钮才可用。
 */
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue'

const props = defineProps<{
  open: boolean
  title: string
  /** 正文（支持 \n 换行） */
  message: string
  confirmText?: string
  cancelText?: string
  /** 危险操作（确认按钮红色，默认 true） */
  danger?: boolean
  /** 确认进行中（按钮转圈禁用） */
  busy?: boolean
  /** 高风险二次确认：必须原样输入该确认词才能点确认（不传 = 沿用按钮确认） */
  requireText?: string
}>()

const emit = defineEmits<{ cancel: []; confirm: [] }>()

/** 确认词输入框（requireText 模式下生效；每次打开/关闭都复位） */
const typed = ref('')
const inputEl = ref<HTMLInputElement | null>(null)
const canConfirm = computed(
  () => !props.requireText || typed.value.trim() === props.requireText.trim(),
)

watch(
  () => props.open,
  (open) => {
    if (!open) {
      typed.value = ''
      return
    }
    if (props.requireText) void nextTick(() => inputEl.value?.focus())
  },
)

function onKey(e: KeyboardEvent) {
  if (!props.open) return
  if (e.key === 'Escape') emit('cancel')
}
onMounted(() => window.addEventListener('keydown', onKey))
onUnmounted(() => window.removeEventListener('keydown', onKey))
</script>

<template>
  <Teleport to="body">
    <Transition name="dlg">
      <div v-if="open" class="mask" @click.self="emit('cancel')" @contextmenu.prevent>
        <div class="dlg" role="alertdialog" aria-modal="true">
          <h5>{{ title }}</h5>
          <p class="msg">{{ message }}</p>
          <input
            v-if="requireText"
            ref="inputEl"
            v-model="typed"
            class="ctext"
            type="text"
            :disabled="busy"
            :placeholder="`输入「${requireText}」以继续`"
            @keydown.esc.stop="emit('cancel')"
          />
          <div class="btns">
            <button class="btn" :disabled="busy" @click="emit('cancel')">
              {{ cancelText ?? '取消' }}
            </button>
            <button
              class="btn"
              :class="danger ? 'btn-danger' : 'btn-primary'"
              :disabled="busy || !canConfirm"
              @click="emit('confirm')"
            >
              {{ busy ? '处理中…' : (confirmText ?? '确认') }}
            </button>
          </div>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
.mask {
  position: fixed;
  inset: 0;
  z-index: 1100;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--c-scrim);
}
.dlg {
  width: 300px;
  max-width: calc(100vw - 48px);
  padding: 16px 16px 14px;
  background: var(--c-bg);
  border: 1px solid var(--c-border);
  border-radius: var(--radius-lg);
  box-shadow: var(--c-shadow-strong);
}
h5 {
  font-size: 14px;
  font-weight: 600;
  color: var(--c-text);
}
.msg {
  margin-top: 8px;
  font-size: 12px;
  line-height: 1.7;
  color: var(--c-text-2);
  white-space: pre-line; /* 支持 \n 换行 */
  word-break: break-word;
}
.btns {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  margin-top: 16px;
}

.ctext {
  width: 100%;
  margin-top: 10px;
  padding: 6px 8px;
  font-size: 12px;
  color: var(--c-text);
  background: transparent;
  border: 1px solid var(--c-border);
  border-radius: 6px;
  outline: none;
}
.ctext:focus {
  border-color: var(--c-text-2);
}

/* 过渡 */
.dlg-enter-active,
.dlg-leave-active {
  transition: opacity 0.14s ease;
}
.dlg-enter-active .dlg,
.dlg-leave-active .dlg {
  transition: transform 0.14s ease;
}
.dlg-enter-from,
.dlg-leave-to {
  opacity: 0;
}
.dlg-enter-from .dlg,
.dlg-leave-to .dlg {
  transform: scale(0.96);
}
</style>
