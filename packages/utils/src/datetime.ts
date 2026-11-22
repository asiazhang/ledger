export function formatIsoMinute(iso: string): string {
  return iso.slice(0, 16).replace("T", " ");
}
