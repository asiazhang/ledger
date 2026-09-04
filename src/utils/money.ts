import { ref } from 'vue'
import type { Currency } from '@/types/currencies'
import { currentLocale, type Locale } from '@/i18n'

/** 金额隐私模式的 localStorage key（轻量设置项，水合与持久化收口在应用设置 store） */
export const AMOUNT_PRIVACY_STORAGE_KEY = 'amount_privacy_enabled'

/**
 * 金额隐私模式开关（issue #566，轻量设置项 ADR-0017 口径）：模块级响应式单点，
 * 三个展示格式化函数消费此开关（同界面语言 currentLocale 注入先例），全应用金额展示
 * 收口隐藏、调用点零判断；应用设置 store 负责启动水合与变更持久化，测试可直接置位。
 */
export const amountPrivacyEnabled = ref(false)

/** 掩码恒为固定长度，无负号、无币种符号、不随语言与数量级变化（spec #564 掩码恒等性） */
const AMOUNT_PRIVACY_MASK = '••••'

/**
 * 分组位数由界面语言隐含（词汇表「数字分组」）：中文每 4 位一组，英文每 3 位一组。
 * 其余口径（小数尾零、负号位置、小数部分不分组）不随语言变。
 */
function groupSizeFor(locale: Locale): number {
  return locale === 'en-US' ? 3 : 4
}

/**
 * 纯数字字符串按指定位数切组：从右向左每 size 位一组、半角逗号分隔。
 * 输入为不含符号与小数点的纯数字串。
 */
function joinGroups(digits: string, size: number): string {
  const groups: string[] = []
  for (let i = digits.length; i > 0; i -= size) groups.unshift(digits.slice(Math.max(0, i - size), i))
  return groups.join(',')
}

/**
 * 数字字符串展示分组核心助手：整数部分从右向左按界面语言位数分组、半角逗号分隔；
 * 小数部分连续输出不插入分隔符；负号保留在最前不受分组影响。
 * formatAmount 与 formatQuantity 共享同一口径（见 CONTEXT.md「数字分组」）。
 */
function groupNumberString(numStr: string, size: number): string {
  const sign = numStr.startsWith('-') ? '-' : ''
  const body = sign ? numStr.slice(1) : numStr
  const dot = body.indexOf('.')
  if (dot === -1) return `${sign}${joinGroups(body, size)}`
  return `${sign}${joinGroups(body.slice(0, dot), size)}${body.slice(dot)}`
}

/**
 * 数量格式化（股数/份额列）：整数部分按界面语言分组，小数部分原样保留
 * （份额为 f64，可能带小数）。与 formatAmount 共享同一分组口径。
 * locale 缺省取应用当前语言（响应式：渲染中调用随语言切换重渲染），
 * 测试可显式传入以回归两种语言口径。
 */
export function formatQuantity(quantity: number, locale: Locale = currentLocale.value): string {
  if (amountPrivacyEnabled.value) return AMOUNT_PRIVACY_MASK
  return groupNumberString(String(quantity), groupSizeFor(locale))
}

/** 分 -> 元字符串，按币种小数位换算后裁剪小数尾零（98.00→98、98.50→98.5，无损去零不涉舍入）；
 *  整数部分走界面语言分组（展示层全局口径，见词汇表「数字分组」）；locale 缺省取应用当前语言 */
export function formatAmount(cents: number, currency?: Currency, locale: Locale = currentLocale.value): string {
  if (amountPrivacyEnabled.value) return AMOUNT_PRIVACY_MASK
  const dp = currency?.decimal_places ?? 2
  const sign = cents < 0 ? '-' : ''
  const abs = Math.abs(cents)
  const value = abs / Math.pow(10, dp)
  const fixed = value.toFixed(dp)
  // 整数分转字符串只去零、不做舍入；小数部分全空时连小数点一起去掉
  const trimmed = dp > 0 ? fixed.replace(/0+$/, '').replace(/\.$/, '') : fixed
  const symbol = currency?.symbol ?? ''
  return `${sign}${symbol}${groupNumberString(trimmed, groupSizeFor(locale))}`
}

/**
 * 分 → 元数值（表单初值等需要数值形态的场景；展示一律用 formatAmount）。
 * 与 formatAmount 共享同一换算口径（按币种小数位 10^dp），不要手写 /100。
 */
export function centsToYuan(cents: number, currency?: Currency): number {
  const dp = currency?.decimal_places ?? 2
  return cents / Math.pow(10, dp)
}

/**
 * 万分之一元 → 元字符串（价格列展示专用，ADR-0038 价格刻度）：固定 4 位小数后裁剪尾零
 * （1.2345 → 1.2345、15.00 → 15、475.2000 → 475.2，无损去零不涉舍入）；
 * 股票两位价、港股三位价、基金四位净值同一直口。整数部分走界面语言分组；locale 缺省取应用当前语言。
 */
export function formatPrice(price: number, currency?: Currency, locale: Locale = currentLocale.value): string {
  if (amountPrivacyEnabled.value) return AMOUNT_PRIVACY_MASK
  const sign = price < 0 ? '-' : ''
  const abs = Math.abs(price)
  const fixed = (abs / 10000).toFixed(4)
  // 万分之一元整转字符串只去零、不做舍入；小数部分全空时连小数点一起去掉
  const trimmed = fixed.replace(/0+$/, '').replace(/\.$/, '')
  const symbol = currency?.symbol ?? ''
  return `${sign}${symbol}${groupNumberString(trimmed, groupSizeFor(locale))}`
}

/**
 * 元（用户输入 string | number）→ 万分之一元整数（价格列，ADR-0038 刻度）。
 * 口径与 yuanToCents 同一：空/非法/非有限数返回 null；合法值四舍五入
 * （12.34505 元 → 123451）；超出安全整数范围返回 null。
 */
export function yuanToPrice(yuan: string | number): number | null {
  const trimmed = typeof yuan === 'number' ? String(yuan) : yuan.trim()
  if (!trimmed) return null
  if (!/^-?\d*\.?\d+$/.test(trimmed)) return null
  const num = Number(trimmed)
  if (!Number.isFinite(num)) return null
  const price = Math.round(Number((num * 10000).toFixed(8)))
  return Number.isSafeInteger(price) ? price : null
}

/**
 * 万分之一元 → 元数值（表单回填等需要数值形态的场景；展示一律用 formatPrice）。
 * 与 formatPrice 共享同一换算口径（固定 ÷ 10000，不按币种小数位）。
 */
export function priceToYuan(price: number): number {
  return price / 10000
}

/**
 * 元（用户输入 string | number，支持小数）→ 分（整数）。
 *
 * - 空字符串 / 纯空白 → null（表示「不筛选」）；NaN / Infinity → null
 * - 非数字（含 1e3 这类科学计数法）→ null
 * - 合法金额四舍五入到分：15.5 → 1550
 * - 超出安全整数范围（如 1e308 乘以 100 溢出为 Infinity）→ null
 *
 * string 与 number 分支收敛到同一 toFixed(8) 消浮点误差口径
 * （如 15.505 * 100 实际为 1550.4999999999998）：number 先 String() 化走同一管道，不另写算法。
 */
export function yuanToCents(yuan: string | number): number | null {
  const trimmed = typeof yuan === 'number' ? String(yuan) : yuan.trim()
  if (!trimmed) return null
  // 允许 .5 这类省略整数部分的写法（'15'、'.5'、'15.5' 均可；'15.'、'abc'、'1e3' 拒绝）
  if (!/^-?\d*\.?\d+$/.test(trimmed)) return null
  const num = Number(trimmed)
  if (!Number.isFinite(num)) return null
  const cents = Math.round(Number((num * 100).toFixed(8)))
  return Number.isSafeInteger(cents) ? cents : null
}
