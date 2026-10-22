import { beforeEach, describe, expect, it } from "vitest";
import { installMockBackend } from "./index";
import { invoke } from "./tauri/core";
import type { DataLocationInfo } from "@ledger/types";

/**
 * 数据位置域替身测试（issue #1966 / spec #1964 / ADR-0143 决策 1）：环境壳不
 * 模拟指针文件与启动引导搬迁——信息恒答「默认目录、无待重启、无回退」，更改
 * 意图与恢复默认答「已落盘」常量回执（目标目录回显），真实搬迁（下次启动
 * 引导）不模拟，保证设置页数据位置卡片可导航不报错。接缝 = tauri 命令面，
 * 删除任一登记行即未命中报错、本文件变红。
 */

beforeEach(() => {
  installMockBackend(); // 重装即重播种：每用例从夹具种子起步
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
    // 常量态：提交后信息保持默认目录无挂起（搬迁不模拟）
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
