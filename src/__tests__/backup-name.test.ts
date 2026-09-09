import { describe, expect, it } from "vitest";
import {
  AUTO_BACKUP_PREFIX,
  MANUAL_BACKUP_PREFIX,
  defaultBackupFileName,
  isManagedBackupFileName,
  isManagedBackupPath,
} from "@/utils/backup-name";

describe("受管备份命名与判定（issue #127）", () => {
  it("defaultBackupFileName 生成手动前缀 + 标准后缀命名", () => {
    expect(defaultBackupFileName(new Date(2026, 1, 17, 9, 30, 5))).toBe(
      "ledger-backup-20260217-093005.db.zip",
    );
  });

  it("defaultBackupFileName 携带账本标识（issue #836）：标识位于时间戳之后", () => {
    expect(
      defaultBackupFileName(new Date(2026, 1, 17, 9, 30, 5), "book-a"),
    ).toBe("ledger-backup-20260217-093005-book-a.db.zip");
    // 标识含连字符（UUID 形态）不受影响；空/缺失退化为旧命名。
    expect(
      defaultBackupFileName(
        new Date(2026, 1, 17, 9, 30, 5),
        "3f2a9c4e-8b1d-4c2a-9f3e-5a7b8c9d0e1f",
      ),
    ).toBe(
      "ledger-backup-20260217-093005-3f2a9c4e-8b1d-4c2a-9f3e-5a7b8c9d0e1f.db.zip",
    );
    expect(defaultBackupFileName(new Date(2026, 1, 17, 9, 30, 5), null)).toBe(
      "ledger-backup-20260217-093005.db.zip",
    );
  });

  it("isManagedBackupFileName 识别携带账本标识的产物（前后缀夹取不受标识影响）", () => {
    expect(
      isManagedBackupFileName(
        `${MANUAL_BACKUP_PREFIX}20260217-093005-book-a.db.zip`,
      ),
    ).toBe(true);
    expect(
      isManagedBackupFileName(
        `${AUTO_BACKUP_PREFIX}20260217-093005-3f2a9c4e-8b1d-4c2a-9f3e-5a7b8c9d0e1f.db.zip`,
      ),
    ).toBe(true);
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
