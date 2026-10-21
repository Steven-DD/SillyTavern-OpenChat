<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import {
  formatInstructChat,
  loadInstruct,
  saveInstruct,
  type InstructSettings,
} from '@/services/st/instruct'
import {
  INSTRUCT_PRESETS,
  loadUserInstructPresets,
  saveUserInstructPreset,
  deleteUserInstructPreset,
  importInstructPresetFile,
  exportInstructPresetFile,
  type InstructPresetItem,
} from '@/services/st/instructPresets'
import type { PromptMessage } from '@/services/st/prompt'

const loading = ref(true)
const saving = ref(false)
const savedTip = ref(false)
const userPresets = ref<InstructPresetItem[]>([])

const allPresets = computed(() => [...INSTRUCT_PRESETS, ...userPresets.value])

const form = ref<InstructSettings>({
  enabled: false,
  preset: 'Default',
  wrap: true,
  names_behavior: 'none',
  system_sequence: '',
  system_suffix: '\n',
  input_sequence: '',
  input_suffix: '\n',
  output_sequence: '',
  output_suffix: '\n',
  first_input_sequence: '',
  last_input_sequence: '',
  first_output_sequence: '',
  last_output_sequence: '',
  system_same_as_user: false,
  stop_sequence: '',
})

const selectedPresetId = ref('default')

async function reloadUserPresets() {
  userPresets.value = await loadUserInstructPresets()
}

onMounted(async () => {
  try {
    await reloadUserPresets()
    const s = await loadInstruct()
    if (s && Object.keys(s).length) {
      form.value = { ...form.value, ...s }
      const matched = allPresets.value.find(
        (p) => p.settings.preset?.toLowerCase() === s.preset?.toLowerCase(),
      )
      if (matched) selectedPresetId.value = matched.id
      else selectedPresetId.value = 'custom'
    }
  } catch (e) {
    console.warn('加载 Instruct 设置失败：', e)
  } finally {
    loading.value = false
  }
})

function onPresetChange() {
  const p = allPresets.value.find((x) => x.id === selectedPresetId.value)
  if (!p) return
  form.value = {
    ...form.value,
    ...p.settings,
    enabled: true,
  }
}

async function onSave() {
  saving.value = true
  try {
    await saveInstruct(form.value)
    if (form.value.preset && form.value.preset !== 'Default' && form.value.preset !== 'Custom') {
      await saveUserInstructPreset(form.value.preset, form.value)
      await reloadUserPresets()
    }
    savedTip.value = true
    setTimeout(() => (savedTip.value = false), 2000)
  } catch (e) {
    alert(`保存失败：${e instanceof Error ? e.message : String(e)}`)
  } finally {
    saving.value = false
  }
}

async function onSaveAs() {
  const name = prompt('请输入新 Instruct 预设名称：', form.value.preset || 'My_Instruct')
  if (!name || !name.trim()) return
  const cleanName = name.trim()
  form.value.preset = cleanName
  await saveUserInstructPreset(cleanName, form.value)
  await reloadUserPresets()
  selectedPresetId.value = `user_${cleanName}`
  await saveInstruct(form.value)
  savedTip.value = true
  setTimeout(() => (savedTip.value = false), 2000)
}

function onExport() {
  exportInstructPresetFile(form.value.preset || 'instruct', form.value)
}

async function onImport(e: Event) {
  const target = e.target as HTMLInputElement
  const file = target.files?.[0]
  if (!file) return
  try {
    const item = await importInstructPresetFile(file)
    await reloadUserPresets()
    selectedPresetId.value = item.id
    form.value = { ...item.settings }
    await saveInstruct(form.value)
    alert(`成功导入指令预设「${item.name}」`)
  } catch (err) {
    alert(`导入失败：${err instanceof Error ? err.message : String(err)}`)
  } finally {
    target.value = ''
  }
}

async function onDeleteCurrent() {
  const matched = userPresets.value.find((p) => p.id === selectedPresetId.value)
  if (!matched) {
    alert('内置预设不可删除')
    return
  }
  const cleanName = matched.settings.preset || matched.name
  if (!confirm(`确定删除指令预设「${cleanName}」吗？`)) return
  await deleteUserInstructPreset(cleanName)
  await reloadUserPresets()
  selectedPresetId.value = 'default'
  onPresetChange()
  await saveInstruct(form.value)
}

// 模拟三轮对话测试样本
const sampleMessages: PromptMessage[] = [
  { role: 'system', content: '你是一位精通角色扮演的 AI 助手。' },
  { role: 'user', content: '你好，今天天气怎么样？' },
  { role: 'assistant', content: '今天阳光明媚，微风正好，非常适合出门散步！' },
  { role: 'user', content: '那我们一起去湖边公园吧。' },
]

// 实时预览组装后的原始提示词
const livePreview = computed(() => {
  try {
    const r = formatInstructChat(
      sampleMessages,
      form.value,
      '旅行者',
      '派蒙',
      {},
    )
    return {
      text: r.text,
      stops: r.stop,
    }
  } catch (e) {
    return {
      text: `格式化出错：${e instanceof Error ? e.message : String(e)}`,
      stops: [],
    }
  }
})
</script>

<template>
  <div class="instruct-card">
    <div class="card-header">
      <div class="title-wrap">
        <h3 class="card-title">提示词格式与指令模式 (Instruct & Context)</h3>
        <p class="card-desc">
          为本地离线开源模型（Llama 3 / Qwen / Mistral / Gemma）精确定制 System / User / Assistant 标记包裹序列与 Stop Tokens。
        </p>
      </div>
      <div class="head-actions">
        <label class="switch-lbl">
          <input v-model="form.enabled" type="checkbox" />
          <span>启用 Instruct 包装</span>
        </label>
      </div>
    </div>

    <div v-if="loading" class="loading-box">加载配置中…</div>

    <div v-else class="card-body">
      <!-- 预设选择与核心参数 -->
      <div class="row-presets">
        <div class="preset-top-bar">
          <label class="fld grow">
            <span class="lb">选择指令预设 (Preset)：</span>
            <select v-model="selectedPresetId" class="in-sel" @change="onPresetChange">
              <optgroup label="内置标准预设">
                <option v-for="p in INSTRUCT_PRESETS" :key="p.id" :value="p.id">
                  {{ p.name }}
                </option>
              </optgroup>
              <optgroup v-if="userPresets.length" label="自定义/导入预设">
                <option v-for="p in userPresets" :key="p.id" :value="p.id">
                  {{ p.name }}
                </option>
              </optgroup>
              <option value="custom">自定义编辑 (Custom)</option>
            </select>
          </label>
          <div class="btn-group">
            <button class="btn btn-sm" @click="onSaveAs">另存为新预设</button>
            <button class="btn btn-sm" @click="onExport">导出 (.json)</button>
            <label class="btn btn-sm btn-file">
              导入 (.json)
              <input type="file" accept=".json" class="file-hidden" @change="onImport" />
            </label>
            <button
              class="btn btn-sm btn-danger"
              :disabled="!userPresets.some((u) => u.id === selectedPresetId)"
              @click="onDeleteCurrent"
            >
              删除
            </button>
          </div>
        </div>

        <label class="fld">
          <span class="lb">发言人名字处理 (Names)：</span>
          <select v-model="form.names_behavior" class="in-sel">
            <option value="none">none（不添加名字前缀）</option>
            <option value="always">always（始终追加角色名）</option>
            <option value="force">force（强制模式）</option>
          </select>
        </label>

        <label class="fld-chk">
          <input v-model="form.wrap" type="checkbox" />
          <span>换行回绕 (Wrap fragments with newline)</span>
        </label>

        <label class="fld-chk">
          <input v-model="form.system_same_as_user" type="checkbox" />
          <span>系统指令与用户格式一致 (System same as user)</span>
        </label>
      </div>

      <!-- 详细序列模板编辑 -->
      <div class="seq-grid">
        <!-- System -->
        <div class="grid-col">
          <h4 class="col-title">系统提示词序列 (System)</h4>
          <label class="fld">
            <span class="lb">前缀 (Prefix)：</span>
            <input v-model="form.system_sequence" class="in-txt" placeholder="<|start_header_id|>system<|end_header_id|>\n\n" />
          </label>
          <label class="fld">
            <span class="lb">后缀 (Suffix)：</span>
            <input v-model="form.system_suffix" class="in-txt" placeholder="<|eot_id|>" />
          </label>
        </div>

        <!-- User -->
        <div class="grid-col">
          <h4 class="col-title">用户输入序列 (User / Input)</h4>
          <label class="fld">
            <span class="lb">普通前缀 (Prefix)：</span>
            <input v-model="form.input_sequence" class="in-txt" placeholder="<|start_header_id|>user<|end_header_id|>\n\n" />
          </label>
          <label class="fld">
            <span class="lb">普通后缀 (Suffix)：</span>
            <input v-model="form.input_suffix" class="in-txt" placeholder="<|eot_id|>" />
          </label>
          <label class="fld">
            <span class="lb">末条用户前缀 (Last Input)：</span>
            <input v-model="form.last_input_sequence" class="in-txt" placeholder="留空默认等于普通前缀" />
          </label>
        </div>

        <!-- Assistant -->
        <div class="grid-col">
          <h4 class="col-title">AI 输出序列 (Assistant / Output)</h4>
          <label class="fld">
            <span class="lb">普通前缀 (Prefix)：</span>
            <input v-model="form.output_sequence" class="in-txt" placeholder="<|start_header_id|>assistant<|end_header_id|>\n\n" />
          </label>
          <label class="fld">
            <span class="lb">普通后缀 (Suffix)：</span>
            <input v-model="form.output_suffix" class="in-txt" placeholder="<|eot_id|>" />
          </label>
          <label class="fld">
            <span class="lb">末条接续前缀 (Last Output)：</span>
            <input v-model="form.last_output_sequence" class="in-txt" placeholder="续写/诱导接续标记" />
          </label>
        </div>
      </div>

      <!-- Stop Sequence -->
      <div class="fld mt-3">
        <span class="lb">自定义停用标记 (Stop Sequences，每行一个)：</span>
        <textarea
          v-model="form.stop_sequence"
          class="in-area"
          rows="2"
          placeholder="<|eot_id|>"
        />
      </div>

      <!-- 实时预览 -->
      <div class="preview-wrap">
        <div class="prev-head">
          <span class="prev-title">实时拼装预览 (Live Prompt Preview)</span>
          <span class="prev-hint">依据当前规则合成的 4 轮上下文样本</span>
        </div>
        <pre class="prev-code"><code>{{ livePreview.text }}</code></pre>
        <div v-if="livePreview.stops.length" class="stop-tags">
          <span class="stop-tag-title">生效停用词：</span>
          <span v-for="(st, si) in livePreview.stops" :key="si" class="stop-tag">{{ JSON.stringify(st) }}</span>
        </div>
      </div>

      <!-- 底部保存 -->
      <div class="foot-bar">
        <button class="btn-save" :disabled="saving" @click="onSave">
          {{ saving ? '保存中…' : '保存 Instruct 配置' }}
        </button>
        <span v-if="savedTip" class="tip-ok">✓ 已持久化保存至 settings.json</span>
      </div>
    </div>
  </div>
</template>

<style scoped>
.instruct-card {
  background: var(--c-surface);
  border: 1px solid var(--c-border);
  border-radius: var(--radius-lg);
  padding: 16px;
  display: flex;
  flex-direction: column;
  gap: 16px;
}
.card-header {
  display: flex;
  justify-content: space-between;
  align-items: flex-start;
  gap: 16px;
  border-bottom: 1px solid var(--c-border);
  padding-bottom: 12px;
}
.card-title {
  font-size: 14px;
  font-weight: 700;
  color: var(--c-text);
  margin: 0 0 4px;
}
.card-desc {
  font-size: 12px;
  color: var(--c-text-2);
  margin: 0;
}
.switch-lbl {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 13px;
  font-weight: 600;
  color: var(--p-600);
  cursor: pointer;
  background: var(--p-50);
  padding: 6px 12px;
  border-radius: var(--radius-md);
  border: 1px solid var(--p-200);
}
.row-presets {
  display: flex;
  flex-direction: column;
  gap: 14px;
  background: var(--c-bg);
  padding: 12px;
  border-radius: var(--radius-md);
}
.preset-top-bar {
  display: flex;
  justify-content: space-between;
  align-items: flex-end;
  gap: 12px;
  flex-wrap: wrap;
  width: 100%;
}
.grow {
  flex-grow: 1;
}
.btn-group {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-wrap: wrap;
}
.btn-file {
  position: relative;
  overflow: hidden;
  cursor: pointer;
}
.file-hidden {
  position: absolute;
  top: 0;
  left: 0;
  opacity: 0;
  width: 100%;
  height: 100%;
  cursor: pointer;
}
.btn-danger {
  color: #ef4444;
}
.fld {
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.lb {
  font-size: 11px;
  font-weight: 600;
  color: var(--c-text-2);
}
.in-sel, .in-txt {
  height: 32px;
  padding: 0 10px;
  font-size: 12px;
  border: 1px solid var(--c-border);
  border-radius: var(--radius-md);
  background: var(--c-surface);
  color: var(--c-text);
}
.in-sel {
  min-width: 180px;
}
.in-area {
  padding: 8px 10px;
  font-size: 12px;
  font-family: monospace;
  border: 1px solid var(--c-border);
  border-radius: var(--radius-md);
  background: var(--c-surface);
  color: var(--c-text);
  resize: vertical;
}
.fld-chk {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 12px;
  color: var(--c-text);
  cursor: pointer;
  margin-top: 14px;
}
.seq-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(260px, 1fr));
  gap: 12px;
  margin-top: 8px;
}
.grid-col {
  display: flex;
  flex-direction: column;
  gap: 8px;
  background: var(--c-bg);
  padding: 12px;
  border-radius: var(--radius-md);
  border: 1px solid var(--c-border);
}
.col-title {
  font-size: 12px;
  font-weight: 700;
  color: var(--c-text);
  margin: 0 0 4px;
}
.mt-3 {
  margin-top: 10px;
}
.preview-wrap {
  margin-top: 12px;
  background: #1e1e24;
  color: #e2e8f0;
  border-radius: var(--radius-md);
  padding: 12px;
}
.prev-head {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 8px;
}
.prev-title {
  font-size: 12px;
  font-weight: 700;
  color: #94a3b8;
}
.prev-hint {
  font-size: 11px;
  color: #64748b;
}
.prev-code {
  margin: 0;
  padding: 8px;
  background: #141417;
  border-radius: 4px;
  font-family: 'Fira Code', Consolas, monospace;
  font-size: 12px;
  line-height: 1.5;
  white-space: pre-wrap;
  word-break: break-all;
  max-height: 240px;
  overflow-y: auto;
}
.stop-tags {
  margin-top: 8px;
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 6px;
  font-size: 11px;
}
.stop-tag-title {
  color: #94a3b8;
}
.stop-tag {
  background: #334155;
  color: #f1f5f9;
  padding: 2px 6px;
  border-radius: 4px;
  font-family: monospace;
}
.foot-bar {
  display: flex;
  align-items: center;
  gap: 12px;
  margin-top: 12px;
}
.btn-save {
  padding: 8px 20px;
  font-size: 13px;
  font-weight: 600;
  background: var(--p-500);
  color: var(--c-on-primary);
  border-radius: var(--radius-md);
  border: none;
  cursor: pointer;
  transition: background 0.15s ease;
}
.btn-save:hover:not(:disabled) {
  background: var(--p-600);
}
.tip-ok {
  font-size: 12px;
  color: var(--s-success, #16a34a);
  font-weight: 600;
}
</style>
