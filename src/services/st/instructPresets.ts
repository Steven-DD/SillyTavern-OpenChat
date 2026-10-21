/**
 * 常见开源与商业模型的标准 Instruct / Context 模板预设库
 * 包含 Llama 3, ChatML, Mistral, Gemma, Command-R, Alpaca, Vicuna 等主流格式。
 */
import type { InstructSettings } from './instruct'

export interface InstructPresetItem {
  id: string
  name: string
  description: string
  settings: InstructSettings
}

export const INSTRUCT_PRESETS: InstructPresetItem[] = [
  {
    id: 'default',
    name: '默认标准 (Default / Vanilla)',
    description: '标准提示词格式，无特殊标记，使用角色名字作为前后缀',
    settings: {
      enabled: false,
      preset: 'Default',
      wrap: true,
      names_behavior: 'none',
      system_sequence: '',
      system_suffix: '\n',
      input_sequence: '',
      input_suffix: '\n',
      output_sequence: '',
      output_suffix: '\n',
      system_same_as_user: false,
      stop_sequence: '',
    },
  },
  {
    id: 'llama3',
    name: 'Llama 3 / 3.1 / 3.2 / 3.3',
    description: 'Meta Llama 3 官方指令格式（<|start_header_id|>...<|end_header_id|>）',
    settings: {
      enabled: true,
      preset: 'Llama 3',
      wrap: false,
      names_behavior: 'none',
      system_sequence: '<|start_header_id|>system<|end_header_id|>\n\n',
      system_suffix: '<|eot_id|>',
      input_sequence: '<|start_header_id|>user<|end_header_id|>\n\n',
      input_suffix: '<|eot_id|>',
      output_sequence: '<|start_header_id|>assistant<|end_header_id|>\n\n',
      output_suffix: '<|eot_id|>',
      system_same_as_user: false,
      stop_sequence: '<|eot_id|>\n<|start_header_id|>',
    },
  },
  {
    id: 'chatml',
    name: 'ChatML (Qwen / Yi / DeepSeek)',
    description: '通用 ChatML 格式（<|im_start|>role\\n...<|im_end|>）',
    settings: {
      enabled: true,
      preset: 'ChatML',
      wrap: false,
      names_behavior: 'none',
      system_sequence: '<|im_start|>system\n',
      system_suffix: '<|im_end|>\n',
      input_sequence: '<|im_start|>user\n',
      input_suffix: '<|im_end|>\n',
      output_sequence: '<|im_start|>assistant\n',
      output_suffix: '<|im_end|>\n',
      system_same_as_user: false,
      stop_sequence: '<|im_end|>\n<|im_start|>',
    },
  },
  {
    id: 'mistral',
    name: 'Mistral / Mixtral Instruct',
    description: 'Mistral 官方 [INST] 格式',
    settings: {
      enabled: true,
      preset: 'Mistral',
      wrap: true,
      names_behavior: 'none',
      system_sequence: '[INST] ',
      system_suffix: ' [/INST]',
      input_sequence: '[INST] ',
      input_suffix: ' [/INST]',
      output_sequence: '',
      output_suffix: '</s>',
      system_same_as_user: true,
      stop_sequence: '</s>\n[INST]',
    },
  },
  {
    id: 'gemma',
    name: 'Google Gemma / Gemma 2',
    description: 'Google Gemma 系列指令格式（<start_of_turn>...<end_of_turn>）',
    settings: {
      enabled: true,
      preset: 'Gemma',
      wrap: false,
      names_behavior: 'none',
      system_sequence: '<start_of_turn>user\n',
      system_suffix: '<end_of_turn>',
      input_sequence: '<start_of_turn>user\n',
      input_suffix: '<end_of_turn>',
      output_sequence: '<start_of_turn>model\n',
      output_suffix: '<end_of_turn>',
      system_same_as_user: true,
      stop_sequence: '<end_of_turn>\n<start_of_turn>',
    },
  },
  {
    id: 'command_r',
    name: 'Cohere Command-R / R+',
    description: 'Cohere 官方对话转义格式（<|START_OF_TURN_TOKEN|>...）',
    settings: {
      enabled: true,
      preset: 'Command-R',
      wrap: false,
      names_behavior: 'none',
      system_sequence: '<|START_OF_TURN_TOKEN|><|SYSTEM_TOKEN|>',
      system_suffix: '<|END_OF_TURN_TOKEN|>',
      input_sequence: '<|START_OF_TURN_TOKEN|><|USER_TOKEN|>',
      input_suffix: '<|END_OF_TURN_TOKEN|>',
      output_sequence: '<|START_OF_TURN_TOKEN|><|CHATBOT_TOKEN|>',
      output_suffix: '<|END_OF_TURN_TOKEN|>',
      system_same_as_user: false,
      stop_sequence: '<|END_OF_TURN_TOKEN|>',
    },
  },
  {
    id: 'alpaca',
    name: 'Alpaca',
    description: '经典 Stanford Alpaca 提示词格式',
    settings: {
      enabled: true,
      preset: 'Alpaca',
      wrap: true,
      names_behavior: 'none',
      system_sequence: '### Instruction:\n',
      system_suffix: '\n\n',
      input_sequence: '### Input:\n',
      input_suffix: '\n\n',
      output_sequence: '### Response:\n',
      output_suffix: '\n\n',
      system_same_as_user: false,
      stop_sequence: '### Instruction:\n### Input:',
    },
  },
  {
    id: 'vicuna',
    name: 'Vicuna',
    description: 'FastChat Vicuna 格式',
    settings: {
      enabled: true,
      preset: 'Vicuna',
      wrap: true,
      names_behavior: 'none',
      system_sequence: 'SYSTEM: ',
      system_suffix: '\n',
      input_sequence: 'USER: ',
      input_suffix: '\n',
      output_sequence: 'ASSISTANT: ',
      output_suffix: '\n',
      system_same_as_user: false,
      stop_sequence: 'USER:\nASSISTANT:',
    },
  },
]

import { getPreset, listPresets, savePreset, deletePreset } from './presets'

/** 将 ST 原始 instruct JSON 转换为 InstructSettings */
export function rawToInstructSettings(raw: Record<string, unknown>, name?: string): InstructSettings {
  return {
    enabled: true,
    preset: name || (typeof raw.name === 'string' ? raw.name : 'Custom'),
    system_sequence: typeof raw.system_sequence === 'string' ? raw.system_sequence : '',
    system_suffix: typeof raw.system_suffix === 'string' ? raw.system_suffix : '\n',
    input_sequence: typeof raw.input_sequence === 'string' ? raw.input_sequence : '',
    input_suffix: typeof raw.input_suffix === 'string' ? raw.input_suffix : '\n',
    output_sequence: typeof raw.output_sequence === 'string' ? raw.output_sequence : '',
    output_suffix: typeof raw.output_suffix === 'string' ? raw.output_suffix : '\n',
    first_input_sequence: typeof raw.first_input_sequence === 'string' ? raw.first_input_sequence : '',
    last_input_sequence: typeof raw.last_input_sequence === 'string' ? raw.last_input_sequence : '',
    first_output_sequence: typeof raw.first_output_sequence === 'string' ? raw.first_output_sequence : '',
    last_output_sequence: typeof raw.last_output_sequence === 'string' ? raw.last_output_sequence : '',
    stop_sequence: typeof raw.stop_sequence === 'string' ? raw.stop_sequence : '',
    wrap: typeof raw.wrap === 'boolean' ? raw.wrap : true,
    macro: typeof raw.macro === 'boolean' ? raw.macro : true,
    names_behavior: (raw.names_behavior as InstructSettings['names_behavior']) || 'none',
    system_same_as_user: typeof raw.system_same_as_user === 'boolean' ? raw.system_same_as_user : false,
  }
}

/** 读取用户自定义 Instruct 预设（从 default-user/instruct/） */
export async function loadUserInstructPresets(): Promise<InstructPresetItem[]> {
  const names = await listPresets('instruct')
  const items: InstructPresetItem[] = []
  for (const n of names) {
    const raw = await getPreset(n, 'instruct')
    if (raw) {
      items.push({
        id: `user_${n}`,
        name: `${n} (用户预设)`,
        description: `来自 instruct/${n}.json`,
        settings: rawToInstructSettings(raw, n),
      })
    }
  }
  return items
}

/** 保存自定义 Instruct 预设 */
export async function saveUserInstructPreset(name: string, settings: InstructSettings): Promise<void> {
  const payload: Record<string, unknown> = {
    name,
    system_sequence: settings.system_sequence,
    system_suffix: settings.system_suffix,
    input_sequence: settings.input_sequence,
    input_suffix: settings.input_suffix,
    output_sequence: settings.output_sequence,
    output_suffix: settings.output_suffix,
    first_input_sequence: settings.first_input_sequence,
    last_input_sequence: settings.last_input_sequence,
    first_output_sequence: settings.first_output_sequence,
    last_output_sequence: settings.last_output_sequence,
    stop_sequence: settings.stop_sequence,
    wrap: settings.wrap,
    macro: settings.macro ?? true,
    names_behavior: settings.names_behavior,
    system_same_as_user: settings.system_same_as_user,
  }
  await savePreset(name, payload, 'instruct')
}

/** 删除自定义 Instruct 预设 */
export async function deleteUserInstructPreset(name: string): Promise<void> {
  await deletePreset(name, 'instruct')
}

/** 导入 .json 文件 */
export async function importInstructPresetFile(file: File): Promise<InstructPresetItem> {
  const text = await file.text()
  const raw = JSON.parse(text) as Record<string, unknown>
  const name = file.name.replace(/\.json$/i, '').trim() || 'Imported_Instruct'
  const settings = rawToInstructSettings(raw, name)
  await saveUserInstructPreset(name, settings)
  return {
    id: `user_${name}`,
    name: `${name} (用户预设)`,
    description: `来自导入文件 ${file.name}`,
    settings,
  }
}

/** 导出 .json 文件 */
export function exportInstructPresetFile(name: string, settings: InstructSettings): void {
  const payload = {
    name,
    system_sequence: settings.system_sequence,
    system_suffix: settings.system_suffix,
    input_sequence: settings.input_sequence,
    input_suffix: settings.input_suffix,
    output_sequence: settings.output_sequence,
    output_suffix: settings.output_suffix,
    first_input_sequence: settings.first_input_sequence,
    last_input_sequence: settings.last_input_sequence,
    first_output_sequence: settings.first_output_sequence,
    last_output_sequence: settings.last_output_sequence,
    stop_sequence: settings.stop_sequence,
    wrap: settings.wrap,
    macro: settings.macro ?? true,
    names_behavior: settings.names_behavior,
    system_same_as_user: settings.system_same_as_user,
  }
  const str = JSON.stringify(payload, null, 4)
  const blob = new Blob([str], { type: 'application/json' })
  const url = URL.createObjectURL(blob)
  const a = document.createElement('a')
  a.href = url
  a.download = `${name.trim() || 'instruct'}.json`
  document.body.appendChild(a)
  a.click()
  document.body.removeChild(a)
  URL.revokeObjectURL(url)
}

