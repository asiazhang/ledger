import { describe, expect, it } from "vitest";
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";

/**
 * mock 后端安装接线守门（原「浏览器开发模式接线守门」，issue #1962 / ADR-0143
 * 决策 4/5；#2083 切轨后 mock 是 Web 参照前端唯一后端，安装从 mode 分支改为
 * 无条件装配）：
 *
 * 接线型验收判据按仓库纪律以源码扫描守门（先例 #959 / #961）：删除 main.ts 的
 * 安装调用 → 至少一条断言变红；断言对准用户可观察结果的最接近守门形态——
 * 「浏览器里应用启动即瘫」由 main.ts 安装接线的存在性与安装先于挂载钉住。
 */

const MAIN_TS = readFileSync(join(import.meta.dirname, "../main.ts"), "utf8");

describe("mock 后端安装接线（ADR-0143 决策 4；#2083 起无条件装配）", () => {
  it("main.ts 安装 mock 后端：静态 import 与安装调用俱在", () => {
    expect(MAIN_TS).toContain('from "@ledger/mock-backend"');
    expect(MAIN_TS).toContain("installMockBackend()");
  });

  it("安装先于应用创建（替身命令表就绪后才可能发出首批 invoke）", () => {
    const install = MAIN_TS.indexOf("installMockBackend()");
    const mount = MAIN_TS.indexOf("app.mount");
    expect(install).toBeGreaterThan(-1);
    expect(mount).toBeGreaterThan(install);
  });

  it("src/ 生产源码经 @ledger/mock-backend 的引用全部落 bridge 子路径（exports 深导入面）", () => {
    // 全包入口（"."）唯一合法消费点是 main.ts 的安装面；其余生产源码只经
    // bridge/* 深导入消费传输面（命令 / 事件 / 窗口 / 对话框等），不许从包
    // 根入口拉替身后端本体。
    const offenders: string[] = [];
    const scan = (path: string, code: string): void => {
      const walkLine = (line: string): void => {
        if (!line.includes("@ledger/mock-backend")) return;
        const isBridgeImport = /@ledger\/mock-backend\/bridge\//.test(line);
        const isInstallFace = path.endsWith("main.ts") && /"@ledger\/mock-backend"/.test(line);
        if (!isBridgeImport && !isInstallFace) offenders.push(`${path}: ${line.trim()}`);
      };
      for (const line of code.split("\n")) walkLine(line);
    };
    scan("src/main.ts", MAIN_TS);
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
});
