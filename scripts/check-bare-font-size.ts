#!/usr/bin/env bun
// 壳根包裸字号直调零残留守门（issue #2108 / spec #2099 contract 票 / ADR-0147）：
// 视图零裸值纪律的源码扫描（DESIGN.md §7），删除扫描规则即存量违规不再被拦。
// ADR-0147 决策 4：代码取值唯一来源是设计常量模块（src-app/src/design.rs 字阶）
// ——视图字号只允许走字阶常量 `text_size(px(font_size::<档>))` 或框架 typed 预设
// （`text_xs()` 等）；`text_size(px(<数字字面量>))` 直调即裸值回归，一出现即红。
// 判据本体取「px() 参数以数字开头（含小数点开头的 `.5` 形态）」而非裸词 text_size：
// 收口后的合规形态（572 处 `px(font_size::…)`）与 typed 预设全部携带常量引用不命中，
// 裸字面量（换数值、拆行、提局部变量都不改「数字开头」这一层；`px(0x…)` 十六进制
// 以 `0` 开头同样命中）藏不出判据面。
//
// 扫描面 = `src-app/src`（壳根包 lib 本体，视图书写面；ADR-0147 病灶闭集——
// 批次票收口前的 643 处裸调全部落在此面）。Rust 词法掩码（maskNonCode，抹注释
// 与字符串字面量）：文档注释里的历史引证（design.rs 头注、研究档案）、字符串
// 节点里的样例文本是合法在场不计数。
//
// 豁免面裁决（按 DESIGN.md 规约裁量，#2108 交付时点全部零命中）：
// - 扫描面 = `src-app/src`，其中 `#[cfg(test)]` 单元测试块不豁免——纪律辖「代码
//   取值唯一来源是常量模块」，夹具写裸字号同样制造第二套数；探针断言形态
//   （span_style(...).font_size 对照 design 常量）本就不需要裸值直调。
// - `src-app/tests/` 目录不在扫描面（位置参数指向夹具根是测试自证的临时替身）：
//   headless 集成测试不在本门辖内，探针断言的裸值另受「常量派生」纪律
//   （DESIGN.md §8：字号断言由 design::font_size 常量派生，色值快照除外）。
// - 设计常量模块 design.rs 自身不豁免——其代码零裸值；注释引证被词法掩码吸收；
//   常量模块若写裸值直调同样是违规（取值唯一来源纪律对它同样成立）。
// - 域 crate（src-app/crates/）不在扫描面：域不依赖壳，无字号 API 可调
//   （check-structure 分层门辖方向）。
//
// 已登记例外（严格相等校验，与 check-eastmoney-residue / check-infra-dml 同纪律）：
// 实际命中 ≠ 登记数、登记文件消失、例外已收敛（命中清零）都红。当前清单为空
// （零例外稳态：批次票 #2101–#2107 已把 643 处存量清零，#2108 交付时 rg 自证
// src-app 全树数字字面量形态零命中）；今后若出现合法在场，须在 REGISTERED_EXCEPTIONS
// 逐条登记（文件 + 预期命中数 + 理由）并在 docs/DESIGN.md §7 例外挂账处成文。
//
// 已知文本不可达逃逸形态（靠评审兜底）：以标识符承载裸值的中转变量
// （`let v = 12.0; text_size(px(v))`——值仍散写，只是绕开判据面）、框架未来新增
// 的裸值入口 API。链式换行（`text_size(\n px(12.0))`）可达，已覆盖。
//
// TypeScript 化 + Bun 运行时（issue #734 / ADR-0083）：类型经 tsconfig.scripts.json
// 门槛检查；调用方式 `bun scripts/check-bare-font-size.ts`。
// 默认扫描本仓库 src-app/src；测试可传位置参数指向夹具扫描根：
// bun scripts/check-bare-font-size.ts [root]
// 包装测试 scripts/check-bare-font-size.test.ts。
// 行号定位与目录遍历消费 gate-primitives.ts（#1680 库归库、门归门）导出的家族
// 共享原语（lineAt / maskNonCode / walkTextFiles，issue #1625）；判据与豁免面属
// 本守门政策，自持。

import { existsSync, readFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { lineAt, maskNonCode, walkTextFiles } from "./gate-primitives.ts";

const DEFAULT_ROOT = join(fileURLToPath(import.meta.url), "..", "..", "src-app", "src");

/** 扫描面扩展名闭集：壳根包 Rust 源（RUST_EXTENSIONS 单文件面收缩，政策自持） */
const SCAN_EXTENSIONS: ReadonlySet<string> = new Set([".rs"]);

/** 判据形态：`text_size( px( <数字开头` ——px() 参数以数字字面量开头即裸值直调；
 *  `.?` 补齐小数点开头形态（`px(.5)`——无整数位小数，仅 `\d` 会漏）；`\b` 挡住
 *  `my_text_size` 类标识符前缀；空白容忍（`\s*`）覆盖链式换行书写；`px(0x…)`
 *  十六进制以 `0` 开头本就命中。
 *  已知不可达逃逸形态见文件头（评审兜底）。 */
const BARE_FONT_SIZE_RE = /\btext_size\s*\(\s*px\s*\(\s*\.?\d/g;

/** 单条命中：扫描根相对路径、行号（1 起算）、原文行 */
export interface BareFontSizeHit {
  file: string;
  line: number;
  text: string;
}

/** 已登记例外条目：裸字号直调的合法在场（当前零例外稳态），逐条附理由 */
export interface BareFontSizeException {
  file: string;
  count: number;
  why: string;
}

/**
 * 已登记例外（严格相等校验）：实际命中 ≠ 登记数即红（漂移——多出未登记新命中、
 * 少掉说明已收口），例外清零也红（登记条目应随收口删除），登记文件消失同样红
 * （清单漂移）。当前为空：#2108 交付时点存量已随批次票 #2101–#2107 全量收口，
 * 守门收紧到零例外（同 check-eastmoney-residue / check-infra-dml 稳态）。
 */
export const REGISTERED_EXCEPTIONS: readonly BareFontSizeException[] = [];

function fail(message: string): never {
  console.error(`✗ 壳根包裸字号直调零残留守门：${message}`);
  process.exit(1);
}

/** 扫描单个 Rust 源：返回全部裸字号直调命中（行号 1 起算，原文行去首尾空白；
 *  词法掩码先行——注释与字符串字面量里的形态不计数） */
export function scanSource(rel: string, source: string): BareFontSizeHit[] {
  const masked = maskNonCode(source);
  const lines = source.split("\n");
  const hits: BareFontSizeHit[] = [];
  for (const m of masked.matchAll(BARE_FONT_SIZE_RE)) {
    const line = lineAt(source, m.index);
    hits.push({ file: rel, line, text: (lines[line - 1] ?? "").trim() });
  }
  return hits.sort((a, b) => a.line - b.line);
}

function main(): void {
  const root = process.argv[2] ? resolve(process.argv[2]) : DEFAULT_ROOT;
  if (!existsSync(root)) fail(`扫描根不存在：${root}`);

  const scanned = walkTextFiles(root, "", { extensions: SCAN_EXTENSIONS });
  if (scanned.length === 0) {
    fail(`扫描面为空：${root} 下无 .rs 文件——扫描根漂移即守门空转，拒绝静默绿`);
  }

  const hits = scanned.flatMap((f) => scanSource(f.rel, readFileSync(f.abs, "utf8")));
  const byFile = new Map<string, BareFontSizeHit[]>();
  for (const hit of hits) {
    const list = byFile.get(hit.file) ?? [];
    list.push(hit);
    byFile.set(hit.file, list);
  }

  const problems: string[] = [];
  // 未登记命中：逐条定位 文件:行 + 原文行 + 收口指引
  const registered = new Map(REGISTERED_EXCEPTIONS.map((e) => [e.file, e]));
  for (const [file, list] of byFile) {
    if (registered.has(file)) continue;
    for (const hit of list) {
      problems.push(
        `✗ ${file}:${hit.line} 裸字号直调「${hit.text}」\n` +
          `    —— 字号取值唯一来源是 design::font_size 字阶（ADR-0147 决策 4）：` +
          `改走 text_size(px(font_size::<档>)) 或 typed 预设（DESIGN.md §7 纪律）`,
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
          `${e.why}；逐条复核后同步登记`,
      );
    }
  }

  if (problems.length > 0) {
    console.error(
      `✗ 壳根包裸字号直调零残留守门：${problems.length} 处问题\n${problems.join("\n")}`,
    );
    process.exit(1);
  }
  console.log(
    `✓ 壳根包裸字号直调零残留守门通过：扫描 ${scanned.length} 个文件，` +
      `裸字号直调零命中；已登记例外 ${REGISTERED_EXCEPTIONS.length} 条` +
      `${REGISTERED_EXCEPTIONS.length > 0 ? `（${REGISTERED_EXCEPTIONS.map((e) => e.file).join("、")}）` : "（零例外）"}`,
  );
}

// 仅直接运行时执行 main；包装测试以子进程调用（行为等价判据：只测外部可观察结果）。
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main();
}
