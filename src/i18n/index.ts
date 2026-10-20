import { createI18n } from 'vue-i18n'
import zhCN from '../locales/zh-CN.json'
import enUS from '../locales/en-US.json'

export type MessageSchema = typeof zhCN

const savedAppearance = typeof localStorage !== 'undefined' ? localStorage.getItem('app.appearance') : null
let initialLocale = 'zh-CN'
try {
  if (savedAppearance) {
    const parsed = JSON.parse(savedAppearance)
    if (parsed.lang) initialLocale = parsed.lang
  }
} catch {
  // ignore
}

export const i18n = createI18n<[MessageSchema], 'zh-CN' | 'en-US'>({
  legacy: false,
  locale: initialLocale,
  fallbackLocale: 'zh-CN',
  messages: {
    'zh-CN': zhCN,
    'en-US': enUS,
  },
})

/**
 * 切换全局语言
 */
export function setAppLanguage(lang: 'zh-CN' | 'en-US'): void {
  // @ts-expect-error vue-i18n locale ref
  i18n.global.locale.value = lang
}
