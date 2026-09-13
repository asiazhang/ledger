#!/usr/bin/env bun
// 结构守门（issue #396 / ADR-0056；模型域化禁令 issue #424 / ADR-0059 决策 6）：
// 白名单式分层依赖检查。
// 分层规则：壳 → 域 → 基础设施，域永不依赖壳。白名单 = 已归位域目录 + 全部
// 基础设施（「已验证对壳层零依赖」固化为规格）；白名单内出现对壳层
// （src-tauri/src/commands/）的模块路径依赖即红——每归位一域追加一行白名单。
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
// 计划来源反查 / 期次落账置脏）的域间直接依赖随接缝反转消亡——
// transaction→accounts、transaction→scheduled_transactions、
// scheduled_transactions→backup 残留引用即红（认许边逐条留痕于
// DOMAIN_PAIR_ALLOWED_EDGES，掩码后匹配，外挂测试豁免不变）。
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
// crate 边界核对（spec #1086 / issue #1087 门禁前置）：模块路径白名单之上再加
// crate 级核对——CRATES 是 workspace 成员、分层与允许依赖方向的唯一事实源；
// 成员目录（crates/*）与 CRATES 双向全等（新 crate 未登记即红）；每个成员须写
// `[lints] workspace = true` 继承六件套门禁（漏写即红——clippy 本身不会报）；
// 依赖方向按 壳 → 域 → 基础设施 单向核对；scripts/check.sh、scripts/test.sh 与
// scripts/lint-fix.sh、CI workflow 的 cargo clippy/test/fmt 命令须显式 `--workspace`
// 或词尾 `--all`（非虚拟 workspace 下默认只作用于根包，缺范围参数会静默漏检成员；
// `--all-targets` 等 `--all*` 旗标不算范围——\b 匹配会在这里假绿，故按词尾判定）。
// TypeScript 化 + Bun 运行时（issue #734 / ADR-0083）：类型经 tsconfig.scripts.json
// 门槛检查；调用方式 `bun scripts/check-structure.ts`。
// 默认校验本仓库；测试可传位置参数指向夹具：
// bun scripts/check-structure.ts [src-dir] [src-tauri-dir]
// 挂载于 scripts/check.sh 质量门槛序列与 CI（build.yml frontend job），
// 与命令注册一致性检查并列。

import { existsSync, readdirSync, readFileSync, statSync, type Stats } from 'node:fs'
import { dirname, join } from 'node:path'
import { pathToFileURL, fileURLToPath } from 'node:url'

/** 白名单条目（ADR-0056 决策 4） */
export interface WhitelistEntry {
  path: string
  layer: Layer
  note: string
}

/** 分层词汇（白名单 layer 字段取值）：比较侧单一来源，防字面量漂移 */
export const LAYER = {
  DOMAIN: '域目录',
  INFRA: '基础设施',
  PROTOCOL: '协议',
} as const

export type Layer = (typeof LAYER)[keyof typeof LAYER]

/**
 * 守门白名单（ADR-0056 决策 4）：路径相对 src-tauri/src。
 * 首批 = 已归位域目录；每迁一域在此追加一行。基础设施自 #1088 起整体住
 * `crates/infra/src`（不再有根 src 路径），改由下面的 INFRA_MODULES 清单核对。
 * #1091 起域目录开始拆独立 crate（backup 首个），拆出即从本清单移除、改登记
 * CRATES（BACKUP_MODULES 承接模块级扫描）。
 */
export const WHITELIST: readonly WhitelistEntry[] = [
  { path: 'scheduled_transactions', layer: '域目录', note: '定时计划域' },
  { path: 'item', layer: '域目录', note: '物品域（#397 阶段 1 归位，主体自 commands/item 随迁）' },
  { path: 'budget', layer: '域目录', note: '预算域（#399 阶段 3 归位）' },
  { path: 'physical_asset', layer: '域目录', note: '实物资产域（issue #466 新建即归位，ADR-0064）' },
  { path: 'investment', layer: '域目录', note: '投资域（#401 阶段 5 归位，主体自 commands/investment 随迁；价格写入单点自 sync/persist 迁入）' },
  { path: 'reports', layer: '域目录', note: '报表域（#405 归位，月度汇总/分类/商户/日期极值聚合读模型，消费 transaction::amount 矩阵）' },
  { path: 'dashboard', layer: '域目录', note: '仪表盘域（#405 归位，全仓净资产跨币种折算聚合）' },
  { path: 'sync', layer: '域目录', note: '行情同步域（#407 归位，HTTP 爬取/东财基金净值/同步编排自 commands/sync 随迁；全量修字典翼已退役，issue #698）' },
  { path: 'sync_engine', layer: '域目录', note: '多端同步域（issue #855 新建即归位，ADR-0091 OpLog 基座；与行情同步域 sync 相邻不同域）' },
  { path: 'test_support', layer: '域目录', note: '测试支持域（统一测试数据库工厂与共享断言库，ADR-0084 / #751；依赖域与基础设施合法，对壳层零依赖）' },
]

/**
 * 基础设施 crate 的模块清单（spec #1086 / issue #1088）：路径相对
 * `src-tauri/crates/infra/src`。数据库、错误、设置、文件工具、日志、事件、
 * 信号、闭集与壳层统一读写入口全量归位于此——它们对根包只以再导出面存在，
 * 故模块级守门（对壳层零依赖、基础设施→域认许边）随之落到 crate 根下扫描。
 */
export const INFRA_MODULES: readonly WhitelistEntry[] = [
  { path: 'lib.rs', layer: '基础设施', note: 'crate 根声明文件（#1134 双向全等起入清单）：pub mod 声明与再导出面（含 test_utils cfg 门，ADR-0111 决策 5 / #1132）——模块清单与 crate 实形的双向全等含根声明文件'},
  { path: 'boot', layer: '基础设施', note: '引导层（#1131 自 db 升顶层目录：disposition 启动处置判定与失败门 / data_location 引导 / book_registry 账本注册表 / encryption 加密基座 / passphrase_cache 口令缓存；依赖方向 boot → db 单向，原 db 路径经再导出保持）'},
  { path: 'db', layer: '基础设施', note: '数据库连接与 schema 守卫（#1127 起 mod.rs 只留声明与再导出，按职责分 migrate / connection / runtime 三文件；时间与身份工厂自 #1128 升顶层 ids、引导层五模块自 #1131 升顶层 boot，原 db 路径经再导出保持）' },
  { path: 'ids.rs', layer: '基础设施', note: '时间与身份工厂（当前时刻 / ISO 格式 / UUID v7 与 v5 确定性派生，#1128 自 db 升入——非数据库关切，文件工具等原语引用不穿透 db）' },
  { path: 'signals', layer: '基础设施', note: '信号映射（ADR-0044；#1129 起为目录模块，mod.rs 只做声明与再导出，测试外挂 tests/ 与 db/ 同形）' },
  { path: 'error.rs', layer: '基础设施', note: '错误' },
  { path: 'settings.rs', layer: '基础设施', note: '设置' },
  { path: 'fs_util.rs', layer: '基础设施', note: '文件级原子操作工具（备份与 DataLocation 搬迁共用，#408 纳入守门）' },
  { path: 'events.rs', layer: '基础设施', note: '事件发射机制（ADR-0054，#408 纳入守门；消费方跨出壳层——备份域、同步域，不随壳机制分组，ADR-0111 决策 2）' },
  { path: 'closed_set.rs', layer: '基础设施', note: '闭集字符串枚举宏（ADR-0108；模式先例 signals/write_op.rs write_op_set!，ADR-0102）' },
  { path: 'shell_support', layer: '基础设施', note: '壳机制暂住分组（ADR-0111 决策 2 / #1130）：壳层统一写入口 write_entry（ADR-0073）、读入口 read_entry（ADR-0104）、IPC 载荷脱敏 redact、日志初始化 logger——只被壳层消费，正住址是壳层（#1086 P5 迁出）；crate 根再导出保持原调用点路径' },
  { path: 'test_utils.rs', layer: '基础设施', note: '测试器具（捕获 tracing 事件的 Layer / 闸门式假发射器，#1088 随类型身份约束归位；`#[cfg(any(test, feature = "test-utils"))]` + `#[doc(hidden)]`，默认不进生产编译，#1132）' },
]

/** 基础设施 crate 的模块根（相对 src-tauri），与 CRATES 的 ledger-infra.dir 同源。 */
export const INFRA_SRC_REL = 'crates/infra/src'

/**
 * 同步协议 crate 的模块清单（spec #1086 / issue #1089）：路径相对
 * `src-tauri/crates/sync-protocol/src`。多端同步的最底层共享协议——设备标识、
 * 领域命令契约、op 本地记录与读取、流位点；业务域与 sync_engine 的共同底座，
 * 对壳层与全部域目录零依赖（反向引用由 cargo 依赖图拒绝 + 本扫描固化）。
 */
export const PROTOCOL_MODULES: readonly WhitelistEntry[] = [
  { path: 'command.rs', layer: '协议', note: '同步命令契约（SyncCommand 实体标签/实体键）与重放效果（ReplayEffect，ADR-0101）' },
  { path: 'device.rs', layer: '协议', note: '设备标识与逻辑时钟（`sync_device` 单行唯一 SQL 收口，ADR-0091）' },
  { path: 'op.rs', layer: '协议', note: 'op 行落库与读取（`sync_ops` 唯一 SQL 收口）+ 写后钩子登记点（ADR-0091 决策 9）' },
  { path: 'position.rs', layer: '协议', note: '流位点（`sync_stream_positions` 唯一 SQL 收口，issue #857）' },
]

/** 同步协议 crate 的模块根（相对 src-tauri），与 CRATES 的 ledger-sync-protocol.dir 同源。 */
export const PROTOCOL_SRC_REL = 'crates/sync-protocol/src'

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
  { path: 'auto.rs', layer: '域目录', note: '自动备份调度（状态 / 到期判定纯函数 / 三触发入口 / 轮询线程 / 追补触发注册点，挂载点④，issue #1091）' },
  { path: 'engine.rs', layer: '域目录', note: '备份引擎（zip 打包 / 恢复 / 受管列表与滚动清理，ADR-0007 / ADR-0016）' },
]

/** 备份域 crate 的模块根（相对 src-tauri），与 CRATES 的 ledger-backup.dir 同源。 */
export const BACKUP_SRC_REL = 'crates/backup/src'

/** 交易域 crate 四区词汇（ADR-0113 决策 2）：区归属登记与层序判向共用，字面量单一来源。 */
export const TRANSACTION_ZONE = {
  SHARED: '共享语义',
  SEAM: '跨域接缝',
  WRITE: '写路径',
  READ: '读路径',
} as const

export type TransactionZone = (typeof TRANSACTION_ZONE)[keyof typeof TRANSACTION_ZONE]

/** 交易域模块清单条目：白名单条目 + 区归属（ADR-0113 决策 2——区归属与清单同址单点声明，据此判向）。 */
export interface TransactionModuleEntry extends WhitelistEntry {
  zone: TransactionZone
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
  { path: 'amount', zone: TRANSACTION_ZONE.SHARED, layer: '域目录', note: '共享语义：金额口径权威（kind 枚举真源 + kind→度量矩阵 + 本位币折算）；子模块 base_currency 承载本位币基准读取接缝契约（ADR-0113 决策 3.1）' },
  { path: 'command', zone: TRANSACTION_ZONE.SHARED, layer: '域目录', note: '共享语义：同步命令载荷契约（payload 载荷 + fields 语义字段，issue #855）；被接缝与写路径共同消费，op 产出点归写路径 write/op.rs（ADR-0113 决策 3.3）' },
  { path: 'model', zone: TRANSACTION_ZONE.SHARED, layer: '域目录', note: '共享语义：域集中模型（transaction / input / normalized / filter / repair，#423 随域归位）；到 writer::NormalizedRow 的转换 impl 归写路径（ADR-0113 决策 3.2）' },
  { path: 'seams', zone: TRANSACTION_ZONE.SEAM, layer: '域目录', note: '跨域接缝：商户 / 投资 / 余额刷新 / 出资账户视图 / 来源列反查（计划/保单/物品）；只持契约与注册点' },
  { path: 'search_text.rs', zone: TRANSACTION_ZONE.SHARED, layer: '域目录', note: '共享语义：统一模糊搜索语义纯函数（拼音首字母/子序列/词条匹配，ADR-0027）；被投资域下拉共同消费（ADR-0113 决策 2 先例）' },
  { path: 'write', zone: TRANSACTION_ZONE.WRITE, layer: '域目录', note: '写路径：写入协议（protocol，Local/Replay 同址 ADR-0105）/ 行写入（writer）/ 批量（batch）/ 出资准入（funding）/ op 产出（op）' },
  { path: 'read', zone: TRANSACTION_ZONE.READ, layer: '域目录', note: '读路径：列表与单笔（list.rs）/ 来源列与转换投影（source.rs）/ 搜索与拼音修复（search.rs）' },
]

/** 核心交易域 crate 的模块根（相对 src-tauri），与 CRATES 的 ledger-transaction.dir 同源。 */
export const TRANSACTION_SRC_REL = 'crates/transaction/src'

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
  { path: 'balance.rs', layer: '域目录', note: '余额口径权威（实时计算 + V017 余额缓存整体重算刷新与读取 + 余额清单，ADR-0067/ADR-0071；余额刷新接缝实现注册点）' },
  { path: 'command.rs', layer: '域目录', note: '同步命令（op 载荷形态、产出单点与重放分派，issue #860）' },
  { path: 'core.rs', layer: '域目录', note: 'CRUD / 幂等创建 / 软删除 / 黑洞账户 / 余额调整编排 + 出资账户视图接缝实现（issue #1092）' },
  { path: 'model.rs', layer: '域目录', note: '域集中模型（账户类型枚举、实体、入参与余额读模型 DTO，#419 随域归位）' },
]

/** 账户域 crate 的模块根（相对 src-tauri），与 CRATES 的 ledger-accounts.dir 同源。 */
export const ACCOUNTS_SRC_REL = 'crates/accounts/src'

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
  { path: 'command.rs', layer: '域目录', note: '同步命令（issue #860 / ADR-0091）：op 载荷的分类域形态、产出单点与重放执行' },
  { path: 'core.rs', layer: '域目录', note: 'CRUD / 幂等创建 / 软删除 / 两级分类校验 / 预算删除守卫 / 排序重排（issue #91 域内收口）' },
  { path: 'model.rs', layer: '域目录', note: '分类实体与入参、排序项（#419 随域归位）' },
]

/** 分类域 crate 的模块根（相对 src-tauri），与 CRATES 的 ledger-categories.dir 同源。 */
export const CATEGORIES_SRC_REL = 'crates/categories/src'
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
  { path: 'base_currency.rs', layer: '域目录', note: '本位币基准（LedgerLevelSetting 首个成员，issue #858 / ADR-0091 决策 3）+ 交易×币种接缝实现注册（#1092）' },
  { path: 'command.rs', layer: '域目录', note: '账本级设置同步命令（op 载荷形态 LedgerSettingCommand / 产出单点 / 重放分派，issue #858）' },
  { path: 'list.rs', layer: '域目录', note: '币种清单查询（#404 自命令壳层迁入，IPC 与 HTTP 共用）' },
  { path: 'model.rs', layer: '域目录', note: '域模型（#418 随域归位）：币种实体 + 汇率实体' },
]

/** 币种域 crate 的模块根（相对 src-tauri），与 CRATES 的 ledger-currencies.dir 同源。 */
export const CURRENCIES_SRC_REL = 'crates/currencies/src'

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
}

/** 交易域区级认许边条目：拥有模块的清单条目路径 + 目标模块键 + 成因留痕 */
interface TransactionZoneEdge {
  file: string
  target: string
  reason: string
}

/**
 * 交易域区级认许边（ADR-0113 决策 3 / #1181）：原形状上与区级层序冲突的既有
 * 设计意图边逐条留痕于本脚本（与 INFRA_DOMAIN_ALLOWED_EDGES 同款纪律），精确到
 * 清单条目路径 + 目标模块键，附 ADR 指针；清单之外的区级反向引用一律红。两条
 * 原反边已由重排票（#1182）消除（本位币接缝归共享语义区、model→writer 转换归
 * 写路径），故本清单归空——再出现区级反向引用即红，不设认许。
 */
export const TRANSACTION_ZONE_ALLOWED_EDGES: readonly TransactionZoneEdge[] = []

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
  { path: 'command.rs', layer: '域目录', note: '商户同步命令（op 载荷形态、产出单点与重放分派，issue #860）' },
  { path: 'crud.rs', layer: '域目录', note: '商户字典域行为（列表/创建/改名/软删 + 按名查找/即建 + 交易×商户接缝实现注册，#1092）' },
  { path: 'model.rs', layer: '域目录', note: '商户域模型（#419 随域归位）' },
]

/** 商户域 crate 的模块根（相对 src-tauri），与 CRATES 的 ledger-merchants.dir 同源。 */
export const MERCHANTS_SRC_REL = 'crates/merchants/src'

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
  { path: 'command.rs', layer: '域目录', note: '保险域同步命令（issue #860 / ADR-0091）：保单与保司字典的 op 载荷形态、产出单点与重放分派' },
  { path: 'crud.rs', layer: '域目录', note: '保单档案 CRUD / 软删历史保留（ADR-0051 决策 5）+ 交易×保单接缝实现注册（#1092）' },
  { path: 'insurer.rs', layer: '域目录', note: '保司字典（issue #712 / ADR-0082）：CRUD / 在用名唯一 / 按名查找与即建' },
  { path: 'model.rs', layer: '域目录', note: '保单域模型（#420 随域归位）：保单实体 / 建档入参 / 来源列投影 / 统计行' },
  { path: 'stats.rs', layer: '域目录', note: '保单视角统计（issue #363）：实时推导不落库，度量经交易域 kind→度量矩阵驱动' },
  { path: 'validation.rs', layer: '域目录', note: '建档/编辑入参校验与归一化（保司在用 / 日期成对 / 保额币种成对）' },
]

/** 保单域 crate 的模块根（相对 src-tauri），与 CRATES 的 ledger-policy.dir 同源。 */
export const POLICY_SRC_REL = 'crates/policy/src'

/**
 * crate 分层词汇（crate 边界核对用）：壳 → 域 → 基础设施单向。
 * 与上面的 `LAYER`（单 crate 内的**模块路径**分层：域目录 / 基础设施）刻意分开——
 * 两者是不同粒度的事实源，同名值不合并（合并只会让任一侧语义被动漂移）。
 */
export const CRATE_LAYER = {
  SHELL: '壳',
  DOMAIN: '域',
  INFRA: '基础设施',
  PROTOCOL: '协议',
} as const

export type CrateLayer = (typeof CRATE_LAYER)[keyof typeof CRATE_LAYER]

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
}

/** crate 边界条目（单一事实源，spec #1086 / issue #1087）：dir 相对 src-tauri。 */
export interface CrateEntry {
  name: string
  dir: string
  layer: CrateLayer
  note: string
}

/**
 * crate 边界清单：workspace 成员、分层与允许的依赖方向（壳 → 域 → 基础设施）
 * 的唯一事实源——结构守门据此核对成员登记、门禁继承与依赖方向；每拆一个域
 * crate 在此追加一行（与 WHITELIST 同为「已验证事实固化为规格」）。
 */
export const CRATES: readonly CrateEntry[] = [
  {
    name: 'tauri-app',
    dir: '.',
    layer: CRATE_LAYER.SHELL,
    note: 'tauri 应用包：命令注册扫描、IPC/HTTP 壳与集成测试入口；过渡期仍承载尚未迁出的域，随 P1–P5 逐域迁出',
  },
  {
    name: 'ledger-infra',
    dir: 'crates/infra',
    layer: CRATE_LAYER.INFRA,
    note: '基础设施 crate（#1088 全量归位：数据库/错误/设置/文件工具/日志/事件/信号/闭集/壳层统一读写入口 + 载荷脱敏；对根包只以再导出面存在，基础设施→域生产边为 0——提交点后置动作经注册点反转，接线在壳层启动）',
  },
  {
    name: 'ledger-sync-protocol',
    dir: 'crates/sync-protocol',
    layer: CRATE_LAYER.PROTOCOL,
    note: '同步协议 crate（#1089 下放：设备标识、领域命令契约 SyncCommand/ReplayEffect、op 本地记录与读取、流位点——业务域与 sync_engine 共同底座；对壳层与域目录零依赖，反向引用由 cargo 依赖图拒绝）',
  },
  {
    name: 'ledger-backup',
    dir: 'crates/backup',
    layer: CRATE_LAYER.DOMAIN,
    note: '备份域 crate（#1091 首个自根包域目录拆出的业务域 crate：备份/恢复引擎与自动备份调度，spec #1086）；依赖面只有基础设施——对定时计划域的置脏实现与追补触发两条引用经注册点反转（挂载点①/④，ADR-0112 决策 5），对壳层/域目录零直接依赖，反向引用由生产依赖面编译期拒绝（dev-dependency 环只覆盖测试目标）',
  },
  {
    name: 'ledger-transaction',
    dir: 'crates/transaction',
    layer: CRATE_LAYER.DOMAIN,
    note: '核心交易域 crate（#1092，P2 首个底层业务域 crate：交易写入协议/金额口径/读取与搜索，全部业务域可依赖的最底层域）；依赖面只有基础设施与同步协议——对投资/商户/币种/物品/保单/账户六向的残留边经挂载点反转收敛（#1092 前置提交，ADR-0112 决策 5），反向引用由生产依赖面编译期拒绝（dev-dependency 环只覆盖测试目标）',
  },
  {
    name: 'ledger-accounts',
    dir: 'crates/accounts',
    layer: CRATE_LAYER.DOMAIN,
    note: '账户域 crate（#1093，P3 叶子业务域 crate：账户 CRUD/余额口径与余额缓存/同步命令，可被投资域与多端同步域依赖）；依赖面只有基础设施、同步协议与核心交易域（accounts → transaction 单向，ADR-0071 决策 5 修订后方向）——核心交易域写路径的余额刷新与出资账户视图两处接缝实现住本域、壳层启动接线，反向引用由生产依赖面编译期拒绝（dev-dependency 环只覆盖测试目标）',
  },
  {
    name: 'ledger-categories',
    dir: 'crates/categories',
    layer: CRATE_LAYER.DOMAIN,
    note: '分类域 crate（#1094，P3 叶子域，参考数据三域各自独立 crate 不合并：分类 CRUD/幂等创建/预算删除守卫/排序重排）；依赖面只有基础设施与同步协议（允许集「基础设施、协议、核心交易域」的子集，对交易域亦零依赖），无接缝无注册点、壳层启动零接线，反向引用由生产依赖面编译期拒绝（dev-dependency 环只覆盖测试目标）',
  },
  {
    name: 'ledger-merchants',
    dir: 'crates/merchants',
    layer: CRATE_LAYER.DOMAIN,
    note: '商户域 crate（#1096，参考数据三域各自独立 crate、不合并：商户字典 CRUD 与按名查找/即建）；依赖面只有基础设施、同步协议与核心交易域——交易×商户接缝（#1092）的实现注册侧是域→域合法上层依赖（商户 → 核心交易单向），对壳层与同步域零直接依赖，反向引用由生产依赖面编译期拒绝（dev-dependency 环只覆盖测试目标）',
  },
  {
    name: 'ledger-currencies',
    dir: 'crates/currencies',
    layer: CRATE_LAYER.DOMAIN,
    note: '币种域 crate（#1095，P3 叶子域，参考数据三域之二：币种字典/汇率/本位币基准，spec 明文裁决三域各自独立 crate 不合并）；依赖面只有基础设施、同步协议与核心交易域——本位币基准读取经注册点供给核心交易域接缝（下层提供实现、壳层启动接线，ADR-0112 决策 5），对壳层/同级业务域零直接依赖，反向引用由生产依赖面编译期拒绝（dev-dependency 环只覆盖测试目标）',
  },
  {
    name: 'ledger-policy',
    dir: 'crates/policy',
    layer: CRATE_LAYER.DOMAIN,
    note: '保单域 crate（#1100，P3 叶子业务域 crate：保单静态档案 CRUD/保司字典/保单视角统计，可被多端同步域依赖）；依赖面只有基础设施、同步协议与核心交易域——统计读路径消费 kind→度量矩阵与折算口径，交易×保单接缝（#1092）的实现注册侧是域→域合法上层依赖（保单 → 核心交易单向），对壳层与同步域零直接依赖，反向引用由生产依赖面编译期拒绝（dev-dependency 环只覆盖测试目标）',
  },
]

/** 成员目录约定（workspace glob）：新增 crate 只需落在此目录下即自动入 workspace。 */
const MEMBER_DIR_GLOB = 'crates/*'

/** 易 panic 构造六件套（ADR-0060）：workspace 级声明的键集（单一来源）。 */
const PANIC_LINT_KEYS = [
  'unwrap_used',
  'expect_used',
  'panic',
  'todo',
  'unimplemented',
  'unreachable',
] as const

/** 显式声明 workspace 范围的 cargo 命令宿主（静态检查与测试命令覆盖全成员）。 */
const WORKSPACE_COMMAND_FILES = [
  'scripts/check.sh',
  'scripts/test.sh',
  'scripts/lint-fix.sh',
  '.github/workflows/build.yml',
] as const

/** workspace 范围参数：`--workspace` 或 `--all`（后者仅在词尾，防止 `--all-targets` 假绿）。 */
const WORKSPACE_SCOPE_PATTERN = /(?:^|\s)--workspace(?:\s|$)/
const ALL_SCOPE_PATTERN = /(?:^|\s)--all(?:\s|$)/

/** 壳层依赖形态：模块路径引用（crate::commands::x / commands::x）与别名引入 */
const SHELL_DEP_PATTERN = /\bcommands\s*::|\bcommands\s+as\b/

/** 已归位域目录名（自白名单派生，单一事实源）；按长度降序防前缀吞匹配 */
const DOMAIN_NAMES: string[] = WHITELIST.filter((w) => w.layer === LAYER.DOMAIN)
  .map((w) => w.path)
  .sort((a, b) => b.length - a.length)

/**
 * 基础设施→域依赖形态（ADR-0071 决策 6 / #538）：crate 根前缀 + 域目录名，
 * 再随 `::`（路径引用）、` as `（别名引入）或 `;`（模块自身导入）；
 * 捕获组 1 = 目标域名（认许边匹配用）。`\{?\s*` 容纳花括号列举首段
 * （use crate::{accounts::x, …}）。
 */
const INFRA_DOMAIN_DEP_PATTERN = new RegExp(
  `\\b(?:crate|tauri_app_lib)\\s*::\\s*\\{?\\s*(${DOMAIN_NAMES.join('|')})\\b(?:\\s*::|\\s+as\\b|\\s*;)`,
)

/** 认许边条目：基础设施文件相对路径 + 目标域目录名 + 成因留痕 */
interface InfraDomainEdge {
  file: string
  domain: string
  reason: string
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
 * 基础设施→业务域边；清单因此由 5 条降为 4 条，且余下 4 条全是内联 cfg(test)
 * 经测试工厂建库的测试专用边（生产挂载点 0 条）。
 */
const INFRA_DOMAIN_ALLOWED_EDGES: readonly InfraDomainEdge[] = [
  {
    file: 'settings.rs',
    domain: 'test_support',
    reason: 'ADR-0084 迁移状态段 + ADR-0071 决策 6：内联 cfg(test) 测试经测试工厂建库/取常量（#758 收口），测试专用边、非产品依赖',
  },
  {
    file: 'shell_support/logger.rs',
    domain: 'test_support',
    reason: 'ADR-0084 迁移状态段 + ADR-0071 决策 6：内联 cfg(test) 测试经测试工厂建库（#758 收口），测试专用边、非产品依赖（#1130 起住 shell_support/）',
  },
  {
    file: 'shell_support/write_entry.rs',
    domain: 'test_support',
    reason: 'ADR-0084 迁移状态段 + ADR-0071 决策 6：内联 cfg(test) 测试经测试工厂建库/簿记戳引用 FIXED_NOW（#758 收口），测试专用边、非产品依赖（#1130 起住 shell_support/）',
  },
  {
    file: 'shell_support/read_entry.rs',
    domain: 'test_support',
    reason: 'ADR-0084 迁移状态段 + ADR-0071 决策 6：内联 cfg(test) 测试经测试工厂建库/种子（#758 收口），测试专用边、非产品依赖（#1130 起住 shell_support/）',
  },
]

/**
 * crate 内块间禁边（ADR-0111 决策 4 / #1134）：子目录级反向依赖断言——
 * 原语 ← db/ ← boot/ ← shell_support/ 单向，events / signals / settings 是被
 * 各层引用的共享接缝；db 不得引用 boot / shell_support / signals，boot 不得
 * 引用 shell_support。键 = 拥有该文件的块（路径首段），值 = 禁止引用的目标块；
 * 顶层单文件（ids / error / fs_util 等原语与共享接缝）不受块间禁边约束。
 */
const INFRA_BLOCK_FORBIDDEN: Record<string, readonly string[]> = {
  db: ['boot', 'shell_support', 'signals'],
  boot: ['shell_support'],
}

/** 块间依赖形态：与 INFRA_DOMAIN_DEP_PATTERN 同款形态——crate 根前缀 + 目标块名，
 *  再随 `::`（路径引用）、` as `（别名引入）或 `;`（模块自身导入）；\b 防前缀吞
 *  匹配，`\{?\s*` 容纳花括号列举首段（use crate::{boot::x, …}）；花括号列举
 *  非首段与 super:: 改写文本不可达，靠评审兜底。 */
function infraBlockDepPattern(targets: readonly string[]): RegExp {
  return new RegExp(
    `\\bcrate\\s*::\\s*\\{?\\s*(${targets.join('|')})\\b(?:\\s*::|\\s+as\\b|\\s*;)`,
  )
}

/** crate 内块间认许边条目：基础设施文件相对路径 + 目标块名 + 成因留痕 */
interface InfraBlockEdge {
  file: string
  target: string
  reason: string
}

/**
 * crate 内块间认许边（ADR-0111 决策 4 / #1134）：块间反向依赖的既有设计意图
 * 边逐条留痕于本脚本，与 INFRA_DOMAIN_ALLOWED_EDGES 同款留痕纪律——精确到
 * 文件相对路径（相对 `crates/infra/src`）+ 目标块名，附 ADR 指针；清单之外的
 * 块间反向引用一律红。
 */
const INFRA_BLOCK_ALLOWED_EDGES: readonly InfraBlockEdge[] = [
  {
    file: 'db/mod.rs',
    target: 'boot',
    reason: 'ADR-0111 决策 2 / #1131：引导层五模块升顶层 boot 后，既有 `crate::db::{boot,…}` 调用点与协议 crate 的 `ledger_infra::db::…` 路径经本再导出保持零改动——路径兼容面，非机制依赖（#1128 ids 同款口径）',
  },
]

/** 规则①形态：全局模型模块路径（全局目录已消亡，任何引用即残留） */
const GLOBAL_MODEL_PATH_PATTERN = /\b(?:crate|tauri_app_lib)\s*::\s*models\b/

/** 规则②形态：模型模块的 glob 再导出——域接缝 `pub use model::*` 与
 *  跨域/旧目录同名拍平 `pub use …::models::*`；逐类型花括号列举不命中 */
const MODEL_GLOB_REEXPORT_PATTERN = /\bpub\s+use\s+[\w:]*\bmodels?\b\s*::\s*\*/

/** 规则②形态：任意 glob 再导出（仅用于域模型文件内的聚合扫描） */
const MODEL_FILE_GLOB_PATTERN = /\bpub\s+use\s+[\w:]*\*/

/** 规则③形态：产品代码原生事务语句（issue #1014 / #1003 grilling 定案 7）——
 *  事务壳（无条件自持 `hold_transaction` / 嵌套感知 `ensure_transaction`）归
 *  基础设施 `db::tx_scope`，其余位置手写 `BEGIN` / `COMMIT` / `ROLLBACK` 即红。
 *  靶形态落在字符串字面量里，扫描须 `keepLiterals=true`（只掩码注释）。 */
const NATIVE_TX_STMT_PATTERN = /\bexecute\s*\(\s*"(?:BEGIN|COMMIT|ROLLBACK)\b/

/** 原生事务语句唯一合法住址（事务原语本体，issue #1014；#1088 起住基础设施 crate） */
const NATIVE_TX_STMT_ALLOWED = `${INFRA_SRC_REL}/db/tx_scope.rs`

/** 业务域→同步域引用锚点（crate 根前缀限定；掩码后匹配）——#1089 起零容忍，任何命中即红 */
const SYNC_ENGINE_REF_PATTERN = /\b(?:crate|tauri_app_lib)\s*::\s*sync_engine\b/

/** 域间禁边规则条目：from 域目录内文件引用 to 域目录即红（认许边除外） */
interface DomainPairRule {
  from: string
  to: string
  /**
   * 附加文本形态（#1091 crate 拆分）：目标域拆为独立 crate 后的 crate 名直引
   * 前缀（`ledger_backup::`），与 domainPairDepPattern(to) 的 crate 根前缀形态
   * （`crate::backup` / `tauri_app_lib::backup` 再导出面）并扫——两形都红。
   * 经别名改名的间接引用文本不可达，靠评审兜底。
   */
  extraPattern?: RegExp
  reason: string
}

/**
 * 域间禁边（issue #1090 / spec #1086 形态推广）：三类写路径副作用（受影响账户
 * 余额重算 / 来源列计划反查 / 期次落账置脏）已收口为「下层定义注册点、上层注册
 * 实现、壳层启动时接线」的接缝反转形态（与 #1088 基础设施提交点后置动作同构），
 * 域间横向直接依赖随接缝消亡——残留引用（import、全限定调用、花括号列举首段）
 * 即红。作用域限业务域目录（认许边逐条留痕于 DOMAIN_PAIR_ALLOWED_EDGES），
 * 文本级扫描、掩码注释与字面量后匹配，别名改写不可达靠评审兑底。
 */
/**
 * 域间禁边（issue #1090 / spec #1086 形态推广）：写路径/读路径副作用已收口为
 * 「下层定义注册点、上层注册实现、壳层启动时接线」的接缝反转形态（与 #1088
 * 基础设施提交点后置动作同构），域间横向直接依赖随接缝消亡——残留引用（import、
 * 全限定调用、花括号列举首段）即红。作用域限业务域目录（认许边逐条留痕于
 * DOMAIN_PAIR_ALLOWED_EDGES），文本级扫描、掩码注释与字面量后匹配，别名改写
 * 不可达靠评审兑底。
 *
 * 以 transaction 为起点的禁边已随 #1092 crate 化退役：核心交易域拆为
 * ledger-transaction crate 后，对任何业务域/壳层的引用由 cargo 依赖图编译期
 * 拒绝（生产依赖面无根包），文本扫描不再可及（crate 名直引形态亦不存在——
 * 域层对下层 crate 的合法引用走 ledger_transaction::，方向合法不属禁边）。
 */
export const DOMAIN_PAIR_FORBIDDEN: readonly DomainPairRule[] = [
  {
    from: 'scheduled_transactions',
    to: 'backup',
    extraPattern: /\bledger_backup\s*::/,
    reason:
      'issue #1090 / spec #1086 形态推广：期次落账置脏经注册点反转'
      + '（auto_run 注册点 + backup::occurrence_dirty_hook 实现注册，#1091 起实现住 ledger-backup crate），'
      + 'scheduled_transactions → backup 直接引用禁令——再导出面（crate::backup / '
      + 'tauri_app_lib::backup）与 crate 名直引（ledger_backup::）两形都红',
  },
]

/** 域间禁边认许边条目：文件相对路径（相对根 src）+ from/to + 成因留痕 */
interface DomainPairAllowedEdge {
  file: string
  from: string
  to: string
  reason: string
}

/**
 * 域间禁边认许边（issue #1090）：既有设计意图边逐条留痕于本脚本，与
 * INFRA_DOMAIN_ALLOWED_EDGES 同款留痕纪律——精确到文件相对路径，附成因；
 * 清单之外的域间禁边引用一律红。
 */
export const DOMAIN_PAIR_ALLOWED_EDGES: readonly DomainPairAllowedEdge[] = [
  // #1092 后为空：唯一一条（transaction/funding.rs → accounts 的 AccountType
  // 类型只读边）已随出资账户视图接缝反转消亡，清单保留为空集留痕。
]

/** 域间禁边依赖形态：与 INFRA_DOMAIN_DEP_PATTERN 同款——crate 根前缀 + 目标域名。 */
function domainPairDepPattern(to: string): RegExp {
  return new RegExp(
    `\\b(?:crate|tauri_app_lib)\\s*::\\s*\\{?\\s*(${to})\\b(?:\\s*::|\\s+as\\b|\\s*;)`,
  )
}

/** 花括号列举内的违规条目头（深度 0 逐条切分后取首个标识符；#1089 零容忍，
 *  全部条目违规）。返回违规条目头，供调用方构造命中。 */
function disallowedBraceEntries(body: string): string[] {
  const out: string[] = []
  let depth = 0
  let current = ''
  const flush = (): void => {
    const head = current.trim().split(/[\s:{]/)[0] ?? ''
    if (head !== '') {
      out.push(head)
    }
    current = ''
  }
  for (const ch of body) {
    if (ch === '{' || ch === '(' || ch === '[') depth++
    else if (ch === '}' || ch === ')' || ch === ']') depth--
    if (ch === ',' && depth === 0) flush()
    else current += ch
  }
  flush()
  return out
}

/**
 * 业务域→同步域零容忍扫描（ADR-0101 决策 4b / #1089 收紧）：业务域对同步域的
 * 任何代码引用（根引入、别名引入、`::` 子路径、花括号列举）一律命中——同步
 * 协议面已下放协议 crate，业务域只依赖 `ledger_sync_protocol`。文本级扫描
 *（掩码注释与字面量）。
 */
export function scanSyncEngineRefs(text: string): ScanHit[] {
  const masked = maskNonCode(text)
  const rawLines = text.split('\n')
  const hits: ScanHit[] = []
  const lineOf = (index: number): number => (masked.slice(0, index).match(/\n/g)?.length ?? 0) + 1
  const push = (index: number, match: string): void => {
    const line = lineOf(index)
    hits.push({ line, text: (rawLines[line - 1] ?? '').trim(), match, captured: undefined })
  }
  for (const m of masked.matchAll(new RegExp(SYNC_ENGINE_REF_PATTERN, 'g'))) {
    const start = m.index ?? 0
    const tail = masked.slice(start + m[0].length)
    const afterModule = /^\s*::\s*/.exec(tail)
    if (!afterModule) {
      // 无 `::` 子路径：根模块引入（`use crate::sync_engine;` / `as se;`）——
      // 零容忍下同样红（别名改写会让后续引用文本不可达，与既有壳层扫描的
      // `commands as` 同款堵漏）。
      push(start, /^\s+as\b/.test(tail) ? `${m[0]} as …` : m[0])
      continue
    }
    const cursor = start + m[0].length + afterModule[0].length
    if (masked[cursor] === '{') {
      // 根花括号列举：跨行取匹配闭括号后逐条报违规（零容忍，全条目违规）。
      let depth = 0
      let close = -1
      for (let i = cursor; i < masked.length; i++) {
        if (masked[i] === '{') depth++
        else if (masked[i] === '}') {
          depth--
          if (depth === 0) {
            close = i
            break
          }
        }
      }
      const body = masked.slice(cursor + 1, close === -1 ? masked.length : close)
      for (const entry of disallowedBraceEntries(body)) {
        push(start, `sync_engine::{…${entry}…}`)
      }
      continue
    }
    const segment = /^([A-Za-z_][A-Za-z0-9_]*)/.exec(masked.slice(cursor))
    if (!segment) {
      // `sync_engine::*` 等非具名形态：一律红。
      push(start, masked.slice(start, cursor + 1))
      continue
    }
    push(start, `sync_engine::${segment[1]}`)
  }
  return hits
}

/**
 * 域模型文件或模型目录成员（ADR-0059 目标形状：每域一个 model.rs，先例名
 * models.rs；#1181 起判据扩到目录形态——模型目录 model/ / models/ 下的成员
 * 文件同守「域模型禁止 glob 聚合」，模型目录化不再静默失靶）
 */
function isModelFile(relPath: string): boolean {
  const segments = relPath.split('/')
  const file = segments[segments.length - 1]
  if (file === 'model.rs' || file === 'models.rs') return true
  return segments.slice(0, -1).some((s) => s === 'model' || s === 'models')
}

/** 测试豁免形态（ADR-0056 决策 5）：tests.rs 文件与 tests/ 目录 */
function isTestFile(relPath: string): boolean {
  const segments = relPath.split('/')
  const file = segments[segments.length - 1]
  return file === 'tests.rs' || segments.slice(0, -1).includes('tests')
}

/**
 * 掩码 Rust 源文本中的注释与字符串/char 字面量：内容替换为等长空白
 * （保留换行与列位，行号不变），使依赖扫描只落在真实代码上。
 * 处理形态：行注释（//、///、//!）、块注释（/* .. *&#47;，可嵌套）、
 * 普通字符串（含转义）、原始字符串 r"…" / r#"…"#（多级 #）、
 * char 字面量（'a'、'\n'、'\u{…}'）；生命周期标注（'a）按非字面量处理。
 * `keepLiterals=true` 时保留字符串/char 字面量内容、只掩码注释——用于靶形态
 * 落在字符串里的扫描（原生事务语句 `execute("BEGIN")`，issue #1014）。
 */
export function maskNonCode(text: string, keepLiterals = false): string {
  const out = text.split('')
  const n = text.length
  const blank = (from: number, to: number): void => {
    for (let k = from; k < to && k < n; k++) if (out[k] !== '\n') out[k] = ' '
  }
  let i = 0
  while (i < n) {
    const c = text[i]
    if (c === '/' && text[i + 1] === '/') {
      // 行注释（含 /// 与 //!）到行尾
      const end = text.indexOf('\n', i)
      const stop = end === -1 ? n : end
      blank(i, stop)
      i = stop
    } else if (c === '/' && text[i + 1] === '*') {
      // 块注释，Rust 可嵌套
      let depth = 1
      let j = i + 2
      while (j < n && depth > 0) {
        if (text[j] === '/' && text[j + 1] === '*') {
          depth++
          j += 2
        } else if (text[j] === '*' && text[j + 1] === '/') {
          depth--
          j += 2
        } else {
          j++
        }
      }
      blank(i, j)
      i = j
    } else if (c === '"') {
      // 普通字符串：跳过转义对
      let j = i + 1
      while (j < n) {
        if (text[j] === '\\') j += 2
        else if (text[j] === '"') {
          j++
          break
        } else j++
      }
      if (!keepLiterals) blank(i, j)
      i = j
    } else if (c === 'r' && (text[i + 1] === '"' || (text[i + 1] === '#' && text[i + 2] === '"'))) {
      // 原始字符串 r"…" / r#"…"# / r##"…"##；前一字 符为标识符成分时是普通名字（如 for），不误伤
      const prev = i > 0 ? text[i - 1] : ''
      if (/[A-Za-z0-9_]/.test(prev)) {
        i++
        continue
      }
      let hashes = 0
      let j = i + 1
      while (text[j] === '#') {
        hashes++
        j++
      }
      const close = '"' + '#'.repeat(hashes)
      const end = text.indexOf(close, j + 1)
      const stop = end === -1 ? n : end + close.length
      if (!keepLiterals) blank(i, stop)
      i = stop
    } else if (c === "'") {
      // char 字面量 vs 生命周期：有闭引号为字面量，否则是生命周期标注（'a）
      let j = i + 1
      if (text[j] === '\\') {
        j++
        if (text[j] === '{') {
          const e = text.indexOf('}', j)
          j = e === -1 ? n : e + 1
        } else {
          j++
        }
      } else {
        j++
      }
      if (text[j] === "'") {
        const stop = j + 1
        if (!keepLiterals) blank(i, stop)
        i = stop
      } else {
        i++
      }
    } else {
      i++
    }
  }
  return out.join('')
}

/** 单条扫描命中：行号（1 起算）、原文行、匹配文本、捕获组 1
 *  （无捕获组时 undefined，基础设施→域形态为目标域名） */
export interface ScanHit {
  line: number
  text: string
  match: string
  captured: string | undefined
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
  const hits: ScanHit[] = []
  const masked = maskNonCode(text, keepLiterals)
  const maskedLines = masked.split('\n')
  const rawLines = text.split('\n')
  for (let i = 0; i < maskedLines.length; i++) {
    const m = maskedLines[i].match(pattern)
    if (m) hits.push({ line: i + 1, text: rawLines[i].trim(), match: m[0], captured: m[1] })
  }
  return hits
}

/** 收集到的 Rust 文件引用：绝对路径 + 相对路径（输出与报文用） */
interface RustFileRef {
  abs: string
  rel: string
}

/** 递归收集目录下全部 .rs 文件（跳过测试豁免形态），相对路径排序保证输出确定 */
function collectRustFiles(dir: string, relBase: string): RustFileRef[] {
  const out: RustFileRef[] = []
  for (const entry of readdirSync(dir, { withFileTypes: true }).sort((a, b) =>
    a.name.localeCompare(b.name),
  )) {
    const abs = join(dir, entry.name)
    const rel = relBase ? `${relBase}/${entry.name}` : entry.name
    if (isTestFile(rel)) continue
    if (entry.isDirectory()) out.push(...collectRustFiles(abs, rel))
    else if (entry.name.endsWith('.rs')) out.push({ abs, rel })
  }
  return out
}

/** 取 TOML 段内容（段头到下一个段头之间，不含段头行）；段不存在返回 null。 */
function manifestSection(text: string, section: string): string | null {
  const lines = text.split('\n')
  const header = `[${section}]`
  let start = -1
  for (let i = 0; i < lines.length; i++) {
    const trimmed = lines[i].trim()
    if (start === -1) {
      if (trimmed === header) start = i + 1
    } else if (trimmed.startsWith('[')) {
      return lines.slice(start, i).join('\n')
    }
  }
  return start === -1 ? null : lines.slice(start).join('\n')
}

/** 清单 [package] name（缺失返回 null）。 */
function manifestPackageName(manifest: string): string | null {
  const section = manifestSection(manifest, 'package')
  const m = section?.match(/(?:^|\n)\s*name\s*=\s*"([^"]+)"/)
  return m ? m[1] : null
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
  const names = new Set<string>()
  const tableRe = /^(?:target\..+\.)?dependencies(?:\.([A-Za-z0-9_-]+))?$/
  let current = ''
  for (const raw of manifest.split('\n')) {
    const header = raw.trim().match(/^\[([^\]]+)\]$/)
    if (header) {
      current = header[1]
      const sub = tableRe.exec(current)
      if (sub?.[1]) names.add(sub[1])
      continue
    }
    const sub = tableRe.exec(current)
    if (!sub || sub[1]) continue
    const key = raw.trim().match(/^([A-Za-z0-9_-]+)\s*=/)
    if (key) names.add(key[1])
  }
  return [...names]
}

/** [lints] 段声明 workspace 继承（`workspace = true`）——六件套门禁的继承接线。 */
function inheritsWorkspaceLints(manifest: string): boolean {
  const section = manifestSection(manifest, 'lints')
  return section !== null && /(?:^|\n)\s*workspace\s*=\s*true\b/.test(section)
}

/**
 * 声明（declIndex）前属性链中的首条单行 `#[cfg(...)]` 原文行：跳过空行与注释、
 * 透明放行其它属性（如 `#[doc(hidden)]`），停在首个非属性行——无 cfg 即 null。
 * 供生产编译 feature 门的各判定共用（test_utils / http 投影，ADR-0111 决策 5）。
 */
function firstCfgLineBefore(lines: readonly string[], declIndex: number): string | null {
  for (let i = declIndex - 1; i >= 0; i--) {
    const line = lines[i].trim()
    if (line === '' || line.startsWith('//')) continue
    if (line.startsWith('#[')) {
      if (line.startsWith('#[cfg(')) return line
      continue
    }
    return null
  }
  return null
}

/**
 * 测试器具生产编译门的「放行测试」判定（ADR-0111 决策 5 / issue #1132）：声明
 * （`pub mod test_utils;` / `pub use ledger_infra::test_utils;`）前的属性链中须有
 * 一条单行 `#[cfg(...)]`，且该 cfg 在 `test` 或 `test-utils` feature 下放行——无门、
 * `#[cfg(not(test))]` 等反向门、与测试无关的 cfg 一律不合格（判为生产会编译）。
 * 声明前允许注释与其它属性（如 `#[doc(hidden)]`），属性顺序不敏感。
 */
function hasTestAllowingCfgGate(lines: readonly string[], declIndex: number): boolean {
  const cfg = firstCfgLineBefore(lines, declIndex)
  return cfg !== null && /\btest\b/.test(cfg) && !cfg.includes('not(')
}

/**
 * HTTP 投影 impl 的 feature cfg 门判定（ADR-0111 决策 5 / issue #1133）：声明
 * （`impl axum::response::IntoResponse for AppError`）前的属性链中须有一条单行
 * `#[cfg(...)]` 含 `feature = "http"`；无门、`#[cfg(not(feature = "http"))]` 等
 * 反向门一律不合格（feature 开启实现反而消失，等价于无门）。同 hasTestAllowingCfgGate，
 * 声明前允许注释与其它属性，属性顺序不敏感。
 */
function hasHttpFeatureCfgGate(lines: readonly string[], declIndex: number): boolean {
  const cfg = firstCfgLineBefore(lines, declIndex)
  return cfg !== null && /feature\s*=\s*"http"/.test(cfg) && !cfg.includes('not(')
}

/**
 * 生产依赖表（`[dependencies]` 与 `target.*.dependencies`，inline 或子表形态，
 * 刻意排除 `[dev-dependencies]`）中对 `ledger-infra` 启用指定 feature 的原文行；
 * 命中的行即「生产构建会把该 feature 编入」的证据（ADR-0111 决策 5：#1132 用
 * 于 test-utils、#1133 用于 http）。
 */
function productionLedgerInfraEnablement(manifest: string, feature: string): string | null {
  // 词边界匹配：'http' 不得误命中 https:// 等 URL 里的裸 'http' 子串。
  const featureRe = new RegExp(`\\b${feature}\\b`)
  let section = ''
  for (const raw of manifest.split('\n')) {
    const header = raw.trim().match(/^\[([^\]]+)\]$/)
    if (header) {
      section = header[1]
      continue
    }
    if (/^(?:target\..+\.)?dependencies$/.test(section)) {
      if (/^\s*ledger-infra\s*=/.test(raw) && featureRe.test(raw)) return raw.trim()
    } else if (/^(?:target\..+\.)?dependencies\.ledger-infra$/.test(section)) {
      if (/^\s*features\s*=/.test(raw) && featureRe.test(raw)) return raw.trim()
    }
  }
  return null
}

/**
 * `[features] default` 是否（直接或经本清单内 feature 转发）触达指定 feature——
 * 默认 feature 在生产构建启用，等价于无条件编入该 feature 的内容。转发链上的
 * 边按 `includes(feature)` 判定，因而也覆盖 `ledger-infra/test-utils` 这类
 * 跨 crate 引用形态（ADR-0111 决策 5：#1132 用于 test-utils、#1133 用于 http）。
 * 跨清单的依赖 feature 图不在文本可辨范围，靠评审兜底。
 */
function defaultFeaturesInclude(manifest: string, feature: string): boolean {
  const section = manifestSection(manifest, 'features')
  if (section === null) return false
  const featureEdges = new Map<string, string[]>()
  for (const line of section.split('\n')) {
    const m = line.match(/^\s*([A-Za-z0-9_-]+)\s*=\s*\[([^\]]*)\]/)
    if (m === null) continue
    featureEdges.set(
      m[1],
      [...m[2].matchAll(/"([^"]+)"/g)].map((x) => x[1]),
    )
  }
  const seen = new Set<string>()
  const pending = [...(featureEdges.get('default') ?? [])]
  while (pending.length > 0) {
    const current = pending.pop() as string
    if (current.includes(feature)) return true
    if (seen.has(current)) continue
    seen.add(current)
    pending.push(...(featureEdges.get(current) ?? []))
  }
  return false
}

/** `crates/` 下含 Cargo.toml 的成员 crate 目录（相对 src-tauri，排序保证输出确定）。 */
function memberCrateDirs(srcTauriDir: string): string[] {
  const cratesDir = join(srcTauriDir, 'crates')
  if (!existsSync(cratesDir)) return []
  return readdirSync(cratesDir, { withFileTypes: true })
    .filter((e) => e.isDirectory() && existsSync(join(cratesDir, e.name, 'Cargo.toml')))
    .map((e) => `crates/${e.name}`)
    .sort()
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
  const problems: string[] = []
  const repoRoot = dirname(srcTauriDir)
  const rootManifestPath = join(srcTauriDir, 'Cargo.toml')
  if (!existsSync(rootManifestPath)) {
    problems.push(`✗ crate 边界：workspace 根清单不存在：${rootManifestPath}`)
    return problems
  }
  const rootManifest = readFileSync(rootManifestPath, 'utf8')

  // ① workspace 骨架 + 成员目录 glob（新增 crate 自动成为 workspace 成员）
  const workspaceSection = manifestSection(rootManifest, 'workspace')
  if (workspaceSection === null) {
    problems.push(
      '✗ crate 边界：src-tauri/Cargo.toml 缺 [workspace] 段——Rust 根须为 workspace 根（spec #1086）',
    )
  } else if (!workspaceSection.includes(`"${MEMBER_DIR_GLOB}"`)) {
    problems.push(
      `✗ crate 边界：[workspace] members 未包含 "${MEMBER_DIR_GLOB}"——成员目录须用 glob 纳入，` +
        '新增 crate 自动入 workspace，漏项即静默漏检',
    )
  }

  // ② 六件套门禁的唯一声明处（workspace 级）
  const clippyLints = manifestSection(rootManifest, 'workspace.lints.clippy')
  if (clippyLints === null) {
    problems.push(
      '✗ 门禁继承：[workspace.lints.clippy] 缺失——六件套 deny 门禁的唯一声明处（ADR-0060 / spec #1086）',
    )
  } else {
    for (const key of PANIC_LINT_KEYS) {
      if (!new RegExp(`(?:^|\\n)\\s*${key}\\s*=\\s*"deny"`).test(clippyLints)) {
        problems.push(`✗ 门禁继承：[workspace.lints.clippy] 缺 ${key} = "deny"（ADR-0060 六件套）`)
      }
    }
  }

  // ③ 根包同为 workspace 成员，须继承门禁
  if (!inheritsWorkspaceLints(rootManifest)) {
    problems.push(
      '✗ 门禁继承：workspace 根包缺 [lints] workspace = true——根包同受六件套约束（ADR-0060）',
    )
  }

  // ④ 成员登记：磁盘成员目录 ↔ CRATES 双向核对（新 crate 未登记即红）
  const onDisk = memberCrateDirs(srcTauriDir)
  const registeredMemberDirs = CRATES.filter((c) => c.dir.startsWith('crates/'))
    .map((c) => c.dir)
    .sort()
  for (const dir of onDisk) {
    if (!CRATES.some((c) => c.dir === dir)) {
      problems.push(
        `✗ crate 边界：成员 crate 未登记 CRATES：${dir}\n` +
          '    新增 crate 后须在 scripts/check-structure.ts 的 CRATES 追加一行（分层 + 注释），' +
          '否则边界知识分裂成两份、依赖方向失守',
      )
    }
  }
  for (const dir of registeredMemberDirs) {
    if (!onDisk.includes(dir)) {
      problems.push(`✗ crate 边界：CRATES 登记的成员目录不存在：${dir}（清单漂移 fail loud）`)
    }
  }

  // ⑤ 每个 crate：清单存在、包名一致、门禁继承、依赖方向单向
  for (const crate of CRATES) {
    const isRoot = crate.dir === '.'
    const manifestPath = isRoot ? rootManifestPath : join(srcTauriDir, crate.dir, 'Cargo.toml')
    if (!existsSync(manifestPath)) {
      problems.push(`✗ crate 边界：crate 清单不存在：${crate.name}（${crate.dir}）`)
      continue
    }
    const manifest = isRoot ? rootManifest : readFileSync(manifestPath, 'utf8')
    const name = manifestPackageName(manifest)
    if (name !== crate.name) {
      problems.push(
        `✗ crate 边界：CRATES 登记名 ${crate.name} 与清单包名 ${name ?? '（缺 name）'} 不一致（${crate.dir}）`,
      )
    }
    if (!isRoot && !inheritsWorkspaceLints(manifest)) {
      problems.push(
        `✗ 门禁继承：成员 crate ${crate.name} 缺 [lints] workspace = true（${crate.dir}/Cargo.toml）\n` +
          '    缺失即六件套 deny 门禁静默消失而 clippy 依然全绿——删除继承行即变红（ADR-0060 / spec #1086）',
      )
    }
    // 依赖方向只看生产依赖（dev-dependency 环是 spec #1086 明文裁决的测试专用边，
    // 见 declaredProductionDependencyNames 注释）。
    for (const dep of declaredProductionDependencyNames(manifest)) {
      const target = CRATES.find((c) => c.name === dep)
      if (target && CRATE_LAYER_RANK[target.layer] > CRATE_LAYER_RANK[crate.layer]) {
        problems.push(
          `✗ crate 依赖方向：${crate.name}（${crate.layer}）依赖 ${target.name}（${target.layer}）\n` +
            '    分层规则：壳 → 域 → 基础设施单向；被依赖逻辑应下沉到更低层（spec #1086）',
        )
      }
    }
  }

  // ⑥ 静态检查与测试命令覆盖全成员（缺 --workspace 即静默漏检成员）
  for (const rel of WORKSPACE_COMMAND_FILES) {
    const abs = join(repoRoot, rel)
    if (!existsSync(abs)) {
      problems.push(`✗ workspace 命令覆盖：宿主文件不存在：${rel}`)
      continue
    }
    const lines = readFileSync(abs, 'utf8').split('\n')
    lines.forEach((line, i) => {
      if (line.trim().startsWith('#')) return // 注释行（含 workflow 说明）不算命令
      // 逐条命令核对（一行可有 `cargo fmt … && cargo clippy …` 多条：只看首个
      // 匹配会把未覆盖的 clippy 放过去）；命令段截到下一个 shell 控制符为止。
      const re = /\bcargo\s+(clippy|test|fmt)\b/g
      let m: RegExpExecArray | null
      while ((m = re.exec(line))) {
        const rest = line.slice(m.index)
        const end = rest.search(/&&|;|\|/)
        const segment = end === -1 ? rest : rest.slice(0, end)
        // `--all` 仅在词尾才算 workspace 别名：`--all-targets` / `--all-features`
        // 的 `\b` 落在 `-` 前，用 \b 会假绿（本核对要拦的正是这一形态）。
        if (WORKSPACE_SCOPE_PATTERN.test(segment) || ALL_SCOPE_PATTERN.test(segment)) continue
        problems.push(
          `✗ workspace 命令覆盖：${rel}:${i + 1} cargo ${m[1]} 缺 --workspace` +
            '（非虚拟 workspace 下默认只作用于根包，会静默漏检成员 crate）\n' +
            `    ${line.trim()}`,
        )
      }
    })
  }

  // ⑦ 测试导出生产编译门（ADR-0111 决策 5）：测试目标专用导出默认不进生产
  // 编译，由构建形态保证，而非注释约定。删除即变红——clippy 走
  // `--all-features`、测试走 dev-dependency，都发现不了门被摘掉：
  //   ① infra `test_utils` 模块声明须带「放行测试」的 cfg 门（无门/反向门即生产编译）；
  //   ② 根包 `test_utils` 再导出须带同一形态的门（无门即生产构建解析失败）；
  //   ③ 生产依赖（`[dependencies]` 与 target 变体）不得对 ledger-infra 启用 test-utils；
  //   ④ 根包与 infra 的 `[features] default` 不得包含 test-utils（默认 feature 即生产）；
  //   ⑤ 投资五节标题锚点常量（issue #1185，住 `handlers/import.rs`）须带同一形态的门；
  //   ⑥ 锚点再导出（`api_server/mod.rs`）须带同一形态的门。
  const gatedDecls = [
    {
      file: join(srcTauriDir, INFRA_SRC_REL, 'lib.rs'),
      re: /^\s*pub\s+mod\s+test_utils\s*;/,
      label: 'pub mod test_utils;',
      gate: 'test_utils 生产编译门',
      src: 'ADR-0111 决策 5 / issue #1132',
      productionArtifact: '测试器具',
    },
    {
      file: join(srcTauriDir, 'src', 'lib.rs'),
      re: /^\s*pub\s+use\s+ledger_infra::test_utils\s*;/,
      label: 'pub use ledger_infra::test_utils;',
      gate: 'test_utils 生产编译门',
      src: 'ADR-0111 决策 5 / issue #1132',
      productionArtifact: '测试器具',
    },
    // 投资五节标题锚点（issue #1185）：#1121 常量结构锁与 #1123 API 集成锁的
    // 共享单一住处，仅测试构建编译——门摘掉即测试锚点静默进生产二进制。
    {
      file: join(srcTauriDir, 'src', 'api_server', 'handlers', 'import.rs'),
      re: /^\s*pub\s+const\s+INVESTMENT_SECTION_HEADERS\b/,
      label: 'pub const INVESTMENT_SECTION_HEADERS',
      gate: '投资五节锚点生产编译门',
      src: 'issue #1185',
      productionArtifact: '测试锚点',
    },
    {
      file: join(srcTauriDir, 'src', 'api_server', 'mod.rs'),
      re: /^\s*pub\s+use\s+handlers::import::INVESTMENT_SECTION_HEADERS\s*;/,
      label: 'pub use handlers::import::INVESTMENT_SECTION_HEADERS;',
      gate: '投资五节锚点生产编译门',
      src: 'issue #1185',
      productionArtifact: '测试锚点',
    },
  ]
  for (const { file, re, label, gate, src, productionArtifact } of gatedDecls) {
    const rel = file.slice(srcTauriDir.length + 1)
    if (!existsSync(file)) {
      problems.push(`✗ ${gate}：${rel} 不存在，无法核对 cfg 门（${src}）`)
      continue
    }
    const lines = readFileSync(file, 'utf8').split('\n')
    const declIndex = lines.findIndex((l) => re.test(l))
    if (declIndex === -1) {
      problems.push(`✗ ${gate}：${rel} 找不到 \`${label}\` 声明`)
    } else if (!hasTestAllowingCfgGate(lines, declIndex)) {
      problems.push(
        `✗ ${gate}：${rel} \`${label}\` 未加「放行测试」cfg 门\n` +
          `    ${lines[declIndex].trim()}\n` +
          '    门须为 `#[cfg(any(test, feature = "test-utils"))]`（或等价单行 cfg）；' +
          `无门 / \`#[cfg(not(test))]\` / 与测试无关的 cfg 都会让生产编译${productionArtifact}` +
          `（${src}），删除或写反 cfg 门即变红`,
      )
    }
  }

  const prodEnableLine = productionLedgerInfraEnablement(rootManifest, 'test-utils')
  if (prodEnableLine !== null) {
    problems.push(
      '✗ test_utils 生产编译门：根包生产依赖 ledger-infra 启用了 test-utils\n' +
        `    ${prodEnableLine}\n` +
        '    test-utils 只许经测试目标（dev-dependency / cfg(test)）启用；' +
        '生产依赖启用即把测试器具编入生产构建（ADR-0111 决策 5 / issue #1132），删除该 feature 即变红',
    )
  }

  // infra 清单读一次：test-utils（⑦）与 http（⑧）两道 default 门共用同一份
  // [清单, 出处] 对与同一套核对（defaultFeaturesInclude）。
  const infraManifestPath = join(srcTauriDir, dirname(INFRA_SRC_REL), 'Cargo.toml')
  const infraManifest = existsSync(infraManifestPath)
    ? readFileSync(infraManifestPath, 'utf8')
    : null
  if (infraManifest === null) {
    problems.push(`✗ 生产编译 feature 门：${dirname(INFRA_SRC_REL)}/Cargo.toml 不存在`)
  }
  const defaultFeatureManifests: ReadonlyArray<readonly [string, string]> = [
    [rootManifest, 'src-tauri/Cargo.toml'],
    ...(infraManifest !== null
      ? [[infraManifest, `${dirname(INFRA_SRC_REL)}/Cargo.toml`] as const]
      : []),
  ]
  for (const [manifest, where] of defaultFeatureManifests) {
    if (defaultFeaturesInclude(manifest, 'test-utils')) {
      problems.push(
        `✗ test_utils 生产编译门：${where} [features] default 包含 test-utils\n` +
          '    默认 feature 在生产构建启用，等价于把测试器具编入生产构建' +
          '（ADR-0111 决策 5 / issue #1132），从 default 移除 test-utils 即变红',
      )
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
  const httpGateLabel = 'http 投影 feature 门'
  if (infraManifest !== null) {
    const axumLine = infraManifest.split('\n').find((l) => /^\s*axum\s*=/.test(l))
    if (axumLine === undefined) {
      problems.push(`✗ ${httpGateLabel}：infra Cargo.toml 找不到 axum 依赖声明`)
    } else if (!axumLine.includes('optional = true')) {
      problems.push(
        `✗ ${httpGateLabel}：infra Cargo.toml 的 axum 依赖未声明 optional\n` +
          `    ${axumLine.trim()}\n` +
          '    非 optional 即无条件编入 axum 及其传递依赖（ADR-0111 决策 5 / issue #1133），加回 optional 即变绿',
      )
    }
    const httpFeatureLine = manifestSection(infraManifest, 'features')
      ?.split('\n')
      .find((l) => /^\s*http\s*=/.test(l))
    if (httpFeatureLine === undefined || !httpFeatureLine.includes('dep:axum')) {
      problems.push(
        `✗ ${httpGateLabel}：infra Cargo.toml [features] 缺 \`http = ["dep:axum"]\`\n` +
          '    门与依赖面绑死——feature 不转发 dep:axum 即门形同虚设' +
          '（ADR-0111 决策 5 / issue #1133）',
      )
    }
  }
  for (const [manifest, where] of defaultFeatureManifests) {
    if (defaultFeaturesInclude(manifest, 'http')) {
      problems.push(
        `✗ ${httpGateLabel}：${where} [features] default 包含 http\n` +
          '    默认 feature 在生产构建启用，等价于无条件编入 axum' +
          '（ADR-0111 决策 5 / issue #1133），从 default 移除 http 即变红',
      )
    }
  }

  // ③' impl cfg 门：error.rs 的 IntoResponse impl 必须带 feature = "http" 的门。
  const errorRsPath = join(srcTauriDir, INFRA_SRC_REL, 'error.rs')
  if (!existsSync(errorRsPath)) {
    problems.push(
      `✗ ${httpGateLabel}：${INFRA_SRC_REL}/error.rs 不存在，无法核对 impl cfg 门（issue #1133）`,
    )
  } else {
    const lines = readFileSync(errorRsPath, 'utf8').split('\n')
    const declIndex = lines.findIndex((l) =>
      /^\s*impl\s+axum::response::IntoResponse\s+for\s+AppError\b/.test(l),
    )
    if (declIndex === -1) {
      problems.push(
        `✗ ${httpGateLabel}：error.rs 找不到 \`impl axum::response::IntoResponse for AppError\``,
      )
    } else if (!hasHttpFeatureCfgGate(lines, declIndex)) {
      problems.push(
        `✗ ${httpGateLabel}：error.rs IntoResponse impl 未加 feature cfg 门\n` +
          `    ${lines[declIndex].trim()}\n` +
          '    门须为 `#[cfg(feature = "http")]`（紧贴 impl 的属性链）；无门即无条件编译 axum 投影' +
          '（ADR-0111 决策 5 / issue #1133），补回 cfg 门即变绿',
      )
    }
  }

  // ⑤' 域侧成员启用 http → 红：http 只许壳侧（根包 tauri-app）启用，域侧启用
  // 即把 axum 编入域依赖图，违背「域侧依赖不引入 axum」口径（issue #1133）。
  for (const crateDir of memberCrateDirs(srcTauriDir)) {
    if (crateDir === dirname(INFRA_SRC_REL)) continue // infra 自身是门宿主，非消费方
    const memberManifest = readFileSync(join(srcTauriDir, crateDir, 'Cargo.toml'), 'utf8')
    const enableLine = productionLedgerInfraEnablement(memberManifest, 'http')
    if (enableLine !== null) {
      problems.push(
        `✗ ${httpGateLabel}：域侧成员 ${crateDir} 生产依赖 ledger-infra 启用了 http\n` +
          `    ${enableLine}\n` +
          '    http 只许壳侧启用；域侧启用即把 axum 编入域依赖图' +
          '（ADR-0111 决策 5 / issue #1133），移除该 feature 即变红',
      )
    }
    // 域侧直接声明 axum 同样越界（域侧依赖不引入 axum，issue #1133）——不只拦
    // ledger-infra/http 转发一条路；[dev-dependencies] 不在核对范围（测试专用边）。
    if (declaredProductionDependencyNames(memberManifest).includes('axum')) {
      problems.push(
        `✗ ${httpGateLabel}：域侧成员 ${crateDir} 生产依赖直接声明 axum\n` +
          '    域侧依赖不引入 axum——axum 只有壳层需要（ADR-0111 决策 5 / issue #1133），' +
          '移除该依赖即变红',
      )
    }
  }

  return problems
}

/**
 * INFRA_MODULES 与实际模块双向全等（ADR-0111 决策 5 / #1134）：磁盘侧枚举
 * `crates/infra/src` 顶层的实际模块——非测试豁免形态的 .rs 文件，与扫得到
 * 非测试 .rs 文件的目录（目录型条目覆盖其全部子目录，子文件不再逐行登记）——
 * 磁盘上存在而清单未登记即红（清单漂移不再只单向 fail loud）。
 * 反方向（登记路径消失 / 条目扫不到非测试文件）由 scanModuleEntries 的
 * 既有路径存在性与非测试文件数核对承担，本核对不重复报文。
 */
function checkInfraModuleEquality(srcTauriDir: string): string[] {
  const problems: string[] = []
  const srcDir = join(srcTauriDir, INFRA_SRC_REL)
  if (!existsSync(srcDir)) return problems // 清单循环逐条报「路径不存在」
  const onDisk: string[] = []
  for (const entry of readdirSync(srcDir, { withFileTypes: true }).sort((a, b) =>
    a.name.localeCompare(b.name),
  )) {
    if (entry.isFile() && entry.name.endsWith('.rs') && !isTestFile(entry.name)) {
      onDisk.push(entry.name)
    } else if (entry.isDirectory() && collectRustFiles(join(srcDir, entry.name), entry.name).length > 0) {
      onDisk.push(entry.name)
    }
  }
  for (const mod of onDisk) {
    if (!INFRA_MODULES.some((m) => m.path === mod)) {
      problems.push(
        `✗ INFRA_MODULES 未登记模块：${mod}（${INFRA_SRC_REL}）\n` +
          '    清单与实际模块双向全等（ADR-0111 决策 5 / #1134）：新增模块后须在 ' +
          'scripts/check-structure.ts 的 INFRA_MODULES 追加一行（附注释），' +
          '否则清单漏登记、块间分层与认许边核对静默漏检',
      )
    }
  }
  return problems
}

/**
 * TRANSACTION_MODULES 与实际模块双向全等（ADR-0113 决策 7 / #1181，沿用 infra
 * 侧 #1134 形态）：磁盘侧枚举 `crates/transaction/src` 顶层的实际模块——非测试
 * 豁免形态的 .rs 文件，与扫得到非测试 .rs 文件的目录（目录型条目覆盖其全部子
 * 目录，子文件不再逐行登记）——磁盘上存在而清单未登记即红（清单漂移不再只单
 * 向 fail loud）。crate 根 lib.rs 是声明与再导出面（#1092 同款不入清单），磁盘
 * 枚举同步排除：crate 根文件名恒定，新增模块经 lib.rs 声明后落磁盘即被本核对
 * 捕获，不因 lib.rs 免登产生漏检。反方向（登记路径消失 / 条目扫不到非测试文
 * 件）由 scanModuleEntries 的既有核对承担，本核对不重复报文。
 */
function checkTransactionModuleEquality(srcTauriDir: string): string[] {
  const problems: string[] = []
  const srcDir = join(srcTauriDir, TRANSACTION_SRC_REL)
  if (!existsSync(srcDir)) return problems // 清单循环逐条报「路径不存在」
  const onDisk: string[] = []
  for (const entry of readdirSync(srcDir, { withFileTypes: true }).sort((a, b) =>
    a.name.localeCompare(b.name),
  )) {
    if (
      entry.isFile() &&
      entry.name.endsWith('.rs') &&
      entry.name !== 'lib.rs' &&
      !isTestFile(entry.name)
    ) {
      onDisk.push(entry.name)
    } else if (
      entry.isDirectory() &&
      collectRustFiles(join(srcDir, entry.name), entry.name).length > 0
    ) {
      onDisk.push(entry.name)
    }
  }
  for (const mod of onDisk) {
    if (!TRANSACTION_MODULES.some((m) => m.path === mod)) {
      problems.push(
        `✗ TRANSACTION_MODULES 未登记模块：${mod}（${TRANSACTION_SRC_REL}）\n` +
          '    清单与实际模块双向全等（ADR-0113 决策 7 / #1181）：新增模块后须在 ' +
          'scripts/check-structure.ts 的 TRANSACTION_MODULES 追加一行（区归属 + 注释），' +
          '否则清单漏登记、区级层序与认许边核对静默漏检',
      )
    }
  }
  return problems
}

/** 清单条目路径 → 模块键（去 .rs 后缀：flat 文件条目 amount.rs 与目录条目 amount 同键）。 */
function transactionModuleKey(path: string): string {
  return path.replace(/\.rs$/, '')
}

/** 文件所属模块条目：精确匹配优先，其次目录前缀（目录型条目覆盖其全部子目录）。 */
function transactionOwningEntry(rel: string): TransactionModuleEntry | undefined {
  return [...TRANSACTION_MODULES]
    .sort((a, b) => b.path.length - a.path.length)
    .find((e) => rel === e.path || rel.startsWith(`${e.path}/`))
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
  const masked = maskNonCode(text)
  const rawLines = text.split('\n')
  const hits: ScanHit[] = []
  const lineOf = (index: number): number => (masked.slice(0, index).match(/\n/g)?.length ?? 0) + 1
  const push = (index: number, match: string, captured: string): void => {
    const line = lineOf(index)
    hits.push({ line, text: (rawLines[line - 1] ?? '').trim(), match, captured })
  }
  const re = /\b(?:super|crate)\s*::\s*(\{)?/g
  for (const m of masked.matchAll(re)) {
    const start = m.index ?? 0
    const after = start + m[0].length
    if (m[1] === undefined) {
      const segment = /^([A-Za-z_][A-Za-z0-9_]*)/.exec(masked.slice(after))
      if (segment) push(start, m[0] + segment[1], segment[1])
      continue
    }
    // 花括号列举：跨行取匹配闭括号，深度 0 逐条切分后取各条目首段标识符。
    // `{` 已被正则消费进 m[0]（after 在其后），深度从 1 起数——从 0 起数会使
    // 闭括号落到 -1、close 恒为 -1，body 吞到文件尾、后续枚举变体等被误报。
    let depth = 1
    let close = -1
    for (let i = after; i < masked.length; i++) {
      if (masked[i] === '{') depth++
      else if (masked[i] === '}') {
        depth--
        if (depth === 0) {
          close = i
          break
        }
      }
    }
    const body = masked.slice(after, close === -1 ? masked.length : close)
    for (const entry of disallowedBraceEntries(body)) {
      push(start, `{…${entry}…}`, entry)
    }
  }
  return hits
}

/**
 * 交易域 crate 内区级层序核对（ADR-0113 决策 3/7 / #1181）：对 crate 内全部非
 * 测试 Rust 文件，按其所属模块条目的 zone 判向——同区互依合法；跨区时秩大者
 * 方可依赖秩小者（写读 → 接缝 → 共享语义，写读两径互不依赖）；认许边之外即红。
 * crate 根 lib.rs（声明与再导出面，无所属条目）与未登记孤儿文件（双向全等已
 * 红）不参与判向。测试豁免形态由 collectRustFiles 过滤（ADR-0056 决策 5）。
 */
function checkTransactionZoneDirection(srcTauriDir: string): string[] {
  const problems: string[] = []
  const srcDir = join(srcTauriDir, TRANSACTION_SRC_REL)
  if (!existsSync(srcDir)) return problems // 清单循环逐条报「路径不存在」
  for (const f of collectRustFiles(srcDir, TRANSACTION_SRC_REL)) {
    const rel = f.rel.slice(TRANSACTION_SRC_REL.length + 1)
    const owner = transactionOwningEntry(rel)
    if (!owner) continue
    const source = readFileSync(f.abs, 'utf8')
    for (const hit of scanTransactionZoneRefs(source)) {
      const target = TRANSACTION_MODULES.find((m) => transactionModuleKey(m.path) === hit.captured)
      if (!target || target.zone === owner.zone) continue
      if (TRANSACTION_ZONE_RANK[owner.zone] > TRANSACTION_ZONE_RANK[target.zone]) continue
      const allowed = TRANSACTION_ZONE_ALLOWED_EDGES.some(
        (e) => e.file === owner.path && e.target === transactionModuleKey(target.path),
      )
      if (allowed) continue
      problems.push(
        `✗ 区级反向依赖：${owner.zone}「${owner.path}」引用 ${target.zone}「${target.path}」 → ` +
          `${f.rel}:${hit.line}（${hit.match}）\n` +
          `    ${hit.text}\n` +
          `    区级层序唯一：写路径/读路径 → 跨域接缝 → 共享语义（ADR-0113 决策 3）——` +
          `共享语义不得依赖接缝与路径区，接缝不得依赖路径区，写读两径互不依赖；` +
          `区归属与清单同址单点声明（TRANSACTION_MODULES 条目的 zone 字段）；` +
          `设计意图边须逐条留痕于本脚本 TRANSACTION_ZONE_ALLOWED_EDGES（附 ADR 指针），` +
          `或把依赖下沉到层序更低的区`,
      )
    }
  }
  return problems
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
): number {
  let scanned = 0
  for (const w of entries) {
    const abs = join(baseDir, w.path)
    let stat: Stats | undefined
    try {
      stat = statSync(abs)
    } catch {
      stat = undefined
    }
    if (!stat) {
      problems.push(`✗ 白名单路径不存在：${w.path}（${w.layer}：${w.note}）——目录改名/迁移后未同步守门清单`)
      continue
    }
    const files: RustFileRef[] = stat.isDirectory()
      ? collectRustFiles(abs, w.path)
      : [{ abs, rel: w.path }]
    if (files.length === 0) {
      problems.push(
        `✗ 白名单条目扫不到非测试 Rust 文件：${w.path}（${w.layer}：${w.note}）——` +
          `全部是测试豁免形态或已空，清单与目录形状漂移`,
      )
      continue
    }
    scanned += files.length
    const isInfra = w.layer === LAYER.INFRA
    const isProtocol = w.layer === LAYER.PROTOCOL
    // 业务域→同步域零容忍作用域：域目录，除同步域自身与测试支持域
    //（test_support→sync_engine 为测试专用边，登记处 ADR-0084 迁移状态段）。
    const isBusinessDomain =
      w.layer === LAYER.DOMAIN && w.path !== 'sync_engine' && w.path !== 'test_support'
    for (const f of files) {
      const source = readFileSync(f.abs, 'utf8')
      for (const hit of scanRustSource(source)) {
        problems.push(
          `✗ 反向依赖：${w.path} 层（${w.note}）引用壳层 → ${f.rel}:${hit.line}（${hit.match}）\n` +
            `    ${hit.text}\n` +
            `    分层规则：壳 → 域 → 基础设施，域永不依赖壳（ADR-0056）；` +
            `被依赖逻辑应下沉到域目录或基础设施，或本次迁移应把该文件一并归位`,
        )
      }
      if (isInfra) {
        // crate 内块间反向依赖（ADR-0111 决策 4 / #1134）：db 不得引用
        // boot / shell_support / signals；boot 不得引用 shell_support；
        // 认许边逐条留痕（INFRA_BLOCK_ALLOWED_EDGES），清单之外即红。
        const block = f.rel.split('/')[0]
        const forbiddenTargets = INFRA_BLOCK_FORBIDDEN[block]
        if (forbiddenTargets) {
          for (const hit of scanRustSource(source, infraBlockDepPattern(forbiddenTargets))) {
            const allowed = INFRA_BLOCK_ALLOWED_EDGES.some(
              (e) => e.file === f.rel && e.target === hit.captured,
            )
            if (allowed) continue
            problems.push(
              `✗ crate 内反向依赖：${block} 引用 ${hit.captured} → ${f.rel}:${hit.line}（${hit.match}）\n` +
                `    ${hit.text}\n` +
                `    crate 内分层（ADR-0111 决策 4）：原语 ← db ← boot ← shell_support 单向，` +
                `db 不得引用 boot/shell_support/signals，boot 不得引用 shell_support；` +
                `设计意图边须逐条留痕于本脚本 INFRA_BLOCK_ALLOWED_EDGES（附 ADR 指针），` +
                `或把逻辑下沉到更低的块`,
            )
          }
        }
        for (const hit of scanRustSource(source, INFRA_DOMAIN_DEP_PATTERN)) {
          const allowed = INFRA_DOMAIN_ALLOWED_EDGES.some(
            (e) => e.file === f.rel && e.domain === hit.captured,
          )
          if (allowed) continue
          problems.push(
            `✗ 反向依赖：${w.path}（${w.layer}：${w.note}）引用域目录 ${hit.captured} → ` +
              `${f.rel}:${hit.line}（${hit.match}）\n` +
              `    ${hit.text}\n` +
              `    分层规则：壳 → 域 → 基础设施；基础设施→域业务边归零且可机械守门` +
              `（ADR-0071 决策 6，#538）；设计意图边须逐条留痕于本脚本 ` +
              `INFRA_DOMAIN_ALLOWED_EDGES（附 ADR 指针），或把逻辑下沉域目录`,
          )
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
          )
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
          )
        }
        // 域间禁边（issue #1090）：残留的域间直接引用即红（认许边逐条留痕）。
        // 目标域拆为独立 crate 后（#1091）追加 crate 名直引前缀并扫（extraPattern）。
        for (const rule of DOMAIN_PAIR_FORBIDDEN.filter((r) => r.from === w.path)) {
          const patterns = rule.extraPattern
            ? [domainPairDepPattern(rule.to), rule.extraPattern]
            : [domainPairDepPattern(rule.to)]
          for (const pattern of patterns) {
            for (const hit of scanRustSource(source, pattern)) {
              const allowed = DOMAIN_PAIR_ALLOWED_EDGES.some(
                (e) => e.file === f.rel && e.from === rule.from && e.to === rule.to,
              )
              if (allowed) continue
              problems.push(
                `✗ 域间禁边：${f.rel} 引用 ${rule.to} → ${f.rel}:${hit.line}（${hit.match}）\n` +
                  `    ${hit.text}\n` +
                  `    ${rule.reason}\n` +
                  `    写路径副作用一律经注册点反转形态（下层定义注册点、上层注册实现、` +
                  `壳层启动接线，spec #1086 / #1090）；设计意图边须逐条留痕于本脚本 ` +
                  `DOMAIN_PAIR_ALLOWED_EDGES（附成因）`,
              )
            }
          }
        }
      }
    }
  }
  return scanned
}

function main(): void {
  const repoRoot = fileURLToPath(new URL('..', import.meta.url))
  const srcDir = process.argv[2] ?? join(repoRoot, 'src-tauri', 'src')
  const srcTauriDir = process.argv[3] ?? join(repoRoot, 'src-tauri')
  const problems: string[] = []
  let scannedFiles = 0
  const domainCount = WHITELIST.filter((w) => w.layer === LAYER.DOMAIN).length

  // 模型域化禁令（规则①/②）+ 原生事务语句禁令（规则③）：全树扫描（壳、域、
  // 基础设施、顶层文件），残留引用可出现在任何层；collectRustFiles 自带测试
  // 豁免（ADR-0056 决策 5）——外挂测试目录的直置事务边界合法。
  // srcDir 整体不可达时静默交由白名单循环报「路径不存在」，不在此抛栈。
  let allFiles: RustFileRef[] = []
  try {
    allFiles = [
      ...collectRustFiles(srcDir, ''),
      ...collectRustFiles(join(srcTauriDir, INFRA_SRC_REL), INFRA_SRC_REL),
      ...collectRustFiles(join(srcTauriDir, PROTOCOL_SRC_REL), PROTOCOL_SRC_REL),
      ...collectRustFiles(join(srcTauriDir, BACKUP_SRC_REL), BACKUP_SRC_REL),
      ...collectRustFiles(join(srcTauriDir, TRANSACTION_SRC_REL), TRANSACTION_SRC_REL),
      ...collectRustFiles(join(srcTauriDir, ACCOUNTS_SRC_REL), ACCOUNTS_SRC_REL),
      ...collectRustFiles(join(srcTauriDir, CATEGORIES_SRC_REL), CATEGORIES_SRC_REL),
      ...collectRustFiles(join(srcTauriDir, MERCHANTS_SRC_REL), MERCHANTS_SRC_REL),
      ...collectRustFiles(join(srcTauriDir, CURRENCIES_SRC_REL), CURRENCIES_SRC_REL),
      ...collectRustFiles(join(srcTauriDir, POLICY_SRC_REL), POLICY_SRC_REL),
    ]
  } catch {
    // 目录缺失：白名单循环会逐条报错并 fail loud
  }
  for (const f of allFiles) {
    const source = readFileSync(f.abs, 'utf8')
    for (const hit of scanRustSource(source, GLOBAL_MODEL_PATH_PATTERN)) {
      problems.push(
        `✗ 全局模型路径残留：${f.rel}:${hit.line}（${hit.match}）\n` +
          `    ${hit.text}\n` +
          `    全局模型目录已随 ADR-0059 模型域化消亡（T7 / #424），` +
          `模型类型一律走域路径显式 import（如 crate::transaction::model::Transaction）` +
          `——防扁平命名空间复活`,
      )
    }
    for (const hit of scanRustSource(source, MODEL_GLOB_REEXPORT_PATTERN)) {
      problems.push(
        `✗ 域模型 glob 再导出：${f.rel}:${hit.line}（${hit.match}）\n` +
          `    ${hit.text}\n` +
          `    域 model 只许逐类型再导出，所有权必须逐类型可见 ` +
          `（ADR-0059 决策 3/6，#424）：改为 pub use model::{TypeA, TypeB} 形态`,
      )
    }
    if (isModelFile(f.rel)) {
      for (const hit of scanRustSource(source, MODEL_FILE_GLOB_PATTERN)) {
        problems.push(
          `✗ 域模型文件内 glob 聚合：${f.rel}:${hit.line}（${hit.match}）\n` +
            `    ${hit.text}\n` +
            `    域模型文件只承载本域类型定义与逐类型再导出，禁止 glob 聚合 ` +
            `（ADR-0059 决策 3/6，#424）`,
        )
      }
    }
    for (const hit of scanRustSource(source, NATIVE_TX_STMT_PATTERN, true)) {
      if (f.rel === NATIVE_TX_STMT_ALLOWED) continue
      problems.push(
        `✗ 原生事务语句：${f.rel}:${hit.line}（${hit.match}）\n` +
          `    ${hit.text}\n` +
          `    事务壳归基础设施 db::tx_scope（无条件自持 hold_transaction / ` +
          `嵌套感知 ensure_transaction，ADR-0056 / ADR-0105；#1013/#1014）——` +
          `产品代码不得手写 BEGIN/COMMIT/ROLLBACK，唯一合法住址 ${NATIVE_TX_STMT_ALLOWED}`,
      )
    }
  }

  // 模块清单核对：域目录相对根 src 扫描（壳层反向依赖 + 业务域→同步域严形态），
  // 基础设施模块相对 `crates/infra/src` 扫描（壳层反向依赖 + 基础设施→域认许边）——
  // 自 #1088 全量归位起基础设施不再住根 src，路径基准随归位改一次、事实源仍只有
  // 本脚本一份（ADR-0056「白名单即规格」不变）。
  scannedFiles += scanModuleEntries(WHITELIST, srcDir, problems)
  scannedFiles += scanModuleEntries(INFRA_MODULES, join(srcTauriDir, INFRA_SRC_REL), problems)
  scannedFiles += scanModuleEntries(PROTOCOL_MODULES, join(srcTauriDir, PROTOCOL_SRC_REL), problems)
  scannedFiles += scanModuleEntries(BACKUP_MODULES, join(srcTauriDir, BACKUP_SRC_REL), problems)
  scannedFiles += scanModuleEntries(TRANSACTION_MODULES, join(srcTauriDir, TRANSACTION_SRC_REL), problems)
  scannedFiles += scanModuleEntries(ACCOUNTS_MODULES, join(srcTauriDir, ACCOUNTS_SRC_REL), problems)
  scannedFiles += scanModuleEntries(CATEGORIES_MODULES, join(srcTauriDir, CATEGORIES_SRC_REL), problems)
  scannedFiles += scanModuleEntries(MERCHANTS_MODULES, join(srcTauriDir, MERCHANTS_SRC_REL), problems)
  scannedFiles += scanModuleEntries(CURRENCIES_MODULES, join(srcTauriDir, CURRENCIES_SRC_REL), problems)
  scannedFiles += scanModuleEntries(POLICY_MODULES, join(srcTauriDir, POLICY_SRC_REL), problems)

  if (scannedFiles === 0) {
    problems.push('✗ 全部白名单条目扫不到任何非测试 Rust 文件——src 目录指错或白名单整体漂移，拒绝以空集假绿通过')
  }

  // crate 边界核对（spec #1086 / issue #1087）：成员登记、门禁继承、依赖方向、
  // 静态检查/测试命令的 workspace 覆盖——与模块路径白名单并列，同为删除即变红。
  problems.push(...checkCrateBoundaries(srcTauriDir))

  // INFRA_MODULES 与实际模块双向全等（ADR-0111 决策 5 / #1134）：磁盘侧反向
  // 核对（磁盘模块未登记即红）；登记路径消失 / 扫不到非测试文件已由清单循环红。
  problems.push(...checkInfraModuleEquality(srcTauriDir))

  // TRANSACTION_MODULES 与实际模块双向全等 + 区级层序（ADR-0113 决策 7 / #1181）：
  // 磁盘侧反向核对（磁盘模块未登记即红）；区归属据清单条目 zone 字段判向，
  // 认许边（ADR-0113 决策 3 原形状反边）之外即红。
  problems.push(...checkTransactionModuleEquality(srcTauriDir))
  problems.push(...checkTransactionZoneDirection(srcTauriDir))

  if (problems.length > 0) {
    for (const p of problems) console.error(p)
    console.error(
      `❌ 结构守门失败：${problems.length} 处问题` +
        `（分层规则：壳 → 域 → 基础设施，域永不依赖壳，白名单即规格，见 ADR-0056）`,
    )
    process.exit(1)
  }
  console.log(
    `✓ 结构守门：白名单 ${WHITELIST.length} 项（域目录 ${domainCount}）` +
      `+ 基础设施模块 ${INFRA_MODULES.length} 项（crate ${INFRA_SRC_REL}）` +
      `+ 协议模块 ${PROTOCOL_MODULES.length} 项（crate ${PROTOCOL_SRC_REL}）` +
      `+ 备份域模块 ${BACKUP_MODULES.length} 项（crate ${BACKUP_SRC_REL}，#1091）` +
      `+ 核心交易域模块 ${TRANSACTION_MODULES.length} 项（crate ${TRANSACTION_SRC_REL}，#1092）` +
      `+ 账户域模块 ${ACCOUNTS_MODULES.length} 项（crate ${ACCOUNTS_SRC_REL}，#1093）` +
      `+ 分类域模块 ${CATEGORIES_MODULES.length} 项（crate ${CATEGORIES_SRC_REL}，#1094）` +
      `+ 商户域模块 ${MERCHANTS_MODULES.length} 项（crate ${MERCHANTS_SRC_REL}，#1096）` +
      `+ 币种域模块 ${CURRENCIES_MODULES.length} 项（crate ${CURRENCIES_SRC_REL}，#1095）` +
      `+ 保单域模块 ${POLICY_MODULES.length} 项（crate ${POLICY_SRC_REL}，#1100）` +
      `· 白名单面非测试文件 ${scannedFiles} 个 · 对壳层零依赖` +
      `· 基础设施→域零未认许引用（认许边 ${INFRA_DOMAIN_ALLOWED_EDGES.length} 条，ADR-0071）` +
      `· 协议 crate→壳层/域目录零引用（共享底座，#1089）` +
      `· 业务域→同步域零容忍零违规（ADR-0101 / #1089 收紧）` +
      `· 域间禁边 ${DOMAIN_PAIR_FORBIDDEN.length} 对零未认许引用（认许边 ${DOMAIN_PAIR_ALLOWED_EDGES.length} 条，#1090 接缝反转）` +
      `· 模型域化禁令全树扫描 ${allFiles.length} 个文件零残留（ADR-0059）` +
      `· 原生事务语句全树扫描 ${allFiles.length} 个文件仅 ${NATIVE_TX_STMT_ALLOWED} 一处（#1014）` +
      `· crate 边界 ${CRATES.length} 个（成员登记 / 门禁继承 / 依赖方向 / workspace 命令覆盖，#1087）` +
      `· INFRA_MODULES 双向全等（磁盘模块全部登记，ADR-0111 决策 5 / #1134）` +
      `· crate 内块间反向依赖零未认许引用（认许边 ${INFRA_BLOCK_ALLOWED_EDGES.length} 条，ADR-0111 决策 4 / #1134）` +
      `· TRANSACTION_MODULES 双向全等（磁盘模块全部登记，ADR-0113 决策 7 / #1181）` +
      `· 交易域区级层序零未认许反向引用（写读 → 接缝 → 共享语义，认许边 ${TRANSACTION_ZONE_ALLOWED_EDGES.length} 条，ADR-0113 决策 3 / #1181）` +
      `· test_utils 生产编译门（cfg 门 + 生产依赖不启用 test-utils，#1132）` +
      `· 投资五节锚点生产编译门（cfg 门，#1185）` +
      `· http 投影 feature 门（axum optional + impl cfg 门 + default 不含 http + 域侧不启用，#1133）`,
  )
}

// 仅直接运行时执行 main；被测试/其他工具 import 时只取导出的扫描函数。
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main()
}
