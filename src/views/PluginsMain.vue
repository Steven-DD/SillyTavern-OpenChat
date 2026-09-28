<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { usePluginsStore } from '@/stores/plugins'
import { useExtensionsStore } from '@/stores/extensions'
import ConfirmDialog from '@/components/ConfirmDialog.vue'
import ExtensionDetail from '@/components/ExtensionDetail.vue'
import {
  componentRemove,
  componentUse,
  componentVersions,
  stDataRemoveExternal,
  type ComponentVersions,
} from '@/services/tauri/plugins'
import { pluginStatusText } from '@/services/plugin-status'

const plugins = usePluginsStore()
const ext = useExtensionsStore()

const cur = computed(() => plugins.selected)
const isRequired = computed(() => cur.value?.tier === 'required')

/* ---------------- 绑定 ---------------- */

const manualPath = ref('')
const notice = ref('')

watch(
  () => cur.value?.id,
  () => {
    manualPath.value = cur.value?.source === 'manual' ? (cur.value.path ?? '') : ''
    notice.value = ''
  },
)

async function chooseSource(source: string) {
  const p = cur.value
  if (!p) return
  notice.value = ''
  try {
    if (source === 'manual') {
      const v = manualPath.value.trim()
      if (!v) {
        notice.value = '路径为空'
        return
      }
      await plugins.setBinding(p.id, 'manual', v)
      notice.value = '已切换为「指定路径」绑定'
    } else if (source === 'managed') {
      // managed 绑定通过版本列表的「使用此版本」按钮完成；点这里只是引导
      if (versions.value?.versions.some((x) => x.complete)) {
        notice.value = '宿主安装需在版本列表选择版本'
      } else {
        notice.value = '宿主目录暂无可用版本'
      }
    } else {
      await plugins.setBinding(p.id, 'auto', null)
      manualPath.value = ''
      notice.value = '已切换为「自动探测」，将复用本机已安装的环境'
    }
  } catch (e) {
    notice.value = e instanceof Error ? e.message : String(e)
  }
}

async function applyManualPath() {
  const p = cur.value
  if (!p) return
  const v = manualPath.value.trim()
  if (!v) {
    notice.value = '路径为空'
    return
  }
  notice.value = ''
  try {
    await plugins.setBinding(p.id, 'manual', v)
    notice.value = '路径已绑定'
  } catch (e) {
    notice.value = e instanceof Error ? e.message : String(e)
  }
}

/* ---------------- 启停（系统插件不可停用，仅可选插件可启停）---------------- */

async function toggleEnabled() {
  const p = cur.value
  if (!p) return
  // 系统插件（required）是 App 运行的前提，不提供停用入口
  if (p.enabled && isRequired.value) return
  notice.value = ''
  try {
    await plugins.setEnabled(p.id, !p.enabled)
    notice.value = p.enabled ? '已停用' : '已启用'
  } catch (e) {
    notice.value = e instanceof Error ? e.message : String(e)
  }
}

/* ---------------- 宿主安装（managed / S4c） ---------------- */

const versions = ref<ComponentVersions | null>(null)
const installVersion = ref('')

/** 安装进度与「安装中」标记是**全局**的（store 持有）—— 切换菜单不丢 */
const installing = computed(() => plugins.installInFlight)
const progress = computed(() => plugins.installProgress)
const installError = computed(() => plugins.error)

const canManage = computed(() => !!cur.value?.downloadable)

const PHASE_LABEL: Record<string, string> = {
  download: '下载',
  extract: '解压',
  npm: '安装依赖',
  verify: '校验',
  done: '完成',
  error: '失败',
}

function fmtSize(n: number): string {
  if (n >= 1073741824) return (n / 1073741824).toFixed(2) + ' GB'
  if (n >= 1048576) return (n / 1048576).toFixed(1) + ' MB'
  return (n / 1024).toFixed(0) + ' KB'
}

async function loadVersions() {
  const p = cur.value
  if (!p || !p.downloadable) {
    versions.value = null
    return
  }
  try {
    versions.value = await componentVersions(p.id)
    if (versions.value?.default_version) {
      installVersion.value = versions.value.default_version
    }
  } catch {
    versions.value = null
  }
}

async function startInstall() {
  const p = cur.value
  if (!p || installing.value) return
  const v = installVersion.value.trim()
  if (!v) {
    notice.value = '版本号为空'
    return
  }
  notice.value = ''
  try {
    await plugins.installComponent(p.id, v)
    notice.value = `安装完成：${p.name} ${v}（如需启用请点版本列表里的「使用此版本」）`
  } catch {
    /* 错误已在 store.error，模板会显示 */
  } finally {
    await loadVersions()
  }
}

async function useVersion(version: string) {
  const p = cur.value
  if (!p || !versions.value) return
  const v = versions.value.versions.find((x) => x.version === version)
  if (!v?.complete) {
    notice.value = '版本不完整，无法绑定'
    return
  }
  notice.value = ''
  try {
    // 路径由 Rust 侧裁决（node → node.exe；ST → 版本目录），前端只传版本号
    await componentUse(p.id, version)
    notice.value = `已绑定 ${p.name} ${version}（重启应用后生效）`
    await plugins.load()
    await loadVersions()
  } catch (e) {
    notice.value = e instanceof Error ? e.message : String(e)
  }
}

async function removeVersion(version: string) {
  const p = cur.value
  if (!p) return
  notice.value = ''
  try {
    versions.value = await componentRemove(p.id, version)
    notice.value = `已删除版本 ${version}`
  } catch (e) {
    notice.value = e instanceof Error ? e.message : String(e)
  }
}

/** 删除版本确认弹窗（删除不可恢复） */
const delVersion = ref('')
const delVersionBusy = ref(false)

/* ---- ST 插件：数据目录展示 + 删除外移副本 ---- */
const delExternal = ref(false)
const delExternalBusy = ref(false)

async function doRemoveExternal() {
  delExternalBusy.value = true
  notice.value = ''
  try {
    await stDataRemoveExternal()
    await plugins.load()
    notice.value = '已删除外移数据副本'
  } catch (e) {
    notice.value = `删除失败：${e instanceof Error ? e.message : String(e)}`
  } finally {
    delExternalBusy.value = false
    delExternal.value = false
  }
}

async function doRemoveVersion() {
  const v = delVersion.value
  if (!v) return
  delVersionBusy.value = true
  try {
    await removeVersion(v)
    delVersion.value = ''
  } finally {
    delVersionBusy.value = false
  }
}

watch(
  () => cur.value?.id,
  () => {
    // 进度与错误是全局状态（store 持有），这里只刷新版本列表
    void loadVersions()
  },
)

/* ---------------- 展示辅助 ---------------- */

const TIER_LABEL: Record<string, string> = { required: '系统插件', optional: '可选插件' }
const KIND_LABEL: Record<string, string> = {
  process: '进程型',
  resource: '资源型',
  frontend: '前端型',
  'st-extension': 'ST 插件型',
}
const SOURCE_LABEL: Record<string, string> = {
  auto: '自动探测（复用本机已装）',
  manual: '指定路径',
  managed: '宿主安装',
}
function label(map: Record<string, string>, k: string, fallback = '—') {
  return map[k] ?? (k || fallback)
}

const willAffect = computed(() => cur.value?.required_by ?? [])

onMounted(() => {
  void plugins.load()
  void plugins.initInstallWatcher()
  void loadVersions()
})
</script>

<template>
  <main class="main page" data-page="plugins">
    <!-- 选中扩展 → 扩展详情；否则组件（Node/ST）详情 -->
    <ExtensionDetail v-if="ext.selectedName" />

    <template v-else>
      <header class="head">
        <h3>插件</h3>
      </header>

      <div class="body">
      <section v-if="plugins.unavailable" class="card">
        <h4>桌面壳未运行</h4>
        <p class="hint">插件管理不可用。</p>
      </section>

      <template v-else>
        <section v-if="cur" class="card">
          <div class="title">
            <h4>{{ cur.name }}</h4>
            <span class="badge" :class="cur.tier">{{ label(TIER_LABEL, cur.tier) }}</span>
            <span class="badge kind">{{ label(KIND_LABEL, cur.kind) }}</span>
            <span class="badge" :class="cur.status">{{ pluginStatusText(cur.status) }}</span>
          </div>
          <p class="hint mono">{{ cur.message }}</p>

          <h5>满足方式</h5>
          <div class="radios">
            <label class="radio" :class="{ on: cur.source === 'auto' }">
              <input
                type="radio"
                name="bind"
                :checked="cur.source === 'auto'"
                :disabled="plugins.mutating"
                @change="chooseSource('auto')"
              />
              <span class="rt">{{ SOURCE_LABEL.auto }}</span>
            </label>
            <label class="radio" :class="{ on: cur.source === 'manual' }">
              <input
                type="radio"
                name="bind"
                :checked="cur.source === 'manual'"
                :disabled="plugins.mutating"
                @change="chooseSource('manual')"
              />
              <span class="rt">{{ SOURCE_LABEL.manual }}</span>
            </label>
            <label
              class="radio"
              :class="{ on: cur.source === 'managed', disabled: !canManage }"
            >
              <input
                type="radio"
                name="bind"
                :checked="cur.source === 'managed'"
                :disabled="!canManage || plugins.mutating"
                @change="chooseSource('managed')"
              />
              <span class="rt">{{ SOURCE_LABEL.managed }}</span>
            </label>
          </div>

          <div class="row">
            <input
              v-model="manualPath"
              class="field font-mono flex-1 min-w-0"
              :placeholder="
                cur.provides.includes('node-runtime')
                  ? '例如 C:\\Program Files\\nodejs\\node.exe'
                  : '例如 D:\\SillyTavern'
              "
            />
            <button class="btn flex-none" :disabled="plugins.mutating" @click="applyManualPath">
              绑定此路径
            </button>
          </div>

          <!-- 宿主安装（managed）：版本列表 + 安装向导 -->
          <template v-if="canManage && versions">
            <h5>宿主安装</h5>

            <table v-if="versions.versions.length" class="kv versions">
              <tbody>
                <tr v-for="v in versions.versions" :key="v.version">
                  <td class="k mono">{{ v.version }}</td>
                  <td class="v">
                    {{ fmtSize(v.bytes) }} · {{ v.files }} 文件
                    <span v-if="!v.complete" class="incomplete">不完整</span>
                  </td>
                  <td class="ops">
                    <span v-if="v.bound" class="badge ready">使用中</span>
                    <template v-else>
                      <button
                        class="btn btn-sm flex-none"
                        :disabled="!v.complete || installing"
                        @click="useVersion(v.version)"
                      >
                        使用此版本
                      </button>
                      <button class="btn btn-sm danger-text flex-none" :disabled="installing" @click="delVersion = v.version">
                        删除
                      </button>
                    </template>
                  </td>
                </tr>
              </tbody>
            </table>

            <div class="row">
              <span class="lb">版本</span>
              <input
                v-model="installVersion"
                class="field sm-wide [font-family:inherit] flex-none"
                :placeholder="versions.default_version ?? '如 1.19.0'"
                :disabled="installing"
              />
              <button class="btn btn-primary flex-none" :disabled="installing" @click="startInstall">
                {{ installing ? '安装中…' : '安装' }}
              </button>
            </div>

            <div v-if="progress" class="prog">
              <div class="prog-head">
                <span>{{ PHASE_LABEL[progress.phase] ?? progress.phase }} · {{ progress.version }}</span>
                <span class="pct">{{ progress.percent.toFixed(0) }}%</span>
              </div>
              <div class="bar">
                <i :style="{ width: progress.percent + '%' }" />
              </div>
              <p class="hint">
                {{ progress.message }}
                <template v-if="progress.download && progress.download.total > 0">
                  （{{ fmtSize(progress.download.done) }} / {{ fmtSize(progress.download.total) }}）
                </template>
              </p>
            </div>
            <p v-if="installError" class="notice bad">{{ installError }}</p>
          </template>

          <table class="kv">
            <tbody>
              <tr>
                <td class="k">当前解析</td>
                <td class="v">
                  {{ label(SOURCE_LABEL, cur.source) }}
                  <template v-if="cur.version"> · v{{ cur.version }}</template>
                </td>
              </tr>
              <tr>
                <td class="k">实际路径</td>
                <td class="v">{{ cur.path ?? '—' }}</td>
              </tr>
              <tr v-if="cur.min_version">
                <td class="k">版本要求</td>
                <td class="v">&gt;= {{ cur.min_version }}</td>
              </tr>
            </tbody>
          </table>

          <!-- ST 专属：数据目录位置（外移副本残留时提供清理入口） -->
          <template v-if="cur.id === 'sillytavern' && plugins.dataStatus">
            <h5>数据目录</h5>
            <table class="kv">
              <tbody>
                <tr>
                  <td class="k">位置</td>
                  <td class="v mono">{{ plugins.dataStatus.active_root }}</td>
                </tr>
                <tr v-if="!plugins.dataStatus.external && plugins.dataStatus.external_data_exists">
                  <td class="k">外移副本</td>
                  <td class="v">
                    <span class="mono">{{ plugins.dataStatus.external_root }}</span>
                    <button
                      class="btn btn-sm danger-text flex-none"
                      :disabled="delExternalBusy"
                      @click="delExternal = true"
                    >
                      删除副本
                    </button>
                  </td>
                </tr>
              </tbody>
            </table>
          </template>

          <template v-if="cur.dependencies.length">
            <h5>依赖</h5>
            <div v-for="d in cur.dependencies" :key="d.id" class="dep">
              <i class="dot" :class="d.ok ? 'ok' : 'bad'" />
              <span class="mono">{{ d.message }}</span>
            </div>
          </template>

          <template v-if="willAffect.length">
            <h5>被以下能力依赖</h5>
            <div class="tags">
              <span v-for="a in willAffect" :key="a" class="tg">{{ a }}</span>
            </div>
          </template>

          <div class="actions">
            <button class="btn flex-none" :disabled="plugins.loading" @click="plugins.rescan()">
              重新检测
            </button>
            <button
              v-if="!isRequired || !cur.enabled"
              :class="cur.enabled ? 'btn flex-none' : 'btn btn-primary flex-none'"
              :disabled="plugins.mutating"
              @click="toggleEnabled"
            >
              {{ cur.enabled ? '停用' : '启用' }}
            </button>
          </div>

          <p v-if="notice" class="notice">{{ notice }}</p>
        </section>

        <p v-else class="hint">未选择插件</p>
      </template>
    </div>

    <!-- 删除组件版本确认 -->
    <ConfirmDialog
      :open="!!delVersion"
      title="删除版本"
      :message="delVersion ? `删除 ${cur?.name ?? '组件'} 的版本 ${delVersion}？\n删除后需重新下载才能恢复。` : ''"
      confirm-text="删除"
      :busy="delVersionBusy"
      @confirm="doRemoveVersion"
      @cancel="delVersion = ''"
    />

    <!-- 删除外移数据副本确认（当前数据在外移目录时后端会拒绝） -->
    <ConfirmDialog
      :open="delExternal"
      title="删除外移数据副本"
      :message="`删除外移时留下的数据副本？\n${plugins.dataStatus?.external_root ?? ''}\n当前使用中的数据不受影响，删除后不可恢复。`"
      confirm-text="删除"
      :busy="delExternalBusy"
      @confirm="doRemoveExternal"
      @cancel="delExternal = false"
    />
    </template>
  </main>
</template>

<style scoped>
.main {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  /* 背景交给全局 .page[data-page]（见 base.css），此处不再重复声明 */
}
.head {
  flex: none;
  display: flex;
  align-items: baseline;
  gap: 10px;
  padding: 15px 20px 11px;
  border-bottom: 1px solid var(--c-border);
}
.head h3 {
  font-size: 15px;
  font-weight: 500;
}
.body {
  flex: 1;
  overflow-y: auto;
  padding: 14px 20px 30px;
  display: flex;
  flex-direction: column;
  gap: 12px;
}
/* ⚠ flex:none 防止 flex 纵向滚动容器把超高卡片压缩裁切（内容无法滚动） */
.card {
  flex: none;
}
.card h4 {
  font-size: 13px;
  font-weight: 500;
}
.card h5 {
  margin: 12px 0 6px;
  font-size: 11px;
  font-weight: 500;
  color: var(--c-text-2);
}
.hint.inline {
  margin: 0;
}
.hint.mono,
.mono {
  font-family: var(--font-mono);
}
.hint.warn {
  margin-top: 8px;
  color: var(--s-warning);
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
.title {
  display: flex;
  align-items: center;
  gap: 7px;
  flex-wrap: wrap;
}
.badge {
  padding: 1px 7px;
  font-size: 10px;
  border-radius: 9px;
  background: var(--c-panel);
  color: var(--c-text-2);
}
.badge.required {
  background: var(--p-100);
  color: var(--p-700);
}
.badge.kind {
  background: transparent;
  border: 1px solid var(--c-border);
}
.badge.ready {
  background: var(--s-success-bg);
  color: var(--s-success);
}
.badge.missing,
.badge.mismatch {
  background: var(--s-warning-bg);
  color: var(--s-warning);
}
.badge.disabled {
  background: var(--c-panel);
  color: var(--c-text-3);
}
.radios {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.radio {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 7px;
  padding: 8px 10px;
  border: 1px solid var(--c-border);
  border-radius: var(--radius-md);
  background: var(--c-bg);
  cursor: pointer;
}
.radio.on {
  border-color: var(--p-500);
  background: var(--p-50);
}
.radio.disabled {
  opacity: 0.6;
  cursor: not-allowed;
}
.rt {
  font-size: 12px;
  font-weight: 500;
}
.row {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-top: 9px;
}
.lb {
  font-size: 11px;
  color: var(--c-text-2);
  white-space: nowrap;
}
/* 本页输入件特例：小宽（宿主安装版本输入） */
.sm-wide {
  width: 140px;
}
/* .btn / .btn-primary / .btn-danger 外观走全局 shortcut（自带禁用态） */
.kv {
  width: 100%;
  border-collapse: collapse;
  font-size: 12px;
  margin-top: 9px;
}
.kv td {
  padding: 5px 8px;
  border-bottom: 1px solid var(--c-border);
}
.k {
  width: 88px;
  color: var(--c-text-3);
}
.v {
  font-family: var(--font-mono);
  color: var(--c-text);
  word-break: break-all;
  font-size: 11px;
}
.dep {
  display: flex;
  align-items: center;
  gap: 7px;
  font-size: 11px;
  color: var(--c-text-2);
  padding: 3px 0;
}
.tags {
  display: flex;
  flex-wrap: wrap;
  gap: 5px;
}
.tg {
  font-size: 10px;
  padding: 2px 8px;
  border-radius: 9px;
  background: var(--c-panel);
  color: var(--c-text-2);
}
.actions {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  margin-top: 14px;
}
.notice {
  margin-top: 9px;
  padding: 7px 10px;
  font-size: 11px;
  border-radius: var(--radius-md);
  background: var(--c-panel);
  color: var(--c-text-2);
}
.notice.bad {
  background: var(--s-warning-bg);
  color: var(--s-warning);
}

/* ---- 宿主安装 ---- */
.versions td.ops {
  text-align: right;
  white-space: nowrap;
}
.btn-sm {
  padding: 3px 9px;
  font-size: 11px;
  margin-left: 6px;
}
.danger-text:hover:not(:disabled) {
  border-color: var(--s-error);
  color: var(--s-error);
}
.incomplete {
  margin-left: 6px;
  padding: 1px 7px;
  font-size: 10px;
  border-radius: 9px;
  background: var(--s-warning-bg);
  color: var(--s-warning);
}
.prog {
  margin-top: 10px;
  padding: 9px 11px;
  border: 1px solid var(--c-border);
  border-radius: var(--radius-md);
  background: var(--c-bg);
}
.prog-head {
  display: flex;
  justify-content: space-between;
  font-size: 12px;
  font-weight: 500;
}
.pct {
  font-family: var(--font-mono);
}
.bar {
  height: 5px;
  margin: 7px 0 5px;
  border-radius: 3px;
  background: var(--c-panel);
  overflow: hidden;
}
.bar i {
  display: block;
  height: 100%;
  border-radius: 3px;
  background: var(--p-500);
  transition: width 0.2s ease;
}
</style>
