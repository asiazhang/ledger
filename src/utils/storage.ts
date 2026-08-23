// localStorage 读写辅助：JSON 序列化 + 静默容错。
// 项目约定：UI 状态（偏好、视图状态）存 localStorage，与业务数据（SQLite）分域。

export function loadLocal<T>(key: string, fallback: T): T {
  try {
    const raw = localStorage.getItem(key)
    if (raw !== null) return JSON.parse(raw) as T
  } catch { /* ignore */ }
  return fallback
}

export function saveLocal<T>(key: string, value: T) {
  try {
    localStorage.setItem(key, JSON.stringify(value))
  } catch { /* ignore */ }
}
