// 守门挂载登记（issue #1682 spec）：质量门挂载声明的单一事实源——「哪些门被质量
// 门槛跑到、挂哪」的唯一查询点。此前该事实由三本互不核对的登记簿维持（check.sh
// 手写行、CI workflow 手写步骤、各门头注释的「挂载于」声明），删任一行零测试变红；
// 本模块把挂载收成家族共用的 interface，接线（wiring）成为登记条目的一环。
//
// 纯数据 + 类型，无运行时副作用、无 CLI（grilling 定案：查询直接读导出常量，
// 不加 --list 报告命令，YAGNI）。核对，不生成：check.sh 与 CI workflow 保持手写，
// 守门接线测试（gate-mounts.test.ts）遍历本登记断言双向全等——删任一宿主挂载行
// 或删任一登记条目至少一条断言变红（删除即变红，ADR-0087 断言强度）。
//
// 判定语义 = per-host 行首前缀：登记条目按宿主声明 {file, prefix, where}，宿主
// 文件的非注释行 trim 后行首匹配该前缀。echo 展示行行首是 `echo "▶`，永不可能
// 命中执行形态——echo 假绿免疫是判定语义的结构后果（#1112 第三轮审查实测教训）。
// 同一门在两宿主的命令文本实差（`( cd src-tauri && …)` 包裹 vs `working-directory`
// / 裸命令、`-- --check` vs `--check`）由 per-host prefix 显式表达。
//
// 新增守门的登记动作（story 5）：在 GATE_MOUNTS 追加一条——唯一名称、issue/ADR
// 出处（导航字段）、按宿主的执行行形态；check.sh 执行行与 CI 步骤同步挂载，
// 接线测试自动核对三侧全等。门的核心判定规则归各门自身，本登记只管挂载。

/** 宿主文件：本地质量门槛序列（唯一权威的全量执行面）。 */
export const CHECK_SH_FILE = "scripts/check.sh";

/** 宿主文件：CI workflow（挂载子集，按 job 分布）。 */
export const CI_WORKFLOW_FILE = ".github/workflows/build.yml";

/** 宿主标识：登记只认这两份宿主文件，挂到清单外的宿主是类型错误。 */
export type GateHostFile = typeof CHECK_SH_FILE | typeof CI_WORKFLOW_FILE;

/** 按宿主的挂载声明。 */
export interface GateHostFileMount {
  /** 宿主文件（相对仓库根） */
  readonly file: GateHostFile;
  /** 执行行形态：宿主非注释行 trim 后行首前缀匹配 */
  readonly prefix: string;
  /** 宿主内定位说明（job / 序列，导航用） */
  readonly where: string;
}

/** 一条质量门的挂载登记。 */
export interface GateMount {
  /** 唯一名称（= 门的展示名，全表唯一） */
  readonly name: string;
  /** issue/ADR 出处（导航字段）：门本体的权威出处，供 all-entries 核对清单一次生成 */
  readonly refs: string;
  /** 按宿主的挂载声明：check.sh 恒有一条；CI 挂载仅在该门实际入 CI 时声明 */
  readonly hosts: readonly GateHostFileMount[];
}

/** CI 挂门 job 内的非门步骤豁免（政策显式，#1591：政策无源就显式）：
 *  构建步骤 `pnpm run build` 内含前端类型检查（package.json build 脚本 =
 *  `vue-tsc --noEmit && vite build`），类型检查由该脚本承接、CI 无独立步骤，
 *  故「前端类型检查」条目只声明 check.sh 宿主；本行随之不属门挂载面。 */
export const CI_NON_GATE_RUNS: readonly string[] = ["run: pnpm run build"];

/**
 * 登记清单（check.sh 全部质量门步骤：bun TS 门与非 TS 步骤同表；check-docs.sh 作
 * 单条，其内部九项校验不展开。spec #1682 定案口径 22 条 = 13 + 9——实际集合以
 * 守门接线测试与 check.sh / CI 的双向全等断言为准，本注释不作计数依据）。
 * 顺序与 check.sh 执行序列一致，便于逐行核对；CI 挂载为子集（frontend job /
 * backend-lint job）。
 */
export const GATE_MOUNTS: readonly GateMount[] = [
  {
    name: "前端类型检查（vue-tsc）",
    refs: "仓库既有门槛（早于 issue 化，无单票出处）",
    hosts: [
      {
        file: CHECK_SH_FILE,
        prefix: "pnpm exec vue-tsc --noEmit",
        where: "scripts/check.sh 质量门槛序列",
      },
    ],
  },
  {
    name: "守门脚本与测试类型检查（vue-tsc -p tsconfig.scripts.json）",
    refs: "issue #734 / #740 / ADR-0083",
    hosts: [
      {
        file: CHECK_SH_FILE,
        prefix: "pnpm exec vue-tsc -p tsconfig.scripts.json --noEmit",
        where: "scripts/check.sh 质量门槛序列",
      },
      {
        file: CI_WORKFLOW_FILE,
        prefix: "run: pnpm exec vue-tsc -p tsconfig.scripts.json --noEmit",
        where: ".github/workflows/build.yml frontend job",
      },
    ],
  },
  {
    name: "前端 lint（oxlint）",
    refs: "issue #743",
    hosts: [
      {
        file: CHECK_SH_FILE,
        prefix: "pnpm exec oxlint --deny-warnings",
        where: "scripts/check.sh 质量门槛序列",
      },
      {
        file: CI_WORKFLOW_FILE,
        prefix: "run: pnpm exec oxlint --deny-warnings",
        where: ".github/workflows/build.yml frontend job",
      },
    ],
  },
  {
    name: "前端格式检查（oxfmt）",
    refs: "issue #1519 / ADR-0128",
    hosts: [
      {
        file: CHECK_SH_FILE,
        prefix: "pnpm exec oxfmt --check .",
        where: "scripts/check.sh 质量门槛序列",
      },
      {
        file: CI_WORKFLOW_FILE,
        prefix: "run: pnpm exec oxfmt --check .",
        where: ".github/workflows/build.yml frontend job",
      },
    ],
  },
  {
    name: "Rust clippy",
    refs: "issue #1087（--workspace 范围；门槛本体早于 issue 化）",
    hosts: [
      {
        file: CHECK_SH_FILE,
        prefix:
          "( cd src-tauri && cargo clippy --workspace --all-targets --all-features -- -D warnings )",
        where: "scripts/check.sh 质量门槛序列",
      },
      {
        file: CI_WORKFLOW_FILE,
        prefix: "run: cargo clippy --workspace --all-targets --all-features -- -D warnings",
        where: ".github/workflows/build.yml backend-lint job",
      },
    ],
  },
  {
    name: "Rust gate-off 编译检查（ledger-infra 默认 feature）",
    refs: "issue #1133 / #1473",
    hosts: [
      {
        file: CHECK_SH_FILE,
        prefix: "( cd src-tauri && cargo check -p ledger-infra )",
        where: "scripts/check.sh 质量门槛序列",
      },
      {
        file: CI_WORKFLOW_FILE,
        prefix: "run: cargo check -p ledger-infra",
        where: ".github/workflows/build.yml backend-lint job",
      },
    ],
  },
  {
    name: "Rust 格式检查（cargo fmt）",
    refs: "issue #1087（--all 范围；门槛本体早于 issue 化）",
    hosts: [
      {
        file: CHECK_SH_FILE,
        prefix: "( cd src-tauri && cargo fmt --all -- --check )",
        where: "scripts/check.sh 质量门槛序列",
      },
      {
        file: CI_WORKFLOW_FILE,
        prefix: "run: cargo fmt --all --check",
        where: ".github/workflows/build.yml backend-lint job",
      },
    ],
  },
  {
    name: "基础设施 rustdoc 门禁（cargo doc -D warnings）",
    refs: "issue #1139",
    hosts: [
      {
        file: CHECK_SH_FILE,
        prefix:
          "( cd src-tauri && RUSTDOCFLAGS='-D warnings' cargo doc -p ledger-infra --no-deps )",
        where: "scripts/check.sh 质量门槛序列",
      },
      {
        file: CI_WORKFLOW_FILE,
        prefix: "run: RUSTDOCFLAGS='-D warnings' cargo doc -p ledger-infra --no-deps",
        where: ".github/workflows/build.yml backend-lint job",
      },
    ],
  },
  {
    name: "文档一致性检查（check-docs.sh）",
    refs: "issue #166 起（九项校验逐票增补）",
    hosts: [
      {
        file: CHECK_SH_FILE,
        prefix: "./scripts/check-docs.sh",
        where: "scripts/check.sh 质量门槛序列",
      },
      {
        file: CI_WORKFLOW_FILE,
        prefix: "run: ./scripts/check-docs.sh",
        where: ".github/workflows/build.yml frontend job",
      },
    ],
  },
  {
    name: "命令注册一致性检查（check-commands）",
    refs: "issue #315 / ADR-0047 / #1398",
    hosts: [
      {
        file: CHECK_SH_FILE,
        prefix: "bun scripts/check-commands.ts",
        where: "scripts/check.sh 质量门槛序列",
      },
      {
        file: CI_WORKFLOW_FILE,
        prefix: "run: bun scripts/check-commands.ts",
        where: ".github/workflows/build.yml frontend job",
      },
    ],
  },
  {
    name: "结构守门检查（check-structure）",
    refs: "spec #1086 / ADR-0056",
    hosts: [
      {
        file: CHECK_SH_FILE,
        prefix: "bun scripts/check-structure.ts",
        where: "scripts/check.sh 质量门槛序列",
      },
      {
        file: CI_WORKFLOW_FILE,
        prefix: "run: bun scripts/check-structure.ts",
        where: ".github/workflows/build.yml frontend job",
      },
    ],
  },
  {
    name: "前端 workspace 结构守门（check-frontend-structure）",
    refs: "issue #1149 / spec #1148",
    hosts: [
      {
        file: CHECK_SH_FILE,
        prefix: "bun scripts/check-frontend-structure.ts",
        where: "scripts/check.sh 质量门槛序列",
      },
      {
        file: CI_WORKFLOW_FILE,
        prefix: "run: bun scripts/check-frontend-structure.ts",
        where: ".github/workflows/build.yml frontend job",
      },
    ],
  },
  {
    name: "基础设施账本数据表 DML 禁令（check-infra-dml）",
    refs: "issue #1135 / ADR-0111",
    hosts: [
      {
        file: CHECK_SH_FILE,
        prefix: "bun scripts/check-infra-dml.ts",
        where: "scripts/check.sh 质量门槛序列",
      },
      {
        file: CI_WORKFLOW_FILE,
        prefix: "run: bun scripts/check-infra-dml.ts",
        where: ".github/workflows/build.yml frontend job（issue #1682 CI 补齐）",
      },
    ],
  },
  {
    name: "东财行情面端点零残留源码扫描（check-eastmoney-residue）",
    refs: "issue #1572 / ADR-0130 守门判据①",
    hosts: [
      {
        file: CHECK_SH_FILE,
        prefix: "bun scripts/check-eastmoney-residue.ts",
        where: "scripts/check.sh 质量门槛序列",
      },
      {
        file: CI_WORKFLOW_FILE,
        prefix: "run: bun scripts/check-eastmoney-residue.ts",
        where: ".github/workflows/build.yml frontend job（issue #1682 CI 补齐）",
      },
    ],
  },
  {
    name: "后台服务成对拉起守门（check-background-services）",
    refs: "issue #961",
    hosts: [
      {
        file: CHECK_SH_FILE,
        prefix: "bun scripts/check-background-services.ts",
        where: "scripts/check.sh 质量门槛序列",
      },
      {
        file: CI_WORKFLOW_FILE,
        prefix: "run: bun scripts/check-background-services.ts",
        where: ".github/workflows/build.yml frontend job",
      },
    ],
  },
  {
    name: "样式块守门（check-style-blocks）",
    refs: "issue #888 / ADR-0093",
    hosts: [
      {
        file: CHECK_SH_FILE,
        prefix: "bun scripts/check-style-blocks.ts",
        where: "scripts/check.sh 质量门槛序列",
      },
      {
        file: CI_WORKFLOW_FILE,
        prefix: "run: bun scripts/check-style-blocks.ts",
        where: ".github/workflows/build.yml frontend job（issue #1682 CI 补齐）",
      },
    ],
  },
  {
    name: "i18n key 全等检查（check-i18n-keys）",
    refs: "issue #342 / ADR-0049 / #1188",
    hosts: [
      {
        file: CHECK_SH_FILE,
        prefix: "bun scripts/check-i18n-keys.ts",
        where: "scripts/check.sh 质量门槛序列",
      },
      {
        file: CI_WORKFLOW_FILE,
        prefix: "run: bun scripts/check-i18n-keys.ts",
        where: ".github/workflows/build.yml frontend job（issue #1682 CI 补齐）",
      },
    ],
  },
  {
    name: "弹窗表单节奏守门（check-dialog-forms）",
    refs: "issue #804 / ADR-0079 决策 4",
    hosts: [
      {
        file: CHECK_SH_FILE,
        prefix: "bun scripts/check-dialog-forms.ts",
        where: "scripts/check.sh 质量门槛序列",
      },
      {
        file: CI_WORKFLOW_FILE,
        prefix: "run: bun scripts/check-dialog-forms.ts",
        where: ".github/workflows/build.yml frontend job（issue #1682 CI 补齐）",
      },
    ],
  },
  {
    name: "测试桩守门（check-test-stubs）",
    refs: "issue #725 / #750 / #822",
    hosts: [
      {
        file: CHECK_SH_FILE,
        prefix: "bun scripts/check-test-stubs.ts",
        where: "scripts/check.sh 质量门槛序列",
      },
      {
        file: CI_WORKFLOW_FILE,
        prefix: "run: bun scripts/check-test-stubs.ts",
        where: ".github/workflows/build.yml frontend job（issue #1682 CI 补齐）",
      },
    ],
  },
  {
    name: "Rust 测试守门（check-test-support）",
    refs: "issue #752 / #758 / ADR-0084",
    hosts: [
      {
        file: CHECK_SH_FILE,
        prefix: "bun scripts/check-test-support.ts",
        where: "scripts/check.sh 质量门槛序列",
      },
      {
        file: CI_WORKFLOW_FILE,
        prefix: "run: bun scripts/check-test-support.ts",
        where: ".github/workflows/build.yml frontend job（issue #1682 CI 补齐）",
      },
    ],
  },
  {
    name: "测试执行覆盖守门（test-exec check）",
    refs: "issue #1112 / spec #1086",
    hosts: [
      {
        file: CHECK_SH_FILE,
        prefix: "bun scripts/test-exec.ts check",
        where: "scripts/check.sh 质量门槛序列",
      },
      {
        file: CI_WORKFLOW_FILE,
        prefix: "run: bun scripts/test-exec.ts check",
        where: ".github/workflows/build.yml frontend job",
      },
    ],
  },
  {
    name: "前端异步守门（check-async-guards）",
    refs: "issue #1039 / #1008 决议 4",
    hosts: [
      {
        file: CHECK_SH_FILE,
        prefix: "bun scripts/check-async-guards.ts",
        where: "scripts/check.sh 质量门槛序列",
      },
      {
        file: CI_WORKFLOW_FILE,
        prefix: "run: bun scripts/check-async-guards.ts",
        where: ".github/workflows/build.yml frontend job",
      },
    ],
  },
];
