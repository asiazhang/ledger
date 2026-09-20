#!/usr/bin/env bun
// 结构守门（issue #396 / ADR-0056；模型域化禁令 issue #424 / ADR-0059 决策 6）。
//
// 数据分界（ADR-0056 决策 4 修订注记 / #1591 定案）：**事实有权威源就投影，政策
// 无源就显式**——模块面的 expected 由各 crate `lib.rs` 的 `mod` 声明投影、与磁盘
// 顶层模块双向全等（孤儿文件/目录照红）；政策核显式声明：`CRATES`（成员 / 包名 /
// dir / layer）、允许依赖方向与方向禁令、认许边台账。手写模块清单、登记表与
// #1593 的「手写 × 派生并行核对」期随之退役（#1595），#1589 的清单拆行/定序工件
// 一并清除——同域串行票（建模块文件 + `lib.rs` 加 `mod` + 写文件头）不再触碰本脚本。
//
// 分层规则：壳 → 域 → 基础设施，域永不依赖壳。`WHITELIST` 是「已验证对壳层零
// 依赖」的**政策条目表**（根包 `src` 面），不是模块全册登记——从根包 `src/lib.rs`
// 投影会把 `commands/`、`shell_support/`、`main.rs` 拉进扫描面，语义就变了。
//
// 测试豁免（ADR-0056 决策 5）：外挂测试模块/目录（tests.rs 文件、tests/ 目录）
// 不参与守门——BDD/单元 fixture 合法引用壳层入口，不制造虚假违规；内联
// `#[cfg(test)]` 模块不豁免。条目路径缺失或扫不到非测试 Rust 文件即红（fail loud）。
//
// 扫描边界：文本级扫描，注释与字符串/char 字面量掩码后匹配 `commands::` 路径引用
// 与 `commands as` 别名引入；经别名改名的间接引用文本不可达，靠评审兜底。
// 报文形态（T2-2）：定位（文件:行）+ 分层/区 + 规则名，规则级 ADR/issue 指针住在
// 规则里（一处编号），不插值条目注记——注记的唯一作者位是 crate `lib.rs` 的 `//!`
// 与模块文件头（ADR-0113 决策 5）。
//
// `mod` 声明形状判定表（ADR-0056 决策 4 / ADR-0113 决策 7 / ADR-0111 决策 5 修订
// 注记）：`pub mod x;` / `mod x;` / `pub(crate) mod x;` 与可见性前置的声明，带
// lint 属性（allow/warn/deny/forbid/expect）、doc 属性或 `#[cfg]` / `#[cfg_attr]`
// 门控者一律计入 expected；`#[cfg(test)] mod tests;` 豁免（按目标模块名 tests 判，
// 与磁盘枚举 isTestFile 同规）；`#[path]`（含 `cfg_attr` 夹带 path）/ `include!` /
// 内联模块块 `mod x { … }` / 上述之外认不出的属性链 fail loud（报文给登记指引），
// 不静默跳过。crate 根 `lib.rs` 自身不是 `mod` 声明，不入 expected——它是声明与
// 再导出面。
//
// 跨 crate 依赖方向（#1596 / ADR-0056 决策 4 修订注记、ADR-0071 与 ADR-0101 修订
// 注记）：跨 crate 的越界方向只有两种违规形态，都不需要源码文本扫描——未声明依赖
// 即编译失败（cargo 强于文本扫描：`use crate::x as y` 别名改写对文本不可达、
// 编译期逃不掉）；已声明的越界方向由 crate 边界核对按 `Cargo.toml` 声明面判定
//（层秩 + 业务域→多端同步域禁令，见 CRATES）。据此退役的四族源码扫描：基础设施
// →域（ADR-0071 决策 6）、协议 crate→壳/域（#1089）、业务域→同步域（ADR-0101
// 决策 4b）、域间禁边（#1090，退役前已是空集——空集规则在活化前零测试覆盖，读者
// 以为在守，故代码路径整体删除）。cargo 唯一盲区是 dev-dependency 环（测试目标与
// 生产依赖图分离，cargo 放行）：基础设施对更高层 workspace crate 的
// `[dev-dependencies]` 逐条留痕于 INFRA_DOMAIN_ALLOWED_EDGES（目标 crate + 成因 +
// ADR 指针），清单之外的声明即红——方向禁令的判定载体自 #1596 起收成声明面。
// 同 crate 内的方向规则照旧文本扫描（cargo 看不见）：壳层反向依赖（白名单根 src
// 面——test_support 与 commands 同居根包，属 crate 内引用）与基础设施块间禁边
//（原语 ← db ← boot 单向，db 不得引用 boot / signals；ADR-0111 决策 4，认许边逐条
// 留痕 INFRA_BLOCK_ALLOWED_EDGES）。
//
// 模型域化禁令（ADR-0059 决策 6/T7 / #424 收口）：① 全局模型模块路径残留即红；
// ② 域模型 glob 再导出（域接缝 `pub use model::*`、跨域拍平、模型文件/目录内的
// glob 聚合）即红，所有权必须逐类型可见；③ 产品代码手写
// `BEGIN`/`COMMIT`/`ROLLBACK` 即红——靶形态落在字符串里，扫描保留字符串、只掩码
// 注释；唯一合法住址 `db/tx_scope.rs`（issue #1014）。
//
// 交易域区级层序（ADR-0113 决策 2/3/7 / #1181 / #1597）：区归属按目录约定投影——
// zone = crate 顶层区目录（TRANSACTION_ZONE_BY_DIR 一表单点声明，per-module zone
// 字段退役）；crate 根顶层单文件必须进区目录、未登记区归属的顶层目录即红（fail
// loud，不允许「根级单文件默认为共享语义」——那会给写路径模块留静默错分类的
// 洞）。引用形态掩码后匹配 `super::` / `crate::` 前缀 + 目标首段（含花括号列举
// 逐条展开）；设计意图边逐条留痕 TRANSACTION_ZONE_ALLOWED_EDGES，清单之外即红。
//
// crate 边界核对（spec #1086 / issue #1087 门禁前置）：`CRATES` 是 workspace 成员、
// 分层与允许依赖方向的唯一政策核——成员目录（crates/*）与 CRATES 双向全等（新 crate
// 未登记即红）；每个成员须写 `[lints] workspace = true` 继承六件套门禁（漏写即红）；
// 依赖方向按壳 → 域 → 基础设施单向核对；业务域 crate 不得生产依赖多端同步域
// （ADR-0101 决策 4b / #1107）；scripts/check.sh、scripts/test.sh、scripts/lint-fix.sh
// 与 CI workflow 的 cargo clippy/test/fmt 命令须显式 `--workspace` 或 `--all`（非虚拟
// workspace 下默认只作用于根包，缺范围参数会静默漏检成员；`--all-targets` 等 `--all*`
// 旗标不算范围）；引号内的命令字样是说明文字不算命令面，宿主里一条命令都核不到即红
// （空集假绿）。另核 test_utils 生产编译门（ADR-0111 决策 5 / #1132）、投资五节锚点
// 门（#1185）与 http 投影 feature 门（ADR-0111 决策 5 / #1133）。
//
// TypeScript 化 + Bun 运行时（issue #734 / ADR-0083）：类型经 tsconfig.scripts.json
// 门槛检查；调用方式 `bun scripts/check-structure.ts`。
// 默认校验本仓库；测试可传位置参数指向夹具：
// bun scripts/check-structure.ts [src-dir] [src-tauri-dir]
// 挂载于 scripts/check.sh 质量门槛序列与 CI（build.yml frontend job），
// 与命令注册一致性检查并列。

import { existsSync, readdirSync, readFileSync, statSync, type Stats } from "node:fs";
import { dirname, join } from "node:path";
import { pathToFileURL, fileURLToPath } from "node:url";

/** 白名单条目（ADR-0056 决策 4）：路径 + 分层；注记归模块文件头（ADR-0113 决策 5）。 */
export interface WhitelistEntry {
  path: string;
  layer: Layer;
}

/** 分层词汇（白名单 layer 字段取值）：比较侧单一来源，防字面量漂移 */
export const LAYER = {
  DOMAIN: "域目录",
  INFRA: "基础设施",
  PROTOCOL: "协议",
} as const;

export type Layer = (typeof LAYER)[keyof typeof LAYER];

/**
 * 守门白名单（ADR-0056 决策 4）：路径相对 src-tauri/src，条目 = 「已验证对壳层
 * 零依赖」的**政策条目表**（不是模块全册登记——见文件头）。今仅测试支持域一条；
 * 域与基础设施整体归 crate 后，crate 模块面由各 crate `lib.rs` 投影（#1595），
 * 根包 src 面（壳层反向依赖，同 crate 引用）由本表单独辖。
 */
export const WHITELIST: readonly WhitelistEntry[] = [{ path: "test_support", layer: LAYER.DOMAIN }];

/** 基础设施 crate 的模块根（相对 src-tauri）：`CRATES` 的 ledger-infra.dir + /src。 */
export const INFRA_SRC_REL = "crates/infra/src";

/** 交易域 crate 四区词汇（ADR-0113 决策 2/3）：区级层序判向共用，字面量单一来源。 */
export const TRANSACTION_ZONE = {
  SHARED: "共享语义",
  SEAM: "跨域接缝",
  WRITE: "写路径",
  READ: "读路径",
} as const;

export type TransactionZone = (typeof TRANSACTION_ZONE)[keyof typeof TRANSACTION_ZONE];

/**
 * 交易域区目录 → 区归属（ADR-0113 决策 2 目录约定 / #1597）：zone 靠路径投影——
 * 文件的区归属 = 其所在 crate 顶层区目录的归属，#1181 期随清单条目同址登记的
 * per-module zone 字段退役；区归属改变即目录移动 + 本表一行改注，移动本身是
 * 评审可见的改动。crate 根 lib.rs 与测试豁免形态（tests.rs / tests/）不投影；
 * 顶层单文件不在本表（fail loud：根级单文件必须进区目录，不允许「默认为共享
 * 语义」——那会给写路径模块留静默错分类的洞）；未登记区归属的顶层目录同样
 * fail loud。
 */
export const TRANSACTION_ZONE_BY_DIR: Readonly<Record<string, TransactionZone>> = {
  amount: TRANSACTION_ZONE.SHARED,
  command: TRANSACTION_ZONE.SHARED,
  model: TRANSACTION_ZONE.SHARED,
  read: TRANSACTION_ZONE.READ,
  seams: TRANSACTION_ZONE.SEAM,
  shared: TRANSACTION_ZONE.SHARED,
  write: TRANSACTION_ZONE.WRITE,
};

/**
 * 交易域区级层序（ADR-0113 决策 3）：允许依赖方向唯一——写路径/读路径 → 跨域
 * 接缝 → 共享语义。同区互依合法；跨区时秩大者方可依赖秩小者；写路径与读路径
 * 同秩，互不依赖由「跨区且秩不大即红」承担。
 */
const TRANSACTION_ZONE_RANK: Record<TransactionZone, number> = {
  [TRANSACTION_ZONE.SHARED]: 0,
  [TRANSACTION_ZONE.SEAM]: 1,
  [TRANSACTION_ZONE.WRITE]: 2,
  [TRANSACTION_ZONE.READ]: 2,
};

/** 交易域区级认许边条目：拥有文件的区目录名 + 目标区目录名 + 成因留痕 */
interface TransactionZoneEdge {
  dir: string;
  target: string;
  reason: string;
}

/**
 * 交易域区级认许边（ADR-0113 决策 3 / #1181）：原形状上与区级层序冲突的既有
 * 设计意图边逐条留痕于本脚本（与 INFRA_DOMAIN_ALLOWED_EDGES 同款纪律），精确到
 * 区目录名 + 目标区目录名（目录约定 / #1597 后的判向单位），附 ADR 指针；清单
 * 之外的区级反向引用一律红。两条原反边已由重排票（#1182）消除（本位币接缝归
 * 共享语义区、model→writer 转换归写路径），故本清单归空——再出现区级反向引用
 * 即红，不设认许。
 */
export const TRANSACTION_ZONE_ALLOWED_EDGES: readonly TransactionZoneEdge[] = [];

/**
 * crate 分层词汇（crate 边界核对用）：壳 → 域 → 基础设施单向。
 * 与上面的 `LAYER`（单 crate 内的**模块路径**分层：域目录 / 基础设施）刻意分开——
 * 两者是不同粒度的事实源，同名值不合并（合并只会让任一侧语义被动漂移）。
 */
export const CRATE_LAYER = {
  SHELL: "壳",
  DOMAIN: "域",
  INFRA: "基础设施",
  PROTOCOL: "协议",
} as const;

export type CrateLayer = (typeof CRATE_LAYER)[keyof typeof CRATE_LAYER];

/**
 * crate 依赖方向优先级（数值大者可依赖数值小者）：壳 → 域 → 基础设施 → 协议
 *（#1089：协议 crate 是全部业务域与同步域的共享底座，自身仍消费基础设施——
 * 基础设施之下再无更底层）。
 */
const CRATE_LAYER_RANK: Record<CrateLayer, number> = {
  [CRATE_LAYER.SHELL]: 3,
  [CRATE_LAYER.DOMAIN]: 2,
  [CRATE_LAYER.PROTOCOL]: 1,
  [CRATE_LAYER.INFRA]: 0,
};

/** crate 政策核条目（spec #1086 / issue #1087，ADR-0112 决策 4 修订）：dir 相对 src-tauri。 */
export interface CrateEntry {
  name: string;
  dir: string;
  layer: CrateLayer;
}

/**
 * crate 政策核：workspace 成员、包名、dir、分层与允许依赖方向的唯一显式事实源
 * （ADR-0112 决策 4 修订注记——模块面已改由各 crate `lib.rs` 投影，条目注记归
 * crate `//!`）。结构守门据此核对成员登记、门禁继承与依赖方向；每拆一个域 crate
 * 按 name 字节序插入一行。报文只给定位与规则级指针，不插值注记。
 */
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

/** 成员目录约定（workspace glob）：新增 crate 只需落在此目录下即自动入 workspace。 */
const MEMBER_DIR_GLOB = "crates/*";

/**
 * crate 模块扫描目标（#1595）：模块面的分层自 crate 政策核投影（`CRATES` 每
 * crate 一条 layer），模块清单 expected 由各 crate `lib.rs` 的 `mod` 声明投影。
 * 根包（壳层）不整册登记模块面——`WHITELIST` 是根 `src` 的政策条目表，单独扫描。
 */
export interface CrateModuleTarget {
  /** crate 名（报文定位用；与 Cargo.toml 包名一致由 crate 边界核对保证） */
  crate: string;
  /** 模块根（相对 src-tauri） */
  srcRel: string;
  /** 模块路径分层（自 crate layer 投影：域 → 域目录，基础设施 → 基础设施，协议 → 协议） */
  layer: Layer;
}

/** crate 分层 → 模块路径分层（壳层返回 null：根包模块面不整册登记）。 */
function moduleLayerOf(crateLayer: CrateLayer): Layer | null {
  if (crateLayer === CRATE_LAYER.DOMAIN) return LAYER.DOMAIN;
  if (crateLayer === CRATE_LAYER.INFRA) return LAYER.INFRA;
  if (crateLayer === CRATE_LAYER.PROTOCOL) return LAYER.PROTOCOL;
  return null;
}

/** crate 模块扫描目标表（自 `CRATES` 政策核投影，无第二事实源）。 */
export const CRATE_MODULE_TARGETS: readonly CrateModuleTarget[] = CRATES.flatMap((crate) => {
  const layer = moduleLayerOf(crate.layer);
  if (layer === null) return [];
  return [{ crate: crate.name, srcRel: `${crate.dir}/src`, layer }];
});

/** 易 panic 构造六件套（ADR-0060）：workspace 级声明的键集（单一来源）。 */
const PANIC_LINT_KEYS = [
  "unwrap_used",
  "expect_used",
  "panic",
  "todo",
  "unimplemented",
  "unreachable",
] as const;

/** 显式声明 workspace 范围的 cargo 命令宿主（静态检查与测试命令覆盖全成员）。 */
const WORKSPACE_COMMAND_FILES = [
  "scripts/check.sh",
  "scripts/test.sh",
  "scripts/lint-fix.sh",
  // 执行器程序化拼装 cargo 命令（`runChild(cargo, ['test', '--workspace', …])`）：
  // 宿主形态是 .ts 而非 shell。该宿主的真实命令面是**数组形态**（cargo 标识符 + 数组
  // 字面量），逐行字面量扫描只看得见说明文字（console.log 模板串），故另配数组形态核对
  // （checkTsCargoArrays）真正约束命令面，见 #1112 第三轮审查 P2。
  "scripts/test-exec.ts",
  ".github/workflows/build.yml",
] as const;

/** workspace 范围参数：`--workspace` 或 `--all`（精确词，防止 `--all-targets` 假绿）。 */
const WORKSPACE_SCOPE_PATTERN = /(?:^|\s)--workspace(?:\s|$)/;
const ALL_SCOPE_PATTERN = /(?:^|\s)--all(?:\s|$)/;

/** cargo 命令词（workspace 范围核对的适用范围）。 */
const CARGO_SUBCOMMANDS = ["clippy", "test", "fmt"] as const;

/**
 * shell / YAML 引号与注释掩码（逐行，不跨行）：把解释性文字换成空格、保留列位置。
 * 引号里的 `cargo test …` 是说明文字（`echo "( cd src-tauri && cargo test … )"`），
 * `#` 之后是注释，都不构成命令面；不掩码会把 echo 字符串当成命令，把三条真命令全包成
 * echo 后核对仍然全绿（#1112 第三轮审查 P1）。逐行做也保证一个未闭合引号不会吞掉
 * 后续行。`.ts` 宿主不掩码——那里的命令面恰恰是字符串字面量。
 */
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
        // `#` 起注释：行首或前面是空白（`foo#bar` 不是注释）
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

/** `.ts` 宿主里程序化拼装的 cargo 命令：数组起始行（1-based）+ 数组内的字符串元素。 */
interface TsCargoArray {
  line: number;
  elements: string[];
}

/** 注释行（`.ts` 的行注释 `//` 与块注释续行 ` * `）：注释里的命令字样不算命令面。 */
function isTsCommentLine(trimmed: string): boolean {
  return (
    trimmed === "" ||
    trimmed.startsWith("//") ||
    trimmed.startsWith("/*") ||
    trimmed.startsWith("*")
  );
}

/**
 * 找 `.ts` 宿主的数组形态 cargo 命令 `runChild(cargo, ['test', '--workspace', …])`：
 * `cargo` 标识符 + 逗号之后，数组要么同行，要么紧随的下一个非空行以 `[` 开头（仓内
 * 多行调用形态）；再向后收括号，取出引号字符串元素。不这样限定就会把 `f(cargo, x)`
 * 之后随便一个数组误认成命令面。
 */
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

/**
 * `.ts` 宿主的数组形态 cargo 命令核对（#1112 第三轮审查 P2）：逐行字面量扫描只看得见
 * 说明文字（console.log 模板串），真实命令面是数组参数——首元素是 cargo 子命令时必须
 * 带 workspace 范围。返回命中条数。
 */
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

/** 壳层依赖形态：模块路径引用（crate::commands::x / commands::x）与别名引入 */
const SHELL_DEP_PATTERN = /\bcommands\s*::|\bcommands\s+as\b/;

/** 认许边条目（#1596 换载体）：目标 workspace crate 名 + 成因留痕 */
interface InfraDevDepEdge {
  crate: string;
  reason: string;
}

/**
 * 基础设施 dev-dependency 认许边（ADR-0071 决策 6 修订注记 / #1596）：跨 crate
 * 源码文本扫描退役后，基础设施→域方向的判定载体收成声明面——`[dependencies]`
 * 的越界由 crate 层秩核对拦下、未声明依赖即编译失败，只有 `[dev-dependencies]`
 * 是 cargo 盲区（测试目标与生产依赖图分离，cargo 对 dev-dep 环放行）。
 *
 * 故本台账逐条留痕基础设施对**更高层** workspace crate 的 dev-dependency：
 * 目标是测试专用边（测试目标可见、生产依赖图不含），条目附 ADR 指针与成因；
 * 清单之外的声明即红。台账形状自 #1596 起由「文件 + 目标域目录名」收成
 * 「目标 crate 名」——源码形态由编译器判定，声明面才是唯一可核对处。
 */
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

/**
 * crate 内块间禁边（ADR-0111 决策 4 / #1134；#1108 修订）：子目录级反向依赖
 * 断言——原语 ← db/ ← boot/ 单向，events / signals / settings 是被各层引用的
 * 共享接缝；db 不得引用 boot / signals。shell_support 靶随 #1108 迁出根包退役
 *（块在 crate 内已不存在，残留引用归编译期拒绝）。键 = 拥有该文件的块
 *（路径首段），值 = 禁止引用的目标块；顶层单文件（ids / error / fs_util 等
 * 原语与共享接缝）不受块间禁边约束。
 */
const INFRA_BLOCK_FORBIDDEN: Record<string, readonly string[]> = {
  db: ["boot", "signals"],
};

/** 块间依赖形态：crate 根前缀 + 目标块名（掩码后匹配），
 *  再随 `::`（路径引用）、` as `（别名引入）或 `;`（模块自身导入）；\b 防前缀吞
 *  匹配，`\{?\s*` 容纳花括号列举首段（use crate::{boot::x, …}）；花括号列举
 *  非首段与 super:: 改写文本不可达，靠评审兜底。 */
function infraBlockDepPattern(targets: readonly string[]): RegExp {
  return new RegExp(
    `\\bcrate\\s*::\\s*\\{?\\s*(${targets.join("|")})\\b(?:\\s*::|\\s+as\\b|\\s*;)`,
  );
}

/** crate 内块间认许边条目：基础设施文件相对路径 + 目标块名 + 成因留痕 */
interface InfraBlockEdge {
  file: string;
  target: string;
  reason: string;
}

/**
 * crate 内块间认许边（ADR-0111 决策 4 / #1134）：块间反向依赖的既有设计意图
 * 边逐条留痕于本脚本，与 INFRA_DOMAIN_ALLOWED_EDGES 同款留痕纪律——精确到
 * 文件相对路径（相对 `crates/infra/src`）+ 目标块名，附 ADR 指针；清单之外的
 * 块间反向引用一律红。
 */
const INFRA_BLOCK_ALLOWED_EDGES: readonly InfraBlockEdge[] = [
  {
    file: "db/mod.rs",
    target: "boot",
    reason:
      "ADR-0111 决策 2 / #1131：引导层五模块升顶层 boot 后，既有 `crate::db::{boot,…}` 调用点与协议 crate 的 `ledger_infra::db::…` 路径经本再导出保持零改动——路径兼容面，非机制依赖（#1128 ids 同款口径）",
  },
];

/** 规则①形态：全局模型模块路径（全局目录已消亡，任何引用即残留） */
const GLOBAL_MODEL_PATH_PATTERN = /\b(?:crate|tauri_app_lib)\s*::\s*models\b/;

/** 规则②形态：模型模块的 glob 再导出——域接缝 `pub use model::*` 与
 *  跨域/旧目录同名拍平 `pub use …::models::*`；逐类型花括号列举不命中 */
const MODEL_GLOB_REEXPORT_PATTERN = /\bpub\s+use\s+[\w:]*\bmodels?\b\s*::\s*\*/;

/** 规则②形态：任意 glob 再导出（仅用于域模型文件内的聚合扫描） */
const MODEL_FILE_GLOB_PATTERN = /\bpub\s+use\s+[\w:]*\*/;

/** 规则③形态：产品代码原生事务语句（issue #1014 / #1003 grilling 定案 7）——
 *  事务壳（无条件自持 `hold_transaction` / 嵌套感知 `ensure_transaction`）归
 *  基础设施 `db::tx_scope`，其余位置手写 `BEGIN` / `COMMIT` / `ROLLBACK` 即红。
 *  靶形态落在字符串字面量里，扫描须 `keepLiterals=true`（只掩码注释）。
 *  #1469：API 形态覆盖 `execute` 与 `execute_batch` 两变体（仓内多处在用）——
 *  手写事务边界不得借 API 变体漏检。 */
const NATIVE_TX_STMT_PATTERN = /\bexecute(?:_batch)?\s*\(\s*"(?:BEGIN|COMMIT|ROLLBACK)\b/;

/** 原生事务语句唯一合法住址（事务原语本体，issue #1014；#1088 起住基础设施 crate） */
const NATIVE_TX_STMT_ALLOWED = `${INFRA_SRC_REL}/db/tx_scope.rs`;

/** 花括号列举内的违规条目头（深度 0 逐条切分后取首个标识符；#1089 零容忍，
 *  全部条目违规）。返回违规条目头，供调用方构造命中。 */
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

/**
 * 域模型文件或模型目录成员（ADR-0059 目标形状：每域一个 model.rs，先例名
 * models.rs；#1181 起判据扩到目录形态——模型目录 model/ / models/ 下的成员
 * 文件同守「域模型禁止 glob 聚合」，模型目录化不再静默失靶）
 */
function isModelFile(relPath: string): boolean {
  const segments = relPath.split("/");
  const file = segments[segments.length - 1];
  if (file === "model.rs" || file === "models.rs") return true;
  return segments.slice(0, -1).some((s) => s === "model" || s === "models");
}

/** 测试豁免形态（ADR-0056 决策 5）：tests.rs 文件与 tests/ 目录 */
function isTestFile(relPath: string): boolean {
  const segments = relPath.split("/");
  const file = segments[segments.length - 1];
  return file === "tests.rs" || segments.slice(0, -1).includes("tests");
}

/** 若 i 起是 Rust 原始字符串前缀，返回其后开引号下标；否则 null。
 *  覆盖 r"…" / r#"…" 与字节变体 br"…" / br#"…"，# 数任意；前一字符为
 *  标识符成分时是普通名字（如 for），不误伤。 */
function rawStringOpenQuoteAt(text: string, i: number): number | null {
  const prev = i > 0 ? text[i - 1] : "";
  if (/[A-Za-z0-9_]/.test(prev)) return null;
  let j = i;
  if (text[j] === "b" && text[j + 1] === "r") j += 2;
  else if (text[j] === "r") j += 1;
  else return null;
  while (text[j] === "#") j++;
  return text[j] === '"' ? j : null;
}

/**
 * 掩码 Rust 源文本中的注释与字符串/char 字面量：内容替换为等长空白
 * （保留换行与列位，行号不变），使依赖扫描只落在真实代码上。
 * 处理形态：行注释（//、///、//!）、块注释（/* .. *&#47;，可嵌套）、
 * 普通字符串（含转义）、原始字符串 r"…" / r#"…" / r##"…" 及其字节变体
 * br"…" / br#"…" / br##"…"（# 数任意；'\u{…}' 转义不按字面量识别——与
 * Rust 侧一致，见下）、
 * char 字面量（'a'、'\n'、'\\'、'\''）；生命周期标注（'a）按非字面量处理。
 * `keepLiterals=true` 时保留字符串/char 字面量内容、只掩码注释——用于靶形态
 * 落在字符串里的扫描（原生事务语句 `execute("BEGIN")`，issue #1014）。
 *
 * **双源登记**（issue #1433）：本函数与 Rust 侧唯一实现
 * `src-tauri/src/test_support/scan.rs` 的 `mask_non_code` 是同一条词法掩码规则
 * 的两个运行时载体，规则改动必须两侧同步；防漂移断言消费共享语料夹具
 * `scripts/fixtures/rust-mask-corpus.rs`（check-structure.test.ts 与 Rust 测试
 * 双侧消费，任一侧单独改规则即红）。
 */
export function maskNonCode(text: string, keepLiterals = false): string {
  const out = text.split("");
  const n = text.length;
  const blank = (from: number, to: number): void => {
    for (let k = from; k < to && k < n; k++) if (out[k] !== "\n") out[k] = " ";
  };
  let i = 0;
  while (i < n) {
    const c = text[i];
    if (c === "/" && text[i + 1] === "/") {
      // 行注释（含 /// 与 //!）到行尾
      const end = text.indexOf("\n", i);
      const stop = end === -1 ? n : end;
      blank(i, stop);
      i = stop;
    } else if (c === "/" && text[i + 1] === "*") {
      // 块注释，Rust 可嵌套
      let depth = 1;
      let j = i + 2;
      while (j < n && depth > 0) {
        if (text[j] === "/" && text[j + 1] === "*") {
          depth++;
          j += 2;
        } else if (text[j] === "*" && text[j + 1] === "/") {
          depth--;
          j += 2;
        } else {
          j++;
        }
      }
      blank(i, j);
      i = j;
    } else if (c === '"') {
      // 普通字符串：跳过转义对
      let j = i + 1;
      while (j < n) {
        if (text[j] === "\\") j += 2;
        else if (text[j] === '"') {
          j++;
          break;
        } else j++;
      }
      if (!keepLiterals) blank(i, j);
      i = j;
    } else if (c === "r" || c === "b") {
      // 原始字符串 r"…" / r#"…" / r##"…" 与字节变体 br"…" / br#"…" / br##"…"
      const open = rawStringOpenQuoteAt(text, i);
      if (open === null) {
        i++;
        continue;
      }
      const prefixEnd = c === "b" ? i + 2 : i + 1;
      const hashes = open - prefixEnd;
      const close = '"' + "#".repeat(hashes);
      const end = text.indexOf(close, open + 1);
      const stop = end === -1 ? n : end + close.length;
      if (!keepLiterals) blank(i, stop);
      i = stop;
    } else if (c === "'") {
      // char 字面量 vs 生命周期：有闭引号为字面量，否则是生命周期标注（'a）
      let j = i + 1;
      if (text[j] === "\\") {
        j++;
        if (text[j] === "{") {
          const e = text.indexOf("}", j);
          j = e === -1 ? n : e + 1;
        } else {
          j++;
        }
      } else {
        j++;
      }
      if (text[j] === "'") {
        const stop = j + 1;
        if (!keepLiterals) blank(i, stop);
        i = stop;
      } else {
        i++;
      }
    } else {
      i++;
    }
  }
  return out.join("");
}

/** 单条扫描命中：行号（1 起算）、原文行、匹配文本、捕获组 1
 *  （无捕获组时 undefined，基础设施→域形态为目标域名） */
export interface ScanHit {
  line: number;
  text: string;
  match: string;
  captured: string | undefined;
}

/** 扫描单个 Rust 文本（掩码注释与字符串/char 字面量）：返回命中指定形态的
 *  行号（1 起算）与原文；形态缺省为壳层依赖（白名单分层检查的既有行为）。
 *  `keepLiterals=true` 保留字符串/char 字面量内容、只掩码注释——靶形态落在
 *  字符串里的扫描（原生事务语句，issue #1014） */
export function scanRustSource(
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

/** 收集到的 Rust 文件引用：绝对路径 + 相对路径（输出与报文用） */
interface RustFileRef {
  abs: string;
  rel: string;
}

/** 递归收集目录下全部 .rs 文件（跳过测试豁免形态），相对路径排序保证输出确定 */
function collectRustFiles(dir: string, relBase: string): RustFileRef[] {
  const out: RustFileRef[] = [];
  for (const entry of readdirSync(dir, { withFileTypes: true }).sort((a, b) =>
    a.name.localeCompare(b.name),
  )) {
    const abs = join(dir, entry.name);
    const rel = relBase ? `${relBase}/${entry.name}` : entry.name;
    if (isTestFile(rel)) continue;
    if (entry.isDirectory()) out.push(...collectRustFiles(abs, rel));
    else if (entry.name.endsWith(".rs")) out.push({ abs, rel });
  }
  return out;
}

/** 取 TOML 段内容（段头到下一个段头之间，不含段头行）；段不存在返回 null。 */
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

/** 清单 [package] name（缺失返回 null）。 */
function manifestPackageName(manifest: string): string | null {
  const section = manifestSection(manifest, "package");
  const m = section?.match(/(?:^|\n)\s*name\s*=\s*"([^"]+)"/);
  return m ? m[1] : null;
}

/**
 * 清单声明的依赖 crate 名（`table` = dependencies / dev-dependencies；含
 * `target.*` 变体与 `[dependencies.x]` 子表形态）。只取依赖表本身，段落其余内容
 * 不参与——供 crate 依赖方向核对使用。
 */
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

/**
 * 清单声明的**生产**依赖 crate 名。刻意排除 `[dev-dependencies]`：spec #1086
 * 明文裁决「数据库 ↔ 核心交易域的双向引用是测试专用边，用 dev-dependency 环
 * 解决（Cargo 允许）」，生产依赖方向仍按「壳 → 域 → 基础设施」单向核对。
 */
function declaredProductionDependencyNames(manifest: string): string[] {
  return declaredDependencyNames(manifest, "dependencies");
}

/** 清单声明的 **dev-dependency** crate 名（#1596 基础设施方向核对用）。 */
function declaredDevDependencyNames(manifest: string): string[] {
  return declaredDependencyNames(manifest, "dev-dependencies");
}

/** [lints] 段声明 workspace 继承（`workspace = true`）——六件套门禁的继承接线。 */
function inheritsWorkspaceLints(manifest: string): boolean {
  const section = manifestSection(manifest, "lints");
  return section !== null && /(?:^|\n)\s*workspace\s*=\s*true\b/.test(section);
}

/** 行内括号净计数（`(` 减 `)`）——属性全文跨行闭合判定用（文本级扫描，
 *  字符串与注释内的括号不豁免；属性谓词不含带括号字符串的常态下可靠，
 *  残余场景靠评审兜底）。 */
function parenDelta(line: string): number {
  let delta = 0;
  for (const ch of line) {
    if (ch === "(") delta++;
    else if (ch === ")") delta--;
  }
  return delta;
}

/**
 * 声明（declIndex）前属性链中的首条 `#[cfg(...)]` 属性全文：跳过空行与注释、
 * 透明放行其它属性（如 `#[doc(hidden)]`），停在首个非属性行——无 cfg 即 null。
 * 属性自起点行向下读到配对闭合括号为止（#1469）：rustfmt 拆行的多行属性同样
 * 识别——声明前命中续行（如 `))]`）时向上找最近 `#[` 行作起点，途中被普通
 * 代码行隔开即属性链终止（属性与被饰声明必须相邻）；起点与续行不相干
 * （闭合行未覆盖续行，如更上层无关属性的闭合在代码行之前）按无门处理，
 * 不吞更上层的无关属性（防借无关 cfg 门假绿）。属性判定在掩码注释后进行
 * （keepLiterals=true，保留字符串字面量），属性内注释不参与门匹配。
 * 供生产编译 feature 门的各判定共用（test_utils / http 投影，ADR-0111 决策 5）。
 */
function firstCfgTextBefore(lines: readonly string[], declIndex: number): string | null {
  let i = declIndex - 1;
  while (i >= 0) {
    const line = lines[i].trim();
    if (line === "" || line.startsWith("//")) {
      i--;
      continue;
    }
    // 定位本条属性的起点行：首个相关行为 `#[` 时即其自身；为续行（如 `))]`）时
    // 向上找最近 `#[` 行作候选起点（途中代码行不拦截，交由下方闭合连续性校验拒绝）
    let start = -1;
    if (line.startsWith("#[")) {
      start = i;
    } else {
      for (let j = i - 1; j >= 0; j--) {
        if (lines[j].trim().startsWith("#[")) {
          start = j;
          break;
        }
      }
      if (start === -1) return null;
    }
    // 自起点向下读属性全文，到配对闭合（累计括号归零）为止
    let balance = 0;
    let end = -1;
    for (let k = start; k < declIndex; k++) {
      balance += parenDelta(lines[k]);
      if (balance <= 0) {
        end = k;
        break;
      }
    }
    if (end === -1) return null; // 到声明仍未闭合——残缺属性，按无门处理
    if (end < i) return null; // 闭合行未覆盖声明前相关行——属性与续行/代码不相干，属性链终止
    const attr = maskNonCode(lines.slice(start, end + 1).join("\n"), true);
    if (attr.startsWith("#[cfg(")) return attr;
    i = start - 1; // 其它属性（如 `#[doc(hidden)]`）：透明放行，继续向上
  }
  return null;
}

/**
 * 测试器具生产编译门的「放行测试」判定（ADR-0111 决策 5 / issue #1132）：声明
 * （`pub mod test_utils;` 等）前的属性链中须有
 * 一条 `#[cfg(...)]`（单行或 rustfmt 拆行的多行属性，#1469），且该 cfg 在
 * `test` 或 `test-utils` feature 下放行——无门、
 * `#[cfg(not(test))]` 等反向门、与测试无关的 cfg 一律不合格（判为生产会编译）。
 * 声明前允许注释与其它属性（如 `#[doc(hidden)]`），属性顺序不敏感。
 */
function hasTestAllowingCfgGate(lines: readonly string[], declIndex: number): boolean {
  const cfg = firstCfgTextBefore(lines, declIndex);
  return cfg !== null && /\btest\b/.test(cfg) && !cfg.includes("not(");
}

/**
 * HTTP 投影 impl 的 feature cfg 门判定（ADR-0111 决策 5 / issue #1133）：声明
 * （`impl axum::response::IntoResponse for AppError`）前的属性链中须有一条
 * `#[cfg(...)]`（单行或多行，#1469）含 `feature = "http"`；无门、`#[cfg(not(feature = "http"))]` 等
 * 反向门一律不合格（feature 开启实现反而消失，等价于无门）。同 hasTestAllowingCfgGate，
 * 声明前允许注释与其它属性，属性顺序不敏感。
 */
function hasHttpFeatureCfgGate(lines: readonly string[], declIndex: number): boolean {
  const cfg = firstCfgTextBefore(lines, declIndex);
  return cfg !== null && /feature\s*=\s*"http"/.test(cfg) && !cfg.includes("not(");
}

/**
 * 生产依赖表（`[dependencies]` 与 `target.*.dependencies`，inline 或子表形态，
 * 刻意排除 `[dev-dependencies]`）中对 `ledger-infra` 启用指定 feature 的原文行；
 * 命中的行即「生产构建会把该 feature 编入」的证据（ADR-0111 决策 5：#1132 用
 * 于 test-utils、#1133 用于 http）。
 */
function productionLedgerInfraEnablement(manifest: string, feature: string): string | null {
  // 词边界匹配：'http' 不得误命中 https:// 等 URL 里的裸 'http' 子串。
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

/**
 * `[features] default` 是否（直接或经本清单内 feature 转发）触达指定 feature——
 * 默认 feature 在生产构建启用，等价于无条件编入该 feature 的内容。转发链上的
 * 边按 `includes(feature)` 判定，因而也覆盖 `ledger-infra/test-utils` 这类
 * 跨 crate 引用形态（ADR-0111 决策 5：#1132 用于 test-utils、#1133 用于 http）。
 * 跨清单的依赖 feature 图不在文本可辨范围，靠评审兜底。
 */
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

/** `crates/` 下含 Cargo.toml 的成员 crate 目录（相对 src-tauri，排序保证输出确定）。 */
function memberCrateDirs(srcTauriDir: string): string[] {
  const cratesDir = join(srcTauriDir, "crates");
  if (!existsSync(cratesDir)) return [];
  return readdirSync(cratesDir, { withFileTypes: true })
    .filter((e) => e.isDirectory() && existsSync(join(cratesDir, e.name, "Cargo.toml")))
    .map((e) => `crates/${e.name}`)
    .sort();
}

/**
 * crate 边界核对（spec #1086 / issue #1087 门禁前置）：workspace 成员登记、
 * 六件套 deny 门禁继承、依赖方向（壳 → 域 → 基础设施）与静态检查/测试命令
 * 的 workspace 覆盖，外加 test_utils 生产编译门（ADR-0111 决策 5 / #1132）与
 * HTTP 错误响应投影 feature 门（ADR-0111 决策 5 / #1133），
 * 全部 fail loud、删除即变红：
 * - 成员漏写 `[lints] workspace = true` → 门禁静默消失，clippy 仍绿，本核对红；
 * - `crates/` 下新增 crate 未登记 CRATES → 边界知识分裂，本核对红；
 * - cargo 命令缺 `--workspace` → 默认只作用于根包，本核对红；
 * - infra `test_utils` 模块摘掉 cfg 门、或根包生产依赖启用 `test-utils` → 测试
 *   器具被编入生产构建，本核对红；
 * - infra `http` 投影门被摘（axum 裸依赖 / impl 无 cfg / default 含 http / 域侧
 *   成员启用 http）→ axum 无条件编入或域侧引入，本核对红。
 */
function checkCrateBoundaries(srcTauriDir: string): string[] {
  const problems: string[] = [];
  const repoRoot = dirname(srcTauriDir);
  const rootManifestPath = join(srcTauriDir, "Cargo.toml");
  if (!existsSync(rootManifestPath)) {
    problems.push(`✗ crate 边界：workspace 根清单不存在：${rootManifestPath}`);
    return problems;
  }
  const rootManifest = readFileSync(rootManifestPath, "utf8");

  // ① workspace 骨架 + 成员目录 glob（新增 crate 自动成为 workspace 成员）
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

  // ② 六件套门禁的唯一声明处（workspace 级）
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

  // ③ 根包同为 workspace 成员，须继承门禁
  if (!inheritsWorkspaceLints(rootManifest)) {
    problems.push(
      "✗ 门禁继承：workspace 根包缺 [lints] workspace = true——根包同受六件套约束（ADR-0060）",
    );
  }

  // ④ 成员登记：磁盘成员目录 ↔ CRATES 双向核对（新 crate 未登记即红）
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

  // ⑤ 每个 crate：清单存在、包名一致、门禁继承、依赖方向单向
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
    // 依赖方向只看生产依赖（dev-dependency 环是 spec #1086 明文裁决的测试专用边，
    // 见 declaredProductionDependencyNames 注释）。
    for (const dep of declaredProductionDependencyNames(manifest)) {
      const target = CRATES.find((c) => c.name === dep);
      if (target && CRATE_LAYER_RANK[target.layer] > CRATE_LAYER_RANK[crate.layer]) {
        problems.push(
          `✗ crate 依赖方向：${crate.name}（${crate.layer}）依赖 ${target.name}（${target.layer}）\n` +
            "    分层规则：壳 → 域 → 基础设施单向；被依赖逻辑应下沉到更低层（spec #1086）",
        );
      }
    }
    // 同级域 crate 的同步禁边（ADR-0101 决策 4b / #1107）：多端同步域是全部业务域
    // 的消费方，业务域只可依赖同步协议 crate；业务域声明 `ledger-sync-engine`
    // 生产依赖即形成「业务域 ↔ 同步域」环，与分层秩无关，故在 crate 边界单点
    // 显式拒绝（源码文本扫描已随 #1596 退役，声明面是唯一判定处）。
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
    // 基础设施→域 dev-dependency（ADR-0071 决策 6 修订注记 / #1596 换载体）：
    // 生产依赖越界由上方层秩核对拦下、未声明依赖即编译失败；cargo 对
    // dev-dependency 环放行（测试目标与生产依赖图分离），是唯一盲区——基础设施
    // 对域层 crate（含承载 WHITELIST 域条目的根包）的 `[dev-dependencies]` 逐条
    // 留痕于 INFRA_DOMAIN_ALLOWED_EDGES，清单之外的声明即红。协议层不在本规则
    // 辖下（退役的原 infra→域文本扫描同样不辖），其生产方向仍归层秩核对。
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

  // ⑥ 静态检查与测试命令覆盖全成员（缺 --workspace 即静默漏检成员）
  for (const rel of WORKSPACE_COMMAND_FILES) {
    const abs = join(repoRoot, rel);
    if (!existsSync(abs)) {
      problems.push(`✗ workspace 命令覆盖：宿主文件不存在：${rel}`);
      continue;
    }
    const source = readFileSync(abs, "utf8");
    // `.ts` 宿主的命令面是数组字面量（另核对）；shell / YAML 宿主先掩掉引号内内容，
    // 否则 `echo "…cargo test…"` 这类说明文字会被当成命令（P1 假绿）。
    const isTsHost = rel.endsWith(".ts");
    const text = isTsHost ? source : maskShellQuoted(source);
    let hits = isTsHost ? checkTsCargoArrays(rel, source, problems) : 0;
    text.split("\n").forEach((line, i) => {
      // 注释行不算命令：shell / workflow 用 `#`，登记进来的 .ts 宿主（test-exec.ts）
      // 用 `//` 与 `/** … */`（含 ` * ` 续行），注释里的 `cargo test` 只是说明文字，
      // 不构成命令面。
      const trimmed = line.trim();
      if (trimmed === "" || trimmed.startsWith("#") || isTsCommentLine(trimmed)) return;
      // 逐条命令核对（一行可有 `cargo fmt … && cargo clippy …` 多条：只看首个
      // 匹配会把未覆盖的 clippy 放过去）；命令段截到下一个 shell 控制符为止。
      const re = /\bcargo\s+(clippy|test|fmt)\b/g;
      let m: RegExpExecArray | null;
      while ((m = re.exec(line))) {
        hits += 1;
        const rest = line.slice(m.index);
        const end = rest.search(/&&|;|\|/);
        const segment = end === -1 ? rest : rest.slice(0, end);
        // `--all` 按整词才算 workspace 别名：`--all-targets` / `--all-features`
        // 的 `\b` 落在 `-` 前，用 \b 会假绿（本核对要拦的正是这一形态）。
        if (WORKSPACE_SCOPE_PATTERN.test(segment) || ALL_SCOPE_PATTERN.test(segment)) continue;
        problems.push(
          `✗ workspace 命令覆盖：${rel}:${i + 1} cargo ${m[1]} 缺 --workspace` +
            "（非虚拟 workspace 下默认只作用于根包，会静默漏检成员 crate）\n" +
            `    ${line.trim()}`,
        );
      }
    });
    // 空集拒绝（与覆盖守门的「拒绝以空集假绿」同口径）：宿主里一条命令位置上的
    // cargo 命令都没有——命令被 echo 成说明文字、被注释掉或整段删除时，逐条核对会
    // 变成空转全绿（#1112 第三轮审查 P1）。
    if (hits === 0) {
      problems.push(
        `✗ workspace 命令覆盖：${rel} 未发现任何命令位置上的 cargo 命令` +
          "（引号内的说明文字不算命令）——拒绝以空集假绿，命令被 echo/注释/删除即红",
      );
    }
  }

  // ⑦ 测试导出生产编译门（ADR-0111 决策 5）：测试目标专用导出默认不进生产
  // 编译，由构建形态保证，而非注释约定。删除即变红——clippy 走
  // `--all-features`、测试走 dev-dependency，都发现不了门被摘掉：
  //   ① infra `test_utils` 模块声明须带「放行测试」的 cfg 门（无门/反向门即生产编译）；
  //   ② 生产依赖（`[dependencies]` 与 target 变体）不得对 ledger-infra 启用 test-utils；
  //   ③ 根包与 infra 的 `[features] default` 不得包含 test-utils（默认 feature 即生产）；
  //   ④ 投资五节标题锚点常量（issue #1185，住 `handlers/import.rs`）须带同一形态的门；
  //   ⑤ 锚点再导出（`api_server/mod.rs`）须带同一形态的门。
  // （根包 `test_utils` 再导出面已随 #1108 清除——测试器具经 dev-dependency 以
  // `ledger_infra::test_utils` 直达，根包侧门条目随之退役；再引入无门再导出会被
  // 生产构建解析失败拦下。）
  const gatedDecls = [
    {
      file: join(srcTauriDir, INFRA_SRC_REL, "lib.rs"),
      re: /^\s*pub\s+mod\s+test_utils\s*;/,
      label: "pub mod test_utils;",
      gate: "test_utils 生产编译门",
      src: "ADR-0111 决策 5 / issue #1132",
      productionArtifact: "测试器具",
    },
    // 投资五节标题锚点（issue #1185）：#1121 常量结构锁与 #1123 API 集成锁的
    // 共享单一住处，仅测试构建编译——门摘掉即测试锚点静默进生产二进制。
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

  // infra 清单读一次：test-utils（⑦）与 http（⑧）两道 default 门共用同一份
  // [清单, 出处] 对与同一套核对（defaultFeaturesInclude）。
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

  // ⑧ HTTP 错误响应投影 feature 门（ADR-0111 决策 5 / issue #1133）：`impl
  // IntoResponse for AppError` 因孤儿规则必须住 infra，axum 改 optional、经
  // `http` feature 门控，仅壳侧根包启用——避免每出现一个域 crate 就无条件编入
  // axum 及其传递依赖。五处删除即变红——clippy 走 --all-features、壳侧生产依赖
  // 恒启用 http，都发现不了门被摘掉：
  //   ① infra 的 axum 依赖须声明 optional（裸依赖即门形同虚设）；
  //   ② infra [features] 须有 http 转发 dep:axum（门与依赖面绑死）；
  //   ③ error.rs 的 IntoResponse impl 须带 feature = "http" 的 cfg 门；
  //   ④ 根包与 infra 的 [features] default 不得包含 http（默认 feature 即生产）；
  //   ⑤ 域侧成员 crate 生产依赖不得对 ledger-infra 启用 http（域侧不引 axum）。
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

  // ③' impl cfg 门：error.rs 的 IntoResponse impl 必须带 feature = "http" 的门。
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

  // ⑤' 域侧成员启用 http → 红：http 只许壳侧（根包 tauri-app）启用，域侧启用
  // 即把 axum 编入域依赖图，违背「域侧依赖不引入 axum」口径（issue #1133）。
  for (const crateDir of memberCrateDirs(srcTauriDir)) {
    if (crateDir === dirname(INFRA_SRC_REL)) continue; // infra 自身是门宿主，非消费方
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
    // 域侧直接声明 axum 同样越界（域侧依赖不引入 axum，issue #1133）——不只拦
    // ledger-infra/http 转发一条路；[dev-dependencies] 不在核对范围（测试专用边）。
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

/**
 * 模块面投影核对（#1595，ADR-0056 决策 4 修订注记）：expected 由 crate 根
 * `lib.rs` 的 `mod` 声明派生（形状判定表见文件头），磁盘侧枚举 crate `src`
 * 顶层的实际模块——非测试豁免形态的 `.rs` 文件（去后缀的同名键）与扫得到非测试
 * `.rs` 文件的目录（目录覆盖其全部子目录，子文件不逐行登记）。两侧双向核对：
 * 磁盘上多出未声明的生产模块（孤儿文件/目录）即红；`lib.rs` 声明而 `<key>.rs`
 * 与 `<key>/` 都不存在即红。crate 根 `lib.rs` 是声明与再导出面，不入 expected、
 * 也不参与磁盘枚举（文件名恒定，另一侧承担声明漂移）。
 */
/** 模块面投影核对的规则级指针（T2-2：报文只引一处编号，不插值注记） */
const MODULE_PROJECTION_RULE =
  "模块面 expected 由 crate 根 lib.rs 的 mod 声明投影（ADR-0056 决策 4 修订注记 / #1595）";

/** 磁盘顶层模块键：非测试 `.rs` 文件（去后缀）与扫得到非测试文件的目录名。 */
function diskModuleKeys(srcDir: string): string[] {
  const keys: string[] = [];
  for (const entry of readdirSync(srcDir, { withFileTypes: true })) {
    if (
      entry.isFile() &&
      entry.name.endsWith(".rs") &&
      entry.name !== "lib.rs" &&
      !isTestFile(entry.name)
    ) {
      keys.push(entry.name.replace(/\.rs$/, ""));
    } else if (
      entry.isDirectory() &&
      collectRustFiles(join(srcDir, entry.name), entry.name).length > 0
    ) {
      keys.push(entry.name);
    }
  }
  return keys.sort();
}

/**
 * 派生单个 crate 的模块面并双向核对（#1595）：返回磁盘实际形态的扫描条目；
 * `#[path]` / `include!` / 内联模块块 / 认不出的属性链在此 fail loud。
 */
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
    // 目录形态只在含非测试 Rust 文件时才是模块面的一员（只挂 tests.rs 的目录是
    // 外挂测试豁免形态，ADR-0056 决策 5）。
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

/**
 * lib.rs `mod` 声明扫描（#1593）：模块清单投影的权威源，形状判定表见文件头。
 * 注释与字符串掩码后匹配声明；`#[path]` / `include!` / 内联模块块 / 认不出的
 * 属性链记入 violations（调用方 fail loud），不静默跳过。
 */
interface ModDeclScan {
  /** 计入 expected 的模块名（去 .rs 后缀的模块键），按声明出现顺序 */
  modules: string[];
  /** 认不出的形状（行号 + 形状描述），调用方 fail loud */
  violations: { line: number; shape: string }[];
}

/** 属性链允许的形状：cfg 门、lint 属性与 doc 属性不改变模块↔文件映射。
 *  `#[path]` 改写映射（单列 fail loud），`cfg_attr` 夹带 path 等价。 */
function isRecognizedModAttr(attr: string): boolean {
  if (!/^#\[\s*(?:cfg|cfg_attr|allow|warn|deny|forbid|expect|doc)\b/.test(attr)) return false;
  if (/^#\[\s*cfg_attr\b/.test(attr) && /\bpath\b/.test(attr)) return false;
  return true;
}

/** 声明（idx 处 `mod` 关键字）前的属性链全文，自近及远收集：跳过空白与已掩码的
 *  注释，逐条按配对括号取回 `#[…]`，遇到非属性代码即止。 */
function attributesBefore(text: string, idx: number): string[] {
  const attrs: string[] = [];
  let i = idx - 1;
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

/** 去掉字符串开头连续的属性块（`#[…]`，含同行内联），返回其余文本。 */
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
  const lineOf = (idx: number): number => (code.slice(0, idx).match(/\n/g)?.length ?? 0) + 1;
  const violations: { line: number; shape: string }[] = [];
  const modules: string[] = [];

  // include! 展开出的模块面对文本扫描不可达：fail loud，不静默跳过。
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
    const attrs = attributesBefore(keep, idx);
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
    if (name === "tests") continue; // 测试豁免（ADR-0056 决策 5，与 isTestFile 同规）
    modules.push(name);
  }
  return { modules, violations };
}

/**
 * 交易域 crate 内区级依赖引用扫描（ADR-0113 决策 7 / #1181）：掩码注释与字面量
 * 后匹配 `super::` / `crate::` 前缀 + 目标首段标识符（目录约定 / #1597 后顶层
 * 只有区目录，`crate::` 首段即区目录；深层 `super::` 指向同区子模块，不命中
 * 区目录表即被调用方跳过）；`::{…}` 花括号列举跨行取匹配闭括号后逐条目切分取
 * 首段标识符。捕获 = 目标首段（区归属查表用）。表达式位裸路径（`writer::x`
 * 无前缀形态，依赖边已由 use 语句承载）与别名改写文本不可达，靠评审兜底
 * （与壳层/基础设施扫描同款边界）。
 */
function scanTransactionZoneRefs(text: string): ScanHit[] {
  const masked = maskNonCode(text);
  const rawLines = text.split("\n");
  const hits: ScanHit[] = [];
  const lineOf = (index: number): number => (masked.slice(0, index).match(/\n/g)?.length ?? 0) + 1;
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
    // 花括号列举：跨行取匹配闭括号，深度 0 逐条切分后取各条目首段标识符。
    // `{` 已被正则消费进 m[0]（after 在其后），深度从 1 起数——从 0 起数会使
    // 闭括号落到 -1、close 恒为 -1，body 吞到文件尾、后续枚举变体等被误报。
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

/**
 * 交易域 crate 内区级层序核对（ADR-0113 决策 2/3/7 / #1181 / #1597）：区归属按
 * 目录约定投影——文件的区 = 其所在 crate 顶层区目录的区（TRANSACTION_ZONE_BY_DIR），
 * 同区互依合法；跨区时秩大者方可依赖秩小者（写读 → 接缝 → 共享语义，写读两径
 * 互不依赖）；认许边之外即红。fail loud 两类区登记面违例：① crate 根顶层单
 * 文件（无区目录可投影，不允许「根级单文件默认为共享语义」——那会给写路径模块
 * 留静默错分类的洞）；② 未登记区归属的顶层目录（新区目录漏登区表，判向静默
 * 失靶）。crate 根 lib.rs（声明与再导出面）豁免判向。测试豁免形态由
 * collectRustFiles 过滤（ADR-0056 决策 5）。
 */
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
  if (!existsSync(srcDir)) return problems; // 模块面投影核对逐条报「找不到声明文件」
  for (const f of collectRustFiles(srcDir, "")) {
    const rel = f.rel;
    if (rel === "lib.rs") continue; // crate 根声明与再导出面，免判向（#1181 同规）
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
      // 目标首段非区目录（同区子模块 / 未登记孤儿）不判向：同区子模块随 owner
      // 同区，孤儿文件由模块面投影核对另行报红。
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

/**
 * 单个非测试 Rust 文件的分层与方向扫描（白名单政策项与 crate 模块面共用）：
 * 壳层反向依赖（`options.scanShellRefs` 开启时——只有根包 src 白名单面开，
 * 同 crate 引用 cargo 看不见，属 #1596 退役后仅存的源码方向规则）；基础设施
 * 条目另核 crate 内块间反向依赖（ADR-0111 决策 4 / #1134）。跨 crate 方向不在
 * 本扫描面（#1596：声明面 + 编译期）。报文只给定位 + 分层 + 规则指针（T2-2）。
 */
function scanModuleFile(
  file: RustFileRef,
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
    // crate 内块间反向依赖（ADR-0111 决策 4 / #1134）：db 不得引用 boot / signals
    //（shell_support 已随 #1108 迁出根包）；认许边逐条留痕，清单之外即红。
    // 块名 = 模块键（路径首段去 `.rs`）——目录模块（db/…）与单文件模块（db.rs）
    // 同键，块降为单文件时不静默失靶。
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

/**
 * 条目化扫描（ADR-0056 决策 4）：白名单政策项与 crate 派生模块面共用——条目
 * 必须存在且扫得到非测试 Rust 文件（漂移 fail loud），逐文件交 scanModuleFile。
 * 返回扫到的非测试文件数；返回 0 由调用方统一拒绝（拒绝以空集假绿通过）。
 */
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
    const files: RustFileRef[] = stat.isDirectory()
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

/**
 * crate 模块面扫描（#1595）：条目由 `deriveCrateModules` 的声明投影派生，
 * 逐条交 scanModuleEntries——模块级分层扫描与声明↔磁盘双向全等共用同一 target。
 */
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
    // crate 模块面不做源码方向扫描（#1596：跨 crate 方向归声明面 + 编译期）；
    // 条目存在性与非测试文件核对、基础设施块间禁边照旧。
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

  // 模型域化禁令（规则①/②）+ 原生事务语句禁令（规则③）：全树扫描（壳、域、
  // 基础设施、顶层文件），残留引用可出现在任何层；collectRustFiles 自带测试
  // 豁免（ADR-0056 决策 5）——外挂测试目录的直置事务边界合法。
  // srcDir 整体不可达时静默交由白名单循环报「路径不存在」，不在此抛栈。
  let allFiles: RustFileRef[] = [];
  try {
    allFiles = [
      ...collectRustFiles(srcDir, ""),
      ...CRATE_MODULE_TARGETS.flatMap((target) =>
        collectRustFiles(join(srcTauriDir, target.srcRel), target.srcRel),
      ),
    ];
  } catch {
    // 目录缺失：白名单循环会逐条报错并 fail loud
  }
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

  // 模块面扫描（#1595）：WHITELIST 政策项相对根 src 扫描（壳层反向依赖——同
  // crate 引用，cargo 看不见）；crate 模块面 expected 由各 crate `lib.rs` 的
  // `mod` 声明投影（含声明↔磁盘双向全等），逐条做条目核对与基础设施块间禁边，
  // 源码方向扫描随 #1596 退役给声明面 + 编译期——基础设施模块自 #1088 全量归位
  // 起不再住根 src（ADR-0056「白名单即规格」与决策 4 修订注记不变）。
  scannedFiles += scanModuleEntries(WHITELIST, srcDir, problems, {
    // 壳层反向依赖是仅存的源码方向规则（同 crate 引用，cargo 看不见，#1596）。
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

  // crate 边界核对（spec #1086 / issue #1087）：成员登记、门禁继承、依赖方向、
  // 静态检查/测试命令的 workspace 覆盖——与模块路径白名单并列，同为删除即变红。
  problems.push(...checkCrateBoundaries(srcTauriDir));

  // 交易域区级层序（ADR-0113 决策 2/3/7 / #1181 / #1597）：区归属据
  // TRANSACTION_ZONE_BY_DIR 按目录约定投影判向（根级单文件与未登记区目录 fail
  // loud），认许边（决策 3 原形状反边）之外即红。
  problems.push(...checkTransactionZoneDirection(srcTauriDir));

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
      `· test_utils 生产编译门（cfg 门 + 生产依赖不启用 test-utils，#1132）` +
      `· 投资五节锚点生产编译门（cfg 门，#1185）` +
      `· http 投影 feature 门（axum optional + impl cfg 门 + default 不含 http + 域侧不启用，#1133）`,
  );
}

// 仅直接运行时执行 main；被测试/其他工具 import 时只取导出的扫描函数。
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main();
}
