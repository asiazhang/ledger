#!/usr/bin/env bun
// 东财行情面端点与常量零残留守门（issue #1572 / ADR-0130 守门判据①——源码扫描，
// 删除任一遗漏即红）：
// ADR-0130 决策 1 移除东方财富后，行情面不得再出现任何东财端点引用与常量。
// 本守门以「东财自有域名」为禁令标记做全仓源码扫描：任何东财端点的引用必然
// 携带其自有域名（*.eastmoney.com / dfcfw.com / fundgz.1234567.com.cn），换主机、
// 换路径、换常量名都藏不出这一层；重新引入任一东财通道（哪怕只进测试夹具）即红。
//
// 禁令取「域名」而非裸词 eastmoney/东财：存量来源标记 `eastmoney` 是历史事实
// （ADR-0130 决策 7——种子夹具、类型闭集与断言合法在场），ADR / 词汇表 / 调研
// 档案合法讨论东财（历史叙述），裸词禁令会把这些全打红，语义过宽；域名禁令
// 精确对准「端点回归」这一判据本体。
//
// 已登记例外（严格相等校验，与 check-infra-dml 的 REGISTERED_EXCEPTIONS 同纪律）：
// 实际命中 ≠ 登记数、登记文件消失、例外已收敛（命中清零）都红。当前清单为空：
// 唯一例外（东财 FX 日 K 腿主机池）已随 #1551 换 ECB 退役并删除登记，守门收紧到
// 零例外（同 check-infra-dml 自 #1108 起的空清单稳态）；今后若出现新的合法在场，
// 须在此逐条登记（文件 + 预期命中数 + 理由）。
//
// 扫描边界：全仓文本面（rs/ts/tsx/vue/md/json/sql/sh/html/toml/yml/yaml）。
// docs/ 整域豁免——ADR、调研档案与词汇表的历史叙述（含端点 URL 引证）是其职责
// 本体；CHANGELOG.md 豁免（历史条目）；守门脚本与其包装测试豁免（禁令字面量以
// 数据形态住在其中）。豁免全部 fail loud：登记文件消失即红（清单漂移），不放任
// 守门空转。已知文本不可达逃逸形态（靠评审兜底）：经变量拼接或编码（URL encode、
// IDN）的域名、不携带东财域名的代理中转形态，以及不携带域名的纯常量形态（如
// secid 数字前缀映射、报文字段布局常量——单点回归无域名可扫，评审时按
// ADR-0130 决策 2「东财 secid 前缀映射退役」人工判认）。
//
// TypeScript 化 + Bun 运行时（issue #734 / ADR-0083）：类型经 tsconfig.scripts.json
// 门槛检查；调用方式 `bun scripts/check-eastmoney-residue.ts`。
// 默认校验本仓库；测试可传位置参数指向夹具仓库根：bun scripts/check-eastmoney-residue.ts [root]
// 包装测试 scripts/check-eastmoney-residue.test.ts。
// 行号定位与目录遍历消费 gate-primitives.ts（#1680 库归库、门归门）导出的家族共享原语
// （lineAt / walkTextFiles，issue #1625）；禁令标记与豁免面属本守门政策，自持。

import { existsSync, readFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { lineAt, walkTextFiles } from "./gate-primitives.ts";

const DEFAULT_ROOT = join(fileURLToPath(import.meta.url), "..", "..");

/** 守门脚本本体（相对仓库根）：豁免自证对象，缺失即清单漂移红 */
const SELF_REL = "scripts/check-eastmoney-residue.ts";
/** 包装测试（相对仓库根）：同上，禁令字面量的另一处数据载体 */
const WRAPPER_TEST_REL = "scripts/check-eastmoney-residue.test.ts";

/** 扫描的文本面扩展名：源码、契约、脚本与文档化文本；二进制资产不在判据面 */
const SCAN_EXTENSIONS = new Set([
  ".rs",
  ".ts",
  ".tsx",
  ".vue",
  ".md",
  ".json",
  ".sql",
  ".sh",
  ".html",
  ".toml",
  ".yml",
  ".yaml",
  ".feature",
]);

/** 整目录豁免：依赖与产物（非源码面）、docs/（历史叙述职责本体，见文件头） */
const SKIP_DIRS = new Set(["node_modules", "target", ".git", "docs", "dist", "coverage"]);

/** 整文件豁免（相对仓库根）：历史记录与禁令字面量的数据载体，逐条登记于 SELF/CHANGELOG */
const SKIP_FILES = new Set(["CHANGELOG.md", SELF_REL, WRAPPER_TEST_REL]);

function fail(message: string): never {
  console.error(`✗ 东财行情面零残留守门：${message}`);
  process.exit(1);
}

/** 单条残留命中：仓库相对路径、行号（1 起算）、命中的禁令标记、原文行 */
export interface ResidueHit {
  file: string;
  line: number;
  marker: string;
  text: string;
}

/** 已登记例外条目：东财端点域名的合法在场（当前仅 FX 腿），逐条附理由 */
export interface ResidueException {
  file: string;
  count: number;
  why: string;
}

/**
 * 已登记例外（严格相等校验）：实际命中 ≠ 登记数即红（漂移——多出未登记新命中、
 * 少掉说明对应腿已退役），例外清零也红（登记条目应随退役删除），登记文件消失
 * 同样红（清单漂移，与 EXEMPT_FILES 家族纪律一致）。当前为空：东财 FX 日 K 腿
 * 主机池已随 #1551 换 ECB 退役，登记条目随之删除，守门收紧到零例外。
 */
export const REGISTERED_EXCEPTIONS: readonly ResidueException[] = [];

/** 禁令标记：东财自有域名闭集。任何东财端点的引用（主机池、URL、Referer、
 *  文档化常量）必然携带其一；新增东财关联域名须显式扩此清单并复核动机。 */
export const BANNED_MARKERS: readonly string[] = ["eastmoney.com", "dfcfw", "1234567.com.cn"];

/** 转义为字面量正则（域名含点），大小写不敏感（主机常量可能全大写） */
function markerPattern(marker: string): RegExp {
  return new RegExp(marker.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"), "gi");
}

/** 扫描单个文本：返回全部禁令标记命中（行号 1 起算，原文行去首尾空白） */
export function scanResidue(rel: string, source: string): ResidueHit[] {
  const hits: ResidueHit[] = [];
  const lines = source.split("\n");
  for (const marker of BANNED_MARKERS) {
    for (const m of source.matchAll(markerPattern(marker))) {
      const line = lineAt(source, m.index);
      hits.push({ file: rel, line, marker, text: (lines[line - 1] ?? "").trim() });
    }
  }
  return hits.sort((a, b) => a.line - b.line || a.marker.localeCompare(b.marker));
}

function main(): void {
  const root = process.argv[2] ? resolve(process.argv[2]) : DEFAULT_ROOT;
  if (!existsSync(root)) fail(`扫描根不存在：${root}`);

  // 豁免自证先行：脚本本体与包装测试必须在场（清单漂移 fail loud，不放任守门空转）
  for (const rel of [SELF_REL, WRAPPER_TEST_REL]) {
    if (!existsSync(join(root, rel))) {
      fail(`豁免清单漂移：${rel} 不存在——守门或其包装测试被改名/搬迁，请同步 SKIP_FILES 登记`);
    }
  }

  // 单次收集全仓文件（家族共享单点 walkTextFiles，abs 读文件 / rel 报文定位），逐文件扫描
  //（读文件与扫描同轮完成，不做二次遍历）
  const scanned = walkTextFiles(root, "", {
    extensions: SCAN_EXTENSIONS,
    skipDirs: SKIP_DIRS,
    skipFiles: SKIP_FILES,
  });
  const hits = scanned.flatMap((f) => scanResidue(f.rel, readFileSync(f.abs, "utf8")));
  const byFile = new Map<string, ResidueHit[]>();
  for (const hit of hits) {
    const list = byFile.get(hit.file) ?? [];
    list.push(hit);
    byFile.set(hit.file, list);
  }

  const problems: string[] = [];
  // 未登记命中：逐条定位 文件:行 + 标记 + 原文行
  const registered = new Map(REGISTERED_EXCEPTIONS.map((e) => [e.file, e]));
  for (const [file, list] of byFile) {
    if (registered.has(file)) continue;
    for (const hit of list) {
      problems.push(
        `✗ ${file}:${hit.line} 命中禁令标记「${hit.marker}」：${hit.text}\n` +
          `    —— 东财行情面端点已随 ADR-0130 决策 1 删除，不得回归（守门判据①）`,
      );
    }
  }
  // 例外三向校验：文件消失（清单漂移）、命中清零（例外已收敛）、计数漂移
  for (const e of REGISTERED_EXCEPTIONS) {
    const actual = byFile.get(e.file);
    if (!actual && !existsSync(join(root, e.file))) {
      problems.push(
        `✗ 例外清单漂移：${e.file} 不存在——${e.why}，请复核后更新 REGISTERED_EXCEPTIONS`,
      );
      continue;
    }
    const count = actual?.length ?? 0;
    if (count === 0) {
      problems.push(
        `✗ 例外已收敛：${e.file} 零命中——${e.why}；删除 REGISTERED_EXCEPTIONS 登记条目，` +
          "守门收紧到零例外",
      );
    } else if (count !== e.count) {
      problems.push(
        `✗ 例外计数漂移：${e.file} 实际命中 ${count} 处 ≠ 登记 ${e.count} 处——` +
          `${e.why}；逐条复核 ${fileHitsLabel(actual)} 后同步登记`,
      );
    }
  }

  if (problems.length > 0) {
    console.error(`✗ 东财行情面零残留守门：${problems.length} 处问题\n${problems.join("\n")}`);
    process.exit(1);
  }
  console.log(
    `✓ 东财行情面零残留守门通过：扫描 ${scanned.length} 个文件，未登记端点域名零命中；` +
      `已登记例外 ${REGISTERED_EXCEPTIONS.length} 条` +
      `${REGISTERED_EXCEPTIONS.length > 0 ? `（${REGISTERED_EXCEPTIONS.map((e) => e.file).join("、")}）` : "（零例外）"}`,
  );
}

function fileHitsLabel(hits?: ResidueHit[]): string {
  return (hits ?? []).map((h) => `${h.file}:${h.line}`).join("、");
}

// 仅直接运行时执行 main；包装测试以子进程调用（行为等价判据：只测外部可观察结果）。
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main();
}
