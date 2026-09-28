<script setup lang="ts">
/**
 * 用户（右栏）：人设详情与编辑
 *
 * 第一版：头像（上传/更换）、名称、描述、注入位置（+深度/角色）、设为默认、删除、
 *        {{user}} 用户名。
 * 第二版预留：描述关联世界书 / 人设绑定角色 / 会话锁定 —— 数据字段已备好，
 *        这里只以「即将支持」占位呈现（见 persona.ts 的类型注释）。
 */
import { computed, nextTick, ref, watch } from 'vue'
import { usePersonaStore } from '@/stores/persona'
import { PERSONA_POSITIONS, PERSONA_ROLES, personaAvatarUrl } from '@/services/st/persona'
import ConfirmDialog from '@/components/ConfirmDialog.vue'

/**
 * ⚠ 不能在模板里写 `{{ '{{user}}' }}` —— Vue 的 mustache 解析器遇到插值内部的 `{{`
 * 会直接报 "Unterminated string constant"（整页白屏）。字面量放 script 里再插值出去。
 */
const USER_MACRO = '{{user}}'

const p = usePersonaStore()
const cur = computed(() => p.selected)

const fileInput = ref<HTMLInputElement | null>(null)
const newName = ref('')
const newFile = ref<File | null>(null)
const creating = ref(false)
const createCard = ref<HTMLElement | null>(null)

/** 中栏点「新增用户」→ 滚到新建区，省得用户自己找 */
watch(
  () => p.createMode,
  (on) => {
    if (!on) return
    creating.value = true
    void nextTick(() => createCard.value?.scrollIntoView({ behavior: 'smooth', block: 'center' }))
  },
)

function onPick(e: Event): void {
  const el = e.target as HTMLInputElement
  newFile.value = el.files?.[0] ?? null
}

async function doCreate(): Promise<void> {
  if (!newFile.value) return
  await p.create(newFile.value, newName.value)
  newFile.value = null
  newName.value = ''
  creating.value = false
  if (fileInput.value) fileInput.value.value = ''
}

async function onChangeAvatar(e: Event): Promise<void> {
  const el = e.target as HTMLInputElement
  const f = el.files?.[0]
  if (f) await p.changeAvatar(f)
  el.value = ''
}

/** 删除人设（弹窗确认，头像文件一并删除） */
const confirmDel = ref(false)
const delBusy = ref(false)

async function doRemove(): Promise<void> {
  const c = cur.value
  if (!c) return
  delBusy.value = true
  try {
    await p.remove(c.id)
    confirmDel.value = false
  } finally {
    delBusy.value = false
  }
}
</script>

<template>
  <main class="main page" data-page="personas">
    <header class="head">
      <h3>{{ cur ? cur.name : '用户' }}</h3>
      <span class="crumb">{{ p.personas.length }} 个人设</span>
      <span v-if="p.message" class="crumb ok">{{ p.message }}</span>
    </header>

    <div class="body">
      <p v-if="p.error" class="rsp err">{{ p.error }}</p>

      <!-- ===== 用户名（{{user}} 宏） ===== -->
      <section class="card">
        <h4>用户名</h4>
        <p class="hint">角色卡里 <code>{{ USER_MACRO }}</code> 宏取的就是这个值。</p>
        <div class="row">
          <input v-model="p.userName" class="field" placeholder="例如：Alice" />
          <button class="btn" :disabled="p.saving" @click="p.saveAll()">保存</button>
        </div>
      </section>

      <!-- ===== 人设详情 ===== -->
      <template v-if="cur">
        <section class="card">
          <h4>头像</h4>
          <div class="avatar-row">
            <img class="face" :src="personaAvatarUrl(cur.id)" :alt="cur.name" />
            <div class="ops">
              <input
                ref="fileInput"
                class="file"
                type="file"
                accept="image/*"
                @change="onChangeAvatar"
              />
              <p class="hint">上传新图会替换当前头像，描述与设置保留。</p>
            </div>
          </div>
        </section>

        <section class="card">
          <h4>人设</h4>
          <label class="fld">
            <span class="lb">名称</span>
            <input
              v-model="cur.name"
              class="field"
              placeholder="人设名"
              @change="p.saveAll()"
              @input="p.patchSelected({ name: cur.name })"
            />
          </label>
          <label class="fld">
            <span class="lb">描述</span>
            <textarea
              v-model="cur.description"
              class="field field-area"
              rows="6"
              placeholder="这个世界观下你是谁：性格、身份、说话方式…"
              @change="p.saveAll()"
            ></textarea>
          </label>
        </section>

        <section class="card">
          <h4>描述注入位置</h4>
          <select v-model.number="cur.position" class="field field-select" @change="p.saveAll()">
            <option v-for="o in PERSONA_POSITIONS" :key="o.v" :value="o.v">{{ o.label }}</option>
          </select>
          <div v-if="cur.position === 4" class="grid2">
            <label class="fld">
              <span class="lb">插入深度</span>
              <input v-model.number="p.depth" class="field" type="number" min="0" step="1" @change="p.saveAll()" />
            </label>
            <label class="fld">
              <span class="lb">插入身份</span>
              <select v-model.number="p.role" class="field field-select" @change="p.saveAll()">
                <option v-for="r in PERSONA_ROLES" :key="r.v" :value="r.v">{{ r.label }}</option>
              </select>
            </label>
          </div>
        </section>

        <section class="card">
          <div class="btns">
            <button
              class="btn"
              :disabled="p.saving || p.defaultId === cur.id"
              @click="p.usePersona(cur.id)"
            >
              {{ p.defaultId === cur.id ? '使用中' : '使用' }}
            </button>
            <button class="btn danger" :disabled="p.saving" @click="confirmDel = true">删除人设</button>
          </div>
        </section>
      </template>

      <!-- ===== 新建 ===== -->
      <section ref="createCard" class="card">
        <h4>新建人设</h4>
        <p class="hint">先选一张头像图，人设就以该头像为标识。</p>
        <div class="row">
          <input class="field" type="file" accept="image/*" @change="onPick" />
        </div>
        <div class="row">
          <input v-model="newName" class="field" placeholder="人设名（留空则用文件名）" />
          <button class="btn" :disabled="p.saving || !newFile" @click="doCreate">
            {{ p.saving ? '创建中…' : '创建' }}
          </button>
        </div>
      </section>

      <!-- ===== 第二版预留（数据字段已备好，UI 待开放） ===== -->
      <section class="card future">
        <h4>即将支持</h4>
        <ul class="future-list">
          <li>描述关联世界书（persona.lorebook）</li>
          <li>人设绑定角色：切换角色自动带出对应人设（persona.characters）</li>
          <li>会话锁定人设（persona.lockedChat）</li>
          <li>从角色卡一键转换成人设</li>
        </ul>
      </section>
    </div>

    <!-- 删除人设确认 -->
    <ConfirmDialog
      :open="confirmDel"
      title="删除人设"
      :message="cur ? `删除人设「${cur.name}」？\n其头像文件会一并删除，不可恢复。` : ''"
      confirm-text="删除"
      :busy="delBusy"
      @confirm="doRemove"
      @cancel="confirmDel = false"
    />
  </main>
</template>

<style scoped>
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
.crumb.ok {
  color: var(--s-success);
}
.body {
  flex: 1;
  overflow-y: auto;
  padding: 14px 20px 30px;
  display: flex;
  flex-direction: column;
  gap: 12px;
}
/* ⚠ flex:none 防止 flex 纵向滚动容器把超高卡片压缩裁切（内容无法滚动） */
.card {
  flex: none;
}
.card h4 {
  font-size: 13px;
  font-weight: 600;
  margin-bottom: 9px;
}
.card.future {
  opacity: 0.75;
}
/* .hint 外观走全局 shortcut；只保留本页的布局性底距 */
.hint {
  margin-bottom: 8px;
}
code {
  padding: 0 3px;
  font-family: var(--font-mono);
  font-size: 11px;
  border-radius: 4px;
  background: var(--c-panel);
  color: var(--c-text-2);
}
.row {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 8px;
}
.lb {
  display: block;
  font-size: 11px;
  font-weight: 600;
  color: var(--c-text-2);
  margin-bottom: 4px;
}
.in:focus {
  border-color: var(--p-400);
}
.grid2 {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 10px;
  margin-top: 10px;
}
.file {
  font-size: 12px;
}
.avatar-row {
  display: flex;
  align-items: center;
  gap: 14px;
}
.face {
  width: 64px;
  height: 64px;
  flex: none;
  object-fit: cover;
  border-radius: 50%;
  background: var(--c-panel);
}
.ops {
  flex: 1;
  min-width: 0;
}
.btns {
  display: flex;
  flex-wrap: wrap;
  gap: 7px;
}
/* .btn 基础外观走全局 shortcut；「危险按钮」= 中性按钮的红色悬停（本页特例） */
.btn.danger:hover:not(:disabled) {
  border-color: var(--s-error);
  color: var(--s-error);
}
.rsp {
  padding: 7px 10px;
  font-size: 11px;
  border-radius: var(--radius-md);
  background: var(--c-panel);
  color: var(--c-text-2);
}
.rsp.err {
  background: var(--s-error-bg);
  color: var(--s-error);
}
.future-list {
  margin: 0;
  padding-left: 18px;
  font-size: 11px;
  line-height: 1.9;
  color: var(--c-text-3);
}
</style>
