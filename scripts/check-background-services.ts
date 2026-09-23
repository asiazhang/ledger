#!/usr/bin/env bun
// 后台服务成组拉起守门（issue #961 / #1375 / #1546）：`backup::start_scheduler`（自动备份调度，
// 轮询同轮承载定时追补）、`sync_engine::start_triggers`（同步触发编排，分平台
// 门收在域内一处，ADR-0098 决策 4）、`market_sync::start_history_backfill`
//（价格历史后台补全，ADR-0122 / issue #1375）、`market_sync::start_daily_price_refresh`
//（每日现价刷新，ADR-0122 决策 3 / issue #1377）与 `market_sync::start_daily_fx_sync`
//（每日汇率增量同步，ADR-0019 修订记录 / issue #1546）在**所有业务可用起点**
// 必须成组拉起。
// 各调用独立书写时无任何机制保证成组——#863 会话已由同一根因造成两次
// 真实缺陷（分平台门漂移、`restart_app` 落 Ready 漏接同步触发），且「缺失一个
// 调用」不会让任何断言变红。守门规则（「白名单即规格」，ADR-0056 决策 4 哲学）：
//
// ① 这些域入口的**生产调用**只允许出现在壳层唯一编排点 `src/lib.rs` 的
//    `start_background_services` 函数体内；其余位置命中即红——新加入口只调
//    其中一个（或干脆各写各的）必然在此变红。
// ② 编排点函数体内全部受守标识符必须**同时**出现（成组性在单点自证）；删掉
//    其一或整函数即红（「删除即变红」，与 #963 同一验收哲学）。
// ③ `sync_engine::start_sync_scheduler`（桌面轮询线程拉起）一并纳入守门：
//    它是分平台门的域内实现细节（仅 `start_triggers` 消费），直接调用即绕过
//    分平台门——#863 缺陷 1（解锁路径无门拉轮询线程）的形态，编排点函数体
//    内同样不放行。
// ④ 白名单条目（定义与域接缝再导出文件；#1091 起备份域定义住 ledger-backup
//    crate，路径带 `crates/` 前缀相对 src-tauri 根）必须存在且各自含其受守标识符
//    ——清单漂移 fail loud，防白名单烂掉后守门空转。
// ⑤ 启动接线单点（issue #1088 / #1090 / #1091）：写路径副作用的注册必须出现在壳层
//    启动接线处（`BOOT_WIRING`：提交点后置动作①、期次落账置脏对装 #1090、
//    追补触发对装④，ADR-0112 决策 5）——缺失即生产静默丢置脏/到期检查/追补，
//    而启动路径不被任何测试直接执行（「缺失一个调用」不会让断言变红），故以源码
//    扫描守门（先例 #959 / #961 的接线在测试不可达处用扫描守门替代）。
//
// 扫描边界（issue #1472）：受守标识符扫描面 = `src-tauri/` **全树**，仅排除
// `target/`（构建产物不参与，防扫描面膨胀与生成代码假红）——此前只扫根包 src，
// crates/ 内新增模块绕过分平台门直接拉起受守调度器（恰是 #863 的历史缺陷形态）
// 守门看不见；lane 守门已示范扫 crates 子树，本票把受守名扫描跟进。文本级扫描，
// 形态同守门家族——复用 gate-primitives.ts 库的注释与字符串/char 字面量掩码（文档注释
// 提到函数名不误报）；外挂测试模块/目录豁免（ADR-0056 决策 5），内联 #[cfg(test)]
// 不豁免；裸标识符 \b 边界匹配，`start_sync_scheduler` 等更长标识符不含更短名
// 子串、天然不误伤；经别名改名的间接引用文本不可达，靠评审兜底。扫描根提不出
// 任何非测试 Rust 文件即红（零源文件拒绝假绿，与 check-async-guards 同款哨兵）。
// TypeScript 化 + Bun 运行时（issue #734 / ADR-0083）：类型经 tsconfig.scripts.json
// 门槛检查；调用方式 `bun scripts/check-background-services.ts`。
// 默认校验本仓库；测试可传位置参数指向夹具（src-tauri 根等价目录）：
// bun scripts/check-background-services.ts [src-tauri-dir]

import { readFileSync } from "node:fs";
import { join } from "node:path";
import { pathToFileURL, fileURLToPath } from "node:url";
import { maskNonCode, RUST_EXTENSIONS, walkTextFiles, type WalkedFile } from "./gate-primitives.ts";

/** 唯一编排点：壳层文件（相对 src-tauri 根，#1472 起扫描面基准同址）与函数名
 *  （issue #961 单点） */
export const ORCHESTRATOR_FILE = "src/lib.rs";
export const ORCHESTRATOR_FN = "start_background_services";

/** 成组拉起名单（编排点函数体内必须同时出现的各域入口） */
const PAIRED_NAMES = [
  "start_scheduler",
  "start_triggers",
  "start_history_backfill",
  "start_daily_price_refresh",
  "start_daily_fx_sync",
] as const;

/**
 * 受守标识符及其合法住址（导出供测试夹具派生，check-structure.test.ts 消费
 * 导出常量的同款先例）。路径一律相对 src-tauri 根（#1472 起扫描面基准即
 * src-tauri 全树：根包文件带 `src/` 前缀，crate 文件带 `crates/` 前缀）；
 * wholeFile = 整文件豁免（定义/再导出/域内调用）；
 * orchestratorBodyAllowed = 唯一编排点函数体内是否放行（start_sync_scheduler
 * 不放行：它是分平台门的域内实现细节，出域即绕门）。
 */
export interface GuardedName {
  name: string;
  wholeFile: readonly string[];
  orchestratorBodyAllowed: boolean;
  note: string;
}

export const GUARDED_NAMES: readonly GuardedName[] = [
  {
    name: "start_scheduler",
    // #1091 起备份域拆独立 crate：定义住 crate 的 auto.rs、接缝再导出住 crate 根
    // lib.rs（根包 `pub use ledger_backup as backup;` 不含标识符原文，不入列）。
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
];

/** 启动接线单点（issue #1088）：标识符 + 唯一合法接线文件（相对 src-tauri 根，
 *  #1472 起扫描面基准同址）。 */
export interface BootWiring {
  name: string;
  file: string;
  note: string;
}

/**
 * 后台车道执行器守门（issue #1413 / ADR-0125 决策 7）：行情同步域 crate 的生产面
 * 禁止自建 OS 线程——后台车道（价格历史补全 / 每日现价刷新 / 每日汇率增量同步）
 * 必须是挂全局
 * 运行时的 async 任务（`tauri::async_runtime::spawn` 拉起 + `tokio::time::sleep`
 * 异步定时）。改回 `std::thread::spawn` / `std::thread::sleep` 即红；删掉异步
 * 执行器或异步定时接线同样红。扫描面 = 行情域 crate 生产源码（tests.rs 与
 * tests/ 目录为测试豁免形态，与家族一致）；文本级扫描，掩码注释与字面量
 *（复用 maskNonCode），别名与车道模块本地同名遮蔽盲区靠评审兜底。
 *
 * issue #1622 收敛后，调度循环（含异步执行器 / 定时接线）单点住 lane.rs
 *（LANE_SCHEDULER_REL，在两枚异步标识符上断言）；三车道文件只留调度接线
 *——每文件必须调起 `start_daily_lane`（LANE_WIRE_TOKEN，缺失即红：车道模块
 * 自建平行调度循环，含 async 形态重写，即「规则改一处漏两处」回归）。
 */
export const MARKET_SYNC_SRC_REL = "crates/market-sync/src";
export const LANE_EXECUTOR_TOKEN = "tauri::async_runtime::spawn";
export const LANE_TIMER_TOKEN = "tokio::time::sleep";
export const LANE_BANNED_TOKENS = ["thread::spawn", "std::thread::sleep"] as const;
/** 调度循环单点（issue #1622 收敛）：三车道共用的巡检循环住 lane.rs——进程级
 *  守卫 + 门检 + 自然日窗口 + 异步执行器/定时都在此，异步接线在本文件断言
 * （删除即变红）。 */
export const LANE_SCHEDULER_REL = "crates/market-sync/src/lane.rs";
/** 各车道文件必须持有的调度接线标识符：调起共享调度循环（删除即变红）。 */
export const LANE_WIRE_TOKEN = "start_daily_lane";
/** 三条后台车道的住址（相对 src-tauri 根；#1622 收敛后是「接线面」：每文件必须
 *  调起 lane::start_daily_lane，异步执行器/定时住 LANE_SCHEDULER_REL 单点） */
export const LANE_FILES = [
  "crates/market-sync/src/daily_refresh.rs",
  "crates/market-sync/src/history.rs",
  "crates/market-sync/src/fx_daily.rs",
] as const;

/**
 * 壳层启动接线清单（issue #1088 提交点后置动作注册）：每条须在指定文件内出现
 * 至少一次（掩码后匹配，测试豁免同全树扫描）；缺失即红——生产启动被删无断言
 * 可捕获，只能靠扫描守门。
 */
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

/** 受守标识符的裸名匹配形态（\b 边界；global 供 matchAll 逐行报出全部命中；
 *  任意限定路径与 use 引入均命中） */
const GUARDED_NAME_PATTERN = new RegExp(
  `\\b(?:${GUARDED_NAMES.map((g) => g.name).join("|")})\\b`,
  "g",
);

/** 测试豁免形态（ADR-0056 决策 5，与 check-structure.ts 同款）：tests.rs 文件与
 *  tests/ 目录；内联 #[cfg(test)] 模块不豁免（更严，与家族一致）。 */
function isTestFile(relPath: string): boolean {
  const segments = relPath.split("/");
  const file = segments[segments.length - 1];
  return file === "tests.rs" || segments.slice(0, -1).includes("tests");
}

/** 扫描面：Rust 源文件扩展名闭集 + `target/` 目录剪枝（#1472：构建产物不参与
 *  扫描面，防膨胀与生成代码假红），walkTextFiles 消费参数。 */
/** 扩展名闭集消费库 RUST_EXTENSIONS（#1680 收口全等副本）；target/ 剪枝是本门政策 */
const SKIP_DIRS: ReadonlySet<string> = new Set(["target"]);

/** 收集目录下全部非测试 .rs 文件：遍历机制归守门家族共享单点 walkTextFiles
 *（#1625 收口，#1637 起本脚本同源），localeCompare 排序保证输出确定；文件结构
 * 复用 WalkedFile（#1634 先例同款）。测试豁免是本守门政策谓词（isTestFile 只看
 * rel 形状），walk 后按 rel 过滤——与遍历中剪枝输出全等；唯一分歧形态是名为
 * tests.rs 的目录（非合法 Rust 模块布局），walk 后过滤会收进其中 .rs 文件，
 * 只多扫不漏扫，不致假绿（#1634 同款取舍）。 */
function collectRustFiles(dir: string, relBase: string): WalkedFile[] {
  return walkTextFiles(dir, relBase, { extensions: RUST_EXTENSIONS, skipDirs: SKIP_DIRS }).filter(
    (f) => !isTestFile(f.rel),
  );
}

/** 编排点函数体在掩码文本中的行区间 [fnLine, closeLine]（1 起算，含端点）。
 *  起点为 `fn <name>` 所在行，终点为其后首个列 0 的 `}`（rustfmt 由 check.sh
 *  的 cargo fmt --check 保证，列 0 闭括号可靠）。 */
function orchestratorBodySpan(maskedLines: string[]): [number, number] | null {
  const fnLine = maskedLines.findIndex((l) => l.match(new RegExp(`fn\\s+${ORCHESTRATOR_FN}\\b`)));
  if (fnLine === -1) return null;
  for (let i = fnLine + 1; i < maskedLines.length; i++) {
    if (maskedLines[i].trim() === "}") return [fnLine + 1, i + 1];
  }
  return null;
}

/** 单条扫描命中：行号（1 起算）、原文行、命中标识符。
 *  与 check-structure.ts 的 scanRustSource（每行首个命中）不同：本守门按名
 *  核对合法住址，同一行（如 mod.rs 再导出行）可携带多个受守名，须全部报出。 */
interface NameHit {
  line: number;
  text: string;
  match: string;
}

/** 扫描单个 Rust 文本（掩码注释与字符串/char 字面量）：返回全部受守名命中。
 *  同行同名多次出现只报一次（行号定位足够，不重复计数）。 */
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
  // 扫描面基准 = src-tauri 根（#1472 前为根包 src）：受守名扫描覆盖全树，故
  // 白名单路径与编排点坐标一律相对本根解析。
  const scanRoot = process.argv[2] ?? join(repoRoot, "src-tauri");
  const problems: string[] = [];

  // 白名单存在性自证：每个整文件豁免条目必须存在且含其受守标识符（fail loud）
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

  // 唯一编排点自证：函数必须存在，成对名单在其函数体内同时出现
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

  // src-tauri 全树扫描（排除 target/，#1472）：命中按名核对合法住址
  //（整文件豁免 / 编排点函数体按名放行），其余一律红
  // 启动接线自证（issue #1088）：注册必须在壳层启动接线处出现，缺失即红。
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

  // 后台车道执行器守门（issue #1413 / ADR-0125 决策 7；#1622 收敛后调度循环
  // 单点住 lane.rs）：调度循环必须以全局运行时 async 任务拉起 + 异步定时；
  // 三车道文件必须各自调起共享循环；生产面零自建线程。目录缺失 fail loud
  //（拒绝以空集假绿通过，与全树扫描同款取舍）。
  const marketSyncSrcDir = join(scanRoot, MARKET_SYNC_SRC_REL);
  let laneFiles: WalkedFile[] = [];
  try {
    laneFiles = collectRustFiles(marketSyncSrcDir, MARKET_SYNC_SRC_REL);
  } catch {
    problems.push(
      `✗ 车道执行器扫描面不可达：${MARKET_SYNC_SRC_REL}——行情域 crate 目录漂移，拒绝以空集假绿通过`,
    );
  }
  // 调度循环单点（#1622）：异步执行器 / 定时接线在 lane.rs 上断言（删除即变红）。
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
    // 断言对准调用形状（名字后随括号）：只留 use 导入、删掉调用不动本测。
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
      if (guarded === undefined) continue; // 形态自 GUARDED_NAMES 派生，不可达；防御性跳过
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

// 仅直接运行时执行 main；被其他工具 import 时只取导出的扫描逻辑。
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main();
}
