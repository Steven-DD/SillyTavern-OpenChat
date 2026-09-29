<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useRoute } from 'vue-router'
import { useSettingsStore } from '@/stores/settings'
import { useChatStore } from '@/stores/chat'
import { usePersonaStore } from '@/stores/persona'
import { useThemeStore, type ThemeMode } from '@/stores/theme'
import { useAppearanceStore, type FontScale, type Lang } from '@/stores/appearance'
import {
  generate,
  probeSt,
  fetchModels,
  writeSecret,
  CHAT_SOURCES,
  getConnConfig,
  setConnConfig,
} from '@/services/st/api'
import type { ChatSourceDef } from '@/services/st/api'
import { TEXTGEN_BACKENDS, fetchTextModels } from '@/services/st/textgen'
import { getSettings, loadAutoContinue } from '@/services/st/data'
import { runMaidReport, finalizeMaid, deleteMaidFiles, maidViewPath, type MaidReport } from '@/services/st/datamaid'
import { stGetText } from '@/services/st/client'
import { sidecar } from '@/services/tauri/bridge'
import DataDirCard from '@/components/DataDirCard.vue'
import PromptManagerCard from '@/components/PromptManagerCard.vue'
import ExtensionsCard from '@/components/ExtensionsCard.vue'
import { useBackgroundStore } from '@/stores/background'

const route = useRoute()
const gen = useSettingsStore()
const chat = useChatStore()
const persona = usePersonaStore()
const theme = useThemeStore()
const bgc = useBackgroundStore()

const sec = computed(() => String(route.query.sec ?? 'api'))

/* ---------- API 与模型 ---------- */
const testing = ref(false)
const testMsg = ref('')
const testOk = ref(false)

/* ---------- 生成行为 ---------- */
const acEnabled = ref(false)
const acTarget = ref(400)
const acSaving = ref(false)

async function persistAutoContinue(): Promise<void> {
  acSaving.value = true
  try {
    await chat.setAutoContinue({ enabled: acEnabled.value, targetLength: acTarget.value })
  } finally {
    acSaving.value = false
  }
}

/* ---------- 数据巡检 ---------- */
const MAID_GROUPS: Record<string, string> = {
  images: '未引用图片',
  files: '未引用附件',
  chats: '孤儿会话',
  groupChats: '孤儿群聊',
  avatarThumbnails: '头像缩略图',
  backgroundThumbnails: '背景缩略图',
  personaThumbnails: '人设缩略图',
  chatBackups: '聊天备份',
  settingsBackups: '设置备份',
}
const maid = ref<{
  report: MaidReport | null
  token: string
  busy: boolean
  msg: string
  total: number
  /** 勾选的 hash（选择性清理） */
  checked: Record<string, boolean>
  openGroup: string
}>({
  report: null,
  token: '',
  busy: false,
  msg: '',
  total: 0,
  checked: {},
  openGroup: '',
})

function maidCheckedHashes(): string[] {
  return Object.entries(maid.value.checked)
    .filter(([, v]) => v)
    .map(([k]) => k)
}

async function runMaidReportUi(): Promise<void> {
  maid.value.busy = true
  maid.value.msg = ''
  try {
    const r = await runMaidReport()
    const total = Object.values(r.report).reduce((n, l) => n + l.length, 0)
    maid.value.report = r.report
    maid.value.token = r.token
    maid.value.total = total
    maid.value.checked = {}
    maid.value.openGroup = ''
    maid.value.msg = total ? `发现 ${total} 个未引用文件，可逐项查看、勾选清理或一键清理（不可恢复）` : '数据区很干净，没有可清理的文件'
  } catch (e) {
    maid.value.msg = `扫描失败：${e instanceof Error ? e.message : String(e)}`
  } finally {
    maid.value.busy = false
  }
}

async function finalizeMaidUi(): Promise<void> {
  maid.value.busy = true
  try {
    await finalizeMaid(maid.value.token)
    maid.value.msg = `已清理 ${maid.value.total} 个文件`
    maid.value.report = null
    maid.value.token = ''
    maid.value.total = 0
    maid.value.checked = {}
  } catch (e) {
    maid.value.msg = `清理失败：${e instanceof Error ? e.message : String(e)}`
  } finally {
    maid.value.busy = false
  }
}

/** 选择性清理：按勾选 hash 调 /delete */
async function deleteSelectedMaidUi(): Promise<void> {
  const hashes = maidCheckedHashes()
  if (!hashes.length) {
    maid.value.msg = '请先勾选要清理的文件'
    return
  }
  maid.value.busy = true
  try {
    await deleteMaidFiles(maid.value.token, hashes)
    maid.value.msg = `已清理选中的 ${hashes.length} 个文件`
    // 本地移除已删条目
    if (maid.value.report) {
      for (const key of Object.keys(maid.value.report)) {
        maid.value.report[key] = maid.value.report[key].filter((x) => !maid.value.checked[x.hash])
      }
    }
    maid.value.checked = {}
    maid.value.total = maid.value.report
      ? Object.values(maid.value.report).reduce((n, l) => n + l.length, 0)
      : 0
    if (!maid.value.total) maid.value.report = null
  } catch (e) {
    maid.value.msg = `清理失败：${e instanceof Error ? e.message : String(e)}`
  } finally {
    maid.value.busy = false
  }
}

const maidViewer = ref<{ open: boolean; title: string; content: string; busy: boolean }>({
  open: false,
  title: '',
  content: '',
  busy: false,
})

/** /view 查看器：App 内嵌模态（window.open 在 Tauri 无效，且 /view 走中继需带 token） */
async function maidViewUi(hash: string): Promise<void> {
  if (!maid.value.token) return
  const rec = Object.values(maid.value.report ?? {})
    .flat()
    .find((r) => r.hash === hash)
  maidViewer.value = { open: true, title: rec?.name ?? hash, content: '', busy: true }
  try {
    maidViewer.value.content = await stGetText(maidViewPath(maid.value.token, hash))
  } catch (e) {
    maidViewer.value.content = `读取失败：${e instanceof Error ? e.message : String(e)}`
  } finally {
    maidViewer.value.busy = false
  }
}

function fmtMaidSize(size?: number): string {
  if (size === undefined) return '—'
  if (size >= 1048576) return `${(size / 1048576).toFixed(1)} MB`
  if (size >= 1024) return `${(size / 1024).toFixed(1)} KB`
  return `${size} B`
}

/* ---------- API 连接 ---------- */
const keyDraft = ref('')
const revDraft = ref('')
const customUrlDraft = ref('')
const connSaving = ref(false)
const pulling = ref(false)
const connMsg = ref('')
const connOk = ref(false)

const curDef = computed<ChatSourceDef | undefined>(() =>
  CHAT_SOURCES.find((s) => s.id === gen.source),
)

/** 源专属字段草稿（vertexai / azure / workers_ai） */
const vaiAuthDraft = ref<'express' | 'full'>('express')
const vaiRegionDraft = ref('')
const vaiProjectDraft = ref('')
const azBaseDraft = ref('')
const azDeployDraft = ref('')
const azVersionDraft = ref('')
const cfAccountDraft = ref('')

/** 切换通道时清空密钥草稿、载入该通道的连接配置 */
function onSourceChanged(): void {
  keyDraft.value = ''
  const c = getConnConfig(gen.source)
  revDraft.value = c.reverseProxy ?? ''
  customUrlDraft.value = c.customUrl ?? ''
  vaiAuthDraft.value = c.vertexaiAuthMode ?? 'express'
  vaiRegionDraft.value = c.vertexaiRegion ?? ''
  vaiProjectDraft.value = c.vertexaiProjectId ?? ''
  azBaseDraft.value = c.azureBaseUrl ?? ''
  azDeployDraft.value = c.azureDeploymentName ?? ''
  azVersionDraft.value = c.azureApiVersion ?? ''
  cfAccountDraft.value = c.workersAiAccountId ?? ''
  connMsg.value = ''
}

function pickSource(id: string): void {
  gen.update({ source: id })
  onSourceChanged()
}

function errText(e: unknown): string {
  return e instanceof Error ? e.message : String(e)
}

/** 保存密钥到 ST secrets（服务端托管，前端不留明文） */
async function saveKey(): Promise<void> {
  const def =
    gen.genType === 'text' ? textBackendDef.value : curDef.value
  if (!def?.secretKey) return
  connSaving.value = true
  connMsg.value = ''
  try {
    await writeSecret(def.secretKey, keyDraft.value.trim())
    keyDraft.value = ''
    connOk.value = true
    connMsg.value = '密钥已写入 ST secrets（服务端托管）。点「连接并拉取模型」验证。'
  } catch (e) {
    connOk.value = false
    connMsg.value = errText(e)
  } finally {
    connSaving.value = false
  }
}

/** 保存反代/端点等附加连接字段 */
async function saveConnExtras(): Promise<void> {
  if (gen.genType === 'text') return
  setConnConfig(gen.source, {
    reverseProxy: revDraft.value.trim() || undefined,
    customUrl: customUrlDraft.value.trim() || undefined,
    vertexaiAuthMode: (vaiAuthDraft.value as 'express' | 'full') || undefined,
    vertexaiRegion: vaiRegionDraft.value.trim() || undefined,
    vertexaiProjectId: vaiProjectDraft.value.trim() || undefined,
    azureBaseUrl: azBaseDraft.value.trim() || undefined,
    azureDeploymentName: azDeployDraft.value.trim() || undefined,
    azureApiVersion: azVersionDraft.value.trim() || undefined,
    workersAiAccountId: cfAccountDraft.value.trim() || undefined,
  })
}

/* ---------- Text Completion 连接 ---------- */
const textServerDraft = ref('')
const textBackendDef = computed(() => TEXTGEN_BACKENDS.find((b) => b.id === gen.textBackend))

function onGenTypeChanged(t: 'chat' | 'text'): void {
  gen.update({ genType: t })
  onSourceChanged()
  connMsg.value = ''
}

function onTextBackendChanged(id: string): void {
  gen.update({ textBackend: id })
  keyDraft.value = ''
  connMsg.value = ''
}

function saveTextServer(): void {
  gen.update({ textServer: textServerDraft.value.trim() })
}

async function connectText(): Promise<void> {
  pulling.value = true
  connMsg.value = ''
  try {
    gen.update({ textServer: textServerDraft.value.trim() })
    const ids = await fetchTextModels(gen.textBackend, gen.textServer || undefined)
    gen.setModelOptions(ids)
    connOk.value = true
    connMsg.value = `连接成功：${ids.length} 个模型，可在下方选择。`
  } catch (e) {
    connOk.value = false
    connMsg.value = errText(e)
  } finally {
    pulling.value = false
  }
}

/** 一键连接：先保存连接字段，再拉模型（拉成功 = 密钥有效） */
async function connect(): Promise<void> {
  if (gen.genType === 'text') {
    await connectText()
    return
  }
  pulling.value = true
  connMsg.value = ''
  try {
    await saveConnExtras()
    const ids = await fetchModels(gen.source)
    gen.setModelOptions(ids)
    connOk.value = true
    connMsg.value = `连接成功：${ids.length} 个模型，可在下方选择。`
  } catch (e) {
    connOk.value = false
    connMsg.value = errText(e)
  } finally {
    pulling.value = false
  }
}

/* ---------- 开发者模式（隐藏彩蛋：通用页连点版本号 7 次切换） ---------- */
const devMode = ref(localStorage.getItem('app.dev') === '1')

function setDev(on: boolean): void {
  devMode.value = on
  localStorage.setItem('app.dev', on ? '1' : '0')
}

/** 下拉候选 = ST 实拉列表（不给任何预设默认值） */
const modelChoices = computed(() => gen.modelOptions)

/**
 * 点开模型下拉时兜底自动拉取（用户没先点「连接并拉取模型」的场景）。
 * 只在列表为空时拉，成功后不再自动重复。
 */
async function ensureModels(): Promise<void> {
  if (pulling.value || gen.modelOptions.length) return
  if (gen.genType === 'text') {
    await connectText()
    return
  }
  pulling.value = true
  connMsg.value = ''
  try {
    await saveConnExtras()
    const ids = await fetchModels(gen.source)
    gen.setModelOptions(ids)
    connOk.value = true
    connMsg.value = `已自动拉取 ${ids.length} 个模型`
  } catch (e) {
    connOk.value = false
    connMsg.value = errText(e)
  } finally {
    pulling.value = false
  }
}

async function testConn() {
  testing.value = true
  testMsg.value = ''
  try {
    const ok = await probeSt()
    if (!ok) {
      testOk.value = false
      testMsg.value = '无法连接 ST 后端（8000），请确认服务已启动'
      return
    }
    const st = await getSettings()
    // ST 初始化 username 为空串 —— 空值不覆盖，保留兜底（{{user}} 宏需要可读值）
    const stName = String(st.username ?? '').trim()
    if (stName) chat.userName = stName
    testOk.value = true
    testMsg.value = `ST 后端在线 · 用户名 ${chat.userName} · main_api=${st.main_api ?? '—'}`
  } catch (e) {
    testOk.value = false
    testMsg.value = e instanceof Error ? e.message : String(e)
  } finally {
    testing.value = false
  }
}

/** 真实生成自检（会消耗 1 次上游配额） */
const genTesting = ref(false)
const genMsg = ref('')
const genOk = ref(false)
async function testGenerate() {
  genTesting.value = true
  genMsg.value = ''
  let out = ''
  try {
    const used = await generate(
      {
        messages: [
          { role: 'system', content: 'You are a connectivity probe. Reply with exactly: OK' },
          { role: 'user', content: 'ping' },
        ],
        model: gen.model,
        source: gen.source,
        temperature: 0,
        maxTokens: 64,
      },
      (d) => {
        out += d
      },
    )
    genOk.value = true
    genMsg.value = `生成通路正常（模型 ${used}）：${out.trim().slice(0, 60) || '—'}`
  } catch (e) {
    genOk.value = false
    genMsg.value = e instanceof Error ? e.message : String(e)
  } finally {
    genTesting.value = false
  }
}

/* ---------- 下载与安装设置（代理 / 组件目录） ---------- */
import { getAppSettings, setAppSettings, type AppSettings } from '@/services/tauri/plugins'

const dl = ref<AppSettings>({ proxy: '', proxy_for_npm: true, install_root: '' })
const dlSaving = ref(false)
const dlMsg = ref('')

async function loadDl() {
  const s = await getAppSettings()
  if (s) dl.value = s
}
async function saveDl() {
  dlSaving.value = true
  dlMsg.value = ''
  try {
    const s = await setAppSettings(dl.value)
    if (s) dl.value = s
    dlMsg.value = '已保存（下次下载 / 安装时生效）'
  } catch (e) {
    dlMsg.value = e instanceof Error ? e.message : String(e)
  } finally {
    dlSaving.value = false
  }
}

/* ---------- 数据与连接 ---------- */
async function loadStInfo() {
  try {
    const st = await getSettings()
    if (st.username) {
      chat.userName = String(st.username)
      gen.stUserName = String(st.username)
    }
    // ST 确认在线后再做映射检测（否则只会得到一个无意义的连接错误）；映射仅开发者模式关心
    if (devMode.value) void gen.inspectMappingNow()
  } catch {
    /* ST 侧设置卡已移除，这里只做用户名同步，失败静默（在线状态由「系统状态」卡呈现） */
  }
}

/** 系统状态行（开发模式可见，浏览器开发环境 isDesktop = false 整卡隐藏） */
const sysRows = computed(() => {
  const s = sidecar.status
  if (!sidecar.isDesktop) return null
  return [
    { k: 'ST 状态', v: s ? `${s.state} · ${s.message}` : '—' },
    { k: 'ST 后端地址', v: s?.port ? `http://127.0.0.1:${s.port}` : '—' },
    { k: 'ST 前端地址', v: sidecar.relayBase || '—' },
    { k: 'ST 目录', v: s?.st_dir || '—' },
    { k: '组件安装目录', v: dl.value.install_root || '软件同目录 plugins（默认）' },
  ]
})

/** 映射表里的值可能是任意类型，统一成可读文本 */
function show(v: unknown): string {
  if (v === null || v === undefined) return '—'
  return typeof v === 'object' ? JSON.stringify(v) : String(v)
}

const THEMES: { mode: ThemeMode; name: string; desc: string }[] = [
  { mode: 'light', name: '浅色', desc: '默认主题（Indigo 主色）' },
  { mode: 'dark', name: '深色', desc: '夜间长时间阅读' },
  { mode: 'dusk', name: '暮光', desc: '示例皮肤 · 仅覆盖主色 token，验证换肤通道' },
]

/* ---------- 通用：字体与大小 ---------- */
const appearance = useAppearanceStore()

const FONT_SCALES: { key: FontScale; name: string }[] = [
  { key: 'sm', name: '小' },
  { key: 'md', name: '标准' },
  { key: 'lg', name: '大' },
  { key: 'xl', name: '特大' },
]

const FONT_FAMILIES: { value: string; name: string }[] = [
  { value: '', name: '系统默认' },
  { value: "'Microsoft YaHei'", name: '微软雅黑' },
  { value: "'PingFang SC'", name: '苹方（macOS）' },
  { value: "'Noto Sans SC', 'Source Han Sans SC'", name: '思源黑体' },
  { value: "'SimSun', serif", name: '宋体（衬线）' },
  { value: "'KaiTi', serif", name: '楷体' },
]

/* ---------- 通用：语言（i18n 落地前仅存偏好） ---------- */
const LANGS: { value: Lang; name: string; ready: boolean }[] = [
  { value: 'zh-CN', name: '简体中文', ready: true },
  { value: 'en-US', name: 'English', ready: false },
]

/* ---------- 通用：版本与更新 + 开发者模式彩蛋 ---------- */
const appVersion = ref('…')

onMounted(async () => {
  try {
    const { getVersion } = await import('@tauri-apps/api/app')
    appVersion.value = (await getVersion()) || 'dev'
  } catch {
    appVersion.value = 'dev（浏览器）'
  }
})

const tapCount = ref(0)
const tapHint = ref('')
let tapTimer: ReturnType<typeof setTimeout> | undefined

/** 连点版本号 7 次 = 切换开发者模式（Android 彩蛋）；3 秒无操作重置 */
function onVersionTap(): void {
  const goal = devMode.value ? '关闭' : '开启'
  tapCount.value++
  if (tapTimer) clearTimeout(tapTimer)
  tapTimer = setTimeout(() => {
    tapCount.value = 0
    tapHint.value = ''
  }, 3000)

  if (tapCount.value >= 7) {
    tapCount.value = 0
    setDev(!devMode.value)
    tapHint.value = devMode.value ? '已进入开发者模式' : '已退出开发者模式'
    return
  }
  const left = 7 - tapCount.value
  tapHint.value = left <= 4 ? `再点 ${left} 次${goal}开发者模式` : ''
}

const updateMsg = ref('')
function checkUpdate(): void {
  // 预留：App 未发布，无更新源可查；发布后接更新渠道
  updateMsg.value = `当前为开发版（${appVersion.value}），暂未提供更新检查`
}

onMounted(() => {
  void loadStInfo()
  void loadDl()
  void persona.load()
  onSourceChanged()
  textServerDraft.value = gen.textServer
  // auto-continue 当前配置
  void loadAutoContinue().then((ac) => {
    acEnabled.value = ac.enabled
    acTarget.value = ac.targetLength
  })
})
</script>

<template>
  <main class="main page" data-page="settings">
    <header class="head">
      <h3>
        {{ sec === 'api' ? '模型' : sec === 'theme' ? '通用' : '数据' }}
      </h3>
    </header>

    <div class="body">
      <!-- ============ 模型 ============ -->
      <template v-if="sec === 'api'">
        <section class="card">
          <h4>模型</h4>
          <label class="fld">
            <span class="lb">生成类型</span>
            <select class="field field-select" :value="gen.genType" @change="onGenTypeChanged(($event.target as HTMLSelectElement).value as 'chat' | 'text')">
              <option value="chat">Chat Completion（结构化消息，主流云端 API）</option>
              <option value="text">Text Completion（单字符串 prompt，本地/托管推理后端）</option>
            </select>
          </label>

          <!-- ===== Chat Completion 源 ===== -->
          <template v-if="gen.genType === 'chat'">
          <label class="fld">
            <span class="lb">选择供应商</span>
            <select class="field field-select" :value="gen.source" @change="pickSource(($event.target as HTMLSelectElement).value)">
              <option v-for="s in CHAT_SOURCES" :key="s.id" :value="s.id" :disabled="!!s.disabled">
                {{ s.disabled ? s.label + '（暂不支持）' : s.label }}
              </option>
            </select>
          </label>

          <!-- Vertex AI（Express）附加字段 -->
          <template v-if="gen.source === 'vertexai'">
            <div class="row2f">
              <label class="fld">
                <span class="lb">认证模式</span>
                <select class="field field-select" v-model="vaiAuthDraft" @change="saveConnExtras">
                  <option value="express">Express（API Key）</option>
                  <option value="full">Full（服务账号，需在 ST 侧配好）</option>
                </select>
              </label>
              <label class="fld">
                <span class="lb">区域</span>
                <input v-model="vaiRegionDraft" class="field" placeholder="us-central1" @change="saveConnExtras" />
              </label>
            </div>
            <label v-if="vaiAuthDraft === 'express'" class="fld">
              <span class="lb">Express 项目 ID（可选）</span>
              <input v-model="vaiProjectDraft" class="field" @change="saveConnExtras" />
            </label>
            <label v-if="vaiAuthDraft === 'full'" class="fld">
              <span class="lb">服务账号 JSON（写入 ST secrets）</span>
              <input v-model="keyDraft" class="field" type="password" autocomplete="off" placeholder='{"type": "service_account", ...}' />
            </label>
          </template>

          <!-- Azure OpenAI 附加字段 -->
          <template v-if="gen.source === 'azure_openai'">
            <label class="fld">
              <span class="lb">Azure Base URL</span>
              <input v-model="azBaseDraft" class="field" placeholder="https://<resource>.openai.azure.com" @change="saveConnExtras" />
            </label>
            <div class="row2f">
              <label class="fld">
                <span class="lb">部署名（Deployment）</span>
                <input v-model="azDeployDraft" class="field" @change="saveConnExtras" />
              </label>
              <label class="fld">
                <span class="lb">API 版本</span>
                <input v-model="azVersionDraft" class="field" placeholder="2024-02-15-preview" @change="saveConnExtras" />
              </label>
            </div>
          </template>

          <!-- Workers AI 附加字段 -->
          <label v-if="gen.source === 'workers_ai'" class="fld">
            <span class="lb">Cloudflare 账号 ID</span>
            <input v-model="cfAccountDraft" class="field" @change="saveConnExtras" />
          </label>

          <label v-if="gen.source !== 'vertexai' || vaiAuthDraft !== 'full'" class="fld">
            <span class="lb">API KEY</span>
            <input
              v-model="keyDraft"
              class="field"
              type="password"
              autocomplete="off"
              placeholder="••••••••"
            />
          </label>
          <label v-if="curDef?.customUrl" class="fld">
            <span class="lb">端点 Base URL</span>
            <input v-model="customUrlDraft" class="field" placeholder="https://…/v1" />
          </label>
          <label v-if="curDef?.reverseProxy" class="fld">
            <span class="lb">反向代理（可选）</span>
            <input v-model="revDraft" class="field" placeholder="https://your-proxy.example/v1" />
          </label>
          </template>

          <!-- ===== Text Completion 后端 ===== -->
          <template v-else>
            <label class="fld">
              <span class="lb">推理后端</span>
              <select class="field field-select" :value="gen.textBackend" @change="onTextBackendChanged(($event.target as HTMLSelectElement).value)">
                <option v-for="b in TEXTGEN_BACKENDS" :key="b.id" :value="b.id">{{ b.label }}</option>
              </select>
            </label>
            <label v-if="!textBackendDef?.fixedServer" class="fld">
              <span class="lb">API 服务器地址（含端口，不含路径）</span>
              <input v-model="textServerDraft" class="field" placeholder="http://127.0.0.1:5001" @change="saveTextServer" />
            </label>
            <p v-else class="hint">托管后端：{{ textBackendDef.fixedServer }}</p>
            <label v-if="textBackendDef?.secretKey" class="fld">
              <span class="lb">API KEY</span>
              <input v-model="keyDraft" class="field" type="password" autocomplete="off" placeholder="••••••••" />
            </label>
          </template>

          <div class="btns">
            <button class="btn" :disabled="connSaving || !keyDraft.trim()" @click="saveKey">
              保存密钥
            </button>
            <button class="btn" :disabled="pulling" @click="connect">
              {{ pulling ? '连接中…' : '连接并拉取模型' }}
            </button>
          </div>
          <p v-if="connMsg" class="rsp" :class="connOk ? 'okish' : 'err'">{{ connMsg }}</p>

          <div class="sep"></div>
          <label class="fld">
            <span class="lb">选择主模型</span>
            <select class="field field-select" v-model="gen.model" @mousedown="ensureModels" @change="gen.persist()">
              <option value="" disabled>{{ gen.modelOptions.length ? '请选择' : '连接并拉取模型后可选' }}</option>
              <option v-for="m in modelChoices" :key="m" :value="m">{{ m }}</option>
            </select>
          </label>
          <label class="fld">
            <span class="lb">选择备用模型（主模型过载时自动切换）</span>
            <select class="field field-select" v-model="gen.fallbackModel" @mousedown="ensureModels" @change="gen.persist()">
              <option value="">不启用</option>
              <option v-for="m in modelChoices" :key="m" :value="m">{{ m }}</option>
            </select>
          </label>
        </section>

        <PromptManagerCard />

        <section class="card">
          <h4>人设</h4>
          <label class="fld">
            <span class="lb">默认人设（未锁定会话时生效）</span>
            <select
              class="field"
              :value="persona.defaultId ?? ''"
              :disabled="!persona.personas.length"
              @change="void persona.usePersona(($event.target as HTMLSelectElement).value)"
            >
              <option v-if="!persona.personas.length" value="">暂无人设</option>
              <option v-for="p in persona.personas" :key="p.id" :value="p.id">{{ p.name }}</option>
            </select>
          </label>
          <p class="hint">新建会话默认使用该人设，单个会话内可在「··· → 信息」单独锁定。</p>
        </section>

        <section class="card">
          <h4>对话参数</h4>
          <label class="fld">
            <span class="lb">温度：{{ gen.temperature.toFixed(2) }}</span>
            <input
              v-model.number="gen.temperature"
              class="range"
              type="range"
              min="0"
              max="2"
              step="0.05"
              @change="gen.persist()"
            />
          </label>
          <div class="grid2">
            <label class="fld">
              <span class="lb">单次回复上限</span>
              <input v-model.number="gen.maxTokens" class="field" type="number" min="64" step="64" @change="gen.persist()" />
            </label>
            <label class="fld">
              <span class="lb">上下文窗口</span>
              <input v-model.number="gen.maxContext" class="field" type="number" min="1024" step="1024" @change="gen.persist()" />
            </label>
            <label class="fld">
              <span class="lb">Top P：{{ gen.topP.toFixed(2) }}</span>
              <input v-model.number="gen.topP" class="range" type="range" min="0" max="1" step="0.01" @change="gen.persist()" />
            </label>
            <label class="fld">
              <span class="lb">Top K（0 = 关闭）</span>
              <input v-model.number="gen.topK" class="field" type="number" min="0" max="200" step="1" @change="gen.persist()" />
            </label>
            <label class="fld">
              <span class="lb">Min P（0 = 关闭）</span>
              <input v-model.number="gen.minP" class="field" type="number" min="0" max="1" step="0.01" @change="gen.persist()" />
            </label>
            <label class="fld">
              <span class="lb">Top A（0 = 关闭）</span>
              <input v-model.number="gen.topA" class="field" type="number" min="0" max="1" step="0.01" @change="gen.persist()" />
            </label>
            <label class="fld">
              <span class="lb">频率惩罚</span>
              <input v-model.number="gen.frequencyPenalty" class="field" type="number" min="-2" max="2" step="0.05" @change="gen.persist()" />
            </label>
            <label class="fld">
              <span class="lb">存在惩罚</span>
              <input v-model.number="gen.presencePenalty" class="field" type="number" min="-2" max="2" step="0.05" @change="gen.persist()" />
            </label>
            <label class="fld">
              <span class="lb">重复惩罚</span>
              <input v-model.number="gen.repetitionPenalty" class="field" type="number" min="1" max="2" step="0.01" @change="gen.persist()" />
            </label>
          </div>
          <label class="chk">
            <input
              type="checkbox"
              :checked="gen.includeExamples"
              @change="gen.update({ includeExamples: ($event.target as HTMLInputElement).checked })"
            />
            <span>注入角色对话示例（few-shot）</span>
          </label>
        </section>

        <!-- 生成行为 -->
        <section class="card">
          <h4>生成行为</h4>
          <label class="chk">
            <input type="checkbox" v-model="acEnabled" />
            <span>自动续写（auto-continue）：回复过短时自动 Continue 补足</span>
          </label>
          <label class="fld">
            <span class="lb">目标回复 token 数（低于则续写）</span>
            <input v-model.number="acTarget" class="field" type="number" min="0" max="4096" :disabled="!acEnabled" />
          </label>
          <div class="btns">
            <button class="btn btn-primary" :disabled="acSaving" @click="persistAutoContinue">
              {{ acSaving ? '保存中…' : '保存生成行为' }}
            </button>
          </div>
          <p class="hint">配置存 power_user.auto_continue。</p>
        </section>

        <section class="card">
          <h4>连接自检</h4>
          <div class="btns">
            <button class="btn" :disabled="testing" @click="testConn">
              {{ testing ? '检测中…' : '检测后端连接' }}
            </button>
            <button class="btn" :disabled="genTesting" @click="testGenerate">
              {{ genTesting ? '生成中…' : '测试生成（消耗 1 次配额）' }}
            </button>
            <button class="btn" @click="gen.reset()">恢复默认参数</button>
          </div>
          <p v-if="testMsg" class="rsp" :class="testOk ? 'okish' : 'err'">{{ testMsg }}</p>
          <p v-if="genMsg" class="rsp" :class="genOk ? 'okish' : 'err'">{{ genMsg }}</p>
        </section>

        <section v-if="devMode" class="card">
          <h4>配置同步（开发者）</h4>
          <table v-if="gen.mapEntries.length" class="kv">
            <tbody>
              <tr v-for="e in gen.mapEntries" :key="e.stPath">
                <td class="k">{{ e.app }}</td>
                <td class="v">
                  <code>{{ e.stPath }}</code>
                  <span class="sub">
                    {{ show(e.appValue) }} → ST 现值 {{ show(e.stValue) }}
                  </span>
                </td>
                <td class="st" :class="{ on: e.inSync }">{{ e.inSync ? '一致' : '待同步' }}</td>
              </tr>
            </tbody>
          </table>
          <p v-else class="hint">尚未检测（需要 ST 在线）。</p>
          <div class="btns">
            <button class="btn" :disabled="gen.syncing" @click="gen.inspectMappingNow()">
              重新检测
            </button>
            <button class="btn" :disabled="gen.syncing" @click="gen.syncNow(true)">
              {{ gen.syncing ? '同步中…' : '立即同步' }}
            </button>
          </div>
          <p v-if="gen.mapError" class="rsp err">{{ gen.mapError }}</p>
          <p v-else-if="gen.mapEntries.length" class="rsp">{{ gen.mapSummary }}</p>
        </section>
      </template>

      <!-- ============ 通用 ============ -->
      <template v-else-if="sec === 'theme'">
        <section class="card">
          <h4>聊天背景</h4>
          <div class="btns">
            <button class="btn" :disabled="bgc.loading" @click="bgc.loadList(true)">
              {{ bgc.loading ? '加载中…' : '从 ST 加载背景列表' }}
            </button>
            <button class="btn" :class="{ 'btn-primary': !bgc.enabled }" @click="bgc.choose('')">不使用</button>
          </div>
          <p v-if="bgc.error" class="rsp err">{{ bgc.error }}</p>
          <div class="bgs">
            <button
              v-for="b in bgc.list"
              :key="b.filename"
              class="bgi"
              :class="{ on: bgc.name === b.filename }"
              @click="bgc.choose(b.filename)"
            >
              {{ b.filename }}
            </button>
          </div>
          <p class="hint">图片位于 ST 数据目录 backgrounds/ 下；选择后立即作为聊天页背景并记忆。</p>
        </section>

        <section class="card">
          <h4>主题外观</h4>
          <div class="themes">
            <button
              v-for="t in THEMES"
              :key="t.mode"
              class="thm"
              :class="{ on: theme.mode === t.mode }"
              @click="theme.setMode(t.mode)"
            >
              <span class="tn">{{ t.name }}</span>
              <span class="td">{{ t.desc }}</span>
            </button>
          </div>
        </section>

        <section class="card">
          <h4>字体与大小</h4>
          <div class="fld">
            <span class="lb">界面大小</span>
            <div class="seg">
              <button
                v-for="f in FONT_SCALES"
                :key="f.key"
                class="seg-item"
                :class="{ on: appearance.fontScale === f.key }"
                @click="appearance.setFontScale(f.key)"
              >
                {{ f.name }}
              </button>
            </div>
          </div>
          <label class="fld">
            <span class="lb">字体</span>
            <select
              class="field field-select"
              :value="appearance.fontFamily"
              @change="appearance.setFontFamily(($event.target as HTMLSelectElement).value)"
            >
              <option v-for="f in FONT_FAMILIES" :key="f.name" :value="f.value">{{ f.name }}</option>
            </select>
          </label>
          <p class="preview" :style="{ fontFamily: appearance.fontFamily || undefined }">
            预览 Aa：你好，SillyTavern。
          </p>
        </section>

        <section class="card">
          <h4>语言</h4>
          <div class="lang">
            <button
              v-for="l in LANGS"
              :key="l.value"
              class="lang-item"
              :class="{ on: appearance.lang === l.value }"
              :disabled="!l.ready"
              @click="appearance.setLang(l.value)"
            >
              <span class="ln">{{ l.name }}</span>
              <span v-if="!l.ready" class="lt">即将支持</span>
            </button>
          </div>
        </section>

        <section class="card">
          <h4>版本与更新</h4>
          <div class="row">
            <button class="ver" title="连续点击 7 次可开启开发者模式" @click="onVersionTap">
              版本 {{ appVersion }}
            </button>
            <button class="btn flex-none" @click="checkUpdate">检查更新</button>
          </div>
          <p v-if="tapHint" class="rsp" :class="{ ok: devMode }">{{ tapHint }}</p>
          <p v-if="updateMsg" class="hint">{{ updateMsg }}</p>
        </section>

        <!-- 扩展管理：正则脚本 / 快捷回复 / 翻译 / TTS（安装与列表在「插件」页） -->
        <ExtensionsCard />
      </template>

      <!-- ============ 数据与连接 ============ -->
      <template v-else>
        <section v-if="devMode && sysRows" class="card">
          <h4>系统状态</h4>
          <table class="kv">
            <tbody>
              <tr v-for="r in sysRows" :key="r.k">
                <td class="k">{{ r.k }}</td>
                <td class="v">{{ r.v }}</td>
              </tr>
            </tbody>
          </table>
        </section>

        <!-- 数据目录（简版）：位置/大小 + 导入导出；默认配置由应用自动生成 -->
        <DataDirCard />

        <!-- 数据巡检 -->
        <section class="card">
          <h4>数据巡检（data-maid）</h4>
          <p class="hint">扫描 ST 数据区的孤儿文件（未引用图片/备份/缩略图等），确认后一键清理。</p>
          <div class="btns">
            <button class="btn" :disabled="maid.busy" @click="runMaidReportUi">
              {{ maid.busy ? '扫描中…' : '生成巡检报告' }}
            </button>
            <button
              v-if="maid.report && maidCheckedHashes().length"
              class="btn"
              :disabled="maid.busy"
              @click="deleteSelectedMaidUi"
            >
              清理选中（{{ maidCheckedHashes().length }} 项）
            </button>
            <button
              v-if="maid.report"
              class="btn btn-primary"
              :disabled="maid.busy"
              @click="finalizeMaidUi"
            >
              确认清理全部（{{ maid.total }} 项）
            </button>
          </div>
          <table v-if="maid.report" class="kv">
            <tbody>
              <tr v-for="(list, group) in maid.report" :key="group">
                <td class="k">
                  <a href="javascript:void(0)" @click="maid.openGroup = maid.openGroup === String(group) ? '' : String(group)">
                    {{ MAID_GROUPS[group] ?? group }}
                  </a>
                </td>
                <td class="v">{{ list.length }} 个文件</td>
              </tr>
            </tbody>
          </table>
          <div v-if="maid.report && maid.openGroup && maid.report[maid.openGroup]?.length">
            <table class="kv">
              <tbody>
                <tr v-for="rec in maid.report[maid.openGroup]" :key="rec.hash">
                  <td><input v-model="maid.checked[rec.hash]" type="checkbox" /></td>
                  <td class="k">
                    {{ rec.name }}
                    <span v-if="rec.parent" class="crumb">（{{ rec.parent }}）</span>
                  </td>
                  <td class="v">{{ fmtMaidSize(rec.size) }}</td>
                  <td class="v"><a href="javascript:void(0)" @click="maidViewUi(rec.hash)">查看</a></td>
                </tr>
              </tbody>
            </table>
          </div>
          <p v-if="maid.msg" class="hint">{{ maid.msg }}</p>
        </section>

        <!-- 下载与安装设置：代理 / 组件目录 -->
        <section class="card">
          <h4>下载与安装</h4>
          <p class="hint">用于组件下载与依赖安装。</p>
          <label class="fld">
            <span class="lb">HTTP 代理（留空 = 直连）</span>
            <input v-model="dl.proxy" class="field" placeholder="http://127.0.0.1:7890" />
          </label>
          <label class="chk">
            <input v-model="dl.proxy_for_npm" type="checkbox" />
            <span>npm 安装依赖时也走该代理</span>
          </label>
          <label class="fld" style="margin-top: 10px">
            <span class="lb">组件安装目录</span>
            <input
              v-model="dl.install_root"
              class="field"
              placeholder="留空 = 软件同目录下的 plugins"
            />
            <span class="crumb">
              留空时装到软件所在目录；该目录不可写会自动改用 %LOCALAPPDATA%。
            </span>
          </label>
          <div class="btns">
            <button class="btn" :disabled="dlSaving" @click="saveDl">
              {{ dlSaving ? '保存中…' : '保存' }}
            </button>
            <span v-if="dlMsg" class="crumb">{{ dlMsg }}</span>
          </div>
        </section>
      </template>
    </div>

    <!-- maid /view 内嵌查看器（Tauri 下 window.open 无效；fetch 走中继带 token） -->
    <div v-if="maidViewer.open" class="viewer-mask" @click.self="maidViewer.open = false">
      <div class="viewer">
        <div class="viewer-head">
          <span class="viewer-title">{{ maidViewer.title }}</span>
          <button class="btn btn-sm" @click="maidViewer.open = false">关闭</button>
        </div>
        <pre class="viewer-body">{{ maidViewer.busy ? '读取中…' : maidViewer.content }}</pre>
      </div>
    </div>
  </main>
</template>

<style scoped>
/* ---- maid /view 查看器 ---- */
.viewer-mask {
  position: fixed;
  inset: 0;
  z-index: 200;
  background: rgb(0 0 0 / 45%);
  display: flex;
  align-items: center;
  justify-content: center;
}
.viewer {
  width: min(76vw, 860px);
  max-height: 80vh;
  display: flex;
  flex-direction: column;
  background: var(--c-panel, #fff);
  border: 1px solid var(--c-line, #ddd);
  border-radius: 10px;
  overflow: hidden;
}
.viewer-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  padding: 8px 14px;
  border-bottom: 1px solid var(--c-line, #ddd);
  font-size: 13px;
}
.viewer-title {
  font-weight: 500;
  word-break: break-all;
}
.viewer-body {
  margin: 0;
  padding: 12px 14px;
  overflow: auto;
  font-size: 12px;
  line-height: 1.5;
  white-space: pre-wrap;
  word-break: break-all;
  max-height: 70vh;
}

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
  font-weight: 600;
}
.crumb {
  font-size: 11px;
  color: var(--c-text-3);
}
.body {
  flex: 1;
  overflow-y: auto;
  padding: 14px 20px 30px;
  display: flex;
  flex-direction: column;
  gap: 12px;
}
/* .card 基础外观走全局 shortcut；这里只保留卡片内标题的子元素规则。
   ⚠ flex:none 防止 flex 纵向滚动容器把超高卡片压缩裁切（内容无法滚动） */
.card {
  flex: none;
}
.card h4 {
  font-size: 13px;
  font-weight: 600;
  margin-bottom: 9px;
}
/* .hint 外观走全局 shortcut；只保留本页的布局性底距 */
.hint {
  margin-bottom: 8px;
}
/* 聊天背景选择 */
.bgs {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  margin-bottom: 8px;
}
.bgi {
  max-width: 100%;
  padding: 4px 10px;
  font-size: 11px;
  border: 1px solid var(--c-border);
  border-radius: var(--radius-md);
  background: var(--c-panel);
  color: var(--c-text-2);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.bgi:hover {
  border-color: var(--p-400);
}
.bgi.on {
  border-color: var(--p-500);
  color: var(--p-600);
  background: var(--p-50);
}
/* 两列字段行（vertexai/azure 等附加字段） */
.row2f {
  display: flex;
  gap: 8px;
}
.row2f > .fld {
  flex: 1;
  min-width: 0;
}
.hint.mono,
code {
  font-family: var(--font-mono);
}
code {
  padding: 0 3px;
  font-size: 11px;
  border-radius: 4px;
  background: var(--c-panel);
  color: var(--c-text-2);
}
.radios {
  display: flex;
  flex-direction: column;
  gap: 7px;
}
.srcs {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  gap: 7px;
}
.src {
  padding: 8px 10px;
  font-size: 12px;
  text-align: left;
  border: 1px solid var(--c-border);
  border-radius: var(--radius-md);
  background: var(--c-bg);
  color: var(--c-text-2);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.src:hover:not(:disabled) {
  border-color: var(--p-400);
}
.src.on {
  border-color: var(--p-500);
  background: var(--p-50);
  color: var(--p-600);
  font-weight: 600;
}
[data-theme='dark'] .src.on,
[data-theme='dusk'] .src.on {
  background: color-mix(in srgb, var(--p-500) 14%, transparent);
}
.src.off {
  opacity: 0.45;
  cursor: not-allowed;
}
.sep {
  height: 1px;
  margin: 12px 0;
  background: var(--c-border);
}
.opt {
  font-weight: 400;
  color: var(--c-text-3);
}
.radio {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 7px;
  padding: 9px 11px;
  border: 1px solid var(--c-border);
  border-radius: var(--radius-md);
  background: var(--c-bg);
  cursor: pointer;
}
.radio.on {
  border-color: var(--p-500);
  background: var(--p-50);
}
[data-theme='dark'] .radio.on,
[data-theme='dusk'] .radio.on {
  background: color-mix(in srgb, var(--p-500) 14%, transparent);
}
.rt {
  font-size: 12px;
  font-weight: 600;
}
.rd {
  flex-basis: 100%;
  padding-left: 21px;
  font-size: 10px;
  color: var(--c-text-3);
}
.lb {
  display: block;
  font-size: 11px;
  font-weight: 600;
  color: var(--c-text-2);
  margin-bottom: 4px;
}
.in,
.range {
  width: 100%;
}
.in:focus {
  border-color: var(--p-400);
}
.range {
  accent-color: var(--p-500);
}
.grid2 {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 10px;
}
.chk {
  display: flex;
  align-items: flex-start;
  gap: 7px;
  font-size: 11px;
  color: var(--c-text-2);
  cursor: pointer;
}
.btns {
  display: flex;
  flex-wrap: wrap;
  gap: 7px;
}
.ghost:hover:not(:disabled) {
  border-color: var(--p-400);
  color: var(--p-600);
}
.ghost:disabled {
  opacity: 0.55;
  cursor: not-allowed;
}
.rsp {
  margin-top: 9px;
  padding: 7px 10px;
  font-size: 11px;
  line-height: 1.6;
  word-break: break-all;
  border-radius: var(--radius-md);
  background: var(--c-panel);
  color: var(--c-text-2);
}
.rsp.err {
  background: var(--s-error-bg);
  color: var(--s-error);
}
.themes {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  gap: 9px;
}
.thm {
  display: flex;
  flex-direction: column;
  gap: 3px;
  padding: 11px 12px;
  text-align: left;
  border: 1px solid var(--c-border);
  border-radius: var(--radius-md);
  background: var(--c-bg);
}
.thm.on {
  border-color: var(--p-500);
  background: var(--p-50);
}
[data-theme='dark'] .thm.on,
[data-theme='dusk'] .thm.on {
  background: color-mix(in srgb, var(--p-500) 14%, transparent);
}
.tn {
  font-size: 12px;
  font-weight: 600;
}
.td {
  font-size: 10px;
  color: var(--c-text-3);
  line-height: 1.6;
}
.row {
  display: flex;
  gap: 8px;
}
.kv {
  width: 100%;
  border-collapse: collapse;
  font-size: 12px;
  margin-top: 8px;
}
.kv td {
  padding: 5px 8px;
  border-bottom: 1px solid var(--c-border);
}
.k {
  width: 170px;
  color: var(--c-text-3);
}
.v {
  color: var(--c-text);
  font-family: var(--font-mono);
  word-break: break-all;
}
.st {
  width: 68px;
  text-align: right;
  font-size: 11px;
  white-space: nowrap;
  color: var(--c-text-3);
}
.st.on {
  color: var(--s-success);
}
.sub {
  display: block;
  margin-top: 2px;
  font-size: 10px;
  color: var(--c-text-3);
}
/* ---- 通用：字号分段选择 ---- */
.seg {
  display: flex;
  gap: 4px;
  padding: 3px;
  border-radius: var(--radius-md);
  background: var(--c-panel);
}
.seg-item {
  flex: 1;
  padding: 6px 0;
  font-size: 12px;
  text-align: center;
  border-radius: calc(var(--radius-md) - 2px);
  color: var(--c-text-2);
  transition:
    background 0.15s,
    color 0.15s;
}
.seg-item:hover {
  color: var(--c-text);
}
.seg-item.on {
  background: var(--c-bg);
  color: var(--p-600);
  font-weight: 600;
  box-shadow: 0 1px 3px var(--c-shadow);
}
[data-theme='dark'] .seg-item.on,
[data-theme='dusk'] .seg-item.on {
  background: color-mix(in srgb, var(--p-500) 18%, transparent);
  color: var(--p-300);
  box-shadow: none;
}
.preview {
  margin-top: 10px;
  padding: 9px 11px;
  font-size: 13px;
  border-radius: var(--radius-md);
  background: var(--c-panel);
  color: var(--c-text-2);
}
/* ---- 通用：语言 ---- */
.lang {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.lang-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 9px 11px;
  font-size: 12px;
  text-align: left;
  border: 1px solid var(--c-border);
  border-radius: var(--radius-md);
  background: var(--c-bg);
  color: var(--c-text-2);
}
.lang-item:hover:not(:disabled) {
  border-color: var(--p-400);
}
.lang-item.on {
  border-color: var(--p-500);
  background: var(--p-50);
  color: var(--p-600);
  font-weight: 600;
}
[data-theme='dark'] .lang-item.on,
[data-theme='dusk'] .lang-item.on {
  background: color-mix(in srgb, var(--p-500) 14%, transparent);
  color: var(--p-300);
}
.lang-item:disabled {
  opacity: 0.55;
  cursor: not-allowed;
}
.lt {
  font-size: 10px;
  color: var(--c-text-3);
}
/* ---- 通用：版本与更新（版本号即彩蛋入口，长得别像按钮） ---- */
.ver {
  padding: 4px 2px;
  font-size: 12px;
  color: var(--c-text-2);
  user-select: none;
  cursor: default;
}
.ver:hover {
  color: var(--c-text);
}
</style>
