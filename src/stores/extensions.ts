/**
 * 扩展（插件）状态：中栏列表 + 右栏详情 + 安装/搜索/启停/更新/删除
 */
import { defineStore, acceptHMRUpdate } from 'pinia'
import {
  discoverExtensions,
  installExtension,
  updateExtension,
  deleteExtension,
  getExtensionVersion,
  getExtensionBranches,
  switchExtensionBranch,
  moveExtension,
  getDisabledExtensions,
  setExtensionDisabled,
  getDisabledExtensions as fetchDisabled,
  type DiscoveredExtension,
  type ExtVersionInfo,
  type ExtUpdateResult,
  type ExtInstallResult,
  type ExtBranch,
} from '@/services/st/extensions'

export const useExtensionsStore = defineStore('extensions', {
  state: () => ({
    list: [] as DiscoveredExtension[],
    loading: false,
    error: '',
    /** 搜索关键字（按名称过滤） */
    search: '',
    /** 右栏当前选中的扩展名 */
    selectedName: '',
    /** 停用名单（extension_settings.disabledExtensions） */
    disabledNames: [] as string[],
    busy: false,
    busyText: '',
  }),

  getters: {
    filtered(state): DiscoveredExtension[] {
      const kw = state.search.trim().toLowerCase()
      if (!kw) return state.list
      return state.list.filter((x) => x.name.toLowerCase().includes(kw))
    },
    selected(state): DiscoveredExtension | null {
      return state.list.find((x) => x.name === state.selectedName) ?? null
    },
    isDisabled(state): (name: string) => boolean {
      return (name) => state.disabledNames.includes(name)
    },
  },

  actions: {
    async load(_force = false) {
      if (this.loading) return
      this.loading = true
      this.error = ''
      try {
        this.list = await discoverExtensions()
        this.disabledNames = await getDisabledExtensions()
        // 列表变化时保持选中项；选中项消失则清空
        if (this.selectedName && !this.list.some((x) => x.name === this.selectedName)) {
          this.selectedName = ''
        }
      } catch (e) {
        this.error = e instanceof Error ? e.message : String(e)
      } finally {
        this.loading = false
      }
    },

    select(name: string) {
      this.selectedName = this.selectedName === name ? '' : name
    },

    /** 从 Git URL 安装（下载安装由 ST 服务端执行）；返回清单信息 */
    async install(url: string, global = false): Promise<ExtInstallResult> {
      this.busy = true
      this.busyText = '正在下载安装扩展…'
      this.error = ''
      try {
        const info = await installExtension(url, global)
        await this.load(true)
        if (info.folderName) this.selectedName = info.folderName
        return info
      } catch (e) {
        this.error = e instanceof Error ? e.message : String(e)
        throw e
      } finally {
        this.busy = false
        this.busyText = ''
      }
    },

    async update(name: string, global = false): Promise<ExtUpdateResult> {
      this.busy = true
      this.busyText = '正在检查更新…'
      try {
        return await updateExtension(name, global)
      } finally {
        this.busy = false
        this.busyText = ''
      }
    },

    async remove(name: string, global = false): Promise<void> {
      this.busy = true
      this.busyText = '正在删除…'
      try {
        await deleteExtension(name, global)
        if (this.selectedName === name) this.selectedName = ''
        await this.load(true)
      } finally {
        this.busy = false
        this.busyText = ''
      }
    },

    async version(name: string, global = false): Promise<ExtVersionInfo> {
      return getExtensionVersion(name, global)
    },

    async branches(name: string, global = false): Promise<ExtBranch[]> {
      return getExtensionBranches(name, global)
    },

    async switchBranch(name: string, global: boolean, branch: string): Promise<void> {
      this.busy = true
      this.busyText = '正在切换分支…'
      try {
        await switchExtensionBranch(name, global, branch)
        await this.load(true)
      } finally {
        this.busy = false
        this.busyText = ''
      }
    },

    async move(name: string, global: boolean): Promise<void> {
      this.busy = true
      this.busyText = global ? '正在移到全局目录…' : '正在移到用户目录…'
      try {
        await moveExtension(name, global)
        await this.load(true)
      } finally {
        this.busy = false
        this.busyText = ''
      }
    },

    async toggleDisabled(name: string): Promise<void> {
      this.disabledNames = await setExtensionDisabled(name, !this.disabledNames.includes(name))
    },

    async refreshDisabled(): Promise<void> {
      this.disabledNames = await fetchDisabled()
    },
  },
})

if (import.meta.hot) {
  import.meta.hot.accept(acceptHMRUpdate(useExtensionsStore, import.meta.hot))
}
