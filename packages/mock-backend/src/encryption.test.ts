import { beforeEach, describe, expect, it } from "vitest";
import { installMockBackend } from "./index";
import { invoke } from "./tauri/core";

/**
 * 加密与记住主口令域替身测试（issue #1966 / spec #1964 / ADR-0143 决策 1）：
 * 环境壳不模拟真实加密行为——mock 库恒已解锁明文态，全部命令答常量态
 * （「明文库未锁定」/ void / 不支持形态），保证设置页加密卡片与解锁链路
 * 可导航不报错。接缝 = tauri 命令面，删除任一登记行即未命中报错、本文件
 * 变红（接线负向判据，ADR-0087 断言强度）。
 */

beforeEach(() => {
  installMockBackend(); // 重装即重播种：每用例从夹具种子起步
});

describe("加密：mock 库恒已解锁明文态（常量直答）", () => {
  it("get_encryption_status 答明文库未锁定", async () => {
    await expect(invoke("get_encryption_status")).resolves.toEqual({
      locked: false,
      file_encrypted: false,
    });
  });

  it("unlock_encryption 收口令答未搬迁的解锁结果", async () => {
    await expect(invoke("unlock_encryption", { passphrase: "any-pass" })).resolves.toEqual({
      relocated: false,
    });
  });

  it("unlock_encryption 缺 passphrase 以含参数名的错误拒绝", async () => {
    await expect(invoke("unlock_encryption", {})).rejects.toThrow(/参数 passphrase/);
  });

  it("enable / disable / change / 两类重置命令答 void，状态保持明文未锁定", async () => {
    await expect(invoke("enable_encryption", { passphrase: "any-pass" })).resolves.toBeUndefined();
    await expect(invoke("disable_encryption", { passphrase: "any-pass" })).resolves.toBeUndefined();
    await expect(
      invoke("change_encryption_passphrase", { passphrase: "old", newPassphrase: "new" }),
    ).resolves.toBeUndefined();
    await expect(invoke("reset_after_forgotten_passphrase")).resolves.toBeUndefined();
    await expect(invoke("reset_after_startup_failure")).resolves.toBeUndefined();
    // 常量态：转换与重置后加密状态不变（不模拟真实环境行为）
    await expect(invoke("get_encryption_status")).resolves.toEqual({
      locked: false,
      file_encrypted: false,
    });
  });

  it("change_encryption_passphrase 缺 newPassphrase 以含参数名的错误拒绝", async () => {
    await expect(invoke("change_encryption_passphrase", { passphrase: "old" })).rejects.toThrow(
      /参数 newPassphrase/,
    );
  });
});

describe("记住主口令：不支持形态应答", () => {
  it("get_remember_passphrase_support 答不支持（前端据此隐藏选项、回退手输）", async () => {
    await expect(invoke("get_remember_passphrase_support")).resolves.toEqual({
      supported: false,
      mode: "biometry", // 占位：supported=false 时前端只读 supported
    });
  });

  it("set_remember_passphrase 以码化错误拒绝（不支持平台形态）", async () => {
    await expect(
      invoke("set_remember_passphrase", { passphrase: "any-pass" }),
    ).rejects.toMatchObject({ code: "encryption.remember-unsupported" });
  });

  it("clear_remember_passphrase 幂等成功答 void（不支持平台形态）", async () => {
    await expect(invoke("clear_remember_passphrase")).resolves.toBeUndefined();
  });

  it("unlock_with_remembered_passphrase 以无缓存码化错误拒绝（回退手输形态）", async () => {
    await expect(invoke("unlock_with_remembered_passphrase")).rejects.toMatchObject({
      code: "encryption.remember-no-cache",
    });
  });
});
