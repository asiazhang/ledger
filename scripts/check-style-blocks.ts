#!/usr/bin/env bun

import { readFileSync } from "node:fs";
import { relative } from "node:path";
import { pathToFileURL, fileURLToPath } from "node:url";
import { walkTextFiles } from "./gate-primitives.ts";
import { parse } from "vue/compiler-sfc";

const VUE_EXTENSIONS: ReadonlySet<string> = new Set([".vue"]);
const TS_EXTENSIONS: ReadonlySet<string> = new Set([".ts"]);

export const STYLE_BLOCK_WHITELIST: readonly string[] = [
  "src/App.vue",
  "src/accounts/AccountLink.vue",
  "src/backup/StartupFailureScreen.vue",
  "src/backup/UnlockScreen.vue",
  "src/components/GlobalBusyBar.vue",
  "src/components/MobileNavShell.vue",
  "src/components/QuickTimeRange.vue",
  "src/investment/InstrumentBrowser.vue",
  "src/investment/PortfolioTrendPanel.vue",
  "src/merchants/MerchantLink.vue",
  "src/physical-asset/PhysicalAssetDisposeModal.vue",
  "src/physical-asset/PhysicalAssetValuationModal.vue",
  "src/policy/PolicyFormModal.vue",
  "src/reports/MerchantRankingPanel.vue",
  "src/scheduled/PlanDetailModal.vue",
  "src/scheduled/SubscriptionSpendPanel.vue",
  "src/settings/BookSidebarEntry.vue",
  "src/settings/EncryptionSettings.vue",
  "src/settings/PassphraseStrengthMeter.vue",
  "src/transaction/AmountCell.vue",
  "src/transaction/SourceLink.vue",
  "src/views/GroupMoreView.vue",
  "src/views/InvestmentsView.vue",
  "src/views/ItemsView.vue",
  "src/views/PoliciesView.vue",
  "src/views/ReportsView.vue",
  "src/views/ScheduledView.vue",
  "src/views/SettingsView.vue",
];

function normalizePath(file: string): string {
  return relative(process.cwd(), file).split("\\").join("/");
}

export function collectVueFiles(root: string): string[] {
  return walkTextFiles(root, "", { extensions: VUE_EXTENSIONS }).map((f) => f.abs);
}

export function collectCssFiles(root: string): string[] {
  return walkTextFiles(root, "", { extensions: TS_EXTENSIONS })
    .filter((f) => f.abs.endsWith(".css.ts"))
    .map((f) => f.abs);
}

export const CJS_LOOKALIKE_RE =
  /(?:[\s;]|^)(?:module\.exports\b|exports\.\w|require\s*\(|global\.\w)/g;

export function checkCssFile(file: string): Array<{ line: number; text: string }> {
  const source = readFileSync(file, "utf-8");
  const hits: Array<{ line: number; text: string }> = [];
  const lines = source.split("\n");
  for (const [i, line] of lines.entries()) {
    CJS_LOOKALIKE_RE.lastIndex = 0;
    if (CJS_LOOKALIKE_RE.test(line)) {
      hits.push({ line: i + 1, text: line.trim() });
    }
  }
  return hits;
}

export function checkVueFile(file: string): { hasStyleBlock: boolean; failure?: string } {
  const source = readFileSync(file, "utf-8");
  const { descriptor, errors } = parse(source, { filename: file });
  if (errors.length > 0) {
    return { hasStyleBlock: false, failure: errors.map((e) => e.message).join("; ") };
  }
  return { hasStyleBlock: descriptor.styles.length > 0 };
}

function main(): void {
  const scanRoot = process.argv[2] ?? fileURLToPath(new URL("../src", import.meta.url));
  const files = collectVueFiles(scanRoot);
  const violations: string[] = [];
  const failures: string[] = [];
  for (const file of files) {
    const r = checkVueFile(file);
    if (r.failure) {
      failures.push(`✗ SFC 解析失败：${normalizePath(file) || file} — ${r.failure}`);
      continue;
    }
    if (r.hasStyleBlock && !STYLE_BLOCK_WHITELIST.includes(normalizePath(file))) {
      violations.push(normalizePath(file));
    }
  }
  const cssFiles = collectCssFiles(scanRoot);
  const cssHits = cssFiles.flatMap((file) =>
    checkCssFile(file).map((hit) => ({ file: normalizePath(file) || file, ...hit })),
  );
  const scannedRels = new Set(files.map((file) => normalizePath(file)));
  const unreachable = STYLE_BLOCK_WHITELIST.filter((rel) => !scannedRels.has(rel));
  if (
    violations.length === 0 &&
    failures.length === 0 &&
    unreachable.length === 0 &&
    cssHits.length === 0
  ) {
    console.log(
      `✅ 样式块守门：${files.length} 个 .vue 文件 + ${cssFiles.length} 个 .css.ts 检查通过——白名单（${STYLE_BLOCK_WHITELIST.length} 个存量文件）外零 <style> 块且全条目可达，css.ts 零 CJS 形态字面串（ADR-0093 / #1360 / #2189）`,
    );
    return;
  }
  for (const f of failures) console.error(f);
  for (const rel of unreachable) {
    console.error(
      `✗ 白名单条目不可达：${rel}——文件删除/改名/搬迁后未同步 STYLE_BLOCK_WHITELIST` +
        `（清单漂移 fail loud，#1360；请同步删除或改写该条目，陈目不静默存留）`,
    );
  }
  for (const v of violations) {
    console.error(
      `✗ 新增 <style> 块：${v}\n  样式方案守门（ADR-0093 / issue #888）：白名单外禁止 <style> 块，` +
        `新样式一律写组件旁路 vanilla-extract 样式文件（*.css.ts，与组件同目录共置）。` +
        `若属存量文件迁移回退或清单漂移，请同步 scripts/check-style-blocks.ts 的 STYLE_BLOCK_WHITELIST。`,
    );
  }
  for (const hit of cssHits) {
    console.error(
      `✗ css.ts 内 CJS 形态字面串：${hit.file}:${hit.line} — ${hit.text}\n` +
        `  vanilla-extract 以 mlly CJS_RE（不剥注释）判模块语法，命中即按 CommonJS 注入` +
        `require 版作用域包装，ESM 求值下失效、vite build 失败（#2189）。` +
        `改写措辞（如 global.css → 全局样式基座）或移出该文件。`,
    );
  }
  process.exit(1);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main();
}
