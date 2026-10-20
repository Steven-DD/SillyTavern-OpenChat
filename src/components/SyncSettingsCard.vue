<script setup lang="ts">
import { ref } from 'vue'
import { loadSyncConfig, saveSyncConfig, testSyncConnection, executeSync, type SyncConfig } from '@/services/st/sync'

const cfg = ref<SyncConfig>(loadSyncConfig())
const testing = ref(false)
const syncing = ref(false)
const testMsg = ref('')
const testSuccess = ref(false)
const syncMsg = ref('')
const syncSuccess = ref(false)

function onFieldChange(): void {
  saveSyncConfig(cfg.value)
  testMsg.value = ''
  syncMsg.value = ''
}

async function runTest(): Promise<void> {
  testing.value = true
  testMsg.value = ''
  try {
    const res = await testSyncConnection(cfg.value)
    testSuccess.value = true
    testMsg.value = res
  } catch (err) {
    testSuccess.value = false
    testMsg.value = `连接失败: ${err instanceof Error ? err.message : String(err)}`
  } finally {
    testing.value = false
  }
}

async function runSync(): Promise<void> {
  syncing.value = true
  syncMsg.value = ''
  try {
    const res = await executeSync(cfg.value)
    syncSuccess.value = res.success
    syncMsg.value = res.message + (res.errors.length ? ` (${res.errors.length} 项跳过/失败)` : '')
  } catch (err) {
    syncSuccess.value = false
    syncMsg.value = `同步失败: ${err instanceof Error ? err.message : String(err)}`
  } finally {
    syncing.value = false
  }
}
</script>

<template>
  <section class="card sync-card">
    <div class="card-head">
      <div class="card-title-wrap">
        <svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <path d="M21 12a9 9 0 0 0-9-9 9.75 9.75 0 0 0-6.74 2.74L3 8"></path>
          <path d="M3 3v5h5"></path>
          <path d="M3 12a9 9 0 0 0 9 9 9.75 9.75 0 0 0 6.74-2.74L21 16"></path>
          <path d="M16 21h5v-5"></path>
        </svg>
        <h4>WebDAV 云端增量同步 (Sync Engine)</h4>
      </div>
      <span class="sub-label">支持坚果云、Nextcloud、InfiniCLOUD 等标准 WebDAV 网盘</span>
    </div>

    <div class="form-grid">
      <label class="field">
        <span class="label">服务器地址 (WebDAV URL)</span>
        <input
          v-model="cfg.url"
          type="url"
          placeholder="https://dav.jianguoyun.com/dav/stoc/ 或 Nextcloud 路径"
          class="input"
          @input="onFieldChange"
        />
      </label>

      <div class="row-two">
        <label class="field">
          <span class="label">用户名</span>
          <input
            v-model="cfg.username"
            type="text"
            placeholder="WebDAV 账号或邮箱"
            class="input"
            @input="onFieldChange"
          />
        </label>

        <label class="field">
          <span class="label">应用密码 / Token</span>
          <input
            v-model="cfg.password"
            type="password"
            placeholder="网盘应用专用密码（非主登录密码）"
            class="input"
            @input="onFieldChange"
          />
        </label>
      </div>

      <label class="checkbox-row">
        <input v-model="cfg.auto_sync" type="checkbox" @change="onFieldChange" />
        <span>会话完成后自动防抖同步更改</span>
      </label>
    </div>

    <div class="actions-row">
      <button class="btn btn-test" :disabled="testing || syncing || !cfg.url" @click="runTest">
        {{ testing ? '探测中…' : '测试连接' }}
      </button>
      <button class="btn btn-primary" :disabled="testing || syncing || !cfg.url" @click="runSync">
        {{ syncing ? '正在增量同步…' : '立即增量同步' }}
      </button>
    </div>

    <div v-if="testMsg" class="status-box" :class="{ ok: testSuccess, fail: !testSuccess }">
      {{ testMsg }}
    </div>

    <div v-if="syncMsg" class="status-box" :class="{ ok: syncSuccess, fail: !syncSuccess }">
      {{ syncMsg }}
    </div>
  </section>
</template>

<style scoped>
.sync-card {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.card-head {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.card-title-wrap {
  display: flex;
  align-items: center;
  gap: 8px;
  color: var(--text-primary, #111827);
}

.card-title-wrap h4 {
  margin: 0;
  font-size: 14px;
  font-weight: 600;
}

.sub-label {
  font-size: 12px;
  color: var(--text-secondary, #6b7280);
}

.form-grid {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.row-two {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 10px;
}

.field {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.label {
  font-size: 12px;
  font-weight: 500;
  color: var(--text-secondary, #374151);
}

.input {
  padding: 6px 10px;
  border-radius: 6px;
  border: 1px solid var(--border-color, #d1d5db);
  background: var(--bg-card, #ffffff);
  font-size: 13px;
  color: var(--text-primary, #111827);
}

.input:focus {
  outline: none;
  border-color: var(--brand-primary, #3b82f6);
}

.checkbox-row {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 13px;
  cursor: pointer;
  color: var(--text-primary, #111827);
  margin-top: 4px;
}

.actions-row {
  display: flex;
  gap: 10px;
  margin-top: 4px;
}

.btn {
  padding: 6px 14px;
  border-radius: 6px;
  font-size: 13px;
  cursor: pointer;
  transition: all 0.15s ease;
}

.btn-test {
  border: 1px solid var(--border-color, #d1d5db);
  background: var(--bg-surface, #ffffff);
  color: var(--text-primary, #374151);
}

.btn-test:hover:not(:disabled) {
  background: var(--bg-hover, #f3f4f6);
}

.btn-primary {
  border: none;
  background: var(--brand-primary, #3b82f6);
  color: #ffffff;
}

.btn-primary:hover:not(:disabled) {
  opacity: 0.9;
}

.btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.status-box {
  padding: 8px 12px;
  border-radius: 6px;
  font-size: 12px;
  line-height: 1.4;
}

.status-box.ok {
  background: #ecfdf5;
  color: #065f46;
  border: 1px solid #a7f3d0;
}

.status-box.fail {
  background: #fef2f2;
  color: #991b1b;
  border: 1px solid #fecaca;
}
</style>
