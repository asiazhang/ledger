import { describe, expect, it } from "vitest";
import {
  AUTO_BACKUP_PREFIX,
  MANUAL_BACKUP_PREFIX,
  defaultBackupFileName,
  isManagedBackupFileName,
  isManagedBackupPath,
} from "@/utils/backupName";

describe("受管备份命名与判定（issue #127）", () => {
  it("defaultBackupFileName 生成手动前缀 + 标准后缀命名", () => {
    expect(defaultBackupFileName(new Date(2026, 1, 17, 9, 30, 5))).toBe(
      "ledger-backup-20260217-093005.db.zip",
    );
  });

  it("isManagedBackupFileName 覆盖手动与自动两类前缀", () => {
    expect(
      isManagedBackupFileName(`${MANUAL_BACKUP_PREFIX}20260217-093005.db.zip`),
    ).toBe(true);
    expect(
      isManagedBackupFileName(`${AUTO_BACKUP_PREFIX}20260217-093005.db.zip`),
    ).toBe(true);
  });

  it("isManagedBackupFileName 拒绝非受管命名", () => {
    expect(isManagedBackupFileName("notes.zip")).toBe(false);
    // 前缀匹配但缺标准后缀：不是受管备份。
    expect(isManagedBackupFileName(`${MANUAL_BACKUP_PREFIX}notes.txt`)).toBe(
      false,
    );
    expect(
      isManagedBackupFileName(`my-ledger-auto-20260217-093005.db.zip`),
    ).toBe(false);
  });

  it("isManagedBackupPath 仅认可备份目录内的受管命名目标", () => {
    const dir = "/data/backups";
    expect(
      isManagedBackupPath(`${dir}/ledger-auto-20260217-093005.db.zip`, dir),
    ).toBe(true);
    // 目录尾部斜杠归一化后仍命中。
    expect(
      isManagedBackupPath(
        `${dir}/ledger-backup-20260217-093005.db.zip`,
        `${dir}/`,
      ),
    ).toBe(true);
    // 目录外 / 未配置目录 / 前缀但文件名不符 → 非受管。
    expect(
      isManagedBackupPath(`/elsewhere/ledger-auto-20260217-093005.db.zip`, dir),
    ).toBe(false);
    expect(
      isManagedBackupPath(`${dir}/ledger-auto-20260217-093005.db.zip`, ""),
    ).toBe(false);
    expect(isManagedBackupPath(`${dir}/other.db.zip`, dir)).toBe(false);
  });
});
