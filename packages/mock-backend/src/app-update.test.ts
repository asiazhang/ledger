import { beforeEach, describe, expect, it } from "vitest";
import { installMockBackend } from "./index";
import { invoke } from "./tauri/core";

beforeEach(() => {
  installMockBackend();
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
