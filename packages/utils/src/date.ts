/** 本地日历年月日 → YYYY-MM-DD（月份 0 起）。本地日历日语义的格式化单点。 */
export function formatLocalDateISO(y: number, month0: number, day: number): string {
  const month = String(month0 + 1).padStart(2, '0')
  const d = String(day).padStart(2, '0')
  return `${y}-${month}-${d}`
}

/**
 * 本地日历日语义 → YYYY-MM-DD：取该时刻在本地时区的年月日。
 * 替代 UTC `toISOString().slice(0, 10)` 截断——后者在本地 0–8 点（东八区）
 * 会把用户所选日漂移为前一天。时间戳（number）与 Date 双输入。
 */
export function toLocalDateISO(date: number | Date): string {
  const d = typeof date === 'number' ? new Date(date) : date
  return formatLocalDateISO(d.getFullYear(), d.getMonth(), d.getDate())
}

/** 本地时区今天（YYYY-MM-DD），作为日期表单默认值。与 toLocalDateISO 同一口径。 */
export function todayStr(): string {
  return toLocalDateISO(new Date())
}
