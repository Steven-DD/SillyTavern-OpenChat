<script setup lang="ts">
/**
 * 扩展管理卡片：挂「设置 → 通用」。
 * - 正则脚本 CRUD（settings.json extension_settings.regex，与 ST 互通）
 * - 快捷回复 CRUD（extension_settings.stchat_quick_replies，App 自有键）
 * - 翻译设置（extension_settings.stchat_translate；8 家供应商走 ST 服务端）
 * - TTS 设置与试听（/api/speech/synthesize，本地 transformers 引擎）
 * 扩展的安装/搜索/列表/详情在「插件」页。
 */
import { onMounted, ref } from 'vue'
import {
  loadRegexScripts,
  saveRegexScripts,
  newRegexScript,
  type RegexScript,
} from '@/services/st/regex'
import {
  loadQuickReplies,
  saveQuickReplies,
  listQrPresets,
  importQrPreset,
  exportQrPreset,
  type QuickReply,
  type StQrPreset,
} from '@/services/st/extensions'
import {
  loadMemoryAppEnabled,
  loadMemorySettings,
  saveMemoryAppEnabled,
  saveMemorySettings,
} from '@/services/st/memory'
import {
  loadVectorSettings,
  loadVectorsAppEnabled,
  saveVectorSettings,
  saveVectorsAppEnabled,
} from '@/services/st/vectors'
import {
  loadTranslateSettings,
  saveTranslateSettings,
  TRANSLATE_PROVIDERS,
} from '@/services/st/translate'
import { loadSpeechSettings, saveSpeechSettings, speakText, stopSpeech, isSpeaking } from '@/services/st/speech'
import { loadInstruct, type InstructSettings } from '@/services/st/instruct'

const msg = ref('')
const msgOk = ref(false)
function note(text: string, ok = true): void {
  msg.value = text
  msgOk.value = ok
}

/* ---- 正则脚本 ---- */
const scripts = ref<RegexScript[]>([])
const scriptsOpen = ref<Set<number>>(new Set())

async function reloadScripts(): Promise<void> {
  scripts.value = await loadRegexScripts(true)
}

function addScript(): void {
  scripts.value = [...scripts.value, newRegexScript()]
  scriptsOpen.value = new Set([...scriptsOpen.value, scripts.value.length - 1])
}

async function persistScripts(): Promise<void> {
  try {
    await saveRegexScripts(scripts.value)
    note('正则脚本已保存（即时生效）')
  } catch (e) {
    note(`保存失败：${e instanceof Error ? e.message : String(e)}`, false)
  }
}

function removeScript(i: number): void {
  scripts.value = scripts.value.filter((_, k) => k !== i)
}

function toggleOpen(i: number): void {
  const next = new Set(scriptsOpen.value)
  if (next.has(i)) next.delete(i)
  else next.add(i)
  scriptsOpen.value = next
}

const PLACEMENT_OPTIONS = [
  { id: 1, label: '用户输入' },
  { id: 2, label: 'AI 输出' },
  { id: 5, label: '世界书' },
  { id: 6, label: '推理内容' },
]

/* ---- 快捷回复 ---- */
const qrs = ref<QuickReply[]>([])

async function reloadQrs(): Promise<void> {
  qrs.value = await loadQuickReplies(true)
}

function addQr(): void {
  qrs.value = [...qrs.value, { label: '新按钮', message: '', enabled: true }]
}

function removeQr(i: number): void {
  qrs.value = qrs.value.filter((_, k) => k !== i)
}

async function persistQrs(): Promise<void> {
  try {
    await saveQuickReplies(qrs.value)
    note('快捷回复已保存')
  } catch (e) {
    note(`保存失败：${e instanceof Error ? e.message : String(e)}`, false)
  }
}

/* ---- ST 原生 QR 集（QuickReplies/*.json 文件互通，B5） ---- */
const stPresets = ref<StQrPreset[]>([])
const presetPick = ref('')
const exportName = ref('')
const presetBusy = ref(false)

async function reloadPresets(): Promise<void> {
  stPresets.value = await listQrPresets()
}

async function doImportPreset(): Promise<void> {
  if (!presetPick.value || presetBusy.value) return
  presetBusy.value = true
  try {
    const n = await importQrPreset(presetPick.value)
    await reloadQrs()
    note(`已从「${presetPick.value}」导入 ${n} 个按钮（重复 label 自动跳过）`)
  } catch (e) {
    note(`导入失败：${e instanceof Error ? e.message : String(e)}`, false)
  } finally {
    presetBusy.value = false
  }
}

async function doExportPreset(): Promise<void> {
  const name = exportName.value.trim()
  if (!name || presetBusy.value) return
  presetBusy.value = true
  try {
    await exportQrPreset(name, qrs.value)
    await reloadPresets()
    note(`已导出为 ST 集「${name}」（QuickReplies/${name}.json；需在网页端 QR 管理器导入后生效）`)
  } catch (e) {
    note(`导出失败：${e instanceof Error ? e.message : String(e)}`, false)
  } finally {
    presetBusy.value = false
  }
}

/* ---- Instruct 模板（只读透传，B5；编辑在 ST 网页端） ---- */
const instruct = ref<InstructSettings | null>(null)

/* ---- 翻译 / TTS ---- */
const provider = ref('google')
const targetLang = ref('zh')
const ttsModel = ref('')
const ttsBusy = ref(false)

/* ---- 记忆（Summarize）/ 向量记忆（B3） ---- */
const memEnabled = ref(false)
const memInterval = ref(10)
const memDepth = ref(2)
const memPosition = ref(1)
const vecEnabled = ref(false)
const vecTopK = ref(4)
const vecDepth = ref(4)

async function persistMemory(): Promise<void> {
  try {
    await saveMemoryAppEnabled(memEnabled.value)
    await saveMemorySettings({
      interval: Math.max(1, Math.floor(memInterval.value) || 1),
      depth: Math.max(0, Math.floor(memDepth.value) || 0),
      position: memPosition.value,
    })
    note('记忆设置已保存')
  } catch (e) {
    note(`保存失败：${e instanceof Error ? e.message : String(e)}`, false)
  }
}

async function persistVectors(): Promise<void> {
  try {
    await saveVectorsAppEnabled(vecEnabled.value)
    await saveVectorSettings({
      top_k: Math.max(1, Math.floor(vecTopK.value) || 4),
      depth: Math.max(0, Math.floor(vecDepth.value) || 0),
    })
    note('向量记忆设置已保存')
  } catch (e) {
    note(`保存失败：${e instanceof Error ? e.message : String(e)}`, false)
  }
}

async function persistTranslate(): Promise<void> {
  try {
    await saveTranslateSettings({ provider: provider.value, target_language: targetLang.value.trim() || 'zh' })
    note('翻译设置已保存')
  } catch (e) {
    note(`保存失败：${e instanceof Error ? e.message : String(e)}`, false)
  }
}

async function persistTts(): Promise<void> {
  try {
    await saveSpeechSettings({ model: ttsModel.value.trim() })
    note('TTS 模型已保存')
  } catch (e) {
    note(`保存失败：${e instanceof Error ? e.message : String(e)}`, false)
  }
}

async function testTts(): Promise<void> {
  ttsBusy.value = true
  try {
    if (isSpeaking()) stopSpeech()
    else await speakText('你好，这是语音朗读测试。')
  } catch (e) {
    note(`合成失败（首次使用会下载模型，请稍后重试）：${e instanceof Error ? e.message : String(e)}`, false)
  } finally {
    ttsBusy.value = false
  }
}

onMounted(async () => {
  instruct.value = await loadInstruct().catch(() => null)
  await Promise.all([reloadScripts(), reloadQrs(), reloadPresets(), loadTranslateSettings(true), loadSpeechSettings(true)])
  provider.value = (await loadTranslateSettings()).provider
  targetLang.value = (await loadTranslateSettings()).target_language
  ttsModel.value = (await loadSpeechSettings()).model
  // 记忆 / 向量（B3）
  memEnabled.value = await loadMemoryAppEnabled()
  const mem = await loadMemorySettings(true)
  memInterval.value = mem.interval
  memDepth.value = mem.depth
  memPosition.value = mem.position
  vecEnabled.value = await loadVectorsAppEnabled()
  const vec = await loadVectorSettings(true)
  vecTopK.value = vec.top_k
  vecDepth.value = vec.depth
})
</script>

<template>
  <section class="card ext">
    <h4>插件功能（正则 / 快捷回复 / 翻译 / TTS）</h4>

    <!-- 正则脚本 -->
    <div class="sub">
      <div class="sub-h">
        <span>正则脚本（与 ST 数据互通）</span>
        <span class="grow" />
        <button class="btn btn-sm" @click="addScript">＋ 新增</button>
        <button class="btn btn-sm btn-primary" @click="persistScripts">保存</button>
      </div>
      <p v-if="!scripts.length" class="hint">暂无脚本。脚本可对 AI 输出 / 用户输入做查找替换（支持 /正则/flags）。</p>
      <div v-for="(sc, i) in scripts" :key="sc.id || i" class="script">
        <div class="script-h" role="button" @click="toggleOpen(i)">
          <span class="chev" :class="{ open: scriptsOpen.has(i) }">▸</span>
          <span class="script-name">{{ sc.scriptName || '未命名' }}</span>
          <span v-if="sc.disabled" class="bd">停用</span>
          <span v-if="sc.promptOnly" class="bd">仅提示词</span>
          <span v-if="sc.markdownOnly" class="bd">仅显示</span>
        </div>
        <div v-if="scriptsOpen.has(i)" class="script-body">
          <label class="fld"><span class="lb">名称</span>
            <input v-model="sc.scriptName" class="field" />
          </label>
          <label class="fld"><span class="lb">查找（正则，支持 /pattern/flags）</span>
            <input v-model="sc.findRegex" class="field" />
          </label>
          <label class="fld"><span class="lb">替换为（支持 $1、{{ '\{\{match\}\}' }}）</span>
            <input v-model="sc.replaceString" class="field" />
          </label>
          <div class="chk-row">
            <label class="chk"><input v-model="sc.disabled" type="checkbox" /> 停用</label>
            <label class="chk"><input v-model="sc.promptOnly" type="checkbox" /> 仅提示词</label>
            <label class="chk"><input v-model="sc.markdownOnly" type="checkbox" /> 仅显示</label>
            <label class="chk"><input v-model="sc.runOnEdit" type="checkbox" /> 编辑时重跑</label>
          </div>
          <div class="chk-row">
            <span class="lb">作用位置：</span>
            <label v-for="p in PLACEMENT_OPTIONS" :key="p.id" class="chk">
              <input type="checkbox" :checked="sc.placement.includes(p.id)"
                @change="($event.target as HTMLInputElement).checked
                  ? sc.placement.push(p.id)
                  : (sc.placement = sc.placement.filter((x) => x !== p.id))" />
              {{ p.label }}
            </label>
          </div>
          <div class="row2f">
            <label class="fld"><span class="lb">最小深度（空 = 不限）</span>
              <input :value="sc.minDepth ?? ''" class="field" type="number" min="0"
                @change="sc.minDepth = ($event.target as HTMLInputElement).value === '' ? null : Number(($event.target as HTMLInputElement).value)" />
            </label>
            <label class="fld"><span class="lb">最大深度</span>
              <input :value="sc.maxDepth ?? ''" class="field" type="number" min="0"
                @change="sc.maxDepth = ($event.target as HTMLInputElement).value === '' ? null : Number(($event.target as HTMLInputElement).value)" />
            </label>
          </div>
          <div class="btns">
            <button class="btn btn-sm danger-text" @click="removeScript(i)">删除脚本</button>
          </div>
        </div>
      </div>
    </div>

    <!-- 快捷回复 -->
    <div class="sub">
      <div class="sub-h">
        <span>快捷回复（聊天输入区按钮条）</span>
        <span class="grow" />
        <button class="btn btn-sm" @click="addQr">＋ 新增</button>
        <button class="btn btn-sm btn-primary" @click="persistQrs">保存</button>
      </div>
      <p v-if="!qrs.length" class="hint">暂无按钮。按钮点击后直接把「消息」发送给当前角色。</p>
      <div v-for="(q, i) in qrs" :key="i" class="qr-row">
        <input v-model="q.label" class="field w-label" placeholder="按钮文字" />
        <input v-model="q.message" class="field grow" placeholder="内容" />
        <select v-model="q.mode" class="field w-mode" title="点击行为">
          <option value="send">发送</option>
          <option value="insert">填入输入框</option>
        </select>
        <label class="chk"><input v-model="q.enabled" type="checkbox" /> 启用</label>
        <button class="btn btn-sm danger-text" @click="removeQr(i)">删</button>
      </div>
      <!-- ST 原生集文件互通 -->
      <div class="row2f">
        <label class="fld">
          <span class="lb">ST 集文件（{{ stPresets.length }}）</span>
          <div class="qr-row">
            <select v-model="presetPick" class="field field-select grow">
              <option value="">选择要导入的 ST 集…</option>
              <option v-for="p in stPresets" :key="p.name" :value="p.name">{{ p.name }}</option>
            </select>
            <button class="btn btn-sm" :disabled="!presetPick || presetBusy" @click="doImportPreset">导入</button>
          </div>
        </label>
        <label class="fld">
          <span class="lb">导出为 ST 集（名称）</span>
          <div class="qr-row">
            <input v-model="exportName" class="field grow" placeholder="如 stchat-default" />
            <button class="btn btn-sm" :disabled="!exportName.trim() || presetBusy" @click="doExportPreset">导出</button>
          </div>
        </label>
      </div>
      <p class="hint">ST 集文件存 QuickReplies/*.json：导入会把 qrList 追加进上方按钮条；导出补齐 version/idIndex/isHidden 等网页端字段（网页端运行数据源是 settings.quickReplyPresets，导出文件需在其 QR 管理器手动导入）</p>
    </div>

    <!-- Instruct 模板（只读） -->
    <div v-if="instruct" class="sub">
      <div class="sub-h"><span>Instruct 模板（Text Completion）</span></div>
      <div class="chk-row">
        <label class="chk"><input :checked="!!instruct.enabled" type="checkbox" disabled /> 已启用</label>
        <span class="ext-item">预设：{{ instruct.preset || '—' }}</span>
        <span class="ext-item">名字：{{ instruct.names_behavior || 'none' }}</span>
        <span class="ext-item">wrap：{{ instruct.wrap === false ? '关' : '开' }}</span>
      </div>
      <p class="hint">
        模板来自 ST 网页端配置（power_user.instruct，两端同一份数据）；App 在 Text Completion
        生成时按此模板拼接 prompt（含首/末序列、{{ '\{\{name\}\}' }} 宏与 stop 序列）。模板编辑请在 ST 网页端进行。
      </p>
    </div>

    <!-- 翻译 / TTS -->
    <div class="sub">
      <div class="sub-h"><span>消息翻译</span></div>
      <div class="row2f">
        <label class="fld">
          <span class="lb">供应商</span>
          <select v-model="provider" class="field field-select">
            <option v-for="p in TRANSLATE_PROVIDERS" :key="p.id" :value="p.id">{{ p.label }}</option>
          </select>
        </label>
        <label class="fld">
          <span class="lb">目标语言</span>
          <input v-model="targetLang" class="field" placeholder="zh / en / ja…" />
        </label>
      </div>
      <div class="btns">
        <button class="btn btn-sm btn-primary" @click="persistTranslate">保存翻译设置</button>
      </div>
      <p class="hint">密钥（DeepL/Libre 等）在 ST 网页端配置，由服务端托管；消息气泡下方悬停工具条点「译」使用。</p>
    </div>

    <div class="sub">
      <div class="sub-h"><span>TTS 朗读（本地引擎）</span></div>
      <label class="fld">
        <span class="lb">模型</span>
        <input v-model="ttsModel" class="field" placeholder="Xenova/speecht5_tts" />
      </label>
      <div class="btns">
        <button class="btn btn-sm" @click="persistTts">保存</button>
        <button class="btn btn-sm" :disabled="ttsBusy" @click="testTts">
          {{ ttsBusy ? '合成中…' : '试听' }}
        </button>
      </div>
      <p class="hint">首次合成会下载语音模型（较慢）。消息工具条点 🔊 朗读 / 再点停止。</p>

    <!-- 记忆（Summarize） -->
    <div class="sub">
      <div class="sub-h"><span>自动记忆（Summarize）</span></div>
      <div class="row2f">
        <label class="fld">
          <span class="lb">启用</span>
          <input type="checkbox" v-model="memEnabled" />
        </label>
        <label class="fld">
          <span class="lb">更新频率（每 N 条消息）</span>
          <input v-model.number="memInterval" class="field" type="number" min="1" max="99" />
        </label>
      </div>
      <div class="row2f">
        <label class="fld">
          <span class="lb">注入位置</span>
          <select v-model.number="memPosition" class="field field-select">
            <option :value="1">聊天内 @Depth（IN_CHAT）</option>
            <option :value="0">系统提示顶部（IN_PROMPT）</option>
            <option :value="2">作者注释槽位（IN_NOTE）</option>
            <option :value="3">发送前追加（IN_API）</option>
          </select>
        </label>
        <label class="fld">
          <span class="lb">注入深度</span>
          <input v-model.number="memDepth" class="field" type="number" min="0" :disabled="memPosition !== 1" />
        </label>
      </div>
      <div class="btns">
        <button class="btn btn-sm btn-primary" @click="persistMemory">保存记忆设置</button>
      </div>
      <p class="hint">
        每 N 条消息自动生成聊天摘要并注入提示词；摘要存 chat_metadata.summary（与 ST memory 扩展互通）。
        手动更新在会话配置 → 记忆 Tab。默认关闭（自动更新会消耗生成配额）。
      </p>
    </div>

    <!-- 向量记忆 -->
    <div class="sub">
      <div class="sub-h"><span>向量记忆（Vector Storage）</span></div>
      <div class="row2f">
        <label class="fld">
          <span class="lb">启用</span>
          <input type="checkbox" v-model="vecEnabled" />
        </label>
        <label class="fld">
          <span class="lb">检索条数（Top K）</span>
          <input v-model.number="vecTopK" class="field" type="number" min="1" max="20" />
        </label>
        <label class="fld">
          <span class="lb">注入深度</span>
          <input v-model.number="vecDepth" class="field" type="number" min="0" />
        </label>
      </div>
      <div class="btns">
        <button class="btn btn-sm btn-primary" @click="persistVectors">保存向量设置</button>
      </div>
      <p class="hint">
        消息自动向量化（ST 服务端本地嵌入），生成前按当前输入检索相关历史并注入。
        数据存 ST 服务端 vectors/ 目录。
      </p>
    </div>
    </div>

    <p v-if="msg" class="rsp" :class="msgOk ? 'okish' : 'err'">{{ msg }}</p>
  </section>
</template>

<style scoped>
.ext {
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.sub {
  border: 1px dashed var(--c-border);
  border-radius: var(--radius-md);
  padding: 10px;
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.sub-h {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 12px;
  font-weight: 600;
  color: var(--c-text);
}
.grow {
  flex: 1;
  min-width: 0;
}
.ext-list {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}
.ext-item {
  padding: 2px 8px;
  font-size: 11px;
  border: 1px solid var(--c-border);
  border-radius: var(--radius-md);
  background: var(--c-panel);
  color: var(--c-text-2);
}
.ext-type {
  margin-left: 5px;
  font-size: 9px;
  font-style: normal;
  color: var(--c-text-3);
}
.script {
  border: 1px solid var(--c-border);
  border-radius: var(--radius-md);
  overflow: hidden;
}
.script-h {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 7px 9px;
  cursor: pointer;
  font-size: 12px;
  background: var(--c-panel);
}
.script-h:hover {
  filter: brightness(0.97);
}
.chev {
  color: var(--c-text-3);
  transition: transform 0.15s;
}
.chev.open {
  transform: rotate(90deg);
}
.script-name {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.bd {
  font-size: 9px;
  padding: 0 5px;
  border-radius: 8px;
  background: var(--c-bg);
  color: var(--c-text-3);
}
.script-body {
  padding: 10px;
  display: flex;
  flex-direction: column;
  gap: 8px;
  border-top: 1px solid var(--c-border);
}
.chk-row {
  display: flex;
  align-items: center;
  gap: 12px;
  flex-wrap: wrap;
}
.chk {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  font-size: 11px;
  color: var(--c-text-2);
}
.row2f {
  display: flex;
  gap: 8px;
}
.row2f > .fld {
  flex: 1;
  min-width: 0;
}
.qr-row {
  display: flex;
  align-items: center;
  gap: 6px;
}
.w-label {
  width: 110px;
  flex: none;
}
.w-mode {
  width: 104px;
  flex: none;
}
</style>
