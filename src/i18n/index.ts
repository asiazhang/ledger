// 界面语言（i18n）单例与语言判定/切换（issue #342 / ADR-0048）。
//
// 选型与口径：
// - vue-i18n Composition 模式（legacy: false）；中文 zh-CN 是源语言与回退语言，
//   未翻译 key 一律回退显示中文原文而非 key 代号；en-US 文案包懒加载（切换时才拉取）。
// - 语言判定链：手动覆盖（localStorage，轻量设置项 ADR-0017）> 系统语言 > zh-CN。
// - 组件统一经本模块导出的 t() 消费翻译（直接绑定全局 Composer，天然响应 locale
//   变化），不另建组件级 scope；模块/普通函数（列定义、菜单构造器）同样可用。
// - 测试环境不调用 initAppLocale()，语言恒为 zh-CN——既有中文断言测试零改动。
import { ref } from 'vue'
import { createI18n } from 'vue-i18n'
import { loadLocal, saveLocal } from '@/utils/storage'
import zhCN from './locales/zh-CN'

/** 生效界面语言（已解析；'system' 不是生效语言，只是偏好层取值） */
export type Locale = 'zh-CN' | 'en-US'
/** 语言偏好：'system' = 跟随系统语言（判定链默认层） */
export type LocaleSetting = Locale | 'system'

export const LOCALE_STORAGE_KEY = 'locale'
export const SOURCE_LOCALE: Locale = 'zh-CN'
export const SUPPORTED_LOCALES: Locale[] = ['zh-CN', 'en-US']

/** 当前生效语言（响应式）：数字分组、组件库 locale 等展示层口径统一消费此处。
 *  初始恒为 zh-CN，由 initAppLocale() 在应用启动时按判定链刷新。 */
export const currentLocale = ref<Locale>('zh-CN')

/** 解析系统语言：navigator.language 主子标签以 en 开头 → en-US，其余 → zh-CN（源语言兜底） */
export function resolveSystemLocale(): Locale {
  const lang = typeof navigator !== 'undefined' ? navigator.language : ''
  return lang && lang.toLowerCase().startsWith('en') ? 'en-US' : 'zh-CN'
}

/** 语言判定链（ADR-0048）：手动覆盖 > 系统语言 > zh-CN。'system' 即无手动覆盖。 */
export function resolveLocale(setting: LocaleSetting): Locale {
  return setting === 'system' ? resolveSystemLocale() : setting
}

export const i18n = createI18n({
  legacy: false,
  locale: 'zh-CN' as Locale,
  fallbackLocale: SOURCE_LOCALE,
  // 显式宽化消息表类型：locale 联合由 Locale 决定，不被初始字面量收窄成 zh-CN 单值
  messages: { 'zh-CN': zhCN } as Record<Locale, typeof zhCN>,
  // 开发模式缺失 key 控制台告警：当前 locale 缺 key 即告警（含已回退中文的情形），
  // 让漏翻在写代码时暴露；生产环境静默回退。
  missing: (locale, key) => {
    if (import.meta.env.DEV) {
      console.warn(`[i18n] 缺失文案 key：${key}（locale=${String(locale)}）`)
    }
  },
})

/** 惰性加载非源语言文案包：Vite 按动态 import 把每个 locale 切成独立 chunk */
async function loadLocaleBundle(locale: Locale): Promise<void> {
  if (locale === SOURCE_LOCALE || i18n.global.availableLocales.includes(locale)) return
  const messages = await import(`./locales/${locale}/index.ts`)
  i18n.global.setLocaleMessage(locale, messages.default)
}

/** 切换生效语言：按需加载文案包 → 切换 → currentLocale 通知全部展示层口径 */
export async function applyLocale(locale: Locale): Promise<void> {
  await loadLocaleBundle(locale)
  i18n.global.locale.value = locale
  currentLocale.value = locale
}

/** 读取语言偏好（轻量设置项：localStorage，不随 Backup/Restore 迁移） */
export function getLocaleSetting(): LocaleSetting {
  return loadLocal<LocaleSetting>(LOCALE_STORAGE_KEY, 'system')
}

/** 写入语言偏好并即时生效（无需重启） */
export async function setLocaleSetting(setting: LocaleSetting): Promise<void> {
  saveLocal(LOCALE_STORAGE_KEY, setting)
  await applyLocale(resolveLocale(setting))
}

/** 应用启动时按判定链初始化语言（main.ts 显式 await；测试环境不调用，恒为中文） */
export async function initAppLocale(): Promise<void> {
  await applyLocale(resolveLocale(getLocaleSetting()))
}

/**
 * 翻译函数：全项目统一消费入口。绑定全局 Composer，在渲染/计算属性等响应式
 * 上下文中调用时自动追踪 locale 变化（切换语言即时重渲染）。
 * 第二参可传命名插值对象（`{n}` 消费 `{n}`）或位置插值数组（消息消费 `{0} {1}`）。
 */
export function t(key: string, params?: Record<string, unknown> | unknown[]): string {
  if (params === undefined) return i18n.global.t(key)
  if (Array.isArray(params)) return i18n.global.t(key, params)
  return i18n.global.t(key, params)
}
