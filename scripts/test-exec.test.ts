import { afterAll, describe, expect, it } from "bun:test";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { gateScript, runGateScript } from "./run-gate-script.test-helper.ts";

const script = gateScript("test-exec.ts");
const run = (args: string[]) => runGateScript(script, args);

const tempDirs: string[] = [];
afterAll(() => {
  for (const dir of tempDirs) rmSync(dir, { recursive: true, force: true });
});

interface FixtureOverrides {
  rootManifest?: string;
  alphaManifest?: string;
  testSh?: string;
  files?: Record<string, string>;
  withoutLib?: boolean;
}

const DEFAULT_TEST_SH = [
  "#!/bin/sh",
  "set -eu",
  "# 入口自检先行（真实 scripts/test.sh 同址接线）",
  "bun scripts/test-exec.ts check",
  "bun scripts/test-exec.ts",
  "( cd src-tauri && cargo test --workspace --test e2e )",
  "( cd src-tauri && cargo test --workspace --doc )",
  "",
].join("\n");

function makeFixture(overrides: FixtureOverrides = {}): string {
  const root = mkdtempSync(join(tmpdir(), "test-exec-"));
  tempDirs.push(root);
  const srcTauri = join(root, "src-tauri");
  const alpha = join(srcTauri, "crates", "alpha");
  mkdirSync(join(alpha, "src"), { recursive: true });
  mkdirSync(join(alpha, "tests"), { recursive: true });
  mkdirSync(join(srcTauri, "src"), { recursive: true });
  mkdirSync(join(root, "scripts"), { recursive: true });

  writeFileSync(
    join(srcTauri, "Cargo.toml"),
    overrides.rootManifest ??
      [
        "[package]",
        'name = "root-app"',
        'version = "0.0.0"',
        'edition = "2021"',
        "",
        "[workspace]",
        'members = ["crates/*"]',
        "",
      ].join("\n"),
  );
  if (overrides.withoutLib !== true) {
    writeFileSync(join(srcTauri, "src", "lib.rs"), "pub fn boot() {}\n");
    writeFileSync(join(alpha, "src", "lib.rs"), "pub fn alpha() {}\n");
  }
  writeFileSync(
    join(alpha, "Cargo.toml"),
    overrides.alphaManifest ??
      [
        "[package]",
        'name = "alpha"',
        'version = "0.0.0"',
        'edition = "2021"',
        "",
        "[[test]]",
        'name = "e2e"',
        'path = "tests/e2e.rs"',
        "harness = false",
        "",
      ].join("\n"),
  );
  writeFileSync(join(alpha, "tests", "e2e.rs"), "fn main() {}\n");
  writeFileSync(join(alpha, "tests", "api.rs"), "#[test]\nfn api() {}\n");
  writeFileSync(join(root, "scripts", "test.sh"), overrides.testSh ?? DEFAULT_TEST_SH);
  for (const [rel, content] of Object.entries(overrides.files ?? {})) {
    const abs = join(root, rel);
    mkdirSync(dirname(abs), { recursive: true });
    writeFileSync(abs, content);
  }
  return root;
}

const ROOT_WITH_TEST_FALSE_BIN = [
  "[package]",
  'name = "root-app"',
  'version = "0.0.0"',
  'edition = "2021"',
  "",
  "[workspace]",
  'members = ["crates/*"]',
  "",
  "[[bin]]",
  'name = "root-app"',
  'path = "src/main.rs"',
  "test = false",
  "",
].join("\n");

const ROOT_NAMED_TAURI_APP = [
  "[package]",
  'name = "tauri-app"',
  'version = "0.0.0"',
  'edition = "2021"',
  "",
  "[workspace]",
  'members = ["crates/*"]',
  "",
].join("\n");

describe("测试执行器两入口覆盖守门（issue #1112）", () => {
  it("合规夹具：目标清单与两入口并集全等 → 通过", () => {
    const r = run(["check", "--root", makeFixture()]);
    expect(r.status).toBe(0);
    expect(r.output).toContain("目标 6 个");
    expect(r.output).toContain("并发入口 3");
    expect(r.output).toContain("集成测试 1");
    expect(r.output).toContain("e2e");
  });

  it("同包内 lib 与集成 target 同名不撞键（执行面核对键含 kind）", () => {
    const r = run([
      "check",
      "--root",
      makeFixture({
        files: { "src-tauri/crates/alpha/tests/alpha.rs": "#[test]\nfn same_name() {}\n" },
      }),
    ]);
    expect(r.status).toBe(0);
    expect(r.output).toContain("目标 7 个");
    expect(r.output).toContain("lib 单测 2");
    expect(r.output).toContain("集成测试 2");
  });

  it("新增 harness=false 测试目标未登记非并发入口 → 红（目标静默漏跑即红）", () => {
    const r = run([
      "check",
      "--root",
      makeFixture({
        alphaManifest: [
          "[package]",
          'name = "alpha"',
          'version = "0.0.0"',
          'edition = "2021"',
          "",
          "[[test]]",
          'name = "e2e"',
          'path = "tests/e2e.rs"',
          "harness = false",
          "",
          "[[test]]",
          'name = "custom"',
          'path = "tests/custom.rs"',
          "harness = false",
          "",
        ].join("\n"),
        files: { "src-tauri/crates/alpha/tests/custom.rs": "fn main() {}\n" },
      }),
    ]);
    expect(r.status).toBe(1);
    expect(r.output).toContain("alpha::custom");
    expect(r.output).toContain("静默漏跑");
  });

  it("非并发入口未登记 e2e（删 --test e2e）→ 红", () => {
    const r = run([
      "check",
      "--root",
      makeFixture({
        testSh: [
          "#!/bin/sh",
          "set -eu",
          "bun scripts/test-exec.ts",
          "( cd src-tauri && cargo test --workspace --doc )",
          "",
        ].join("\n"),
      }),
    ]);
    expect(r.status).toBe(1);
    expect(r.output).toContain("alpha::e2e");
    expect(r.output).toContain("--test e2e");
  });

  it("非并发入口登记了非自定义 harness 目标（--test api 漂移）→ 红", () => {
    const r = run([
      "check",
      "--root",
      makeFixture({
        testSh: [
          "#!/bin/sh",
          "set -eu",
          "bun scripts/test-exec.ts",
          "( cd src-tauri && cargo test --workspace --test e2e )",
          "( cd src-tauri && cargo test --workspace --test api )",
          "( cd src-tauri && cargo test --workspace --doc )",
          "",
        ].join("\n"),
      }),
    ]);
    expect(r.status).toBe(1);
    expect(r.output).toContain("--test api");
    expect(r.output).toContain("不对应任何 harness = false 目标");
  });

  it("缺 cargo test --doc → 红（doc-test 静默漏跑即红）", () => {
    const r = run([
      "check",
      "--root",
      makeFixture({
        testSh: [
          "#!/bin/sh",
          "set -eu",
          "bun scripts/test-exec.ts",
          "( cd src-tauri && cargo test --workspace --test e2e )",
          "",
        ].join("\n"),
      }),
    ]);
    expect(r.status).toBe(1);
    expect(r.output).toContain("doc-test");
  });

  it("doc 入口退化成 `cargo test --doc`（缺 workspace 范围）→ 红", () => {
    const r = run([
      "check",
      "--root",
      makeFixture({
        testSh: [
          "#!/bin/sh",
          "set -eu",
          "bun scripts/test-exec.ts",
          "( cd src-tauri && cargo test --workspace --test e2e )",
          "( cd src-tauri && cargo test --doc )",
          "",
        ].join("\n"),
      }),
    ]);
    expect(r.status).toBe(1);
    expect(r.output).toContain("缺 workspace 范围");
    expect(r.output).toContain("doc-test 静默漏跑");
  });

  it("执行器等价性：测试运行期读 cargo 注入环境变量 → 红（fail loud）", () => {
    const r = run([
      "check",
      "--root",
      makeFixture({
        files: {
          "src-tauri/src/reads_manifest.rs":
            'pub fn root() -> String { std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default() }\n',
        },
      }),
    ]);
    expect(r.status).toBe(1);
    expect(r.output).toContain("执行器等价性");
    expect(r.output).toContain("CARGO_MANIFEST_DIR");
  });

  it("执行器等价性：注释里提到变量名不算命中（注释掩码，不假红）", () => {
    const r = run([
      "check",
      "--root",
      makeFixture({
        files: {
          "src-tauri/src/notes.rs":
            '// 约定：测试运行期不得写 std::env::var("CARGO_MANIFEST_DIR")\n' +
            '/// 同上：env::var("OUT_DIR")\n' +
            "pub fn ok() {}\n",
        },
      }),
    ]);
    expect(r.status).toBe(0);
  });

  it("工作区无 doc-test 目标却在非并发入口登记 --doc → 红（登记失效）", () => {
    const r = run([
      "check",
      "--root",
      makeFixture({
        withoutLib: true,
        alphaManifest: [
          "[package]",
          'name = "alpha"',
          'version = "0.0.0"',
          'edition = "2021"',
          "",
          "[[test]]",
          'name = "e2e"',
          'path = "tests/e2e.rs"',
          "harness = false",
          "",
        ].join("\n"),
      }),
    ]);
    expect(r.status).toBe(1);
    expect(r.output).toContain("无 doc-test 目标");
  });

  it("scripts/test.sh 删掉并发入口调用 → 红（接线删除即红）", () => {
    const r = run([
      "check",
      "--root",
      makeFixture({
        testSh: [
          "#!/bin/sh",
          "set -eu",
          "( cd src-tauri && cargo test --workspace --test e2e )",
          "( cd src-tauri && cargo test --workspace --doc )",
          "",
        ].join("\n"),
      }),
    ]);
    expect(r.status).toBe(1);
    expect(r.output).toContain("未调用并发入口");
  });

  it("scripts/test.sh 三条入口全被 echo 包成说明文字 → 红（说明文字不算命令位置）", () => {
    const r = run([
      "check",
      "--root",
      makeFixture({
        testSh: [
          "#!/bin/sh",
          "set -eu",
          "bun scripts/test-exec.ts check",
          'echo "bun scripts/test-exec.ts"',
          'echo "( cd src-tauri && cargo test --workspace --test e2e )"',
          'echo "( cd src-tauri && cargo test --workspace --doc )"',
          "",
        ].join("\n"),
      }),
    ]);
    expect(r.status).toBe(1);
    expect(r.output).toContain("静默漏跑");
    expect(r.output).toContain("未调用并发入口");
  });

  it("scripts/test.sh 只留自检行 `… check`、运行行被 echo → 红（自检行不算运行接线）", () => {
    const r = run([
      "check",
      "--root",
      makeFixture({
        testSh: [
          "#!/bin/sh",
          "set -eu",
          "bun scripts/test-exec.ts check",
          'echo "bun scripts/test-exec.ts"',
          "( cd src-tauri && cargo test --workspace --test e2e )",
          "( cd src-tauri && cargo test --workspace --doc )",
          "",
        ].join("\n"),
      }),
    ]);
    expect(r.status).toBe(1);
    expect(r.output).toContain("未调用并发入口");
  });

  it("scripts/test.sh 合法前缀写法（command / if / VAR=x）仍算运行接线 → 不假红", () => {
    for (const parallelLine of [
      "command bun scripts/test-exec.ts",
      "if bun scripts/test-exec.ts; then :; fi",
      "LEDGER_JOBS=4 bun scripts/test-exec.ts",
    ]) {
      const r = run([
        "check",
        "--root",
        makeFixture({
          testSh: [
            "#!/bin/sh",
            "set -eu",
            parallelLine,
            "( cd src-tauri && cargo test --workspace --test e2e )",
            "( cd src-tauri && cargo test --workspace --doc )",
            "",
          ].join("\n"),
        }),
      ]);
      expect(r.status, parallelLine).toBe(0);
    }
  });

  it("doc 入口用 `--all` 别名（`cargo test --all --doc`）→ 不假红", () => {
    const r = run([
      "check",
      "--root",
      makeFixture({
        testSh: [
          "#!/bin/sh",
          "set -eu",
          "bun scripts/test-exec.ts",
          "( cd src-tauri && cargo test --workspace --test e2e )",
          "( cd src-tauri && cargo test --all --doc )",
          "",
        ].join("\n"),
      }),
    ]);
    expect(r.status).toBe(0);
  });

  it("manifest 出现 auto* 开关（发现面被改写）→ 红（拒绝猜测）", () => {
    const r = run([
      "check",
      "--root",
      makeFixture({
        alphaManifest: [
          "[package]",
          'name = "alpha"',
          'version = "0.0.0"',
          'edition = "2021"',
          "autotests = false",
          "",
          "[[test]]",
          'name = "e2e"',
          'path = "tests/e2e.rs"',
          "harness = false",
          "",
        ].join("\n"),
      }),
    ]);
    expect(r.status).toBe(1);
    expect(r.output).toContain("autotests");
  });

  it("members 出现未支持的 glob 形态 → 红", () => {
    const r = run([
      "check",
      "--root",
      makeFixture({
        rootManifest: [
          "[package]",
          'name = "root-app"',
          'version = "0.0.0"',
          'edition = "2021"',
          "",
          "[workspace]",
          'members = ["crates/**"]',
          "",
        ].join("\n"),
      }),
    ]);
    expect(r.status).toBe(1);
    expect(r.output).toContain("未支持的 members glob");
  });

  it("bin test = false 且源码零单测 → 绿（bin 不入目标清单）", () => {
    const r = run([
      "check",
      "--root",
      makeFixture({
        rootManifest: ROOT_WITH_TEST_FALSE_BIN,
        files: { "src-tauri/src/main.rs": "fn main() {}\n" },
      }),
    ]);
    expect(r.status).toBe(0);
    expect(r.output).toContain("目标 6 个");
    expect(r.output).toContain("bin 单测 0");
  });

  it("bin test = false 但源码新增 #[test] → 红（静默漏跑即红）", () => {
    const r = run([
      "check",
      "--root",
      makeFixture({
        rootManifest: ROOT_WITH_TEST_FALSE_BIN,
        files: {
          "src-tauri/src/main.rs": "#[test]\nfn stray() {}\n\nfn main() {}\n",
        },
      }),
    ]);
    expect(r.status).toBe(1);
    expect(r.output).toContain("test = false");
    expect(r.output).toContain("静默漏跑");
  });

  it("bin test = false 源码注释里提到 #[test] → 不假红（注释掩码）", () => {
    const r = run([
      "check",
      "--root",
      makeFixture({
        rootManifest: ROOT_WITH_TEST_FALSE_BIN,
        files: {
          "src-tauri/src/main.rs": "// 不要在 bin 里加 #[test] 或 #[cfg(test)]\nfn main() {}\n",
        },
      }),
    ]);
    expect(r.status).toBe(0);
  });

  it("bin 带单测且未登记豁免 → 红（bin 单测不入并发入口）", () => {
    const r = run([
      "check",
      "--root",
      makeFixture({
        files: { "src-tauri/src/main.rs": "#[test]\nfn unit() {}\n\nfn main() {}\n" },
      }),
    ]);
    expect(r.status).toBe(1);
    expect(r.output).toContain("bin 单测未登记豁免");
    expect(r.output).toContain("root-app::root-app");
  });

  it("豁免登记命中（tauri-app::ledger-perf）→ 出并发入口，summary 标注承接入口", () => {
    const r = run([
      "check",
      "--root",
      makeFixture({
        rootManifest: ROOT_NAMED_TAURI_APP,
        files: {
          "src-tauri/src/bin/ledger-perf/main.rs": "#[test]\nfn gen() {}\n\nfn main() {}\n",
        },
      }),
    ]);
    expect(r.status).toBe(0);
    expect(r.output).toContain("bin 单测豁免 1 个");
    expect(r.output).toContain("ledger-perf → perf-bench.yml 每日基准");
    expect(r.output).toContain("bin 单测 0");
  });

  it("bin 删除后残留的死登记 → 不红（无副作用，summary 不再展示）", () => {
    const r = run(["check", "--root", makeFixture({ rootManifest: ROOT_NAMED_TAURI_APP })]);
    expect(r.status).toBe(0);
    expect(r.output).not.toContain("bin 单测豁免");
  });
});

describe("覆盖守门真实仓库绿基线（删除即变红，观察面 = 守门退出码与输出）", () => {
  it("真实仓库：两条入口 + 等价性守门齐备 → 通过", () => {
    const r = run(["check"]);
    expect(r.status).toBe(0);
    expect(r.output).toContain("测试执行覆盖守门");
  });
});
