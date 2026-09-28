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
  PM_IDENTIFIERS,
  DEFAULT_ORDER,
} from '@/services/st/promptmanager'

const mainText = ref('')
const nsfwText = ref('')
const jbText = ref('')
const order = ref<{ identifier: string; enabled: boolean }[]>([])
const loaded = ref(false)
const saving = ref(false)
const msg = ref('')
const msgOk = ref(false)

onMounted(async () => {
  const pm = await loadPm(true)
  mainText.value = pm.main
  nsfwText.value = pm.nsfw
  jbText.value = pm.jailbreak
  order.value = [...pm.order.map((x) => ({ ...x }))]
  loaded.value = pm.loaded
})

const items = computed(() =>
  order.value.map((o) => ({
    ...o,
    label: PM_IDENTIFIERS[o.identifier] ?? o.identifier,
    builtin: o.identifier in PM_IDENTIFIERS,
  })),
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

async function save(): Promise<void> {
  saving.value = true
  msg.value = ''
  try {
    await savePm({ main: mainText.value, nsfw: nsfwText.value, jailbreak: jbText.value }, order.value)
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
    <h4>提示词管理（Prompt Manager）</h4>
    <p class="hint">
      数据存 settings.json 的 prompts / prompt_order；
      未加载到 ST 数据{{ loaded ? '' : '（当前为默认值，连接 ST 后保存即写入）' }}。
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

    <div class="sub-h">
      <span>条目顺序与启停（chatHistory 之后 = post-history 注入）</span>
      <span class="grow" />
      <button class="btn btn-sm" @click="resetOrder">重置默认顺序</button>
    </div>
    <div class="order">
      <div v-for="(it, i) in items" :key="it.identifier" class="order-row" :class="{ off: !it.enabled }">
        <input type="checkbox" v-model="it.enabled" />
        <span class="ol">{{ it.label }}</span>
        <code v-if="!it.builtin" class="unk" title="App 无对应块，保存后保留但跳过">{{ it.identifier }}</code>
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
