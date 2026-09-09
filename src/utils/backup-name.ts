/**
 * 受管备份命名规则（与后端 `MANAGED_BACKUP_PREFIXES` 保持一致，ADR-0016，issue #127）：
 * 手动 `ledger-backup-YYYYMMDD-HHMMSS[-<账本标识>].db.zip` +
 * 自动 `ledger-auto-YYYYMMDD-HHMMSS[-<账本标识>].db.zip`。
 * 两类同等参与滚动清理与首次兜底判定；自动备份产物由后端生成，
 * 前端只消费前缀集合做受管判定（不代为命名自动备份）。
 * 账本标识（issue #836 / ADR-0089 决策 5）：手动备份默认名携带当前活动账本
 * 标识，两本账的产物在共享备份目录内可区分、互不覆盖；标识位于时间戳之后、
 * 可缺省（无作用域现场退化为旧命名）。
 */
export const MANUAL_BACKUP_PREFIX = "ledger-backup-";
export const AUTO_BACKUP_PREFIX = "ledger-auto-";
export const MANAGED_BACKUP_SUFFIX = ".db.zip";

const pad = (n: number) => String(n).padStart(2, "0");

function timestamp(d: Date): string {
  return `${d.getFullYear()}${pad(d.getMonth() + 1)}${pad(d.getDate())}-${pad(d.getHours())}${pad(d.getMinutes())}${pad(d.getSeconds())}`;
}

/** 手动备份默认文件名：`ledger-backup-YYYYMMDD-HHMMSS[-<账本标识>].db.zip`。 */
export function defaultBackupFileName(
  now: Date = new Date(),
  bookId?: string | null,
): string {
  const base = `${MANUAL_BACKUP_PREFIX}${timestamp(now)}`;
  return bookId ? `${base}-${bookId}.db.zip` : `${base}.db.zip`;
}

/** 文件名是否为受管备份：命中任一受管前缀且带标准后缀。 */
export function isManagedBackupFileName(name: string): boolean {
  return (
    [MANUAL_BACKUP_PREFIX, AUTO_BACKUP_PREFIX].some((prefix) =>
      name.startsWith(prefix),
    ) && name.endsWith(MANAGED_BACKUP_SUFFIX)
  );
}

/** 规整备份目录：去掉尾部斜杠，返回目录与分隔符。 */
export function normalizeBackupDir(raw: string): {
  dir: string;
  sep: "/" | "\\";
} {
  const dir = raw.replace(/[\\/]+$/, "");
  const sep = dir.includes("\\") ? "\\" : "/";
  return { dir, sep };
}

/** 目标路径是否为受管备份：位于配置的备份目录内且文件名匹配受管命名规则。 */
export function isManagedBackupPath(
  target: string,
  backupDir: string,
): boolean {
  if (!backupDir) return false;
  const { dir, sep } = normalizeBackupDir(backupDir);
  const base = target.split(/[\\/]/).pop() ?? "";
  return target.startsWith(dir + sep) && isManagedBackupFileName(base);
}
