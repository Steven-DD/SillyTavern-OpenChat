<script setup lang="ts">
/**
 * 提示词管理卡片
 * - main / nsfw / jailbreak 三槽编辑（settings.json prompts）
 * - prompt_order 顺序/启停（全局序）
 */
import { computed, onMounted, ref } from 'vue'
import {
  loadPm,
  savePm,
  exportPmPreset,
  importPmPreset,
  PM_IDENTIFIERS,
  DEFAULT_ORDER,
  type PmEntry,
} from '@/services/st/promptmanager'

const mainText = ref('')
const nsfwText = ref('')
const jbText = ref('')
const order = ref<{ identifier: string; enabled: boolean }[]>([])
const customBlocks = ref<PmEntry[]>([])
const rawPrompts = ref<PmEntry[]>([])
const loaded = ref(false)
const saving = ref(false)
const msg = ref('')
const msgOk = ref(false)

async function reload() {
  const pm = await loadPm(true)
  mainText.value = pm.main
  nsfwText.value = pm.nsfw
  jbText.value = pm.jailbreak
  order.value = [...pm.order.map((x) => ({ ...x }))]
  rawPrompts.value = pm.rawPrompts
  customBlocks.value = pm.rawPrompts.filter(
    (p) => !['main', 'nsfw', 'jailbreak'].includes(p.identifier),
  )
  loaded.value = pm.loaded
}

onMounted(async () => {
  await reload()
})

const items = computed(() =>
  order.value.map((o) => {
    const custom = customBlocks.value.find((c) => c.identifier === o.identifier)
    return {
      ...o,
      label: custom?.name || (PM_IDENTIFIERS[o.identifier] ?? o.identifier),
      builtin: o.identifier in PM_IDENTIFIERS,
      isCustom: !!custom,
    }
  }),
)

function move(i: number, dir: -1 | 1): void {
  const j = i + dir
  if (j < 0 || j >= order.value.length) return
  const next = [...order.value]
  ;[next[i], next[j]] = [next[j]!, next[i]!]
  order.value = next
}

function resetOrder(): void {
  order.value = DEFAULT_ORDER.map((x) => ({ ...x }))
  msg.value = '已重置为 ST 默认顺序（尚未保存）'
  msgOk.value = true
}

function addCustomBlock(): void {
  const name = prompt('请输入新提示词块名称（例如：输出格式约束）：')
  if (!name || !name.trim()) return
  const id = `custom_${Date.now()}`
  const newBlock: PmEntry = {
    identifier: id,
    name: name.trim(),
    role: 'system',
    system_prompt: true,
    content: '',
  }
  customBlocks.value.push(newBlock)
  order.value.push({ identifier: id, enabled: true })
}

function removeCustomBlock(id: string): void {
  customBlocks.value = customBlocks.value.filter((b) => b.identifier !== id)
  order.value = order.value.filter((o) => o.identifier !== id)
}

function onExport(): void {
  const name = prompt('请输入导出的提示词预设方案名称：', 'My_Prompt_Preset')
  if (!name || !name.trim()) return
  exportPmPreset(name.trim(), {
    main: mainText.value,
    nsfw: nsfwText.value,
    jailbreak: jbText.value,
    order: order.value,
    rawPrompts: [
      ...customBlocks.value,
      { identifier: 'main', name: 'main', system_prompt: true, role: 'system', content: mainText.value },
      { identifier: 'nsfw', name: 'nsfw', system_prompt: true, role: 'system', content: nsfwText.value },
      { identifier: 'jailbreak', name: 'jailbreak', system_prompt: true, role: 'system', content: jbText.value },
    ],
    loaded: true,
  })
}

async function onImport(e: Event): Promise<void> {
  const target = e.target as HTMLInputElement
  const file = target.files?.[0]
  if (!file) return
  try {
    const data = await importPmPreset(file)
    mainText.value = data.main
    nsfwText.value = data.nsfw
    jbText.value = data.jailbreak
    order.value = [...data.order]
    customBlocks.value = data.rawPrompts.filter(
      (p) => !['main', 'nsfw', 'jailbreak'].includes(p.identifier),
    )
    msgOk.value = true
    msg.value = `已成功导入提示词预设「${file.name}」并保存生效`
  } catch (err) {
    msgOk.value = false
    msg.value = `导入失败：${err instanceof Error ? err.message : String(err)}`
  } finally {
    target.value = ''
  }
}

async function save(): Promise<void> {
  saving.value = true
  msg.value = ''
  try {
    await savePm(
      { main: mainText.value, nsfw: nsfwText.value, jailbreak: jbText.value },
      order.value,
      customBlocks.value,
    )
    msgOk.value = true
    msg.value = '已保存（生成时即时生效）'
  } catch (e) {
    msgOk.value = false
    msg.value = e instanceof Error ? e.message : String(e)
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <section class="card pm">
    <div class="head-row">
      <h4>提示词管理（Prompt Manager）</h4>
      <div class="btn-group">
        <button class="btn btn-sm" @click="onExport">导出预设 (.json)</button>
        <label class="btn btn-sm btn-file">
          导入预设 (.json)
          <input type="file" accept=".json" class="file-hidden" @change="onImport" />
        </label>
      </div>
    </div>
    <p class="hint">
      数据存 settings.json 的 prompts / prompt_order；
      未加载到数据{{ loaded ? '' : '（当前为默认值，连接后端后保存即写入）' }}。
    </p>

    <label class="fld">
      <span class="lb">主提示词（main）</span>
      <textarea v-model="mainText" class="field field-area" rows="4"></textarea>
    </label>
    <label class="fld">
      <span class="lb">NSFW</span>
      <textarea v-model="nsfwText" class="field field-area" rows="3"></textarea>
    </label>
    <label class="fld">
      <span class="lb">越狱（jailbreak / post-history）</span>
      <textarea v-model="jbText" class="field field-area" rows="3"></textarea>
    </label>

    <!-- 自定义提示词块展示与编辑 -->
    <div v-for="cb in customBlocks" :key="cb.identifier" class="custom-block-card">
      <div class="custom-block-head">
        <span class="lb">{{ cb.name || cb.identifier }} (自定义块)</span>
        <button class="btn btn-sm btn-danger" @click="removeCustomBlock(cb.identifier)">删除块</button>
      </div>
      <textarea v-model="cb.content" class="field field-area" rows="3" placeholder="填写该自定义提示词块内容…"></textarea>
    </div>

    <div class="sub-h">
      <span>条目顺序与启停（chatHistory 之后 = post-history 注入）</span>
      <span class="grow" />
      <button class="btn btn-sm" @click="addCustomBlock">+ 添加自定义块</button>
      <button class="btn btn-sm" @click="resetOrder">重置默认顺序</button>
    </div>
    <div class="order">
      <div v-for="(it, i) in items" :key="it.identifier" class="order-row" :class="{ off: !it.enabled }">
        <input type="checkbox" v-model="it.enabled" />
        <span class="ol">{{ it.label }}</span>
        <code v-if="!it.builtin && !it.isCustom" class="unk" title="App 无对应块，保存后保留但跳过">{{ it.identifier }}</code>
        <code v-else-if="it.isCustom" class="custom-tag">自定义</code>
        <span class="grow" />
        <button class="mv" :disabled="i === 0" @click="move(i, -1)">↑</button>
        <button class="mv" :disabled="i === order.length - 1" @click="move(i, 1)">↓</button>
      </div>
    </div>

    <div class="btns">
      <button class="btn btn-primary" :disabled="saving" @click="save">
        {{ saving ? '保存中…' : '保存到 ST' }}
      </button>
    </div>
    <p v-if="msg" class="rsp" :class="msgOk ? 'okish' : 'err'">{{ msg }}</p>
  </section>
</template>

<style scoped>
.pm {
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.head-row {
  display: flex;
  justify-content: space-between;
  align-items: center;
  flex-wrap: wrap;
  gap: 8px;
}
.head-row h4 {
  margin: 0;
}
.btn-group {
  display: flex;
  align-items: center;
  gap: 6px;
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
.custom-block-card {
  padding: 8px 10px;
  background: var(--c-panel);
  border: 1px solid var(--c-border);
  border-radius: var(--radius-md);
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.custom-block-head {
  display: flex;
  justify-content: space-between;
  align-items: center;
}
.custom-tag {
  font-size: 10px;
  color: var(--p-600);
  background: var(--p-100);
  padding: 2px 4px;
  border-radius: 3px;
}
.btn-danger {
  color: #ef4444;
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
.order {
  border: 1px solid var(--c-border);
  border-radius: var(--radius-md);
  overflow: hidden;
}
.order-row {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 10px;
  font-size: 12px;
  border-top: 1px solid var(--c-border);
}
.order-row:first-child {
  border-top: none;
}
.order-row.off {
  opacity: 0.55;
}
.ol {
  color: var(--c-text);
}
.unk {
  font-size: 10px;
  color: var(--s-warning);
}
.mv {
  width: 24px;
  height: 22px;
  display: grid;
  place-items: center;
  border: 1px solid var(--c-border);
  border-radius: var(--radius-sm);
  color: var(--c-text-2);
}
.mv:hover:not(:disabled) {
  border-color: var(--p-400);
  color: var(--p-600);
}
.mv:disabled {
  opacity: 0.35;
}
</style>
