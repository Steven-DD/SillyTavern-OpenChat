/**
 * WebDAV 云端增量同步服务
 */
import { invoke } from '@tauri-apps/api/core'
import { isTauri } from '@/services/tauri/bridge'

export interface SyncConfig {
  url: string
  username: string
  password?: string
  auto_sync: boolean
}

export interface SyncResult {
  success: boolean
  pushed: number
  pulled: number
  merged: number
  errors: string[]
  message: string
}

const SYNC_STORAGE_KEY = 'app.sync_webdav'

export const DEFAULT_SYNC_CONFIG: SyncConfig = {
  url: '',
  username: '',
  password: '',
  auto_sync: false,
}

export function loadSyncConfig(): SyncConfig {
  try {
    const raw = localStorage.getItem(SYNC_STORAGE_KEY)
    if (!raw) return { ...DEFAULT_SYNC_CONFIG }
    return { ...DEFAULT_SYNC_CONFIG, ...JSON.parse(raw) }
  } catch {
    return { ...DEFAULT_SYNC_CONFIG }
  }
}

export function saveSyncConfig(cfg: SyncConfig): void {
  localStorage.setItem(SYNC_STORAGE_KEY, JSON.stringify(cfg))
}

/** 测试 WebDAV 连接状态 */
export async function testSyncConnection(cfg: SyncConfig): Promise<string> {
  if (!isTauri()) {
    return '网页预览模式：WebDAV 同步需要桌面端环境运行'
  }
  return invoke<string>('st_sync_test', { cfg })
}

/** 执行 WebDAV 增量推送同步 */
export async function executeSync(cfg: SyncConfig): Promise<SyncResult> {
  if (!isTauri()) {
    return {
      success: false,
      pushed: 0,
      pulled: 0,
      merged: 0,
      errors: ['非桌面端环境不可用'],
      message: '网页预览模式无法读写本地同步目录',
    }
  }
  return invoke<SyncResult>('st_sync_execute', { cfg })
}
