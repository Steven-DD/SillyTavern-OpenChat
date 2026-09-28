<script setup lang="ts">
/**
 * 插件详情（右栏）：manifest 说明 + git 状态 + 右键菜单操作
 * （对齐 ST 扩展管理器：/version 状态、/branches+/switch 分支、/move 移动）
 * 操作入口 = 页面右键菜单（启停 / 更新 / 分支 / 移动 / 删除）。
 */
import { computed, ref, watch } from 'vue'
import { useExtensionsStore } from '@/stores/extensions'
import { useCtxMenuStore } from '@/stores/ctxmenu'
import { extUsageInfo, getExtensionManifest, type ExtBranch, type ExtManifest } from '@/services/st/extensions'
import ConfirmDialog from '@/components/ConfirmDialog.vue'

const ext = useExtensionsStore()
const ctx = useCtxMenuStore()
const selected = computed(() => ext.selected)
const isGlobal = computed(() => selected.value?.type === 'global')
const info = computed(() => {
  const x = selected.value
  return x ? extUsageInfo(x.name, x.type) : null
})

const manifest = ref<ExtManifest | null>(null)

/* ---- git 状态（/version） ---- */
interface GitState {
  branchName: string
  commit: string
  upToDate: boolean | null
  remoteUrl: string
  notRepo: boolean
}
const git = ref<GitState | null>(null)
const gitLoading = ref(false)
const gitErr = ref('')

async function refreshGit(): Promise<void> {
  const x = selected.value
  if (!x) return
  gitLoading.value = true
  gitErr.value = ''
  try {
    const v = await ext.version(x.name, isGlobal.value)
    git.value = {
      branchName: v.currentBranchName ?? '',
      commit: (v.currentCommitHash ?? '').slice(0, 7),
      upToDate: v.isUpToDate ?? null,
      remoteUrl: v.remoteUrl ?? '',
      notRepo: !v.currentCommitHash,
    }
  } catch (e) {
    gitErr.value = e instanceof Error ? e.message : String(e)
    git.value = null
  } finally {
    gitLoading.value = false
  }
}

watch(
  () => ext.selectedName,
  async (name) => {
    manifest.value = null
    git.value = null
    branchOpen.value = false
    branches.value = []
    if (!name) return
    void getExtensionManifest(name).then((m) => (manifest.value = m))
    void refreshGit()
  },
  { immediate: true },
)

/* ---- 更新 ---- */
const updateMsg = ref('')
const updateOk = ref(false)
async function onUpdate(): Promise<void> {
  const x = selected.value
  if (!x) return
  updateMsg.value = ''
  try {
    const r = await ext.update(x.name, isGlobal.value)
    updateOk.value = true
    updateMsg.value = r.isUpToDate
      ? '已是最新版本'
      : `已更新到 ${r.commitHash?.slice(0, 7) ?? '最新'}`
    void refreshGit()
  } catch (e) {
    updateOk.value = false
    updateMsg.value = e instanceof Error ? e.message : String(e)
  }
}

/* ---- 分支（/branches + /switch） ---- */
const branchOpen = ref(false)
const branches = ref<ExtBranch[]>([])
const branchLoading = ref(false)

async function toggleBranchPanel(): Promise<void> {
  const x = selected.value
  if (!x) return
  branchOpen.value = !branchOpen.value
  if (!branchOpen.value || branches.value.length) return
  branchLoading.value = true
  try {
    branches.value = await ext.branches(x.name, isGlobal.value)
  } catch (e) {
    ext.error = e instanceof Error ? e.message : String(e)
  } finally {
    branchLoading.value = false
  }
}

async function onSwitchBranch(branch: string): Promise<void> {
  const x = selected.value
  if (!x || !branch) return
  try {
    await ext.switchBranch(x.name, isGlobal.value, branch)
    branchOpen.value = false
    void refreshGit()
  } catch (e) {
    ext.error = e instanceof Error ? e.message : String(e)
  }
}

/* ---- 移动（/move） ---- */
async function onMove(): Promise<void> {
  const x = selected.value
  if (!x) return
  await ext.move(x.name, !isGlobal.value)
}

/* ---- 启停 / 删除 ---- */
async function toggleDisabled(): Promise<void> {
  const x = selected.value
  if (!x) return
  await ext.toggleDisabled(x.name)
}

const delConfirm = ref(false)
async function doDelete(): Promise<void> {
  const x = selected.value
  if (!x) return
  await ext.remove(x.name, isGlobal.value)
  delConfirm.value = false
}

/* ---- 右键菜单（操作入口） ---- */
const isThirdParty = computed(() => !!selected.value && !selected.value.type.includes('system'))

function onCtx(e: MouseEvent): void {
  if (!selected.value) return
  const items = [
    {
      id: 'toggle',
      label: ext.isDisabled(selected.value.name) ? '启用' : '停用',
    },
    { id: 'update', label: '更新' },
    ...(isThirdParty.value ? [{ id: 'branch', label: branchOpen.value ? '收起分支列表' : '切换分支' }] : []),
    { id: 'move', label: isGlobal.value ? '移到用户目录' : '移到全局目录' },
    { id: 'delete', label: '删除', danger: true },
  ]
  ctx.show(e, items, (id) => {
    if (id === 'toggle') void toggleDisabled()
    else if (id === 'update') void onUpdate()
    else if (id === 'branch') void toggleBranchPanel()
    else if (id === 'move') void onMove()
    else if (id === 'delete') delConfirm.value = true
  })
}

const displayName = computed(() => manifest.value?.display_name || selected.value?.name || '')
</script>

<template>
  <main v-if="selected" class="main page" data-page="plugins" @contextmenu.prevent="onCtx">
    <header class="head">
      <div class="who">
        <div class="nm">
          {{ displayName }}
          <span class="bd">{{ selected.type === 'system' ? '内置' : selected.type === 'global' ? '全局' : '第三方' }}</span>
          <span class="bd" :class="{ off: ext.isDisabled(selected.name) }">
            {{ ext.isDisabled(selected.name) ? '已停用' : '已启用' }}
          </span>
        </div>
        <div class="sub">
          <template v-if="gitLoading">读取状态…</template>
          <template v-else-if="git && git.branchName">
            {{ git.branchName }} @ {{ git.commit }} ·
            <span :class="git.upToDate ? 'ok-t' : 'warn-t'">{{ git.upToDate ? '已是最新' : '有可用更新' }}</span>
          </template>
          <template v-else-if="git?.notRepo">非 Git 仓库（本地目录安装）</template>
          <template v-else>未获取到版本信息</template>
        </div>
      </div>
      <span class="ctx-hint">右键 → 操作菜单</span>
    </header>

    <div class="body">
      <!-- 分支选择面板 -->
      <div v-if="branchOpen" class="branch-panel">
        <div class="bp-h">切换分支（当前：{{ git?.branchName || '—' }}）</div>
        <div v-if="branchLoading" class="bp-item dim">加载分支列表…</div>
        <button
          v-for="b in branches"
          :key="b.name"
          class="bp-item"
          :class="{ on: b.current }"
          type="button"
          @click="onSwitchBranch(b.name)"
        >
          <span class="bp-name">{{ b.name }}</span>
          <span class="bp-meta">{{ b.commit.slice(0, 7) }} {{ b.label }}</span>
          <span v-if="b.current" class="bp-cur">当前</span>
        </button>
      </div>

      <p v-if="updateMsg" class="rsp" :class="{ okish: updateOk, err: !updateOk }">{{ updateMsg }}</p>
      <p v-if="gitErr" class="rsp err">{{ gitErr }}</p>
      <p v-if="ext.error" class="rsp err">{{ ext.error }}</p>

      <section class="sec">
        <h4>说明</h4>
        <p class="txt">
          {{ manifest?.description || info?.desc }}
          <template v-if="manifest?.author">（作者：{{ manifest.author }}<template v-if="manifest.version"> · v{{ manifest.version }}</template>）</template>
        </p>
        <p v-if="manifest?.requires?.length" class="txt dim">依赖服务：{{ manifest.requires.join('、') }}</p>
      </section>

      <section class="sec">
        <h4>在 App 中的使用方式</h4>
        <p class="txt">{{ info?.app }}</p>
      </section>

      <section class="sec">
        <h4>仓库信息</h4>
        <p v-if="git?.remoteUrl" class="txt dim break">{{ git.remoteUrl }}</p>
        <p v-else class="txt dim">无远端地址</p>
      </section>

      <section class="sec">
        <h4>启停说明</h4>
        <p class="txt dim">
          停用状态写入 ST settings.json（extension_settings.disabledExtensions），与 ST
          网页端的启用开关为同一份数据。第三方插件是前端脚本，仅在其宿主 UI
          中加载运行；本 App 提供下载/更新/删除/启停管理。
        </p>
      </section>
    </div>

    <!-- 删除确认 -->
    <ConfirmDialog
      :open="delConfirm"
      title="删除插件"
      :message="selected ? `删除插件「${displayName}」？\n该操作不可恢复。` : ''"
      confirm-text="删除"
      :busy="ext.busy"
      @confirm="doDelete"
      @cancel="delConfirm = false"
    />
  </main>
</template>

<style scoped>
.main {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
}
.head {
  height: 56px;
  flex: none;
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 0 16px;
  border-bottom: 1px solid var(--c-border);
}
.who {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 1px;
}
.nm {
  font-size: 14px;
  font-weight: 600;
  color: var(--c-text);
  display: flex;
  align-items: center;
  gap: 6px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.bd {
  flex: none;
  padding: 0 6px;
  font-size: 9px;
  line-height: 16px;
  border-radius: 8px;
  background: var(--c-panel);
  color: var(--c-text-2);
}
.bd.off {
  color: var(--s-error);
}
.sub {
  font-size: 11px;
  color: var(--c-text-3);
}
.ok-t {
  color: var(--s-success);
}
.warn-t {
  color: var(--s-warning);
}
.ctx-hint {
  flex: none;
  font-size: 10px;
  color: var(--c-text-3);
}
.body {
  flex: 1;
  overflow-y: auto;
  padding: 12px 16px 24px;
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.branch-panel {
  border: 1px solid var(--p-400, var(--c-border));
  border-radius: var(--radius-md);
  overflow: hidden;
}
.bp-h {
  padding: 7px 10px;
  font-size: 11px;
  color: var(--c-text-2);
  background: var(--c-panel);
}
.bp-item {
  display: flex;
  align-items: baseline;
  gap: 8px;
  width: 100%;
  padding: 7px 10px;
  text-align: left;
  font-size: 11px;
  border-top: 1px solid var(--c-border);
}
.bp-item:hover {
  background: var(--c-panel);
}
.bp-item.on {
  background: var(--p-50);
}
.bp-name {
  flex: none;
  font-weight: 600;
  color: var(--c-text);
}
.bp-meta {
  flex: 1;
  min-width: 0;
  color: var(--c-text-3);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.bp-cur {
  flex: none;
  font-size: 9px;
  color: var(--p-600);
}
.sec {
  border: 1px solid var(--c-border);
  border-radius: var(--radius-md);
  padding: 10px 12px;
}
.sec h4 {
  margin: 0 0 6px;
  font-size: 12px;
  color: var(--c-text-2);
}
.txt {
  margin: 0;
  font-size: 12px;
  line-height: 1.7;
  color: var(--c-text);
  white-space: pre-wrap;
  word-break: break-word;
}
.txt.dim {
  color: var(--c-text-3);
}
.break {
  word-break: break-all;
}
.rsp {
  margin: 0;
  padding: 7px 11px;
  font-size: 11px;
  border-radius: var(--radius-md);
}
.rsp.okish {
  background: var(--s-success-bg, transparent);
  color: var(--s-success);
}
.rsp.err {
  background: color-mix(in srgb, var(--s-error) 10%, transparent);
  color: var(--s-error);
}
</style>
