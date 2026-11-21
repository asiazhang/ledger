#!/usr/bin/env bun

import { readFileSync } from "node:fs";
import { join } from "node:path";
import { pathToFileURL, fileURLToPath } from "node:url";
import { maskNonCode, RUST_EXTENSIONS, walkTextFiles, type WalkedFile } from "./gate-primitives.ts";

export const ORCHESTRATOR_FILE = "src/lib.rs";
export const ORCHESTRATOR_FN = "start_background_services";

const PAIRED_NAMES = [
  "start_scheduler",
  "start_triggers",
  "start_history_backfill",
  "start_daily_price_refresh",
  "start_daily_fx_sync",
  "start_auto_update_check",
] as const;

export interface GuardedName {
  name: string;
  wholeFile: readonly string[];
  orchestratorBodyAllowed: boolean;
  note: string;
}

export const GUARDED_NAMES: readonly GuardedName[] = [
  {
    name: "start_scheduler",
    wholeFile: ["crates/backup/src/auto.rs", "crates/backup/src/lib.rs"],
    orchestratorBodyAllowed: true,
    note: "自动备份调度入口（ledger-backup crate 定义 + crate 根再导出，#1091）",
  },
  {
    name: "start_triggers",
    wholeFile: [
      "crates/sync-engine/src/trigger/scheduler.rs",
      "crates/sync-engine/src/trigger/mod.rs",
      "crates/sync-engine/src/lib.rs",
    ],
    orchestratorBodyAllowed: true,
    note: "同步触发编排单一入口（分平台门住址，ADR-0098 决策 4；issue #1107 拆 crate 后定义住 crates/sync-engine/src/trigger/scheduler.rs，crate 根再导出）",
  },
  {
    name: "start_sync_scheduler",
    wholeFile: [
      "crates/sync-engine/src/trigger/scheduler.rs",
      "crates/sync-engine/src/trigger/mod.rs",
      "crates/sync-engine/src/lib.rs",
    ],
    orchestratorBodyAllowed: false,
    note: "桌面轮询线程拉起（仅 start_triggers 域内消费；直接调用即绕过分平台门，#863 缺陷 1 形态）",
  },
  {
    name: "start_history_backfill",
    wholeFile: ["crates/market-sync/src/history.rs", "crates/market-sync/src/lib.rs"],
    orchestratorBodyAllowed: true,
    note: "价格历史后台补全调度入口（ADR-0122 / issue #1375；定义住 ledger-market-sync crate 的 history.rs，crate 根再导出）",
  },
  {
    name: "start_daily_price_refresh",
    wholeFile: ["crates/market-sync/src/daily_refresh.rs", "crates/market-sync/src/lib.rs"],
    orchestratorBodyAllowed: true,
    note: "后台每日现价刷新调度入口（ADR-0122 决策 3 / issue #1377；定义住 ledger-market-sync crate 的 daily_refresh.rs，crate 根再导出）",
  },
  {
    name: "start_daily_fx_sync",
    wholeFile: ["crates/market-sync/src/fx_daily.rs", "crates/market-sync/src/lib.rs"],
    orchestratorBodyAllowed: true,
    note: "后台每日汇率增量同步调度入口（ADR-0019 修订记录 / issue #1546；定义住 ledger-market-sync crate 的 fx_daily.rs，crate 根再导出）",
  },
  {
    name: "start_auto_update_check",
    wholeFile: ["src/app_update.rs"],
    orchestratorBodyAllowed: true,
    note: "应用更新启动延迟静默检查入口（ADR-0124 决策 4 / issue #1429；定义住壳层 app_update.rs）",
  },
];

export interface BootWiring {
  name: string;
  file: string;
  note: string;
}

export const MARKET_SYNC_SRC_REL = "crates/market-sync/src";
export const LANE_EXECUTOR_TOKEN = "tauri::async_runtime::spawn";
export const LANE_TIMER_TOKEN = "tokio::time::sleep";
export const LANE_BANNED_TOKENS = ["thread::spawn", "std::thread::sleep"] as const;
export const LANE_SCHEDULER_REL = "crates/market-sync/src/lane.rs";
export const LANE_WIRE_TOKEN = "start_daily_lane";
export const LANE_FILES = [
  "crates/market-sync/src/daily_refresh.rs",
  "crates/market-sync/src/history.rs",
  "crates/market-sync/src/fx_daily.rs",
] as const;

export const BOOT_WIRING: readonly BootWiring[] = [
  {
    name: "install_after_commit_hook",
    file: ORCHESTRATOR_FILE,
    note: "提交点后置动作注册：备份域实现接到基础设施注册点（spec #1086 / issue #1088，挂载点①）",
  },
  {
    name: "register_after_occurrence_hook",
    file: ORCHESTRATOR_FILE,
    note: "期次落账置脏注册：备份域实现（#1091 起 ledger-backup crate 的 occurrence_dirty_hook）接到定时计划域注册点（issue #1090；#1091 起实现住 crate）",
  },
  {
    name: "register_catch_up_hook",
    file: ORCHESTRATOR_FILE,
    note: "追补触发注册：定时计划域实现（auto_run::catch_up_hook）接到备份域注册点（issue #1091 挂载点④，ADR-0112 决策 5）",
  },
];

const GUARDED_NAME_PATTERN = new RegExp(
  `\\b(?:${GUARDED_NAMES.map((g) => g.name).join("|")})\\b`,
  "g",
);

function isTestFile(relPath: string): boolean {
  const segments = relPath.split("/");
  const file = segments[segments.length - 1];
  return file === "tests.rs" || segments.slice(0, -1).includes("tests");
}

const SKIP_DIRS: ReadonlySet<string> = new Set(["target"]);

function collectRustFiles(dir: string, relBase: string): WalkedFile[] {
  return walkTextFiles(dir, relBase, { extensions: RUST_EXTENSIONS, skipDirs: SKIP_DIRS }).filter(
    (f) => !isTestFile(f.rel),
  );
}

function orchestratorBodySpan(maskedLines: string[]): [number, number] | null {
  const fnLine = maskedLines.findIndex((l) => l.match(new RegExp(`fn\\s+${ORCHESTRATOR_FN}\\b`)));
  if (fnLine === -1) return null;
  for (let i = fnLine + 1; i < maskedLines.length; i++) {
    if (maskedLines[i].trim() === "}") return [fnLine + 1, i + 1];
  }
  return null;
}

interface NameHit {
  line: number;
  text: string;
  match: string;
}

function scanGuardedNames(rawLines: string[], maskedLines: string[]): NameHit[] {
  const hits: NameHit[] = [];
  const seen = new Set<string>();
  for (let i = 0; i < maskedLines.length; i++) {
    for (const m of maskedLines[i].matchAll(GUARDED_NAME_PATTERN)) {
      const key = `${i + 1}:${m[0]}`;
      if (seen.has(key)) continue;
      seen.add(key);
      hits.push({ line: i + 1, text: rawLines[i].trim(), match: m[0] });
    }
  }
  return hits;
}

function main(): void {
  const repoRoot = fileURLToPath(new URL("..", import.meta.url));
  const scanRoot = process.argv[2] ?? join(repoRoot, "src-tauri");
  const problems: string[] = [];

  for (const guarded of GUARDED_NAMES) {
    for (const relPath of guarded.wholeFile) {
      let source: string;
      try {
        source = readFileSync(join(scanRoot, relPath), "utf8");
      } catch {
        problems.push(
          `✗ 白名单条目缺失：${relPath}（${guarded.name} 的 ${guarded.note}）——文件不存在，清单漂移 fail loud`,
        );
        continue;
      }
      const hits = scanGuardedNames(source.split("\n"), maskNonCode(source).split("\n"));
      if (!hits.some((h) => h.match === guarded.name)) {
        problems.push(
          `✗ 白名单条目漂移：${relPath}（${guarded.note}）不再含标识符 \`${guarded.name}\`` +
            `——确认域入口改名/搬迁后同步更新本脚本白名单`,
        );
      }
    }
  }

  let orchestratorSpan: [number, number] | null = null;
  try {
    const libSource = readFileSync(join(scanRoot, ORCHESTRATOR_FILE), "utf8");
    const maskedLines = maskNonCode(libSource).split("\n");
    orchestratorSpan = orchestratorBodySpan(maskedLines);
    if (orchestratorSpan === null) {
      problems.push(
        `✗ 唯一编排点缺失：${ORCHESTRATOR_FILE} 内找不到 \`fn ${ORCHESTRATOR_FN}\`` +
          `——成对拉起的单点被删或改名，业务可用起点将退回「各写各的」（issue #961 根因复现）`,
      );
    } else {
      const bodyRaw = libSource.split("\n").slice(orchestratorSpan[0] - 1, orchestratorSpan[1]);
      const bodyMasked = maskedLines.slice(orchestratorSpan[0] - 1, orchestratorSpan[1]);
      const bodyHits = new Set(scanGuardedNames(bodyRaw, bodyMasked).map((h) => h.match));
      for (const name of PAIRED_NAMES) {
        if (!bodyHits.has(name)) {
          problems.push(
            `✗ 成组性破坏：${ORCHESTRATOR_FILE} 的 \`${ORCHESTRATOR_FN}\` 函数体内缺少 \`${name}\`` +
              `——后台服务只拉一半，缺失一侧的业务在本会话静默失效（#863 两次缺陷的形态）`,
          );
        }
      }
    }
  } catch {
    problems.push(`✗ 唯一编排点文件缺失：${ORCHESTRATOR_FILE}——src-tauri 根指错或壳层文件漂移`);
  }

  for (const wiring of BOOT_WIRING) {
    let source: string;
    try {
      source = readFileSync(join(scanRoot, wiring.file), "utf8");
    } catch {
      problems.push(
        `✗ 启动接线文件缺失：${wiring.file}（${wiring.note}）——壳层启动文件漂移，fail loud`,
      );
      continue;
    }
    if (!new RegExp(`\\b${wiring.name}\\b`).test(maskNonCode(source))) {
      problems.push(
        `✗ 启动接线缺失：${wiring.file} 内未接线 \`${wiring.name}\`（${wiring.note}）\n` +
          `    启动路径不被测试直接执行，删掉这行不会让任何断言变红——生产将静默丢` +
          `置脏/到期检查；请在壳层启动接线处恢复调用`,
      );
    }
  }

  const marketSyncSrcDir = join(scanRoot, MARKET_SYNC_SRC_REL);
  let laneFiles: WalkedFile[] = [];
  try {
    laneFiles = collectRustFiles(marketSyncSrcDir, MARKET_SYNC_SRC_REL);
  } catch {
    problems.push(
      `✗ 车道执行器扫描面不可达：${MARKET_SYNC_SRC_REL}——行情域 crate 目录漂移，拒绝以空集假绿通过`,
    );
  }
  let schedulerMasked: string | null = null;
  try {
    schedulerMasked = maskNonCode(readFileSync(join(scanRoot, LANE_SCHEDULER_REL), "utf8"));
  } catch {
    problems.push(
      `✗ 调度循环单点缺失：${LANE_SCHEDULER_REL}——三车道共用的巡检循环住址漂移，` +
        `确认收敛结构（issue #1622）后同步更新本脚本`,
    );
  }
  if (schedulerMasked !== null) {
    for (const token of [LANE_EXECUTOR_TOKEN, LANE_TIMER_TOKEN]) {
      if (!new RegExp(`\\b${token.replace(/::/g, "\\s*::\\s*")}\\b`).test(schedulerMasked)) {
        problems.push(
          `✗ 调度循环执行器接线缺失：${LANE_SCHEDULER_REL} 未以 \`${token}\` 表达` +
            (token === LANE_TIMER_TOKEN ? "异步定时（启动延迟/自然日窗口）" : "async 任务拉起") +
            `\n    调度循环必须是挂全局运行时的 async 任务（ADR-0125 决策 7 / issue #1413）；删掉接线不会让\n` +
            `    行为测试直接变红，故以源码扫描守门（#959 / #961 先例）`,
        );
      }
    }
  }
  for (const f of laneFiles) {
    const masked = maskNonCode(readFileSync(f.abs, "utf8"));
    const isLaneFile = (LANE_FILES as readonly string[]).includes(f.rel);
    if (
      isLaneFile &&
      !new RegExp(`\\b${LANE_WIRE_TOKEN.replace(/::/g, "\\s*::\\s*")}\\s*\\(`).test(masked)
    ) {
      problems.push(
        `✗ 车道调度接线缺失：${f.rel} 未调起共享调度循环 \`${LANE_WIRE_TOKEN}\`\n` +
          `    三条后台车道的巡检循环已收敛为域内单点（issue #1622）；车道模块自建平行循环\n` +
          `    （含 async 形态重写）即「规则改一处漏两处」回归，删掉接线即本守门红`,
      );
    }
    for (const token of LANE_BANNED_TOKENS) {
      if (new RegExp(`\\b${token.replace(/::/g, "\\s*::\\s*")}\\b`).test(masked)) {
        problems.push(
          `✗ 车道回归自建线程：${f.rel} 出现 \`${token}\`\n` +
            `    后台车道（价格历史补全 / 每日现价刷新 / 每日汇率增量同步）必须是挂全局运行时的 async 任务\n` +
            `   （tauri::async_runtime::spawn + tokio::time::sleep，ADR-0125 决策 7 / issue #1413）`,
        );
      }
    }
  }

  let files: WalkedFile[] = [];
  try {
    files = collectRustFiles(scanRoot, "");
  } catch {
    problems.push(`✗ 扫描根不可达：${scanRoot}——拒绝以空集假绿通过`);
  }
  if (files.length === 0) {
    problems.push(
      `✗ 扫描面提不出任何非测试 Rust 文件：${scanRoot}——src-tauri 根指错或源码整体` +
        `漂移，拒绝以空集假绿通过`,
    );
  }
  for (const f of files) {
    const source = readFileSync(f.abs, "utf8");
    const rawLines = source.split("\n");
    const maskedLines = maskNonCode(source).split("\n");
    for (const hit of scanGuardedNames(rawLines, maskedLines)) {
      const guarded = GUARDED_NAMES.find((g) => g.name === hit.match);
      if (guarded === undefined) continue;
      if (guarded.wholeFile.includes(f.rel)) continue;
      const inOrchestratorBody =
        f.rel === ORCHESTRATOR_FILE &&
        orchestratorSpan !== null &&
        hit.line >= orchestratorSpan[0] &&
        hit.line <= orchestratorSpan[1];
      if (inOrchestratorBody && guarded.orchestratorBodyAllowed) continue;
      problems.push(
        `✗ 后台服务入口的生产调用脱离唯一编排点：${f.rel}:${hit.line}（${hit.match}）\n` +
          `    ${hit.text}\n` +
          `    规则：\`${guarded.name}\`——${guarded.note}；\n` +
          `    自动备份与同步触发必须经 \`${ORCHESTRATOR_FN}\`（${ORCHESTRATOR_FILE}）成对拉起` +
          `（issue #961），新增业务可用起点请调用该编排点，不要单独调用任一域入口`,
      );
    }
  }

  if (problems.length > 0) {
    for (const p of problems) console.error(p);
    console.error(
      `❌ 后台服务成组拉起守门失败：${problems.length} 处问题` +
        `（单点编排 + 白名单即规格，见 issue #961 / ADR-0056 决策 4 哲学）`,
    );
    process.exit(1);
  }
  const pathCount = new Set(GUARDED_NAMES.flatMap((g) => [...g.wholeFile])).size;
  console.log(
    `✓ 后台服务成组拉起守门：受守入口 ${GUARDED_NAMES.length} 个` +
      `（${GUARDED_NAMES.map((g) => g.name).join(" / ")}）· 白名单路径 ${pathCount} 个（定义与再导出）` +
      ` · 生产调用收敛于 \`${ORCHESTRATOR_FN}\` 单点 · 启动接线 ${BOOT_WIRING.length} 项已接线（#1088）` +
      ` · 车道执行器守门：调度循环单点 ${LANE_SCHEDULER_REL}（异步执行器 + 异步定时，#1622）+ ` +
      `${LANE_FILES.length} 条车道接线（#1413；扫描 ${laneFiles.length} 个生产文件零自建线程）` +
      ` · src-tauri 全树（排除 target/）扫描 ${files.length} 个非测试文件零脱离`,
  );
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main();
}
