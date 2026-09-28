<script setup lang="ts">
/**
 * 数据目录卡片（简版，老板定的形态）
 *
 * 数据位置由应用自动生成（首次运行自动绑定到 st-runtime，ST 首启自动生成默认配置），
 * **没有迁移概念、没有向导**。这里只回答两件事：
 *   1. 数据在哪、多大（只读展示）
 *   2. 导出 / 导入（数据袋 = data 目录内容 + config.yaml 的 zip）
 *
 * 安全边界：
 *   - 导出：ST 运行中拒绝（数据使用中的 zip 不完整）
 *   - 导入：覆盖现有数据，导入前自动备份为 data.bak-<时间戳>
 */
import { computed, onMounted, ref } from 'vue'
import { usePluginsStore } from '@/stores/plugins'
import { stDataExport, stDataImport } from '@/services/tauri/plugins'
import { hasTauri } from '@/services/tauri/plugins'
import ConfirmDialog from '@/components/ConfirmDialog.vue'

const plugins = usePluginsStore()

const busy = ref<'export' | 'import' | null>(null)
const note = ref('')
const errMsg = ref('')
/** 导入确认弹窗（pendingZip 暂存待导入的 zip 路径） */
const confirmImport = ref(false)
const pendingZip = ref('')

const data = computed(() => plugins.dataStatus)

function fmtSize(n: number): string {
  if (n < 1024) return `${n} B`
  if (n < 1048576) return `${(n / 1024).toFixed(1)} KB`
  if (n < 1073741824) return `${(n / 1048576).toFixed(1)} MB`
  return `${(n / 1073741824).toFixed(2)} GB`
}

onMounted(() => {
  if (!plugins.stack) void plugins.load()
})

async function pickSavePath(defaultName: string): Promise<string | null> {
  const { save } = await import('@tauri-apps/plugin-dialog')
  const p = await save({
    title: '选择导出位置',
    defaultPath: defaultName,
    filters: [{ name: '数据袋', extensions: ['zip'] }],
  })
  return p ?? null
}

async function pickZipPath(): Promise<string | null> {
  const { open } = await import('@tauri-apps/plugin-dialog')
  const p = await open({
    title: '选择要导入的数据袋（.zip）',
    multiple: false,
    filters: [{ name: '数据袋', extensions: ['zip'] }],
  })
  if (Array.isArray(p)) return p[0] ?? null
  return p ?? null
}

async function doExport() {
  if (busy.value) return
  note.value = ''
  errMsg.value = ''
  const stamp = new Date().toISOString().slice(0, 10).replace(/-/g, '')
  const dest = await pickSavePath(`st-chat-data-${stamp}.zip`)
  if (!dest) return
  busy.value = 'export'
  try {
    const r = await stDataExport(dest)
    note.value = r ? `${r.message} → ${r.path}` : ''
  } catch (e) {
    errMsg.value = e instanceof Error ? e.message : String(e)
  } finally {
    busy.value = null
  }
}

async function doImport() {
  if (busy.value) return
  note.value = ''
  errMsg.value = ''
  const src = await pickZipPath()
  if (!src) return
  // 覆盖性操作：先弹确认窗（导入会清掉当前数据，旧数据进 .bak 备份）
  pendingZip.value = src
  confirmImport.value = true
}

async function doImportConfirmed() {
  if (busy.value || !pendingZip.value) return
  confirmImport.value = false
  busy.value = 'import'
  try {
    const r = await stDataImport(pendingZip.value)
    note.value = r ? r.message : ''
    await plugins.load()
  } catch (e) {
    errMsg.value = e instanceof Error ? e.message : String(e)
  } finally {
    busy.value = null
    pendingZip.value = ''
  }
}
</script>

<template>
  <section class="card">
    <h4>数据目录</h4>

    <p v-if="!data" class="hint">
      数据目录信息需要桌面壳。浏览器开发环境下数据由 ST 自行管理，本页不显示。
    </p>

    <template v-else>
      <table class="kv">
        <tbody>
          <tr>
            <td class="k">位置</td>
            <td class="v mono-sm">{{ data.active_root }}</td>
          </tr>
          <tr>
            <td class="k">大小</td>
            <td class="v">{{ data.total_files }} 个文件 · {{ fmtSize(data.total_bytes) }}</td>
          </tr>
        </tbody>
      </table>

      <p class="hint">
        数据由应用自动生成与维护（角色卡 / 会话 / 密钥 / 世界书），无需手动迁移。
        用「导出」做备份，换机器或重装后用「导入」恢复。
      </p>

      <div class="btns">
        <button class="btn" :disabled="busy !== null" @click="doExport">
          {{ busy === 'export' ? '导出中…' : '导出' }}
        </button>
        <button class="btn" :disabled="busy !== null" @click="doImport">
          {{ busy === 'import' ? '导入中…' : '导入' }}
        </button>
        <span v-if="!hasTauri()" class="sub">（需要桌面环境）</span>
      </div>

      <p v-if="note" class="rsp okish">{{ note }}</p>
      <p v-if="errMsg" class="rsp err">{{ errMsg }}</p>
      <p v-else-if="plugins.error" class="rsp err">{{ plugins.error }}</p>
    </template>

    <!-- 导入数据确认（覆盖当前全部数据） -->
    <ConfirmDialog
      :open="confirmImport"
      title="导入数据"
      message="导入会覆盖当前全部数据（角色卡 / 会话 / 密钥 / 设置）。\n导入前会自动把现有数据备份为 data.bak-<时间戳>。确定继续？"
      confirm-text="覆盖导入"
      :busy="busy === 'import'"
      @confirm="doImportConfirmed"
      @cancel="confirmImport = false"
    />
  </section>
</template>

<style scoped>
.card h4 {
  font-size: 13px;
  font-weight: 600;
  margin-bottom: 9px;
}
/* .hint 外观走全局 shortcut；只保留本组件的上下距 */
.hint {
  margin: 8px 0;
}
.kv {
  width: 100%;
  border-collapse: collapse;
  font-size: 12px;
  margin-top: 4px;
}
.kv td {
  padding: 5px 8px;
  border-bottom: 1px solid var(--c-border);
  vertical-align: top;
}
.k {
  width: 64px;
  color: var(--c-text-3);
  white-space: nowrap;
}
.v {
  color: var(--c-text);
  word-break: break-all;
}
.mono-sm {
  font-family: var(--font-mono);
  font-size: 11px;
}
.btns {
  display: flex;
  align-items: center;
  gap: 7px;
  margin-top: 9px;
}
.ghost:hover:not(:disabled) {
  border-color: var(--p-400);
  color: var(--p-600);
}
.ghost:disabled {
  opacity: 0.55;
  cursor: not-allowed;
}
.sub {
  font-size: 11px;
  color: var(--c-text-3);
}
.rsp {
  margin-top: 9px;
  padding: 7px 10px;
  font-size: 11px;
  line-height: 1.6;
  border-radius: var(--radius-md);
  background: var(--c-panel);
  color: var(--c-text-2);
  word-break: break-word;
}
.rsp.err {
  background: var(--s-error-bg);
  color: var(--s-error);
}
.rsp.okish {
  background: var(--p-50);
  color: var(--p-700, var(--p-600));
}
</style>
