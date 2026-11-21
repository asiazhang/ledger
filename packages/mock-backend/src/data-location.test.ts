import { beforeEach, describe, expect, it } from "vitest";
import { installMockBackend } from "./index";
import { invoke } from "./tauri/core";
import type { DataLocationInfo } from "@ledger/types";

beforeEach(() => {
  installMockBackend();
});

describe("数据位置：默认目录常量态", () => {
  it("get_data_location_info 答默认目录、无待重启、无回退", async () => {
    const info = await invoke<DataLocationInfo>("get_data_location_info");
    expect(info.active_dir.length).toBeGreaterThan(0);
    expect(info.configured_dir).toBeNull();
    expect(info.pending_restart).toBe(false);
    expect(info.fallback_reason).toBeNull();
  });

  it("submit_data_location_change 答已落盘回执（目标目录回显）", async () => {
    const outcome = await invoke<{
      requires_choice: boolean;
      committed: boolean;
      target_dir: string | null;
    }>("submit_data_location_change", {
      targetDir: "/mock/selected/new-dir",
      adoptExisting: false,
    });
    expect(outcome).toEqual({
      requires_choice: false,
      committed: true,
      target_dir: "/mock/selected/new-dir",
    });
    const info = await invoke<DataLocationInfo>("get_data_location_info");
    expect(info.pending_restart).toBe(false);
  });

  it("submit_data_location_change 缺 targetDir 以含参数名的错误拒绝", async () => {
    await expect(invoke("submit_data_location_change", { adoptExisting: false })).rejects.toThrow(
      /参数 targetDir/,
    );
  });

  it("restore_default_data_location 答已落盘回执（目标为默认目录）", async () => {
    const outcome = await invoke<{
      requires_choice: boolean;
      committed: boolean;
      target_dir: string | null;
    }>("restore_default_data_location", { adoptExisting: false });
    expect(outcome.requires_choice).toBe(false);
    expect(outcome.committed).toBe(true);
    expect(typeof outcome.target_dir).toBe("string");
  });
});
