/**
 * 后端错误信息提取：Tauri invoke 失败时 reject 的值是后端 `AppError` 的
 * serde 序列化形态 `{"kind": "...", "message": "..."}`（对象而非 Error 实例），
 * 直接模板字符串拼接会得到 `[object Object]`。此处统一抽取 message 字段，
 * 兼容字符串、Error 实例与未知形态。
 */
export function errorMessage(e: unknown): string {
  if (typeof e === 'string') return e
  if (e instanceof Error) return e.message
  if (typeof e === 'object' && e !== null && 'message' in e) {
    const message = (e as { message: unknown }).message
    if (typeof message === 'string' && message) return message
  }
  return String(e)
}
