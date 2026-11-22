#!/usr/bin/env bun

import { existsSync, readFileSync } from "node:fs";
import { join, relative } from "node:path";
import { pathToFileURL, fileURLToPath } from "node:url";

import { lineAt, readDirEntries, walkTextFiles, type WalkedFile } from "./gate-primitives.ts";
const HAND_ROLLED_EPOCH_PATTERN = /\blet\s+\w*(?:[Ss]eq|[Ee]poch|[Gg]eneration)\s*=\s*0\b/;

export const EPOCH_SEAM_FILE = "packages/latest-wins/src/latest-wins.ts";

const SCAN_ROOTS: readonly string[] = [
  "src",
  "packages/api/src",
  "packages/field-errors/src",
  "packages/i18n/src",
  "packages/latest-wins/src",
  "packages/loadable/src",
  "packages/mock-backend/src",
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

export const TOAST_WINDOW_LINES = 5;

const CATCH_TOAST_PATTERN = new RegExp(
  `message\\.error\\(\\s*t\\((?:[^\\n]*\\n){0,${TOAST_WINDOW_LINES}}?[^\\n]*errorMessage`,
  "g",
);

export const TOAST_BASELINE: Readonly<Record<string, number>> = {
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
  "src/settings/BaseCurrencySettings.vue": 2,
  "src/settings/DataLocationSettings.vue": 3,
  "src/settings/LogSettings.vue": 2,
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
  "src/investment/useInvestmentForm.ts": 1,
  "packages/scheduled-plan-list/src/useScheduledPlanList.ts": 1,
};

function isCommentLine(trimmed: string): boolean {
  return (
    trimmed.startsWith("//") ||
    trimmed.startsWith("/*") ||
    trimmed.startsWith("*") ||
    trimmed.startsWith("<!--")
  );
}

function blankCommentLines(source: string): string {
  return source
    .split("\n")
    .map((raw) => (isCommentLine(raw.trim()) ? "" : raw))
    .join("\n");
}

const SCAN_EXTENSIONS: ReadonlySet<string> = new Set([".ts", ".vue"]);

function collectSourceFiles(root: string, repoRoot: string): WalkedFile[] {
  if (!existsSync(root)) return [];
  return walkTextFiles(root, "", { extensions: SCAN_EXTENSIONS }).map((f) => ({
    abs: f.abs,
    rel: relative(repoRoot, f.abs).split("\\").join("/"),
  }));
}

interface LineHit {
  line: number;
  text: string;
}

function scanLines(source: string, pattern: RegExp): LineHit[] {
  const hits: LineHit[] = [];
  source.split("\n").forEach((raw, i) => {
    const trimmed = raw.trim();
    if (isCommentLine(trimmed)) return;
    if (pattern.test(raw)) hits.push({ line: i + 1, text: trimmed });
  });
  return hits;
}

function scanCrossLine(source: string, pattern: RegExp): number[] {
  const blanked = blankCommentLines(source);
  const lines: number[] = [];
  for (const m of blanked.matchAll(pattern)) {
    lines.push(lineAt(blanked, m.index));
  }
  return lines;
}

function collectUnregisteredPackageSrcRoots(repoRoot: string): string[] {
  const packagesDir = join(repoRoot, "packages");
  if (!existsSync(packagesDir)) return [];
  return readDirEntries(packagesDir)
    .filter((e) => e.isDirectory)
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

  if (!existsSync(join(repoRoot, EPOCH_SEAM_FILE))) {
    problems.push(
      `✗ 竞态纪元唯一合法住址不可达：${EPOCH_SEAM_FILE}——@ledger/latest-wins 共享 module 文件` +
        `删除/搬迁后未同步 EPOCH_SEAM_FILE 与 SCAN_ROOTS（清单漂移 fail loud，#1678 规则 1）`,
    );
  }

  for (const rel of collectUnregisteredPackageSrcRoots(repoRoot)) {
    problems.push(
      `✗ packages 子包源码目录未登记扫描根：${rel}——异步守门扫描面按 SCAN_ROOTS ` +
        `清单收口，新包源码目录落盘即须同步登记（磁盘一致性自证，#1467）`,
    );
  }

  let epochHits = 0;
  const toastCounts = new Map<string, number>();
  const toastLinesByFile = new Map<string, string[]>();
  for (const f of files) {
    const source = readFileSync(f.abs, "utf8");
    if (f.rel !== EPOCH_SEAM_FILE) {
      for (const hit of scanLines(source, HAND_ROLLED_EPOCH_PATTERN)) {
        epochHits++;
        problems.push(
          `✗ 手搓竞态纪元：${f.rel}:${hit.line}（${hit.text}）\n` +
            `    竞态裁决一律走共享 module @ledger/latest-wins（#1678：begin/observe/` +
            `invalidate + token isStale，最新胜出裁决唯一实现，五处消费点收敛）；手搓` +
            `纪元已归零，此处为回潮——唯一合法住址 ${EPOCH_SEAM_FILE}（module 本体，规则 1）`,
        );
      }
    }
    const toastHitLines = scanCrossLine(source, CATCH_TOAST_PATTERN);
    if (toastHitLines.length > 0) {
      toastCounts.set(f.rel, toastHitLines.length);
      toastLinesByFile.set(f.rel, toastHitLines.map(String));
    }
  }

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
      `❌ 异步守门失败：手搓竞态纪元 ${epochHits} 处（唯一合法住址 ${EPOCH_SEAM_FILE}）· ` +
        `toast 基线失配 ${toastMismatches} 处（基线 ${Object.keys(TOAST_BASELINE).length} 文件）` +
        `——手搓竞态纪元一律走 @ledger/latest-wins（#1678；#1008 / #1039 收编史，ADR-0040 当点一处 → 走查三处的教训）`,
    );
    process.exit(1);
  }
  console.log(
    `✅ 异步守门：${files.length} 个前端源文件 · 手搓竞态纪元 0（唯一合法住址 ${EPOCH_SEAM_FILE}）· ` +
      `catch 直弹 toast 基线全等（${toastCounts.size} 文件 ${scannedToastTotal} 处，只减不增，#1039）`,
  );
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main();
}
