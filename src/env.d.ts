/// <reference types="vite/client" />

declare module '*.vue' {
  import type { DefineComponent } from 'vue'
  const component: DefineComponent<Record<string, never>, Record<string, never>, unknown>
  export default component
}

declare module 'fengari' {
  export const lua: any
  export const lauxlib: any
  export const lualib: any
  export function to_luastring(s: string): any
  export function to_jsstring(s: any): string
}
