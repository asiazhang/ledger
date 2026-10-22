import { beforeEach, describe, expect, it } from "vitest";
import { installMockBackend } from "./index";
import { invoke } from "./tauri/core";

/**
 * 应用更新域替身测试（issue #1966 / spec #1964 / ADR-0143 决策 1）：环境壳不
 * 模拟更新源——手动检查恒答「已是最新」（same）、更新入口形态答桌面完整更新
 * 面、下载答占位版本回执、安装 void，不发任何进度事件，保证设置页更新卡片
 * 可导航不报错。接缝 = tauri 命令面，删除任一登记行即未命中报错、本文件变红。
 */

beforeEach(() => {
  installMockBackend(); // 重装即重播种：每用例从夹具种子起步
});

describe("应用更新：已是最新常量态", () => {
  it("check_for_updates 恒答 same（无新版本，不携版本号与说明）", async () => {
    await expect(invoke("check_for_updates")).resolves.toEqual({
      status: "same",
      version: null,
      notes: null,
    });
  });

  it("get_update_surface 答 updater（桌面完整更新面：检查/下载/安装）", async () => {
    await expect(invoke("get_update_surface")).resolves.toBe("updater");
  });

  it("download_update 答占位版本回执（形状：version）", async () => {
    const outcome = await invoke<{ version: string }>("download_update");
    expect(outcome.version).toMatch(/^\d+\.\d+\.\d+/);
  });

  it("install_update 答 void", async () => {
    await expect(invoke<void>("install_update")).resolves.toBeUndefined();
  });
});
