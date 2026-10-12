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
import { onBeforeUnmount, onMounted, ref } from 'vue'
import { buildEmbedSrcdoc } from '@/services/st/embed'
import { handleEmbedCall } from '@/services/st/embedBridge'

const props = defineProps<{
  /** 完整 HTML 文档（extractEmbedHtml 的产出） */
  html: string
}>()

const frame = ref<HTMLIFrameElement | null>(null)
const height = ref(160)

/** 桥接引导脚本：挂 TavernHelper shim + 高度上报（运行在 iframe 内） */
const BOOTSTRAP = `
(function () {
  var seq = 0
  var pending = {}
  function call(api, args) {
    return new Promise(function (resolve, reject) {
      var id = ++seq
      pending[id] = { resolve: resolve, reject: reject }
      parent.postMessage({ source: 'tavern-embed', type: 'th-call', id: id, api: api, args: args }, '*')
      window.setTimeout(function () {
        if (pending[id]) { delete pending[id]; reject(new Error('调用超时：' + api)) }
      }, 120000)
    })
  }
  window.addEventListener('message', function (ev) {
    var d = ev.data
    if (!d || d.source !== 'tavern-host' || d.type !== 'th-result') return
    var p = pending[d.id]
    if (!p) return
    delete pending[d.id]
    if (d.error) p.reject(new Error(d.error))
    else p.resolve(d.result)
  })
  var api = {
    getChatMessages: function (range) { return call('getChatMessages', { range: range }) },
    getVariables: function () { return call('getVariables', {}) },
    replaceVariables: function (value) { return call('replaceVariables', { value: value }) },
    insertVariables: function (value) { return call('insertVariables', { value: value }) },
    triggerSlash: function (cmds) { return call('triggerSlash', { cmds: cmds }) },
  }
  window.TavernHelper = api
  Object.keys(api).forEach(function (k) { window[k] = api[k] })
  function postHeight() {
    parent.postMessage({ source: 'tavern-embed', type: 'th-resize', height: document.documentElement.scrollHeight }, '*')
  }
  if (window.ResizeObserver) new ResizeObserver(postHeight).observe(document.documentElement)
  window.addEventListener('load', postHeight)
  document.addEventListener('DOMContentLoaded', postHeight)
})()
`

const srcdoc = buildEmbedSrcdoc(props.html, BOOTSTRAP)

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
  if (d.type === 'th-call') {
    const { id, api, args } = ev.data as { id: number; api: string; args: unknown }
    handleEmbedCall(api, args)
      .then((result) => {
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
  <iframe
    ref="frame"
    class="embed-frame"
    sandbox="allow-scripts allow-popups allow-forms"
    :srcdoc="srcdoc"
    :style="{ height: height + 'px' }"
  />
</template>

<style scoped>
.embed-frame {
  display: block;
  width: 100%;
  border: 0;
  border-radius: var(--radius-md);
  background: transparent;
}
</style>
