<script setup lang="ts">
/**
 * 前端界面沙箱帧：把卡内 HTML 应用渲染进 sandbox="allow-scripts" 的 iframe。
 *
 * - srcdoc = 卡 HTML + 桥接引导脚本（buildEmbedSrcdoc 注入 <head>）
 * - 卡脚本通过 shim（TavernHelper / 同名全局函数）postMessage 调宿主，
 *   宿主侧分发在 services/st/embedBridge.ts（白名单 API）
 * - 沙箱不给 allow-same-origin：卡脚本拿不到宿主源/cookie，
 *   相对路径请求也不可用，数据只能走桥
 * - 高度自适应：帧内 ResizeObserver 上报 scrollHeight，宿主随之调高
 */
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { buildEmbedSrcdoc, SANDBOX_BOOTSTRAP } from '@/services/st/embed'
import { handleEmbedCall } from '@/services/st/embedBridge'

const props = defineProps<{
  /** 完整 HTML 文档（extractEmbedHtml 的产出） */
  html: string
}>()

const frame = ref<HTMLIFrameElement | null>(null)
const height = ref(160)
/** 卡内脚本错误（上报展示，最多留 3 条） */
const errors = ref<string[]>([])


// ⚠ 必须 computed：组件随气泡列表复用（key=索引），props.html 变化时
//    srcdoc 若是 setup 常量不会更新——表现为"切换会话后界面还是旧的那份"
const srcdoc = computed(() => buildEmbedSrcdoc(props.html, SANDBOX_BOOTSTRAP))

function onMessage(ev: MessageEvent): void {
  const w = frame.value?.contentWindow
  if (!w || ev.source !== w) return
  const d = ev.data as { source?: string; type?: string } | null
  if (!d || d.source !== 'tavern-embed') return
  if (d.type === 'th-resize') {
    const h = (ev.data as { height?: number }).height
    if (typeof h === 'number' && h > 0) height.value = Math.min(Math.max(h, 60), 4000)
    return
  }
  if (d.type === 'th-error') {
    const { msg, where } = ev.data as { msg?: string; where?: string }
    const line = `[前端界面] ${msg ?? '未知错误'}${where ? `（${where}）` : ''}`
    console.warn(line)
    errors.value = [...errors.value.slice(-2), line]
    return
  }
  if (d.type === 'th-call') {
    const { id, api, args } = ev.data as { id: number; api: string; args: unknown }
    handleEmbedCall(api, args)
      .then((result) => {
        // ⚠ targetOrigin 恒 '*'：沙箱帧（无 allow-same-origin）的 origin 是
        // opaque null，任何具体 origin 都不匹配会静默丢消息 —— 安全面由
        // 接收侧 ev.source === w 校验 + iframe sandbox 封住（P1-2 评审项结论）
        w.postMessage({ source: 'tavern-host', type: 'th-result', id, result }, '*')
      })
      .catch((e: unknown) => {
        w.postMessage(
          { source: 'tavern-host', type: 'th-result', id, error: e instanceof Error ? e.message : String(e) },
          '*',
        )
      })
  }
}

onMounted(() => window.addEventListener('message', onMessage))
onBeforeUnmount(() => window.removeEventListener('message', onMessage))
</script>

<template>
  <div class="embed-wrap">
    <div v-if="errors.length" class="embed-errs">
      <span v-for="e in errors" :key="e" class="embed-err">{{ e }}</span>
    </div>
    <iframe
      ref="frame"
      class="embed-frame"
      sandbox="allow-scripts allow-popups allow-forms"
      :srcdoc="srcdoc"
      :style="{ height: height + 'px' }"
    />
  </div>
</template>

<style scoped>
.embed-wrap {
  min-width: 0;
}
.embed-frame {
  display: block;
  width: 100%;
  border: 0;
  border-radius: var(--radius-md);
  background: transparent;
}
.embed-errs {
  display: flex;
  flex-direction: column;
  gap: 2px;
  margin-bottom: 4px;
}
.embed-err {
  font-size: 11px;
  color: var(--s-error, #c0392b);
  word-break: break-all;
}
</style>
