/**
 * 数据巡检：
 * - /report 生成 {report, token}；report 分组 9 组（sanitizeReport:151-162）
 *   每项为脱敏记录 {name, hash(sha256(路径)), parent?, size?, mtime?}（#sanitizeRecord:135-144）
 * - /view?token=&hash= 逐项查看文件内容（GET，浏览器直接打开即可）
 * - /delete {token, hashes} 按 hash 选择性删除
 * - /finalize {token} 全量清理（token 一次性，用后失效）
 */
import { stPostJson, stPostVoid } from './client'

/** 脱敏后的文件记录 */
export interface MaidRecord {
  name: string
  hash: string
  parent?: string
  size?: number
  mtime?: number
}

export interface MaidReport {
  images: MaidRecord[]
  files: MaidRecord[]
  chats: MaidRecord[]
  groupChats: MaidRecord[]
  avatarThumbnails: MaidRecord[]
  backgroundThumbnails: MaidRecord[]
  personaThumbnails: MaidRecord[]
  chatBackups: MaidRecord[]
  settingsBackups: MaidRecord[]
  [k: string]: MaidRecord[]
}

export interface MaidRun {
  report: MaidReport
  token: string
}

export async function runMaidReport(): Promise<MaidRun> {
  const r = await stPostJson<MaidRun & { report?: MaidReport }>('/api/data-maid/report', {})
  if (!r?.report || !r?.token) throw new Error('巡检报告生成失败')
  return r
}

/** 按 token 清理全部（服务端 204，token 一次性，用后失效） */
export async function finalizeMaid(token: string): Promise<void> {
  await stPostVoid('/api/data-maid/finalize', { token })
}

/** 按 hash 选择性删除 */
export async function deleteMaidFiles(token: string, hashes: string[]): Promise<void> {
  if (!hashes.length) return
  await stPostVoid('/api/data-maid/delete', { token, hashes })
}

/** 逐项查看文件的浏览器 URL（GET，cookie 鉴权，可直接 window.open） */
export function maidViewPath(token: string, hash: string): string {
  return `/api/data-maid/view?token=${encodeURIComponent(token)}&hash=${encodeURIComponent(hash)}`
}
