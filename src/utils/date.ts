/** 本地时区今天（YYYY-MM-DD），作为日期表单默认值。 */
export function todayStr(): string {
  const d = new Date()
  const month = String(d.getMonth() + 1).padStart(2, '0')
  const day = String(d.getDate()).padStart(2, '0')
  return `${d.getFullYear()}-${month}-${day}`
}
