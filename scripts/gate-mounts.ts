export const CHECK_SH_FILE = "scripts/check.sh";

export const CI_WORKFLOW_FILE = ".github/workflows/build.yml";

export type GateHostFile = typeof CHECK_SH_FILE | typeof CI_WORKFLOW_FILE;

interface GateHostFileMount {
  readonly file: GateHostFile;
  readonly prefix: string;
  readonly where: string;
}

export interface GateMount {
  readonly name: string;
  readonly refs: string;
  readonly hosts: readonly GateHostFileMount[];
}

export const CI_NON_GATE_RUNS: readonly string[] = ["run: pnpm run build"];

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
    refs: "issue #166 起（八项校验逐票增补；⑨ Migration 索引门随 #2199 退役）",
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
    name: "版本元数据一致性检查（check-versions）",
    refs: "issue #1957 / ADR-0022 修订注记",
    hosts: [
      {
        file: CHECK_SH_FILE,
        prefix: "bun scripts/check-versions.ts",
        where: "scripts/check.sh 质量门槛序列",
      },
      {
        file: CI_WORKFLOW_FILE,
        prefix: "run: bun scripts/check-versions.ts",
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
  {
    name: "注释禁令棘轮守门（check-comment-gate）",
    refs: "issue #2202 / ADR-0145",
    hosts: [
      {
        file: CHECK_SH_FILE,
        prefix: "bun scripts/check-comment-gate.ts",
        where: "scripts/check.sh 质量门槛序列",
      },
      {
        file: CI_WORKFLOW_FILE,
        prefix: "run: bun scripts/check-comment-gate.ts",
        where: ".github/workflows/build.yml frontend job",
      },
    ],
  },
  {
    name: "守门脚本测试（bun test scripts/）",
    refs: "issue #1991 / ADR-0083",
    hosts: [
      {
        file: CHECK_SH_FILE,
        prefix: "bun test scripts/",
        where: "scripts/check.sh 质量门槛序列",
      },
      {
        file: CI_WORKFLOW_FILE,
        prefix: "run: bun test scripts/",
        where: ".github/workflows/build.yml frontend job",
      },
    ],
  },
];
