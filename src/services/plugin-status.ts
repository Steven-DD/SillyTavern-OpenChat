/**
 * 插件状态：文案与色点的**唯一来源**
 *
 * 以前 PluginsMid 与 PluginsMain 各维护一份（STATUS / STATUS_LABEL），
 * 内容重复且容易改歪一处。这里统一导出，两边都从这里取。
 */
export interface PluginStatusStyle {
  /** 色点 class：ok 绿 / bad 红 / warn 橙 / off 灰 */
  dot: string
  text: string
}

export const PLUGIN_STATUS: Record<string, PluginStatusStyle> = {
  ready: { dot: 'ok', text: '已就绪' },
  missing: { dot: 'bad', text: '未就绪' },
  mismatch: { dot: 'warn', text: '不满足要求' },
  disabled: { dot: 'off', text: '已停用' },
}

/** 未知状态：灰点 + 原样显示状态码 */
export function pluginStatusOf(status: string): PluginStatusStyle {
  return PLUGIN_STATUS[status] ?? { dot: 'off', text: status }
}

/** 只要文案（列表、表格等不需要色点的场景） */
export function pluginStatusText(status: string, fallback = '—'): string {
  return PLUGIN_STATUS[status]?.text ?? (status || fallback)
}
