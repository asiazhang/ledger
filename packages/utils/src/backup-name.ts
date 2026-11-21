export const MANUAL_BACKUP_PREFIX = "ledger-backup-";
export const AUTO_BACKUP_PREFIX = "ledger-auto-";
export const MANAGED_BACKUP_SUFFIX = ".db.zip";

const pad = (n: number) => String(n).padStart(2, "0");

function timestamp(d: Date): string {
  return `${d.getFullYear()}${pad(d.getMonth() + 1)}${pad(d.getDate())}-${pad(d.getHours())}${pad(d.getMinutes())}${pad(d.getSeconds())}`;
}

export function defaultBackupFileName(now: Date = new Date(), bookTag?: string | null): string {
  const base = `${MANUAL_BACKUP_PREFIX}${timestamp(now)}`;
  return bookTag ? `${base}-${bookTag}.db.zip` : `${base}.db.zip`;
}

export function isManagedBackupFileName(name: string): boolean {
  return (
    [MANUAL_BACKUP_PREFIX, AUTO_BACKUP_PREFIX].some((prefix) => name.startsWith(prefix)) &&
    name.endsWith(MANAGED_BACKUP_SUFFIX)
  );
}

export function normalizeBackupDir(raw: string): {
  dir: string;
  sep: "/" | "\\";
} {
  const dir = raw.replace(/[\\/]+$/, "");
  const sep = dir.includes("\\") ? "\\" : "/";
  return { dir, sep };
}

export function isManagedBackupPath(target: string, backupDir: string): boolean {
  if (!backupDir) return false;
  const { dir, sep } = normalizeBackupDir(backupDir);
  const base = target.split(/[\\/]/).pop() ?? "";
  return target.startsWith(dir + sep) && isManagedBackupFileName(base);
}
