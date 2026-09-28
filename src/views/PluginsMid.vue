<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import MidPane from '@/components/MidPane.vue'
import { usePluginsStore } from '@/stores/plugins'
import { useLayoutStore } from '@/stores/layout'
import { useExtensionsStore } from '@/stores/extensions'
import type { PluginReport } from '@/services/tauri/plugins'
import { pluginStatusOf } from '@/services/plugin-status'

const plugins = usePluginsStore()
const ext = useExtensionsStore()
const layout = useLayoutStore()

/** 系统插件组：required 全部 + Git 等可选组件合并展示（不再单列「可选插件」分组） */
const stackMain = computed<PluginReport[]>(() => [
  ...plugins.required,
  ...plugins.optional,
])

/** 列表右侧的摘要：优先版本 → 绑定来源 → 状态文案 */
function summaryOf(p: PluginReport): string {
  if (p.status === 'disabled') return '已停用'
  if (p.version) return `v${p.version}`
  if (p.status === 'missing') {
    if (p.source === 'manual') return '路径无效'
    // auto 找不到 = 需要用户介入，右侧给行动指向，点进详情看完整说明
    return p.id === 'sillytavern' ? '未找到·去详情指定' : '未探测到'
  }
  return '已就绪'
}

/* ---- 扩展：搜索 + 安装 ---- */
const installOpen = ref(false)
const installUrl = ref('')
const installing = ref(false)
const installMsg = ref('')
const installOk = ref(false)

const shown = computed(() => ext.filtered)

function pickExt(name: string): void {
  plugins.selectedId = '' // 组件与扩展选中互斥（右栏二选一）
  ext.select(name)
  layout.showDetail() // 窄屏单栏：进入内容页
}

function pickStack(id: string): void {
  ext.selectedName = ''
  plugins.select(id)
}

async function doInstall(): Promise<void> {
  const url = installUrl.value.trim()
  if (!url || installing.value) return
  installing.value = true
  installMsg.value = ''
  try {
    const info = await ext.install(url)
    installOk.value = true
    installMsg.value =
      info.display_name || info.folderName
        ? `已安装：${info.display_name || info.folderName}${info.version ? ` · v${info.version}` : ''}${info.author ? ` · ${info.author}` : ''}`
        : '安装完成'
    installUrl.value = ''
  } catch (e) {
    installOk.value = false
    installMsg.value = e instanceof Error ? e.message : String(e)
  } finally {
    installing.value = false
  }
}

function extTypeLabel(type: string): string {
  if (type === 'system') return '内置'
  if (type === 'global') return '全局'
  return '第三方'
}

onMounted(() => {
  void plugins.load()
  void ext.load()
})
</script>

<template>
  <MidPane title="插件">
    <template #head>
      <div class="toolbar">
        <button class="mini" :disabled="plugins.loading || plugins.unavailable" @click="plugins.rescan()">
          {{ plugins.loading ? '检测中…' : '重新检测' }}
        </button>
        <span v-if="plugins.problems" class="alert">{{ plugins.problems }} 个系统插件未就绪</span>
      </div>

      <!-- 插件搜索 + 安装入口 -->
      <div class="toolbar">
        <input v-model="ext.search" class="search" type="text" placeholder="搜索插件" />
        <button class="mini" :disabled="ext.loading || ext.busy" @click="installOpen = !installOpen">
          ＋ 安装
        </button>
      </div>
      <div v-if="installOpen" class="panel">
        <input
          v-model="installUrl"
          class="search"
          type="text"
          placeholder="插件 Git 仓库地址（https://github.com/…）"
          @keyup.enter="doInstall"
        />
        <div class="panel-acts">
          <button class="mini primary" :disabled="installing || !installUrl.trim()" @click="doInstall">
            {{ ext.busy ? (ext.busyText || '下载中…') : '下载并安装' }}
          </button>
        </div>
        <p class="panel-hint">由 ST 服务端执行 Git 下载安装（用户级目录）</p>
        <p v-if="installMsg" class="op-msg" :class="{ err: !installOk }">{{ installMsg }}</p>
      </div>
    </template>

    <div v-if="plugins.unavailable" class="notice-bar">
      桌面壳未运行，插件管理不可用。
    </div>
    <div v-else-if="plugins.error" class="notice-bar err">{{ plugins.error }}</div>

    <template v-else>
      <div class="plug-group">系统插件</div>
      <button
        v-for="p in stackMain"
        :key="p.id"
        class="item"
        :class="{ on: plugins.selectedId === p.id }"
        @click="pickStack(p.id)"
      >
        <i class="dot" :class="pluginStatusOf(p.status).dot" />
        <span class="col">
          <span class="nm">{{ p.name }}</span>
          <span class="ds">{{ pluginStatusOf(p.status).text }}<template v-if="p.source === 'manual'"> · 指定路径</template></span>
        </span>
        <span class="rt">{{ summaryOf(p) }}</span>
      </button>

      <!-- ST 插件（与 ST 扩展管理器数据互通） -->
      <div class="plug-group">ST 插件（{{ shown.length }}）</div>
      <div v-if="ext.loading && !shown.length" class="notice-bar">检测插件中…</div>
      <div v-else-if="ext.error" class="notice-bar err">插件检测失败：{{ ext.error }}</div>
      <div v-else-if="!shown.length" class="notice-bar">没有匹配的插件</div>
      <button
        v-for="x in shown"
        :key="x.name"
        class="item"
        :class="{ on: ext.selectedName === x.name }"
        @click="pickExt(x.name)"
      >
        <i class="dot" :class="ext.isDisabled(x.name) ? '' : 'ok'" />
        <span class="col">
          <span class="nm">{{ x.name }}</span>
          <span class="ds">{{ extTypeLabel(x.type) }}{{ ext.isDisabled(x.name) ? ' · 已停用' : '' }}</span>
        </span>
      </button>
    </template>
  </MidPane>
</template>

<style scoped>
.toolbar {
  display: flex;
  align-items: center;
  gap: 6px;
  margin-top: 8px;
}
.toolbar .search {
  flex: 1;
  min-width: 0;
  padding: 5px 9px;
  font-size: 12px;
  border: 1px solid var(--c-border);
  border-radius: var(--radius-md);
  background: var(--c-bg);
  color: var(--c-text);
  outline: none;
}
.toolbar .search:focus {
  border-color: var(--p-400);
}
.mini {
  padding: 4px 9px;
  font-size: 11px;
  border: 1px solid var(--c-border);
  border-radius: var(--radius-md);
  color: var(--c-text-2);
  background: var(--c-bg);
}
.mini:hover:not(:disabled) {
  border-color: var(--p-400);
  color: var(--p-600);
}
.mini.primary {
  background: var(--p-500);
  border-color: var(--p-500);
  color: #fff;
}
.mini:disabled {
  opacity: 0.55;
  cursor: not-allowed;
}
.panel {
  margin-top: 6px;
  padding: 8px;
  border: 1px solid var(--c-border);
  border-radius: var(--radius-md);
  background: var(--c-bg);
}
.panel .search {
  margin-top: 0;
}
.panel-acts {
  display: flex;
  gap: 6px;
  margin-top: 6px;
}
.panel-hint {
  margin-top: 6px;
  font-size: 10px;
  line-height: 1.6;
  color: var(--c-text-3);
}
.op-msg {
  margin-top: 6px;
  font-size: 11px;
  color: var(--s-success);
  word-break: break-all;
}
.op-msg.err {
  color: var(--s-error);
}
.alert {
  font-size: 10px;
  color: var(--s-error);
}
.notice-bar {
  margin: 10px 4px;
  padding: 16px 10px;
  border: 1px dashed var(--c-border);
  border-radius: var(--radius-md);
  text-align: center;
  font-size: 12px;
  line-height: 1.8;
  color: var(--c-text-3);
}
.notice-bar.err {
  border-color: var(--s-error);
  color: var(--s-error);
}
.plug-group {
  margin: 12px 6px 4px;
  font-size: 10px;
  color: var(--c-text-3);
}
.item {
  display: flex;
  align-items: center;
  gap: 8px;
  width: 100%;
  padding: 8px 8px;
  text-align: left;
  border-radius: var(--radius-md);
  transition: background 0.15s;
}
.item:hover {
  background: var(--c-panel);
}
.item.on {
  background: var(--p-50);
}
.dot {
  width: 7px;
  height: 7px;
  flex: none;
  border-radius: 50%;
  background: var(--c-text-3);
}
.dot.ok {
  background: var(--s-success);
}
.dot.bad {
  background: var(--s-error);
}
.dot.warn {
  background: var(--s-warning);
}
.col {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 1px;
}
.nm {
  font-size: 13px;
  font-weight: 500;
  color: var(--c-text);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.ds {
  font-size: 10px;
  color: var(--c-text-3);
}
.rt {
  flex: none;
  font-size: 10px;
  font-family: var(--font-mono);
  color: var(--c-text-3);
}
</style>
