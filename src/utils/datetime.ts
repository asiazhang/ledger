// 展示用时刻格式化的单一定义点：ISO UTC 字符串 → 「YYYY-MM-DD HH:MM」本地
// 可读截断（精确到分钟；后端时刻恒 UTC ISO，展示不做时区换算，备份域先例）。
export function formatIsoMinute(iso: string): string {
  return iso.slice(0, 16).replace('T', ' ')
}
