// 后端错误信息提取与按码本地化（issue #342 二期 / ADR-0049）：
// Tauri invoke 失败时 reject 的值是后端 `AppError` 的 serde 序列化形态
// `{"kind": "...", "message": "..."}`（对象而非 Error 实例），直接模板字符串
// 拼接会得到 `[object Object]`。此处统一抽取 message 字段，兼容字符串、
// Error 实例与未知形态。
//
// 错误码化（只增不改契约）：序列化形态可含稳定 `code` 与可选 `params`
// （插值参数数组）。当前语言配置了该码的本地化文案（`errors.<code>`，码内
// 点号即嵌套路径）则插值翻译；无码、未知码或未配翻译时降级透传后端中文原文。
import { currentLocale, i18n } from '@/i18n'

/** 从错误对象中提取码化字段（code 必须为非空字符串；params 过滤保留字符串项） */
function extractCode(e: unknown): { code: string; params: string[] } | null {
  if (typeof e !== 'object' || e === null) return null
  const code = (e as { code?: unknown }).code
  if (typeof code !== 'string' || !code) return null
  const raw = (e as { params?: unknown }).params
  const params = Array.isArray(raw) ? raw.filter((p): p is string => typeof p === 'string') : []
  return { code, params }
}

export function errorMessage(e: unknown): string {
  // 码化错误优先：按码查当前语言文案并插值（如缺汇率错误插出 USD→CNY）
  const coded = extractCode(e)
  if (coded) {
    const key = `errors.${coded.code}`
    if (i18n.global.te(key, currentLocale.value)) {
      return coded.params.length > 0 ? i18n.global.t(key, coded.params) : i18n.global.t(key)
    }
    // 无码或未知码：降级透传原文（message 恒中文，永远可读）
  }
  if (typeof e === 'string') return e
  if (e instanceof Error) return e.message
  if (typeof e === 'object' && e !== null && 'message' in e) {
    const message = (e as { message: unknown }).message
    if (typeof message === 'string' && message) return message
  }
  return String(e)
}
