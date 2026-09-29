/**
 * 时间格式化（会话列表 / 消息气泡共用）
 * 今天 → HH:MM，昨天 → 昨天，本周 → 周X，更早 → M/D
 */
const WEEK = ['日', '一', '二', '三', '四', '五', '六']

function startOfDay(d: Date): number {
  return new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime()
}

/** 会话列表用的短时间 */
export function shortTime(ms?: number): string {
  if (!ms) return ''
  const d = new Date(ms)
  if (Number.isNaN(d.getTime())) return ''
  const now = new Date()
  const dayDiff = Math.round((startOfDay(now) - startOfDay(d)) / 86400000)

  if (dayDiff <= 0) {
    return `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`
  }
  if (dayDiff === 1) return '昨天'
  if (dayDiff < 7) return `周${WEEK[d.getDay()]}`
  return `${d.getMonth() + 1}/${d.getDate()}`
}

/** 消息气泡上方的时间戳（更细，含年月） */
export function bubbleTime(ms?: number): string {
  if (!ms) return ''
  const d = new Date(ms)
  if (Number.isNaN(d.getTime())) return ''
  const now = new Date()
  const sameYear = d.getFullYear() === now.getFullYear()
  const hm = `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`
  const dayDiff = Math.round((startOfDay(now) - startOfDay(d)) / 86400000)
  if (dayDiff <= 0) return hm
  if (dayDiff === 1) return `昨天 ${hm}`
  if (sameYear) return `${d.getMonth() + 1}/${d.getDate()} ${hm}`
  return `${d.getFullYear()}/${d.getMonth() + 1}/${d.getDate()} ${hm}`
}

/** 分组分隔用的日期标题 */
export function dayLabel(ms?: number): string {
  if (!ms) return '未知时间'
  const d = new Date(ms)
  const now = new Date()
  const dayDiff = Math.round((startOfDay(now) - startOfDay(d)) / 86400000)
  if (dayDiff <= 0) return '今天'
  if (dayDiff === 1) return '昨天'
  if (dayDiff < 7) return `周${WEEK[d.getDay()]}`
  return `${d.getFullYear()}年${d.getMonth() + 1}月${d.getDate()}日`
}
