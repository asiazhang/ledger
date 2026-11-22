export function formatLocalDateISO(y: number, month0: number, day: number): string {
  const month = String(month0 + 1).padStart(2, "0");
  const d = String(day).padStart(2, "0");
  return `${y}-${month}-${d}`;
}

export function toLocalDateISO(date: number | Date): string {
  const d = typeof date === "number" ? new Date(date) : date;
  return formatLocalDateISO(d.getFullYear(), d.getMonth(), d.getDate());
}

export function todayStr(): string {
  return toLocalDateISO(new Date());
}
