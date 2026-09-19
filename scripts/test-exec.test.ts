import { afterAll, describe, expect, it } from "vitest";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { gateScript, runGateScript } from "./run-gate-script.test-helper.ts";

// 被测对象是仓库工具脚本 scripts/test-exec.ts（测试执行器与两入口覆盖守门，
// issue #1112）。脚本以 Bun 运行时执行（ADR-0083）：runGateScript 以
// spawnSync('bun') 与门槛调用同款拉起，测的就是门槛路径。按测试决策只测外部
// 可观察结果——进程退出码与输出，不测内部函数；夹具经 `--root` 指向临时工作区
// （只需 manifest + 目录形态，守门不调用 cargo，仓与 CI 上均零依赖）。
const script = gateScript("test-exec.ts");
const run = (args: string[]) => runGateScript(script, args);

const tempDirs: string[] = [];
afterAll(() => {
  for (const dir of tempDirs) rmSync(dir, { recursive: true, force: true });
});

/** 夹具工作区：根包（lib + workspace glob）+ 成员 alpha（lib + 集成测试 api/e2e）。 */
interface FixtureOverrides {
  rootManifest?: string;
  alphaManifest?: string;
  /** test.sh 内容；缺省 = 合规双入口形态。 */
  testSh?: string;
  /** check.sh 内容；缺省 = 含覆盖守门步骤（守门规则④的接线宿主）。 */
  checkSh?: string;
  /** build.yml 内容；缺省 = frontend job 含覆盖守门步骤（守门规则④的接线宿主）。 */
  workflow?: string;
  /** 追加的成员文件（相对夹具根，含目录）。 */
  files?: Record<string, string>;
  /** 为 true 时删除两个 lib 目标（用于 --doc 登记失效夹具）。 */
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

/** check.sh / CI 的守门接线（守门规则④的合规形态）。 */
const DEFAULT_CHECK_SH = ["#!/bin/sh", "set -eu", "bun scripts/test-exec.ts check", ""].join("\n");
const DEFAULT_WORKFLOW = [
  "jobs:",
  "  frontend:",
  "    steps:",
  "      - run: bun scripts/test-exec.ts check",
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
  writeFileSync(join(root, "scripts", "check.sh"), overrides.checkSh ?? DEFAULT_CHECK_SH);
  mkdirSync(join(root, ".github", "workflows"), { recursive: true });
  writeFileSync(
    join(root, ".github", "workflows", "build.yml"),
    overrides.workflow ?? DEFAULT_WORKFLOW,
  );
  for (const [rel, content] of Object.entries(overrides.files ?? {})) {
    const abs = join(root, rel);
    mkdirSync(dirname(abs), { recursive: true });
    writeFileSync(abs, content);
  }
  return root;
}

describe("测试执行器两入口覆盖守门（issue #1112）", () => {
  it("合规夹具：目标清单与两入口并集全等 → 通过", () => {
    const r = run(["check", "--root", makeFixture()]);
    expect(r.status).toBe(0);
    // 6 = 根包 lib + doc、alpha lib + doc、alpha 集成 api + e2e；e2e 归非并发入口。
    expect(r.output).toContain("目标 6 个");
    expect(r.output).toContain("并发入口 3");
    expect(r.output).toContain("集成测试 1");
    expect(r.output).toContain("e2e");
  });

  it("同包内 lib 与集成 target 同名不撞键（执行面核对键含 kind）", () => {
    // cargo 允许同包内 lib 与集成 target 同名；只用「包::名」会把两者合成一个，
    // 静态清单与 run 交叉核对双双假绿、实际少跑一个二进制。
    const r = run([
      "check",
      "--root",
      makeFixture({
        files: { "src-tauri/crates/alpha/tests/alpha.rs": "#[test]\nfn same_name() {}\n" },
      }),
    ]);
    expect(r.status).toBe(0);
    // 7 = 根包 lib + doc、alpha lib + doc、alpha 集成 api / alpha / e2e。
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
    // 只查 `--doc` 存在不够：非虚拟 workspace 下 `cargo test --doc` 只跑根包的
    // doctest，成员 crate 的 doc-test 静默漏跑而守门仍绿（issue #1112 审查发现）。
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

  it("check.sh 删掉 / 换成别的子命令的守门调用 → 红（接线删除即红）", () => {
    const r = run([
      "check",
      "--root",
      makeFixture({
        // 行仍在、但调的是 plan：命令词序不对即判定未接线，不是子串匹配。
        checkSh: ["#!/bin/sh", "set -eu", "bun scripts/test-exec.ts plan", ""].join("\n"),
      }),
    ]);
    expect(r.status).toBe(1);
    expect(r.output).toContain("覆盖守门接线");
    expect(r.output).toContain("check.sh");
  });

  it("CI frontend job 删掉守门步骤 → 红（CI 接线删除即红）", () => {
    const r = run([
      "check",
      "--root",
      makeFixture({
        workflow: ["jobs:", "  frontend:", "    steps:", "      - run: pnpm run build", ""].join(
          "\n",
        ),
      }),
    ]);
    expect(r.status).toBe(1);
    expect(r.output).toContain("覆盖守门接线");
    expect(r.output).toContain("build.yml");
  });

  it("check.sh 只剩 echo 标签行含命令字样 → 红（说明文字不算接线）", () => {
    // 实测教训：真实 check.sh 的命令改成 plan 后，仅剩的 `echo "…（bun
    // scripts/test-exec.ts check）…"` 标签行曾让守门假绿——接线判定要求 `bun`
    // 落在命令位置（剥掉 YAML / 子 shell 装饰后的首个词）。
    const r = run([
      "check",
      "--root",
      makeFixture({
        checkSh: [
          "#!/bin/sh",
          "set -eu",
          'echo "覆盖守门 (bun scripts/test-exec.ts check)"',
          "",
        ].join("\n"),
      }),
    ]);
    expect(r.status).toBe(1);
    expect(r.output).toContain("覆盖守门接线");
  });

  it("check.sh 接线带引号/前置参数仍算接线 → 不假红", () => {
    const r = run([
      "check",
      "--root",
      makeFixture({
        checkSh: ["#!/bin/sh", "set -eu", '  bun "scripts/test-exec.ts" check --jobs 4', ""].join(
          "\n",
        ),
      }),
    ]);
    expect(r.status).toBe(0);
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
    // 相对固定点的强度削弱（#1112 第三轮审查 P1）：旧判定只看「非注释行含
    // scripts/test-exec.ts 子串」与「按空白切词找 --test / --doc」，把三条真命令
    // 全包成 echo "…" 后守门仍绿——而 `./scripts/test.sh` 一条测试都没跑，却因
    // 三条 echo 全成功而退出 0。判据必须落在命令位置。
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
    // 入口自检行与运行行都调 scripts/test-exec.ts：若按子串/词序判「已调用并发入口」，
    // 删掉运行行只留 `check` 也能假绿，本轮把两者分开（run 才算运行接线）。
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
});

describe("测试执行接线（删除即变红，观察面 = 守门退出码与输出）", () => {
  // 本 describe 的断言不钉命令字符串形态（ADR-0087 断言强度）：观察面统一是
  // `bun scripts/test-exec.ts check` 的**退出码与输出**——守门读真实仓库的
  // scripts/test.sh（三入口）、scripts/check.sh 与 CI frontend job（门禁接线）与
  // 运行期环境扫描，任一处接线删除/漂移都让这里变红。各删除形态的「制造变红」
  // 另由上面的夹具用例逐条覆盖（删并发入口、删/漂移 --test、删 --doc、
  // doc 缺 workspace 范围、check.sh 与 CI 接线缺失、运行期读 cargo 注入环境）。
  it("真实仓库：两条入口 + 门禁接线 + 等价性守门齐备 → 通过", () => {
    const r = run(["check"]);
    expect(r.status).toBe(0);
    expect(r.output).toContain("测试执行覆盖守门");
  });

  it("真实仓库接线仍然可见：删掉 scripts/check.sh 的守门调用 → 同一条断言变红", () => {
    // 反向证明观察面对准可观察结果：把真实 check.sh 的接线镜像成夹具缺接线形态，
    // 守门退出码非零；恢复即绿（真实仓库的删除实测见验证文档「负向验收」）。
    const broken = run([
      "check",
      "--root",
      makeFixture({
        checkSh: ["#!/bin/sh", "set -eu", "bun scripts/test-exec.ts run", ""].join("\n"),
      }),
    ]);
    expect(broken.status).toBe(1);
    expect(broken.output).toContain("覆盖守门接线");
  });
});
