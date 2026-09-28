/**
 * 群组数据层（对齐 ST groups.js / group-chats.js；数据全落 ST 目录，与网页端互通）
 *
 * - 群对象：groups/<id>.json（name/members/activation_strategy/generation_mode/...）
 * - 群聊：group_chats/<chat_id>.jsonl（首行 header user_name/character_name = 'unused'，
 *   消息行带 name = 发言成员名）
 */
import { stPostJson } from './client'

/** 群对象（ST Group，全字段透传） */
export interface StGroup {
  id: string
  name: string
  /** 成员 avatar 文件名列表 */
  members: string[]
  avatar_url?: string
  allow_self_responses?: boolean
  hideMutedSprites?: boolean
  /** 激活策略（ST 1.19）：0 NATURAL 掷骰 / 1 LIST 全员顺序 / 2 MANUAL 非输入时随机 1 人 / 3 POOLED 未发言者随机 1 人 */
  activation_strategy: number
  /** 生成模式（只影响成员卡片合并，不影响发言人数）：0 SWAP / 1 APPEND / 2 APPEND_DISABLED */
  generation_mode: number
  /** 手动静音的成员 avatar 列表 */
  disabled_members: string[]
  fav?: boolean
  chat_id: string
  chats: string[]
  auto_mode_delay?: number
  [k: string]: unknown
}

/** 群聊行（消息行带 name = 发言者名；首行 header 同普通会话） */
export type GroupChatLine = Record<string, unknown>

export async function listGroups(): Promise<StGroup[]> {
  const data = await stPostJson<StGroup[] | unknown>('/api/groups/all')
  return Array.isArray(data) ? (data as StGroup[]) : []
}

export async function createGroup(
  model: Omit<StGroup, 'id'> & { id?: string },
): Promise<void> {
  await stPostJson('/api/groups/create', model)
}

export async function editGroup(patch: Partial<StGroup> & { id: string }): Promise<void> {
  await stPostJson('/api/groups/edit', patch)
}

export async function deleteGroup(id: string, deleteChats = false): Promise<void> {
  await stPostJson('/api/groups/delete', { id, delete_chats: deleteChats })
}

/** 读群聊（jsonl 行数组，首行 header） */
export async function getGroupChat(id: string): Promise<GroupChatLine[]> {
  const data = await stPostJson<GroupChatLine[] | unknown>('/api/chats/group/get', { id })
  return Array.isArray(data) ? (data as GroupChatLine[]) : []
}

/** 写群聊（整文件：header + 消息行） */
export async function saveGroupChat(id: string, chat: GroupChatLine[]): Promise<void> {
  await stPostJson('/api/chats/group/save', { id, chat })
}

export async function deleteGroupChat(id: string, chatName: string): Promise<void> {
  await stPostJson('/api/chats/group/delete', { id, chat_name: chatName })
}

/**
 * 群头像上传（对齐 ST uploadGroupAvatar，group-chats.js:1896-1935）：
 * 图片缩至 300×300 jpg → /api/images/upload 落盘 → 返回路径写回 group.avatar_url。
 * 调用方负责随后 editGroup 持久化。
 */
export async function uploadGroupAvatarDataUrl(id: string, dataUrl: string): Promise<string> {
  const thumb = await resizeImageDataUrl(dataUrl, 300, 300)
  const base64 = thumb.replace(/^data:image\/[a-z]+;base64,/, '')
  const filename = `${id}_${Date.now()}`
  const r = await stPostJson<{ path?: string; error?: unknown }>('/api/images/upload', {
    image: base64,
    format: 'jpg',
    ch_name: String(id),
    filename,
  })
  if (!r?.path) throw new Error('头像上传失败')
  await editGroup({ id, avatar_url: r.path })
  return r.path
}

/** 图片缩放为 300×300 jpg data URL（ST createThumbnail 的 canvas 等价实现） */
function resizeImageDataUrl(dataUrl: string, width: number, height: number): Promise<string> {
  return new Promise((resolve, reject) => {
    const img = new Image()
    img.onload = () => {
      // 按比例裁剪至目标尺寸（正方形缩略图）
      const side = Math.min(img.width, img.height)
      const sx = (img.width - side) / 2
      const sy = (img.height - side) / 2
      const canvas = document.createElement('canvas')
      canvas.width = width
      canvas.height = height
      const ctx = canvas.getContext('2d')
      if (!ctx) {
        reject(new Error('canvas 不可用'))
        return
      }
      ctx.drawImage(img, sx, sy, side, side, 0, 0, width, height)
      resolve(canvas.toDataURL('image/jpeg', 0.9))
    }
    img.onerror = () => reject(new Error('图片解析失败'))
    img.src = dataUrl
  })
}

/** File → data URL（群头像上传入口用） */
export function fileToDataUrl(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const r = new FileReader()
    r.onload = () => resolve(String(r.result))
    r.onerror = () => reject(new Error('读取图片失败'))
    r.readAsDataURL(file)
  })
}

/** 群聊元数据（chat_metadata） */
export async function groupChatInfo(id: string): Promise<Record<string, unknown>> {
  const data = await stPostJson<{ chat_metadata?: Record<string, unknown> } | Record<string, unknown>>(
    '/api/chats/group/info',
    { id },
  )
  const meta = (data as { chat_metadata?: Record<string, unknown> })?.chat_metadata
  return meta && typeof meta === 'object' ? meta : {}
}
