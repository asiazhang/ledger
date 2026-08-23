/**
 * 元（用户输入字符串，支持小数）→ 分（整数）。
 *
 * - 空字符串 / 纯空白 → null（表示「不筛选」）
 * - 非数字（含 1e3 这类科学计数法）→ null
 * - 合法金额四舍五入到分：15.5 → 1550
 * - 超出安全整数范围（如 1e308 乘以 100 溢出为 Infinity）→ null
 *
 * 用 toFixed(8) 消除二进制浮点误差（如 15.505 * 100 实际为 1550.4999999999998）。
 */
export function yuanToCents(yuan: string): number | null {
  const trimmed = yuan.trim()
  if (!trimmed) return null
  // 允许 .5 这类省略整数部分的写法（'15'、'.5'、'15.5' 均可；'15.'、'abc'、'1e3' 拒绝）
  if (!/^-?\d*\.?\d+$/.test(trimmed)) return null
  const num = Number(trimmed)
  if (!Number.isFinite(num)) return null
  const cents = Math.round(Number((num * 100).toFixed(8)))
  return Number.isSafeInteger(cents) ? cents : null
}
