import { describe, expect, it } from "vitest";
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { mockModeAliases } from "../../vite.config";

const MAIN_TS = readFileSync(join(import.meta.dirname, "../main.ts"), "utf8");

describe("mock mode 安装接线（ADR-0143 决策 4）", () => {
  it("main.ts 按 mode 静态分支安装替身后端：分支、动态 import 与安装调用俱在", () => {
    expect(MAIN_TS).toContain('import.meta.env.MODE === "mock"');
    expect(MAIN_TS).toContain('import("@ledger/mock-backend")');
    expect(MAIN_TS).toContain("installMockBackend()");
  });

  it("安装先于应用创建（替身命令表就绪后才可能发出首批 invoke）", () => {
    const install = MAIN_TS.indexOf("installMockBackend()");
    const mount = MAIN_TS.indexOf("app.mount");
    expect(install).toBeGreaterThan(-1);
    expect(mount).toBeGreaterThan(install);
  });
});

describe("静态消除构造条件：替身不进生产包（ADR-0143 决策 4）", () => {
  it("src/ 生产源码中 @ledger/mock-backend 唯一出现点是 main.ts 的 mode 分支动态 import", () => {
    const offenders: string[] = [];
    const scan = (path: string, code: string): void => {
      if (!code.includes("@ledger/mock-backend")) return;
      for (const line of code.split("\n")) {
        if (!line.includes("@ledger/mock-backend")) continue;
        const isWiredDynamicImport = path.endsWith("main.ts") && /import\(/.test(line);
        if (!isWiredDynamicImport) offenders.push(`${path}: ${line.trim()}`);
      }
    };
    scan("src/main.ts", MAIN_TS);
    const walk = (dir: string): void => {
      for (const entry of readdirSync(dir, { withFileTypes: true })) {
        const full = join(dir, entry.name);
        if (entry.isDirectory()) {
          if (entry.name === "__tests__") continue;
          walk(full);
        } else if (entry.name.endsWith(".ts") || entry.name.endsWith(".vue")) {
          scan(full, readFileSync(full, "utf8"));
        }
      }
    };
    walk(join(import.meta.dirname, ".."));

    expect(offenders).toEqual([]);
  });

  it("vite 别名契约：非 mock mode 恒空表（生产构建不指向替身模块）", () => {
    expect(mockModeAliases("production")).toEqual([]);
    expect(mockModeAliases("development")).toEqual([]);
  });

  it("vite 别名契约：mock mode 恰替换六个 tauri API 面，目标全部落替身包 tauri/", () => {
    const aliases = mockModeAliases("mock");
    expect(aliases.map((a) => a.find)).toEqual([
      "@tauri-apps/api/core",
      "@tauri-apps/api/event",
      "@tauri-apps/api/window",
      "@tauri-apps/api/app",
      "@tauri-apps/plugin-dialog",
      "@tauri-apps/plugin-opener",
    ]);
    expect(aliases.every((a) => a.replacement.includes("packages/mock-backend/src/tauri/"))).toBe(
      true,
    );
  });

  it("vite 配置实际挂载：resolve.alias 展开 mockModeAliases(mode)（删除挂载即红）", () => {
    const configSource = readFileSync(join(import.meta.dirname, "../../vite.config.ts"), "utf8");
    expect(configSource).toContain("...mockModeAliases(mode)");
  });
});
