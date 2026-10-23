import { beforeEach, describe, expect, it } from "vitest";
import { installMockBackend } from "./index";
import { invoke } from "./tauri/core";

/**
 * 设置域替身行为测试（issue #1965 / spec #1964 / ADR-0143）：接缝 = 应用所见
 * 的 tauri 命令面（`installMockBackend` + `invoke`），断言布线与行为一体——
 * 删除任一登记行即未命中报错、本文件变红（接线负向判据，ADR-0087 断言强度）。
 *
 * 域取向（spec #1964）：本位币 / 日志等级为内存态（缺省 CNY / info，写入改
 * 内存、读投影内存、重装重播种）；构建信息与 AI 提示答字面量；开关推送与壳
 * 命令 no-op 应答——保证启动链路不弹错、设置页「通用」「关于」可导航。
 */

beforeEach(() => {
  installMockBackend(); // 重装即重播种：每用例从夹具种子起步
});

describe("启动链路开关推送：no-op 应答 void", () => {
  it("set_auto_execution_enabled / set_auto_update_enabled 收布尔推送应答 void", async () => {
    await expect(
      invoke<void>("set_auto_execution_enabled", { enabled: true }),
    ).resolves.toBeUndefined();
    await expect(
      invoke<void>("set_auto_execution_enabled", { enabled: false }),
    ).resolves.toBeUndefined();
    await expect(
      invoke<void>("set_auto_update_enabled", { enabled: true }),
    ).resolves.toBeUndefined();
  });

  it("缺 enabled 参数以含参数名的错误拒绝（参数窄化，与真后端必需参数同语义）", async () => {
    await expect(invoke<void>("set_auto_execution_enabled", {})).rejects.toThrow(/参数 enabled/);
  });
});

describe("本位币基准：内存态，缺省 CNY", () => {
  it("get_base_currency 缺省回 { code: 'CNY' }", async () => {
    await expect(invoke("get_base_currency")).resolves.toEqual({ code: "CNY" });
  });

  it("set_base_currency 写内存并回显新态，get 反映", async () => {
    await expect(invoke("set_base_currency", { code: "USD" })).resolves.toEqual({
      code: "USD",
    });
    await expect(invoke("get_base_currency")).resolves.toEqual({ code: "USD" });
  });

  it("币种字典外的 code 报错（存在性校验，spec #1964 取向）", async () => {
    await expect(invoke("set_base_currency", { code: "XYZ" })).rejects.toThrow(/XYZ/);
  });
});

describe("日志等级：内存态，缺省 info", () => {
  it("get_log_level 缺省回 { level: 'info' }", async () => {
    await expect(invoke("get_log_level")).resolves.toEqual({ level: "info" });
  });

  it("set_log_level 写内存，get 反映", async () => {
    await expect(invoke<void>("set_log_level", { level: "debug" })).resolves.toBeUndefined();
    await expect(invoke("get_log_level")).resolves.toEqual({ level: "debug" });
  });

  it("缺 level 参数以含参数名的错误拒绝", async () => {
    await expect(invoke<void>("set_log_level", {})).rejects.toThrow(/参数 level/);
  });
});

describe("构建信息与 AI 提示：字面量应答", () => {
  it("app_build_info 答版本/平台/架构字面量（前端显示名映射可消费）", async () => {
    const info = await invoke<{ version: string; platform: string; arch: string }>(
      "app_build_info",
    );
    expect(info.version).toMatch(/^\d+\.\d+\.\d+/);
    expect(["macos", "windows", "linux", "android", "ios"]).toContain(info.platform);
    expect(info.arch.length).toBeGreaterThan(0);
  });

  it("get_ai_prompt 答非空字面量字符串", async () => {
    const prompt = await invoke<string>("get_ai_prompt");
    expect(prompt.length).toBeGreaterThan(0);
    expect(prompt).toContain("替身");
  });
});

describe("壳命令：no-op 应答 void（环境壳不模拟，spec #1964）", () => {
  it("open_log_dir / restart_app 无参应答 void", async () => {
    await expect(invoke<void>("open_log_dir")).resolves.toBeUndefined();
    await expect(invoke<void>("restart_app")).resolves.toBeUndefined();
  });

  it("log_frontend_error 收 message / reveal_in_file_manager 收 path 应答 void", async () => {
    await expect(
      invoke<void>("log_frontend_error", { message: "渲染层异常" }),
    ).resolves.toBeUndefined();
    await expect(
      invoke<void>("reveal_in_file_manager", { path: "/mock/selected/file" }),
    ).resolves.toBeUndefined();
  });
});

describe("设置域重装重播种：写入丢弃、回夹具种子", () => {
  it("写入本位币与日志等级后重装，两者回到缺省", async () => {
    await invoke<void>("set_base_currency", { code: "USD" });
    await invoke<void>("set_log_level", { level: "trace" });

    installMockBackend();

    await expect(invoke("get_base_currency")).resolves.toEqual({ code: "CNY" });
    await expect(invoke("get_log_level")).resolves.toEqual({ level: "info" });
  });
});
