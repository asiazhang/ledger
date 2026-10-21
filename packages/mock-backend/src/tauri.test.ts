import { describe, expect, it, vi } from "vitest";
import { onBackButtonPress } from "./tauri/app";
import { invoke, registerMockCommand } from "./tauri/core";
import { open, save } from "./tauri/plugin-dialog";
import { openUrl } from "./tauri/plugin-opener";
import { getCurrentWindow } from "./tauri/window";

/**
 * tauri API 面模块替身的行为测试（issue #1962 / ADR-0143 决策 5）：
 * 消费面形状（返回值形态、可撤销句柄、假路径非空）逐一对准生产调用点，
 * 「对话框替身返回假路径、外链打开空操作」的决策边界在此钉住。
 */

describe("invoke 路由（tauri/core 替身）", () => {
  it("已登记命令转发实现并透传参数对象；返回非 promise 值包装为 resolved", async () => {
    const handler = vi.fn((args: Record<string, unknown>) => args.id);
    registerMockCommand("probe_cmd", handler);

    await expect(invoke("probe_cmd", { id: "x1" })).resolves.toBe("x1");
    expect(handler).toHaveBeenCalledWith({ id: "x1" });
  });

  it("重复登记同名命令按覆盖计（重装语义）", async () => {
    registerMockCommand("probe_overwrite", () => 1);
    registerMockCommand("probe_overwrite", () => 2);
    await expect(invoke("probe_overwrite")).resolves.toBe(2);
  });
});

describe("文件对话框替身（plugin-dialog）：返回确定性假路径", () => {
  it("open 返回非 null 假路径；带过滤器时推导扩展名", async () => {
    await expect(open()).resolves.toBe("/mock/selected/file");

    const csv = await open({
      filters: [{ name: "CSV", extensions: ["csv"] }],
    });
    expect(csv).toBe("/mock/selected/file.csv");
  });

  it("multiple 场景返回单元素数组（与真插件契约同构）", async () => {
    const paths = await open({ multiple: true, directory: false });
    expect(paths).toEqual([expect.stringContaining("/mock/selected/")]);
  });

  it("save 返回非 null 假路径，defaultPath 文件名保留", async () => {
    const path = await save({ defaultPath: "ledger-backup.zip" });
    expect(path).toBe("/mock/selected/ledger-backup.zip");
  });
});

describe("外链打开替身（plugin-opener）：空操作", () => {
  it("openUrl resolve 为空操作并留 console 痕迹", async () => {
    const infoSpy = vi.spyOn(console, "info").mockImplementation(() => {});
    await expect(openUrl("https://example.com")).resolves.toBeUndefined();
    expect(infoSpy).toHaveBeenCalledWith(expect.stringContaining("https://example.com"));
    infoSpy.mockRestore();
  });
});

describe("窗口与应用替身（window / app）：no-op promise 应答", () => {
  it("getCurrentWindow 的 setTitle / destroy 可调用且 resolve", async () => {
    const win = getCurrentWindow();
    await expect(win.setTitle("开源记账")).resolves.toBeUndefined();
    await expect(win.destroy()).resolves.toBeUndefined();
  });

  it("onBackButtonPress 注册即成功，unregister 撤销", async () => {
    const handler = vi.fn();
    const listener = await onBackButtonPress(handler);
    await expect(listener.unregister()).resolves.toBeUndefined();
  });
});
