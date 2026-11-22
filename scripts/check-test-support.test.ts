import { afterAll, describe, expect, it } from "bun:test";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { FIXED_NOW_REGISTRY_REL } from "./check-test-support.ts";
import { gateScript, runGateScript } from "./run-gate-script.test-helper.ts";

const script = gateScript("check-test-support.ts");
const run = (args: string[] = []) => runGateScript(script, args);

const tempDirs: string[] = [];
afterAll(() => {
  for (const dir of tempDirs) rmSync(dir, { recursive: true, force: true });
});

const FIXTURE_NOW = "2031-03-04T05:06:07Z";
const FIXTURE_NOW_REVISED = "2032-08-09T10:11:12Z";

const fixedNowDecl = (value: string) => `pub const FIXED_NOW: &str = "${value}";`;

const SEED_RS = [
  "// 测试夹具：统一测试数据库工厂种子登记处（形状同构 src/test_support/seed.rs）",
  'pub fn open() { let _ = "INSERT INTO accounts"; }',
  'pub fn seed_instrument() { let _ = "INSERT INTO instruments"; }',
].join("\n");

function makeFixture(
  files: Record<string, string>,
  registry: string | null = fixedNowDecl(FIXTURE_NOW),
): string {
  const dir = mkdtempSync(join(tmpdir(), "check-test-support-"));
  tempDirs.push(dir);
  mkdirSync(join(dir, "src", "test_support"), { recursive: true });
  writeFileSync(join(dir, "src", "test_support", "seed.rs"), SEED_RS);
  if (registry !== null) {
    mkdirSync(dirname(join(dir, FIXED_NOW_REGISTRY_REL)), { recursive: true });
    writeFileSync(join(dir, FIXED_NOW_REGISTRY_REL), registry);
  }
  for (const [name, content] of Object.entries(files)) {
    mkdirSync(dirname(join(dir, name)), { recursive: true });
    writeFileSync(join(dir, name), content);
  }
  return dir;
}

describe("check-test-support（Rust 测试守门，纯禁令）", () => {
  it("默认扫描本仓（门槛路径）：纯禁令全绿（#758 收口后存量清零）", () => {
    const r = run();
    expect(r.status).toBe(0);
    expect(r.output).toContain("Rust 测试守门通过");
    expect(r.output).toContain("纯禁令");
    expect(r.output).toContain("禁用种子表 6 张");
  });

  it("四条规则违规样本全部命中：直连建库、夹具裸 SQL、默认时刻字面量、自建通道线格式", () => {
    const dir = makeFixture({
      "src/ledger/tests.rs": [
        "use crate::db::{init_db, open_in_memory};",
        "fn t() {",
        "  let mut c = open_in_memory().unwrap();",
        "  init_db(&mut c).unwrap();",
        '  let sql = "INSERT INTO accounts (id) VALUES (1)";',
        `  let stamp = "${FIXTURE_NOW}";`,
        "}",
      ].join("\n"),
      "src/ledger/tests/state.rs": "fn t() { let s = DbState::open_in_memory().unwrap(); }",
      "tests/api_server/common.rs":
        'fn seed() { let _ = "INSERT INTO instruments (id) VALUES (1)"; }',
      "tests/commands/channel.rs": [
        "use crate::sync_engine::{ChannelManifest, SegmentEntry, EnvelopeMode};",
        "fn t() {",
        "  let m = ChannelManifest { version: 1, streams: vec![], checkpoint: None };",
        '  let e = SegmentEntry { file: "x".into(), first_clock: 1, last_clock: 1, size: 1, sha256: "y".into() };',
        "  use sha2::{Digest, Sha256};",
        '  let _ = Sha256::digest(b"z");',
        "  let _ = (m, e);",
        "}",
      ].join("\n"),
      "src/ledger/perf/tests.rs": [
        "use sha2::Digest;",
        "fn digest_db() { let mut h = sha2::Sha256::new(); let _ = &mut h; }",
      ].join("\n"),
    });
    const r = run([dir]);
    expect(r.status).toBe(1);
    expect(r.output).toContain("src/ledger/tests.rs");
    expect(r.output).toContain("规则 1（直连建库）命中 4 处");
    expect(r.output).toContain("规则 2（夹具裸SQL）命中 1 处");
    expect(r.output).toContain("规则 3（默认时刻字面量）命中 1 处");
    expect(r.output).toContain("src/ledger/tests/state.rs");
    expect(r.output).toContain("tests/api_server/common.rs");
    expect(r.output).toContain("规则 4（自建通道线格式）命中 5 处");
    expect(r.output).toContain("tests/commands/channel.rs");
    expect(r.output).not.toContain("src/ledger/perf/tests.rs");
  });

  it("合法形态不误报：工厂本体、域薄皮种子 SQL、域时刻字面量、db 产品代码", () => {
    const dir = makeFixture({
      "src/test_support/mod.rs": [
        fixedNowDecl(FIXTURE_NOW),
        "fn t() { db::open_in_memory(); db::init_db(); }",
        'fn seed() { let _ = "INSERT INTO accounts (id) VALUES (1)"; }',
      ].join("\n"),
      "src/db/mod.rs": "fn open_db() { crate::db::open_in_memory(); crate::db::init_db(); }",
      "src/ledger/tests/common.rs":
        'fn seed_local() { let _ = "INSERT INTO accounts (id) VALUES (1)"; }',
      "src/ledger/tests/trend.rs": 'fn t() { let at = "2026-01-15T12:00:00Z"; }',
      "src/ledger/tests/commented.rs":
        "// db::open_in_memory() 与 INSERT INTO instruments 已迁工厂",
    });
    const r = run([dir]);
    expect(r.status).toBe(0);
    expect(r.output).toContain("Rust 测试守门通过");
  });

  it("产品文件只扫内联 #[cfg(test)] 块：块内违规命中，产品本体同形态不误报", () => {
    const dir = makeFixture({
      "src/ledger/core.rs": [
        "fn open_db() { crate::db::open_in_memory(); crate::db::init_db(); }",
        'fn seed() { let _ = "INSERT INTO instruments (id) VALUES (1)"; }',
        `const T0: &str = "${FIXTURE_NOW}";`,
        "",
        "#[cfg(test)]",
        "mod tests {",
        "  #[test]",
        "  fn t() {",
        "    let mut c = crate::db::open_in_memory();",
        "    crate::db::init_db(&mut c);",
        '    let _ = r#"INSERT INTO instruments"#;',
        `    let stamp = "${FIXTURE_NOW}";`,
        "  }",
        "}",
      ].join("\n"),
    });
    const r = run([dir]);
    expect(r.status).toBe(1);
    expect(r.output).toContain("src/ledger/core.rs");
    expect(r.output).toContain("规则 1（直连建库）命中 2 处");
    expect(r.output).toContain("规则 2（夹具裸SQL）命中 1 处");
    expect(r.output).toContain("规则 3（默认时刻字面量）命中 1 处");
  });

  it("tests/e2e 规则 2/3 整目录覆盖（#764 恢复）：未登记命中即红；规则 1 不辖（分层形态）", () => {
    const violation = [
      "fn t() {",
      "  let mut c = crate::db::open_in_memory();",
      "  crate::db::init_db(&mut c);",
      '  let _ = "INSERT INTO accounts (id) VALUES (1)";',
      "}",
    ].join("\n");
    const dir = makeFixture({
      "tests/e2e/steps.rs": violation,
      "tests/api_server/common.rs": violation,
    });
    const r = run([dir]);
    expect(r.status).toBe(1);
    expect(r.output).toContain("tests/api_server/common.rs");
    expect(r.output).toContain("tests/e2e/steps.rs");
    expect(r.output).toContain("规则 2（夹具裸SQL）命中 1 处——未登记例外");
    expect(r.output).toContain("登记处 ADR-0086 修订注记");
  });

  it("tests/e2e 已登记例外严格相等校验：命中数漂移即红、命中清零即红", () => {
    const violation = [
      "fn t() {",
      "  let mut c = crate::db::open_in_memory();",
      "  crate::db::init_db(&mut c);",
      '  let _ = "INSERT INTO instruments (id) VALUES (1)";',
      `  let _ = "${FIXTURE_NOW}";`,
      "}",
    ].join("\n");
    const dir = makeFixture({
      "tests/e2e/instruments_steps.rs": violation,
      "tests/e2e/investment_trend_steps.rs": "fn t() {}",
      "tests/e2e/transactions_query_steps.rs": "fn t() {}",
    });
    const r = run([dir]);
    expect(r.status).toBe(1);
    expect(r.output).toContain("实际命中 1 处 ≠ 登记的 3 处");
    expect(r.output).toContain("规则 3（默认时刻字面量）命中 1 处——未登记例外");
    expect(r.output).toContain("已登记例外 1 处实际命中 0 处——例外已收敛");
  });

  it("规则 3 时刻值以登记处为单一来源（#1468）：登记处改值禁令自动跟随", () => {
    const revised = makeFixture(
      { "src/ledger/tests.rs": `fn t() { let stamp = "${FIXTURE_NOW_REVISED}"; }` },
      fixedNowDecl(FIXTURE_NOW_REVISED),
    );
    const red = run([revised]);
    expect(red.status).toBe(1);
    expect(red.output).toContain(`禁用时刻值：${FIXTURE_NOW_REVISED}`);
    expect(red.output).toContain("规则 3（默认时刻字面量）命中 1 处");
    const stale = makeFixture(
      { "src/ledger/tests.rs": `fn t() { let stamp = "${FIXTURE_NOW}"; }` },
      fixedNowDecl(FIXTURE_NOW_REVISED),
    );
    const green = run([stale]);
    expect(green.status).toBe(0);
    expect(green.output).toContain("Rust 测试守门通过");
  });

  it("固定时刻登记处缺失或提不出现值即 fail loud（#1468）：禁令不静默失效", () => {
    const missing = makeFixture({}, null);
    const r1 = run([missing]);
    expect(r1.status).toBe(1);
    expect(r1.output).toContain("固定时刻登记处缺失");
    const drained = makeFixture({}, "// 登记处已搬空：没有任何 pub const 声明可提取");
    const r2 = run([drained]);
    expect(r2.status).toBe(1);
    expect(r2.output).toContain("提不出任何");
  });
});
