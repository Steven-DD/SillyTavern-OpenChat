<script setup lang="ts">
/**
 * 数据目录卡片（native 架构）。
 *
 * 数据由应用自动维护在应用数据目录（角色卡 / 会话 / 密钥 / 世界书），无需手动迁移。
 * 这里只回答两件事：
 *   1. 导出 / 导入（数据袋 = 数据目录内容 + config.yaml 的 zip）
 *
 * 安全边界：
 *   - 导入：仅接受本应用的数据库袋 zip；不扫描、不读取 SillyTavern 安装目录
 *   - 导入：覆盖现有数据，导入前自动备份为 data.bak-<时间戳>
 *   - 导出含密钥：默认不导出；勾选后需手动输入确认词「导出密钥」才执行
 */
import { ref } from 'vue'
import { stDataExport, stDataImport, hasTauri } from '@/services/tauri/desktop'
import ConfirmDialog from '@/components/ConfirmDialog.vue'

const busy = ref<'export' | 'import' | null>(null)
const note = ref('')
const errMsg = ref('')
/** 导入确认弹窗（pendingZip 暂存待导入的 zip 路径） */
const confirmImport = ref(false)
const pendingZip = ref('')
/** 是否把明文密钥打进数据袋（默认否）+ 二次确认弹窗状态 */
const includeSecrets = ref(false)
const secretConfirm = ref(false)
const pendingDest = ref('')
/** 导出含明文密钥的确认词（须原样输入才能继续） */
const SECRET_CONFIRM_WORD = '导出密钥'

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
  const dest = await pickSavePath(`SillyTavern-OpenChat-data-${stamp}.zip`)
  if (!dest) return
  if (includeSecrets.value) {
    secretConfirm.value = true
    pendingDest.value = dest
    return
  }
  await doExportRun(dest)
}

async function doExportRun(dest: string) {
  busy.value = 'export'
  try {
    const r = await stDataExport(dest, includeSecrets.value)
    note.value = r ? `${r.message} → ${r.path}` : ''
  } catch (e) {
    errMsg.value = e instanceof Error ? e.message : String(e)
  } finally {
    busy.value = null
    pendingDest.value = ''
  }
}

async function doImport() {
  if (busy.value) return
  note.value = ''
  errMsg.value = ''
  const src = await pickZipPath()
  if (!src) return
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

    <p class="hint">
      数据由应用自动生成与维护（角色卡 / 会话 / 密钥 / 世界书），无需手动迁移。
      用「导出」做备份，换机器或重装后用「导入」恢复。
    </p>

    <p class="hint">
      导入仅支持数据袋 zip：<strong>不扫描、不读取 SillyTavern 安装目录</strong>。
      如需从 ST 迁移，请在 ST 侧导出数据袋（或把其 data 目录打包为 zip）后，用此处「导入」载入。
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

    <label class="secret-opt">
      <input v-model="includeSecrets" type="checkbox" />
      包含 API 密钥明文（secrets.json 等）
    </label>
    <p v-if="includeSecrets" class="hint warn-hint">
      ⚠ 勾选后导出的数据袋将包含<strong>全部 API 密钥明文</strong>（secrets.json 等）。
      导出前需要手动输入确认词「${SECRET_CONFIRM_WORD}」才能继续；备份用途结束后请删除该文件。
    </p>

    <p v-if="note" class="rsp okish">{{ note }}</p>
    <p v-if="errMsg" class="rsp err">{{ errMsg }}</p>

    <ConfirmDialog
      :open="confirmImport"
      title="导入数据"
      message="导入会覆盖当前全部数据（角色卡 / 会话 / 密钥 / 设置）。\n导入前会自动把现有数据备份为 data.bak-<时间戳>。确定继续？"
      confirm-text="覆盖导入"
      :busy="busy === 'import'"
      @confirm="doImportConfirmed"
      @cancel="confirmImport = false"
    />

    <ConfirmDialog
      :open="secretConfirm"
      title="导出包含明文密钥"
      message="⚠ 即将导出的数据袋将包含全部 API 密钥明文（secrets.json 等）。\n请确保目标位置安全（不要随意上传 / 分享），备份用途结束后及时删除。\n\n确认风险后，请在下方输入确认词以继续。"
      :require-text="SECRET_CONFIRM_WORD"
      confirm-text="确认导出（含明文密钥）"
      :busy="busy === 'export'"
      @confirm="() => { secretConfirm = false; if (pendingDest) void doExportRun(pendingDest) }"
      @cancel="secretConfirm = false; pendingDest = ''"
    />
  </section>
</template>

<style scoped>
.card h4 {
  font-size: 13px;
  font-weight: 600;
  margin-bottom: 9px;
}
.hint {
  margin: 8px 0;
}
.secret-opt {
  display: flex;
  align-items: center;
  gap: 6px;
  margin-top: 7px;
  font-size: 12px;
  color: var(--c-text-2);
  cursor: pointer;
}
.warn-hint {
  color: var(--c-err, #d57f4a);
}
</style>
