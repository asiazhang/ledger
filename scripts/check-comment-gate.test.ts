import { afterAll, describe, expect, it } from "bun:test";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { gateScript, runGateScript } from "./run-gate-script.test-helper.ts";

const script = gateScript("check-comment-gate.ts");
const run = (args: string[]) => runGateScript(script, args);

const tempDirs: string[] = [];
afterAll(() => {
  for (const dir of tempDirs) rmSync(dir, { recursive: true, force: true });
});

interface FixtureFile {
  rel: string;
  content: string;
}

function makeFixture(files: FixtureFile[], baseline: Record<string, number>): string {
  const root = mkdtempSync(join(tmpdir(), "check-comment-gate-"));
  tempDirs.push(root);
  for (const file of files) {
    const abs = join(root, file.rel);
    mkdirSync(join(abs, ".."), { recursive: true });
    writeFileSync(abs, file.content);
  }
  mkdirSync(join(root, "scripts", "fixtures"), { recursive: true });
  writeFileSync(
    join(root, "scripts", "fixtures", "comment-baseline.json"),
    JSON.stringify(baseline, null, 2) + "\n",
  );
  return root;
}

function baselinePathOf(root: string): string {
  return join(root, "scripts", "fixtures", "comment-baseline.json");
}

const RUST_FIXTURE = [
  "#![allow(dead_code)]",
  "",
  "/// 文档注释：示范形态",
  "pub struct Demo;",
  "",
  "// 普通行注释",
  "// SAFETY：unsafe 前提说明",
  "pub fn demo() {",
  '    let s = "// 字面量里的注释字样";',
  "    let t = /* 行内块注释 */ 1;",
  "}",
  "/* 多行块注释起始",
  "多行块注释结束 */",
  "",
].join("\n");

const TS_FIXTURE = [
  "export const a = 1;",
  "// 行注释形态",
  "/* 块注释一行 */",
  "/**",
  " * JSDoc 形态",
  " */",
  'export const b = "// 不是注释的字样";',
  'export const url = "https://example.com";',
  "",
].join("\n");

const VUE_FIXTURE = [
  '<script setup lang="ts">',
  "// vue 行注释形态",
  "/* vue 块注释形态 */",
  "</script>",
  "<!-- HTML 注释形态不入掩码闭集 -->",
  "",
].join("\n");

const SHELL_FIXTURE = [
  "#!/bin/sh",
  "# shell 注释形态",
  'echo "# 字面量井号形态"',
  "echo done # 行尾注释不计数",
  "",
].join("\n");

const CARGO_TOML_FIXTURE = ["[package]", "# Cargo 配置注释形态", 'name = "demo"', ""].join("\n");

const KACHE_TOML_FIXTURE = [
  "# kache 配置注释形态",
  "[cache]",
  "preserve_incremental = true",
  "",
].join("\n");

const PACKAGE_JSON_FIXTURE = [
  "{",
  '  "name": "demo",',
  '  "//": "包定位说明数据键形态",',
  '  "version": "0.0.0"',
  "}",
  "",
].join("\n");

const OTHER_TOML_FIXTURE = ["# 非 Cargo/kache 命名的 toml 不入辖域", "", ""].join("\n");

const LANGUAGE_FIXTURE_FILES: FixtureFile[] = [
  { rel: "src-tauri/crates/demo/src/lib.rs", content: RUST_FIXTURE },
  { rel: "src/ui.ts", content: TS_FIXTURE },
  { rel: "src/App.vue", content: VUE_FIXTURE },
  { rel: "scripts/tool.sh", content: SHELL_FIXTURE },
  { rel: "src-tauri/Cargo.toml", content: CARGO_TOML_FIXTURE },
  { rel: ".kache.toml", content: KACHE_TOML_FIXTURE },
  { rel: "package.json", content: PACKAGE_JSON_FIXTURE },
  { rel: "scripts/other.toml", content: OTHER_TOML_FIXTURE },
];

const ZERO_LANGUAGE_BASELINE: Record<string, number> = {
  ".kache.toml": 0,
  "package.json": 0,
  scripts: 0,
  src: 0,
  "src-tauri": 0,
  "src-tauri/crates/demo": 0,
};

const MAIN_TS_FIXTURE = ["export const x = 1;", "// 第一行注释", "// 第二行注释", ""].join("\n");

const SINGLE_COMMENT_TS = ["export const x = 1;", "// 新增注释形态", ""].join("\n");

const CLEAN_MAIN_TS = ["export const x = 1;", ""].join("\n");

const MINIMAL_BASELINE: Record<string, number> = { scripts: 0, src: 0 };

describe("check-comment-gate", () => {
  it("真实仓库默认通过：基线 = 治理落地现状计数，全绿（现行仓库绿）", () => {
    const r = run([]);
    expect(r.status).toBe(0);
    expect(r.output).toContain("注释棘轮通过");
    expect(r.output).toContain("comment-baseline.json");
    expect(r.output).toContain("ADR-0145");
  });

  it("真实基线文件 schema：目录键字符串键、计数为非负整数", () => {
    const raw = readFileSync(
      join(process.cwd(), "scripts", "fixtures", "comment-baseline.json"),
      "utf8",
    );
    const parsed = JSON.parse(raw) as Record<string, unknown>;
    const entries = Object.entries(parsed);
    expect(entries.length).toBeGreaterThan(0);
    for (const [key, value] of entries) {
      expect(typeof key).toBe("string");
      expect(typeof value, `非数值计数：${key} = ${String(value)}`).toBe("number");
      const count = value as number;
      expect(Number.isInteger(count), `非整数计数：${key} = ${String(count)}`).toBe(true);
      expect(count >= 0, `负数计数：${key} = ${String(count)}`).toBe(true);
    }
  });

  it("各语言注释形态逐一计数，豁免闭集与字面量不误报（基线全零 → 零容忍逐行定位）", () => {
    const root = makeFixture(LANGUAGE_FIXTURE_FILES, ZERO_LANGUAGE_BASELINE);
    const r = run([root]);
    expect(r.status).toBe(1);
    expect(r.output).toContain("零容忍命中（基线 0）：src-tauri/crates/demo");
    expect(r.output).toContain("lib.rs:3");
    expect(r.output).toContain("lib.rs:6");
    expect(r.output).toContain("lib.rs:10");
    expect(r.output).toContain("lib.rs:12");
    expect(r.output).toContain("lib.rs:13");
    expect(r.output).toContain("ui.ts:2");
    expect(r.output).toContain("ui.ts:6");
    expect(r.output).toContain("App.vue:2");
    expect(r.output).toContain("tool.sh:2");
    expect(r.output).toContain("Cargo.toml:2");
    expect(r.output).toContain(".kache.toml:1");
    expect(r.output).toContain("package.json:3");
    expect(r.output).toContain("❌ 注释禁令失败");
    expect(r.output).toContain("ADR-0145");
    expect(r.output).not.toContain("SAFETY：unsafe");
    expect(r.output).not.toContain("#![allow");
    expect(r.output).not.toContain("字面量里的注释字样");
    expect(r.output).not.toContain("不是注释的字样");
    expect(r.output).not.toContain("example.com");
    expect(r.output).not.toContain("字面量井号形态");
    expect(r.output).not.toContain("行尾注释不计数");
    expect(r.output).not.toContain("HTML 注释形态");
    expect(r.output).not.toContain("other.toml");
  });

  it("基线上升变红：正基线目录报目录级对比与逐文件计数，不做逐行定位", () => {
    const root = makeFixture([{ rel: "src/main.ts", content: MAIN_TS_FIXTURE }], {
      scripts: 0,
      src: 1,
    });
    const r = run([root]);
    expect(r.status).toBe(1);
    expect(r.output).toContain("棘轮超线：src：实际 2 行 > 基线 1 行（+1）");
    expect(r.output).toContain("src/main.ts：2 行");
    expect(r.output).not.toContain("main.ts:2");
  });

  it("基线下降通过：实际计数低于基线即绿", () => {
    const root = makeFixture([{ rel: "src/main.ts", content: MAIN_TS_FIXTURE }], {
      scripts: 0,
      src: 3,
    });
    const r = run([root]);
    expect(r.status).toBe(0);
    expect(r.output).toContain("注释棘轮通过");
    expect(r.output).toContain("2 行注释在基线内");
  });

  it("基线归零目录零容忍：新增一行注释即红并给出 文件:行", () => {
    const root = makeFixture(
      [{ rel: "src/main.ts", content: SINGLE_COMMENT_TS }],
      MINIMAL_BASELINE,
    );
    const r = run([root]);
    expect(r.status).toBe(1);
    expect(r.output).toContain("零容忍命中（基线 0）：src");
    expect(r.output).toContain("src/main.ts:2：// 新增注释形态");
  });

  it("基线缺键即红（键集双向全等，新目录须显式登记）", () => {
    const root = makeFixture([{ rel: "src/main.ts", content: CLEAN_MAIN_TS }], { src: 0 });
    const r = run([root]);
    expect(r.status).toBe(1);
    expect(r.output).toContain("基线缺键：scripts");
  });

  it("基线冗余键即红（键在扫描面已不存在）", () => {
    const root = makeFixture([{ rel: "src/main.ts", content: CLEAN_MAIN_TS }], {
      scripts: 0,
      src: 0,
      "src-tauri/ghost": 0,
    });
    const r = run([root]);
    expect(r.status).toBe(1);
    expect(r.output).toContain("基线冗余键：src-tauri/ghost");
  });

  it("基线负数与非整数计数即红（schema 校验）", () => {
    const negative = makeFixture([{ rel: "src/main.ts", content: CLEAN_MAIN_TS }], {
      scripts: 0,
      src: -1,
    });
    expect(run([negative]).status).toBe(1);
    expect(run([negative]).output).toContain("基线非法计数：src = -1");
    const fractional = makeFixture([{ rel: "src/main.ts", content: CLEAN_MAIN_TS }], {
      scripts: 0,
      src: 1.5,
    });
    expect(run([fractional]).status).toBe(1);
    expect(run([fractional]).output).toContain("基线非法计数：src = 1.5");
  });

  it("基线文件缺失或非法 JSON 即红（fail loud，不静默按零基线放行）", () => {
    const missing = makeFixture([{ rel: "src/main.ts", content: CLEAN_MAIN_TS }], MINIMAL_BASELINE);
    rmSync(baselinePathOf(missing));
    const missingRun = run([missing]);
    expect(missingRun.status).toBe(1);
    expect(missingRun.output).toContain("基线文件不存在");
    expect(missingRun.output).toContain("--write-baseline");
    const malformed = makeFixture(
      [{ rel: "src/main.ts", content: CLEAN_MAIN_TS }],
      MINIMAL_BASELINE,
    );
    writeFileSync(baselinePathOf(malformed), "not-json{");
    const malformedRun = run([malformed]);
    expect(malformedRun.status).toBe(1);
    expect(malformedRun.output).toContain("不是合法 JSON");
  });

  it("扫描文件落不进任何基线键即红（未登记位置 fail loud）", () => {
    const root = makeFixture([{ rel: "rogue/x.rs", content: "// 游离注释形态\n" }], {
      scripts: 0,
    });
    const r = run([root]);
    expect(r.status).toBe(1);
    expect(r.output).toContain("未登记目录键：rogue/x.rs");
  });

  it("--write-baseline 再生成基线：写入现状计数，随后棘轮放行", () => {
    const root = makeFixture([{ rel: "src/main.ts", content: MAIN_TS_FIXTURE }], MINIMAL_BASELINE);
    const written = run([root, "--write-baseline"]);
    expect(written.status).toBe(0);
    expect(written.output).toContain("基线已写入");
    const regenerated = JSON.parse(readFileSync(baselinePathOf(root), "utf8")) as Record<
      string,
      number
    >;
    expect(regenerated["src"]).toBe(2);
    expect(regenerated["scripts"]).toBe(0);
    const rechecked = run([root]);
    expect(rechecked.status).toBe(0);
    expect(rechecked.output).toContain("注释棘轮通过");
  });

  it("compile_fail doctest 载体豁免：/// 与 //! 形态、fence 体内 // 注释行、未闭合 fence 豁免至文件尾，豁免行不进棘轮计数", () => {
    const content = [
      "//! crate 文档行不在 fence 内照常计数",
      "",
      "/// ```compile_fail",
      "/// use ledger_scheduled::nope;",
      "/// ```",
      "",
      "//! ```compile_fail",
      "//! use tauri_app_lib::nope;",
      "//! ```",
      "",
      "//! ```compile_fail",
      "//! // fence 体内注释行一并豁免",
      "//! use tauri_app_lib::nope;",
      "//! ```",
      "",
      "//! ```compile_fail",
      "//! use tauri_app_lib::unclosed_nope;",
      "pub fn demo() {}",
      "// 文件尾前普通注释行，仍在未闭合 fence 内一并豁免",
      "",
    ].join("\n");
    const root = makeFixture([{ rel: "src-tauri/crates/demo/src/lib.rs", content }], {
      scripts: 0,
      "src-tauri": 0,
      "src-tauri/crates/demo": 1,
    });
    const r = run([root]);
    expect(r.status).toBe(0);
    expect(r.output).toContain("compile_fail 载体豁免 13 行");
    expect(r.output).toContain("注释棘轮通过");
    expect(r.output).not.toContain("nope;");
    expect(r.output).not.toContain("fence 体内注释行");
    expect(r.output).not.toContain("文件尾前普通注释行");

    const zeroTolerance = makeFixture([{ rel: "src-tauri/crates/demo/src/lib.rs", content }], {
      scripts: 0,
      "src-tauri": 0,
      "src-tauri/crates/demo": 0,
    });
    const z = run([zeroTolerance]);
    expect(z.status).toBe(1);
    expect(z.output).toContain("零容忍命中（基线 0）：src-tauri/crates/demo");
    expect(z.output).toContain("lib.rs:1：//! crate 文档行不在 fence 内照常计数");
    expect(z.output).not.toContain("nope;");
  });

  it("非 compile_fail fence（rust/ignore/no_run）不豁免：文档示例内行照常计数", () => {
    const content = [
      "/// ```rust",
      "/// let x = 1;",
      "/// ```",
      "/// ```ignore",
      "/// let y = 2;",
      "/// ```",
      "/// ```no_run",
      "/// let z = 3;",
      "/// ```",
      "",
    ].join("\n");
    const root = makeFixture([{ rel: "src-tauri/crates/demo/src/lib.rs", content }], {
      scripts: 0,
      "src-tauri": 0,
      "src-tauri/crates/demo": 0,
    });
    const r = run([root]);
    expect(r.status).toBe(1);
    expect(r.output).toContain("零容忍命中（基线 0）：src-tauri/crates/demo");
    expect(r.output).toContain("lib.rs:1：/// ```rust");
    expect(r.output).toContain("lib.rs:2：/// let x = 1;");
    expect(r.output).toContain("lib.rs:5：/// let y = 2;");
    expect(r.output).toContain("lib.rs:8：/// let z = 3;");
    expect(r.output).toContain("❌ 注释禁令失败");
  });
});
