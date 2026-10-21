<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import {
  BUILTIN_CONTEXT_TEMPLATES,
  listContextTemplates,
  loadActiveContextTemplate,
  saveActiveContextTemplate,
  loadContextTemplate,
  saveNamedContextTemplate,
  deleteContextTemplate,
  importContextTemplateFile,
  exportContextTemplateFile,
  type ContextTemplateData,
} from '@/services/st/contextTemplates'
import Handlebars from 'handlebars'

const loading = ref(true)
const saving = ref(false)
const savedTip = ref(false)
const templateList = ref<string[]>([])
const selectedTemplateName = ref('Default (标准酒馆排布)')

const form = ref<ContextTemplateData>({
  name: 'Default',
  story_string: '',
  chat_start: '',
  example_separator: '',
  chat_separator: '',
  story_string_position: 0,
  story_string_depth: 1,
  story_string_role: 0,
})

const MACRO_TAGS = [
  { tag: '{{system}}', desc: '系统提示词' },
  { tag: '{{description}}', desc: '角色描述' },
  { tag: '{{personality}}', desc: '角色性格' },
  { tag: '{{scenario}}', desc: '对话场景' },
  { tag: '{{persona}}', desc: '用户人设' },
  { tag: '{{wiBefore}}', desc: '角色前世界书' },
  { tag: '{{examples}}', desc: '对话示例' },
  { tag: '{{chat}}', desc: '聊天历史' },
  { tag: '{{wiAfter}}', desc: '角色后世界书' },
]

async function refreshList() {
  templateList.value = await listContextTemplates()
}

onMounted(async () => {
  try {
    await refreshList()
    const active = await loadActiveContextTemplate()
    form.value = { ...active }
    selectedTemplateName.value = active.name
  } catch (e) {
    console.warn('加载上下文模板失败：', e)
  } finally {
    loading.value = false
  }
})

async function onTemplateChange() {
  const t = await loadContextTemplate(selectedTemplateName.value)
  if (t) {
    form.value = { ...t }
  }
}

function insertMacro(tag: string) {
  const current = form.value.story_string || ''
  const wrap = `{{#if ${tag.replace(/[{}]/g, '')}}}${tag}\\n{{/if}}`
  form.value.story_string = current ? `${current}\n${wrap}` : wrap
}

async function onSaveActive() {
  saving.value = true
  try {
    await saveActiveContextTemplate(form.value)
    // 同步另存到命名预设
    if (form.value.name && form.value.name !== 'Custom') {
      await saveNamedContextTemplate(form.value.name, form.value.name === form.value.name ? form.value : { ...form.value })
    }
    await refreshList()
    savedTip.value = true
    setTimeout(() => (savedTip.value = false), 2000)
  } catch (e) {
    alert(`保存失败：${e instanceof Error ? e.message : String(e)}`)
  } finally {
    saving.value = false
  }
}

async function onSaveAs() {
  const name = prompt('请输入新上下文模板名称：', form.value.name || 'My_Context_Template')
  if (!name || !name.trim()) return
  const cleanName = name.trim()
  form.value.name = cleanName
  selectedTemplateName.value = cleanName
  await saveNamedContextTemplate(cleanName, form.value)
  await saveActiveContextTemplate(form.value)
  await refreshList()
  savedTip.value = true
  setTimeout(() => (savedTip.value = false), 2000)
}

async function onDeleteCurrent() {
  if (BUILTIN_CONTEXT_TEMPLATES.some((b) => b.name === selectedTemplateName.value)) {
    alert('内置模板不可删除')
    return
  }
  if (!confirm(`确定删除上下文模板「${selectedTemplateName.value}」吗？`)) return
  await deleteContextTemplate(selectedTemplateName.value)
  await refreshList()
  selectedTemplateName.value = BUILTIN_CONTEXT_TEMPLATES[0]!.name
  await onTemplateChange()
}

async function onImportFile(e: Event) {
  const target = e.target as HTMLInputElement
  const file = target.files?.[0]
  if (!file) return
  try {
    const imported = await importContextTemplateFile(file)
    await refreshList()
    selectedTemplateName.value = imported.name
    form.value = { ...imported }
    await saveActiveContextTemplate(imported)
    alert(`成功导入上下文模板「${imported.name}」`)
  } catch (err) {
    alert(`导入失败：${err instanceof Error ? err.message : String(err)}`)
  } finally {
    target.value = ''
  }
}

function onExportFile() {
  exportContextTemplateFile(form.value)
}

// 实时预览组装效果
const livePreview = computed(() => {
  try {
    const tpl = Handlebars.compile(form.value.story_string || '', { noEscape: true })
    const sample = {
      system: '[系统指令：以第三人称扮演派蒙]',
      description: '派蒙是提瓦特最好的应急向导。',
      personality: '活泼、贪吃、喜欢给别人起外号。',
      scenario: '在蒙德城外的湖边偶遇旅行者。',
      persona: '旅行者：来自异世界的金发冒险家。',
      wiBefore: '【世界书：提瓦特大陆通用常识】',
      examples: '旅行者: 你好啊！\n派蒙: 喂！别把派蒙当应急食品！',
      chat: '旅行者: 今天吃什么？\n派蒙: 当然是香喷喷的蜜酱胡萝卜煎肉！',
      wiAfter: '【世界书：派蒙的食谱】',
    }
    return tpl(sample).trim()
  } catch (e) {
    return `模板编译出错：${e instanceof Error ? e.message : String(e)}`
  }
})
</script>

<template>
  <div class="context-card">
    <div class="card-header">
      <div class="title-wrap">
        <h3 class="card-title">上下文故事模板 (Context Template & Story String)</h3>
        <p class="card-desc">
          使用 Handlebars 宏自定义系统提示、角色描述、人设、世界书与会话历史的排布次序，完全兼容 ST 官方 context/*.json。
        </p>
      </div>
      <div class="head-actions">
        <span v-if="savedTip" class="save-tip">已保存并生效！</span>
        <button class="btn btn-primary" :disabled="saving" @click="onSaveActive">
          {{ saving ? '保存中…' : '保存并生效' }}
        </button>
      </div>
    </div>

    <div v-if="loading" class="loading-box">加载配置中…</div>

    <div v-else class="card-body">
      <!-- 预设操作栏 -->
      <div class="toolbar-row">
        <div class="preset-sel-wrap">
          <span class="lb">选择模板预设：</span>
          <select v-model="selectedTemplateName" class="in-sel" @change="onTemplateChange">
            <option v-for="t in templateList" :key="t" :value="t">{{ t }}</option>
          </select>
        </div>

        <div class="btn-group">
          <button class="btn btn-sm" @click="onSaveAs">另存为新预设</button>
          <button class="btn btn-sm" @click="onExportFile">导出 (.json)</button>
          <label class="btn btn-sm btn-file">
            导入 (.json)
            <input type="file" accept=".json" class="file-hidden" @change="onImportFile" />
          </label>
          <button
            class="btn btn-sm btn-danger"
            :disabled="BUILTIN_CONTEXT_TEMPLATES.some((b) => b.name === selectedTemplateName)"
            @click="onDeleteCurrent"
          >
            删除
          </button>
        </div>
      </div>

      <!-- 宏快捷插入标签 -->
      <div class="macro-bar">
        <span class="bar-lbl">点击插入插值宏：</span>
        <div class="tags-wrap">
          <button
            v-for="m in MACRO_TAGS"
            :key="m.tag"
            class="tag-btn"
            :title="m.desc"
            @click="insertMacro(m.tag)"
          >
            {{ m.tag }}
            <span class="tag-desc">({{ m.desc }})</span>
          </button>
        </div>
      </div>

      <!-- Story String 编辑框 -->
      <div class="editor-area">
        <label class="fld">
          <span class="lb">Story String 模板结构（Handlebars 语法）：</span>
          <textarea
            v-model="form.story_string"
            class="in-txt font-mono"
            rows="8"
            spellcheck="false"
            placeholder="{{#if system}}{{system}}\n{{/if}}{{#if description}}{{description}}\n{{/if}}..."
          ></textarea>
        </label>
      </div>

      <!-- 附加分隔符 -->
      <div class="grid3">
        <label class="fld">
          <span class="lb">对话起始提示 (Chat Start)：</span>
          <input v-model="form.chat_start" class="in-txt" placeholder="[Start a new chat]" />
        </label>
        <label class="fld">
          <span class="lb">示例分隔符 (Example Separator)：</span>
          <input v-model="form.example_separator" class="in-txt" placeholder="<START>" />
        </label>
        <label class="fld">
          <span class="lb">历史分隔符 (Chat Separator)：</span>
          <input v-model="form.chat_separator" class="in-txt" placeholder="留空默认" />
        </label>
      </div>

      <!-- 实时预览 -->
      <div class="preview-box">
        <h4 class="preview-title">排布实时预览（样本数据渲染）</h4>
        <pre class="preview-code">{{ livePreview }}</pre>
      </div>
    </div>
  </div>
</template>

<style scoped>
.context-card {
  padding: 1.5rem;
  background: var(--c-surface, #ffffff);
  border-radius: var(--radius-lg, 12px);
  border: 1px solid var(--c-border, #e5e7eb);
  box-shadow: 0 1px 3px rgba(0, 0, 0, 0.05);
  margin-top: 1.5rem;
}

.card-header {
  display: flex;
  justify-content: space-between;
  align-items: flex-start;
  gap: 1rem;
  padding-bottom: 1.25rem;
  border-bottom: 1px solid var(--c-border, #e5e7eb);
}

.card-title {
  margin: 0;
  font-size: 1.15rem;
  font-weight: 600;
  color: var(--c-text-1, #111827);
}

.card-desc {
  margin: 0.25rem 0 0;
  font-size: 0.875rem;
  color: var(--c-text-3, #6b7280);
}

.head-actions {
  display: flex;
  align-items: center;
  gap: 0.75rem;
}

.save-tip {
  font-size: 0.85rem;
  color: #10b981;
  font-weight: 500;
}

.toolbar-row {
  display: flex;
  justify-content: space-between;
  align-items: center;
  flex-wrap: wrap;
  gap: 1rem;
  margin: 1.25rem 0;
  padding: 0.75rem 1rem;
  background: var(--c-panel, #f9fafb);
  border-radius: var(--radius-md, 8px);
}

.preset-sel-wrap {
  display: flex;
  align-items: center;
  gap: 0.5rem;
}

.in-sel,
.in-txt {
  padding: 0.45rem 0.65rem;
  border-radius: var(--radius-md, 6px);
  border: 1px solid var(--c-border, #d1d5db);
  background: var(--c-surface, #fff);
  color: var(--c-text-1, #111827);
  font-size: 0.9rem;
}

.in-txt:focus,
.in-sel:focus {
  outline: none;
  border-color: var(--p-500, #3b82f6);
}

.btn-group {
  display: flex;
  align-items: center;
  gap: 0.5rem;
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

.macro-bar {
  margin-bottom: 1rem;
}

.bar-lbl {
  display: block;
  font-size: 0.8rem;
  font-weight: 500;
  color: var(--c-text-2, #4b5563);
  margin-bottom: 0.4rem;
}

.tags-wrap {
  display: flex;
  flex-wrap: wrap;
  gap: 0.4rem;
}

.tag-btn {
  padding: 0.25rem 0.5rem;
  background: var(--c-panel, #f3f4f6);
  border: 1px solid var(--c-border, #e5e7eb);
  border-radius: 4px;
  font-size: 0.75rem;
  font-family: monospace;
  color: var(--p-600, #2563eb);
  cursor: pointer;
  transition: all 0.15s ease;
}

.tag-btn:hover {
  background: var(--p-100, #eff6ff);
  border-color: var(--p-400, #60a5fa);
}

.tag-desc {
  font-size: 0.7rem;
  color: var(--c-text-3, #9ca3af);
  margin-left: 2px;
}

.editor-area {
  margin-bottom: 1.25rem;
}

.font-mono {
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
  line-height: 1.5;
}

.grid3 {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(220px, 1fr));
  gap: 1rem;
  margin-bottom: 1.25rem;
}

.fld {
  display: flex;
  flex-direction: column;
  gap: 0.35rem;
}

.lb {
  font-size: 0.85rem;
  font-weight: 500;
  color: var(--c-text-2, #374151);
}

.preview-box {
  margin-top: 1.25rem;
  padding: 1rem;
  background: var(--c-panel, #f8fafc);
  border-radius: var(--radius-md, 8px);
  border: 1px dashed var(--c-border, #cbd5e1);
}

.preview-title {
  margin: 0 0 0.5rem;
  font-size: 0.85rem;
  font-weight: 600;
  color: var(--c-text-2, #475569);
}

.preview-code {
  margin: 0;
  padding: 0.75rem;
  background: var(--c-surface, #ffffff);
  border-radius: 6px;
  border: 1px solid var(--c-border, #e2e8f0);
  font-size: 0.8rem;
  font-family: ui-monospace, SFMono-Regular, Consolas, monospace;
  color: var(--c-text-1, #334155);
  max-height: 220px;
  overflow-y: auto;
  white-space: pre-wrap;
  word-break: break-all;
}
</style>
