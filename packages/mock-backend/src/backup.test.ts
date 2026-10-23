import { beforeEach, describe, expect, it } from "vitest";
import { installMockBackend } from "./index";
import { invoke } from "./tauri/core";
import type { AutoBackupState, BackupMetaSummary, PruneResult } from "@ledger/types";

/**
 * 备份域替身测试（issue #1966 / spec #1964 / ADR-0143 决策 1）：环境壳不模拟
 * 文件系统备份引擎——创建/恢复答占位回执（路径回显来自 plugin-dialog 假路径）、
 * 元数据答明文手动占位、清单空、修剪零动作；自动备份状态答默认开启从未备份
 * 常量、开关与目录推送 void，不发 ledger:backups-changed，保证设置页备份卡片
 * 可导航不报错。接缝 = tauri 命令面，删除任一登记行即未命中报错、本文件变红。
 */

beforeEach(() => {
  installMockBackend(); // 重装即重播种：每用例从夹具种子起步
});

describe("手动备份与恢复：占位回执", () => {
  it("create_backup 收目标路径答占位结果（路径回显 + 形状齐全）", async () => {
    const result = await invoke<{
      path: string;
      size_bytes: number;
      schema_version: number;
      created_at: string;
    }>("create_backup", { targetPath: "/mock/selected/backup.db" });
    expect(result.path).toBe("/mock/selected/backup.db");
    expect(typeof result.size_bytes).toBe("number");
    expect(typeof result.schema_version).toBe("number");
    expect(typeof result.created_at).toBe("string");
  });

  it("create_backup 缺 targetPath 以含参数名的错误拒绝", async () => {
    await expect(invoke("create_backup", {})).rejects.toThrow(/参数 targetPath/);
  });

  it("restore_backup 收备份路径（口令可空）答占位结果（形状：版本 + 时刻）", async () => {
    const result = await invoke<{ schema_version: number; restored_at: string }>("restore_backup", {
      backupPath: "/mock/selected/backup.db",
      passphrase: null,
    });
    expect(typeof result.schema_version).toBe("number");
    expect(result.restored_at).toMatch(/^\d{4}-\d{2}-\d{2}T/);
  });

  it("get_backup_meta 答明文手动备份占位摘要", async () => {
    await expect(
      invoke<BackupMetaSummary>("get_backup_meta", { path: "/mock/selected/x.db" }),
    ).resolves.toEqual({ kind: "manual", encrypted: false });
  });
});

describe("备份清单与修剪：空占位", () => {
  it("list_backups 收目录答空清单", async () => {
    await expect(invoke("list_backups", { dir: "/mock/backups" })).resolves.toEqual([]);
  });

  it("prune_backups 收目录与保留数答零动作结果", async () => {
    const result = await invoke<PruneResult>("prune_backups", { dir: "/mock/backups", keep: 7 });
    expect(result).toEqual({ kept: 0, deleted: [], failed: [] });
  });
});

describe("自动备份：默认开启、从未备份常量态", () => {
  it("get_auto_backup_state 答默认开启、从未备份、零失败", async () => {
    await expect(invoke<AutoBackupState>("get_auto_backup_state")).resolves.toEqual({
      enabled: true,
      last_backup_at: null,
      consecutive_failures: 0,
      failure_alerting: false,
    });
  });

  it("set_auto_backup_enabled / set_auto_backup_dir 答 void，状态保持常量", async () => {
    await expect(
      invoke<void>("set_auto_backup_enabled", { enabled: false }),
    ).resolves.toBeUndefined();
    await expect(
      invoke<void>("set_auto_backup_dir", { dir: "/mock/backups" }),
    ).resolves.toBeUndefined();
    // 常量态：推送不落内存，回读仍默认开启从未备份
    await expect(invoke<AutoBackupState>("get_auto_backup_state")).resolves.toEqual({
      enabled: true,
      last_backup_at: null,
      consecutive_failures: 0,
      failure_alerting: false,
    });
  });
});
