import { afterAll, describe, expect, it } from "vitest";
import { existsSync, mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { DEEP_MODULE_BOUNDARIES } from "../scripts/check-frontend-structure.ts";
import { cleanupFixtureRepos, fixtureRepo } from "./check-frontend-structure.fixture.ts";
import { gateScript, runGateScript } from "./run-gate-script.test-helper.ts";

// 被测对象是仓库工具脚本 scripts/check-frontend-structure.ts 的规则⑦「深模块边界
// 登记表」（issue #1323 / ADR-0118 决策 7）。与守门脚本测试先例同形制（#1158：测试
// 与所测脚本同目录；#1149 起既有 src/__tests__/check-frontend-structure.test.ts 覆盖
// 规则①—⑥与接线核对，本文件只覆盖新增的规则⑦）。脚本以 Bun 运行时执行（ADR-0083）：
// runGateScript 以 spawnSync('bun') 与门槛调用同款拉起，测的就是门槛路径。按测试
// 决策只测外部可观察结果——进程退出码与输出（ADR-0087 断言强度），不触及脚本内部
// 函数形状；通过位置参数把校验目标指向临时夹具仓库根（[repo-root]
// [packages-manifest.json]），夹具登记表经 arg2 JSON 注入空表隔离规则①—⑤，规则⑦
// 登记表不可注入（生产 DEEP_MODULE_BOUNDARIES 单一事实源，对夹具目录扫描，与规则⑥
// 同形制）。
// 夹具仓库根 builder 住共享辅助 ./check-frontend-structure.fixture.ts（issue #1435：
// 与 src/__tests__ 侧两份手写副本收敛为单份，规则⑥⑦登记项按生产登记表迭代创建，
// 新增登记项夹具零编辑；本文件显式注入空登记表隔离规则①—⑤）。
const script = gateScript("check-frontend-structure.ts");
const run = (args: string[]) => runGateScript(script, args);

afterAll(() => {
  cleanupFixtureRepos();
});

/** 向夹具写入源文件（路径相对夹具根，posix 分隔） */
function writeSource(root: string, rel: string, source: string): void {
  const abs = join(root, ...rel.split("/"));
  mkdirSync(abs.slice(0, abs.lastIndexOf("/")), { recursive: true });
  writeFileSync(abs, source);
}

describe("规则⑦：深模块边界登记表（#1323 / ADR-0118 决策 7）", () => {
  it("删除规则登记项即变红：登记表与已固化边界全等（TransactionFilter → src/views；#1308 起 useInstrumentSearch → src/investment）", () => {
    expect(DEEP_MODULE_BOUNDARIES).toEqual([
      {
        module: "src/transaction/useTransactionFilter.ts",
        allowedConsumers: ["src/views"],
        note: expect.any(String),
      },
      {
        module: "src/investment/useInstrumentSearch.ts",
        allowedConsumers: ["src/investment"],
        note: expect.any(String),
      },
    ]);
  });

  it("夹具绿基线自足：全部登记模块文件在夹具内就位（#1435：登记表迭代创建，登记项增长夹具零编辑）", () => {
    const args = fixtureRepo({ manifest: [] });
    for (const entry of DEEP_MODULE_BOUNDARIES) {
      expect(
        existsSync(join(args[0] as string, ...entry.module.split("/"))),
        `夹具缺登记模块：${entry.module}`,
      ).toBe(true);
    }
  });

  it("白名单内消费绿（src/views 消费面：TransactionsView / ReportsView 形态）", () => {
    const args = fixtureRepo({ manifest: [] });
    writeSource(
      args[0] as string,
      "src/views/TransactionsView.vue",
      "<script setup lang=\"ts\">\nimport { useTransactionFilter } from '@/transaction/useTransactionFilter'\n</script>\n",
    );
    const r = run(args);
    expect(r.status).toBe(0);
  });

  it("白名单外消费即红：报告消费方文件、行号、被消费模块与白名单（失败信息可读）", () => {
    const args = fixtureRepo({ manifest: [] });
    const root = args[0] as string;
    writeSource(
      root,
      "src/components/FilterPanel.vue",
      "<script setup lang=\"ts\">\nimport { useTransactionFilter } from '@/transaction/useTransactionFilter'\n</script>\n",
    );
    const r = run(args);
    expect(r.status).toBe(1);
    expect(r.output).toContain("深模块边界");
    expect(r.output).toContain("src/components/FilterPanel.vue:2");
    expect(r.output).toContain("src/transaction/useTransactionFilter.ts");
    expect(r.output).toContain("src/views");
  });

  it("相对路径形态同样命中（解析落点比对，非仅 @/ 别名文本）", () => {
    const args = fixtureRepo({ manifest: [] });
    writeSource(
      args[0] as string,
      "src/components/Drilldown.vue",
      "import { useTransactionFilter } from '../transaction/useTransactionFilter'\nexport { useTransactionFilter }\n",
    );
    const r = run(args);
    expect(r.status).toBe(1);
    expect(r.output).toContain("深模块边界");
    expect(r.output).toContain("src/components/Drilldown.vue:1");
  });

  it("测试文件消费放行（单测引用被测对象是天然形态，白名单表达生产消费面）", () => {
    const args = fixtureRepo({ manifest: [] });
    writeSource(
      args[0] as string,
      "src/__tests__/useTransactionFilter.test.ts",
      "import { useTransactionFilter } from '@/transaction/useTransactionFilter'\nit('smoke', () => {})\n",
    );
    const r = run(args);
    expect(r.status).toBe(0);
  });

  it("注释中的 import 形态不误报（复用注释掩码机制）", () => {
    const args = fixtureRepo({ manifest: [] });
    writeSource(
      args[0] as string,
      "src/components/Legacy.vue",
      "<script setup lang=\"ts\">\n// import { useTransactionFilter } from '@/transaction/useTransactionFilter'\n</script>\n",
    );
    const r = run(args);
    expect(r.status).toBe(0);
  });

  it("登记模块文件不存在即红（模块改名/删除后拒绝规则静默失效，同规则⑥形制）", () => {
    const r = run(fixtureRepo({ manifest: [], omitModule: true }));
    expect(r.status).toBe(1);
    expect(r.output).toContain("深模块边界");
    expect(r.output).toContain("登记模块不存在");
  });
});
