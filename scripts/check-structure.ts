#!/usr/bin/env bun
// 结构守门（issue #396 / ADR-0056；模型域化禁令 issue #424 / ADR-0059 决策 6）：
// 白名单式分层依赖检查。
// 分层规则：壳 → 域 → 基础设施，域永不依赖壳。白名单 = 已归位域目录 + 全部
// 基础设施（「已验证对壳层零依赖」固化为规格）；白名单内出现对壳层
// （src-tauri/src/commands/）的模块路径依赖即红——每归位一域按 path 字节序插入一行
// 白名单。
// 测试豁免（ADR-0056 决策 5）：外挂测试模块/目录（tests.rs 文件、tests/ 目录）
// 不参与守门——BDD/单元 fixture 合法引用壳层入口，不制造虚假违规；
// 内联 #[cfg(test)] 模块不豁免。白名单路径缺失或条目内扫不到非测试 Rust 文件
// 即红（清单漂移 fail loud）。
// 扫描边界：文本级扫描，注释与字符串/char 字面量掩码后匹配 `commands::`
// 路径引用与 `commands as` 别名引入；经别名改名的间接引用文本不可达，
// 靠评审兜底。
// 基础设施→域扫描（ADR-0071 决策 6 / #538）：基础设施模块（#1088 起住
// `crates/infra/src`，清单见 INFRA_MODULES）内的反向依赖
// 文本级扫描，与壳层扫描同款形态（掩码后匹配、fail loud、外挂测试豁免不变）。
// 匹配限定 crate 根前缀的域模块路径（`crate::`/`tauri_app_lib::` + 域目录名，
// 再随 `::`/` as `/`;`）——不裸匹配域名单词：`sync` 等域名与 std::sync /
// tokio::sync 撞名，裸词形态误报不可用，crate 根限定即 infra→域 import 的
// 文本形状；花括号列举首段（use crate::{accounts::x, …}）同可命中。
// 认许边（INFRA_DOMAIN_ALLOWED_EDGES）：基础设施→域的既有设计意图边逐条
// 留痕于本脚本（精确到文件 + 目标域，附 ADR 指针），与白名单同属「已验证
// 事实固化为规格」；清单之外的基础设施→域引用一律红。首条 db/mod.rs→backup
// 为 ADR-0032 连接层写入口置脏单点（#246）——ADR-0071 §6「落地即全绿」原
// 前提漏数此边，勘误注记见该 ADR（#538 实施时补录）。
// 业务域→同步域零容忍（ADR-0101 决策 4b / #1089 收紧）：同步协议面（命令契约 /
// op 产出 / 设备标识）自 #1089 下放协议 crate（ledger-sync-protocol），业务域对
// 同步域（sync_engine）的引用归零——任何 `crate::sync_engine` /
// `tauri_app_lib::sync_engine` 引用（含根模块引入与别名改写）即红。「重放不产
// 本地 op」从结构巧合升为规格：业务域只依赖协议 crate，重放分派单向住在
// sync_engine。作用域限业务域目录（test_support/ 测试专用边、sync_engine
// 自身、壳层 commands/ 与 tests 不在列）；文本级扫描、注释掩码后匹配，
// 别名改写不可达靠评审兜底。
// 同步协议 crate 模块扫描（spec #1086 / #1089）：协议 crate（业务域与同步域
// 共同底座，仅基础设施在其下）对壳层与全部域目录零依赖——依赖面只有基础设施
// 与数据面惯用库（清单见 PROTOCOL_MODULES），反向引用由 cargo 依赖图拒绝
//（协议 crate 根文档负向用例），本扫描再固化为规格。
// 域间禁边（issue #1090 / spec #1086 形态推广）：三类写路径副作用（余额重算 /
// 计划来源反查 / 期次落账置脏）的域间直接依赖随接缝反转消亡（认许边逐条留痕于
// DOMAIN_PAIR_ALLOWED_EDGES，掩码后匹配，外挂测试豁免不变）。历史规则已随双方
// crate 化全部退役：transaction 起点三条随 #1092、scheduled_transactions→backup
// 一条随 #1098——文本清单不再辖，依赖方向改由 cargo 依赖图编译期拒绝
//（生产依赖面无根包与未声明域）。
// 模型域化禁令（ADR-0059 T7 / #424 收口落地，全树扫描、同样掩码与测试豁免）：
// ① 全局模型模块路径残留禁令——`crate::models` / `tauri_app_lib::models` 即红：
//    全局模型目录已随域归位消亡，防扁平命名空间复活（crate 根裸路径 `models::x`
//    与别名改写文本不可达，靠评审兜底）；
// ② 域模型 glob 再导出禁令——`pub use …model(s)::*`（域接缝或跨域拍平）与
//    域模型文件（model.rs / models.rs）及模型目录（model/ / models/，#1181
//    起判据扩到目录形态，模型目录化不静默失靶）内的 `pub use …::*` 聚合即红，
//    所有权必须逐类型可见（`pub(crate) use` 受限再导出与私有 `use` glob 引入
//    不在文本可辨范围，靠评审兜底）。
// ③ 原生事务语句禁令（issue #1014 / #1003 grilling 定案 7）——产品代码手写
//    `BEGIN`/`COMMIT`/`ROLLBACK` 即红，唯一合法住址 `db/tx_scope.rs`（事务原语
//    本体）；靶形态落在字符串里，扫描保留字符串、只掩码注释；外挂测试豁免不变。
// 交易域模块清单与区级层序（ADR-0113 决策 7 / #1181）：TRANSACTION_MODULES 与
// `crates/transaction/src` 磁盘模块双向全等（新增未登记非测试模块即红；crate
// 根 lib.rs 是声明与再导出面，不入清单也不参与磁盘枚举）；区归属（共享语义 /
// 跨域接缝 / 写路径 / 读路径）在清单条目 zone 字段同址单点声明，据此核对区级
// 层序唯一——写路径/读路径 → 跨域接缝 → 共享语义，共享语义不得依赖接缝与路
// 径区，接缝不得依赖路径区，写读两径互不依赖；同区互依合法。设计意图边
// （ADR-0113 决策 3 登记的原形状反边）逐条留痕 TRANSACTION_ZONE_ALLOWED_EDGES，
// 重排（#1182）消除后同步删除。引用形态：掩码后匹配 `super::`/`crate::` 前缀
// + 目标模块名（含花括号列举逐条展开），flat 布局与重排后区目录两种形状同扫
// 判向不变；表达式位裸路径与别名改写文本不可达，靠评审兜底。
// 模块清单双向全等推广（#1448）：全部 crate 模块清单与磁盘模块双向全等——
// 新增生产模块未登记即红，不再只辖 infra（#1134）/ transaction（#1181）/
// sync-engine（#1107）三面（其余清单磁盘上多出的生产模块曾静默漏过结构守门）；
// 三处专用核对由此合流为通用核对函数，登记面收敛 CRATE_MODULE_LISTS 单表——
// 模块级扫描与双向全等两面共用，新 crate 不可能只接一半。
// 模块清单投影核对（#1593，expand 半：#1591 定案的并行核对期）：模块清单 expected
// 由各 crate 根 lib.rs 的 `mod` 声明文本派生，与手写清单集合等价断言——任一侧
// 单独漂移即红（手写侧多/少一条、lib.rs 声明多/少一条、磁盘多出孤儿文件各自即红）；
// 底层判据「事实有权威源就投影」住 ADR-0056 决策 4 修订注记，手写清单本票不删
//（contract 票 #1595 退役）。形状判定表（ADR-0113 决策 7 / ADR-0111 决策 5 修订
// 注记）：`pub mod x;` / `mod x;` / `pub(crate) mod x;` 与可见性前置的声明，带
// lint 属性（`allow`/`warn`/`deny`/`forbid`/`expect`）、doc 属性或 `#[cfg]` /
// `#[cfg_attr]` 门控者一律计入 expected；`#[cfg(test)] mod tests;` 豁免（ADR-0056
// 决策 5，按目标模块名 tests 判，与磁盘枚举的 isTestFile 同规）；`#[path]`（含
// `cfg_attr` 夹带 path）/ `include!` / 内联模块块 `mod x { … }` / 上述之外认不出的
// 属性链 fail loud（报文给如何登记的指引），不静默跳过。crate 根 lib.rs 自身
// 不是 mod 声明，不入 expected（infra 的 lib.rs 清单条目由磁盘枚举面单独核对）。
// crate 边界核对（spec #1086 / issue #1087 门禁前置）：模块路径白名单之上再加
// crate 级核对——CRATES 是 workspace 成员、分层与允许依赖方向的唯一事实源；
// 成员目录（crates/*）与 CRATES 双向全等（新 crate 未登记即红）；每个成员须写
// `[lints] workspace = true` 继承六件套门禁（漏写即红——clippy 本身不会报）；
// 依赖方向按 壳 → 域 → 基础设施 单向核对；scripts/check.sh、scripts/test.sh 与
// scripts/lint-fix.sh、CI workflow 的 cargo clippy/test/fmt 命令须显式 `--workspace`
// 或 `--all`（非虚拟 workspace 下默认只作用于根包，缺范围参数会静默漏检成员；
// `--all-targets` 等 `--all*` 旗标不算范围——\b 匹配会在这里假绿，故按整词判定）；
// 引号内的命令字样是说明文字不算命令面，且宿主里一条命令都核不到即红（空集假绿，
// 见 maskShellQuoted / checkTsCargoArrays，#1112 第三轮审查）。
// 清单定序与长注记拆行（#1589 排版止血，零守门语义变化）：CRATES 按 name 字节序、
// 模块白名单数组按 path 字节序、market-sync lib.rs/tests.rs 的 mod 声明与再导出同
// 名序——一律按序插入、禁止尾部追加，使不同票的落点自然分离（主撞车形态是同域
// 串行链在同一段追加/改注记）。长注记一律多行字符串拼接：CRATES 按语义片段拆行，
// 片段首冠「依赖面：」「边界：」「豁免：」标签（标签独占一行——相邻片段的不同片段
// 改动就不会落在相邻行，git 三方合并可自动收敛，这是 #1588×#1586 主撞车形态的解；
// CrateEntry.note 不进守门报文，标签与片段边界标点可随拆行微调）；
// MARKET_SYNC_MODULES 按句拆行不加标签，拼接后取值逐字节不变（白名单注记进 5 处
// 守门报文）。清单本身仍手写收口，投影与政策核收缩另见蓝图票 #1591——其 contract
// 票删除清单时一并清走本票拆行/定序工件（退役无悬挂）。
// TypeScript 化 + Bun 运行时（issue #734 / ADR-0083）：类型经 tsconfig.scripts.json
// 门槛检查；调用方式 `bun scripts/check-structure.ts`。
// 默认校验本仓库；测试可传位置参数指向夹具：
// bun scripts/check-structure.ts [src-dir] [src-tauri-dir]
// 挂载于 scripts/check.sh 质量门槛序列与 CI（build.yml frontend job），
// 与命令注册一致性检查并列。

import { existsSync, readdirSync, readFileSync, statSync, type Stats } from "node:fs";
import { dirname, join } from "node:path";
import { pathToFileURL, fileURLToPath } from "node:url";

/** 白名单条目（ADR-0056 决策 4） */
export interface WhitelistEntry {
  path: string;
  layer: Layer;
  note: string;
}

/** 分层词汇（白名单 layer 字段取值）：比较侧单一来源，防字面量漂移 */
export const LAYER = {
  DOMAIN: "域目录",
  INFRA: "基础设施",
  PROTOCOL: "协议",
} as const;

export type Layer = (typeof LAYER)[keyof typeof LAYER];

/**
 * 守门白名单（ADR-0056 决策 4）：路径相对 src-tauri/src。
 * 首批 = 已归位域目录；每迁一域按 path 字节序在此插入一行。基础设施自 #1088 起整体住
 * `crates/infra/src`（不再有根 src 路径），改由下面的 INFRA_MODULES 清单核对。
 * #1091 起域目录开始拆独立 crate（backup 首个），拆出即从本清单移除、改登记
 * CRATES（BACKUP_MODULES 承接模块级扫描）。
 */
export const WHITELIST: readonly WhitelistEntry[] = [
  {
    path: "test_support",
    layer: "域目录",
    note: "测试支持域（统一测试数据库工厂与共享断言库 + 源码扫描掩码器具，ADR-0084 / #751 / #1433；依赖域与基础设施合法，对壳层零依赖）",
  },
];

/**
 * 基础设施 crate 的模块清单（spec #1086 / issue #1088）：路径相对
 * `src-tauri/crates/infra/src`。数据库、错误、设置、文件工具、事件、信号与
 * 闭集全量归位于此，模块级守门（对壳层零依赖、基础设施→域认许边）落在
 * crate 根下扫描；壳层统一读写入口与载荷脱敏（shell_support）已随 #1108
 * 迁出至根包 `src/shell_support`，不再入本清单。
 */
export const INFRA_MODULES: readonly WhitelistEntry[] = [
  {
    path: "boot",
    layer: "基础设施",
    note: "引导层（#1131 自 db 升顶层目录：disposition 启动处置判定与失败门 / data_location 引导 / book_registry 账本注册表 / encryption 加密基座 / passphrase_cache 口令缓存；依赖方向 boot → db 单向，原 db 路径经再导出保持）",
  },
  {
    path: "closed_set.rs",
    layer: "基础设施",
    note: "闭集字符串枚举宏（ADR-0108；模式先例 signals/write_op.rs write_op_set!，ADR-0102）",
  },
  {
    path: "db",
    layer: "基础设施",
    note: "数据库连接与 schema 守卫（#1127 起 mod.rs 只留声明与再导出，按职责分文件：migrate / connection / runtime / query / tx_scope / perf_trace / schema_guard；facade 为 #1408 新增的异步 DB 门面（写读两线程 + 作业通道 + panic 回滚），facade_handles 为 #1410 新增的门面句柄与按槽解析（类型化读 / 写句柄、连接槽对、进程级安装登记），job_gate 为 #1415 新增的限时等待与弃权状态门（决策 8 豁免台账退役），ADR-0125；时间与身份工厂自 #1128 升顶层 ids、引导层五模块自 #1131 升顶层 boot，原 db 路径经再导出保持）",
  },
  { path: "error.rs", layer: "基础设施", note: "错误" },
  {
    path: "events.rs",
    layer: "基础设施",
    note: "事件发射机制（ADR-0054，#408 纳入守门；消费方跨出壳层——备份域、同步域，不随壳机制分组，ADR-0111 决策 2）",
  },
  {
    path: "fs_util.rs",
    layer: "基础设施",
    note: "文件级原子操作工具（备份与 DataLocation 搬迁共用，#408 纳入守门）",
  },
  {
    path: "ids.rs",
    layer: "基础设施",
    note: "时间与身份工厂（当前时刻 / ISO 格式 / UUID v7 与 v5 确定性派生，#1128 自 db 升入——非数据库关切，文件工具等原语引用不穿透 db）",
  },
  {
    path: "lib.rs",
    layer: "基础设施",
    note: "crate 根声明文件（#1134 双向全等起入清单）：pub mod 声明与再导出面（含 test_utils cfg 门，ADR-0111 决策 5 / #1132）——模块清单与 crate 实形的双向全等含根声明文件",
  },
  {
    path: "serde_util.rs",
    layer: "基础设施",
    note: "wire 入参「键缺席 vs null」三态区分器 double_option（键缺席 = 不改 / null = 清空 / 给值 = 落定，#1330 自账户域信用卡档案字段与分类域 icon / parent_id 两份同源拷贝收敛；serde 对 Option<Option<T>> 默认把键缺席与 null 折叠成同一 None，须显式 deserialize_with）",
  },
  { path: "settings.rs", layer: "基础设施", note: "设置" },
  {
    path: "signals",
    layer: "基础设施",
    note: "信号映射（ADR-0044；#1129 起为目录模块，mod.rs 只做声明与再导出，测试外挂 tests/ 与 db/ 同形）",
  },
  {
    path: "test_utils.rs",
    layer: "基础设施",
    note: '测试器具（捕获 tracing 事件的 Layer / 闸门式假发射器，#1088 随类型身份约束归位；`#[cfg(any(test, feature = "test-utils"))]` + `#[doc(hidden)]`，默认不进生产编译，#1132）',
  },
];

/** 基础设施 crate 的模块根（相对 src-tauri），与 CRATES 的 ledger-infra.dir 同源。 */
export const INFRA_SRC_REL = "crates/infra/src";

/**
 * 同步协议 crate 的模块清单（spec #1086 / issue #1089）：路径相对
 * `src-tauri/crates/sync-protocol/src`。多端同步的最底层共享协议——设备标识、
 * 领域命令契约、op 本地记录与读取、流位点；业务域与 sync_engine 的共同底座，
 * 对壳层与全部域目录零依赖（反向引用由 cargo 依赖图拒绝 + 本扫描固化）。
 */
export const PROTOCOL_MODULES: readonly WhitelistEntry[] = [
  {
    path: "command.rs",
    layer: "协议",
    note: "同步命令契约（SyncCommand 实体标签/实体键）与重放效果（ReplayEffect，ADR-0101）",
  },
  {
    path: "device.rs",
    layer: "协议",
    note: "设备标识与逻辑时钟（`sync_device` 单行唯一 SQL 收口，ADR-0091）",
  },
  {
    path: "op.rs",
    layer: "协议",
    note: "op 行落库与读取（`sync_ops` 唯一 SQL 收口）+ 写后钩子登记点（ADR-0091 决策 9）",
  },
  {
    path: "position.rs",
    layer: "协议",
    note: "流位点（`sync_stream_positions` 唯一 SQL 收口，issue #857）",
  },
];

/** 同步协议 crate 的模块根（相对 src-tauri），与 CRATES 的 ledger-sync-protocol.dir 同源。 */
export const PROTOCOL_SRC_REL = "crates/sync-protocol/src";

/**
 * 备份域 crate 的模块清单（spec #1086 / issue #1091）：路径相对
 * `src-tauri/crates/backup/src`。首个自根包域目录拆出的业务域 crate（#1091）——
 * 备份/恢复引擎与自动备份调度。对壳层与全部域目录零依赖：对定时计划域的两条
 * 引用（期次落账置脏的实现接线、追补触发）经注册点反转收敛（挂载点①/④，
 * ADR-0112 决策 5），反向引用由 cargo 依赖图拒绝（生产依赖面无根包，
 * dev-dependency 环只覆盖测试目标）。crate 根 lib.rs 是声明与再导出面
 * （无守门靶向代码），与协议 crate 同款不入清单。
 */
export const BACKUP_MODULES: readonly WhitelistEntry[] = [
  {
    path: "auto.rs",
    layer: "域目录",
    note: "自动备份调度（状态 / 到期判定纯函数 / 三触发入口 / 轮询线程 / 追补触发注册点，挂载点④，issue #1091）",
  },
  {
    path: "engine.rs",
    layer: "域目录",
    note: "备份引擎（zip 打包 / 恢复 / 受管列表与滚动清理，ADR-0007 / ADR-0016）",
  },
];

/** 备份域 crate 的模块根（相对 src-tauri），与 CRATES 的 ledger-backup.dir 同源。 */
export const BACKUP_SRC_REL = "crates/backup/src";

/** 交易域 crate 四区词汇（ADR-0113 决策 2）：区归属登记与层序判向共用，字面量单一来源。 */
export const TRANSACTION_ZONE = {
  SHARED: "共享语义",
  SEAM: "跨域接缝",
  WRITE: "写路径",
  READ: "读路径",
} as const;

export type TransactionZone = (typeof TRANSACTION_ZONE)[keyof typeof TRANSACTION_ZONE];

/** 交易域模块清单条目：白名单条目 + 区归属（ADR-0113 决策 2——区归属与清单同址单点声明，据此判向）。 */
export interface TransactionModuleEntry extends WhitelistEntry {
  zone: TransactionZone;
}

/**
 * 核心交易域 crate 的模块清单（spec #1086 / issue #1092；区归属 ADR-0113 决策
 * 2/7 / #1181）：路径相对 `src-tauri/crates/transaction/src`。P2 首个拆出的底层
 * 业务域 crate——全部业务域可依赖的最底层域。对根包与任何业务域零依赖：对投资/
 * 商户/币种/物品/保单/账户六向的残留边已按挂载点反转收敛（#1092 前置提交），
 * 反向引用由 cargo 依赖图拒绝（生产依赖面无根包，dev-dependency 环只覆盖测试
 * 目标）。crate 根 lib.rs 是声明与再导出面（无守门靶向代码），与协议/备份 crate
 * 同款不入清单，也不参与双向全等的磁盘枚举；`<模块>/tests.rs` 与 `<模块>/tests/`
 * 均为测试豁免形态不入清单。
 *
 * zone 字段按 ADR-0113 决策 2 的消费面判据登记（#1182 重排后的目标形状）：
 * 写读两径可依赖接缝与共享语义，接缝只可依赖共享语义，共享语义是底，写读两径
 * 互不依赖。重排（#1182）已消除三处反边，故 `TRANSACTION_ZONE_ALLOWED_EDGES`
 * 归空：本位币接缝契约随消费概念归共享语义区（`amount/base_currency`）、
 * `model → writer` 转换 impl 搬进写路径、同步命令载荷归共享语义区（op 产出点
 * 留写路径 `write/op.rs`）。
 */
export const TRANSACTION_MODULES: readonly TransactionModuleEntry[] = [
  {
    path: "amount",
    zone: TRANSACTION_ZONE.SHARED,
    layer: "域目录",
    note: "共享语义：金额口径权威（kind 枚举真源 + kind→度量矩阵 + 本位币折算）；子模块 base_currency 承载本位币基准读取接缝契约（ADR-0113 决策 3.1）",
  },
  {
    path: "command",
    zone: TRANSACTION_ZONE.SHARED,
    layer: "域目录",
    note: "共享语义：同步命令载荷契约（payload 载荷 + fields 语义字段，issue #855）；被接缝与写路径共同消费，op 产出点归写路径 write/op.rs（ADR-0113 决策 3.3）",
  },
  {
    path: "model",
    zone: TRANSACTION_ZONE.SHARED,
    layer: "域目录",
    note: "共享语义：域集中模型（transaction / input / normalized / filter / repair，#423 随域归位）；到 writer::NormalizedRow 的转换 impl 归写路径（ADR-0113 决策 3.2）",
  },
  {
    path: "read",
    zone: TRANSACTION_ZONE.READ,
    layer: "域目录",
    note: "读路径：列表与单笔（list.rs）/ 来源列与转换投影（source.rs）/ 搜索与拼音修复（search.rs）",
  },
  {
    path: "seams",
    zone: TRANSACTION_ZONE.SEAM,
    layer: "域目录",
    note: "跨域接缝：商户 / 投资 / 余额刷新 / 出资账户视图 / 来源列反查（计划/保单/物品）；只持契约与注册点",
  },
  {
    path: "search_text.rs",
    zone: TRANSACTION_ZONE.SHARED,
    layer: "域目录",
    note: "共享语义：统一模糊搜索语义纯函数（拼音首字母/子序列/词条匹配，ADR-0027）；被投资域下拉共同消费（ADR-0113 决策 2 先例）",
  },
  {
    path: "write",
    zone: TRANSACTION_ZONE.WRITE,
    layer: "域目录",
    note: "写路径：写入协议（protocol，Local/Replay 同址 ADR-0105）/ 行写入（writer）/ 批量（batch）/ 出资准入（funding）/ op 产出（op）",
  },
];

/** 核心交易域 crate 的模块根（相对 src-tauri），与 CRATES 的 ledger-transaction.dir 同源。 */
export const TRANSACTION_SRC_REL = "crates/transaction/src";

/**
 * 账户域 crate 的模块清单（spec #1086 / issue #1093）：路径相对
 * `src-tauri/crates/accounts/src`。P3 叶子业务域 crate——可被投资域与多端同步
 * 域依赖的独立编译单元。依赖面只有基础设施、同步协议与核心交易域（余额口径
 * 消费 kind→度量矩阵，accounts → transaction 单向），对壳层与同级业务域零依赖：
 * 核心交易域对本域的写路径余额重算（#1090）与出资账户视图（#1092）两条引用
 * 已按挂载点反转收敛，反向引用由 cargo 依赖图拒绝（生产依赖面无根包，
 * dev-dependency 环只覆盖测试目标）。crate 根 lib.rs 是声明与再导出面（无守门
 * 靶向代码），与协议/备份/交易 crate 同款不入清单；tests.rs 与 balance/tests.rs
 * 均为测试豁免形态不入清单。
 */
export const ACCOUNTS_MODULES: readonly WhitelistEntry[] = [
  {
    path: "balance.rs",
    layer: "域目录",
    note: "余额口径权威（实时计算 + V017 余额缓存整体重算刷新与读取 + 余额清单，ADR-0067/ADR-0071；余额刷新接缝实现注册点）",
  },
  {
    path: "command.rs",
    layer: "域目录",
    note: "同步命令（op 载荷形态、产出单点与重放分派，issue #860）",
  },
  {
    path: "core.rs",
    layer: "域目录",
    note: "CRUD / 幂等创建 / 软删除 / 黑洞账户 / 余额调整编排 + 出资账户视图接缝实现（issue #1092）",
  },
  {
    path: "model.rs",
    layer: "域目录",
    note: "域集中模型（账户类型枚举、实体、入参与余额读模型 DTO，#419 随域归位）",
  },
];

/** 账户域 crate 的模块根（相对 src-tauri），与 CRATES 的 ledger-accounts.dir 同源。 */
export const ACCOUNTS_SRC_REL = "crates/accounts/src";

/** 分类域 crate 的模块清单（spec #1086 / issue #1094，P3 叶子域；#1092 的
 * TRANSACTION_MODULES 同形态）：路径相对 `src-tauri/crates/categories/src`。
 * 参考数据三域各自独立 crate 不合并（spec 裁决）；本域无接缝无注册点（op 产出
 * 直呼协议面），依赖面只有基础设施与同步协议。对壳层与全部域目录零依赖：
 * crate 内模块引用壳层/同步域即红（照 TRANSACTION_MODULES 零容忍形态），反向
 * 引用另由 cargo 依赖图拒绝（生产依赖面无根包，dev-dependency 环只覆盖测试
 * 目标）。crate 根 lib.rs 是声明与再导出面（无守门靶向代码），与协议/备份/
 * 交易 crate 同款不入清单；tests.rs 为测试豁免形态不入清单。
 */
export const CATEGORIES_MODULES: readonly WhitelistEntry[] = [
  {
    path: "command.rs",
    layer: "域目录",
    note: "同步命令（issue #860 / ADR-0091）：op 载荷的分类域形态、产出单点与重放执行",
  },
  {
    path: "core.rs",
    layer: "域目录",
    note: "CRUD / 幂等创建 / 软删除 / 两级分类校验 / 预算删除守卫 / 排序重排（issue #91 域内收口）",
  },
  { path: "model.rs", layer: "域目录", note: "分类实体与入参、排序项（#419 随域归位）" },
];

/** 分类域 crate 的模块根（相对 src-tauri），与 CRATES 的 ledger-categories.dir 同源。 */
export const CATEGORIES_SRC_REL = "crates/categories/src";
/**
 * 币种域 crate 的模块清单（spec #1086 / issue #1095）：路径相对
 * `src-tauri/crates/currencies/src`。P3 叶子域，参考数据三域之二——分类/币种/
 * 商户各自独立 crate、不合并（spec 明文裁决）。依赖面只有基础设施、同步协议与
 * 核心交易域——本位币基准读取经注册点供给核心交易域接缝（下层提供实现、壳层
 * 启动接线，ADR-0112 决策 5），对根包与任何同级业务域零依赖，反向引用由 cargo
 * 依赖图拒绝（生产依赖面无根包，dev-dependency 环只覆盖测试目标）。crate 根
 * lib.rs 是声明与再导出面（无守门靶向代码），与协议/备份/交易 crate 同款不入
 * 清单，也不参与双向全等的磁盘枚举；tests.rs 为测试豁免形态不入清单。
 */
export const CURRENCIES_MODULES: readonly WhitelistEntry[] = [
  {
    path: "base_currency.rs",
    layer: "域目录",
    note: "本位币基准（LedgerLevelSetting 首个成员，issue #858 / ADR-0091 决策 3）+ 交易×币种接缝实现注册（#1092）",
  },
  {
    path: "command.rs",
    layer: "域目录",
    note: "账本级设置同步命令（op 载荷形态 LedgerSettingCommand / 产出单点 / 重放分派，issue #858）",
  },
  {
    path: "list.rs",
    layer: "域目录",
    note: "币种清单查询（#404 自命令壳层迁入，IPC 与 HTTP 共用）",
  },
  { path: "model.rs", layer: "域目录", note: "域模型（#418 随域归位）：币种实体 + 汇率实体" },
];

/** 币种域 crate 的模块根（相对 src-tauri），与 CRATES 的 ledger-currencies.dir 同源。 */
export const CURRENCIES_SRC_REL = "crates/currencies/src";

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

/** 交易域区级认许边条目：拥有模块的清单条目路径 + 目标模块键 + 成因留痕 */
interface TransactionZoneEdge {
  file: string;
  target: string;
  reason: string;
}

/**
 * 交易域区级认许边（ADR-0113 决策 3 / #1181）：原形状上与区级层序冲突的既有
 * 设计意图边逐条留痕于本脚本（与 INFRA_DOMAIN_ALLOWED_EDGES 同款纪律），精确到
 * 清单条目路径 + 目标模块键，附 ADR 指针；清单之外的区级反向引用一律红。两条
 * 原反边已由重排票（#1182）消除（本位币接缝归共享语义区、model→writer 转换归
 * 写路径），故本清单归空——再出现区级反向引用即红，不设认许。
 */
export const TRANSACTION_ZONE_ALLOWED_EDGES: readonly TransactionZoneEdge[] = [];

/**
 * 商户域 crate 的模块清单（spec #1086 / issue #1096）：路径相对
 * `src-tauri/crates/merchants/src`。参考数据三域各自独立 crate、不合并（spec
 * #1086 裁决）——商户字典的 CRUD 与按名查找/即建。对壳层与同步域零容忍照扫
 *（与备份/交易 crate 同款，清单条目 layer 为域目录即入业务域扫描面）；对核心
 * 交易域的引用是合法域→域上层依赖（交易×商户接缝的实现注册侧，#1092），由
 * cargo 依赖图与 CRATES 分层核对承担，文本扫描不再辖。crate 根 lib.rs 是声明
 * 与再导出面（无守门靶向代码），与协议/备份/交易 crate 同款不入清单。
 */
export const MERCHANTS_MODULES: readonly WhitelistEntry[] = [
  {
    path: "command.rs",
    layer: "域目录",
    note: "商户同步命令（op 载荷形态、产出单点与重放分派，issue #860）",
  },
  {
    path: "crud.rs",
    layer: "域目录",
    note: "商户字典域行为（列表/创建/改名/软删 + 按名查找/即建 + 交易×商户接缝实现注册，#1092）",
  },
  { path: "model.rs", layer: "域目录", note: "商户域模型（#419 随域归位）" },
];

/** 商户域 crate 的模块根（相对 src-tauri），与 CRATES 的 ledger-merchants.dir 同源。 */
export const MERCHANTS_SRC_REL = "crates/merchants/src";

/**
 * 保单域 crate 的模块清单（spec #1086 / issue #1100）：路径相对
 * `src-tauri/crates/policy/src`。P3 叶子业务域 crate——保单静态档案 CRUD、
 * 保司字典（Insurer，保险域自有独立字典）与保单视角统计。依赖面只有基础设施、
 * 同步协议与核心交易域——统计读路径消费 kind→度量矩阵与本位币折算口径，交易
 * ×保单接缝（#1092）的实现注册侧是域→域合法上层依赖（保单 → 核心交易单向），
 * 对根包与同步域零容忍照扫（与备份/交易 crate 同款，清单条目 layer 为域目录
 * 即入业务域扫描面）；反向引用由 cargo 依赖图拒绝（生产依赖面无根包，
 * dev-dependency 环只覆盖测试目标）。crate 根 lib.rs 是声明与再导出面（无守门
 * 靶向代码），与协议/备份/交易 crate 同款不入清单；tests.rs 为测试豁免形态
 * 不入清单。
 */
export const POLICY_MODULES: readonly WhitelistEntry[] = [
  {
    path: "command.rs",
    layer: "域目录",
    note: "保险域同步命令（issue #860 / ADR-0091）：保单与保司字典的 op 载荷形态、产出单点与重放分派",
  },
  {
    path: "crud.rs",
    layer: "域目录",
    note: "保单档案 CRUD / 软删历史保留（ADR-0051 决策 5）+ 交易×保单接缝实现注册（#1092）",
  },
  {
    path: "insurer.rs",
    layer: "域目录",
    note: "保司字典（issue #712 / ADR-0082）：CRUD / 在用名唯一 / 按名查找与即建",
  },
  {
    path: "model.rs",
    layer: "域目录",
    note: "保单域模型（#420 随域归位）：保单实体 / 建档入参 / 来源列投影 / 统计行",
  },
  {
    path: "stats.rs",
    layer: "域目录",
    note: "保单视角统计（issue #363）：实时推导不落库，度量经交易域 kind→度量矩阵驱动",
  },
  {
    path: "validation.rs",
    layer: "域目录",
    note: "建档/编辑入参校验与归一化（保司在用 / 日期成对 / 保额币种成对）",
  },
];

/** 保单域 crate 的模块根（相对 src-tauri），与 CRATES 的 ledger-policy.dir 同源。 */
export const POLICY_SRC_REL = "crates/policy/src";

/**
 * 定时计划域 crate 的模块清单（spec #1086 / issue #1098）：路径相对
 * `src-tauri/crates/scheduled/src`。定时交易计划/期次引擎/自动执行追补/订阅花费。
 * 对壳层与同步域零容忍照扫（与备份/交易 crate 同款，清单条目 layer 为域目录即入
 * 业务域扫描面）；对核心交易域的引用是合法域→域上层依赖（期次落库/校验经 writer
 * 接缝、花费合计经 amount 矩阵，#1092），由 cargo 依赖图与 CRATES 分层核对承担，
 * 文本扫描不再辖。crate 根 lib.rs 是声明与再导出面（无守门靶向代码），与协议/
 * 备份/交易 crate 同款不入清单，也不参与双向全等的磁盘枚举；tests.rs 与 tests/
 * 均为测试豁免形态不入清单。
 */
export const SCHEDULED_MODULES: readonly WhitelistEntry[] = [
  {
    path: "auto_run.rs",
    layer: "域目录",
    note: "自动执行追补（唯一新增接缝，ADR-0042）：运行时镜像、追补入口、期次落账后置钩子注册点（挂载点③实现侧）与追补触发钩子实现（挂载点④，issue #1090/#1091）",
  },
  {
    path: "command.rs",
    layer: "域目录",
    note: "定时计划同步命令（op 载荷形态、产出单点与重放分派，issue #856 / #860）",
  },
  {
    path: "engine.rs",
    layer: "域目录",
    note: "计划/期次引擎（建档、状态机、期次展开与执行，issue #59/#230/#856）",
  },
  {
    path: "models.rs",
    layer: "域目录",
    note: "域集中模型（#419 随域归位）：计划/期次/扩展实体与入参、闭集枚举",
  },
  {
    path: "source.rs",
    layer: "域目录",
    note: "计划来源反查（spec #704/#1090 接缝反转实现侧）：模块私有，反查行不公开再导出，经 install_plan_source_hook 注册进核心交易域接缝",
  },
  { path: "spend.rs", layer: "域目录", note: "订阅花费双口径（ADR-0023，issue #160/#161/#395）" },
];

/** 定时计划域 crate 的模块根（相对 src-tauri），与 CRATES 的 ledger-scheduled.dir 同源。 */
export const SCHEDULED_SRC_REL = "crates/scheduled/src";

/**
 * 预算域 crate 的模块清单（spec #1086 / issue #1101）：路径相对
 * `src-tauri/crates/budget/src`。P3 叶子业务域 crate——预算 CRUD、软删除与
 * 当前周期进度（实时推导不落库）。依赖面只有基础设施、同步协议与核心交易域
 * ——进度 spent 口径消费 kind→度量矩阵（ExpenseNet），对根包与同级业务域零
 * 依赖，无接缝无注册点、壳层启动零接线；对壳层与同步域零容忍照扫（与备份/
 * 交易 crate 同款，清单条目 layer 为域目录即入业务域扫描面）；反向引用由
 * cargo 依赖图拒绝（生产依赖面无根包，dev-dependency 环只覆盖测试目标）。
 * crate 根 lib.rs 是声明与再导出面（无守门靶向代码），与协议/备份/交易 crate
 * 同款不入清单；tests.rs 为测试豁免形态不入清单。
 */
export const BUDGET_MODULES: readonly WhitelistEntry[] = [
  {
    path: "command.rs",
    layer: "域目录",
    note: "预算同步命令（issue #860 / ADR-0091）：op 载荷形态、产出单点与重放分派",
  },
  {
    path: "crud.rs",
    layer: "域目录",
    note: "预算 CRUD 域行为（issue #91/#183/#184）：写入校验（金额为正/支出分类/「分类+周期」唯一）与软删除",
  },
  {
    path: "model.rs",
    layer: "域目录",
    note: "预算域模型（#420 随域归位）：周期枚举、实体、入参与进度",
  },
  {
    path: "progress.rs",
    layer: "域目录",
    note: "当前周期进度（issue #182）：spent = expense_net 口径，参与 kind 由交易域度量矩阵导出",
  },
];

/** 预算域 crate 的模块根（相对 src-tauri），与 CRATES 的 ledger-budget.dir 同源。 */
export const BUDGET_SRC_REL = "crates/budget/src";

/**
 * 实物资产域 crate 的模块清单（spec #1086 / issue #1102）：路径相对
 * `src-tauri/crates/physical-asset/src`。P3 叶子业务域 crate——大件实物估值
 * 档案的建档（估值必填 = 首条估值历史行）/编辑/估值追加/处置/软删除。依赖面
 * 只有基础设施、同步协议与核心交易域——当前估值折本位币消费交易域 Amount
 * 口径（域间横向依赖，ADR-0056 决策 2 允许）是域→域合法上层依赖
 *（实物资产 → 核心交易单向），对根包与同步域零容忍照扫（与备份/交易 crate
 * 同款，清单条目 layer 为域目录即入业务域扫描面）；无接缝无注册点、壳层启动
 * 零接线（失效信号 notify 回调注入）；反向引用由 cargo 依赖图拒绝（生产依赖
 * 面无根包，dev-dependency 环只覆盖测试目标）。crate 根 lib.rs 是声明与再导
 * 出面（无守门靶向代码），与协议/备份/交易 crate 同款不入清单；域内内联
 * #[cfg(test)] 模块与 tests.rs 为测试豁免形态不入清单。
 */
export const PHYSICAL_ASSET_MODULES: readonly WhitelistEntry[] = [
  {
    path: "command.rs",
    layer: "域目录",
    note: "实物资产同步命令（issue #860 / ADR-0091）：op 载荷形态（建档携带首条估值行、估值追加无实体指向）、产出单点与重放分派",
  },
  {
    path: "crud.rs",
    layer: "域目录",
    note: "域行为（issue #466–#468 / ADR-0064）：建档两表同事务 / 列表与详情（当前估值 = 最新一条 + 在持合计）/ 编辑 / 估值追加 / 处置 / 软删除",
  },
  {
    path: "model.rs",
    layer: "域目录",
    note: "域模型（#419 随域归位）：资产实体 / 入参 / 列表返回（含在持合计）",
  },
  {
    path: "validation.rs",
    layer: "域目录",
    note: "入参校验与归一化（名称 / 金额 / 币种成对 / 日期守卫，issue #467 T2 拆出共享助手）",
  },
];

/** 实物资产域 crate 的模块根（相对 src-tauri），与 CRATES 的 ledger-physical-asset.dir 同源。 */
export const PHYSICAL_ASSET_SRC_REL = "crates/physical-asset/src";

/**
 * 报表域 crate 的模块清单（spec #1086 / issue #1103）：路径相对
 * `src-tauri/crates/reports/src`。P3 叶子业务域 crate——聚合分析读模型（月度
 * 汇总/分类聚合/商户消费排行/报表日期极值），汇总口径全部由核心交易域
 * kind→度量矩阵单一真源驱动。依赖面只有基础设施与核心交易域（允许集
 * 「基础设施、协议、核心交易域」的子集——本域是纯读模型、无同步命令，对同步
 * 协议 crate 亦零生产依赖），无接缝无注册点、壳层启动零接线；对壳层与同步域
 * 零容忍照扫（与备份/交易 crate 同款，清单条目 layer 为域目录即入业务域扫描
 * 面）；反向引用由 cargo 依赖图拒绝（生产依赖面无根包，dev-dependency 环只
 * 覆盖测试目标）。crate 根 lib.rs 是声明与再导出面（无守门靶向代码），与
 * 协议/备份/交易 crate 同款不入清单；tests.rs 为测试豁免形态不入清单。
 */
export const REPORTS_MODULES: readonly WhitelistEntry[] = [
  {
    path: "model.rs",
    layer: "域目录",
    note: "报表读模型（#421 随域归位）：月度汇总行 / 分类份额 / 商户排行行与载荷 / 日期极值对",
  },
];

/** 报表域 crate 的模块根（相对 src-tauri），与 CRATES 的 ledger-reports.dir 同源。 */
export const REPORTS_SRC_REL = "crates/reports/src";

/**
 * 物品域 crate 的模块清单（spec #1086 / issue #1099）：路径相对
 * `src-tauri/crates/item/src`。P3 叶子域——耐用实物物品的 CRUD / 处置与
 * 「每天使用成本」聚合。对壳层与同步域零容忍照扫（与备份/交易 crate 同款，
 * 清单条目 layer 为域目录即入业务域扫描面）；对核心交易域的引用是合法域→域
 * 上层依赖（交易×物品来源列反查接缝的实现注册侧，#1092），由 cargo 依赖图
 * 与 CRATES 分层核对承担，文本扫描不再辖。crate 根 lib.rs 是声明与再导出面
 *（无守门靶向代码），与协议/备份/交易 crate 同款不入清单；tests.rs 与
 * tests/ 均为测试豁免形态不入清单。
 */
export const ITEM_MODULES: readonly WhitelistEntry[] = [
  {
    path: "command.rs",
    layer: "域目录",
    note: "物品同步命令（op 载荷形态、产出单点与重放分派，issue #860）",
  },
  {
    path: "cost.rs",
    layer: "域目录",
    note: "DailyUsageCost 接缝：「每天使用成本」纯计算单一权威（issue #114）",
  },
  {
    path: "domain.rs",
    layer: "域目录",
    note: "域 API 单一权威（创建/修改/处置/删除/列表/成本聚合 + 交易×物品来源列反查接缝实现注册，#1092）",
  },
  {
    path: "guard.rs",
    layer: "域目录",
    note: "溯源守卫（ADR-0025 创建唯一入口的准入接缝，issue #207/#119）",
  },
  { path: "model.rs", layer: "域目录", note: "物品域模型（#420 随域归位）" },
];

/** 物品域 crate 的模块根（相对 src-tauri），与 CRATES 的 ledger-item.dir 同源。 */
export const ITEM_SRC_REL = "crates/item/src";
/**
 * 仪表盘域 crate 的模块清单（spec #1086 / issue #1104）：路径相对
 * `src-tauri/crates/dashboard/src`。P3 叶子业务域 crate——首页净资产跨币种
 * 折算合计（真实财富视角三腿：非投资账户余额 + 持仓市值 + 在持实物资产估值）
 * 与读探针缓存（ADR-0067）。依赖面为修订后票面允许集「基础设施、协议、核心
 * 交易域、账户域、实物资产域」的子集——基础设施（db/error）、账户域（余额口
 * 径与 AccountType）、核心交易域（折算与默认币种口径）、实物资产域（第三腿
 * 在持合计单一读口径，ADR-0064 决策 6；账户域与实物资产域两项系维护者修订
 * 票面 AC 后授权，修订评论留痕），纯读模型无同步命令，对同步协议 crate 亦
 * 零生产依赖；无接缝无注册点、壳层启动零接线；对壳层与同步域零容忍照扫
 *（与备份/交易 crate 同款，清单条目 layer 为域目录即入业务域扫描面）；反向
 * 引用由 cargo 依赖图拒绝（生产依赖面无根包）。crate 根 lib.rs 是声明与再导
 * 出面（无守门靶向代码），与协议/备份/交易 crate 同款不入清单；域内无测试
 * 目标（三层测试全在根包侧），无测试豁免形态条目。
 */
export const DASHBOARD_MODULES: readonly WhitelistEntry[] = [
  {
    path: "model.rs",
    layer: "域目录",
    note: "仪表盘读模型（#421 随域归位）：净资产总览五字段（净额/余额腿/持仓腿/实物腿/基准币种）",
  },
  {
    path: "net_worth.rs",
    layer: "域目录",
    note: "净资产缓存与读探针（issue #491 / ADR-0067，ADR-0071 决策 3 自 db/net_worth.rs 迁入）：输入指纹推导 / 失效判定 / 缓存回写",
  },
];

/** 仪表盘域 crate 的模块根（相对 src-tauri），与 CRATES 的 ledger-dashboard.dir 同源。 */
export const DASHBOARD_SRC_REL = "crates/dashboard/src";

/**
 * 投资域 crate 的模块清单（spec #1086 / issue #1097）：路径相对
 * `src-tauri/crates/investment/src`。P3 业务域 crate——可被行情同步域与多端同步
 * 域依赖的独立编译单元。对壳层与同步域零容忍照扫（与备份/交易 crate 同款，
 * 清单条目 layer 为域目录即入业务域扫描面）；对核心交易域（接缝实现注册侧，
 * #1092）、账户域（余额口径与 AccountType）与币种域（ExchangeRate /
 * ExchangeRateInput，#418 / ADR-0059，#1097 裁决显性化承认）的引用是合法域→域
 * 上层依赖，由 cargo 依赖图与 CRATES 分层核对承担，文本扫描不再辖；行情同步域
 * 的查询半边经注入接缝消费（壳层接线），本域对同步域零直接依赖。crate 根
 * lib.rs 是声明与再导出面（无守门靶向代码），与协议/备份/交易 crate 同款不入
 * 清单；tests.rs 与 tests/ 为测试豁免形态不入清单。
 */
export const INVESTMENT_MODULES: readonly WhitelistEntry[] = [
  {
    path: "backfill.rs",
    layer: "域目录",
    note: "价格历史后台补全的运行态快照与走势空态三态判定（ADR-0122 决策 5 / issue #1377）：补全中（带计数）/ 补全失败待重试 / 无数据——消费派生事实（有价格通道而无历史序列，与补全队列同源）与行情同步域发布的进程内运行态快照（不落库，重启即回补全中）；#1448 补登清单（引入时漏登记）",
  },
  {
    path: "channel.rs",
    layer: "域目录",
    note: "价格通道派生（PriceChannel，issue #1060）——类型 × 市场 × 代码 + 恒定单位价格（ADR-0126）→ 行情/净值/恒定价格/手动报价/无来源的判定单点",
  },
  {
    path: "command.rs",
    layer: "域目录",
    note: "投资同步命令（op 载荷形态、产出单点与重放分派，issue #861）：标的字典/汇率/用户侧价格全域进 OpLog，东财行情外拉数据不进 op",
  },
  {
    path: "constant_price.rs",
    layer: "域目录",
    note: "价格恒定标的（ADR-0126 / issue #1450）：打标单点单向（恒定单位价格列写入 + 净值日期清空）、建档常量价保障与读侧常量取值接缝（装载器 + 周键合成，三消费面共用）",
  },
  {
    path: "crud.rs",
    layer: "域目录",
    note: "标的字典/汇率/现价列表与写入、标的搜索（统一模糊搜索语义）、手动创建守卫与自建标的删除守卫",
  },
  {
    path: "financial_freedom.rs",
    layer: "域目录",
    note: "财务自由度口径——可投资资产 × 3% 安全提取率对年度预算总额的覆盖比例（只读，ADR-0048）；可投资资产分子按两腿拆分（投资账户现金 / 持仓市值），合计 = 两腿之和（issue #1536）",
  },
  {
    path: "fund.rs",
    layer: "域目录",
    note: "场外基金接入——6 位代码校验、行情接入落库半边、AI 降级建行、按代码即拉注入接缝",
  },
  { path: "holdings.rs", layer: "域目录", note: "时点持仓（AsOfHolding）推算单点" },
  {
    path: "lots.rs",
    layer: "域目录",
    note: "持仓批次（security_lots）单点——取批次、逐批次 FIFO 分摊与耗尽批次成本闭合、修改/删除路径的两个精确回补原语（issue #1018）",
  },
  {
    path: "manual_price.rs",
    layer: "域目录",
    note: "手动报价两落点（价格历史周采样 + 现价缓存映像规则）",
  },
  {
    path: "model.rs",
    layer: "域目录",
    note: "域集中模型——全量投资类型与财务自由度总览（#422 随域归位，经 crate 根逐类型再导出禁止 glob）",
  },
  {
    path: "mwr.rs",
    layer: "域目录",
    note: "资金加权收益率（MoneyWeightedReturn，ADR-0115 / issue #1195）——XIRR 求解器（确定性二分）与三消费面读投影（单标的 / 账户级 / 全账级），现金流与区间期初市值口径见模块头注",
  },
  {
    path: "overview.rs",
    layer: "域目录",
    note: "投资概览读数（InvestmentOverview，spec #1532 / issue #1536）——投资页「概览」页签的全页折本位币单值：可投资资产合计 + 现金 / 持仓两腿 + 未计入持仓计数 + 有无投资账户，只读无写入（ADR-0131）",
  },
  { path: "predicates.rs", layer: "域目录", note: "「持仓标的」判定谓词单点（INVESTED_EXISTS）" },
  {
    path: "prices.rs",
    layer: "域目录",
    note: "价格写入单点——现价缓存 upsert、价格历史周采样 upsert、价格刻度换算、价格来源标记（存量 eastmoney / 新写入按实际取数源，ADR-0130 决策 7）（#401 自 sync/persist 迁入）",
  },
  {
    path: "quote.rs",
    layer: "域目录",
    note: "行情接入接缝（QuoteAdoption，ADR-0103）——统一报价载荷 Quote 与落库半边 adopt_quote；查询半边实现在行情同步域网络层、经注入签名供给",
  },
  {
    path: "reports.rs",
    layer: "域目录",
    note: "已实现盈亏汇总与按币种累计收益查询（issue #1077）",
  },
  { path: "source.rs", layer: "域目录", note: "交易列表标的来源反查（spec #704 / issue #709）" },
  {
    path: "split.rs",
    layer: "域目录",
    note: "份额调整（split）批次成本重述单点——按比例重述在用批次与审计落库（ADR-0106 决策 2/3，issue #1049）",
  },
  {
    path: "staleness.rs",
    layer: "域目录",
    note: "价格过期检查（issue #1190）：打开投资页时的本地水位检查单点——零网络请求，消费现价缓存既有两条水位（行情 priced_at / 净值 nav_date），水位日超阈值自然日或持仓缺现价计入过期；#1448 补登清单（引入时漏登记）",
  },
  {
    path: "stock.rs",
    layer: "域目录",
    note: "股票按（市场，代码）查询的领域规则——代码形态 → 市场单点推断、报价币种推导（issue #693 / ADR-0081）",
  },
  {
    path: "trade.rs",
    layer: "域目录",
    note: "buy/sell/convert/split/dividend 协议分派与买卖/转换/份额调整明细投影（TransactionTrade / TransactionConvert）",
  },
  {
    path: "transaction_seam.rs",
    layer: "域目录",
    note: "交易域接缝实现（spec #1086 / issue #1092）——投资 kind 写路径装配/副作用与读路径投影的实现注册面，install_transaction_hooks 一次性装入（壳层启动接线）",
  },
  { path: "trend.rs", layer: "域目录", note: "单标的 / 组合走势查询" },
  {
    path: "unwind.rs",
    layer: "域目录",
    note: "持仓副作用撤销（Unwind）——修改/删除路径的守卫 → 级联/回补 → 清理模板单点（issue #1020，父 spec #1005 决策 D2/D3）",
  },
];

/** 投资域 crate 的模块根（相对 src-tauri），与 CRATES 的 ledger-investment.dir 同源。 */
export const INVESTMENT_SRC_REL = "crates/investment/src";

/**
 * 行情同步域 crate 的模块清单（spec #1086 / issue #1106）：路径相对
 * `src-tauri/crates/market-sync/src`。P4 首个业务域 crate——行情抓取（批量报价 /
 * 单点行情 / 日 K / 历史净值）与增量同步编排，成为可被多端同步域依赖的独立编译
 * 单元。依赖面为票面 AC 允许集全量：基础设施（db / error / events）、同步协议
 *（op 落库行的 device_id）、核心交易域（币种缺省推导）与投资域（价格写入单点 /
 * 名称随行刷新 / 通道派生 / 统一报价载荷，ADR-0103），均为上层域消费下层域的合法
 * 直呼（ADR-0112 决策 2）；对壳层与多端同步域零容忍照扫（与备份/交易/投资 crate
 * 同款，清单条目 layer 为域目录即入业务域扫描面）；反向引用由 cargo 依赖图拒绝
 *（生产依赖面无根包）。crate 根 lib.rs 是声明与再导出面（无守门靶向代码），与
 * 协议/备份/交易 crate 同款不入清单；tests.rs 与 tests/ 为测试豁免形态不入清单。
 */
export const MARKET_SYNC_MODULES: readonly WhitelistEntry[] = [
  {
    path: "bulk.rs",
    layer: "域目录",
    note:
      "行情批量取数面（ADR-0121 / issue #1374）：名称全量字典 + 场外基金净值全市场批量面（各整次同步一次请求）、fail-closed 降级、同步内熔断、跨同步记忆（BulkFetchCircuit）与覆盖缺口容忍；" +
      "取数方式与价格来源正交",
  },
  {
    path: "channels.rs",
    layer: "域目录",
    note:
      "同步网络通道束（issue #1276）：六个抓取闭包的打包形态与生产/测试换装接缝——生产接 HTTP 层（主机池/限流 pacer 单点，报价闭包经腾讯批量报价 issue #1560、日 K 闭包经腾讯 K 线 issue #1561），测试注入桩经命令壳 SyncChannelsSlot 换装使「同步真实在途」可确定复现；" +
      "编排本体经 do_incremental_sync_channels 单点拆交",
  },
  {
    path: "csrc.rs",
    layer: "域目录",
    note:
      "证监会基金电子披露取数单元（issue #1562 / ADR-0130）：官方场外基金净值披露的单只基金区间查询与解析——名称、单位净值、累计净值、净值日期与货基自报形态信号（ADR-0126 决策 3 换源后的确认源，#1563 接线）；" +
      "DataTables 参数全集请求构造单点（缺参数即 500 系统异常的参数门槛）、汇总行与份额行混排过滤、已终止基金可取；" +
      "异常响应 fail-closed 报 sync.disclosure-source-malformed，不误判查无此码；" +
      "已终止基金存在性与最后一期净值的权威兑底面（#1568 接线）",
  },
  {
    path: "daily_refresh.rs",
    layer: "域目录",
    note:
      "现价刷新的后台每日形态（ADR-0122 决策 3 / issue #1377）：启动后延迟补跑一次 + 每自然日窗口一次（自然日窗口巡检与进程级单次拉起守卫）；" +
      "与手动形态同编排、同进度事件、同收尾裁决，差异只有触发方式、后台车道与静默失败面；" +
      "单轮骨架（换装/会话/见证/裁决/发射/失败日志）经 lane.rs 单点（issue #1426），本模块只留编排与统计日志",
  },
  {
    path: "ecb.rs",
    layer: "域目录",
    note:
      "ECB 参考汇率取数单元（ADR-0019 修订记录 / issue #1542）：全量历史与 90 天增量两个取数入口、Cube 报文解析、同日两腿交叉推导（EUR 作基准腿，缺腿日跳过不猜值）与周采样序列产出（可落库形态）；" +
      "非预期形状（空 / 非 XML / 截断）报 fx.source-malformed 码化错误，不静默产出空序列；" +
      "本票只产出序列，不落库、不接 UI",
  },
  {
    path: "fund.rs",
    layer: "域目录",
    note: "东财基金报价访问（按 6 位代码即拉，issue #301 / ADR-0038；搜索建议未命中回退档案通道改判存在，issue #1212）——行情接入接缝查询半边的场外实例，统一载荷 investment::Quote（ADR-0103）",
  },
  {
    path: "fund_backfill.rs",
    layer: "域目录",
    note:
      "基金历史回填单元（issue #1062 / #1377 / #1388 自 fund_nav 拆出）：服务价格历史后台补全的逐只编排——首刷判据 = 磁盘无历史序列、首刷近两年（单请求全量通道优先、fail-closed 回退分页、页数上限 40）、增量按水位；" +
      "一只一事务不留半根历史",
  },
  {
    path: "fund_nav.rs",
    layer: "域目录",
    note:
      "东财历史净值共享件（issue #303 / ADR-0038 决策 6；issue #1388 拆出编排单元后留守）：lsjz / 详情页数据文件访问与报文解析、货基口径、净值水位窗口、分页器与水位读；" +
      "单请求全量通道 fail-closed 回退分页（issue #1062）",
  },
  {
    path: "fund_price_refresh.rs",
    layer: "域目录",
    note:
      "基金现价刷新单元（issue #1377 / #1388 自 fund_nav 拆出）：服务标的信息同步的逐只编排——批量面命中零请求（判定 + 直落库）、未命中退逐只短窗封顶 2 页、无历史序列者一个月短窗；" +
      "FundSyncStats 统计",
  },
  {
    path: "history.rs",
    layer: "域目录",
    note:
      "价格历史后台补全（ADR-0122 / issue #1375）：派生事实队列（有价格通道但历史不完整，持仓优先）+ 一轮排空（与手动同步共用的单只回填单元，单只失败不中断、幂等无冲突）+ 启动延迟与自然日窗口调度（后台服务编排单点接线，issue #961 名单）+ 收尾裁决（置脏 + 价格失效信号，成败同判）；" +
      "后台车道生产束（全局限速器让行前台）经 BackfillChannelsSlot 注入接缝换装",
  },
  {
    path: "http.rs",
    layer: "域目录",
    note:
      "行情 HTTP 网络层（issue #89）：多主机切换 / 重试 / 限流冷却 / Referer 与日 K、汇率 K 报文解析；" +
      "价格换算按精度位单点（单点行情用，#695）",
  },
  {
    path: "incremental.rs",
    layer: "域目录",
    note:
      "标的信息同步编排（issue #103 / #137 / #303 / #695 / #827 / #1560）：腾讯批量报价 upsert 现价（行情日期取交易所当地交易日，来源标记 tencent）+ 汇率 K 线 + 基金净值按水位增量 + 数据源权威名称随行刷新；" +
      "抓取通道全部经闭包注入，编排不碰网络（近两年日 K 周采样已随 ADR-0122 / #1377 移出编排，归价格历史后台补全）",
  },
  {
    path: "js.rs",
    layer: "域目录",
    note: "JS 文本字面量提取原语（fund_nav / bulk）：从 `.js` 数据文件的 `var x = […]` 与对象字段 `datas:[…]` 两种赋值形态取出数组 / 字符串字面量，被拦截形态天然缺声明即返回 None",
  },
  {
    path: "lane.rs",
    layer: "域目录",
    note:
      "后台车道单轮骨架（issue #1426）：价格历史补全与每日现价刷新两条后台车道共用的单轮单点——通道束换装（管理态桩槽优先、生产后台车道束兜底）、门面写槽裸作业会话、进度发射接线（事件名按车道选）、写入见证、收尾裁决（置脏 + 价格失效信号，成败同判）与失败日志；" +
      "车道侧只留编排（LaneRound 实现）与统计日志（ADR-0122 决策 3「同形调度」的代码单点）",
  },
  {
    path: "model.rs",
    layer: "域目录",
    note: "域模型（#407 随域归位）：标的信息同步结果类型 SyncInstrumentInfoResult",
  },
  {
    path: "persist.rs",
    layer: "域目录",
    note:
      "行情同步持久化（issue #137）：ECB 汇率落库单元（issue #1543）——周采样序列 → fx_rate_history（币种对 × 周键整周覆盖幂等）+ 当期汇率表（每对最新一条，经投资域 upsert_auto_exchange_rate 人工行保护），单一事务、不产同步 op；" +
      "东财 FX 通道的周采样 upsert 同住（#1551 退役前）；" +
      "价格写入单点已随投资域归位迁入 ledger_investment::prices，#401",
  },
  {
    path: "progress.rs",
    layer: "域目录",
    note: "同步进度事件（issue #897 / ADR-0095；页级明细 issue #1061）：事件名常量、payload 与 ProgressEmitter 发射器接缝收口（用后即弃的非失效信号，经 events 机制投递）",
  },
  {
    path: "session.rs",
    layer: "域目录",
    note:
      "作用域会话接缝（issue #1275 / ADR-0112 决策 5 挂载点⑥）：编排获取数据库连接的唯一通道——域定义 ScopedSession trait，壳层实现并在命令壳接线；" +
      "编排抓取路径在类型上取不到连接",
  },
  {
    path: "sina_fund.rs",
    layer: "域目录",
    note:
      "新浪场外基金取数单元（ADR-0130 决策 2 / issue #1564）：批量最新净值面（f_ 前缀一次请求多只，GBK、必须带 Referer）与单只全历史面（一次请求取整只历史，含已终止基金末点）；" +
      "货基行的字段错位（万份收益放在单位净值位）按「前一日单位净值位为空」单点判别并显式分类，错位行不产出价格点（ADR-0130 决策 6，判定打标信号归官方披露面 #1563）；" +
      "全历史空序列不等于查无此码，非预期形状 fail-closed；" +
      "本票只取数与解析，接线随 #1565 / #1566",
  },
  {
    path: "stock.rs",
    layer: "域目录",
    note: "东财股票单点行情访问（issue #693 / ADR-0081）：按（市场，代码）实时查询，类型特征探测与更新时间戳投影单点隔离——接缝查询半边的场内实例",
  },
  {
    path: "tencent.rs",
    layer: "域目录",
    note:
      "腾讯行情批量报价取数单元（ADR-0130 决策 2/3 / issue #1558）：一次请求携带多只沪深港美股票与场内基金（GBK、无需 Referer），解出代码 / 名称 / 价格 / 价格日期 / 证券类型码 / 币种 / 交易所后缀；" +
      "三套字段布局与类型探测收口单点，非预期响应 fail-closed；" +
      "场内现价刷新接线见 channels（issue #1560），按代码查询 / 创建接线随 #1567",
  },
  {
    path: "tencent_kline.rs",
    layer: "域目录",
    note:
      "腾讯日线 K 线取数单元（ADR-0130 决策 2 / issue #1559，接线 issue #1561）：市场 + 代码 → 腾讯查询键（沪深港前缀 + 美股三市场交易所后缀）、区间 / 根数参数与日线报文解析（收盘价在下标 2、港美行可带多余元素）；" +
      "无效代码返回空序列而非错误，非预期形状 fail-closed；" +
      "历史补全通道束的日 K 闭包即本单元（#1561）",
  },
];

/** 行情同步域 crate 的模块根（相对 src-tauri），与 CRATES 的 ledger-market-sync.dir 同源。 */
export const MARKET_SYNC_SRC_REL = "crates/market-sync/src";

/**
 * 多端同步域 crate 的模块清单（spec #1086 / issue #1107）：路径相对
 * `src-tauri/crates/sync-engine/src`。P4 业务域 crate——OpLog 基座、双端合并
 * 语义、Checkpoint 与新端引导、通道/信封与同步触发编排，成为在协议 crate 之上、
 * 依赖各业务域的独立编译单元。依赖面只有基础设施、同步协议、核心交易域与各
 * 业务域（重放分派经各域公开接缝消费）；对壳层零依赖，反向引用由 cargo 依赖图
 * 与结构守门共同拒绝（CRATES 依赖方向核对 + 模块级对壳层扫描）。同步域自身是
 * 业务域→同步域零容忍规则的边界（协议面下放 #1089；本 crate 不再被业务域依赖），
 * 故本清单不参与 `isBusinessDomain` 扫描。crate 根 lib.rs 是声明与再导出面
 * （无守门靶向代码），与协议/备份/交易 crate 同款不入清单；tests.rs 与 tests/
 * 均为测试豁免形态不入清单。
 */
export const SYNC_ENGINE_MODULES: readonly WhitelistEntry[] = [
  {
    path: "channel.rs",
    layer: "域目录",
    note: "通道层（ADR-0091 决策 1/8/9）：目录布局与 manifest、发布/拉取同步轮次、Checkpoint 通道传递、通道条目摘要与损坏码化错误单点",
  },
  {
    path: "checkpoint.rs",
    layer: "域目录",
    note: "Checkpoint 产出（全量快照 + 位点）、新端引导与截断机制（issue #857，v1 永不截断、机制默认不启用）",
  },
  {
    path: "command.rs",
    layer: "域目录",
    note: "跨端语义命令信封与重放契约（DomainCommand / ReplayBinding，只增不改）——同步域对业务域暴露的契约面（ADR-0101 决策 4b）；ReplayEffect/SyncCommand 已下放协议 crate",
  },
  {
    path: "engine.rs",
    layer: "域目录",
    note: "同步引擎公开接口：幂等重放 / wire 接入 / 日志读取全序 / 位点后增量 / 挂起队列清单——本域行为唯一断言权威层",
  },
  {
    path: "envelope.rs",
    layer: "域目录",
    note: "SyncEnvelope 信封加密（ADR-0091 决策 8）：AES-256-GCM 与 PBKDF2-HMAC-SHA512，明文直通/密文自描述双形态",
  },
  { path: "model.rs", layer: "域目录", note: "op 信封 wire 模型（SyncOp）：全序字段 + 载荷 JSON" },
  {
    path: "ops.rs",
    layer: "域目录",
    note: "op 行落库与读取的适配层（域信封 ↔ JSON；SQL 收口在协议 crate，ADR-0091/#1089）",
  },
  {
    path: "parked.rs",
    layer: "域目录",
    note: "ParkedOp 挂起队列（不可重放 op 的统一归宿与码化错误，ADR-0091）",
  },
  {
    path: "registry.rs",
    layer: "域目录",
    note: "重放注册表（ADR-0101）：14 个语义命令类型的适配绑定与 DomainCommand::subject 组装臂",
  },
  {
    path: "transport",
    layer: "域目录",
    note: "S3 兼容对象存储通道后端（s3.rs：SigV4 客户端、path-style/virtual-host 寻址、同步桥接线程）",
  },
  {
    path: "transport.rs",
    layer: "域目录",
    note: "Transport 哑字节通道抽象（v1 唯一后端是 S3 兼容对象存储；WebDAV 已随 #1221 退役）与错误归类单点",
  },
  {
    path: "trigger",
    layer: "域目录",
    note: "同步触发编排（ADR-0098）：通道配置与构库单点、轮次编排、打开即同步与桌面低频轮询、写后去抖合流、会话密钥形态判定的信封模式",
  },
];

/** 多端同步域 crate 的模块根（相对 src-tauri），与 CRATES 的 ledger-sync-engine.dir 同源。 */
export const SYNC_ENGINE_SRC_REL = "crates/sync-engine/src";

/**
 * crate 模块清单登记面（#1448 自 #1134/#1181/#1107 三处专用接线合流）：模块级
 * 扫描（scanModuleEntries）与清单↔磁盘双向全等（checkModuleListEquality）共用
 * 的单一事实源——新域 crate 落地在此追加一行，两面不可能只接一半（此前清单级
 * 双向全等只辖 infra/transaction/sync-engine 三面，其余清单磁盘上多出的生产
 * 模块静默漏过结构守门，#1448 现场发现）。WHITELIST（根 src 的 test_support）
 * 不入本表：根包 src 是壳层，模块面不整册登记。
 */
export interface CrateModuleListSpec {
  /** 清单常量名（核对报文用，如 'INFRA_MODULES'） */
  label: string;
  /** 模块清单本体（上方导出常量，条目级事实源） */
  modules: readonly WhitelistEntry[];
  /** 模块根（相对 src-tauri），与 CRATES 的 dir 同源 */
  srcRel: string;
  /** 摘要行展示名（如 '基础设施'、'账户域'） */
  summaryName: string;
  /** 摘要行 crate 出处括注；无括注的清单（基础设施/协议先行于逐域拆分登记）留空 */
  summaryIssue?: string;
  /** 漏登记报文的出处括注（双向全等对该清单生效的来源） */
  provenance: string;
  /** 漏登记报文的登记面提示尾巴（transaction 清单条目带区归属） */
  registerNote: string;
  /** 漏登记的失靶面尾注 */
  why: string;
  /** crate 根 lib.rs 免登清单：磁盘枚举恒排除 lib.rs（infra 例外——lib.rs 已入清单） */
  excludeCrateRoot: boolean;
  /** 业务域→同步域零容忍扫描关闭（仅同步域自身，#1107） */
  scanBusinessSyncRefs?: boolean;
}

export const CRATE_MODULE_LISTS: readonly CrateModuleListSpec[] = [
  {
    label: "INFRA_MODULES",
    modules: INFRA_MODULES,
    srcRel: INFRA_SRC_REL,
    summaryName: "基础设施",
    provenance: "ADR-0111 决策 5 / #1134",
    registerNote: "（附注释）",
    why: "块间分层与认许边核对静默漏检",
    excludeCrateRoot: false,
  },
  {
    label: "PROTOCOL_MODULES",
    modules: PROTOCOL_MODULES,
    srcRel: PROTOCOL_SRC_REL,
    summaryName: "协议",
    provenance: "#1448",
    registerNote: "（附注释）",
    why: "协议 crate 对壳层与域目录零依赖扫描静默漏检",
    excludeCrateRoot: true,
  },
  {
    label: "BACKUP_MODULES",
    modules: BACKUP_MODULES,
    srcRel: BACKUP_SRC_REL,
    summaryName: "备份域",
    summaryIssue: "#1091",
    provenance: "#1448",
    registerNote: "（附注释）",
    why: "域 crate 对壳层零依赖扫描静默漏检",
    excludeCrateRoot: true,
  },
  {
    label: "TRANSACTION_MODULES",
    modules: TRANSACTION_MODULES,
    srcRel: TRANSACTION_SRC_REL,
    summaryName: "核心交易域",
    summaryIssue: "#1092",
    provenance: "ADR-0113 决策 7 / #1181",
    registerNote: "（区归属 + 注释）",
    why: "区级层序与认许边核对静默漏检",
    excludeCrateRoot: true,
  },
  {
    label: "ACCOUNTS_MODULES",
    modules: ACCOUNTS_MODULES,
    srcRel: ACCOUNTS_SRC_REL,
    summaryName: "账户域",
    summaryIssue: "#1093",
    provenance: "#1448",
    registerNote: "（附注释）",
    why: "域 crate 对壳层零依赖扫描静默漏检",
    excludeCrateRoot: true,
  },
  {
    label: "CATEGORIES_MODULES",
    modules: CATEGORIES_MODULES,
    srcRel: CATEGORIES_SRC_REL,
    summaryName: "分类域",
    summaryIssue: "#1094",
    provenance: "#1448",
    registerNote: "（附注释）",
    why: "域 crate 对壳层零依赖扫描静默漏检",
    excludeCrateRoot: true,
  },
  {
    label: "MERCHANTS_MODULES",
    modules: MERCHANTS_MODULES,
    srcRel: MERCHANTS_SRC_REL,
    summaryName: "商户域",
    summaryIssue: "#1096",
    provenance: "#1448",
    registerNote: "（附注释）",
    why: "域 crate 对壳层零依赖扫描静默漏检",
    excludeCrateRoot: true,
  },
  {
    label: "CURRENCIES_MODULES",
    modules: CURRENCIES_MODULES,
    srcRel: CURRENCIES_SRC_REL,
    summaryName: "币种域",
    summaryIssue: "#1095",
    provenance: "#1448",
    registerNote: "（附注释）",
    why: "域 crate 对壳层零依赖扫描静默漏检",
    excludeCrateRoot: true,
  },
  {
    label: "POLICY_MODULES",
    modules: POLICY_MODULES,
    srcRel: POLICY_SRC_REL,
    summaryName: "保单域",
    summaryIssue: "#1100",
    provenance: "#1448",
    registerNote: "（附注释）",
    why: "域 crate 对壳层零依赖扫描静默漏检",
    excludeCrateRoot: true,
  },
  {
    label: "SCHEDULED_MODULES",
    modules: SCHEDULED_MODULES,
    srcRel: SCHEDULED_SRC_REL,
    summaryName: "定时计划域",
    summaryIssue: "#1098",
    provenance: "#1448",
    registerNote: "（附注释）",
    why: "域 crate 对壳层零依赖扫描静默漏检",
    excludeCrateRoot: true,
  },
  {
    label: "BUDGET_MODULES",
    modules: BUDGET_MODULES,
    srcRel: BUDGET_SRC_REL,
    summaryName: "预算域",
    summaryIssue: "#1101",
    provenance: "#1448",
    registerNote: "（附注释）",
    why: "域 crate 对壳层零依赖扫描静默漏检",
    excludeCrateRoot: true,
  },
  {
    label: "PHYSICAL_ASSET_MODULES",
    modules: PHYSICAL_ASSET_MODULES,
    srcRel: PHYSICAL_ASSET_SRC_REL,
    summaryName: "实物资产域",
    summaryIssue: "#1102",
    provenance: "#1448",
    registerNote: "（附注释）",
    why: "域 crate 对壳层零依赖扫描静默漏检",
    excludeCrateRoot: true,
  },
  {
    label: "REPORTS_MODULES",
    modules: REPORTS_MODULES,
    srcRel: REPORTS_SRC_REL,
    summaryName: "报表域",
    summaryIssue: "#1103",
    provenance: "#1448",
    registerNote: "（附注释）",
    why: "域 crate 对壳层零依赖扫描静默漏检",
    excludeCrateRoot: true,
  },
  {
    label: "ITEM_MODULES",
    modules: ITEM_MODULES,
    srcRel: ITEM_SRC_REL,
    summaryName: "物品域",
    summaryIssue: "#1099",
    provenance: "#1448",
    registerNote: "（附注释）",
    why: "域 crate 对壳层零依赖扫描静默漏检",
    excludeCrateRoot: true,
  },
  {
    label: "DASHBOARD_MODULES",
    modules: DASHBOARD_MODULES,
    srcRel: DASHBOARD_SRC_REL,
    summaryName: "仪表盘域",
    summaryIssue: "#1104",
    provenance: "#1448",
    registerNote: "（附注释）",
    why: "域 crate 对壳层零依赖扫描静默漏检",
    excludeCrateRoot: true,
  },
  {
    label: "INVESTMENT_MODULES",
    modules: INVESTMENT_MODULES,
    srcRel: INVESTMENT_SRC_REL,
    summaryName: "投资域",
    summaryIssue: "#1097",
    provenance: "#1448",
    registerNote: "（附注释）",
    why: "域 crate 对壳层零依赖扫描静默漏检",
    excludeCrateRoot: true,
  },
  {
    label: "MARKET_SYNC_MODULES",
    modules: MARKET_SYNC_MODULES,
    srcRel: MARKET_SYNC_SRC_REL,
    summaryName: "行情同步域",
    summaryIssue: "#1106",
    provenance: "#1448",
    registerNote: "（附注释）",
    why: "域 crate 对壳层零依赖扫描静默漏检",
    excludeCrateRoot: true,
  },
  {
    label: "SYNC_ENGINE_MODULES",
    modules: SYNC_ENGINE_MODULES,
    srcRel: SYNC_ENGINE_SRC_REL,
    summaryName: "多端同步域",
    summaryIssue: "#1107",
    provenance: "#1107",
    registerNote: "（附注释）",
    why: "域 crate 对壳层零依赖扫描静默漏检",
    excludeCrateRoot: true,
    scanBusinessSyncRefs: false,
  },
];

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

/** crate 边界条目（单一事实源，spec #1086 / issue #1087）：dir 相对 src-tauri。 */
export interface CrateEntry {
  name: string;
  dir: string;
  layer: CrateLayer;
  note: string;
}

/**
 * crate 边界清单：workspace 成员、分层与允许的依赖方向（壳 → 域 → 基础设施）
 * 的唯一事实源——结构守门据此核对成员登记、门禁继承与依赖方向；每拆一个域
 * crate 按 name 字节序在此插入一行（与 WHITELIST 同为「已验证事实固化为规格」）。
 *
 * 定序（#1589）：条目按 name 字节序插入（禁止尾部追加）——不同票的落点自然
 * 分离，多票同段行级冲突降为自动合并或平凡冲突。长注记按语义片段拆行（见
 * 文件头注释），拼接后取值不变。
 */
export const CRATES: readonly CrateEntry[] = [
  {
    name: "ledger-accounts",
    dir: "crates/accounts",
    layer: CRATE_LAYER.DOMAIN,
    note:
      "账户域 crate（#1093，P3 叶子业务域 crate：账户 CRUD/余额口径与余额缓存/同步命令，可被投资域与多端同步域依赖）；" +
      "依赖面：" +
      "只有基础设施、同步协议与核心交易域（accounts → transaction 单向，ADR-0071 决策 5 修订后方向）——核心交易域写路径的余额刷新与出资账户视图两处接缝实现住本域、壳层启动接线；" +
      "边界：" +
      "反向引用由生产依赖面编译期拒绝（dev-dependency 环只覆盖测试目标）",
  },
  {
    name: "ledger-backup",
    dir: "crates/backup",
    layer: CRATE_LAYER.DOMAIN,
    note:
      "备份域 crate（#1091 首个自根包域目录拆出的业务域 crate：备份/恢复引擎与自动备份调度，spec #1086；#1105 复核归位完整性——域逻辑已全量在 crate，根包仅余壳层命令与启动对装）；" +
      "依赖面：" +
      "只有基础设施——对定时计划域的置脏实现与追补触发两条引用经注册点反转（挂载点①/④，ADR-0112 决策 5）；" +
      "边界：" +
      "对壳层/域目录零直接依赖，反向引用由生产依赖面编译期拒绝（dev-dependency 环只覆盖测试目标）；可被多端同步域依赖的独立编译单元",
  },
  {
    name: "ledger-budget",
    dir: "crates/budget",
    layer: CRATE_LAYER.DOMAIN,
    note:
      "预算域 crate（#1101，P3 叶子业务域 crate：预算 CRUD/软删除/当前周期进度，可被多端同步域依赖）；" +
      "依赖面：" +
      "只有基础设施、同步协议与核心交易域——进度 spent 口径消费 kind→度量矩阵（ExpenseNet）是域→域合法上层依赖（预算 → 核心交易单向），无接缝无注册点、壳层启动零接线；" +
      "边界：" +
      "对壳层与同步域零直接依赖，反向引用由生产依赖面编译期拒绝（dev-dependency 环只覆盖测试目标）",
  },
  {
    name: "ledger-categories",
    dir: "crates/categories",
    layer: CRATE_LAYER.DOMAIN,
    note:
      "分类域 crate（#1094，P3 叶子域，参考数据三域各自独立 crate 不合并：分类 CRUD/幂等创建/预算删除守卫/排序重排）；" +
      "依赖面：" +
      "只有基础设施与同步协议（允许集「基础设施、协议、核心交易域」的子集，对交易域亦零依赖），无接缝无注册点、壳层启动零接线；" +
      "边界：" +
      "反向引用由生产依赖面编译期拒绝（dev-dependency 环只覆盖测试目标）",
  },
  {
    name: "ledger-currencies",
    dir: "crates/currencies",
    layer: CRATE_LAYER.DOMAIN,
    note:
      "币种域 crate（#1095，P3 叶子域，参考数据三域之二：币种字典/汇率/本位币基准，spec 明文裁决三域各自独立 crate 不合并）；" +
      "依赖面：" +
      "只有基础设施、同步协议与核心交易域——本位币基准读取经注册点供给核心交易域接缝（下层提供实现、壳层启动接线，ADR-0112 决策 5）；" +
      "边界：" +
      "对壳层/同级业务域零直接依赖，反向引用由生产依赖面编译期拒绝（dev-dependency 环只覆盖测试目标）",
  },
  {
    name: "ledger-dashboard",
    dir: "crates/dashboard",
    layer: CRATE_LAYER.DOMAIN,
    note:
      "仪表盘域 crate（#1104，P3 叶子业务域 crate：首页净资产跨币种折算合计——真实财富视角三腿——与读探针缓存 ADR-0067）；" +
      "依赖面：" +
      "为修订后票面允许集的子集：基础设施（db/error）、账户域（余额口径与 AccountType）、核心交易域（折算与默认币种口径）、实物资产域（第三腿在持合计单一读口径，ADR-0064 决策 6；账户域与实物资产域两项系维护者修订票面 AC 后授权，dashboard → accounts/transaction/physical-asset 三向均为上层域对底层域合法单向依赖，ADR-0112 决策 2），纯读模型无同步命令，无接缝无注册点、壳层启动零接线；" +
      "边界：" +
      "反向引用由生产依赖面编译期拒绝",
  },
  {
    name: "ledger-infra",
    dir: "crates/infra",
    layer: CRATE_LAYER.INFRA,
    note:
      "基础设施 crate（#1088 全量归位：数据库/错误/设置/文件工具/事件/信号/闭集；#1108 壳层统一读写入口与载荷脱敏迁出至根包 src/shell_support——基础设施不再承载只被壳层消费的机制，再导出面同步清除、消费方以 crate 本名直呼）；" +
      "边界：" +
      "基础设施→域生产边为 0——提交点后置动作经注册点反转，接线在壳层启动",
  },
  {
    name: "ledger-investment",
    dir: "crates/investment",
    layer: CRATE_LAYER.DOMAIN,
    note:
      "投资域 crate（#1097，P3 业务域 crate：标的字典/市场数据/持仓与买卖协议/盈亏与走势，可被行情同步域与多端同步域依赖）；" +
      "依赖面：" +
      "只有基础设施、同步协议、核心交易域、账户域与币种域——交易域接缝实现注册侧（#1092 挂载点⑤反转后的合法方向）与两条域→域上层依赖（投资 → 账户：AccountType/余额口径，spec 明文；投资 → 币种：ExchangeRate/ExchangeRateInput 汇率实体消费方与录入入口，#418/ADR-0059「实体归属优先于消费方分布」，#1097 裁决显性化承认、非新增耦合）均为上层域消费下层域的合法直呼（ADR-0112 决策 2）；" +
      "边界：" +
      "对壳层与行情/多端同步域零直接依赖（行情查询半边经注入接缝倒挂），反向引用由生产依赖面编译期拒绝（dev-dependency 环只覆盖测试目标）",
  },
  {
    name: "ledger-item",
    dir: "crates/item",
    layer: CRATE_LAYER.DOMAIN,
    note:
      "物品域 crate（#1099，P3 叶子域：物品 CRUD/处置/每天使用成本聚合与溯源守卫）；" +
      "依赖面：" +
      "只有基础设施、同步协议与核心交易域——交易×物品来源列反查接缝（#1092）的实现注册侧是域→域合法上层依赖（物品 → 核心交易单向）；" +
      "边界：" +
      "对壳层与同步域零直接依赖，反向引用由生产依赖面编译期拒绝（dev-dependency 环只覆盖测试目标）",
  },
  {
    name: "ledger-market-sync",
    dir: "crates/market-sync",
    layer: CRATE_LAYER.DOMAIN,
    note:
      "行情同步域 crate（#1106，P4 首个业务域 crate：行情抓取——批量报价/单点行情/日 K/历史净值——与增量同步编排，成为可被多端同步域依赖的独立编译单元）；" +
      "依赖面：" +
      "为票面 AC 允许集全量：基础设施（db/error/events）、同步协议（op 落库行 device_id）、核心交易域（币种缺省推导 amount::default_currency_code）、投资域（价格写入单点 prices/名称随行刷新 crud/通道派生 channel/统一报价载荷 Quote，ADR-0103；#1543 起另消费汇率写入 crud::upsert_auto_exchange_rate，人工行保护）——四条域→域均为上层域消费下层域的合法直呼（ADR-0112 决策 2）；" +
      "边界：" +
      "对壳层与多端同步域零直接依赖（壳层同步命令经根包再导出面消费），反向引用由生产依赖面编译期拒绝（dev-dependency 环只覆盖测试目标）",
  },
  {
    name: "ledger-merchants",
    dir: "crates/merchants",
    layer: CRATE_LAYER.DOMAIN,
    note:
      "商户域 crate（#1096，参考数据三域各自独立 crate、不合并：商户字典 CRUD 与按名查找/即建）；" +
      "依赖面：" +
      "只有基础设施、同步协议与核心交易域——交易×商户接缝（#1092）的实现注册侧是域→域合法上层依赖（商户 → 核心交易单向）；" +
      "边界：" +
      "对壳层与同步域零直接依赖，反向引用由生产依赖面编译期拒绝（dev-dependency 环只覆盖测试目标）",
  },
  {
    name: "ledger-physical-asset",
    dir: "crates/physical-asset",
    layer: CRATE_LAYER.DOMAIN,
    note:
      "实物资产域 crate（#1102，P3 叶子业务域 crate：大件实物估值档案的建档/编辑/估值追加/处置/软删除，估值必填 = 首条估值历史行）；" +
      "依赖面：" +
      "只有基础设施、同步协议与核心交易域——当前估值折本位币消费交易域 Amount 口径（域间横向依赖，ADR-0056 决策 2 允许），无接缝无注册点、壳层启动零接线（失效信号 notify 回调注入）；" +
      "边界：" +
      "对壳层与同步域零直接依赖，反向引用由生产依赖面编译期拒绝（dev-dependency 环只覆盖测试目标）",
  },
  {
    name: "ledger-policy",
    dir: "crates/policy",
    layer: CRATE_LAYER.DOMAIN,
    note:
      "保单域 crate（#1100，P3 叶子业务域 crate：保单静态档案 CRUD/保司字典/保单视角统计，可被多端同步域依赖）；" +
      "依赖面：" +
      "只有基础设施、同步协议与核心交易域——统计读路径消费 kind→度量矩阵与折算口径，交易×保单接缝（#1092）的实现注册侧是域→域合法上层依赖（保单 → 核心交易单向）；" +
      "边界：" +
      "对壳层与同步域零直接依赖，反向引用由生产依赖面编译期拒绝（dev-dependency 环只覆盖测试目标）",
  },
  {
    name: "ledger-reports",
    dir: "crates/reports",
    layer: CRATE_LAYER.DOMAIN,
    note:
      "报表域 crate（#1103，P3 叶子业务域 crate：聚合分析读模型——月度汇总/分类聚合/商户消费排行/报表日期极值）；" +
      "依赖面：" +
      "只有基础设施与核心交易域（汇总口径消费 kind→度量矩阵，reports → transaction 单向；允许集「基础设施、协议、核心交易域」的子集，纯读模型无同步命令），无接缝无注册点、壳层启动零接线；" +
      "边界：" +
      "反向引用由生产依赖面编译期拒绝（dev-dependency 环只覆盖测试目标）",
  },
  {
    name: "ledger-scheduled",
    dir: "crates/scheduled",
    layer: CRATE_LAYER.DOMAIN,
    note:
      "定时计划域 crate（#1098，P3 业务域：定时交易计划/期次引擎/自动执行追补/订阅花费，可被多端同步域依赖的独立编译单元，spec #1086）；" +
      "依赖面：" +
      "只有基础设施、同步协议与核心交易域（期次落库/校验经 writer 接缝、花费合计经 amount 矩阵，#1092）——票面允许集内的备份域不声明：期次落账置脏与追补触发两条边已按注册点反转收敛（挂载点③/④，ADR-0112 决策 5，本域持注册点与实现侧、壳层启动对装）；" +
      "边界：" +
      "对壳层/同步域零直接依赖，反向引用由生产依赖面编译期拒绝（dev-dependency 环只覆盖测试目标）",
  },
  {
    name: "ledger-sync-engine",
    dir: "crates/sync-engine",
    layer: CRATE_LAYER.DOMAIN,
    note:
      "多端同步域 crate（#1107，P4 业务域 crate：OpLog 基座、双端合并语义、Checkpoint 与新端引导、通道/信封与同步触发编排）；" +
      "依赖面：" +
      "只有基础设施、同步协议、核心交易域与各业务域（重放注册表消费 14 个语义命令类型与各域公开重放入口，ADR-0101；对 ledger-backup 的调度锁复用为域→域合法上层依赖）；" +
      "边界：" +
      "对壳层零直接依赖，反向引用由 cargo 依赖图编译期拒绝（生产依赖面无根包，dev-dependency 环只覆盖测试目标）——全部域 crate 构成无环单向图，结构守门与 cargo 依赖图双证",
  },
  {
    name: "ledger-sync-protocol",
    dir: "crates/sync-protocol",
    layer: CRATE_LAYER.PROTOCOL,
    note:
      "同步协议 crate（#1089 下放：设备标识、领域命令契约 SyncCommand/ReplayEffect、op 本地记录与读取、流位点——业务域与 sync_engine 共同底座）；" +
      "边界：" +
      "对壳层与域目录零依赖，反向引用由 cargo 依赖图拒绝",
  },
  {
    name: "ledger-transaction",
    dir: "crates/transaction",
    layer: CRATE_LAYER.DOMAIN,
    note:
      "核心交易域 crate（#1092，P2 首个底层业务域 crate：交易写入协议/金额口径/读取与搜索，全部业务域可依赖的最底层域）；" +
      "依赖面：" +
      "只有基础设施与同步协议——对投资/商户/币种/物品/保单/账户六向的残留边经挂载点反转收敛（#1092 前置提交，ADR-0112 决策 5）；" +
      "边界：" +
      "反向引用由生产依赖面编译期拒绝（dev-dependency 环只覆盖测试目标）",
  },
  {
    name: "tauri-app",
    dir: ".",
    layer: CRATE_LAYER.SHELL,
    note:
      "tauri 应用包：命令注册扫描、IPC/HTTP 壳、shell_support 壳机制（#1108 自基础设施迁入正住址）与集成测试入口；" +
      "边界：" +
      "域业务语义已随 P1–P5 全量拆出（#1091–#1107），不再承载域目录",
  },
];

/** 成员目录约定（workspace glob）：新增 crate 只需落在此目录下即自动入 workspace。 */
const MEMBER_DIR_GLOB = "crates/*";

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

/** 已归位域目录名（自白名单派生，单一事实源）；按长度降序防前缀吞匹配 */
const DOMAIN_NAMES: string[] = WHITELIST.filter((w) => w.layer === LAYER.DOMAIN)
  .map((w) => w.path)
  .sort((a, b) => b.length - a.length);

/**
 * 基础设施→域依赖形态（ADR-0071 决策 6 / #538）：crate 根前缀 + 域目录名，
 * 再随 `::`（路径引用）、` as `（别名引入）或 `;`（模块自身导入）；
 * 捕获组 1 = 目标域名（认许边匹配用）。`\{?\s*` 容纳花括号列举首段
 * （use crate::{accounts::x, …}）。
 */
const INFRA_DOMAIN_DEP_PATTERN = new RegExp(
  `\\b(?:crate|tauri_app_lib)\\s*::\\s*\\{?\\s*(${DOMAIN_NAMES.join("|")})\\b(?:\\s*::|\\s+as\\b|\\s*;)`,
);

/** 认许边条目：基础设施文件相对路径 + 目标域目录名 + 成因留痕 */
interface InfraDomainEdge {
  file: string;
  domain: string;
  reason: string;
}

/**
 * 认许边（ADR-0071 决策 6 + §6 勘误注记 / #538）：基础设施→域的既有设计
 * 意图边，与白名单同属「已验证事实固化为规格」——逐条精确到白名单条目内
 * 文件相对路径（相对 `crates/infra/src`，自 #1088 归位起） + 目标域目录名，
 * 新增条目须附 ADR 指针与成因；清单之外的引用一律红。
 *
 * #1088 挂载点清点（「数量有记录、不新增」）：原首条 `db/mod.rs→backup`
 * （ADR-0032 连接层提交点置脏单点，#246）在生产代码里被注册点反转消除——
 * 基础设施只留调用时机、备份域提供实现、壳层启动接线，crate 依赖图不再有
 * 基础设施→业务域边；清单因此由 5 条降为 4 条。#1108 shell_support 迁出根包，
 * 其三条（logger/write_entry/read_entry→test_support）随迁退役，余下 1 条仍是
 * 内联 cfg(test) 经测试工厂建库的测试专用边（生产挂载点 0 条）。
 */
const INFRA_DOMAIN_ALLOWED_EDGES: readonly InfraDomainEdge[] = [
  {
    file: "settings.rs",
    domain: "test_support",
    reason:
      "ADR-0084 迁移状态段 + ADR-0071 决策 6：内联 cfg(test) 测试经测试工厂建库/取常量（#758 收口），测试专用边、非产品依赖",
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

/** 块间依赖形态：与 INFRA_DOMAIN_DEP_PATTERN 同款形态——crate 根前缀 + 目标块名，
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

/** 业务域→同步域引用锚点（掩码后匹配）——#1089 起零容忍：
 *  ①同步协议面下放前经根包再导出面的 `crate::sync_engine` / `tauri_app_lib::sync_engine`；
 *  ②#1107 同步域 crate 化后的 crate 名直引 `ledger_sync_engine::`。
 *  任何命中即红；Cargo.toml 生产依赖声明另由 crate 边界禁边核对拦下。 */
const SYNC_ENGINE_REF_PATTERN =
  /\b(?:crate|tauri_app_lib)\s*::\s*sync_engine\b|\bledger_sync_engine\b/;

/** 域间禁边规则条目：from 域目录内文件引用 to 域目录即红（认许边除外） */
interface DomainPairRule {
  from: string;
  to: string;
  /**
   * 附加文本形态（#1091 crate 拆分）：目标域拆为独立 crate 后的 crate 名直引
   * 前缀（`ledger_backup::`），与 domainPairDepPattern(to) 的 crate 根前缀形态
   * （`crate::backup` / `tauri_app_lib::backup` 再导出面）并扫——两形都红。
   * 经别名改名的间接引用文本不可达，靠评审兜底。
   */
  extraPattern?: RegExp;
  reason: string;
}

/**
 * 域间禁边（issue #1090 / spec #1086 形态推广）：写路径/读路径副作用已收口为
 * 「下层定义注册点、上层注册实现、壳层启动时接线」的接缝反转形态（与 #1088
 * 基础设施提交点后置动作同构），域间横向直接依赖随接缝消亡——残留引用（import、
 * 全限定调用、花括号列举首段）即红。作用域限业务域目录（认许边逐条留痕于
 * DOMAIN_PAIR_ALLOWED_EDGES），文本级扫描、掩码注释与字面量后匹配，别名改写
 * 不可达靠评审兑底。
 *
 * 起点域拆为独立 crate 后的禁边随 crate 化退役，文本清单不再辖、依赖方向改由
 * cargo 依赖图编译期拒绝（生产依赖面无根包与未声明域）——以 transaction 为
 * 起点的规则随 #1092（对业务域/壳层的引用由生产依赖面拒绝；crate 名直引形态
 * 亦不存在——域层对下层 crate 的合法引用走 ledger_transaction::，方向合法不属
 * 禁边）；scheduled_transactions→backup 一条随 #1098：定时计划域拆为
 * ledger-scheduled crate 后，置脏实现已住 ledger-backup、追补触发实现住本域，
 * 双向均经注册点接缝、壳层对装，本域生产依赖面不含 ledger-backup，构造
 * `ledger_backup::` / 再导出面引用即编译失败，文本扫描不再可及。
 */
export const DOMAIN_PAIR_FORBIDDEN: readonly DomainPairRule[] = [
  // #1098 后为空：最后一条（scheduled_transactions → backup，含 crate 名直引
  // extraPattern）已随定时计划域 crate 化退役，清单保留为空集留痕。
];

/** 域间禁边认许边条目：文件相对路径（相对根 src）+ from/to + 成因留痕 */
interface DomainPairAllowedEdge {
  file: string;
  from: string;
  to: string;
  reason: string;
}

/**
 * 域间禁边认许边（issue #1090）：既有设计意图边逐条留痕于本脚本，与
 * INFRA_DOMAIN_ALLOWED_EDGES 同款留痕纪律——精确到文件相对路径，附成因；
 * 清单之外的域间禁边引用一律红。
 */
export const DOMAIN_PAIR_ALLOWED_EDGES: readonly DomainPairAllowedEdge[] = [
  // #1092 后为空：唯一一条（transaction/funding.rs → accounts 的 AccountType
  // 类型只读边）已随出资账户视图接缝反转消亡，清单保留为空集留痕。
];

/** 域间禁边依赖形态：与 INFRA_DOMAIN_DEP_PATTERN 同款——crate 根前缀 + 目标域名。 */
function domainPairDepPattern(to: string): RegExp {
  return new RegExp(
    `\\b(?:crate|tauri_app_lib)\\s*::\\s*\\{?\\s*(${to})\\b(?:\\s*::|\\s+as\\b|\\s*;)`,
  );
}

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
 * 业务域→同步域零容忍扫描（ADR-0101 决策 4b / #1089 收紧）：业务域对同步域的
 * 任何代码引用（根引入、别名引入、`::` 子路径、花括号列举）一律命中——同步
 * 协议面已下放协议 crate，业务域只依赖 `ledger_sync_protocol`。文本级扫描
 *（掩码注释与字面量）。
 */
export function scanSyncEngineRefs(text: string): ScanHit[] {
  const masked = maskNonCode(text);
  const rawLines = text.split("\n");
  const hits: ScanHit[] = [];
  const lineOf = (index: number): number => (masked.slice(0, index).match(/\n/g)?.length ?? 0) + 1;
  const push = (index: number, match: string): void => {
    const line = lineOf(index);
    hits.push({ line, text: (rawLines[line - 1] ?? "").trim(), match, captured: undefined });
  };
  for (const m of masked.matchAll(new RegExp(SYNC_ENGINE_REF_PATTERN, "g"))) {
    const start = m.index ?? 0;
    const moduleName = m[0].startsWith("ledger_sync_engine") ? "ledger_sync_engine" : "sync_engine";
    const tail = masked.slice(start + m[0].length);
    const afterModule = /^\s*::\s*/.exec(tail);
    if (!afterModule) {
      // 无 `::` 子路径：根模块引入（`use crate::sync_engine;` / `as se;`）——
      // 零容忍下同样红（别名改写会让后续引用文本不可达，与既有壳层扫描的
      // `commands as` 同款堵漏）。
      push(start, /^\s+as\b/.test(tail) ? `${m[0]} as …` : m[0]);
      continue;
    }
    const cursor = start + m[0].length + afterModule[0].length;
    if (masked[cursor] === "{") {
      // 根花括号列举：跨行取匹配闭括号后逐条报违规（零容忍，全条目违规）。
      let depth = 0;
      let close = -1;
      for (let i = cursor; i < masked.length; i++) {
        if (masked[i] === "{") depth++;
        else if (masked[i] === "}") {
          depth--;
          if (depth === 0) {
            close = i;
            break;
          }
        }
      }
      const body = masked.slice(cursor + 1, close === -1 ? masked.length : close);
      for (const entry of disallowedBraceEntries(body)) {
        push(start, `${moduleName}::{…${entry}…}`);
      }
      continue;
    }
    const segment = /^([A-Za-z_][A-Za-z0-9_]*)/.exec(masked.slice(cursor));
    if (!segment) {
      // `sync_engine::*` 等非具名形态：一律红。
      push(start, masked.slice(start, cursor + 1));
      continue;
    }
    push(start, `${moduleName}::${segment[1]}`);
  }
  return hits;
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
 * 清单声明的**生产**依赖 crate 名（含 target 变体；`[dependencies.x]` 子表形态）。
 * 只取依赖表本身，段落其余内容不参与——供 crate 依赖方向核对使用。
 *
 * 刻意排除 `[dev-dependencies]`：spec #1086 明文裁决「数据库 ↔ 核心交易域的双向
 * 引用是测试专用边，用 dev-dependency 环解决（Cargo 允许）」，测试工厂与器具以
 * dev-dependency 形态供各域/基础设施复用（#1088 实测：环成立，测试目标与生产依赖
 * 图分离）；生产依赖方向仍按「壳 → 域 → 基础设施」单向核对，`crates/infra` 的
 * `[dev-dependencies] tauri-app` 是本票测试专用边的落点。
 */
function declaredProductionDependencyNames(manifest: string): string[] {
  const names = new Set<string>();
  const tableRe = /^(?:target\..+\.)?dependencies(?:\.([A-Za-z0-9_-]+))?$/;
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
          "    新增 crate 后须在 scripts/check-structure.ts 的 CRATES 追加一行（分层 + 注释），" +
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
    // 显式拒绝（文本扫描另对源码引用同向零容忍）。
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
 * 登记面全等（#1448）：CRATES 成员与 CRATE_MODULE_LISTS 双向全等——每个
 * workspace 成员 crate（根包除外，壳层模块面不整册登记）必有模块清单登记，
 * 登记面每条 srcRel 必可回指 CRATES 成员（srcRel = dir + '/src'）。缺口形态
 * 即本票之洞的上层复现：新 crate 入 CRATES（否则成员登记红）而漏入
 * CRATE_MODULE_LISTS，其模块面完全脱离模块级扫描与清单↔磁盘双向全等，
 * 新增生产模块静默漏过结构守门。
 */
function checkModuleListRegistry(): string[] {
  const problems: string[] = [];
  const registered = new Map(CRATE_MODULE_LISTS.map((spec) => [spec.srcRel, spec.label]));
  for (const crate of CRATES) {
    if (crate.dir === ".") continue; // 根包是壳层，模块面不整册登记（与 WHITELIST 同注）
    const srcRel = `${crate.dir}/src`;
    if (!registered.delete(srcRel)) {
      problems.push(
        `✗ ${crate.name} 未登记模块清单：CRATE_MODULE_LISTS 缺 srcRel ${srcRel}\n` +
          "    登记面全等（#1448）：每个成员 crate 须在 CRATE_MODULE_LISTS 追加一行 " +
          "（label / modules / srcRel / 报文字段），否则其模块面静默脱离模块级扫描与 " +
          "清单↔磁盘双向全等——新增生产模块漏过结构守门",
      );
    }
  }
  for (const [srcRel, label] of registered) {
    problems.push(
      `✗ ${label} 的 srcRel 无对应 CRATES 成员：${srcRel}\n` +
        "    登记面全等（#1448）：CRATE_MODULE_LISTS 条目须回指 CRATES 成员 " +
        "（srcRel = dir + '/src'），清单漂移 fail loud（ADR-0056 决策 4）",
    );
  }
  return problems;
}

/**
 * 模块清单与磁盘模块双向全等的通用核对（#1448 自 infra #1134 / transaction
 * #1181 / sync-engine #1107 三处专用实现合流，并经 CRATE_MODULE_LISTS 推广到
 * 全部 crate 模块清单）：磁盘侧枚举 crate src 顶层的实际模块——非测试豁免形态
 * 的 .rs 文件，与扫得到非测试 .rs 文件的目录（目录型条目覆盖其全部子目录，子
 * 文件不再逐行登记）——磁盘上存在而清单未登记即红（清单漂移不再只单向 fail
 * loud）。crate 根 lib.rs 是声明与再导出面，免登清单（excludeCrateRoot）的
 * 磁盘枚举恒排除：crate 根文件名恒定，新增模块经 lib.rs 声明后落磁盘即被本
 * 核对捕获，不因 lib.rs 免登产生漏检；infra 例外——lib.rs 已入清单，一并参与
 * 枚举。反方向（登记路径消失 / 条目扫不到非测试文件）由 scanModuleEntries 的
 * 既有核对承担，本核对不重复报文。
 */
function checkModuleListEquality(spec: CrateModuleListSpec, srcTauriDir: string): string[] {
  const problems: string[] = [];
  const srcDir = join(srcTauriDir, spec.srcRel);
  if (!existsSync(srcDir)) return problems; // 清单循环逐条报「路径不存在」
  const onDisk: string[] = [];
  for (const entry of readdirSync(srcDir, { withFileTypes: true }).sort((a, b) =>
    a.name.localeCompare(b.name),
  )) {
    const isRustModuleFile =
      entry.isFile() && entry.name.endsWith(".rs") && !isTestFile(entry.name);
    const rootDeclarationExcluded = spec.excludeCrateRoot && entry.name === "lib.rs";
    if (isRustModuleFile && !rootDeclarationExcluded) {
      onDisk.push(entry.name);
    } else if (
      entry.isDirectory() &&
      collectRustFiles(join(srcDir, entry.name), entry.name).length > 0
    ) {
      onDisk.push(entry.name);
    }
  }
  for (const mod of onDisk) {
    if (!spec.modules.some((m) => m.path === mod)) {
      problems.push(
        `✗ ${spec.label} 未登记模块：${mod}（${spec.srcRel}）\n` +
          `    清单与实际模块双向全等（${spec.provenance}）：新增模块后须在 ` +
          `scripts/check-structure.ts 的 ${spec.label} 追加一行${spec.registerNote}，` +
          `否则清单漏登记、${spec.why}`,
      );
    }
  }
  return problems;
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

/** 清单条目路径 → 模块键（去 .rs 后缀：flat 文件条目 amount.rs 与目录条目 amount 同键）。 */
function moduleKey(path: string): string {
  return path.replace(/\.rs$/, "");
}

/**
 * 模块清单投影核对（#1593，expand 半）：expected 由 crate 根 lib.rs 的 `mod`
 * 声明派生，与手写清单集合等价断言——lib.rs 已声明但清单未登记、或清单登记但
 * lib.rs 未声明，任一方向即红。crate 根 lib.rs 自身不是 mod 声明（infra 的
 * `lib.rs` 清单条目由磁盘枚举面核对），故手写侧比较面去掉 `lib` 键。
 */
function checkModuleListProjection(spec: CrateModuleListSpec, srcTauriDir: string): string[] {
  const problems: string[] = [];
  const srcDir = join(srcTauriDir, spec.srcRel);
  if (!existsSync(srcDir)) return problems; // 清单循环逐条报「路径不存在」
  const libRsRel = `${spec.srcRel}/lib.rs`;
  const libRs = join(srcTauriDir, libRsRel);
  if (!existsSync(libRs)) {
    problems.push(
      `✗ 找不到 crate 根声明文件：${libRsRel}\n` +
        "    模块清单 expected 由 crate 根 lib.rs 的 mod 声明投影（ADR-0056 决策 4 / #1593）：" +
        "声明文件缺失即无法派生 expected，fail loud",
    );
    return problems;
  }
  const scan = scanModDeclarations(readFileSync(libRs, "utf8"));
  for (const v of scan.violations) {
    problems.push(
      `✗ 认不出的 lib.rs 声明形状：${libRsRel}:${v.line}（${v.shape}）\n` +
        "    mod 扫描形状判定表（ADR-0056 决策 4 / ADR-0113 决策 7 / ADR-0111 决策 5 修订注记 / #1593）：" +
        "lint 属性（allow/warn/deny/forbid/expect）、doc 属性与 #[cfg] / #[cfg_attr] 门控 mod 计入 expected；" +
        "#[cfg(test)] mod tests; 豁免；#[path]（含 cfg_attr 夹带 path）/ include! / 内联模块块 / 上述之外认不出的属性链 fail loud——" +
        "按上表改写声明，或把新形态登记进形状判定表",
    );
  }
  const derived = new Set(scan.modules);
  const hand = new Set(spec.modules.map((m) => moduleKey(m.path)).filter((k) => k !== "lib"));
  const declaredOnly = [...derived].filter((k) => !hand.has(k)).sort();
  const registeredOnly = [...hand].filter((k) => !derived.has(k)).sort();
  if (declaredOnly.length > 0) {
    problems.push(
      `✗ 模块清单漂移（投影核对）：lib.rs 已声明但手写清单未登记 ${declaredOnly.join(" / ")}（${libRsRel}）\n` +
        "    事实有权威源就投影（ADR-0056 决策 4 / #1593）：expected 由 crate 根 lib.rs 的 mod 声明派生，" +
        `手写清单须与之等价——新增模块须同时落 lib.rs 声明与 ${spec.label} 条目（#1595 后清单整体退役）`,
    );
  }
  if (registeredOnly.length > 0) {
    problems.push(
      `✗ 模块清单漂移（投影核对）：手写清单登记但 lib.rs 未声明 ${registeredOnly.join(" / ")}（${libRsRel}）\n` +
        "    事实有权威源就投影（ADR-0056 决策 4 / #1593）：模块删除或改名后 lib.rs 声明与清单须同步——" +
        `${spec.label} 条目不得先于声明存在，漏删即与声明面分裂`,
    );
  }
  return problems;
}

/** 文件所属模块条目：精确匹配优先，其次目录前缀（目录型条目覆盖其全部子目录）。 */
function transactionOwningEntry(rel: string): TransactionModuleEntry | undefined {
  return [...TRANSACTION_MODULES]
    .sort((a, b) => b.path.length - a.path.length)
    .find((e) => rel === e.path || rel.startsWith(`${e.path}/`));
}

/**
 * 交易域 crate 内区级依赖引用扫描（ADR-0113 决策 7 / #1181）：掩码注释与字面量
 * 后匹配 `super::` / `crate::` 前缀 + 目标模块名（flat 布局的模块名与重排后的
 * 区目录名同形，两种形状同扫，判向不变）；`::{…}` 花括号列举跨行取匹配闭括号
 * 后逐条目切分取首段标识符。捕获 = 目标模块名（区归属查表用）。表达式位裸路径
 * （`writer::x` 无前缀形态，依赖边已由 use 语句承载）与别名改写文本不可达，
 * 靠评审兜底（与壳层/基础设施扫描同款边界）。
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
 * 交易域 crate 内区级层序核对（ADR-0113 决策 3/7 / #1181）：对 crate 内全部非
 * 测试 Rust 文件，按其所属模块条目的 zone 判向——同区互依合法；跨区时秩大者
 * 方可依赖秩小者（写读 → 接缝 → 共享语义，写读两径互不依赖）；认许边之外即红。
 * crate 根 lib.rs（声明与再导出面，无所属条目）与未登记孤儿文件（双向全等已
 * 红）不参与判向。测试豁免形态由 collectRustFiles 过滤（ADR-0056 决策 5）。
 */
function checkTransactionZoneDirection(srcTauriDir: string): string[] {
  const problems: string[] = [];
  const srcDir = join(srcTauriDir, TRANSACTION_SRC_REL);
  if (!existsSync(srcDir)) return problems; // 清单循环逐条报「路径不存在」
  for (const f of collectRustFiles(srcDir, TRANSACTION_SRC_REL)) {
    const rel = f.rel.slice(TRANSACTION_SRC_REL.length + 1);
    const owner = transactionOwningEntry(rel);
    if (!owner) continue;
    const source = readFileSync(f.abs, "utf8");
    for (const hit of scanTransactionZoneRefs(source)) {
      const target = TRANSACTION_MODULES.find((m) => moduleKey(m.path) === hit.captured);
      if (!target || target.zone === owner.zone) continue;
      if (TRANSACTION_ZONE_RANK[owner.zone] > TRANSACTION_ZONE_RANK[target.zone]) continue;
      const allowed = TRANSACTION_ZONE_ALLOWED_EDGES.some(
        (e) => e.file === owner.path && e.target === moduleKey(target.path),
      );
      if (allowed) continue;
      problems.push(
        `✗ 区级反向依赖：${owner.zone}「${owner.path}」引用 ${target.zone}「${target.path}」 → ` +
          `${f.rel}:${hit.line}（${hit.match}）\n` +
          `    ${hit.text}\n` +
          `    区级层序唯一：写路径/读路径 → 跨域接缝 → 共享语义（ADR-0113 决策 3）——` +
          `共享语义不得依赖接缝与路径区，接缝不得依赖路径区，写读两径互不依赖；` +
          `区归属与清单同址单点声明（TRANSACTION_MODULES 条目的 zone 字段）；` +
          `设计意图边须逐条留痕于本脚本 TRANSACTION_ZONE_ALLOWED_EDGES（附 ADR 指针），` +
          `或把依赖下沉到层序更低的区`,
      );
    }
  }
  return problems;
}

/**
 * 模块清单核对（ADR-0056 决策 4）：清单条目必须存在且扫得到非测试 Rust 文件
 * （清单漂移 fail loud），条目内对壳层零依赖；基础设施条目另核
 * 基础设施→域认许边（ADR-0071 决策 6 / #538）与 crate 内块间反向依赖
 * （ADR-0111 决策 4 / #1134），业务域条目另核业务域→同步域
 * 严形态（ADR-0101 决策 4b）。返回扫到的非测试文件数；返回 0 由调用方统一拒绝
 * （拒绝以空集假绿通过）。清单与路径基准分离，使域目录（根 src）与基础设施
 * crate（`crates/infra/src`）共用同一份核对逻辑与同一份认许边清单。
 */
function scanModuleEntries(
  entries: readonly WhitelistEntry[],
  baseDir: string,
  problems: string[],
  options: { scanBusinessSyncRefs?: boolean } = {},
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
        `✗ 白名单路径不存在：${w.path}（${w.layer}：${w.note}）——目录改名/迁移后未同步守门清单`,
      );
      continue;
    }
    const files: RustFileRef[] = stat.isDirectory()
      ? collectRustFiles(abs, w.path)
      : [{ abs, rel: w.path }];
    if (files.length === 0) {
      problems.push(
        `✗ 白名单条目扫不到非测试 Rust 文件：${w.path}（${w.layer}：${w.note}）——` +
          `全部是测试豁免形态或已空，清单与目录形状漂移`,
      );
      continue;
    }
    scanned += files.length;
    const isInfra = w.layer === LAYER.INFRA;
    const isProtocol = w.layer === LAYER.PROTOCOL;
    // 业务域→同步域零容忍作用域：域目录，除同步域自身（SYNC_ENGINE_MODULES
    // 显式关扫，#1107 crate 化后其源码住 workspace 成员）与测试支持域
    //（test_support→sync_engine 为测试专用边，登记处 ADR-0084 迁移状态段）。
    const isBusinessDomain =
      (options.scanBusinessSyncRefs ?? true) &&
      w.layer === LAYER.DOMAIN &&
      w.path !== "test_support";
    for (const f of files) {
      const source = readFileSync(f.abs, "utf8");
      for (const hit of scanRustSource(source)) {
        problems.push(
          `✗ 反向依赖：${w.path} 层（${w.note}）引用壳层 → ${f.rel}:${hit.line}（${hit.match}）\n` +
            `    ${hit.text}\n` +
            `    分层规则：壳 → 域 → 基础设施，域永不依赖壳（ADR-0056）；` +
            `被依赖逻辑应下沉到域目录或基础设施，或本次迁移应把该文件一并归位`,
        );
      }
      if (isInfra) {
        // crate 内块间反向依赖（ADR-0111 决策 4 / #1134）：db 不得引用
        // boot / signals（shell_support 已随 #1108 迁出根包）；
        // 认许边逐条留痕（INFRA_BLOCK_ALLOWED_EDGES），清单之外即红。
        const block = f.rel.split("/")[0];
        const forbiddenTargets = INFRA_BLOCK_FORBIDDEN[block];
        if (forbiddenTargets) {
          for (const hit of scanRustSource(source, infraBlockDepPattern(forbiddenTargets))) {
            const allowed = INFRA_BLOCK_ALLOWED_EDGES.some(
              (e) => e.file === f.rel && e.target === hit.captured,
            );
            if (allowed) continue;
            problems.push(
              `✗ crate 内反向依赖：${block} 引用 ${hit.captured} → ${f.rel}:${hit.line}（${hit.match}）\n` +
                `    ${hit.text}\n` +
                `    crate 内分层（ADR-0111 决策 4；#1108 shell_support 迁出根包）：` +
                `原语 ← db ← boot 单向，db 不得引用 boot/signals；` +
                `设计意图边须逐条留痕于本脚本 INFRA_BLOCK_ALLOWED_EDGES（附 ADR 指针），` +
                `或把逻辑下沉到更低的块`,
            );
          }
        }
        for (const hit of scanRustSource(source, INFRA_DOMAIN_DEP_PATTERN)) {
          const allowed = INFRA_DOMAIN_ALLOWED_EDGES.some(
            (e) => e.file === f.rel && e.domain === hit.captured,
          );
          if (allowed) continue;
          problems.push(
            `✗ 反向依赖：${w.path}（${w.layer}：${w.note}）引用域目录 ${hit.captured} → ` +
              `${f.rel}:${hit.line}（${hit.match}）\n` +
              `    ${hit.text}\n` +
              `    分层规则：壳 → 域 → 基础设施；基础设施→域业务边归零且可机械守门` +
              `（ADR-0071 决策 6，#538）；设计意图边须逐条留痕于本脚本 ` +
              `INFRA_DOMAIN_ALLOWED_EDGES（附 ADR 指针），或把逻辑下沉域目录`,
          );
        }
      }
      if (isProtocol) {
        // 协议 crate（共享底座）对壳层与域目录零依赖（#1089）：无认许边——反向
        // 引用即环，与 cargo 依赖图（协议 crate 根文档负向用例）双保险。
        for (const hit of scanRustSource(source, INFRA_DOMAIN_DEP_PATTERN)) {
          problems.push(
            `✗ 反向依赖：协议 crate（${w.note}）引用域目录 ${hit.captured} → ` +
              `${f.rel}:${hit.line}（${hit.match}）\n` +
              `    ${hit.text}\n` +
              `    分层规则：壳 → 域 → 基础设施 → 协议（#1089 共享底座）；协议 crate ` +
              `对壳层与域目录零依赖，反向引用即环——依赖面只有基础设施与数据面惯用库`,
          );
        }
      }
      if (isBusinessDomain) {
        for (const hit of scanSyncEngineRefs(source)) {
          problems.push(
            `✗ 业务域引用同步域：${f.rel}:${hit.line}（${hit.match}）\n` +
              `    ${hit.text}\n` +
              `    零容忍（ADR-0101 决策 4b / #1089 收紧）：同步协议面（命令契约 / ` +
              `op 产出 / 设备标识）已下放协议 crate，业务域只依赖 ` +
              `ledger_sync_protocol；「重放不产本地 op」从结构巧合升为规格`,
          );
        }
        // 域间禁边（issue #1090）：残留的域间直接引用即红（认许边逐条留痕）。
        // 目标域拆为独立 crate 后（#1091）追加 crate 名直引前缀并扫（extraPattern）。
        for (const rule of DOMAIN_PAIR_FORBIDDEN.filter((r) => r.from === w.path)) {
          const patterns = rule.extraPattern
            ? [domainPairDepPattern(rule.to), rule.extraPattern]
            : [domainPairDepPattern(rule.to)];
          for (const pattern of patterns) {
            for (const hit of scanRustSource(source, pattern)) {
              const allowed = DOMAIN_PAIR_ALLOWED_EDGES.some(
                (e) => e.file === f.rel && e.from === rule.from && e.to === rule.to,
              );
              if (allowed) continue;
              problems.push(
                `✗ 域间禁边：${f.rel} 引用 ${rule.to} → ${f.rel}:${hit.line}（${hit.match}）\n` +
                  `    ${hit.text}\n` +
                  `    ${rule.reason}\n` +
                  `    写路径副作用一律经注册点反转形态（下层定义注册点、上层注册实现、` +
                  `壳层启动接线，spec #1086 / #1090）；设计意图边须逐条留痕于本脚本 ` +
                  `DOMAIN_PAIR_ALLOWED_EDGES（附成因）`,
              );
            }
          }
        }
      }
    }
  }
  return scanned;
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
      ...CRATE_MODULE_LISTS.flatMap((spec) =>
        collectRustFiles(join(srcTauriDir, spec.srcRel), spec.srcRel),
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

  // 模块清单核对（登记面 CRATE_MODULE_LISTS，#1448）：WHITELIST 域目录相对根
  // src 扫描（壳层反向依赖）；crate 清单相对各自模块根扫描（壳层反向依赖 +
  // 业务域→同步域严形态，同步域自身关扫 #1107）——基础设施模块自 #1088 全量
  // 归位起不再住根 src，路径基准随归位改一次、事实源仍只有本脚本一份（ADR-0056
  // 「白名单即规格」不变）。
  scannedFiles += scanModuleEntries(WHITELIST, srcDir, problems);
  for (const spec of CRATE_MODULE_LISTS) {
    scannedFiles += scanModuleEntries(spec.modules, join(srcTauriDir, spec.srcRel), problems, {
      scanBusinessSyncRefs: spec.scanBusinessSyncRefs ?? true,
    });
  }

  if (scannedFiles === 0) {
    problems.push(
      "✗ 全部白名单条目扫不到任何非测试 Rust 文件——src 目录指错或白名单整体漂移，拒绝以空集假绿通过",
    );
  }

  // crate 边界核对（spec #1086 / issue #1087）：成员登记、门禁继承、依赖方向、
  // 静态检查/测试命令的 workspace 覆盖——与模块路径白名单并列，同为删除即变红。
  problems.push(...checkCrateBoundaries(srcTauriDir));
  problems.push(...checkModuleListRegistry());

  // 模块清单↔磁盘双向全等（#1448 自 #1134/#1181/#1107 三处专用接线合流并推广
  // 到全部 crate 清单）：磁盘侧反向核对（磁盘模块未登记即红）；登记路径消失 /
  // 扫不到非测试文件已由清单循环红。交易域区级层序（ADR-0113 决策 7 / #1181）
  // 另行核对：区归属据清单条目 zone 字段判向，认许边（ADR-0113 决策 3 原形状
  // 反边）之外即红。
  for (const spec of CRATE_MODULE_LISTS) {
    problems.push(...checkModuleListEquality(spec, srcTauriDir));
    problems.push(...checkModuleListProjection(spec, srcTauriDir));
  }
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
      CRATE_MODULE_LISTS.map(
        (spec) =>
          `+ ${spec.summaryName}模块 ${spec.modules.length} 项（crate ${spec.srcRel}` +
          `${spec.summaryIssue ? `，${spec.summaryIssue}` : ""}）`,
      ).join("") +
      `· 白名单面非测试文件 ${scannedFiles} 个 · 对壳层零依赖` +
      `· 基础设施→域零未认许引用（认许边 ${INFRA_DOMAIN_ALLOWED_EDGES.length} 条，ADR-0071）` +
      `· 协议 crate→壳层/域目录零引用（共享底座，#1089）` +
      `· 业务域→同步域零容忍零违规（ADR-0101 / #1089 收紧）` +
      `· 域间禁边 ${DOMAIN_PAIR_FORBIDDEN.length} 对零未认许引用（认许边 ${DOMAIN_PAIR_ALLOWED_EDGES.length} 条，#1090 接缝反转）` +
      `· 模型域化禁令全树扫描 ${allFiles.length} 个文件零残留（ADR-0059）` +
      `· 原生事务语句全树扫描 ${allFiles.length} 个文件仅 ${NATIVE_TX_STMT_ALLOWED} 一处（#1014）` +
      `· crate 边界 ${CRATES.length} 个（成员登记 / 门禁继承 / 依赖方向 / workspace 命令覆盖，#1087）` +
      `· 登记面全等：CRATES 成员 ↔ CRATE_MODULE_LISTS 双向全等（#1448）` +
      `· crate 内块间反向依赖零未认许引用（认许边 ${INFRA_BLOCK_ALLOWED_EDGES.length} 条，ADR-0111 决策 4 / #1134）` +
      `· 模块清单双向全等推广至全部 ${CRATE_MODULE_LISTS.length} 份 crate 清单（磁盘模块全部登记，#1134/#1181/#1107 起三面、#1448 推广）` +
      `· 模块清单投影核对：expected 由 crate 根 lib.rs 的 mod 声明派生，手写清单与派生集合等价（${CRATE_MODULE_LISTS.length} 份，#1593 expand）` +
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
