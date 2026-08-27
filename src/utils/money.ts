import type { Currency } from '@/types/currencies'

/**
 * 纯数字字符串按万分位切组：从右向左每 4 位一组、半角逗号分隔。
 * 输入为不含符号与小数点的纯数字串。
 */
function joinGroups(digits: string): string {
  const groups: string[] = []
  for (let i = digits.length; i > 0; i -= 4) groups.unshift(digits.slice(Math.max(0, i - 4), i))
  return groups.join(',')
}

/**
 * 数字字符串展示分组核心助手：整数部分从右向左每 4 位一组、半角逗号分隔；
 * 小数部分连续输出不插入分隔符；负号保留在最前不受分组影响。
 * formatAmount 与 formatQuantity 共享同一口径（见 CONTEXT.md「万分位分组」）。
 */
function groupNumberString(numStr: string): string {
  const sign = numStr.startsWith('-') ? '-' : ''
  const body = sign ? numStr.slice(1) : numStr
  const dot = body.indexOf('.')
  if (dot === -1) return `${sign}${joinGroups(body)}`
  return `${sign}${joinGroups(body.slice(0, dot))}${body.slice(dot)}`
}

/**
 * 数量格式化（股数/份额列）：整数部分按万分位分组，小数部分原样保留
 * （份额为 f64，可能带小数）。与 formatAmount 共享同一分组口径。
 */
export function formatQuantity(quantity: number): string {
  return groupNumberString(String(quantity))
}

/** 分 -> 元字符串，按币种小数位换算后裁剪小数尾零（98.00→98、98.50→98.5，无损去零不涉舍入）；整数部分走万分位分组（展示层全局口径） */
export function formatAmount(cents: number, currency?: Currency): string {
  const dp = currency?.decimal_places ?? 2
  const sign = cents < 0 ? '-' : ''
  const abs = Math.abs(cents)
  const value = abs / Math.pow(10, dp)
  const fixed = value.toFixed(dp)
  // 整数分转字符串只去零、不做舍入；小数部分全空时连小数点一起去掉
  const trimmed = dp > 0 ? fixed.replace(/0+$/, '').replace(/\.$/, '') : fixed
  const symbol = currency?.symbol ?? ''
  return `${sign}${symbol}${groupNumberString(trimmed)}`
}

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
