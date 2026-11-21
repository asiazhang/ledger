#!/usr/bin/env bun

import { existsSync, readFileSync, statSync, type Stats } from "node:fs";
import { dirname, join } from "node:path";
import { pathToFileURL, fileURLToPath } from "node:url";
import {
  RUST_EXTENSIONS,
  hasExtension,
  lineAt,
  maskNonCode,
  readDirEntries,
  walkTextFiles,
  type WalkedFile,
} from "./gate-primitives.ts";

export interface WhitelistEntry {
  path: string;
  layer: Layer;
}

export const LAYER = {
  DOMAIN: "域目录",
  INFRA: "基础设施",
  PROTOCOL: "协议",
} as const;

export type Layer = (typeof LAYER)[keyof typeof LAYER];

export const WHITELIST: readonly WhitelistEntry[] = [{ path: "test_support", layer: LAYER.DOMAIN }];

export const INFRA_SRC_REL = "crates/infra/src";

export const TRANSACTION_ZONE = {
  SHARED: "共享语义",
  SEAM: "跨域接缝",
  WRITE: "写路径",
  READ: "读路径",
} as const;

export type TransactionZone = (typeof TRANSACTION_ZONE)[keyof typeof TRANSACTION_ZONE];

export const TRANSACTION_ZONE_BY_DIR: Readonly<Record<string, TransactionZone>> = {
  amount: TRANSACTION_ZONE.SHARED,
  command: TRANSACTION_ZONE.SHARED,
  model: TRANSACTION_ZONE.SHARED,
  read: TRANSACTION_ZONE.READ,
  seams: TRANSACTION_ZONE.SEAM,
  shared: TRANSACTION_ZONE.SHARED,
  write: TRANSACTION_ZONE.WRITE,
};

const TRANSACTION_ZONE_RANK: Record<TransactionZone, number> = {
  [TRANSACTION_ZONE.SHARED]: 0,
  [TRANSACTION_ZONE.SEAM]: 1,
  [TRANSACTION_ZONE.WRITE]: 2,
  [TRANSACTION_ZONE.READ]: 2,
};

interface TransactionZoneEdge {
  dir: string;
  target: string;
  reason: string;
}

export const TRANSACTION_ZONE_ALLOWED_EDGES: readonly TransactionZoneEdge[] = [];

export const CRATE_LAYER = {
  SHELL: "壳",
  DOMAIN: "域",
  INFRA: "基础设施",
  PROTOCOL: "协议",
} as const;

export type CrateLayer = (typeof CRATE_LAYER)[keyof typeof CRATE_LAYER];

const CRATE_LAYER_RANK: Record<CrateLayer, number> = {
  [CRATE_LAYER.SHELL]: 3,
  [CRATE_LAYER.DOMAIN]: 2,
  [CRATE_LAYER.PROTOCOL]: 1,
  [CRATE_LAYER.INFRA]: 0,
};

export interface CrateEntry {
  name: string;
  dir: string;
  layer: CrateLayer;
}

export const CRATES: readonly CrateEntry[] = [
  {
    name: "ledger-accounts",
    dir: "crates/accounts",
    layer: CRATE_LAYER.DOMAIN,
  },
  {
    name: "ledger-backup",
    dir: "crates/backup",
    layer: CRATE_LAYER.DOMAIN,
  },
  {
    name: "ledger-budget",
    dir: "crates/budget",
    layer: CRATE_LAYER.DOMAIN,
  },
  {
    name: "ledger-categories",
    dir: "crates/categories",
    layer: CRATE_LAYER.DOMAIN,
  },
  {
    name: "ledger-currencies",
    dir: "crates/currencies",
    layer: CRATE_LAYER.DOMAIN,
  },
  {
    name: "ledger-dashboard",
    dir: "crates/dashboard",
    layer: CRATE_LAYER.DOMAIN,
  },
  {
    name: "ledger-infra",
    dir: "crates/infra",
    layer: CRATE_LAYER.INFRA,
  },
  {
    name: "ledger-investment",
    dir: "crates/investment",
    layer: CRATE_LAYER.DOMAIN,
  },
  {
    name: "ledger-item",
    dir: "crates/item",
    layer: CRATE_LAYER.DOMAIN,
  },
  {
    name: "ledger-market-sync",
    dir: "crates/market-sync",
    layer: CRATE_LAYER.DOMAIN,
  },
  {
    name: "ledger-merchants",
    dir: "crates/merchants",
    layer: CRATE_LAYER.DOMAIN,
  },
  {
    name: "ledger-physical-asset",
    dir: "crates/physical-asset",
    layer: CRATE_LAYER.DOMAIN,
  },
  {
    name: "ledger-policy",
    dir: "crates/policy",
    layer: CRATE_LAYER.DOMAIN,
  },
  {
    name: "ledger-reports",
    dir: "crates/reports",
    layer: CRATE_LAYER.DOMAIN,
  },
  {
    name: "ledger-savings-goal",
    dir: "crates/savings-goal",
    layer: CRATE_LAYER.DOMAIN,
  },
  {
    name: "ledger-scheduled",
    dir: "crates/scheduled",
    layer: CRATE_LAYER.DOMAIN,
  },
  {
    name: "ledger-sync-engine",
    dir: "crates/sync-engine",
    layer: CRATE_LAYER.DOMAIN,
  },
  {
    name: "ledger-sync-protocol",
    dir: "crates/sync-protocol",
    layer: CRATE_LAYER.PROTOCOL,
  },
  {
    name: "ledger-transaction",
    dir: "crates/transaction",
    layer: CRATE_LAYER.DOMAIN,
  },
  {
    name: "tauri-app",
    dir: ".",
    layer: CRATE_LAYER.SHELL,
  },
];

const MEMBER_DIR_GLOB = "crates/*";

export interface CrateModuleTarget {
  crate: string;
  srcRel: string;
  layer: Layer;
}

function moduleLayerOf(crateLayer: CrateLayer): Layer | null {
  if (crateLayer === CRATE_LAYER.DOMAIN) return LAYER.DOMAIN;
  if (crateLayer === CRATE_LAYER.INFRA) return LAYER.INFRA;
  if (crateLayer === CRATE_LAYER.PROTOCOL) return LAYER.PROTOCOL;
  return null;
}

export const CRATE_MODULE_TARGETS: readonly CrateModuleTarget[] = CRATES.flatMap((crate) => {
  const layer = moduleLayerOf(crate.layer);
  if (layer === null) return [];
  return [{ crate: crate.name, srcRel: `${crate.dir}/src`, layer }];
});

const PANIC_LINT_KEYS = [
  "unwrap_used",
  "expect_used",
  "panic",
  "todo",
  "unimplemented",
  "unreachable",
] as const;

const WORKSPACE_COMMAND_FILES = [
  "scripts/check.sh",
  "scripts/test.sh",
  "scripts/lint-fix.sh",
  "scripts/test-exec.ts",
  ".github/workflows/build.yml",
] as const;

const WORKSPACE_SCOPE_PATTERN = /(?:^|\s)--workspace(?:\s|$)/;
const ALL_SCOPE_PATTERN = /(?:^|\s)--all(?:\s|$)/;

const CARGO_SUBCOMMANDS = ["clippy", "test", "fmt"] as const;

function maskShellQuoted(text: string): string {
  return text
    .split("\n")
    .map((line) => {
      const chars = line.split("");
      const blank = (from: number, to: number): void => {
        for (let k = from; k < to && k < chars.length; k += 1) chars[k] = " ";
      };
      for (let i = 0; i < chars.length; i += 1) {
        const c = chars[i];
        if (c === "#" && (i === 0 || /\s/.test(chars[i - 1] as string))) {
          blank(i, chars.length);
          break;
        }
        if (c !== '"' && c !== "'") continue;
        let j = i + 1;
        while (j < chars.length) {
          if (c === '"' && chars[j] === "\\") {
            j += 2;
            continue;
          }
          if (chars[j] === c) break;
          j += 1;
        }
        blank(i, Math.min(j + 1, chars.length));
        i = j;
      }
      return chars.join("");
    })
    .join("\n");
}

interface TsCargoArray {
  line: number;
  elements: string[];
}

function isTsCommentLine(trimmed: string): boolean {
  return (
    trimmed === "" ||
    trimmed.startsWith("//") ||
    trimmed.startsWith("/*") ||
    trimmed.startsWith("*")
  );
}

function tsCargoArrays(source: string): TsCargoArray[] {
  const lines = source.split("\n");
  const found: TsCargoArray[] = [];
  for (let i = 0; i < lines.length; i += 1) {
    const raw = lines[i] ?? "";
    if (isTsCommentLine(raw.trim())) continue;
    const marker = /\bcargo\s*,/.exec(raw);
    if (marker === null) continue;
    let text = raw.slice(marker.index + marker[0].length);
    let startLine = i;
    if (!text.includes("[")) {
      let j = i + 1;
      while (j < lines.length && (lines[j] ?? "").trim() === "") j += 1;
      if (!(lines[j] ?? "").trim().startsWith("[")) continue;
      startLine = j;
      text = lines[j] ?? "";
      i = j;
    }
    const open = text.indexOf("[");
    let j = i;
    while (text.indexOf("]", open + 1) === -1 && j + 1 < lines.length) {
      j += 1;
      text += `\n${lines[j] ?? ""}`;
    }
    const close = text.indexOf("]", open + 1);
    if (close === -1) continue;
    const elements = [...text.slice(open, close + 1).matchAll(/'([^']*)'|"([^"]*)"/g)].map(
      (m) => m[1] ?? m[2] ?? "",
    );
    found.push({ line: startLine + 1, elements });
  }
  return found;
}

function checkTsCargoArrays(rel: string, source: string, problems: string[]): number {
  let hits = 0;
  for (const { line, elements } of tsCargoArrays(source)) {
    const subcommand = elements[0];
    if (
      subcommand === undefined ||
      !(CARGO_SUBCOMMANDS as readonly string[]).includes(subcommand)
    ) {
      continue;
    }
    hits += 1;
    if (elements.includes("--workspace") || elements.includes("--all")) continue;
    problems.push(
      `✗ workspace 命令覆盖：${rel}:${line} cargo ${subcommand} 数组形态缺 '--workspace'` +
        "（非虚拟 workspace 下默认只作用于根包，会静默漏检成员 crate）\n" +
        `    ${elements.join(" ")}`,
    );
  }
  return hits;
}

const SHELL_DEP_PATTERN = /\bcommands\s*::|\bcommands\s+as\b/;

interface InfraDevDepEdge {
  crate: string;
  reason: string;
}

const INFRA_DOMAIN_ALLOWED_EDGES: readonly InfraDevDepEdge[] = [
  {
    crate: "tauri-app",
    reason:
      "ADR-0084 迁移状态段 + ADR-0071 决策 6：内联 cfg(test) 测试经测试支持域 test_support 建库/取常量与种子（#758 收口），测试专用边、非产品依赖",
  },
  {
    crate: "ledger-backup",
    reason:
      "ADR-0111 决策 4 / #1134：db 测试注册备份域 after_commit 置脏钩子并读回 AutoBackupState 对拍——跨域接线只在测试目标验证，生产依赖图不含本边",
  },
  {
    crate: "ledger-transaction",
    reason:
      "spec #1086 明文裁决的测试专用环：db 测试经核心交易域金额口径（convert_to_native_current）对拍，生产依赖图不含本边",
  },
  {
    crate: "ledger-accounts",
    reason: "ADR-0067 / #1093：db 测试经账户域余额口径对拍缓存刷新，生产依赖图不含本边",
  },
];

const INFRA_BLOCK_FORBIDDEN: Record<string, readonly string[]> = {
  db: ["boot", "signals"],
};

function infraBlockDepPattern(targets: readonly string[]): RegExp {
  return new RegExp(
    `\\bcrate\\s*::\\s*\\{?\\s*(${targets.join("|")})\\b(?:\\s*::|\\s+as\\b|\\s*;)`,
  );
}

interface InfraBlockEdge {
  file: string;
  target: string;
  reason: string;
}

const INFRA_BLOCK_ALLOWED_EDGES: readonly InfraBlockEdge[] = [
  {
    file: "db/mod.rs",
    target: "boot",
    reason:
      "ADR-0111 决策 2 / #1131：引导层五模块升顶层 boot 后，既有 `crate::db::{boot,…}` 调用点与协议 crate 的 `ledger_infra::db::…` 路径经本再导出保持零改动——路径兼容面，非机制依赖（#1128 ids 同款口径）",
  },
];

interface SymbolCallerEntry {
  file: string;
  reason: string;
}

const CONVERT_CURRENT_CALL_SYMBOL = "convert_to_native_current";

export const CONVERT_CURRENT_CALLERS: readonly SymbolCallerEntry[] = [
  {
    file: "crates/dashboard/src/lib.rs",
    reason: "读路径：净资产与持仓市值卡片的当期折算（消费面见 amount convert.rs 当期入口 doc）",
  },
  {
    file: "crates/investment/src/overview.rs",
    reason: "读路径：投资概览市值与累计盈亏的当期折算",
  },
  {
    file: "crates/investment/src/crud.rs",
    reason:
      "读路径：持仓行逐行当期折算的软形态（try_convert_to_native_current，issue #1797）——持仓页签合计卡与首页投资概览卡的逐行本位币列；单行缺汇率降为行级 null，命令不失败",
  },
  {
    file: "crates/investment/src/reports.rs",
    reason:
      "读路径：累计收益折本位币单值（query_cumulative_pnl_native_total，issue #1797）——三腿逐行当期折算后求和，缺汇率码化上抛不给半截数字",
  },
  {
    file: "crates/investment/src/financial_freedom.rs",
    reason: "读路径：财务自由度可投资资产与现金合计的当期折算",
  },
  {
    file: "crates/physical-asset/src/crud.rs",
    reason: "读路径：实物资产估值的当期折算",
  },
  {
    file: "crates/scheduled/src/spend.rs",
    reason: "读路径：定时计划花费推算的当期折算",
  },
  {
    file: "crates/savings-goal/src/pace.rs",
    reason:
      "读路径：储蓄目标关联计划节奏的当期折算（#1753，订阅花费 spend.rs 同款先例——计划金额折算本位币再乘周期系数；计划币种即专属账户币种=本位币，正常数据零汇率读取）",
  },
  {
    file: "src/cross_book_summary.rs",
    reason:
      "读路径：跨账本汇总的当期折算（活动本读数合并单点 read_active_book_and_merge，#1699 自壳层命令闭包抽出；ADR-0114 跨本唯一例外）",
  },
  {
    file: "src/bin/ledger-perf/bench.rs",
    reason: "压测工具（ledger-perf bench）非生产路径，显式列入注记（#1692）",
  },
];

function convertCurrentCallHits(source: string): ScanHit[] {
  const masked = maskNonCode(source);
  const rawLines = source.split("\n");
  const hits: ScanHit[] = [];
  const re = new RegExp(`\\b(?:try_)?${CONVERT_CURRENT_CALL_SYMBOL}[ \\t]*\\(`, "g");
  for (const m of masked.matchAll(re)) {
    const idx = m.index ?? 0;
    if (/(?:^|[^\w])fn\s+$/.test(masked.slice(Math.max(0, idx - 4), idx))) continue;
    const line = lineAt(masked, idx);
    hits.push({ line, text: (rawLines[line - 1] ?? "").trim(), match: m[0], captured: undefined });
  }
  return hits;
}

const GLOBAL_MODEL_PATH_PATTERN = /\b(?:crate|tauri_app_lib)\s*::\s*models\b/;

const MODEL_GLOB_REEXPORT_PATTERN = /\bpub\s+use\s+[\w:]*\bmodels?\b\s*::\s*\*/;

const MODEL_FILE_GLOB_PATTERN = /\bpub\s+use\s+[\w:]*\*/;

const NATIVE_TX_STMT_PATTERN = /\bexecute(?:_batch)?\s*\(\s*"(?:BEGIN|COMMIT|ROLLBACK)\b/;

const NATIVE_TX_STMT_ALLOWED = `${INFRA_SRC_REL}/db/tx_scope.rs`;

function disallowedBraceEntries(body: string): string[] {
  const out: string[] = [];
  let depth = 0;
  let current = "";
  const flush = (): void => {
    const head = current.trim().split(/[\s:{]/)[0] ?? "";
    if (head !== "") {
      out.push(head);
    }
    current = "";
  };
  for (const ch of body) {
    if (ch === "{" || ch === "(" || ch === "[") depth++;
    else if (ch === "}" || ch === ")" || ch === "]") depth--;
    if (ch === "," && depth === 0) flush();
    else current += ch;
  }
  flush();
  return out;
}

function isModelFile(relPath: string): boolean {
  const segments = relPath.split("/");
  const file = segments[segments.length - 1];
  if (file === "model.rs" || file === "models.rs") return true;
  return segments.slice(0, -1).some((s) => s === "model" || s === "models");
}

function isTestFile(relPath: string): boolean {
  const segments = relPath.split("/");
  const file = segments[segments.length - 1];
  return file === "tests.rs" || segments.slice(0, -1).includes("tests");
}

interface ScanHit {
  line: number;
  text: string;
  match: string;
  captured: string | undefined;
}

function scanRustSource(
  text: string,
  pattern: RegExp = SHELL_DEP_PATTERN,
  keepLiterals = false,
): ScanHit[] {
  const hits: ScanHit[] = [];
  const masked = maskNonCode(text, keepLiterals);
  const maskedLines = masked.split("\n");
  const rawLines = text.split("\n");
  for (let i = 0; i < maskedLines.length; i++) {
    const m = maskedLines[i].match(pattern);
    if (m) hits.push({ line: i + 1, text: rawLines[i].trim(), match: m[0], captured: m[1] });
  }
  return hits;
}

function collectRustFiles(dir: string, relBase: string): WalkedFile[] {
  return walkTextFiles(dir, relBase, { extensions: RUST_EXTENSIONS }).filter(
    (f) => !isTestFile(f.rel),
  );
}

function manifestSection(text: string, section: string): string | null {
  const lines = text.split("\n");
  const header = `[${section}]`;
  let start = -1;
  for (let i = 0; i < lines.length; i++) {
    const trimmed = lines[i].trim();
    if (start === -1) {
      if (trimmed === header) start = i + 1;
    } else if (trimmed.startsWith("[")) {
      return lines.slice(start, i).join("\n");
    }
  }
  return start === -1 ? null : lines.slice(start).join("\n");
}

function manifestPackageName(manifest: string): string | null {
  const section = manifestSection(manifest, "package");
  const m = section?.match(/(?:^|\n)\s*name\s*=\s*"([^"]+)"/);
  return m ? m[1] : null;
}

function declaredDependencyNames(manifest: string, table: string): string[] {
  const names = new Set<string>();
  const tableRe = new RegExp(`^(?:target\\..+\\.)?${table}(?:\\.([A-Za-z0-9_-]+))?$`);
  let current = "";
  for (const raw of manifest.split("\n")) {
    const header = raw.trim().match(/^\[([^\]]+)\]$/);
    if (header) {
      current = header[1];
      const sub = tableRe.exec(current);
      if (sub?.[1]) names.add(sub[1]);
      continue;
    }
    const sub = tableRe.exec(current);
    if (!sub || sub[1]) continue;
    const key = raw.trim().match(/^([A-Za-z0-9_-]+)\s*=/);
    if (key) names.add(key[1]);
  }
  return [...names];
}

function declaredProductionDependencyNames(manifest: string): string[] {
  return declaredDependencyNames(manifest, "dependencies");
}

function declaredDevDependencyNames(manifest: string): string[] {
  return declaredDependencyNames(manifest, "dev-dependencies");
}

function inheritsWorkspaceLints(manifest: string): boolean {
  const section = manifestSection(manifest, "lints");
  return section !== null && /(?:^|\n)\s*workspace\s*=\s*true\b/.test(section);
}

function firstCfgTextBefore(lines: readonly string[], declIndex: number): string | null {
  const keep = maskNonCode(lines.join("\n"), true);
  let boundary = 0;
  for (let k = 0; k < declIndex; k++) boundary += lines[k].length + 1;
  const attrs = attributeChainBefore(keep, boundary);
  for (let k = attrs.length - 1; k >= 0; k--) {
    if (attrs[k].startsWith("#[cfg(")) return attrs[k];
  }
  return null;
}

function hasTestAllowingCfgGate(lines: readonly string[], declIndex: number): boolean {
  const cfg = firstCfgTextBefore(lines, declIndex);
  return cfg !== null && /\btest\b/.test(cfg) && !cfg.includes("not(");
}

function hasHttpFeatureCfgGate(lines: readonly string[], declIndex: number): boolean {
  const cfg = firstCfgTextBefore(lines, declIndex);
  return cfg !== null && /feature\s*=\s*"http"/.test(cfg) && !cfg.includes("not(");
}

function productionLedgerInfraEnablement(manifest: string, feature: string): string | null {
  const featureRe = new RegExp(`\\b${feature}\\b`);
  let section = "";
  for (const raw of manifest.split("\n")) {
    const header = raw.trim().match(/^\[([^\]]+)\]$/);
    if (header) {
      section = header[1];
      continue;
    }
    if (/^(?:target\..+\.)?dependencies$/.test(section)) {
      if (/^\s*ledger-infra\s*=/.test(raw) && featureRe.test(raw)) return raw.trim();
    } else if (/^(?:target\..+\.)?dependencies\.ledger-infra$/.test(section)) {
      if (/^\s*features\s*=/.test(raw) && featureRe.test(raw)) return raw.trim();
    }
  }
  return null;
}

function defaultFeaturesInclude(manifest: string, feature: string): boolean {
  const section = manifestSection(manifest, "features");
  if (section === null) return false;
  const featureEdges = new Map<string, string[]>();
  for (const line of section.split("\n")) {
    const m = line.match(/^\s*([A-Za-z0-9_-]+)\s*=\s*\[([^\]]*)\]/);
    if (m === null) continue;
    featureEdges.set(
      m[1],
      [...m[2].matchAll(/"([^"]+)"/g)].map((x) => x[1]),
    );
  }
  const seen = new Set<string>();
  const pending = [...(featureEdges.get("default") ?? [])];
  while (pending.length > 0) {
    const current = pending.pop() as string;
    if (current.includes(feature)) return true;
    if (seen.has(current)) continue;
    seen.add(current);
    pending.push(...(featureEdges.get(current) ?? []));
  }
  return false;
}

function memberCrateDirs(srcTauriDir: string): string[] {
  const cratesDir = join(srcTauriDir, "crates");
  if (!existsSync(cratesDir)) return [];
  return readDirEntries(cratesDir)
    .filter((e) => e.isDirectory && existsSync(join(cratesDir, e.name, "Cargo.toml")))
    .map((e) => `crates/${e.name}`);
}

function checkCrateBoundaries(srcTauriDir: string): string[] {
  const problems: string[] = [];
  const repoRoot = dirname(srcTauriDir);
  const rootManifestPath = join(srcTauriDir, "Cargo.toml");
  if (!existsSync(rootManifestPath)) {
    problems.push(`✗ crate 边界：workspace 根清单不存在：${rootManifestPath}`);
    return problems;
  }
  const rootManifest = readFileSync(rootManifestPath, "utf8");

  const workspaceSection = manifestSection(rootManifest, "workspace");
  if (workspaceSection === null) {
    problems.push(
      "✗ crate 边界：src-tauri/Cargo.toml 缺 [workspace] 段——Rust 根须为 workspace 根（spec #1086）",
    );
  } else if (!workspaceSection.includes(`"${MEMBER_DIR_GLOB}"`)) {
    problems.push(
      `✗ crate 边界：[workspace] members 未包含 "${MEMBER_DIR_GLOB}"——成员目录须用 glob 纳入，` +
        "新增 crate 自动入 workspace，漏项即静默漏检",
    );
  }

  const clippyLints = manifestSection(rootManifest, "workspace.lints.clippy");
  if (clippyLints === null) {
    problems.push(
      "✗ 门禁继承：[workspace.lints.clippy] 缺失——六件套 deny 门禁的唯一声明处（ADR-0060 / spec #1086）",
    );
  } else {
    for (const key of PANIC_LINT_KEYS) {
      if (!new RegExp(`(?:^|\\n)\\s*${key}\\s*=\\s*"deny"`).test(clippyLints)) {
        problems.push(`✗ 门禁继承：[workspace.lints.clippy] 缺 ${key} = "deny"（ADR-0060 六件套）`);
      }
    }
  }

  if (!inheritsWorkspaceLints(rootManifest)) {
    problems.push(
      "✗ 门禁继承：workspace 根包缺 [lints] workspace = true——根包同受六件套约束（ADR-0060）",
    );
  }

  const onDisk = memberCrateDirs(srcTauriDir);
  const registeredMemberDirs = CRATES.filter((c) => c.dir.startsWith("crates/"))
    .map((c) => c.dir)
    .sort();
  for (const dir of onDisk) {
    if (!CRATES.some((c) => c.dir === dir)) {
      problems.push(
        `✗ crate 边界：成员 crate 未登记 CRATES：${dir}\n` +
          "    新增 crate 后须在 scripts/check-structure.ts 的 CRATES 追加一行" +
          "（name / dir / layer；依赖面注记归 crate lib.rs 的 //!），" +
          "否则边界知识分裂成两份、依赖方向失守",
      );
    }
  }
  for (const dir of registeredMemberDirs) {
    if (!onDisk.includes(dir)) {
      problems.push(`✗ crate 边界：CRATES 登记的成员目录不存在：${dir}（清单漂移 fail loud）`);
    }
  }

  for (const crate of CRATES) {
    const isRoot = crate.dir === ".";
    const manifestPath = isRoot ? rootManifestPath : join(srcTauriDir, crate.dir, "Cargo.toml");
    if (!existsSync(manifestPath)) {
      problems.push(`✗ crate 边界：crate 清单不存在：${crate.name}（${crate.dir}）`);
      continue;
    }
    const manifest = isRoot ? rootManifest : readFileSync(manifestPath, "utf8");
    const name = manifestPackageName(manifest);
    if (name !== crate.name) {
      problems.push(
        `✗ crate 边界：CRATES 登记名 ${crate.name} 与清单包名 ${name ?? "（缺 name）"} 不一致（${crate.dir}）`,
      );
    }
    if (!isRoot && !inheritsWorkspaceLints(manifest)) {
      problems.push(
        `✗ 门禁继承：成员 crate ${crate.name} 缺 [lints] workspace = true（${crate.dir}/Cargo.toml）\n` +
          "    缺失即六件套 deny 门禁静默消失而 clippy 依然全绿——删除继承行即变红（ADR-0060 / spec #1086）",
      );
    }
    for (const dep of declaredProductionDependencyNames(manifest)) {
      const target = CRATES.find((c) => c.name === dep);
      if (target && CRATE_LAYER_RANK[target.layer] > CRATE_LAYER_RANK[crate.layer]) {
        problems.push(
          `✗ crate 依赖方向：${crate.name}（${crate.layer}）依赖 ${target.name}（${target.layer}）\n` +
            "    分层规则：壳 → 域 → 基础设施单向；被依赖逻辑应下沉到更低层（spec #1086）",
        );
      }
    }
    if (
      crate.layer === CRATE_LAYER.DOMAIN &&
      crate.name !== "ledger-sync-engine" &&
      declaredProductionDependencyNames(manifest).includes("ledger-sync-engine")
    ) {
      problems.push(
        `✗ crate 依赖方向：业务域 crate ${crate.name} 生产依赖多端同步域 ledger-sync-engine\n` +
          "    业务域只依赖同步协议 crate `ledger-sync-protocol`，不依赖多端同步域（ADR-0101 决策 4b / #1107）；" +
          "重放分派住同步域单向消费各业务域，反边即环",
      );
    }
    if (crate.layer === CRATE_LAYER.INFRA) {
      for (const dep of declaredDevDependencyNames(manifest)) {
        const target = CRATES.find((c) => c.name === dep);
        if (!target) continue;
        if (CRATE_LAYER_RANK[target.layer] < CRATE_LAYER_RANK[CRATE_LAYER.DOMAIN]) continue;
        if (INFRA_DOMAIN_ALLOWED_EDGES.some((e) => e.crate === dep)) continue;
        problems.push(
          `✗ 基础设施→域方向：${crate.name} 声明 dev-dependency ${target.name}（${target.layer}）未留痕\n` +
            "    基础设施→域生产边归零（ADR-0071 决策 6 / #538）；dev-dependency 环 cargo 放行、" +
            "是编译期盲区——测试专用边须逐条留痕于本脚本 INFRA_DOMAIN_ALLOWED_EDGES（附 ADR 指针），" +
            "或把逻辑下沉域目录",
        );
      }
    }
  }

  for (const rel of WORKSPACE_COMMAND_FILES) {
    const abs = join(repoRoot, rel);
    if (!existsSync(abs)) {
      problems.push(`✗ workspace 命令覆盖：宿主文件不存在：${rel}`);
      continue;
    }
    const source = readFileSync(abs, "utf8");
    const isTsHost = rel.endsWith(".ts");
    const text = isTsHost ? source : maskShellQuoted(source);
    let hits = isTsHost ? checkTsCargoArrays(rel, source, problems) : 0;
    text.split("\n").forEach((line, i) => {
      const trimmed = line.trim();
      if (trimmed === "" || trimmed.startsWith("#") || isTsCommentLine(trimmed)) return;
      const re = /\bcargo\s+(clippy|test|fmt)\b/g;
      let m: RegExpExecArray | null;
      while ((m = re.exec(line))) {
        hits += 1;
        const rest = line.slice(m.index);
        const end = rest.search(/&&|;|\|/);
        const segment = end === -1 ? rest : rest.slice(0, end);
        if (WORKSPACE_SCOPE_PATTERN.test(segment) || ALL_SCOPE_PATTERN.test(segment)) continue;
        problems.push(
          `✗ workspace 命令覆盖：${rel}:${i + 1} cargo ${m[1]} 缺 --workspace` +
            "（非虚拟 workspace 下默认只作用于根包，会静默漏检成员 crate）\n" +
            `    ${line.trim()}`,
        );
      }
    });
    if (hits === 0) {
      problems.push(
        `✗ workspace 命令覆盖：${rel} 未发现任何命令位置上的 cargo 命令` +
          "（引号内的说明文字不算命令）——拒绝以空集假绿，命令被 echo/注释/删除即红",
      );
    }
  }

  const gatedDecls = [
    {
      file: join(srcTauriDir, INFRA_SRC_REL, "lib.rs"),
      re: /^\s*pub\s+mod\s+test_utils\s*;/,
      label: "pub mod test_utils;",
      gate: "test_utils 生产编译门",
      src: "ADR-0111 决策 5 / issue #1132",
      productionArtifact: "测试器具",
    },
    {
      file: join(srcTauriDir, "src", "api_server", "handlers", "import.rs"),
      re: /^\s*pub\s+const\s+INVESTMENT_SECTION_HEADERS\b/,
      label: "pub const INVESTMENT_SECTION_HEADERS",
      gate: "投资五节锚点生产编译门",
      src: "issue #1185",
      productionArtifact: "测试锚点",
    },
    {
      file: join(srcTauriDir, "src", "api_server", "mod.rs"),
      re: /^\s*pub\s+use\s+handlers::import::INVESTMENT_SECTION_HEADERS\s*;/,
      label: "pub use handlers::import::INVESTMENT_SECTION_HEADERS;",
      gate: "投资五节锚点生产编译门",
      src: "issue #1185",
      productionArtifact: "测试锚点",
    },
  ];
  for (const { file, re, label, gate, src, productionArtifact } of gatedDecls) {
    const rel = file.slice(srcTauriDir.length + 1);
    if (!existsSync(file)) {
      problems.push(`✗ ${gate}：${rel} 不存在，无法核对 cfg 门（${src}）`);
      continue;
    }
    const lines = readFileSync(file, "utf8").split("\n");
    const declIndex = lines.findIndex((l) => re.test(l));
    if (declIndex === -1) {
      problems.push(`✗ ${gate}：${rel} 找不到 \`${label}\` 声明`);
    } else if (!hasTestAllowingCfgGate(lines, declIndex)) {
      problems.push(
        `✗ ${gate}：${rel} \`${label}\` 未加「放行测试」cfg 门\n` +
          `    ${lines[declIndex].trim()}\n` +
          '    门须为 `#[cfg(any(test, feature = "test-utils"))]`（或等价 cfg，支持 rustfmt 拆行的多行属性）；' +
          `无门 / \`#[cfg(not(test))]\` / 与测试无关的 cfg 都会让生产编译${productionArtifact}` +
          `（${src}），删除或写反 cfg 门即变红`,
      );
    }
  }

  const prodEnableLine = productionLedgerInfraEnablement(rootManifest, "test-utils");
  if (prodEnableLine !== null) {
    problems.push(
      "✗ test_utils 生产编译门：根包生产依赖 ledger-infra 启用了 test-utils\n" +
        `    ${prodEnableLine}\n` +
        "    test-utils 只许经测试目标（dev-dependency / cfg(test)）启用；" +
        "生产依赖启用即把测试器具编入生产构建（ADR-0111 决策 5 / issue #1132），删除该 feature 即变红",
    );
  }

  const infraManifestPath = join(srcTauriDir, dirname(INFRA_SRC_REL), "Cargo.toml");
  const infraManifest = existsSync(infraManifestPath)
    ? readFileSync(infraManifestPath, "utf8")
    : null;
  if (infraManifest === null) {
    problems.push(`✗ 生产编译 feature 门：${dirname(INFRA_SRC_REL)}/Cargo.toml 不存在`);
  }
  const defaultFeatureManifests: ReadonlyArray<readonly [string, string]> = [
    [rootManifest, "src-tauri/Cargo.toml"],
    ...(infraManifest !== null
      ? [[infraManifest, `${dirname(INFRA_SRC_REL)}/Cargo.toml`] as const]
      : []),
  ];
  for (const [manifest, where] of defaultFeatureManifests) {
    if (defaultFeaturesInclude(manifest, "test-utils")) {
      problems.push(
        `✗ test_utils 生产编译门：${where} [features] default 包含 test-utils\n` +
          "    默认 feature 在生产构建启用，等价于把测试器具编入生产构建" +
          "（ADR-0111 决策 5 / issue #1132），从 default 移除 test-utils 即变红",
      );
    }
  }

  const httpGateLabel = "http 投影 feature 门";
  if (infraManifest !== null) {
    const axumLine = infraManifest.split("\n").find((l) => /^\s*axum\s*=/.test(l));
    if (axumLine === undefined) {
      problems.push(`✗ ${httpGateLabel}：infra Cargo.toml 找不到 axum 依赖声明`);
    } else if (!axumLine.includes("optional = true")) {
      problems.push(
        `✗ ${httpGateLabel}：infra Cargo.toml 的 axum 依赖未声明 optional\n` +
          `    ${axumLine.trim()}\n` +
          "    非 optional 即无条件编入 axum 及其传递依赖（ADR-0111 决策 5 / issue #1133），加回 optional 即变绿",
      );
    }
    const httpFeatureLine = manifestSection(infraManifest, "features")
      ?.split("\n")
      .find((l) => /^\s*http\s*=/.test(l));
    if (httpFeatureLine === undefined || !httpFeatureLine.includes("dep:axum")) {
      problems.push(
        `✗ ${httpGateLabel}：infra Cargo.toml [features] 缺 \`http = ["dep:axum"]\`\n` +
          "    门与依赖面绑死——feature 不转发 dep:axum 即门形同虚设" +
          "（ADR-0111 决策 5 / issue #1133）",
      );
    }
  }
  for (const [manifest, where] of defaultFeatureManifests) {
    if (defaultFeaturesInclude(manifest, "http")) {
      problems.push(
        `✗ ${httpGateLabel}：${where} [features] default 包含 http\n` +
          "    默认 feature 在生产构建启用，等价于无条件编入 axum" +
          "（ADR-0111 决策 5 / issue #1133），从 default 移除 http 即变红",
      );
    }
  }

  const errorRsPath = join(srcTauriDir, INFRA_SRC_REL, "error.rs");
  if (!existsSync(errorRsPath)) {
    problems.push(
      `✗ ${httpGateLabel}：${INFRA_SRC_REL}/error.rs 不存在，无法核对 impl cfg 门（issue #1133）`,
    );
  } else {
    const lines = readFileSync(errorRsPath, "utf8").split("\n");
    const declIndex = lines.findIndex((l) =>
      /^\s*impl\s+axum::response::IntoResponse\s+for\s+AppError\b/.test(l),
    );
    if (declIndex === -1) {
      problems.push(
        `✗ ${httpGateLabel}：error.rs 找不到 \`impl axum::response::IntoResponse for AppError\``,
      );
    } else if (!hasHttpFeatureCfgGate(lines, declIndex)) {
      problems.push(
        `✗ ${httpGateLabel}：error.rs IntoResponse impl 未加 feature cfg 门\n` +
          `    ${lines[declIndex].trim()}\n` +
          '    门须为 `#[cfg(feature = "http")]`（紧贴 impl 的属性链）；无门即无条件编译 axum 投影' +
          "（ADR-0111 决策 5 / issue #1133），补回 cfg 门即变绿",
      );
    }
  }

  for (const crateDir of memberCrateDirs(srcTauriDir)) {
    if (crateDir === dirname(INFRA_SRC_REL)) continue;
    const memberManifest = readFileSync(join(srcTauriDir, crateDir, "Cargo.toml"), "utf8");
    const enableLine = productionLedgerInfraEnablement(memberManifest, "http");
    if (enableLine !== null) {
      problems.push(
        `✗ ${httpGateLabel}：域侧成员 ${crateDir} 生产依赖 ledger-infra 启用了 http\n` +
          `    ${enableLine}\n` +
          "    http 只许壳侧启用；域侧启用即把 axum 编入域依赖图" +
          "（ADR-0111 决策 5 / issue #1133），移除该 feature 即变红",
      );
    }
    if (declaredProductionDependencyNames(memberManifest).includes("axum")) {
      problems.push(
        `✗ ${httpGateLabel}：域侧成员 ${crateDir} 生产依赖直接声明 axum\n` +
          "    域侧依赖不引入 axum——axum 只有壳层需要（ADR-0111 决策 5 / issue #1133），" +
          "移除该依赖即变红",
      );
    }
  }

  return problems;
}

const MODULE_PROJECTION_RULE =
  "模块面 expected 由 crate 根 lib.rs 的 mod 声明投影（ADR-0056 决策 4 修订注记 / #1595）";

function diskModuleKeys(srcDir: string): string[] {
  const keys: string[] = [];
  for (const entry of readDirEntries(srcDir)) {
    if (
      entry.isFile &&
      hasExtension(entry.name, RUST_EXTENSIONS) &&
      entry.name !== "lib.rs" &&
      !isTestFile(entry.name)
    ) {
      keys.push(entry.name.replace(/\.rs$/, ""));
    } else if (
      entry.isDirectory &&
      collectRustFiles(join(srcDir, entry.name), entry.name).length > 0
    ) {
      keys.push(entry.name);
    }
  }
  return keys.sort();
}

function deriveCrateModules(
  target: CrateModuleTarget,
  srcTauriDir: string,
  problems: string[],
): string[] {
  const srcDir = join(srcTauriDir, target.srcRel);
  const libRsRel = `${target.srcRel}/lib.rs`;
  if (!existsSync(srcDir) || !existsSync(join(srcTauriDir, libRsRel))) {
    problems.push(
      `✗ 找不到 crate 根声明文件：${libRsRel}（${target.crate}）\n` +
        `    ${MODULE_PROJECTION_RULE}：声明文件缺失即无法派生 expected，fail loud`,
    );
    return [];
  }
  const scan = scanModDeclarations(readFileSync(join(srcTauriDir, libRsRel), "utf8"));
  for (const v of scan.violations) {
    problems.push(
      `✗ 认不出的 lib.rs 声明形状：${libRsRel}:${v.line}（${v.shape}）\n` +
        "    mod 扫描形状判定表（ADR-0056 决策 4 修订注记 / ADR-0113 决策 7 / ADR-0111 决策 5）：" +
        "lint 属性（allow/warn/deny/forbid/expect）、doc 属性与 #[cfg] / #[cfg_attr] 门控 mod 计入 expected；" +
        "#[cfg(test)] mod tests; 豁免；#[path]（含 cfg_attr 夹带 path）/ include! / 内联模块块 / " +
        "上述之外认不出的属性链 fail loud——按上表改写声明，或把新形态登记进形状判定表",
    );
  }
  const keys = scan.modules;
  const entries: string[] = [];
  for (const key of new Set(keys)) {
    const file = `${key}.rs`;
    const hasFile = existsSync(join(srcDir, file));
    const hasDirCode =
      existsSync(join(srcDir, key)) && collectRustFiles(join(srcDir, key), key).length > 0;
    if (hasFile) entries.push(file);
    if (hasDirCode) entries.push(key);
    if (!hasFile && !hasDirCode) {
      problems.push(
        `✗ 声明缺失：${libRsRel} 声明 \`mod ${key};\`，磁盘上既无 ${file} 也无含非测试文件的 ${key}/（${target.srcRel}）\n` +
          `    模块面与声明双向核对（${MODULE_PROJECTION_RULE}）：声明即 rustc 契约，` +
          "补齐模块文件/目录或删除声明",
      );
    }
  }
  for (const key of diskModuleKeys(srcDir)) {
    if (!keys.includes(key)) {
      problems.push(
        `✗ 未声明模块：${key}（${target.srcRel}）\n` +
          `    ${MODULE_PROJECTION_RULE}：` +
          `新增模块须先在 ${libRsRel} 声明 \`mod ${key};\`——磁盘上多出的生产模块即红（孤儿文件/目录）`,
      );
    }
  }
  return entries;
}

interface ModDeclScan {
  modules: string[];
  violations: { line: number; shape: string }[];
}

function isRecognizedModAttr(attr: string): boolean {
  if (!/^#\[\s*(?:cfg|cfg_attr|allow|warn|deny|forbid|expect|doc)\b/.test(attr)) return false;
  if (/^#\[\s*cfg_attr\b/.test(attr) && /\bpath\b/.test(attr)) return false;
  return true;
}

function attributeChainBefore(text: string, boundaryIdx: number): string[] {
  const attrs: string[] = [];
  let i = boundaryIdx - 1;
  while (i >= 0) {
    while (i >= 0 && /\s/.test(text[i])) i--;
    if (i < 0 || text[i] !== "]") break;
    let depth = 0;
    let open = -1;
    for (let j = i; j >= 0; j--) {
      if (text[j] === "]") depth++;
      else if (text[j] === "[") {
        depth--;
        if (depth === 0) {
          open = j;
          break;
        }
      }
    }
    if (open <= 0 || text[open - 1] !== "#") break;
    attrs.unshift(text.slice(open - 1, i + 1));
    i = open - 2;
  }
  return attrs;
}

function stripLeadingAttrs(text: string): string {
  let s = text;
  while (/^\s*#\[/.test(s)) {
    const open = s.indexOf("[");
    let depth = 0;
    let end = -1;
    for (let i = open; i < s.length; i++) {
      if (s[i] === "[") depth++;
      else if (s[i] === "]") {
        depth--;
        if (depth === 0) {
          end = i;
          break;
        }
      }
    }
    if (end === -1) break;
    s = s.slice(end + 1);
  }
  return s;
}

function scanModDeclarations(source: string): ModDeclScan {
  const keep = maskNonCode(source, true);
  const code = maskNonCode(source, false);
  const lineOf = (idx: number): number => lineAt(code, idx);
  const violations: { line: number; shape: string }[] = [];
  const modules: string[] = [];

  for (const m of code.matchAll(/\binclude\s*!/g)) {
    violations.push({ line: lineOf(m.index ?? 0), shape: "include!(…)" });
  }

  const declRe = /\bmod\s+(?:r#)?([A-Za-z_][A-Za-z0-9_]*)\s*([;{])/g;
  for (const m of code.matchAll(declRe)) {
    const idx = m.index ?? 0;
    const line = lineOf(idx);
    const name = m[1];
    const lineStart = code.lastIndexOf("\n", idx - 1) + 1;
    const prefix = code.slice(lineStart, idx);
    if (!/^\s*(?:pub(?:\s*\([^)]*\))?\s+)?$/.test(stripLeadingAttrs(prefix))) {
      violations.push({
        line,
        shape: `认不出的声明前缀：${source.slice(lineStart, idx + m[0].length).trim()}`,
      });
      continue;
    }
    if (m[2] === "{") {
      violations.push({ line, shape: `内联模块块 mod ${name} { … }` });
      continue;
    }
    const attrs = attributeChainBefore(keep, idx);
    const path = attrs.find((a) => /^#\[\s*path\b/.test(a));
    if (path) {
      violations.push({ line, shape: `${path} mod ${name};` });
      continue;
    }
    const unknown = attrs.find((a) => !isRecognizedModAttr(a));
    if (unknown) {
      violations.push({ line, shape: `${unknown} mod ${name};` });
      continue;
    }
    if (name === "tests") continue;
    modules.push(name);
  }
  return { modules, violations };
}

function scanTransactionZoneRefs(text: string): ScanHit[] {
  const masked = maskNonCode(text);
  const rawLines = text.split("\n");
  const hits: ScanHit[] = [];
  const lineOf = (index: number): number => lineAt(masked, index);
  const push = (index: number, match: string, captured: string): void => {
    const line = lineOf(index);
    hits.push({ line, text: (rawLines[line - 1] ?? "").trim(), match, captured });
  };
  const re = /\b(?:super|crate)\s*::\s*(\{)?/g;
  for (const m of masked.matchAll(re)) {
    const start = m.index ?? 0;
    const after = start + m[0].length;
    if (m[1] === undefined) {
      const segment = /^([A-Za-z_][A-Za-z0-9_]*)/.exec(masked.slice(after));
      if (segment) push(start, m[0] + segment[1], segment[1]);
      continue;
    }
    let depth = 1;
    let close = -1;
    for (let i = after; i < masked.length; i++) {
      if (masked[i] === "{") depth++;
      else if (masked[i] === "}") {
        depth--;
        if (depth === 0) {
          close = i;
          break;
        }
      }
    }
    const body = masked.slice(after, close === -1 ? masked.length : close);
    for (const entry of disallowedBraceEntries(body)) {
      push(start, `{…${entry}…}`, entry);
    }
  }
  return hits;
}

function checkTransactionZoneDirection(srcTauriDir: string): string[] {
  const problems: string[] = [];
  const target = CRATE_MODULE_TARGETS.find((t) => t.crate === "ledger-transaction");
  if (target === undefined) {
    return [
      "✗ 交易域区级层序：CRATES 政策核缺 ledger-transaction 条目，无法定位模块根" +
        "（规则见 ADR-0113 决策 3 / #1181）",
    ];
  }
  const srcDir = join(srcTauriDir, target.srcRel);
  if (!existsSync(srcDir)) return problems;
  for (const f of collectRustFiles(srcDir, "")) {
    const rel = f.rel;
    if (rel === "lib.rs") continue;
    const isRootFile = !rel.includes("/");
    const zoneDir = isRootFile ? rel : (rel.split("/")[0] as string);
    const zone = TRANSACTION_ZONE_BY_DIR[zoneDir];
    if (zone === undefined) {
      problems.push(
        isRootFile
          ? `✗ 根级单文件必须进区目录：${rel}（${target.srcRel}）\n` +
              "    区归属的登记面是目录约定（ADR-0113 决策 2 / #1597）：zone = crate 顶层区目录，" +
              "根级单文件无区目录可投影，不允许「默认为共享语义」——那会给写路径模块留静默错分类的洞；" +
              "把文件移入区目录并同步 lib.rs 声明"
          : `✗ 区目录未登记区归属：${zoneDir}/（${rel}）\n` +
              "    zone 靠路径投影（ADR-0113 决策 2 / #1597）：crate 顶层目录须在 " +
              "TRANSACTION_ZONE_BY_DIR 登记区归属，未登记即无法判向、区级层序静默失靶；" +
              "登记区归属，或把模块并入既有区目录",
      );
      continue;
    }
    const source = readFileSync(f.abs, "utf8");
    for (const hit of scanTransactionZoneRefs(source)) {
      const captured = hit.captured;
      if (captured === undefined) continue;
      const targetZone = TRANSACTION_ZONE_BY_DIR[captured];
      if (targetZone === undefined || targetZone === zone) continue;
      if (TRANSACTION_ZONE_RANK[zone] > TRANSACTION_ZONE_RANK[targetZone]) continue;
      const allowed = TRANSACTION_ZONE_ALLOWED_EDGES.some(
        (e) => e.dir === zoneDir && e.target === captured,
      );
      if (allowed) continue;
      problems.push(
        `✗ 区级反向依赖：${zone}「${zoneDir}/」引用 ${targetZone}「${captured}/」 → ` +
          `${rel}:${hit.line}（${hit.match}）\n` +
          `    ${hit.text}\n` +
          `    区级层序唯一：写路径/读路径 → 跨域接缝 → 共享语义（规则见 ADR-0113 决策 3）——` +
          `共享语义不得依赖接缝与路径区，接缝不得依赖路径区，写读两径互不依赖；` +
          `区归属按目录约定投影（TRANSACTION_ZONE_BY_DIR，ADR-0113 决策 2 / #1597）；` +
          `设计意图边须逐条留痕于本脚本 TRANSACTION_ZONE_ALLOWED_EDGES（附 ADR 指针），` +
          `或把依赖下沉到层序更低的区`,
      );
    }
  }
  return problems;
}

function checkConvertCurrentCallers(srcDir: string, srcTauriDir: string): string[] {
  const problems: string[] = [];
  let files: WalkedFile[] = [];
  try {
    files = [
      ...collectRustFiles(srcDir, "src"),
      ...CRATE_MODULE_TARGETS.flatMap((target) =>
        collectRustFiles(join(srcTauriDir, target.srcRel), target.srcRel),
      ),
    ];
  } catch {}
  const hitsByFile = new Map<string, ScanHit[]>();
  for (const f of files) {
    const hits = convertCurrentCallHits(readFileSync(f.abs, "utf8"));
    if (hits.length > 0) hitsByFile.set(f.rel, hits);
  }
  for (const [rel, hits] of hitsByFile) {
    if (CONVERT_CURRENT_CALLERS.some((entry) => entry.file === rel)) continue;
    for (const hit of hits) {
      problems.push(
        `✗ 当期入口调用方闭集：${rel}:${hit.line}（${CONVERT_CURRENT_CALL_SYMBOL} 调用）\n` +
          `    ${hit.text}\n` +
          `    写路径全部走交易日历史折算、当期表只服务读路径（规则见 ADR-0011 决策 3 / 2026-09-22 修订 ①，#1692）——` +
          `当期入口生产调用方白名单外即红；读路径消费面须逐条留痕于本脚本 CONVERT_CURRENT_CALLERS（附成因），` +
          `写路径改接 convert_to_native_on_trade_date / convert_to_native_on_edit，零腿例外见 NativeConversion::zero_cash_leg`,
      );
    }
  }
  for (const entry of CONVERT_CURRENT_CALLERS) {
    if (!files.some((f) => f.rel === entry.file)) {
      problems.push(
        `✗ 当期入口调用方闭集：在册成员文件不存在：${entry.file}（相对 src-tauri）\n` +
          `    清单漂移 fail loud（规则见 ADR-0011 决策 3 / 2026-09-22 修订 ①，#1692）：` +
          `文件移动 / 删除后须同步本脚本 CONVERT_CURRENT_CALLERS`,
      );
      continue;
    }
    if (!hitsByFile.has(entry.file)) {
      problems.push(
        `✗ 当期入口调用方闭集：在册成员零调用：${entry.file}（相对 src-tauri）\n` +
          `    文件存在但扫不到 ${CONVERT_CURRENT_CALL_SYMBOL} 生产调用——拒绝空集假绿` +
          `（规则见 ADR-0011 决策 3 / 2026-09-22 修订 ①，#1692）：调用已移除即从 CONVERT_CURRENT_CALLERS 摘除该成员`,
      );
    }
  }
  return problems;
}

function scanModuleFile(
  file: WalkedFile,
  layer: Layer,
  problems: string[],
  options: { scanShellRefs: boolean; context: string },
): void {
  const source = readFileSync(file.abs, "utf8");
  if (options.scanShellRefs) {
    for (const hit of scanRustSource(source)) {
      problems.push(
        `✗ 反向依赖：${file.rel}:${hit.line}（${options.context}）引用壳层（${hit.match}）\n` +
          `    ${hit.text}\n` +
          `    分层规则：壳 → 域 → 基础设施，域永不依赖壳（规则见 ADR-0056 决策 4）——` +
          `被依赖逻辑应下沉到域目录或基础设施`,
      );
    }
  }
  if (layer === LAYER.INFRA) {
    const block = file.rel.split("/")[0].replace(/\.rs$/, "");
    const forbiddenTargets = INFRA_BLOCK_FORBIDDEN[block];
    if (forbiddenTargets) {
      for (const hit of scanRustSource(source, infraBlockDepPattern(forbiddenTargets))) {
        const allowed = INFRA_BLOCK_ALLOWED_EDGES.some(
          (e) => e.file === file.rel && e.target === hit.captured,
        );
        if (allowed) continue;
        problems.push(
          `✗ crate 内反向依赖：${block} 引用 ${hit.captured} → ${file.rel}:${hit.line}（${hit.match}）\n` +
            `    ${hit.text}\n` +
            `    crate 内分层（规则见 ADR-0111 决策 4）：原语 ← db ← boot 单向，db 不得引用 boot/signals——` +
            `设计意图边须逐条留痕于本脚本 INFRA_BLOCK_ALLOWED_EDGES（附 ADR 指针），或把逻辑下沉到更低的块`,
        );
      }
    }
  }
}

function scanModuleEntries(
  entries: readonly WhitelistEntry[],
  baseDir: string,
  problems: string[],
  options: { scanShellRefs: boolean; context: string },
): number {
  let scanned = 0;
  for (const w of entries) {
    const abs = join(baseDir, w.path);
    let stat: Stats | undefined;
    try {
      stat = statSync(abs);
    } catch {
      stat = undefined;
    }
    if (!stat) {
      problems.push(
        `✗ 条目路径不存在：${w.path}（${options.context}）——` +
          `目录改名/迁移后未同步政策项或声明面（规则见 ADR-0056 决策 4 修订注记 / #1595）`,
      );
      continue;
    }
    const files: WalkedFile[] = stat.isDirectory()
      ? collectRustFiles(abs, w.path)
      : [{ abs, rel: w.path }];
    if (files.length === 0) {
      problems.push(
        `✗ 条目扫不到非测试 Rust 文件：${w.path}（${options.context}）——` +
          `全部是测试豁免形态或已空（规则见 ADR-0056 决策 5）`,
      );
      continue;
    }
    scanned += files.length;
    for (const f of files) scanModuleFile(f, w.layer, problems, options);
  }
  return scanned;
}

function scanCrateModules(
  target: CrateModuleTarget,
  derived: readonly string[],
  srcTauriDir: string,
  problems: string[],
): number {
  const entries: WhitelistEntry[] = derived.map((path) => ({
    path,
    layer: target.layer,
  }));
  return scanModuleEntries(entries, join(srcTauriDir, target.srcRel), problems, {
    scanShellRefs: false,
    context: `${target.crate} / ${target.layer}`,
  });
}

function main(): void {
  const repoRoot = fileURLToPath(new URL("..", import.meta.url));
  const srcDir = process.argv[2] ?? join(repoRoot, "src-tauri", "src");
  const srcTauriDir = process.argv[3] ?? join(repoRoot, "src-tauri");
  const problems: string[] = [];
  let scannedFiles = 0;
  const domainCount = WHITELIST.filter((w) => w.layer === LAYER.DOMAIN).length;

  let allFiles: WalkedFile[] = [];
  try {
    allFiles = [
      ...collectRustFiles(srcDir, ""),
      ...CRATE_MODULE_TARGETS.flatMap((target) =>
        collectRustFiles(join(srcTauriDir, target.srcRel), target.srcRel),
      ),
    ];
  } catch {}
  for (const f of allFiles) {
    const source = readFileSync(f.abs, "utf8");
    for (const hit of scanRustSource(source, GLOBAL_MODEL_PATH_PATTERN)) {
      problems.push(
        `✗ 全局模型路径残留：${f.rel}:${hit.line}（${hit.match}）\n` +
          `    ${hit.text}\n` +
          `    全局模型目录已随 ADR-0059 模型域化消亡（T7 / #424），` +
          `模型类型一律走域路径显式 import（如 crate::transaction::model::Transaction）` +
          `——防扁平命名空间复活`,
      );
    }
    for (const hit of scanRustSource(source, MODEL_GLOB_REEXPORT_PATTERN)) {
      problems.push(
        `✗ 域模型 glob 再导出：${f.rel}:${hit.line}（${hit.match}）\n` +
          `    ${hit.text}\n` +
          `    域 model 只许逐类型再导出，所有权必须逐类型可见 ` +
          `（ADR-0059 决策 3/6，#424）：改为 pub use model::{TypeA, TypeB} 形态`,
      );
    }
    if (isModelFile(f.rel)) {
      for (const hit of scanRustSource(source, MODEL_FILE_GLOB_PATTERN)) {
        problems.push(
          `✗ 域模型文件内 glob 聚合：${f.rel}:${hit.line}（${hit.match}）\n` +
            `    ${hit.text}\n` +
            `    域模型文件只承载本域类型定义与逐类型再导出，禁止 glob 聚合 ` +
            `（ADR-0059 决策 3/6，#424）`,
        );
      }
    }
    for (const hit of scanRustSource(source, NATIVE_TX_STMT_PATTERN, true)) {
      if (f.rel === NATIVE_TX_STMT_ALLOWED) continue;
      problems.push(
        `✗ 原生事务语句：${f.rel}:${hit.line}（${hit.match}）\n` +
          `    ${hit.text}\n` +
          `    事务壳归基础设施 db::tx_scope（无条件自持 hold_transaction / ` +
          `嵌套感知 ensure_transaction，ADR-0056 / ADR-0105；#1013/#1014）——` +
          `产品代码不得手写 BEGIN/COMMIT/ROLLBACK，唯一合法住址 ${NATIVE_TX_STMT_ALLOWED}`,
      );
    }
  }

  scannedFiles += scanModuleEntries(WHITELIST, srcDir, problems, {
    scanShellRefs: true,
    context: "白名单政策项（根 src）",
  });
  for (const target of CRATE_MODULE_TARGETS) {
    const derived = deriveCrateModules(target, srcTauriDir, problems);
    scannedFiles += scanCrateModules(target, derived, srcTauriDir, problems);
  }

  if (scannedFiles === 0) {
    problems.push(
      "✗ 全部白名单条目扫不到任何非测试 Rust 文件——src 目录指错或白名单整体漂移，拒绝以空集假绿通过",
    );
  }

  problems.push(...checkCrateBoundaries(srcTauriDir));

  problems.push(...checkTransactionZoneDirection(srcTauriDir));

  problems.push(...checkConvertCurrentCallers(srcDir, srcTauriDir));

  if (problems.length > 0) {
    for (const p of problems) console.error(p);
    console.error(
      `❌ 结构守门失败：${problems.length} 处问题` +
        `（分层规则：壳 → 域 → 基础设施，域永不依赖壳，白名单即规格，见 ADR-0056）`,
    );
    process.exit(1);
  }
  console.log(
    `✓ 结构守门：白名单 ${WHITELIST.length} 项（域目录 ${domainCount}）` +
      `· 白名单面非测试文件 ${scannedFiles} 个 · 对壳层零依赖` +
      `· crate 模块面 ${CRATE_MODULE_TARGETS.length} 份（expected 由 crate 根 lib.rs 的 mod 声明投影 + 磁盘双向全等，#1595）` +
      `· 跨 crate 依赖方向：声明面判定（层秩 + 业务域→多端同步域禁令），未声明依赖归编译期（#1596 / ADR-0071·ADR-0101 修订注记）` +
      `· 基础设施 dev-dependency 方向零未留痕声明（认许边 ${INFRA_DOMAIN_ALLOWED_EDGES.length} 条，ADR-0071 决策 6）` +
      `· 模型域化禁令全树扫描 ${allFiles.length} 个文件零残留（ADR-0059）` +
      `· 原生事务语句全树扫描 ${allFiles.length} 个文件仅 ${NATIVE_TX_STMT_ALLOWED} 一处（#1014）` +
      `· crate 边界 ${CRATES.length} 个（成员登记 / 门禁继承 / 依赖方向 / workspace 命令覆盖，#1087）` +
      `· crate 内块间反向依赖零未认许引用（认许边 ${INFRA_BLOCK_ALLOWED_EDGES.length} 条，ADR-0111 决策 4 / #1134）` +
      `· 交易域区级层序零未认许反向引用（写读 → 接缝 → 共享语义，认许边 ${TRANSACTION_ZONE_ALLOWED_EDGES.length} 条，ADR-0113 决策 3 / #1181）` +
      `· 当期入口调用方闭集 ${CONVERT_CURRENT_CALLERS.length} 成员全在册（白名单外即红，ADR-0011 / #1692）` +
      `· test_utils 生产编译门（cfg 门 + 生产依赖不启用 test-utils，#1132）` +
      `· 投资五节锚点生产编译门（cfg 门，#1185）` +
      `· http 投影 feature 门（axum optional + impl cfg 门 + default 不含 http + 域侧不启用，#1133）`,
  );
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main();
}
