import { afterAll, describe, expect, it } from "vitest";
import { mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  CRATE_MODULE_TARGETS,
  CRATES,
  INFRA_SRC_REL,
  LAYER,
  TRANSACTION_ZONE_ALLOWED_EDGES,
  TRANSACTION_ZONE_BY_DIR,
  WHITELIST,
  lineAt,
  maskNonCode,
  walkTextFiles,
} from "../scripts/check-structure.ts";
import { gateScript, runGateScript } from "./run-gate-script.test-helper.ts";

// 被测对象是仓库工具脚本 scripts/check-structure.ts（结构守门，ADR-0056）。
// 脚本以 Bun 运行时执行（ADR-0083）：runGateScript 以 spawnSync('bun') 与门槛
// 调用同款拉起，测的就是门槛路径。
// 按测试决策守门行为只测外部可观察结果——进程退出码与输出，不测内部函数；
// 通过位置参数把扫描目标指向临时夹具目录。
// 例外（直测导出共享面，同址先例）：maskNonCode 双源防漂移语料（#1433）与
// 家族共享扫描单点 lineAt / walkTextFiles（#1625）——它们本身就是供多脚本
// 直接消费的导出面，单测对准可失败断言。
// 夹具自足化（#1595 / T2-3）：每个 crate 的模块面由用例显式声明
// `{crate, dir, libRs, files}`——合成 lib.rs 文本 + 合成磁盘布局一次生成，
// 不再有「相对路径 → 目标 crate」的路由链，也不 import 已退役的手写模块清单。
const script = gateScript("check-structure.ts");
const run = (args: readonly string[]) => runGateScript(script, args);

const tempDirs: string[] = [];
afterAll(() => {
  for (const dir of tempDirs) rmSync(dir, { recursive: true, force: true });
});

/** 夹具桩内容（无壳层依赖的最小 Rust 文件） */
const STUB = "// 结构守门夹具桩\npub fn stub() {}\n";

/** 夹具用现存的壳层引用形态（商户壳层命令）：参考数据三域 #404 归位后账户壳层已无
 *  `*_internal` 下沉函数，夹具文本取现存壳层命令与实际结构保持一致。 */
const shellUse = "use crate::commands::merchants::list_merchants;\npub fn x() {}\n";

/**
 * 用例夹具的显式 crate 模块面声明（#1595 T2-3）：`libRs` 是 crate 根 `lib.rs` 的
 * 声明文本，`files` 是相对 crate `src` 的模块文件与内容——合成声明面与合成磁盘
 * 布局由此一次生成，测试不依赖守门脚本的清单常量。
 */
interface CrateCase {
  /** crate 名（与 CRATES 政策核成员一致；报文定位用） */
  crate: string;
  /** crate 目录（相对 src-tauri） */
  dir: string;
  /** crate 根 lib.rs 内容（mod 声明面；空串=无模块的声明面） */
  libRs: string;
  /** 模块文件：相对 crate src 的路径 → 内容 */
  files: Record<string, string>;
}

/** infra crate 根 lib.rs 的 test_utils「放行测试」cfg 门（同真实仓库，ADR-0111 决策 5）。 */
const INFRA_TEST_UTILS_GATE = '#[cfg(any(test, feature = "test-utils"))]\n#[doc(hidden)]\n';

/** 基础设施 crate 的骨架形状（与真实仓库同形：db / error / test_utils 生产编译门）。 */
const INFRA_CASE: CrateCase = {
  crate: "ledger-infra",
  dir: "crates/infra",
  libRs:
    "pub mod boot;\npub mod db;\npub mod error;\n" +
    INFRA_TEST_UTILS_GATE +
    "pub mod test_utils;\n",
  files: {
    "boot/mod.rs": STUB,
    "db/mod.rs": STUB,
    "error.rs":
      "pub struct AppError;\n" +
      '#[cfg(feature = "http")]\n' +
      "impl axum::response::IntoResponse for AppError {\n" +
      "    fn into_response(self) -> axum::response::Response {}\n" +
      "}\n",
    "test_utils.rs": STUB,
  },
};

/** 核心交易域 crate 的骨架形状（四区目录与区归属政策表一致，供区级层序用例消费）。 */
const TRANSACTION_CASE: CrateCase = {
  crate: "ledger-transaction",
  dir: "crates/transaction",
  libRs:
    "pub mod amount;\npub mod command;\npub mod model;\npub mod read;\n" +
    "pub mod seams;\npub mod shared;\npub mod write;\n",
  files: {
    "amount/mod.rs": STUB,
    "command/mod.rs": STUB,
    "model/mod.rs": STUB,
    "read/mod.rs": STUB,
    "seams/mod.rs": STUB,
    "shared/mod.rs": "pub mod search_text;\n",
    "shared/search_text.rs": STUB,
    "write/mod.rs": STUB,
  },
};

/** 同步协议 crate 的骨架形状（协议面模块，供协议分层用例消费）。 */
const PROTOCOL_CASE: CrateCase = {
  crate: "ledger-sync-protocol",
  dir: "crates/sync-protocol",
  libRs: "pub mod op;\n",
  files: { "op.rs": STUB },
};

/** 骨架默认模块面：每 crate 一枚 core 模块；特定形状的 crate 用常量覆盖。 */
function defaultCase(crate: string, dir: string): CrateCase {
  return { crate, dir, libRs: "pub mod core;\n", files: { "core.rs": STUB } };
}

/** 骨架里形状特殊的 crate（键 = crate 名；其余成员用 defaultCase 的默认形状）。 */
const SPECIAL_CASES = new Map<string, CrateCase>([
  [INFRA_CASE.crate, INFRA_CASE],
  [TRANSACTION_CASE.crate, TRANSACTION_CASE],
  [PROTOCOL_CASE.crate, PROTOCOL_CASE],
]);

/** 骨架成员 crate 的模块面（显式声明；模块名不必与真实仓库相同，机制面一致即可）。 */
const SKELETON_CASES: readonly CrateCase[] = CRATES.filter((crate) => crate.dir !== ".").map(
  (crate) => SPECIAL_CASES.get(crate.name) ?? defaultCase(crate.name, crate.dir),
);

/** 账户域 crate 的模块面声明（域侧扫描用例的靶；`core.rs` 可被覆盖为违规内容）。 */
function accountsCase(files: Record<string, string> = {}, libRs = "pub mod core;\n"): CrateCase {
  return {
    crate: "ledger-accounts",
    dir: "crates/accounts",
    libRs,
    files: { "core.rs": STUB, ...files },
  };
}

/** 基础设施 crate 的模块面声明（在骨架形状上覆盖文件/声明面，供基础层用例消费）。 */
function infraCase(files: Record<string, string> = {}, libRs?: string): CrateCase {
  return {
    ...INFRA_CASE,
    libRs: libRs ?? INFRA_CASE.libRs,
    files: { ...INFRA_CASE.files, ...files },
  };
}

/** 交易域 crate 的模块面声明（在骨架形状上覆盖文件，供区级层序用例消费）。 */
function transactionCase(files: Record<string, string> = {}, libRs?: string): CrateCase {
  return {
    ...TRANSACTION_CASE,
    libRs: libRs ?? TRANSACTION_CASE.libRs,
    files: { ...TRANSACTION_CASE.files, ...files },
  };
}

/** 同步协议 crate 的模块面声明（协议分层用例的靶）。 */
function protocolCase(files: Record<string, string> = {}): CrateCase {
  return { ...PROTOCOL_CASE, files: { ...PROTOCOL_CASE.files, ...files } };
}

/** workspace 骨架夹具的可覆盖面（缺省为一份全绿的骨架）。 */
interface CrateFixtureOverrides {
  rootManifest?: string;
  /** 覆盖成员 crate 的 Cargo.toml：crate 目录（相对 src-tauri）→ 清单文本 */
  memberManifests?: Record<string, string>;
  /** 覆盖 `crates/infra/src/lib.rs` 内容（test_utils cfg 门负向夹具） */
  infraLibRs?: string;
  /** 覆盖 `crates/infra/src/error.rs` 内容（http 投影 impl cfg 门负向夹具） */
  infraErrorRs?: string;
  /** 覆盖 `src/api_server/handlers/import.rs` 内容（投资五节锚点 cfg 门负向夹具，#1185） */
  apiServerImportRs?: string;
  /** 覆盖 `src/api_server/mod.rs` 内容（投资五节锚点再导出 cfg 门负向夹具，#1185） */
  apiServerModRs?: string;
  /** 覆盖根包 `[dependencies]` 的 ledger-infra 行（生产依赖接线负向夹具） */
  rootInfraProdDep?: string;
  /** 追加到根包 `[features]` 段的原文行（default feature 负向夹具） */
  rootFeaturesExtra?: string;
  checkSh?: string;
  testSh?: string;
  lintFixSh?: string;
  /** 覆盖 `scripts/test-exec.ts`（cargo 命令宿主，workspace 范围负向夹具，#1112） */
  testExecTs?: string;
  workflow?: string;
  /** 追加一个未登记的成员目录（crates/<name>）——新 crate 漏登记的负向夹具 */
  orphanCrate?: string;
  /** 覆盖用例声明的 crate 模块面（{crate, dir, libRs, files}，#1595 T2-3） */
  moduleCases?: readonly CrateCase[];
}

/** 六件套 deny 声明（workspace 级唯一声明处，ADR-0060） */
const LINT_DENIES = [
  'unwrap_used = "deny"',
  'expect_used = "deny"',
  'panic = "deny"',
  'todo = "deny"',
  'unimplemented = "deny"',
  'unreachable = "deny"',
].join("\n");

/** 成员 crate 清单骨架：门禁继承 + dev-dependency 测试环（与真实仓库同形）。 */
function memberManifest(name: string): string {
  return [
    "[package]",
    `name = "${name}"`,
    'version = "0.6.0"',
    'edition = "2024"',
    "",
    "[dev-dependencies]",
    'tauri-app = { path = "../.." }',
    "",
    "[lints]",
    "workspace = true",
    "",
  ].join("\n");
}

/** infra 清单骨架：http 投影门宿主（axum optional + `http = ["dep:axum"]`，default 不含 http）。 */
const INFRA_MANIFEST = [
  "[package]",
  'name = "ledger-infra"',
  'version = "0.6.0"',
  'edition = "2024"',
  "",
  "[features]",
  "test-utils = []",
  'http = ["dep:axum"]',
  "",
  "[dependencies]",
  'axum = { version = "0.8", optional = true }',
  "",
  "[lints]",
  "workspace = true",
  "",
].join("\n");

/** 写夹具文件（相对 crate src）：自动建档目录。 */
function writeCrateFile(srcTauri: string, dir: string, rel: string, content: string): void {
  const abs = join(srcTauri, dir, "src", rel);
  mkdirSync(join(abs, ".."), { recursive: true });
  writeFileSync(abs, content);
}

/**
 * 建 workspace 骨架夹具：`<root>/src-tauri/{Cargo.toml,src,crates/*}` + 仓库根门槛
 * 宿主（scripts/check.sh、scripts/test.sh、.github/workflows/build.yml）。成员清单
 * 自 `CRATES` 政策核（成员登记核对的唯一对象），模块面自用例声明的 `{crate, dir,
 * libRs, files}`（#1595 T2-3）。返回脚本参数 `[src-dir, src-tauri-dir]`。
 */
function makeCrateFixture(overrides: CrateFixtureOverrides = {}): string[] {
  const root = mkdtempSync(join(tmpdir(), "check-structure-crate-"));
  tempDirs.push(root);
  const srcTauri = join(root, "src-tauri");
  mkdirSync(srcTauri, { recursive: true });

  const rootManifest =
    overrides.rootManifest ??
    [
      "[package]",
      'name = "tauri-app"',
      'version = "0.6.0"',
      'edition = "2024"',
      "",
      "[features]",
      'test-utils = ["ledger-infra/test-utils"]',
      overrides.rootFeaturesExtra ?? "",
      "",
      "[dependencies]",
      overrides.rootInfraProdDep ?? 'ledger-infra = { path = "crates/infra" }',
      "",
      "[workspace]",
      'members = ["crates/*"]',
      'resolver = "3"',
      "",
      "[workspace.lints.clippy]",
      LINT_DENIES,
      "",
      "[dev-dependencies]",
      'tauri-app = { path = ".", features = ["test-utils"] }',
      "",
      "[lints]",
      "workspace = true",
      "",
    ].join("\n");
  writeFileSync(join(srcTauri, "Cargo.toml"), rootManifest);

  const cases = new Map<string, CrateCase>();
  for (const c of SKELETON_CASES) cases.set(c.dir, c);
  for (const c of overrides.moduleCases ?? []) {
    // 夹具自足化要求用例显式声明成员 crate：声明与政策核不符即夹具自身出错（否则
    // 错字会让用例静默落在骨架默认形状上、变绿变假）。
    const member = CRATES.find((crate) => crate.dir === c.dir);
    if (member === undefined || member.name !== c.crate) {
      throw new Error(`夹具声明与 CRATES 政策核不一致：${c.crate} / ${c.dir}`);
    }
    cases.set(c.dir, c);
  }
  for (const crate of CRATES) {
    if (crate.dir === ".") continue;
    const c = cases.get(crate.dir) ?? defaultCase(crate.name, crate.dir);
    mkdirSync(join(srcTauri, crate.dir, "src"), { recursive: true });
    const manifest =
      overrides.memberManifests?.[crate.dir] ??
      (crate.name === "ledger-infra" ? INFRA_MANIFEST : memberManifest(crate.name));
    writeFileSync(join(srcTauri, crate.dir, "Cargo.toml"), manifest);
    writeFileSync(join(srcTauri, crate.dir, "src", "lib.rs"), c.libRs);
    for (const [rel, content] of Object.entries(c.files)) {
      writeCrateFile(srcTauri, crate.dir, rel, content);
    }
  }

  if (overrides.infraLibRs !== undefined) {
    writeFileSync(join(srcTauri, INFRA_SRC_REL, "lib.rs"), overrides.infraLibRs);
  }
  if (overrides.infraErrorRs !== undefined) {
    writeFileSync(join(srcTauri, INFRA_SRC_REL, "error.rs"), overrides.infraErrorRs);
  }

  // 根包壳层面：lib.rs、白名单政策项（test_support）与投资五节锚点（#1185）。
  mkdirSync(join(srcTauri, "src", "test_support"), { recursive: true });
  writeFileSync(join(srcTauri, "src", "lib.rs"), "pub fn stub() {}\n");
  writeFileSync(join(srcTauri, "src", "test_support", "mod.rs"), STUB);
  mkdirSync(join(srcTauri, "src", "api_server", "handlers"), { recursive: true });
  writeFileSync(
    join(srcTauri, "src", "api_server", "handlers", "import.rs"),
    overrides.apiServerImportRs ??
      '#[cfg(any(test, feature = "test-utils"))]\n' +
        "#[doc(hidden)]\n" +
        'pub const INVESTMENT_SECTION_HEADERS: [&str; 5] = ["## 节"];\n',
  );
  writeFileSync(
    join(srcTauri, "src", "api_server", "mod.rs"),
    overrides.apiServerModRs ??
      '#[cfg(any(test, feature = "test-utils"))]\n' +
        "#[doc(hidden)]\n" +
        "pub use handlers::import::INVESTMENT_SECTION_HEADERS;\n",
  );

  if (overrides.orphanCrate !== undefined) {
    mkdirSync(join(srcTauri, "crates", overrides.orphanCrate, "src"), { recursive: true });
    writeFileSync(
      join(srcTauri, "crates", overrides.orphanCrate, "Cargo.toml"),
      '[package]\nname = "orphan"\nversion = "0.1.0"\nedition = "2024"\n',
    );
    writeFileSync(
      join(srcTauri, "crates", overrides.orphanCrate, "src", "lib.rs"),
      "pub fn stub() {}\n",
    );
  }

  mkdirSync(join(root, "scripts"), { recursive: true });
  mkdirSync(join(root, ".github", "workflows"), { recursive: true });
  writeFileSync(
    join(root, "scripts", "check.sh"),
    overrides.checkSh ??
      "( cd src-tauri && cargo clippy --workspace --all-targets --all-features -- -D warnings )\n" +
        "( cd src-tauri && cargo fmt --all -- --check )\n",
  );
  writeFileSync(
    join(root, "scripts", "test.sh"),
    overrides.testSh ?? "( cd src-tauri && cargo test --workspace )\n",
  );
  writeFileSync(
    join(root, "scripts", "lint-fix.sh"),
    overrides.lintFixSh ??
      "( cd src-tauri && cargo fmt --all && cargo clippy --fix --workspace --all-targets --all-features --allow-dirty --allow-staged )\n",
  );
  writeFileSync(
    join(root, "scripts", "test-exec.ts"),
    overrides.testExecTs ??
      "// 构建一次（口径说明：`cargo test` 默认执行面）\n" +
        'const BUILD = "cargo test --workspace --no-run"\n' +
        'runChild(cargo, ["test", "--workspace", "--no-run"], { cwd: BUILD })\n',
  );
  writeFileSync(
    join(root, ".github", "workflows", "build.yml"),
    overrides.workflow ??
      [
        "jobs:",
        "  b:",
        "    steps:",
        '      - run: cargo test --workspace --lib --test "*"',
        "      - run: cargo fmt --all --check",
        "      - run: cargo clippy --workspace --all-targets --all-features -- -D warnings",
        "",
      ].join("\n"),
  );

  return [join(srcTauri, "src"), srcTauri];
}

/** 骨架 + 用例声明的目标 crate 模块面（#1595 T2-3：{crate, dir, libRs, files}）。 */
function fixtureWith(caseSpec: CrateCase, overrides: CrateFixtureOverrides = {}): string[] {
  return makeCrateFixture({
    ...overrides,
    moduleCases: [caseSpec, ...(overrides.moduleCases ?? [])],
  });
}

describe("check-structure（结构守门）", () => {
  it("真实仓库默认通过：模块面由 lib.rs 声明投影 + 白名单对壳层零依赖（T2-3 判据①）", () => {
    // 真实仓库自检绿是唯一能发现「派生器认不出仓库实际形态」的测试。
    const r = run([]);
    expect(r.status).toBe(0);
    expect(r.output).toContain("零依赖");
    expect(r.output).toContain(`白名单 ${WHITELIST.length} 项`);
    expect(r.output).toContain(`crate 模块面 ${CRATE_MODULE_TARGETS.length} 份`);
    const domainCount = WHITELIST.filter((w) => w.layer === LAYER.DOMAIN).length;
    expect(r.output).toContain(`域目录 ${domainCount}`);
    // 投影覆盖全等（防派生器静默丢成员）：每个非根包政策核成员恰有一份模块面。
    const memberCrates = CRATES.filter((crate) => crate.dir !== ".");
    expect(CRATE_MODULE_TARGETS.length).toBe(memberCrates.length);
    expect([...CRATE_MODULE_TARGETS].map((t) => t.srcRel).sort()).toEqual(
      memberCrates.map((crate) => `${crate.dir}/src`).sort(),
    );
  });

  it("夹具骨架全部为干净桩时通过", () => {
    const r = run(makeCrateFixture());
    expect(r.status).toBe(0);
    expect(r.output).toContain("零依赖");
  });

  it("白名单政策项代码引用壳层 → 失败并定位文件行号", () => {
    // 全部业务域 crate 化后，根包仅余测试支持域（test_support）一条政策项。
    const args = makeCrateFixture();
    writeCrateFile(args[1], ".", "test_support/crud.rs", shellUse);
    const r = run(args);
    expect(r.status).toBe(1);
    expect(r.output).toContain("反向依赖");
    expect(r.output).toContain("test_support/crud.rs:1");
  });

  it("注释与字符串中的 commands:: 不误报（掩码边界）", () => {
    const args = makeCrateFixture();
    writeCrateFile(
      args[1],
      "src",
      "test_support/cost.rs",
      [
        "/// 消费 `commands::item` 接缝（文档注释不算依赖）",
        "// 见 commands::foo 说明",
        'let url = "http://127.0.0.1:9527/commands::x";',
        'let re = r#"commands::\\d+"#;',
        "pub fn f<'a>(x: &'a str) -> &str { x }",
        "",
      ].join("\n"),
    );
    expect(run(args).status).toBe(0);
  });

  it("外挂测试模块/目录豁免：tests.rs 与 tests/ 引用壳层不红（ADR-0056 决策 5）", () => {
    const args = makeCrateFixture();
    writeCrateFile(args[1], ".", "test_support/tests.rs", shellUse);
    writeCrateFile(args[1], ".", "test_support/tests/scaffold.rs", shellUse);
    writeCrateFile(args[1], ".", "test_support/helper/tests/fixture.rs", shellUse);
    expect(run(args).status).toBe(0);
  });

  it("别名引入（use … as）同样识别为依赖（根 src 白名单面）", () => {
    // 跨 crate 面（如 ledger-infra 内的 `crate::commands`）自 #1596 起归编译期；
    // 仅存的壳层反向依赖文本扫描面 = 根包 src 白名单（test_support 与 commands
    // 同居根包，属同 crate 引用）。
    const args = makeCrateFixture();
    writeCrateFile(
      args[1],
      ".",
      "test_support/cost.rs",
      "use crate::commands as shell;\npub fn y() {}\n",
    );
    const r = run(args);
    expect(r.status).toBe(1);
    expect(r.output).toContain("commands as");
  });

  it("白名单政策项路径缺失（政策项漂移）→ fail loud", () => {
    const args = makeCrateFixture();
    rmSync(join(args[1], "src", "test_support"), { recursive: true, force: true });
    const r = run(args);
    expect(r.status).toBe(1);
    expect(r.output).toContain("条目路径不存在：test_support");
  });

  it("白名单条目只剩测试豁免文件（扫不到非测试文件）→ fail loud，拒绝假绿", () => {
    const args = makeCrateFixture();
    rmSync(join(args[1], "src", "test_support", "mod.rs"));
    writeCrateFile(args[1], ".", "test_support/tests.rs", shellUse);
    const r = run(args);
    expect(r.status).toBe(1);
    expect(r.output).toContain("扫不到非测试");
  });
});

describe("check-structure 模块面投影核对（#1595：expected 由 lib.rs mod 声明派生）", () => {
  it("孤儿模块文件（磁盘多出未声明模块）→ 红（判据②：删检测即变红）", () => {
    const args = fixtureWith(accountsCase());
    writeCrateFile(args[1], "crates/accounts", "orphan.rs", STUB);
    const r = run(args);
    expect(r.status).toBe(1);
    expect(r.output).toContain("未声明模块");
    expect(r.output).toContain("orphan");
    expect(r.output).toContain("crates/accounts/src");
  });

  it("孤儿模块目录（扫得到非测试文件的目录）→ 红", () => {
    const args = fixtureWith(accountsCase());
    writeCrateFile(args[1], "crates/accounts", "orphan_dir/helper.rs", STUB);
    const r = run(args);
    expect(r.status).toBe(1);
    expect(r.output).toContain("未声明模块");
    expect(r.output).toContain("orphan_dir");
  });

  it("测试豁免形态不触发未声明：顶层 tests.rs 与仅含 tests.rs 的目录（ADR-0056 决策 5）", () => {
    const args = fixtureWith(accountsCase());
    writeCrateFile(args[1], "crates/accounts", "tests.rs", STUB);
    writeCrateFile(args[1], "crates/accounts", "extra/tests.rs", STUB);
    expect(run(args).status).toBe(0);
  });

  it("lib.rs 声明而磁盘缺失 → 红（声明即 rustc 契约）", () => {
    const args = fixtureWith(accountsCase({}, "pub mod core;\npub mod ghost;\n"));
    const r = run(args);
    expect(r.status).toBe(1);
    expect(r.output).toContain("声明缺失");
    expect(r.output).toContain("ghost");
  });

  it("crate 根 lib.rs 缺失 → fail loud（无法派生 expected）", () => {
    const args = fixtureWith(accountsCase());
    rmSync(join(args[1], "crates/accounts/src/lib.rs"));
    const r = run(args);
    expect(r.status).toBe(1);
    expect(r.output).toContain("找不到 crate 根声明文件");
    expect(r.output).toContain("crates/accounts/src/lib.rs");
  });

  it("crate 根 lib.rs 是声明与再导出面：再导出不入 expected、不误报", () => {
    const args = fixtureWith(accountsCase({}, "pub mod core;\npub use crate::core::Account;\n"));
    expect(run(args).status).toBe(0);
  });

  it("文件模块与目录模块同名共存（transport.rs + transport/）→ 绿（声明面与磁盘双向全等）", () => {
    // `mod transport;` 在 Rust 里同时允许 transport.rs（模块本体）与 transport/
    // （子模块目录）——磁盘枚举按模块键判等，不因同名报孤儿。
    const args = fixtureWith({
      crate: "ledger-sync-engine",
      dir: "crates/sync-engine",
      libRs: "pub mod transport;\n",
      files: { "transport.rs": STUB, "transport/s3.rs": STUB },
    });
    expect(run(args).status).toBe(0);
  });

  it("形状判定：#[cfg(test)] mod tests; 豁免（ADR-0056 决策 5）→ 绿", () => {
    const args = fixtureWith(accountsCase({}, "pub mod core;\n#[cfg(test)]\nmod tests;\n"));
    expect(run(args).status).toBe(0);
  });

  it("形状判定：豁免按目标模块名 tests 判，#[cfg(test)] mod helper; 仍需文件", () => {
    const bad = run(fixtureWith(accountsCase({}, "pub mod core;\n#[cfg(test)]\nmod helper;\n")));
    expect(bad.status).toBe(1);
    expect(bad.output).toContain("声明缺失");
    expect(bad.output).toContain("helper");
    const good = run(
      fixtureWith(
        accountsCase({ "helper.rs": STUB }, "pub mod core;\n#[cfg(test)]\nmod helper;\n"),
      ),
    );
    expect(good.status).toBe(0);
  });

  it("形状判定：非 pub mod、pub(crate) mod、#[allow(dead_code)] mod 计入 expected → 绿", () => {
    const args = fixtureWith(
      accountsCase(
        { "balance.rs": STUB, "command.rs": STUB, "core.rs": STUB, "model.rs": STUB },
        "mod balance;\n#[allow(dead_code)]\nmod command;\n#[allow(dead_code)] mod core;\npub(crate) mod model;\n",
      ),
    );
    expect(run(args).status).toBe(0);
  });

  it("形状判定：feature 门控 mod（含 rustfmt 拆行多行 cfg）计入 expected → 绿", () => {
    const args = fixtureWith(
      accountsCase(
        { "balance.rs": STUB, "command.rs": STUB, "core.rs": STUB, "model.rs": STUB },
        '#[cfg(feature = "x")]\nmod balance;\n' +
          '#[cfg(any(\n    test,\n    feature = "test-utils",\n))]\n#[doc(hidden)]\nmod command;\n' +
          "mod core;\nmod model;\n",
      ),
    );
    expect(run(args).status).toBe(0);
  });

  it("形状判定：raw identifier 声明（mod r#type;）计入 expected → 绿", () => {
    const args = fixtureWith(
      accountsCase({ "core.rs": STUB, "type.rs": STUB }, "pub mod core;\npub mod r#type;\n"),
    );
    expect(run(args).status).toBe(0);
  });

  it("形状判定：注释与字符串中的 mod 不误报（掩码边界）→ 绿", () => {
    const args = fixtureWith(
      accountsCase({}, '// mod phantom;\nconst DOC: &str = "mod phantom;";\npub mod core;\n'),
    );
    expect(run(args).status).toBe(0);
  });

  it("形状判定：#[path] mod → fail loud（改写模块↔文件映射）", () => {
    const args = fixtureWith(accountsCase({}, '#[path = "core_impl.rs"]\nmod core;\n'));
    const r = run(args);
    expect(r.status).toBe(1);
    expect(r.output).toContain("认不出的 lib.rs 声明形状");
    expect(r.output).toContain("path");
  });

  it("形状判定：include! → fail loud（展开面文本不可达）", () => {
    const args = fixtureWith(accountsCase({}, 'include!("generated.rs");\npub mod core;\n'));
    const r = run(args);
    expect(r.status).toBe(1);
    expect(r.output).toContain("认不出的 lib.rs 声明形状");
    expect(r.output).toContain("include!");
  });

  it("形状判定：内联模块块与认不出的属性链 → fail loud", () => {
    const inline = run(fixtureWith(accountsCase({}, "mod core { }\n")));
    expect(inline.status).toBe(1);
    expect(inline.output).toContain("内联模块块");
    const unknown = run(fixtureWith(accountsCase({}, "#[some_proc_macro]\nmod core;\n")));
    expect(unknown.status).toBe(1);
    expect(unknown.output).toContain("认不出的 lib.rs 声明形状");
    expect(unknown.output).toContain("some_proc_macro");
  });

  it("单文件模块声明但目录只剩测试豁免 → 红（声明面要真实模块，不静默失靶）", () => {
    const args = fixtureWith(accountsCase({}, "pub mod core;\npub mod only_tests;\n"));
    writeCrateFile(args[1], "crates/accounts", "only_tests/tests.rs", STUB);
    const r = run(args);
    expect(r.status).toBe(1);
    expect(r.output).toContain("声明缺失");
    expect(r.output).toContain("only_tests");
  });
});

describe("check-structure 跨 crate 依赖方向（#1596 cargo 退役 / ADR-0071 修订注记）", () => {
  /**
   * 退役留痕（#1596 / ADR-0056 决策 4 修订注记）：跨 crate 越界方向的源码文本
   * 扫描退役——未声明依赖即编译失败（cargo 强于文本扫描：`use crate::x as y`
   * 别名改写对文本不可达、编译期逃不掉），已声明的越界方向由 crate 边界核对按
   * `Cargo.toml` 声明面判定（层秩 + 业务域→多端同步域禁令）。下列用例把原
   * 「文本扫描红」钉成「文本扫描不再报红」——退役不得静默回潮（扫描若被恢复，
   * 本组变红）。逐族编译错误证据见 PR #1605。
   */
  /** 成员 crate 清单桩：依赖面 + 门禁继承。 */
  const crateManifest = (name: string, deps: string): string =>
    `[package]\nname = "${name}"\nversion = "0.6.0"\nedition = "2024"\n\n` +
    `[dependencies]\n${deps}\n\n[lints]\nworkspace = true\n`;

  /** infra 清单桩：带 http 投影门 features/axum 形态 + 给定 dev-dependency 行。 */
  const infraFixtureManifest = (devDeps: string): string =>
    '[package]\nname = "ledger-infra"\nversion = "0.6.0"\nedition = "2024"\n\n' +
    '[features]\nhttp = ["dep:axum"]\n\n' +
    '[dependencies]\naxum = { version = "0.8", optional = true }\n\n' +
    `[dev-dependencies]\n${devDeps}\n\n[lints]\nworkspace = true\n`;

  it("基础设施文件引用域模块 / 域 crate 模块引用壳层 → 文本扫描已退役（红源换为编译期）", () => {
    // `crate::test_support` 在 ledger-infra 内即 `ledger_infra::test_support`——不存在；
    // 即便写成 `tauri_app_lib::test_support`，dev-dependency 也只对测试目标可见，
    // 生产面编译失败。原文扫描已无靶向价值。
    expect(
      run(
        fixtureWith(
          infraCase({ "db/helper.rs": "use crate::test_support::open;\npub fn x() {}\n" }),
        ),
      ).status,
    ).toBe(0);
    const account = fixtureWith(accountsCase({ "core.rs": shellUse }));
    expect(run(account).status).toBe(0);
  });

  it("协议 crate 模块引用域目录 → 文本扫描已退役（红源换为编译期 / 层秩声明面）", () => {
    const args = fixtureWith(
      protocolCase({ "op.rs": "use crate::test_support::open;\npub fn x() {}\n" }),
    );
    expect(run(args).status).toBe(0);
  });

  it("域 crate 模块引用同步域 → 文本扫描已退役（红源换为编译期 / 声明面禁令）", () => {
    // `crate::sync_engine` 在域 crate 内编不过；`ledger_sync_engine::` 必伴随声明
    // 依赖，由 crate 边界禁令拦下（负向夹具见下）。
    const args = fixtureWith(
      accountsCase({ "core.rs": "use crate::sync_engine::engine::ReplayEffect;\npub fn x() {}\n" }),
    );
    expect(run(args).status).toBe(0);
  });

  it("基础设施 dev-dependency 反向依赖更高层 crate 未留痕 → 红（换载体：声明面核对）", () => {
    // cargo 对 dev-dependency 环放行（测试目标与生产依赖图分离），是编译期盲区：
    // 基础设施对业务域 crate 的 dev-dependency 须逐条留痕于
    // INFRA_DOMAIN_ALLOWED_EDGES，清单之外的声明即红。
    const args = makeCrateFixture({
      memberManifests: {
        "crates/infra": infraFixtureManifest('ledger-policy = { path = "../policy" }'),
      },
    });
    const r = run(args);
    expect(r.status).toBe(1);
    expect(r.output).toContain("基础设施→域方向");
    expect(r.output).toContain("ledger-policy");
  });

  it("基础设施 dev-dependency 已留痕于台账（测试专用边）→ 绿", () => {
    const args = makeCrateFixture({
      memberManifests: {
        "crates/infra": infraFixtureManifest(
          'tauri-app = { path = "../.." }\nledger-transaction = { path = "../transaction" }',
        ),
      },
    });
    expect(run(args).status).toBe(0);
  });

  it("基础设施 dev-dependency 指向非域层（协议 / 基础设施）→ 不受台账约束（生产方向仍归层秩核对）", () => {
    const args = makeCrateFixture({
      memberManifests: {
        "crates/infra": infraFixtureManifest(
          'ledger-infra = { path = "." }\nledger-sync-protocol = { path = "../sync-protocol" }',
        ),
      },
    });
    expect(run(args).status).toBe(0);
  });

  it("基础设施 crate 生产依赖域 crate → 红（层秩声明面核对，F8 生产面换载体后仍守）", () => {
    const args = makeCrateFixture({
      memberManifests: {
        "crates/infra":
          '[package]\nname = "ledger-infra"\nversion = "0.6.0"\nedition = "2024"\n\n' +
          '[features]\nhttp = ["dep:axum"]\n\n' +
          '[dependencies]\naxum = { version = "0.8", optional = true }\n' +
          'ledger-policy = { path = "../policy" }\n\n[lints]\nworkspace = true\n',
      },
    });
    const r = run(args);
    expect(r.status).toBe(1);
    expect(r.output).toContain("crate 依赖方向");
    expect(r.output).toContain("ledger-infra");
    expect(r.output).toContain("ledger-policy");
  });

  it("协议 crate 生产依赖域 crate / 壳 crate → 红（层秩声明面核对，#1089 退役文本扫描后仍守）", () => {
    const args = makeCrateFixture({
      memberManifests: {
        "crates/sync-protocol": crateManifest(
          "ledger-sync-protocol",
          'ledger-transaction = { path = "../transaction" }\ntauri-app = { path = "../.." }',
        ),
      },
    });
    const r = run(args);
    expect(r.status).toBe(1);
    expect(r.output).toContain("crate 依赖方向");
    expect(r.output).toContain("ledger-sync-protocol");
    expect(r.output).toContain("ledger-transaction");
    expect(r.output).toContain("tauri-app");
  });

  it("账户域 crate 生产依赖核心交易域 crate → 绿（域→域合法上层依赖，ADR-0071 决策 5）", () => {
    const args = makeCrateFixture({
      memberManifests: {
        "crates/accounts": crateManifest(
          "ledger-accounts",
          'ledger-transaction = { path = "../transaction" }',
        ),
      },
    });
    expect(run(args).status).toBe(0);
  });

  it("真实仓库默认通过：跨 crate 方向走声明面（认许边留痕于脚本）", () => {
    const r = run([]);
    expect(r.status).toBe(0);
    expect(r.output).toContain("跨 crate 依赖方向：声明面判定");
    expect(r.output).toContain("基础设施 dev-dependency 方向零未留痕声明（认许边 4 条");
  });
});

describe("check-structure 基础设施 crate 内块间禁边（ADR-0111 决策 4 / #1134）", () => {
  it("db 引用 boot（非认许边文件）→ 红并定位文件行号", () => {
    const args = fixtureWith(
      infraCase({
        "db/helper.rs": "use crate::boot::encryption::probe_file_kind;\npub fn x() {}\n",
      }),
    );
    const r = run(args);
    expect(r.status).toBe(1);
    expect(r.output).toContain("crate 内反向依赖");
    expect(r.output).toContain("db/helper.rs:1");
  });

  it("块间禁边两形态都入扫描：db.rs（单文件模块）注入违规同样红", () => {
    // 目录形态（db/helper.rs）由上一枚用例覆盖；本用例锁单文件模块形态
    // （db.rs）——两形态都在扫描面内（条目由声明投影，不因形态不同漏扫）。
    const args = fixtureWith({
      crate: "ledger-infra",
      dir: "crates/infra",
      libRs: INFRA_CASE.libRs,
      files: {
        "boot/mod.rs": STUB,
        "db.rs": "use crate::boot::encryption::probe_file_kind;\npub fn x() {}\n",
        "error.rs": INFRA_CASE.files["error.rs"] as string,
        "test_utils.rs": STUB,
      },
    });
    const r = run(args);
    expect(r.status).toBe(1);
    expect(r.output).toContain("crate 内反向依赖");
    expect(r.output).toContain("db.rs:1");
  });

  it("db 引用 signals → 红（shell_support 靶已随 #1108 迁出退役）", () => {
    const args = fixtureWith(
      infraCase({ "db/runtime.rs": "use crate::signals::WriteOp;\npub fn x() {}\n" }),
    );
    const r = run(args);
    expect(r.status).toBe(1);
    expect(r.output).toContain("signals");
  });

  it("db/mod.rs 再导出 shim 合规绿；他文件同引用红", () => {
    const shim = "pub use crate::boot::{encryption, data_location};\npub fn x() {}\n";
    expect(run(fixtureWith(infraCase({ "db/mod.rs": shim }))).status).toBe(0);
    const bad = fixtureWith(infraCase({ "db/helper.rs": shim }));
    const r = run(bad);
    expect(r.status).toBe(1);
    expect(r.output).toContain("db/helper.rs:1");
  });

  it("boot → db 合法单向 → 绿", () => {
    const args = fixtureWith(
      infraCase({ "boot/helper.rs": "use crate::db::open_connection;\npub fn x() {}\n" }),
    );
    expect(run(args).status).toBe(0);
  });

  it("注释与字符串中的块路径不误报", () => {
    const args = fixtureWith(
      infraCase({
        "db/helper.rs": [
          "/// [`crate::boot`] 升顶层（文档注释不算）",
          "// 历史 shell_support 引用已随 #1108 迁出根包",
          'let s = "crate::signals::WriteOp";',
          "pub fn f() {}",
          "",
        ].join("\n"),
      }),
    );
    expect(run(args).status).toBe(0);
  });

  it("外挂测试豁免：db/tests/ 引用 boot 不红", () => {
    const args = fixtureWith(
      infraCase({
        "db/tests/common.rs": "pub fn s() { crate::boot::encryption::probe_file_kind(); }\n",
      }),
    );
    expect(run(args).status).toBe(0);
  });
});

describe("check-structure 模型域化禁令（ADR-0059 决策 6 / #424 T7 收口）", () => {
  it("规则①：crate::models 全局模型路径残留 → 红", () => {
    const args = fixtureWith(
      accountsCase({ "core.rs": "use crate::models::Transaction;\npub fn x() {}\n" }),
    );
    const r = run(args);
    expect(r.status).toBe(1);
    expect(r.output).toContain("全局模型路径残留");
    expect(r.output).toContain("core.rs:1");
  });

  it("规则①：tauri_app_lib::models 形态同样识别 → 红（根 src 全树扫描）", () => {
    const args = makeCrateFixture();
    writeCrateFile(
      args[1],
      ".",
      "commands/transactions.rs",
      "let t: tauri_app_lib::models::Transaction;\n",
    );
    const r = run(args);
    expect(r.status).toBe(1);
    expect(r.output).toContain("全局模型路径残留");
  });

  it("规则①：注释与字符串中的 models 路径不误报（掩码边界）", () => {
    const args = makeCrateFixture();
    writeCrateFile(
      args[1],
      "src",
      "test_support/cost.rs",
      [
        "/// 全局模型目录已消亡，crate::models 是历史形态（文档注释不算引用）",
        "// 见 crate::models::Transaction 说明",
        'let s = "crate::models::Transaction";',
        "pub fn f() {}",
      ].join("\n"),
    );
    expect(run(args).status).toBe(0);
  });

  it("规则①：外挂测试豁免（tests.rs / tests/ 目录不参与扫描）", () => {
    const args = makeCrateFixture();
    writeCrateFile(args[1], ".", "test_support/tests.rs", "use crate::models::Transaction;\n");
    writeCrateFile(
      args[1],
      ".",
      "test_support/tests/scaffold.rs",
      "use tauri_app_lib::models::Transaction;\n",
    );
    expect(run(args).status).toBe(0);
  });

  it("规则②：域接缝 glob 再导出 pub use model::* → 红", () => {
    const args = makeCrateFixture();
    writeCrateFile(args[1], ".", "test_support/mod.rs", "mod model;\npub use model::*;\n");
    const r = run(args);
    expect(r.status).toBe(1);
    expect(r.output).toContain("glob 再导出");
    expect(r.output).toContain("test_support/mod.rs:2");
  });

  it("规则②：跨域拍平与旧全局目录同名形态 → 红", () => {
    const flat = makeCrateFixture();
    writeCrateFile(flat[1], ".", "test_support/mod.rs", "pub use crate::transaction::model::*;\n");
    expect(run(flat).output).toContain("glob 再导出");
    const legacy = makeCrateFixture();
    writeCrateFile(legacy[1], ".", "test_support/mod.rs", "pub use models::*;\n");
    expect(run(legacy).output).toContain("glob 再导出");
  });

  it("规则②：域模型文件内 glob 聚合 → 红（文件与目录形态同判）", () => {
    const fileCase = makeCrateFixture();
    writeCrateFile(fileCase[1], ".", "test_support/model.rs", "pub use super::crud::*;\n");
    const r = run(fileCase);
    expect(r.status).toBe(1);
    expect(r.output).toContain("glob 聚合");
    expect(r.output).toContain("test_support/model.rs:1");
    const dirCase = makeCrateFixture();
    writeCrateFile(
      dirCase[1],
      ".",
      "test_support/model/price.rs",
      "pub use crate::test_support::types::*;\npub fn x() {}\n",
    );
    const r2 = run(dirCase);
    expect(r2.status).toBe(1);
    expect(r2.output).toContain("域模型文件内 glob 聚合");
    expect(r2.output).toContain("test_support/model/price.rs");
  });

  it("规则②：逐类型再导出与域内私有 glob 引用合规 → 绿", () => {
    const args = makeCrateFixture();
    writeCrateFile(
      args[1],
      ".",
      "test_support/mod.rs",
      "mod model;\npub use model::{Item, ItemInput};\n",
    );
    writeCrateFile(
      args[1],
      ".",
      "test_support/behavior.rs",
      "use super::model::*;\npub fn x() {}\n",
    );
    writeCrateFile(args[1], ".", "test_support/model.rs", "pub struct Item;\n");
    expect(run(args).status).toBe(0);
  });
});

describe("check-structure 原生事务语句禁令（issue #1014 / #1003 定案 7）", () => {
  it("产品代码手写 BEGIN → 红并定位文件行号", () => {
    const args = fixtureWith(
      accountsCase({
        "core.rs": 'pub fn f(conn: &Connection) {\n    conn.execute("BEGIN", []);\n}\n',
      }),
    );
    const r = run(args);
    expect(r.status).toBe(1);
    expect(r.output).toContain("原生事务语句");
    expect(r.output).toContain("core.rs:2");
    expect(r.output).toContain("db/tx_scope.rs");
  });

  it("COMMIT / ROLLBACK 手写与 execute_batch 变体同样识别 → 红", () => {
    const args = fixtureWith(
      accountsCase({
        "core.rs":
          'pub fn f(conn: &Connection) {\n    conn.execute("COMMIT", []);\n    conn.execute_batch("ROLLBACK");\n}\n',
      }),
    );
    const r = run(args);
    expect(r.status).toBe(1);
    expect(r.output).toContain("core.rs:2");
    expect(r.output).toContain("core.rs:3");
    expect(r.output).toContain("db/tx_scope.rs");
  });

  it("唯一合法住址 db/tx_scope.rs 内的原生事务语句 → 绿（execute 与 execute_batch 同）", () => {
    const args = fixtureWith(
      infraCase({
        "db/tx_scope.rs":
          'pub fn hold(conn: &Connection) {\n    conn.execute("BEGIN", []);\n    conn.execute_batch("COMMIT");\n}\n',
      }),
    );
    expect(run(args).status).toBe(0);
  });

  it('注释中的 execute("BEGIN") 不误报（只掩码注释、保留字符串）', () => {
    const args = fixtureWith(
      accountsCase({
        "core.rs": [
          '// 原生事务语句禁令：conn.execute("BEGIN", []) 只许出现在 db/tx_scope.rs',
          '/// conn.execute("COMMIT", [])',
          "pub fn f() {}",
          "",
        ].join("\n"),
      }),
    );
    expect(run(args).status).toBe(0);
  });

  it("真实仓库默认通过：原生事务语句仅 db/tx_scope.rs 一处", () => {
    const r = run([]);
    expect(r.status).toBe(0);
    expect(r.output).toContain("原生事务语句全树扫描");
  });
});

describe("check-structure 交易域区级层序（ADR-0113 决策 7 / #1181 / #1597）", () => {
  // 负向夹具各由一条断言锚定（ADR-0087 断言强度，断言对准退出码与输出）：
  // 区级层序、根级单文件、未登记区目录各有一枚夹具；删任一条断言须动脚本
  //（清单外无豁免面），对应夹具转绿 → 该夹具测试失败（CI 红）。
  it("④ 根级单文件必须进区目录（fail loud）→ 红（#1597，删除检查即变红）", () => {
    const args = fixtureWith(
      transactionCase({ "search_semantics.rs": STUB }, TRANSACTION_CASE.libRs),
    );
    const r = run(args);
    expect(r.status).toBe(1);
    expect(r.output).toContain("根级单文件必须进区目录");
  });

  it("④ 未登记区归属的顶层目录 → 红（判向不静默失靶，#1597）", () => {
    const args = fixtureWith(
      transactionCase(
        { "rogue_zone/helper.rs": STUB },
        TRANSACTION_CASE.libRs + "pub mod rogue_zone;\n",
      ),
    );
    const r = run(args);
    expect(r.status).toBe(1);
    expect(r.output).toContain("区目录未登记区归属");
  });

  it("区目录表与真实仓库顶层区目录全等（#1597）：zone 表无悬空行、顶层目录全有区归属", () => {
    // 常量表核对无法经夹具红，用双向全等锚替代（同登记面长度锚先例）：新增 /
    // 删除区目录而 zone 表不同步 → 本断言红；zone 表悬空行同理。真实仓库自检
    //（run([])）同样照 zone 表判向，两侧互为见证。
    const srcDir = join(process.cwd(), "src-tauri", "crates", "transaction", "src");
    const dirs = readdirSync(srcDir, { withFileTypes: true })
      .filter((e) => e.isDirectory() && e.name !== "tests")
      .map((e) => e.name)
      .filter((name) => readdirSync(join(srcDir, name)).some((f) => f.endsWith(".rs")))
      .sort();
    expect(Object.keys(TRANSACTION_ZONE_BY_DIR).sort()).toEqual(dirs);
  });

  it("真实仓库默认通过：区级层序零未认许反向引用（#1182 消除反边后认许边归空）", () => {
    const r = run([]);
    expect(r.status).toBe(0);
    expect(r.output).toContain("区级层序零未认许反向引用");
    expect(r.output).toContain(`认许边 ${TRANSACTION_ZONE_ALLOWED_EDGES.length} 条`);
  });

  it("② 共享语义引用写路径（认许边之外）→ 红并定位文件行号", () => {
    const args = fixtureWith(
      transactionCase({
        "command/payload.rs": "use crate::write::writer::NormalizedRow;\npub fn x() {}\n",
      }),
    );
    const r = run(args);
    expect(r.status).toBe(1);
    expect(r.output).toContain("区级反向依赖");
    expect(r.output).toContain("command/payload.rs:1");
  });

  it("② 共享语义引用接缝（认许边之外）→ 红", () => {
    const args = fixtureWith(
      transactionCase({
        "shared/search_text.rs": "use crate::seams::merchant::ensure_merchant;\npub fn x() {}\n",
      }),
    );
    const r = run(args);
    expect(r.status).toBe(1);
    expect(r.output).toContain("区级反向依赖");
  });

  it("② 接缝引用路径区（写 / 读）→ 红", () => {
    const seamToWrite = fixtureWith(
      transactionCase({
        "seams/merchant.rs": "use crate::write::protocol::create;\npub fn x() {}\n",
      }),
    );
    expect(run(seamToWrite).status).toBe(1);
    const seamToRead = fixtureWith(
      transactionCase({
        "seams/investment.rs": "use crate::read::list_transactions;\npub fn x() {}\n",
      }),
    );
    expect(run(seamToRead).status).toBe(1);
  });

  it("② 写读两径互不依赖（双向）→ 红", () => {
    const writeToRead = fixtureWith(
      transactionCase({
        "write/batch.rs": "use crate::read::search::search_transactions;\npub fn x() {}\n",
      }),
    );
    const r = run(writeToRead);
    expect(r.status).toBe(1);
    expect(r.output).toContain("区级反向依赖");
    const readToWrite = fixtureWith(
      transactionCase({
        "read/extra.rs": "use crate::write::batch::TransactionBatch;\npub fn x() {}\n",
      }),
    );
    expect(run(readToWrite).status).toBe(1);
  });

  it("合法层序链：写→接缝→共享语义、读→同区、同区互依 → 绿", () => {
    const args = fixtureWith(
      transactionCase({
        "write/batch.rs":
          "use crate::seams::balance::recalculate;\nuse crate::amount::TransactionKind;\npub fn x() {}\n",
        "read/search.rs":
          "use crate::read::source::list_view;\nuse crate::model::Transaction;\npub fn y() {}\n",
        "write/protocol.rs": "use crate::write::writer::insert_row;\npub fn z() {}\n",
      }),
    );
    expect(run(args).status).toBe(0);
  });

  it("crate 根 lib.rs 是声明与再导出面：跨区再导出不参与区级判向（免登不误报）", () => {
    const args = fixtureWith(
      transactionCase(
        {},
        TRANSACTION_CASE.libRs + "pub use crate::write::writer::NormalizedRow;\n",
      ),
    );
    expect(run(args).status).toBe(0);
  });

  it("注释与字符串中的跨区路径不误报（掩码边界）", () => {
    const args = fixtureWith(
      transactionCase({
        "shared/search_text.rs": [
          "/// 消费方见 `crate::write::writer` 与 `crate::read`（文档注释不算依赖）",
          "// crate::write::protocol::create",
          'let s = "crate::write::batch::run";',
          "pub fn f() {}",
          "",
        ].join("\n"),
      }),
    );
    expect(run(args).status).toBe(0);
  });

  it("花括号列举逐条展开：非首段跨区条目同样命中", () => {
    const args = fixtureWith(
      transactionCase({
        "shared/search_text.rs":
          "use crate::{model::Transaction, read::list_view};\npub fn x() {}\n",
      }),
    );
    const r = run(args);
    expect(r.status).toBe(1);
    expect(r.output).toContain("区级反向依赖");
    expect(r.output).toContain("read");
  });

  it("花括号列举闭括号后文本不吞入：后续枚举变体同名不误报（off-by-one 回归锚）", () => {
    const args = fixtureWith(
      transactionCase({
        "shared/search_text.rs":
          "use crate::{model::Transaction};\npub enum E { A, write }\npub fn x() {}\n",
      }),
    );
    expect(run(args).status).toBe(0);
  });

  it("外挂测试豁免：write/writer/tests/ 引用读路径不红（ADR-0056 决策 5）", () => {
    const args = fixtureWith(
      transactionCase({
        "write/writer/tests/fixture.rs": "use crate::read::list_view;\npub fn s() {}\n",
      }),
    );
    expect(run(args).status).toBe(0);
  });
});

describe("check-structure crate 边界核对（spec #1086 / issue #1087 门禁前置）", () => {
  it("workspace 骨架夹具：成员登记 + 门禁继承 + 命令覆盖齐全 → 通过", () => {
    const r = run(makeCrateFixture());
    expect(r.status).toBe(0);
    expect(r.output).toContain(`crate 边界 ${CRATES.length} 个`);
  });

  it("真实仓库默认通过：crate 边界核对入摘要", () => {
    const r = run([]);
    expect(r.status).toBe(0);
    expect(r.output).toContain(`crate 边界 ${CRATES.length} 个`);
  });

  it("成员 crate 删掉 [lints] workspace = true → 红（门禁继承删除即变红）", () => {
    const args = makeCrateFixture({
      memberManifests: {
        [INFRA_SRC_REL.slice(0, -"/src".length)]:
          '[package]\nname = "ledger-infra"\nversion = "0.6.0"\nedition = "2024"\n',
      },
    });
    const r = run(args);
    expect(r.status).toBe(1);
    expect(r.output).toContain("门禁继承");
    expect(r.output).toContain("ledger-infra");
  });

  it("根包删掉 [lints] workspace = true → 红", () => {
    const args = makeCrateFixture({
      rootManifest: [
        "[package]",
        'name = "tauri-app"',
        'version = "0.6.0"',
        'edition = "2024"',
        "",
        "[workspace]",
        'members = ["crates/*"]',
        'resolver = "3"',
        "",
        "[workspace.lints.clippy]",
        LINT_DENIES,
        "",
      ].join("\n"),
    });
    const r = run(args);
    expect(r.status).toBe(1);
    expect(r.output).toContain("根包缺 [lints] workspace = true");
  });

  it("新增成员目录未登记 CRATES → 红（成员登记删除即变红）", () => {
    const args = makeCrateFixture({ orphanCrate: "newdomain" });
    const r = run(args);
    expect(r.status).toBe(1);
    expect(r.output).toContain("未登记 CRATES");
    expect(r.output).toContain("crates/newdomain");
  });

  it("workspace members 未用 crates/* glob → 红", () => {
    const args = makeCrateFixture({
      rootManifest: [
        "[package]",
        'name = "tauri-app"',
        'version = "0.6.0"',
        'edition = "2024"',
        "",
        "[workspace]",
        'members = ["crates/infra"]',
        'resolver = "3"',
        "",
        "[workspace.lints.clippy]",
        LINT_DENIES,
        "",
        "[lints]",
        "workspace = true",
        "",
      ].join("\n"),
    });
    const r = run(args);
    expect(r.status).toBe(1);
    expect(r.output).toContain("crates/*");
  });

  it("静态检查/测试命令缺 --workspace → 红（命令覆盖全成员删除即变红）", () => {
    const args = makeCrateFixture({ testSh: "( cd src-tauri && cargo test )\n" });
    const r = run(args);
    expect(r.status).toBe(1);
    expect(r.output).toContain("缺 --workspace");
    expect(r.output).toContain("test.sh");
  });

  it("test-exec.ts（.ts 形态宿主）缺 --workspace → 红（#1112 登记）", () => {
    const args = makeCrateFixture({ testExecTs: 'const BUILD = "cargo test --no-run"\n' });
    const r = run(args);
    expect(r.status).toBe(1);
    expect(r.output).toContain("缺 --workspace");
    expect(r.output).toContain("test-exec.ts");
  });

  it("test-exec.ts 注释里的 `cargo test` 不算命令（.ts 注释掩码，不假红）", () => {
    const args = makeCrateFixture({
      testExecTs:
        "// 口径说明：`cargo test` 默认执行面\n" +
        "/** 另一处：cargo test --no-run */\n" +
        'const BUILD = "cargo test --workspace --no-run"\n',
    });
    expect(run(args).status).toBe(0);
  });

  it("test-exec.ts 数组形态命令（真实命令面）缺 --workspace → 红（#1112 P2 登记生效）", () => {
    const args = makeCrateFixture({
      testExecTs: "runChild(cargo, ['test', '--no-run'], { cwd: root })\n",
    });
    const r = run(args);
    expect(r.status).toBe(1);
    expect(r.output).toContain("数组形态缺");
    expect(r.output).toContain("test-exec.ts");
  });

  it("test-exec.ts 数组形态命令带 --workspace → 通过（数组面单独成立）", () => {
    const args = makeCrateFixture({
      testExecTs: "runChild(cargo, ['test', '--workspace', '--no-run'], { cwd: root })\n",
    });
    expect(run(args).status).toBe(0);
  });

  it("命令被 echo 包成说明文字 → 红（引号内不是命令面，拒绝空集假绿，#1112 P1）", () => {
    const args = makeCrateFixture({
      testSh: 'echo "( cd src-tauri && cargo test --workspace )"\n',
    });
    const r = run(args);
    expect(r.status).toBe(1);
    expect(r.output).toContain("未发现任何命令位置上的 cargo 命令");
    expect(r.output).toContain("空集假绿");
  });

  it("`--all-targets` 等 `--all*` 旗标不算 workspace 范围 → 红（防假绿回归）", () => {
    const args = makeCrateFixture({
      lintFixSh:
        "( cd src-tauri && cargo clippy --fix --all-targets --all-features --allow-dirty --allow-staged )\n",
    });
    const r = run(args);
    expect(r.status).toBe(1);
    expect(r.output).toContain("缺 --workspace");
    expect(r.output).toContain("lint-fix.sh");
  });

  it("cargo fmt 缺 --all → 红（fmt 的 workspace 别名是 --all）", () => {
    const args = makeCrateFixture({ checkSh: "( cd src-tauri && cargo fmt -- --check )\n" });
    const r = run(args);
    expect(r.status).toBe(1);
    expect(r.output).toContain("缺 --workspace");
    expect(r.output).toContain("check.sh");
  });

  it("基础设施 crate 反向依赖壳层 crate → 红（依赖方向核对）", () => {
    const args = makeCrateFixture({
      memberManifests: {
        "crates/infra":
          '[package]\nname = "ledger-infra"\nversion = "0.6.0"\nedition = "2024"\n\n' +
          '[dependencies]\ntauri-app = { path = "../.." }\n\n[lints]\nworkspace = true\n',
      },
    });
    const r = run(args);
    expect(r.status).toBe(1);
    expect(r.output).toContain("crate 依赖方向");
    expect(r.output).toContain("ledger-infra");
  });

  it("基础设施 crate 测试以 dev-dependency 反向依赖壳层 → 绿（测试专用边，spec #1086）", () => {
    const args = makeCrateFixture({
      memberManifests: {
        "crates/infra":
          '[package]\nname = "ledger-infra"\nversion = "0.6.0"\nedition = "2024"\n\n' +
          '[features]\nhttp = ["dep:axum"]\n\n' +
          '[dependencies]\naxum = { version = "0.8", optional = true }\n\n' +
          '[dev-dependencies]\ntauri-app = { path = "../.." }\n\n[lints]\nworkspace = true\n',
      },
    });
    expect(run(args).status).toBe(0);
  });

  it("域 crate 生产依赖壳层 crate → 红（依赖方向核对，编译期拒绝的机器面）", () => {
    const args = makeCrateFixture({
      memberManifests: {
        "crates/accounts":
          '[package]\nname = "ledger-accounts"\nversion = "0.6.0"\nedition = "2024"\n\n' +
          '[dependencies]\ntauri-app = { path = "../.." }\n\n[lints]\nworkspace = true\n',
      },
    });
    const r = run(args);
    expect(r.status).toBe(1);
    expect(r.output).toContain("crate 依赖方向");
    expect(r.output).toContain("ledger-accounts");
  });

  it("域 crate 生产依赖更底层域 crate → 绿（域→域合法上层依赖，ADR-0112 决策 2）", () => {
    const args = makeCrateFixture({
      memberManifests: {
        "crates/accounts":
          '[package]\nname = "ledger-accounts"\nversion = "0.6.0"\nedition = "2024"\n\n' +
          '[dependencies]\nledger-transaction = { path = "../transaction" }\n\n' +
          '[dev-dependencies]\ntauri-app = { path = "../.." }\n\n[lints]\nworkspace = true\n',
      },
    });
    expect(run(args).status).toBe(0);
  });

  it("全成员 crate 生产依赖壳层 crate → 逐个红（旧逐 crate describe 的声明面覆盖等价）", () => {
    // 逐 crate 覆盖等价（#1596 前 12 个逐域 describe 各测本 crate 的声明面红线）：
    // 声明面核对对每个成员 crate 都成立，任一 crate 掉出核对面即此处红。
    for (const crate of CRATES) {
      if (crate.dir === ".") continue; // 根包即壳层自身
      const manifest =
        `[package]\nname = "${crate.name}"\nversion = "0.6.0"\nedition = "2024"\n\n` +
        '[dependencies]\ntauri-app = { path = "../.." }\n\n[lints]\nworkspace = true\n';
      const args = makeCrateFixture({ memberManifests: { [crate.dir]: manifest } });
      const r = run(args);
      expect(r.status, `${crate.name} 生产依赖壳层未变红`).toBe(1);
      expect(r.output).toContain("crate 依赖方向");
      expect(r.output).toContain(crate.name);
    }
  });

  it("业务域 crate 生产依赖多端同步域 crate → 红（同层禁边，ADR-0101 决策 4b / #1107）", () => {
    const args = makeCrateFixture({
      memberManifests: {
        "crates/accounts":
          '[package]\nname = "ledger-accounts"\nversion = "0.6.0"\nedition = "2024"\n\n' +
          '[dependencies]\nledger-sync-engine = { path = "../sync-engine" }\n\n' +
          '[dev-dependencies]\ntauri-app = { path = "../.." }\n\n[lints]\nworkspace = true\n',
      },
    });
    const r = run(args);
    expect(r.status).toBe(1);
    expect(r.output).toContain(
      "业务域 crate ledger-accounts 生产依赖多端同步域 ledger-sync-engine",
    );
  });
});

describe("check-structure test_utils 生产编译门（ADR-0111 决策 5 / issue #1132）", () => {
  const infraLibRsWith = (testUtilsDecl: string): string =>
    "pub mod boot;\npub mod db;\npub mod error;\n" + testUtilsDecl;

  it("真实仓库与骨架夹具默认通过：cfg 门 + 生产依赖不启用 test-utils", () => {
    expect(run([]).output).toContain("test_utils 生产编译门");
    expect(run(makeCrateFixture()).status).toBe(0);
  });

  it("infra lib.rs 摘掉 test_utils cfg 门 → 红（删除 cfg 门即变红）", () => {
    const args = makeCrateFixture({
      infraLibRs: infraLibRsWith("pub fn stub() {}\n#[doc(hidden)]\npub mod test_utils;\n"),
    });
    const r = run(args);
    expect(r.status).toBe(1);
    expect(r.output).toContain("test_utils 生产编译门");
    expect(r.output).toContain("cfg 门");
  });

  it("模块声明前只有普通注释 → 仍红（注释不构成门）", () => {
    const args = makeCrateFixture({
      infraLibRs: infraLibRsWith(
        "pub fn stub() {}\n// 仅注释说明，不构成门\npub mod test_utils;\n",
      ),
    });
    const r = run(args);
    expect(r.status).toBe(1);
    expect(r.output).toContain("test_utils 生产编译门");
  });

  it("cfg 门写在 doc(hidden) 之前 → 绿（属性顺序不敏感）", () => {
    const args = makeCrateFixture({
      infraLibRs: infraLibRsWith(
        'pub fn stub() {}\n#[doc(hidden)]\n#[cfg(any(test, feature = "test-utils"))]\npub mod test_utils;\n',
      ),
    });
    expect(run(args).status).toBe(0);
  });

  it("rustfmt 拆行的多行 cfg 门 → 绿（属性链解析读到配对闭合为止，#1469）", () => {
    const args = makeCrateFixture({
      infraLibRs: infraLibRsWith(
        'pub fn stub() {}\n#[cfg(any(\n    test,\n    feature = "test-utils",\n))]\n#[doc(hidden)]\npub mod test_utils;\n',
      ),
    });
    expect(run(args).status).toBe(0);
  });

  it("多行 cfg 门内注释提及 not( 不误判 → 绿（属性全文掩码注释后判定，#1469）", () => {
    const args = makeCrateFixture({
      infraLibRs: infraLibRsWith(
        'pub fn stub() {}\n#[cfg(any(\n    // 反向门 not(test) 形态已废弃，改为 feature 放行\n    feature = "test-utils",\n))]\npub mod test_utils;\n',
      ),
    });
    expect(run(args).status).toBe(0);
  });

  it("多行反向门 #[cfg(not(…))] → 仍红（多行解析不弱化反向门判定，#1469）", () => {
    const args = makeCrateFixture({
      infraLibRs: infraLibRsWith(
        "pub fn stub() {}\n#[cfg(not(\n    test,\n))]\npub mod test_utils;\n",
      ),
    });
    const r = run(args);
    expect(r.status).toBe(1);
    expect(r.output).toContain("test_utils 生产编译门");
    expect(r.output).toContain("放行测试");
  });

  it("反向门 #[cfg(not(test))] → 红（模块只留给生产）", () => {
    const args = makeCrateFixture({
      infraLibRs: infraLibRsWith("pub fn stub() {}\n#[cfg(not(test))]\npub mod test_utils;\n"),
    });
    const r = run(args);
    expect(r.status).toBe(1);
    expect(r.output).toContain("放行测试");
  });

  it("与测试无关的 cfg 门 → 红（等价于无门）", () => {
    const args = makeCrateFixture({
      infraLibRs: infraLibRsWith(
        "pub fn stub() {}\n#[cfg(debug_assertions)]\npub mod test_utils;\n",
      ),
    });
    expect(run(args).status).toBe(1);
  });

  it("根包生产依赖 ledger-infra 启用 test-utils → 红（生产会编入测试器具）", () => {
    const args = makeCrateFixture({
      rootInfraProdDep: 'ledger-infra = { path = "crates/infra", features = ["test-utils"] }',
    });
    const r = run(args);
    expect(r.status).toBe(1);
    expect(r.output).toContain("test_utils 生产编译门");
    expect(r.output).toContain("生产依赖");
  });

  it("infra / 根包 [features] default 含 test-utils（含转发链） → 红", () => {
    const infraDefault = makeCrateFixture({
      memberManifests: {
        "crates/infra":
          '[package]\nname = "ledger-infra"\nversion = "0.6.0"\nedition = "2024"\n\n' +
          '[features]\ntest-utils = []\ndefault = ["test-utils"]\n\n[lints]\nworkspace = true\n',
      },
    });
    expect(run(infraDefault).output).toContain("default 包含 test-utils");
    const rootDefault = makeCrateFixture({ rootFeaturesExtra: 'default = ["test-utils"]' });
    expect(run(rootDefault).output).toContain("default 包含 test-utils");
    const forwarded = makeCrateFixture({
      rootFeaturesExtra: 'default = ["devkit"]\ndevkit = ["ledger-infra/test-utils"]',
    });
    expect(run(forwarded).output).toContain("default 包含 test-utils");
  });
});

describe("check-structure 投资五节锚点生产编译门（issue #1185）", () => {
  it("真实仓库与骨架夹具默认通过：常量与再导出均带「放行测试」cfg 门", () => {
    expect(run([]).output).toContain("投资五节锚点生产编译门");
    expect(run(makeCrateFixture()).status).toBe(0);
  });

  it("锚点常量摘掉 cfg 门 → 红（删除 cfg 门即变红）", () => {
    const args = makeCrateFixture({
      apiServerImportRs:
        '#[doc(hidden)]\npub const INVESTMENT_SECTION_HEADERS: [&str; 5] = ["## 节"];\n',
    });
    const r = run(args);
    expect(r.status).toBe(1);
    expect(r.output).toContain("投资五节锚点生产编译门");
    expect(r.output).toContain("cfg 门");
  });

  it("api_server 再导出摘掉 cfg 门 → 红（测试锚点会静默进生产二进制）", () => {
    const args = makeCrateFixture({
      apiServerModRs: "pub use handlers::import::INVESTMENT_SECTION_HEADERS;\n",
    });
    const r = run(args);
    expect(r.status).toBe(1);
    expect(r.output).toContain("投资五节锚点生产编译门");
    expect(r.output).toContain("放行测试");
  });

  it("锚点声明被删除 → 红（两层锁共享面不可无声明消失）", () => {
    const args = makeCrateFixture({ apiServerImportRs: "pub fn stub() {}\n" });
    const r = run(args);
    expect(r.status).toBe(1);
    expect(r.output).toContain("投资五节锚点生产编译门");
    expect(r.output).toContain("找不到");
  });
});

describe("check-structure http 投影 feature 门（ADR-0111 决策 5 / issue #1133）", () => {
  it("真实仓库与骨架夹具默认通过：axum optional + http 门 + default 不含 http + 域侧不启用", () => {
    expect(run([]).output).toContain("http 投影 feature 门");
    expect(run(makeCrateFixture()).status).toBe(0);
  });

  it("infra axum 依赖摘掉 optional → 红（裸依赖即无条件编入 axum）", () => {
    const args = makeCrateFixture({
      memberManifests: {
        "crates/infra":
          '[package]\nname = "ledger-infra"\nversion = "0.6.0"\nedition = "2024"\n\n' +
          '[features]\nhttp = ["dep:axum"]\n\n' +
          '[dependencies]\naxum = "0.8"\n\n[lints]\nworkspace = true\n',
      },
    });
    const r = run(args);
    expect(r.status).toBe(1);
    expect(r.output).toContain("http 投影 feature 门");
    expect(r.output).toContain("optional");
  });

  it("http feature 未转发 dep:axum → 红（门与依赖面脱钩，门形同虚设）", () => {
    const args = makeCrateFixture({
      memberManifests: {
        "crates/infra":
          '[package]\nname = "ledger-infra"\nversion = "0.6.0"\nedition = "2024"\n\n' +
          "[features]\nhttp = []\n\n" +
          '[dependencies]\naxum = { version = "0.8", optional = true }\n\n[lints]\nworkspace = true\n',
      },
    });
    const r = run(args);
    expect(r.status).toBe(1);
    expect(r.output).toContain("dep:axum");
  });

  it("error.rs impl 摘掉 cfg 门 → 红（删除 cfg 门即变红）", () => {
    const args = makeCrateFixture({
      infraErrorRs:
        "pub struct AppError;\nimpl axum::response::IntoResponse for AppError {\n    fn into_response(self) -> axum::response::Response {}\n}\n",
    });
    const r = run(args);
    expect(r.status).toBe(1);
    expect(r.output).toContain("http 投影 feature 门");
    expect(r.output).toContain("cfg 门");
  });

  it("impl 前只有普通注释 → 仍红（注释不构成门）", () => {
    const args = makeCrateFixture({
      infraErrorRs:
        "pub struct AppError;\n// 仅注释说明，不构成门\nimpl axum::response::IntoResponse for AppError {\n    fn into_response(self) -> axum::response::Response {}\n}\n",
    });
    expect(run(args).status).toBe(1);
  });

  it('反向门 #[cfg(not(feature = "http"))] → 红（feature 开启反而消失）', () => {
    const args = makeCrateFixture({
      infraErrorRs:
        'pub struct AppError;\n#[cfg(not(feature = "http"))]\nimpl axum::response::IntoResponse for AppError {\n    fn into_response(self) -> axum::response::Response {}\n}\n',
    });
    const r = run(args);
    expect(r.status).toBe(1);
    expect(r.output).toContain("http 投影 feature 门");
  });

  it("rustfmt 拆行的多行 cfg 门 → 绿（属性链解析读到配对闭合为止，#1469）", () => {
    const args = makeCrateFixture({
      infraErrorRs:
        'pub struct AppError;\n#[cfg(all(\n    feature = "http",\n    target_os = "macOS",\n))]\nimpl axum::response::IntoResponse for AppError {\n    fn into_response(self) -> axum::response::Response {}\n}\n',
    });
    expect(run(args).status).toBe(0);
  });

  it("多行反向门 #[cfg(not(…))] → 仍红（多行解析不弱化反向门判定，#1469）", () => {
    const args = makeCrateFixture({
      infraErrorRs:
        'pub struct AppError;\n#[cfg(not(\n    feature = "http",\n))]\nimpl axum::response::IntoResponse for AppError {\n    fn into_response(self) -> axum::response::Response {}\n}\n',
    });
    expect(run(args).status).toBe(1);
  });

  it("infra / 根包 [features] default 含 http → 红（默认 feature 即生产编入）", () => {
    const infraDefault = makeCrateFixture({
      memberManifests: {
        "crates/infra":
          '[package]\nname = "ledger-infra"\nversion = "0.6.0"\nedition = "2024"\n\n' +
          '[features]\nhttp = ["dep:axum"]\ndefault = ["http"]\n\n' +
          '[dependencies]\naxum = { version = "0.8", optional = true }\n\n[lints]\nworkspace = true\n',
      },
    });
    expect(run(infraDefault).output).toContain("default 包含 http");
    const rootDefault = makeCrateFixture({ rootFeaturesExtra: 'default = ["http"]' });
    expect(run(rootDefault).output).toContain("default 包含 http");
  });

  it("域侧成员生产依赖对 ledger-infra 启用 http → 红（域侧引入 axum）", () => {
    const args = makeCrateFixture({
      memberManifests: {
        "crates/sync-protocol":
          '[package]\nname = "ledger-sync-protocol"\nversion = "0.6.0"\nedition = "2024"\n\n' +
          '[dependencies]\nledger-infra = { path = "../infra", features = ["http"] }\n\n[lints]\nworkspace = true\n',
      },
    });
    const r = run(args);
    expect(r.status).toBe(1);
    expect(r.output).toContain("http 投影 feature 门");
    expect(r.output).toContain("域侧");
  });

  it("域侧成员直接声明 axum 生产依赖 → 红（不只经 ledger-infra/http 一条路）", () => {
    const args = makeCrateFixture({
      memberManifests: {
        "crates/sync-protocol":
          '[package]\nname = "ledger-sync-protocol"\nversion = "0.6.0"\nedition = "2024"\n\n' +
          '[dependencies]\nledger-infra = { path = "../infra" }\naxum = "0.8"\n\n[lints]\nworkspace = true\n',
      },
    });
    const r = run(args);
    expect(r.status).toBe(1);
    expect(r.output).toContain("直接声明 axum");
  });

  it("域侧成员仅 dev-dependencies 声明 axum → 绿（测试专用边不在此限）", () => {
    const args = makeCrateFixture({
      memberManifests: {
        "crates/sync-protocol":
          '[package]\nname = "ledger-sync-protocol"\nversion = "0.6.0"\nedition = "2024"\n\n' +
          '[dependencies]\nledger-infra = { path = "../infra" }\n\n' +
          '[dev-dependencies]\naxum = "0.8"\n\n[lints]\nworkspace = true\n',
      },
    });
    expect(run(args).status).toBe(0);
  });
});

describe("maskNonCode 双源防漂移语料（issue #1433）", () => {
  // 与 Rust 侧唯一实现（src-tauri/src/test_support/scan.rs 的 mask_non_code）
  // 消费同一夹具：语料输入 + 期望输出双文件共享，任一侧单独改词法规则即
  // 本测或 Rust 语料测试红。（vitest 下取进程 cwd = 仓库根定位夹具，同上款先例）
  const corpusPath = join(process.cwd(), "scripts", "fixtures", "rust-mask-corpus.rs");
  const expectedPath = join(process.cwd(), "scripts", "fixtures", "rust-mask-corpus.expected.txt");

  it("掩码输出与共享语料期望全等（与 Rust 侧 mask_non_code 同规）", () => {
    const corpus = readFileSync(corpusPath, "utf8");
    const expected = readFileSync(expectedPath, "utf8");
    expect(maskNonCode(corpus)).toBe(expected);
  });

  it("keepLiterals=true 只掩注释、保留字符串字面量（TS 侧扩展形态，语料外的直接断言）", () => {
    const src = '// comment\nlet s = "keep";\n';
    expect(maskNonCode(src, true)).toBe('          \nlet s = "keep";\n');
  });
});

describe("守门家族共享扫描单点：命中行定位 lineAt + 文本面遍历 walkTextFiles（issue #1625）", () => {
  // 与 maskNonCode 同址的家族共享面（供 check-infra-dml / check-eastmoney-residue
  // 消费）：行号定位与目录遍历收口单点，扩展名闭集与豁免面属各守门政策经参数注入。

  it("lineAt：按命中点之前的换行数计行（1 起算），index undefined 按文首计", () => {
    const source = ["const a = 1;", "", "const url = 'https://x.example';"].join("\n");
    const m = /x\.example/.exec(source);
    expect(m).not.toBeNull();
    expect(lineAt(source, m?.index)).toBe(3);
    expect(lineAt("abc def", undefined)).toBe(1);
    expect(lineAt("abc def", 0)).toBe(1);
  });

  it("walkTextFiles：扩展名闭集过滤 + 目录名剪枝 + 相对路径剪枝，排序确定", () => {
    const dir = mkdtempSync(join(tmpdir(), "walk-text-files-"));
    tempDirs.push(dir);
    mkdirSync(join(dir, "src", "nested"), { recursive: true });
    mkdirSync(join(dir, "node_modules"), { recursive: true });
    writeFileSync(join(dir, "CHANGELOG.md"), "# history\n");
    writeFileSync(join(dir, "README"), "无扩展名文件不参与文本面");
    writeFileSync(join(dir, "src", "b.ts"), "export const b = 1;\n");
    writeFileSync(join(dir, "src", "skip-me.ts"), "export const s = 1;\n");
    writeFileSync(join(dir, "src", "nested", "a.rs"), "pub fn a() {}\n");
    writeFileSync(join(dir, "src", "nested", "a.rs.bak"), "junk");
    writeFileSync(join(dir, "node_modules", "dep.ts"), "export const dep = 1;\n");

    const options = {
      extensions: new Set([".ts", ".rs", ".md"]),
      skipDirs: new Set(["node_modules"]),
      skipFiles: new Set(["src/skip-me.ts"]),
    };
    const files = walkTextFiles(dir, "", options);
    expect(files.map((f) => f.rel)).toEqual(["CHANGELOG.md", "src/b.ts", "src/nested/a.rs"]);
    // abs 与 rel 指向同一文件（读文件用 abs、报文定位用 rel）
    expect(files[2].abs).toBe(join(dir, "src", "nested", "a.rs"));
    // 排序确定性：同一目录重复调用输出全等
    expect(walkTextFiles(dir, "", options).map((f) => f.rel)).toEqual(files.map((f) => f.rel));
  });

  it("walkTextFiles：relBase 前缀归一 rel 路径（扫描根 ≠ 仓库根的守门形态）", () => {
    const dir = mkdtempSync(join(tmpdir(), "walk-text-files-relbase-"));
    tempDirs.push(dir);
    // check-infra-dml 形态：扫描根 = join(srcTauri, INFRA_SRC_REL)，relBase 同值——
    // rel 不随夹具所在绝对路径漂移，始终以声明前缀归一。
    const scanRoot = join(dir, "crates", "infra", "src");
    mkdirSync(join(scanRoot, "db"), { recursive: true });
    writeFileSync(join(scanRoot, "db", "migrate.rs"), "pub fn migrate() {}\n");

    const files = walkTextFiles(scanRoot, INFRA_SRC_REL, {
      extensions: new Set([".rs"]),
    });
    expect(files.map((f) => f.rel)).toEqual(["crates/infra/src/db/migrate.rs"]);
  });
});
