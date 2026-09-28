/**
 * 角色卡状态（= 微信「联系人」）
 */
import { defineStore, acceptHMRUpdate } from 'pinia'
import {
  convertCharacterBook,
  createCharacter,
  deleteCharacter,
  editCharacter,
  getCharacter,
  importCharacterFile,
  listCharacters,
  saveWorld,
  setCharacterWorld,
} from '@/services/st/data'
import { invalidateWiCache } from '@/services/st/worldinfo'
import { useChatStore } from '@/stores/chat'
import type { StCharacter, StCharacterBook } from '@/services/st/types'

/** 未知值安全取字符串（卡片高级字段可能顶层或 data 内，类型不定） */
function str(v: unknown): string | undefined {
  return typeof v === 'string' ? v : undefined
}
/** 未知值安全取字符串数组 */
function arr(v: unknown): string[] | undefined {
  return Array.isArray(v) ? v.filter((x): x is string => typeof x === 'string') : undefined
}

/** 可编辑的卡片字段（对应 ST /api/characters/edit 的入参） */
export interface CharacterEditPatch {
  name?: string
  description?: string
  personality?: string
  scenario?: string
  first_mes?: string
  mes_example?: string
  creator_notes?: string
  tags?: string[]
  /** 高级字段：作者署名 */
  creator?: string
  /** 高级字段：卡片版本号 */
  characterVersion?: string
  /** 高级字段：备选开场白（每条一项） */
  alternateGreetings?: string[]
}

export const useCharacterStore = defineStore('character', {
  state: () => ({
    list: [] as StCharacter[],
    loading: false,
    loaded: false,
    error: '',
    /** 当前选中角色（按头像文件名标识） */
    currentAvatar: '' as string,
    /** 当前角色详情（/api/characters/get，字段更全） */
    detail: null as StCharacter | null,
    detailLoading: false,
    saving: false,
    /** 中栏右键菜单请求进入编辑态（详情页 watch 后消费并复位） */
    editRequested: false,
  }),

  getters: {
    current(state): StCharacter | null {
      if (!state.currentAvatar) return null
      return (
        state.list.find((c) => c.avatar === state.currentAvatar) ??
        (state.detail?.avatar === state.currentAvatar ? state.detail : null)
      )
    },
    /** 全部角色卡用到的标签去重（中栏筛选用） */
    allTags(state): string[] {
      const s = new Set<string>()
      for (const c of state.list) for (const t of c.tags ?? []) if (t) s.add(t)
      return [...s].sort()
    },
  },

  actions: {
    /** 拉取角色列表。ST 返回的 description 可能很大，只保留列表需要的内容 */
    async load(force = false) {
      if (this.loading) return
      if (this.loaded && !force) return
      this.loading = true
      this.error = ''
      try {
        this.list = await listCharacters()
        this.loaded = true
        // 首次加载自动选中第一个角色
        if (!this.currentAvatar && this.list.length) {
          this.currentAvatar = this.list[0].avatar
        }
      } catch (e) {
        this.error = e instanceof Error ? e.message : String(e)
      } finally {
        this.loading = false
      }
    },

    select(avatar: string) {
      this.currentAvatar = avatar
      void this.loadDetail(avatar)
    },

    /** 读取单卡详情（深层字段） */
    async loadDetail(avatar?: string) {
      const target = avatar ?? this.currentAvatar
      if (!target) return
      this.detailLoading = true
      try {
        this.detail = await getCharacter(target)
      } catch (e) {
        this.detail = this.list.find((c) => c.avatar === target) ?? null
        this.error = e instanceof Error ? e.message : String(e)
      } finally {
        this.detailLoading = false
      }
    },

    /**
     * 新建空白角色卡（默认头像 + 仅名称）。
     * 返回头像文件名；成功后刷新列表并选中。
     */
    async createCard(name: string): Promise<string> {
      this.saving = true
      try {
        const avatar = await createCharacter({ ch_name: name.trim() })
        await this.load(true)
        this.select(avatar)
        return avatar
      } finally {
        this.saving = false
      }
    },

    /**
     * 导入角色卡文件（json / png / charx，与 ST 侧一致）。
     * 若卡内嵌 character_book（v2 spec），自动转成 ST 世界书导入并绑定到该角色
     * （复刻 ST「导入内嵌世界书」行为，world-info.js:5731，只是免掉确认弹窗）。
     * 返回导入的内嵌世界书名（无内嵌书时为 null）。
     */
    async importCard(file: File): Promise<string | null> {
      this.saving = true
      try {
        const avatar = await importCharacterFile(file)
        await this.load(true)
        this.select(avatar)

        // 内嵌世界书：导入 → 绑定。失败不阻断角色卡导入（卡已可用，书可后补）。
        let importedBook: string | null = null
        try {
          const detail = await getCharacter(avatar)
          const book = detail.data?.character_book as StCharacterBook | undefined
          if (book && Array.isArray(book.entries) && book.entries.length) {
            const bookName = book.name || `${detail.name}'s Lorebook`
            await saveWorld(bookName, convertCharacterBook(book))
            await setCharacterWorld(avatar, bookName)
            invalidateWiCache()
            importedBook = bookName
          }
        } catch (e) {
          console.warn('[character] 内嵌世界书导入失败（角色卡已导入）:', e)
        }
        return importedBook
      } finally {
        this.saving = false
      }
    },

    /**
     * 删除角色卡（连带其全部会话记录）。
     * 成功后刷新列表并选中第一张剩余卡；删的是当前卡时先清选中态。
     */
    async deleteCard(avatar: string) {
      // 生成中删卡：SSE 会继续跑、saveCurrent 还会以内存消息重建会话文件（半截僵尸文件）
      const chat = useChatStore()
      if (chat.streaming) {
        // ST group.members 是 avatar 文件名数组
        const inGroup = chat.group?.members?.includes(avatar) ?? false
        if (chat.currentAvatar === avatar || inGroup) {
          chat.lastError = '该角色正在生成回复，请先停止生成再删除'
          return
        }
      }
      this.saving = true
      try {
        await deleteCharacter(avatar)
        invalidateWiCache()
        if (this.currentAvatar === avatar) {
          this.currentAvatar = ''
          this.detail = null
        }
        await this.load(true)
        if (!this.currentAvatar && this.list.length) {
          this.select(this.list[0].avatar)
        }
      } finally {
        this.saving = false
      }
    },

    /**
     * 保存卡片改动。
     * ST /api/characters/edit 需要完整字段（含 json_data 以保留 V2/V3 外键），
     * 所以这里以 detail（或列表项）为基底做合并。
     */
    async saveCard(patch: CharacterEditPatch) {
      const base = this.detail ?? this.current
      if (!base) throw new Error('未选择角色卡')
      this.saving = true
      try {
        await editCharacter({
          avatar_url: base.avatar,
          ch_name: patch.name ?? base.name,
          description: patch.description ?? base.description,
          personality: patch.personality ?? base.personality,
          scenario: patch.scenario ?? base.scenario,
          first_mes: patch.first_mes ?? base.first_mes,
          mes_example: patch.mes_example ?? base.mes_example,
          creator_notes: patch.creator_notes ?? String(base.creatorcomment ?? ''),
          tags: patch.tags ?? base.tags ?? [],
          talkativeness: base.talkativeness ?? 0.5,
          fav: String(base.fav ?? false),
          // 保留原会话指向与创建时间，避免 ST 侧被重置
          chat: base.chat,
          create_date: base.create_date,
          // 保留 V2/V3 卡里的其它外键字段
          json_data: base.json_data,
          // 高级字段（以详情/列表项为基底合并；v2 字段顶层或 data 内取）
          creator: patch.creator ?? str(base.creator) ?? str(base.data?.creator) ?? '',
          character_version:
            patch.characterVersion ??
            str(base.character_version) ??
            str((base.data as { character_version?: string } | undefined)?.character_version) ??
            '',
          alternate_greetings:
            patch.alternateGreetings ??
            arr(base.alternate_greetings) ??
            arr((base.data as { alternate_greetings?: string[] } | undefined)?.alternate_greetings) ??
            [],
          // ⚠ 必须回传世界书绑定：ST 的 edit 是全量重写，
          // 不传 world 会把 data.extensions.world 抹成空（实测确认）
          world: String((base.data?.extensions as { world?: string } | undefined)?.world ?? ''),
        })
        invalidateWiCache() // 换绑世界书后清注入缓存
        await this.load(true)
        await this.loadDetail(base.avatar)
      } finally {
        this.saving = false
      }
    },
  },
})

/* 开发期热更新接管：改本 store 后无需手动刷新页面（Pinia 官方推荐写法） */
if (import.meta.hot) {
  import.meta.hot.accept(acceptHMRUpdate(useCharacterStore, import.meta.hot))
}
