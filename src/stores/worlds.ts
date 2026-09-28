/**
 * 世界书（World Info / Lorebook）状态
 *
 * 数据全部走 ST /api/worldinfo/*；「新建」= edit 一个空 entries（ST 无 create 端点，
 * 与 ST 前端 createWorldInfo 行为一致）。
 * 条目编辑对齐 ST world-info.js 的 newWorldInfoEntryDefinition（4082-4125 行）：
 * 全量字段、新建走同一模板默认值；保存 = 整本写回 /api/worldinfo/edit（与 ST _save 相同），
 * 防抖 800ms 自动保存，成功后清注入缓存（worldinfo.ts）。
 */
import { defineStore, acceptHMRUpdate } from 'pinia'
import {
  createWorld,
  deleteWorld,
  getWorld,
  importWorldFile,
  listWorlds,
  saveWorld,
} from '@/services/st/data'
import { invalidateWiCache } from '@/services/st/worldinfo'
import type { StWorldBook, StWorldEntry, StWorldListItem } from '@/services/st/types'

/* ST 常量（world-info.js:33/96/855） */
export const WI_LOGIC = { AND_ANY: 0, NOT_ALL: 1, NOT_ANY: 2, AND_ALL: 3 } as const
export const WI_POSITION = {
  before: 0,
  after: 1,
  ANTop: 2,
  ANBottom: 3,
  atDepth: 4,
  EMTop: 5,
  EMBottom: 6,
  outlet: 7,
} as const
export const WI_ROLE = { system: 0, user: 1, assistant: 2 } as const

const DEFAULT_DEPTH = 4
const DEFAULT_WEIGHT = 100

/** 新条目模板（对齐 ST newWorldInfoEntryTemplate，world-info.js:4082-4125） */
export function newWorldInfoEntry(uid: number, displayIndex: number): StWorldEntry {
  return {
    uid,
    key: [],
    keysecondary: [],
    comment: '',
    content: '',
    constant: false,
    vectorized: false,
    selective: true,
    selectiveLogic: WI_LOGIC.AND_ANY,
    addMemo: false,
    order: DEFAULT_WEIGHT,
    position: WI_POSITION.before,
    disable: false,
    ignoreBudget: false,
    excludeRecursion: false,
    preventRecursion: false,
    matchPersonaDescription: false,
    matchCharacterDescription: false,
    matchCharacterPersonality: false,
    matchCharacterDepthPrompt: false,
    matchScenario: false,
    matchCreatorNotes: false,
    delayUntilRecursion: 0,
    probability: 100,
    useProbability: true,
    depth: DEFAULT_DEPTH,
    outletName: '',
    group: '',
    groupOverride: false,
    groupWeight: DEFAULT_WEIGHT,
    scanDepth: null,
    caseSensitive: null,
    matchWholeWords: null,
    useGroupScoring: null,
    automationId: '',
    role: WI_ROLE.system,
    sticky: null,
    cooldown: null,
    delay: null,
    triggers: [],
    displayIndex,
  }
}

/** 世界书条目的展示排序（displayIndex 缺省排最后） */
function sortedEntries(detail: StWorldBook): { uid: string; data: StWorldEntry }[] {
  return Object.entries(detail.entries ?? {})
    .map(([uid, data]) => ({ uid, data: data as StWorldEntry }))
    .sort((a, b) => (a.data.displayIndex ?? 1e9) - (b.data.displayIndex ?? 1e9))
}

const AUTOSAVE_MS = 800

export const useWorldsStore = defineStore('worlds', {
  state: () => ({
    list: [] as StWorldListItem[],
    loading: false,
    loaded: false,
    error: '',
    /** 当前选中的世界书（file_id） */
    currentId: '' as string,
    /** 当前世界书详情（/api/worldinfo/get） */
    detail: null as StWorldBook | null,
    detailLoading: false,
    mutating: false,
    /** 有未保存的条目改动（防抖保存期间为真） */
    dirty: false,
    saving: false,
    /** 防抖保存定时器（非响应式语义，放 state 仅为实现便利） */
    saveTimer: undefined as number | undefined,
  }),

  getters: {
    current(state): StWorldListItem | null {
      return state.list.find((w) => w.file_id === state.currentId) ?? null
    },
    /** 条目（按 displayIndex 排序展示） */
    entries(state): { uid: string; data: StWorldEntry }[] {
      return state.detail ? sortedEntries(state.detail) : []
    },
  },

  actions: {
    async load(force = false) {
      if (this.loading) return
      if (this.loaded && !force) return
      this.loading = true
      this.error = ''
      try {
        this.list = await listWorlds()
        this.loaded = true
        if (!this.currentId && this.list.length) {
          this.select(this.list[0]!.file_id)
        }
      } catch (e) {
        this.error = e instanceof Error ? e.message : String(e)
      } finally {
        this.loading = false
      }
    },

    select(fileId: string) {
      if (this.currentId === fileId && this.detail) return
      // 切书前先落盘旧书的未保存改动（P1：防抖定时器跨书竞态会丢改动甚至张冠李戴）
      void this.flushPendingSave()
      this.currentId = fileId
      void this.loadDetail(fileId)
    },

    async loadDetail(fileId?: string) {
      const target = fileId ?? this.currentId
      if (!target) return
      // 直接重载当前书（刷新按钮等路径）同样先落盘，避免未保存改动被 getWorld 覆盖
      await this.flushPendingSave()
      this.detailLoading = true
      this.dirty = false
      try {
        this.detail = await getWorld(target)
      } catch (e) {
        this.detail = null
        this.error = e instanceof Error ? e.message : String(e)
      } finally {
        this.detailLoading = false
      }
    },

    /**
     * 立即落盘未保存改动（清掉防抖定时器）。切书/重载前必须调用 ——
     * 否则 800ms 窗口内的编辑会随 detail 替换而丢失，或定时器触发时
     * 把旧书内容写进新书文件。dirty 为假时是空操作。
     */
    async flushPendingSave(): Promise<void> {
      if (this.saveTimer !== undefined) {
        clearTimeout(this.saveTimer)
        this.saveTimer = undefined
      }
      if (!this.dirty) return
      await this.save()
    },

    /* ---------------- 条目编辑（本地改动 → 防抖保存） ---------------- */

    /** 就地修改条目字段（未知字段经 [k:string]:unknown 透传保留） */
    patchEntry(uid: string | number, patch: Partial<StWorldEntry>) {
      const e = this.detail?.entries?.[String(uid)]
      if (!e) return
      Object.assign(e as StWorldEntry, patch)
      this.markDirty()
    },

    /** 新增条目（uid/displayIndex 取当前最大 +1，默认值对齐 ST 模板） */
    addEntry(): StWorldEntry | null {
      if (!this.detail?.entries) return null
      const entries = this.detail.entries
      const uids = Object.keys(entries).map(Number).filter(Number.isFinite)
      const nextUid = uids.length ? Math.max(...uids) + 1 : 0
      const nextDisp = sortedEntries(this.detail).length
      const entry = newWorldInfoEntry(nextUid, nextDisp)
      entries[String(nextUid)] = entry
      this.markDirty()
      return entry
    },

    /** 删除条目 */
    removeEntry(uid: string | number) {
      if (!this.detail?.entries) return
      delete this.detail.entries[String(uid)]
      this.markDirty()
    },

    /** 上移/下移（交换相邻条目的 displayIndex） */
    moveEntry(uid: string | number, dir: -1 | 1) {
      if (!this.detail) return
      const list = sortedEntries(this.detail)
      const i = list.findIndex((x) => x.uid === String(uid))
      const j = i + dir
      if (i < 0 || j < 0 || j >= list.length) return
      const a = list[i]!.data
      const b = list[j]!.data
      const da = a.displayIndex ?? i
      const db = b.displayIndex ?? j
      a.displayIndex = db
      b.displayIndex = da
      this.markDirty()
    },

    markDirty() {
      this.dirty = true
      this.scheduleSave()
    },

    /** 防抖自动保存（对齐 ST 的 saveWorldDebounced） */
    scheduleSave() {
      if (this.saveTimer !== undefined) clearTimeout(this.saveTimer)
      this.saveTimer = window.setTimeout(() => {
        this.saveTimer = undefined
        void this.save()
      }, AUTOSAVE_MS)
    },

    /** 整本写回 ST（保存发起时刻的快照与书 id；成功后清注入缓存） */
    async save(): Promise<void> {
      // P1：在函数入口捕获 currentId + detail 快照 —— 保存期间用户切书时，
      // 定时器触发的 save 不能把旧书内容写进新书（也不能被新 detail 污染）
      const fileId = this.currentId
      const snapshot = this.detail
        ? (JSON.parse(JSON.stringify(this.detail)) as StWorldBook)
        : null
      if (!snapshot || !fileId) return
      this.saving = true
      try {
        await saveWorld(fileId, snapshot)
        invalidateWiCache()
        this.dirty = false
      } catch (e) {
        this.error = e instanceof Error ? e.message : String(e)
      } finally {
        this.saving = false
      }
    },

    /* ---------------- 书级操作 ---------------- */

    /** 新建空世界书；成功后刷新并选中 */
    async create(name: string): Promise<string> {
      this.mutating = true
      this.error = ''
      try {
        const id = await createWorld(name.trim())
        invalidateWiCache()
        await this.load(true)
        this.currentId = ''
        this.select(id)
        return id
      } catch (e) {
        this.error = e instanceof Error ? e.message : String(e)
        throw e
      } finally {
        this.mutating = false
      }
    },

    /** 导入世界书 JSON 文件；成功后刷新并选中 */
    async importFile(file: File): Promise<string> {
      this.mutating = true
      this.error = ''
      try {
        const name = await importWorldFile(file)
        invalidateWiCache()
        await this.load(true)
        this.currentId = ''
        this.select(name)
        return name
      } catch (e) {
        this.error = e instanceof Error ? e.message : String(e)
        throw e
      } finally {
        this.mutating = false
      }
    },

    /** 删除世界书（两步确认由 UI 负责） */
    async remove(name: string): Promise<void> {
      this.mutating = true
      this.error = ''
      try {
        await deleteWorld(name)
        invalidateWiCache()
        if (this.currentId === name) {
          this.currentId = ''
          this.detail = null
        }
        await this.load(true)
      } catch (e) {
        this.error = e instanceof Error ? e.message : String(e)
        throw e
      } finally {
        this.mutating = false
      }
    },
  },
})

/* 开发期热更新接管：改本 store 后无需手动刷新页面（Pinia 官方推荐写法） */
if (import.meta.hot) {
  import.meta.hot.accept(acceptHMRUpdate(useWorldsStore, import.meta.hot))
}
