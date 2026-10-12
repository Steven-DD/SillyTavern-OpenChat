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
/** 卡内脚本错误（上报展示，最多留 3 条） */
const errors = ref<string[]>([])

/** 桥接引导脚本：挂 TavernHelper shim + 高度上报（运行在 iframe 内） */
const BOOTSTRAP = `
(function () {
  function reportErr(kind, msg, where) {
    try { parent.postMessage({ source: 'tavern-embed', type: 'th-error', msg: String(msg), where: String(where ?? '') }, '*') } catch (e) { /* ignore */ }
  }
  // 沙箱无 allow-same-origin → localStorage/sessionStorage 访问必抛 SecurityError，
  // 大量卡脚本（日志/配置存取）会因此中断。垫内存版兜底（不跨帧持久，同酒馆助手语义）。
  function makeStorage() {
    var store = {}
    return {
      getItem: function (k) { return Object.prototype.hasOwnProperty.call(store, String(k)) ? store[String(k)] : null },
      setItem: function (k, v) { store[String(k)] = String(v) },
      removeItem: function (k) { delete store[String(k)] },
      clear: function () { store = {} },
      key: function (i) { return Object.keys(store)[i] ?? null },
      get length() { return Object.keys(store).length },
    }
  }
  try {
    window.localStorage.getItem('__probe__')
  } catch (e) {
    try {
      Object.defineProperty(window, 'localStorage', { value: makeStorage(), configurable: true })
      Object.defineProperty(window, 'sessionStorage', { value: makeStorage(), configurable: true })
    } catch (e2) { /* 极老内核忽略 */ }
  }
  window.addEventListener('error', function (e) {
    reportErr('error', e.message, (e.filename || '') + ':' + (e.lineno ?? ''))
  })
  window.addEventListener('unhandledrejection', function (e) {
    var r = e.reason
    reportErr('promise', r && r.message ? r.message : String(r), '')
  })
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
