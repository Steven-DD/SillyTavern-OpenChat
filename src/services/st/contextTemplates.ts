/**
 * 上下文模板服务（Context Templates Service）
 *
 * 覆盖：
 * 1. Story String Handlebars 结构定义
 * 2. ST context/*.json 文件读取、保存、删除、导入与导出
 * 3. 内置标准上下文模板（Default / Character First / Minimal）
 */
import { getPreset, listPresets, savePreset, deletePreset } from './presets'
import { getSettings } from './data'
import { mutateSettings } from './settings-writequeue'

export interface ContextTemplateData {
  name: string
  story_string: string
  chat_start?: string
  example_separator?: string
  chat_separator?: string
  story_string_position?: number
  story_string_depth?: number
  story_string_role?: number
  [k: string]: unknown
}

export const BUILTIN_CONTEXT_TEMPLATES: ContextTemplateData[] = [
  {
    name: 'Default (标准酒馆排布)',
    story_string:
      '{{#if system}}{{system}}\n{{/if}}' +
      '{{#if description}}{{description}}\n{{/if}}' +
      '{{#if personality}}{{personality}}\n{{/if}}' +
      '{{#if scenario}}{{scenario}}\n{{/if}}' +
      '{{#if persona}}{{persona}}\n{{/if}}' +
      '{{#if wiBefore}}{{wiBefore}}\n{{/if}}' +
      '{{#if examples}}{{examples}}\n{{/if}}' +
      '{{#if chat}}{{chat}}\n{{/if}}' +
      '{{#if wiAfter}}{{wiAfter}}\n{{/if}}',
    chat_start: '',
    example_separator: '',
    chat_separator: '',
    story_string_position: 0,
    story_string_depth: 1,
    story_string_role: 0,
  },
  {
    name: 'Character First (人设置顶)',
    story_string:
      '{{#if description}}{{description}}\n{{/if}}' +
      '{{#if personality}}{{personality}}\n{{/if}}' +
      '{{#if persona}}{{persona}}\n{{/if}}' +
      '{{#if system}}{{system}}\n{{/if}}' +
      '{{#if scenario}}{{scenario}}\n{{/if}}' +
      '{{#if wiBefore}}{{wiBefore}}\n{{/if}}' +
      '{{#if examples}}{{examples}}\n{{/if}}' +
      '{{#if chat}}{{chat}}\n{{/if}}' +
      '{{#if wiAfter}}{{wiAfter}}\n{{/if}}',
    chat_start: '',
    example_separator: '',
    chat_separator: '',
    story_string_position: 0,
    story_string_depth: 1,
    story_string_role: 0,
  },
  {
    name: 'Minimal (极简剧情)',
    story_string:
      '{{#if system}}{{system}}\n{{/if}}' +
      '{{#if description}}{{description}}\n{{/if}}' +
      '{{#if chat}}{{chat}}\n{{/if}}',
    chat_start: '',
    example_separator: '',
    chat_separator: '',
    story_string_position: 0,
    story_string_depth: 1,
    story_string_role: 0,
  },
]

/** 读取已保存的上下文模板列表（包含内置与用户自定义） */
export async function listContextTemplates(): Promise<string[]> {
  const userPresets = await listPresets('context')
  const builtinNames = BUILTIN_CONTEXT_TEMPLATES.map((t) => t.name)
  const set = new Set([...builtinNames, ...userPresets])
  return Array.from(set)
}

/** 读取当前生效的 Context Template 设置（从 settings.json power_user.context） */
export async function loadActiveContextTemplate(): Promise<ContextTemplateData> {
  const settings = await getSettings()
  const pu = ((settings.power_user ?? {}) as Record<string, unknown>).context as Record<string, unknown> | undefined
  if (pu && typeof pu.story_string === 'string' && pu.story_string.trim()) {
    return {
      name: (pu.name as string) || 'Custom',
      story_string: pu.story_string,
      chat_start: (pu.chat_start as string) || '',
      example_separator: (pu.example_separator as string) || '',
      chat_separator: (pu.chat_separator as string) || '',
      story_string_position: Number(pu.story_string_position) || 0,
      story_string_depth: Number(pu.story_string_depth) || 1,
      story_string_role: Number(pu.story_string_role) || 0,
    }
  }
  return BUILTIN_CONTEXT_TEMPLATES[0]!
}

/** 保存当前 Context Template 为全局生效配置（写回 settings.json power_user.context） */
export async function saveActiveContextTemplate(template: ContextTemplateData): Promise<void> {
  await mutateSettings((s) => {
    const pu = { ...(((s.power_user ?? {}) as Record<string, unknown>) || {}) }
    pu.context = {
      name: template.name,
      story_string: template.story_string,
      chat_start: template.chat_start || '',
      example_separator: template.example_separator || '',
      chat_separator: template.chat_separator || '',
      story_string_position: template.story_string_position ?? 0,
      story_string_depth: template.story_string_depth ?? 1,
      story_string_role: template.story_string_role ?? 0,
    }
    return {
      next: {
        ...s,
        power_user: pu,
      },
    }
  })
}

/** 读取具体命名模板 */
export async function loadContextTemplate(name: string): Promise<ContextTemplateData | null> {
  // 先找内置
  const builtin = BUILTIN_CONTEXT_TEMPLATES.find((t) => t.name === name)
  if (builtin) return { ...builtin }

  // 找文件
  const raw = await getPreset(name, 'context')
  if (!raw) return null

  return {
    name,
    story_string: typeof raw.story_string === 'string' ? raw.story_string : '',
    chat_start: typeof raw.chat_start === 'string' ? raw.chat_start : '',
    example_separator: typeof raw.example_separator === 'string' ? raw.example_separator : '',
    chat_separator: typeof raw.chat_separator === 'string' ? raw.chat_separator : '',
    story_string_position: Number(raw.story_string_position) || 0,
    story_string_depth: Number(raw.story_string_depth) || 1,
    story_string_role: Number(raw.story_string_role) || 0,
  }
}

/** 持久化命名模板到 default-user/context/ */
export async function saveNamedContextTemplate(name: string, data: ContextTemplateData): Promise<void> {
  await savePreset(name, data as unknown as Record<string, unknown>, 'context')
}

/** 删除命名模板 */
export async function deleteContextTemplate(name: string): Promise<void> {
  await deletePreset(name, 'context')
}

/** 导入 .json 文件 */
export async function importContextTemplateFile(file: File): Promise<ContextTemplateData> {
  const text = await file.text()
  const raw = JSON.parse(text) as Record<string, unknown>
  const name = file.name.replace(/\.json$/i, '').trim() || 'Imported_Context'
  const data: ContextTemplateData = {
    name,
    story_string: typeof raw.story_string === 'string' ? raw.story_string : '',
    chat_start: typeof raw.chat_start === 'string' ? raw.chat_start : '',
    example_separator: typeof raw.example_separator === 'string' ? raw.example_separator : '',
    chat_separator: typeof raw.chat_separator === 'string' ? raw.chat_separator : '',
    story_string_position: Number(raw.story_string_position) || 0,
    story_string_depth: Number(raw.story_string_depth) || 1,
    story_string_role: Number(raw.story_string_role) || 0,
  }
  await saveNamedContextTemplate(name, data)
  return data
}

/** 导出 .json 文件 */
export function exportContextTemplateFile(template: ContextTemplateData): void {
  const str = JSON.stringify(template, null, 4)
  const blob = new Blob([str], { type: 'application/json' })
  const url = URL.createObjectURL(blob)
  const a = document.createElement('a')
  a.href = url
  a.download = `${template.name.trim() || 'context-template'}.json`
  document.body.appendChild(a)
  a.click()
  document.body.removeChild(a)
  URL.revokeObjectURL(url)
}
