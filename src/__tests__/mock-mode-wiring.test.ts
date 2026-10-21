import { describe, expect, it } from "vitest";
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { mockModeAliases } from "../../vite.config";

/**
 * 浏览器开发模式接线守门（issue #1962 / ADR-0143 决策 4/5，验收判据的负向条目）：
 *
 * mock mode 分支在本机 CI 不构建、不运行（测试三层权威不变，自动化测试不走替身
 * 后端），接线型验收判据按仓库纪律以源码扫描守门替代（先例 #959 / #961）：
 * 删除 main.ts 的替身安装分支调用 → 至少一条断言变红；删除 vite mock 别名表 →
 * 契约断言变红。断言对准用户可观察结果的最接近守门形态——「mock mode 下应用
 * 启动即瘫」由 main.ts 安装接线的存在性钉住，「替身不进生产包」由静态消除的
 * 两道构造条件钉住（唯一消费点是恒被消除的分支 + 非 mock mode 别名恒空）。
 */

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
    // 逐文件收集命中行；静态 import（无括号 import 形态）在任何文件出现即红——
    // 静态 import 无条件进模块图，破坏「构建期消除」的前提。
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
    // 其余生产源码：App.vue 与 composables/views/stores 全树（文本级扫描，
    // 与 no-native-confirm 守门同口径；新增静态引用即红）
    const walk = (dir: string): void => {
      for (const entry of readdirSync(dir, { withFileTypes: true })) {
        const full = join(dir, entry.name);
        if (entry.isDirectory()) {
          if (entry.name === "__tests__") continue; // 测试文件合法讨论替身
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
    // 纯函数行为测试不覆盖「配置是否真把别名表接进 resolve.alias」——挂载点删除
    // 后 mock mode 下 tauri 面不再被替身，应用启动即瓕而纯函数断言仍绿；此处
    // 钉住挂载接线本体（配置回调内的展开式，与 main.ts 安装分支同级的接线点）。
    const configSource = readFileSync(join(import.meta.dirname, "../../vite.config.ts"), "utf8");
    expect(configSource).toContain("...mockModeAliases(mode)");
  });
});
