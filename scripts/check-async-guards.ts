#!/usr/bin/env bun
// 前端异步守门（issue #1039，#1008 决议 4 follow-up）：两条源码扫描门。
//
// 背景：手搓异步守卫在 ADR-0040 当点（2026-08-30）全仓仅搜索视图一处，架构走查
// （2026-09-11）已增殖到三处——「新代码一律走 Loadable」纯纪律守不住。#1008 把存量
// 六处收编进 useLoadable 后，手搓竞态序号已在源码面归零；本门把约束固化成可执行
// 检查（仿 scripts/check-structure.ts 思路，Bun 运行时 ADR-0083）。
//
// 规则 1（硬零容忍）：手搓竞态纪元 `let <名>(seq|epoch|generation) = 0` 形态即红。
// 竞态纪元唯一合法住址共享 module @ledger/latest-wins 本体（#1678：机制实体化成包，
// 五处消费点收敛；#1008 收编后的旧住址 useLoadable 序号改经 module 消费、不再自持，
// 豁免随迁）。规则对准概念（手搓纪元）而不是某个名字：seq / epoch / generation 三名
// 别名一并识别（#1678 收敛前的三名），裸名与任意前缀（`\w*`）同面——与原生事务
// 语句禁令的「唯一合法住址」同款纪律（check-structure / #1014）。命名尾部通配
//（seqNum / epochAt 等）、非 let 形态（const 字段、ref(0) 计数、函数参数）与未来
// 新别名不在检测面，靠评审兜底——已知盲区显式声明于此，不设注释豁免通道。住址
// 文件不可达即红（搬迁未同步 SEQ_SEAM_FILE 时拒绝白名单静默失效，同结构守门
// 规则⑥⑦形制）。
//
// 规则 2（基线冻结、只减不增）：catch 内直弹 toast 模板
// `message.error(t(…errorMessage…))` 不能零容忍——#1039 交付时实测 61 处 / 31 文件
// （#1008 交付分支票面记 60 / 31，以本票实现时重测为准）。匹配窗口允许跨行且限定
// 最大跨度（TOAST_WINDOW_LINES，#1467）：message.error( 与 t( 之间容忍折行，t( 之后
// errorMessage 须落在窗口行数内——折行/格式化不把违规藏出检测面；#1467 扩面使现役
// 投资表单保存失败路径（src/investment/useInvestmentForm.ts 多行形态）显形，按基线
// 纪律入册（见 TOAST_BASELINE 内注）。TOAST_BASELINE 按文件冻结存量计数，现计数与
// 基线全等：新增/回潮（>）红；收缩未同步（<）同样红并提示下调基线、附多行化/格式
// 漂移排查提示（不可见 ≠ 已收编，#1467）——与样式块白名单「迁移一个删一个」同一
// 纪律（基线即规格，不留陈目）。存量收编本身（连带 i18n 键与插值名清理）另立专项
// （#1039 Out of Scope），本门只封增量。
//
// 扫描边界：src 应用壳 + 全部 packages 子包 src 树（清单 SCAN_ROOTS，#1467 起由
// 磁盘自证保证「有 src 的子包必须登记」——子包源码目录落盘而未登记即红，堵新包
// 静默逃逸扫描）下 .ts/.vue 文本级扫描（含 __tests__，当前零命中）。行首注释行
// （// /* * <!--）置空后再匹配——注释提及靶形态不误报；跨行调用在窗口行数内可识别
// （#1467 扩面），窗口外、三元条件夹层（message.error( 与 t( 之间隔非空白 token）
// 等形态不可达，靠评审兜底。守门自身包装测试已随测试归位迁到 scripts/
// （issue #1158），在扫描根之外，无需文件级豁免（原豁免已随迁移删除）。不设注释
// 豁免通道：逃逸 = 显式回 issue/ADR 讨论后改脚本（check-style-blocks 同一纪律）。
//
// TypeScript 化 + Bun 运行时（issue #734 / ADR-0083）：类型经 tsconfig.scripts.json
// 门槛检查；调用方式 `bun scripts/check-async-guards.ts`。
// 默认扫描本仓库（SCAN_ROOTS 清单，相对仓库根）；测试可传位置参数指向夹具仓库根
// （夹具按 SCAN_ROOTS 布局摆放）：bun scripts/check-async-guards.ts [repo-root]

import { existsSync, readdirSync, readFileSync } from "node:fs";
import { join, relative } from "node:path";
import { pathToFileURL, fileURLToPath } from "node:url";

/** 规则 1 形态：手搓竞态纪元（seq/epoch/generation 三名别名一并识别；`\w*` 含裸名；
 *  `\b` 防吞后缀——seqNum / epochAt 等尾部通配不在检测面，已知盲区见文件头） */
const HAND_ROLLED_EPOCH_PATTERN = /\blet\s+\w*(?:[Ss]eq|[Ee]poch|[Gg]eneration)\s*=\s*0\b/;

/** 规则 1 唯一合法住址（相对仓库根路径）：@ledger/latest-wins 共享 module 本体
 *  （#1678：竞态纪元机制实体化，其内部 `let epoch = 0` 是机制实现而非手搓守卫）。
 *  导出供包装测试同源引用（TOAST_BASELINE 同款纪律，单一事实源无双源漂移）。 */
export const SEQ_SEAM_FILE = "packages/latest-wins/src/latest-wins.ts";

/** 扫描根清单（相对仓库根）：src 应用壳 + 全部 packages 子包 src 树——登记面即
 *  扫描面，清单完整性由磁盘自证兜底（见 collectUnregisteredPackageSrcRoots）：
 *  有 src 的子包未登记即红，新包漏登不再静默逃逸（#1467）。 */
const SCAN_ROOTS: readonly string[] = [
  "src",
  "packages/api/src",
  "packages/field-errors/src",
  "packages/i18n/src",
  "packages/latest-wins/src",
  "packages/loadable/src",
  "packages/modal-intent/src",
  "packages/money/src",
  "packages/row-context-menu/src",
  "packages/scheduled-plan-list/src",
  "packages/storage/src",
  "packages/test-support/src",
  "packages/theme/src",
  "packages/transaction-modal-state/src",
  "packages/types/src",
  "packages/ui-kit/src",
  "packages/utils/src",
  "packages/window-tier/src",
];

/** 规则 2 匹配窗口上限（行，t( 起算）：errorMessage 须落在窗口内——折行/格式化
 *  不藏违规，无界贪婪匹配则把邻近无关 errorMessage 误收入窗（#1467 限定最大跨度）。
 *  导出供包装测试同源引用（SEQ_SEAM_FILE 同款纪律）。 */
export const TOAST_WINDOW_LINES = 5;

/** 规则 2 形态：catch 内直弹 toast 模板（跨行窗口；errorMessage 为 utils/errors
 *  统一错误提取）。`\s*` 容忍 ( 与 t( 之间的空白（含换行，无界——既有语义，格式
 *  噪音不作限距对象）；「限定最大跨度」辖 t( → errorMessage 段：errorMessage 须在
 *  TOAST_WINDOW_LINES 行窗口内——现役投资表单保存失败路径的多行形态与全部单行
 *  存量同窗识别（#1467）；三元条件夹在 message.error( 与 t( 之间（非纯空白）
 *  不匹配，靠评审兜底。窗口量词惰性（{0,N}?）：每处命中取最短窗口，相邻多行调用
 *  逐处计数不互吞；g 位供 matchAll 逐处计数。 */
const CATCH_TOAST_PATTERN = new RegExp(
  `message\\.error\\(\\s*t\\((?:[^\\n]*\\n){0,${TOAST_WINDOW_LINES}}?[^\\n]*errorMessage`,
  "g",
);

/**
 * 规则 2 存量基线（#1039 交付时点实测快照：61 处 / 31 文件；键相对扫描根、posix
 * 分隔，值为该文件允许的命中行数；#1318 扫描根扩至包内 src 树，键由相对 src
 * 改挂仓库根，值不变；#1321 起 useTransactionModalState 条目随包出壳，键同步为
 * 包内路径，值不变）。现计数须与基线全等：新增红、收缩未同步同样红
 * ——收编一处下调一处，收编至零删除条目；基线条目指向的文件不可达亦红（清单漂移
 * fail loud）。基线只减不增；唯一例外是检测面扩宽使存量显形的入册（#1467：扩面
 * 不是增量回潮，入册是新检测面下基线全等的配套动作，diff 须可审计）。
 */
export const TOAST_BASELINE: Readonly<Record<string, number>> = {
  // #1159 起源码按域归位，域文件键为域目录坐标 src/<域>/
  "src/item/AddItemForm.vue": 1,
  "src/physical-asset/PhysicalAssetDisposeModal.vue": 1,
  "src/physical-asset/PhysicalAssetFormModal.vue": 1,
  "src/physical-asset/PhysicalAssetValuationModal.vue": 1,
  "src/policy/PolicyAgreementSection.vue": 2,
  "src/policy/PolicyFormModal.vue": 2,
  "src/categories/CategoryAddForm.vue": 1,
  "src/categories/CategoryEditModal.vue": 1,
  "src/categories/CategoryTree.vue": 2,
  "src/scheduled/PlanDetailModal.vue": 2,
  "src/scheduled/SubscriptionSpendPanel.vue": 1,
  "src/scheduled/SubscriptionsPane.vue": 1,
  "src/settings/AboutSettings.vue": 1,
  "src/settings/BaseCurrencySettings.vue": 2,
  "src/settings/DataLocationSettings.vue": 3,
  "src/settings/LogSettings.vue": 2,
  "src/settings/SearchDataSettings.vue": 1,
  "src/backup/useBackup.ts": 6,
  "src/transaction/useRefundForm.ts": 1,
  "src/backup/useRestoreFromFile.ts": 2,
  "src/scheduled/useScheduledPlanForm.ts": 1,
  "packages/transaction-modal-state/src/useTransactionModalState.ts": 2,
  "src/views/AccountsView.vue": 4,
  "src/views/AiPromptView.vue": 2,
  "src/views/BudgetView.vue": 3,
  "src/views/ItemsView.vue": 4,
  "src/views/PoliciesView.vue": 1,
  "src/views/TransactionsView.vue": 2,
  // #1467 跨行窗口扩面使现役投资表单保存失败路径（多行形态）显形：按基线纪律
  // 入册（存量收编仍属 #1039 专项，本条只是新检测面下的全等配套）
  "src/investment/useInvestmentForm.ts": 1,
  // #1322 起计划清单接缝随包出壳，基线键改挂仓库根包内路径（值不变）
  "packages/scheduled-plan-list/src/useScheduledPlanList.ts": 1,
  "packages/ui-kit/src/NoteCopyButton.vue": 1,
};

/** 行首注释形态：整行跳过/置空（行内尾注与多行块注释内部行不可达，靠评审兜底） */
function isCommentLine(trimmed: string): boolean {
  return (
    trimmed.startsWith("//") ||
    trimmed.startsWith("/*") ||
    trimmed.startsWith("*") ||
    trimmed.startsWith("<!--")
  );
}

/** 注释行置空（保留换行结构）：行首注释行不作为靶形态素材——注释提及靶形态不误报；
 *  跨行窗口语义下由「先置空再整窗匹配」实现，行号因换行结构保留而精确保留；置空行
 *  仍占窗口行数（不隔断窗口、也不作素材） */
function blankCommentLines(source: string): string {
  return source
    .split("\n")
    .map((raw) => (isCommentLine(raw.trim()) ? "" : raw))
    .join("\n");
}

interface SourceFileRef {
  abs: string;
  rel: string;
}

/** 递归收集扫描根下全部 .ts/.vue 文件（rel 相对仓库根，按相对路径排序保证输出确定；
 *  目录不存在时返回空集——缺口由住址可达检查兜底） */
function collectSourceFiles(root: string, repoRoot: string): SourceFileRef[] {
  const out: SourceFileRef[] = [];
  const visit = (dir: string): void => {
    if (!existsSync(dir)) return;
    for (const entry of readdirSync(dir, { withFileTypes: true }).sort((a, b) =>
      a.name.localeCompare(b.name),
    )) {
      const abs = join(dir, entry.name);
      if (entry.isDirectory()) visit(abs);
      else if (entry.name.endsWith(".ts") || entry.name.endsWith(".vue")) {
        const rel = relative(repoRoot, abs).split("\\").join("/");
        out.push({ abs, rel });
      }
    }
  };
  visit(root);
  return out;
}

interface LineHit {
  line: number;
  text: string;
}

/** 行级扫描单文件：返回命中靶形态的行号（1 起算）与原文行；行首注释行跳过 */
function scanLines(source: string, pattern: RegExp): LineHit[] {
  const hits: LineHit[] = [];
  source.split("\n").forEach((raw, i) => {
    const trimmed = raw.trim();
    if (isCommentLine(trimmed)) return;
    if (pattern.test(raw)) hits.push({ line: i + 1, text: trimmed });
  });
  return hits;
}

/** 跨行窗口扫描单文件（规则 2）：注释行置空后整窗匹配，返回命中处起始行号
 *  （1 起算；matchAll 非重叠逐处计数，一处跨行调用计 1） */
function scanCrossLine(source: string, pattern: RegExp): number[] {
  const blanked = blankCommentLines(source);
  const lines: number[] = [];
  for (const m of blanked.matchAll(pattern)) {
    const start = m.index ?? 0;
    lines.push(blanked.slice(0, start).split("\n").length);
  }
  return lines;
}

/** 扫描根磁盘自证（#1467）：packages 子包源码目录（packages/<name>/src 存在）
 *  必须全部登记进 SCAN_ROOTS——新包静默逃逸扫描即红（登记清单漂移 fail loud） */
function collectUnregisteredPackageSrcRoots(repoRoot: string): string[] {
  const packagesDir = join(repoRoot, "packages");
  if (!existsSync(packagesDir)) return [];
  return readdirSync(packagesDir, { withFileTypes: true })
    .filter((e) => e.isDirectory())
    .map((e) => `packages/${e.name}/src`)
    .filter((rel) => existsSync(join(repoRoot, rel)))
    .filter((rel) => !SCAN_ROOTS.includes(rel));
}

function main(): void {
  const repoRoot = process.argv[2] ?? fileURLToPath(new URL("..", import.meta.url));
  const files = SCAN_ROOTS.flatMap((root) => collectSourceFiles(join(repoRoot, root), repoRoot));

  const problems: string[] = [];
  if (files.length === 0) {
    problems.push("✗ 扫描根提不出任何 .ts/.vue 源文件——目录指错或源码整体漂移，拒绝以空集假绿通过");
  }

  // 规则 1 前置：唯一合法住址必须可达——接缝文件删除/搬迁后未同步 SEQ_SEAM_FILE
  // 即红，拒绝白名单静默失效（清单漂移 fail loud，同结构守门规则⑥⑦登记目标形制）
  if (!existsSync(join(repoRoot, SEQ_SEAM_FILE))) {
    problems.push(
      `✗ 竞态纪元唯一合法住址不可达：${SEQ_SEAM_FILE}——@ledger/latest-wins 共享 module 文件` +
        `删除/搬迁后未同步 SEQ_SEAM_FILE 与 SCAN_ROOTS（清单漂移 fail loud，#1678 规则 1）`,
    );
  }

  // 扫描根磁盘自证：子包源码目录未登记进 SCAN_ROOTS 即红（#1467，堵新包静默
  // 逃逸扫描——登记型守门，删除本段即「未登记子包」用例红，ADR-0087）
  for (const rel of collectUnregisteredPackageSrcRoots(repoRoot)) {
    problems.push(
      `✗ packages 子包源码目录未登记扫描根：${rel}——异步守门扫描面按 SCAN_ROOTS ` +
        `清单收口，新包源码目录落盘即须同步登记（磁盘一致性自证，#1467）`,
    );
  }

  // 规则 1：手搓竞态纪元，硬零容忍（唯一合法住址豁免）
  let epochHits = 0;
  // 规则 2：按文件计数，与基线全等校验
  const toastCounts = new Map<string, number>();
  const toastLinesByFile = new Map<string, string[]>();
  for (const f of files) {
    const source = readFileSync(f.abs, "utf8");
    if (f.rel !== SEQ_SEAM_FILE) {
      for (const hit of scanLines(source, HAND_ROLLED_EPOCH_PATTERN)) {
        epochHits++;
        problems.push(
          `✗ 手搓竞态纪元：${f.rel}:${hit.line}（${hit.text}）\n` +
            `    竞态裁决一律走共享 module @ledger/latest-wins（#1678：begin/observe/` +
            `invalidate + token isStale，最新胜出裁决唯一实现，五处消费点收敛）；手搓` +
            `纪元已归零，此处为回潮——唯一合法住址 ${SEQ_SEAM_FILE}（module 本体，规则 1）`,
        );
      }
    }
    const toastHitLines = scanCrossLine(source, CATCH_TOAST_PATTERN);
    if (toastHitLines.length > 0) {
      toastCounts.set(f.rel, toastHitLines.length);
      toastLinesByFile.set(f.rel, toastHitLines.map(String));
    }
  }

  // 规则 2：现计数 vs 基线，全等校验（新增红 / 收缩未同步红 / 条目不可达红）
  let toastMismatches = 0;
  const scannedToastTotal = [...toastCounts.values()].reduce((a, b) => a + b, 0);
  for (const f of files) {
    const current = toastCounts.get(f.rel) ?? 0;
    const baseline = TOAST_BASELINE[f.rel] ?? 0;
    if (current > baseline) {
      toastMismatches++;
      problems.push(
        `✗ 直弹 toast 回潮/新增：${f.rel} 现 ${current} 处 > 基线 ${baseline} 处（行 ` +
          `${(toastLinesByFile.get(f.rel) ?? []).join("、")}）——错误反馈走 useLoadable ` +
          `error 通道（showErrorToast 单点，#1008）；存量收编属专项范围（#1039 Out of Scope）`,
      );
    } else if (current < baseline) {
      toastMismatches++;
      problems.push(
        `✗ 直弹 toast 基线待收缩：${f.rel} 现 ${current} 处 < 基线 ${baseline} 处——` +
          `存量已收编，请同步下调/删除 scripts/check-async-guards.ts 的 TOAST_BASELINE 条目` +
          `（基线即规格、只减不增、不留陈目，#1039 规则 2）；若未做收编，先排查是否` +
          `多行化/格式漂移使靶形态移出匹配窗口（不可见 ≠ 已收编，#1467），确认非漂移再下调`,
      );
    }
  }
  const scannedRels = new Set(files.map((f) => f.rel));
  for (const rel of Object.keys(TOAST_BASELINE)) {
    if (!scannedRels.has(rel)) {
      toastMismatches++;
      problems.push(
        `✗ toast 基线条目不可达：${rel}——文件删除/改名后未同步 TOAST_BASELINE` +
          `（清单漂移 fail loud，#1039 规则 2）`,
      );
    }
  }

  if (problems.length > 0) {
    for (const p of problems) console.error(p);
    console.error(
      `❌ 异步守门失败：手搓竞态纪元 ${epochHits} 处（唯一合法住址 ${SEQ_SEAM_FILE}）· ` +
        `toast 基线失配 ${toastMismatches} 处（基线 ${Object.keys(TOAST_BASELINE).length} 文件）` +
        `——手搓竞态纪元一律走 @ledger/latest-wins（#1678；#1008 / #1039 收编史，ADR-0040 当点一处 → 走查三处的教训）`,
    );
    process.exit(1);
  }
  console.log(
    `✅ 异步守门：${files.length} 个前端源文件 · 手搓竞态纪元 0（唯一合法住址 ${SEQ_SEAM_FILE}）· ` +
      `catch 直弹 toast 基线全等（${toastCounts.size} 文件 ${scannedToastTotal} 处，只减不增，#1039）`,
  );
}

// 仅直接运行时执行 main；被测试 import 时只取导出的基线清单（夹具派生单一事实源）。
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main();
}
