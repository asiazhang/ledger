import { afterAll, describe, expect, it } from "vitest";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { gateScript, runGateScript } from "./run-gate-script.test-helper.ts";

// 被测对象是仓库工具脚本 scripts/check-commands.ts（命令注册一致性校验：命令名腿 + 参数键名腿）。
// 脚本以 Bun 运行时执行（ADR-0083）：runGateScript 以 spawnSync('bun') 与门槛
// 调用同款拉起，测的就是门槛路径。
// 按测试决策只测外部可观察结果——进程退出码与输出，不测内部函数；
// 通过位置参数把扫描目标指向临时夹具目录。
const script = gateScript("check-commands.ts");
const run = (args: string[]) => runGateScript(script, args);

const tempDirs: string[] = [];
afterAll(() => {
  for (const dir of tempDirs) rmSync(dir, { recursive: true, force: true });
});

/** 建临时夹具：commands 命令目录 + api.ts 调用面文件，返回脚本参数 */
function makeFixture(commands: Record<string, string>, apiTs: string): string[] {
  const dir = mkdtempSync(join(tmpdir(), "check-commands-"));
  tempDirs.push(dir);
  const cmdsDir = join(dir, "commands");
  for (const [relPath, content] of Object.entries(commands)) {
    const file = join(cmdsDir, relPath);
    mkdirSync(join(file, ".."), { recursive: true });
    writeFileSync(file, content);
  }
  const apiFile = join(dir, "api.ts");
  writeFileSync(apiFile, apiTs);
  return [cmdsDir, apiFile];
}

// 注入参数用真实形态（State<'_, DbState>）：db 是 Tauri 注入参数，不进 invoke 键集（#1398）
const cmd = (name: string) =>
  `#[tauri::command]\npub fn ${name}(db: State<'_, DbState>) -> String {\n    todo!()\n}\n`;

describe("check-commands（命令注册一致性校验）", () => {
  it("真实仓库默认通过：Rust 注解命令集与 TS 调用面双向全等", () => {
    const r = run([]);
    expect(r.status).toBe(0);
    expect(r.output).toContain("双向全等");
  });

  it("夹具两侧一致时通过", () => {
    const args = makeFixture(
      {
        "alpha.rs": cmd("alpha_one") + cmd("alpha_two"),
        "beta/mod.rs": cmd("beta_one"),
      },
      [
        "import { invoke } from '@tauri-apps/api/core'",
        "export const api = {",
        "  a: () => invoke<void>('alpha_one'),",
        "  b: () => invoke<void>('alpha_two'),",
        "  c: () => invoke<string>('beta_one'),",
        "}",
        "",
      ].join("\n"),
    );
    const r = run(args);
    expect(r.status).toBe(0);
    expect(r.output).toContain("双向全等");
  });

  it("TS 缺方法（Rust 有 TS 无）→ 失败并列出差异", () => {
    const args = makeFixture(
      { "alpha.rs": cmd("alpha_one") + cmd("alpha_two") },
      "invoke<void>('alpha_one')\n",
    );
    const r = run(args);
    expect(r.status).toBe(1);
    expect(r.output).toContain("TS 调用面缺方法");
    expect(r.output).toContain("alpha_two");
    expect(r.output).not.toContain("- alpha_one\n");
  });

  it("TS 调用不存在的命令 → 失败并列出差异", () => {
    const args = makeFixture(
      { "alpha.rs": cmd("alpha_one") },
      "invoke<void>('alpha_one')\ninvoke<void>('ghost_cmd')\n",
    );
    const r = run(args);
    expect(r.status).toBe(1);
    expect(r.output).toContain("Rust 无此命令");
    expect(r.output).toContain("ghost_cmd");
  });

  it("注解后不是 fn 定义（扫描器不认识的形态）→ 报扫描边界错误", () => {
    const args = makeFixture(
      {
        "alpha.rs": '#[tauri::command]\n#[cfg(target_os = "macos")]\npub fn alpha_one() {}\n',
      },
      "invoke<void>('alpha_one')\n",
    );
    const r = run(args);
    expect(r.status).toBe(1);
    expect(r.output).toMatch(/扫描器/);
    expect(r.output).toContain("alpha.rs");
  });

  it("命令名重复定义 → 失败", () => {
    const args = makeFixture(
      { "alpha.rs": cmd("alpha_one"), "beta/mod.rs": cmd("alpha_one") },
      "invoke<void>('alpha_one')\n",
    );
    const r = run(args);
    expect(r.status).toBe(1);
    expect(r.output).toMatch(/重复/);
    expect(r.output).toContain("alpha_one");
  });

  it("空集（两侧皆扫不到命令）→ 拒绝以「0 ↔ 0」假绿通过", () => {
    const args = makeFixture({ "alpha.rs": "// 无命令的文件" }, "export const api = {}\n");
    const r = run(args);
    expect(r.status).toBe(1);
    expect(r.output).toMatch(/未在命令目录扫描到任何/);
    expect(r.output).toMatch(/未在 TS 调用面扫描到任何/);
  });

  it("注释掩码口径（issue #1741 收口）：块注释缀随/包裹的注解不假红，纯注释里的注解形态不认命令", () => {
    // Rust 腿自 #1741 起整文 maskNonCode(·, true) 后逐行扫：注解行被块注释缀随或
    // 前置时掩码后 trim 仍精确命中；整行落在块注释内的注解被掩掉不武装扫描器
    // （弱形态逐行剥离器会漏掩块注释，注解行误武装、`*/` 行报假红）。
    const args = makeFixture(
      {
        "alpha.rs": [
          "/* 前缀说明 */ #[tauri::command]",
          "pub fn alpha_one(db: State<'_, DbState>) -> String { todo!() }",
          "",
          "/*",
          "#[tauri::command]",
          "*/",
          "",
          "#[tauri::command] /* 尾随说明 */",
          "pub async fn alpha_two(db: State<'_, DbState>) -> String { todo!() }",
          "",
        ].join("\n"),
      },
      ["invoke<void>('alpha_one')", "invoke<void>('alpha_two')", ""].join("\n"),
    );
    const r = run(args);
    expect(r.status).toBe(0);
    expect(r.output).toMatch(/双向全等/);
  });

  describe("参数键名腿（issue #1398，#588 类回归根治）", () => {
    const topCmd =
      "#[tauri::command]\npub fn top_report(db: State<'_, DbState>, top_n: Option<i64>) -> String {\n    todo!()\n}\n";

    it("参数键名全等（含属性简写、多行对象、snake→camel 转换、注入参数不进键集）→ 通过", () => {
      const args = makeFixture(
        {
          "items.rs":
            "#[tauri::command]\npub fn calc_cost(db: State<'_, DbState>, id: String, reference_date: Option<String>) -> String {\n    todo!()\n}\n",
        },
        [
          "import { invoke } from '@tauri-apps/api/core'",
          "export const api = {",
          "  calc: (id: string, referenceDate?: string | null) =>",
          "    invoke<string>('calc_cost', {",
          "      id,",
          "      referenceDate: referenceDate ?? null,",
          "    }),",
          "}",
          "",
        ].join("\n"),
      );
      const r = run(args);
      expect(r.status).toBe(0);
      expect(r.output).toMatch(/参数键名/);
    });

    it("top_n 形态失配（Rust top_n ↔ TS 键 top_n）→ 失败且差异列出该键", () => {
      const args = makeFixture(
        { "reports.rs": topCmd },
        "invoke<string>('top_report', { top_n: 5 })\n",
      );
      const r = run(args);
      expect(r.status).toBe(1);
      expect(r.output).toContain("topN"); // 期望键（Rust 参数名转换后）
      expect(r.output).toContain("top_n"); // 失配的实际键
    });

    it("实参缺键（TS 漏传可选参数）→ 失败并列出缺失键", () => {
      const args = makeFixture({ "reports.rs": topCmd }, "invoke<string>('top_report')\n");
      const r = run(args);
      expect(r.status).toBe(1);
      expect(r.output).toContain("topN");
    });

    it("同命令多调用点键集不一致 → 失败", () => {
      const args = makeFixture(
        {
          "reports.rs":
            "#[tauri::command]\npub fn ms(db: State<'_, DbState>, year: i64, from: Option<String>, to: Option<String>) -> String {\n    todo!()\n}\n",
        },
        [
          "invoke<string>('ms', { year: 2026, from: null, to: null })",
          "invoke<string>('ms', { year: 2026, from: null })",
          "",
        ].join("\n"),
      );
      const r = run(args);
      expect(r.status).toBe(1);
      expect(r.output).toMatch(/键集不同/);
      expect(r.output).toContain("ms");
    });

    it("含数字段转换与 Tauri 绑定一致（s3_bucket → s3Bucket，数字不成词界）→ 通过", () => {
      const args = makeFixture(
        {
          "sync.rs":
            "#[tauri::command]\npub fn s3_report(db: State<'_, DbState>, s3_bucket: Option<String>) -> String {\n    todo!()\n}\n",
        },
        "invoke<string>('s3_report', { s3Bucket: 'demo' })\n",
      );
      const r = run(args);
      expect(r.status).toBe(0);
    });

    it("实参展开语法（...）→ fail loud 拒绝", () => {
      const args = makeFixture(
        { "reports.rs": topCmd },
        "invoke<string>('top_report', { topN: 5, ...rest })\n",
      );
      const r = run(args);
      expect(r.status).toBe(1);
      expect(r.output).toMatch(/展开/);
    });

    it("实参计算键（[...]）→ fail loud 拒绝", () => {
      const args = makeFixture(
        { "reports.rs": topCmd },
        "invoke<string>('top_report', { ['topN']: 5 })\n",
      );
      const r = run(args);
      expect(r.status).toBe(1);
      expect(r.output).toMatch(/计算键/);
    });

    it("实参非对象字面量（变量透传）→ fail loud 拒绝", () => {
      const args = makeFixture(
        { "reports.rs": topCmd },
        "invoke<string>('top_report', someArgs)\n",
      );
      const r = run(args);
      expect(r.status).toBe(1);
      expect(r.output).toMatch(/不是对象字面量/);
    });
  });

  describe("TS 调用面口径（issue #1471：注释掩码 + 双引号识别）", () => {
    it("注释掉的 invoke 形态不报「Rust 无此命令」假红，注释外真实调用仍参与核对", () => {
      const args = makeFixture(
        { "alpha.rs": cmd("alpha_one") },
        [
          "invoke<void>('alpha_one')",
          "// 临时注释掉的调用 // invoke<void>('ghost_commented')",
          "invoke<void>('ghost_real')",
          "",
        ].join("\n"),
      );
      const r = run(args);
      expect(r.status).toBe(1);
      expect(r.output).toContain("Rust 无此命令");
      expect(r.output).toContain("ghost_real"); // 注释外的真实调用照旧核对（未因掩码放过）
      expect(r.output).not.toContain("ghost_commented"); // 注释里的调用不进调用面（删除掩码即红）
    });

    it("注释掉的调用不参与参数键名核对（参数键名腿同口径）", () => {
      const args = makeFixture(
        {
          "reports.rs":
            "#[tauri::command]\npub fn top_report(db: State<'_, DbState>, top_n: Option<i64>) -> String {\n    todo!()\n}\n",
        },
        [
          "invoke<string>('top_report', { topN: 5 })",
          "// invoke<string>('top_report', { top_n: 5 })",
          "",
        ].join("\n"),
      );
      const r = run(args);
      expect(r.status).toBe(0);
    });

    it("调用面只剩被注释掉的 invoke → 视作空集，拒绝假绿", () => {
      const args = makeFixture({ "alpha.rs": cmd("alpha_one") }, "// invoke<void>('alpha_one')\n");
      const r = run(args);
      expect(r.status).toBe(1);
      expect(r.output).toMatch(/未在 TS 调用面扫描到任何/);
    });

    it("块注释里的 invoke 形态不误报（与行注释同口径）", () => {
      const args = makeFixture(
        { "alpha.rs": cmd("alpha_one") },
        ["invoke<void>('alpha_one')", "/* invoke<void>('ghost_block') */", ""].join("\n"),
      );
      const r = run(args);
      expect(r.status).toBe(0);
    });

    it("双引号幽灵命令 → 失败并列出（旧识别面只认单引号会静默漏检）", () => {
      const args = makeFixture(
        { "alpha.rs": cmd("alpha_one") },
        ["invoke<void>('alpha_one')", 'invoke<void>("ghost_dq")', ""].join("\n"),
      );
      const r = run(args);
      expect(r.status).toBe(1);
      expect(r.output).toContain("Rust 无此命令");
      expect(r.output).toContain("ghost_dq");
    });

    it("双引号合法调用与 Rust 注解对齐 → 通过（含泛型与参数键名腿）", () => {
      const args = makeFixture(
        {
          "reports.rs":
            "#[tauri::command]\npub fn top_report(db: State<'_, DbState>, top_n: Option<i64>) -> String {\n    todo!()\n}\n",
        },
        'invoke<string>("top_report", { topN: 5 })\n',
      );
      const r = run(args);
      expect(r.status).toBe(0);
      expect(r.output).toMatch(/双向全等/);
    });

    it("双引号调用仍参与参数键名核对（键错 → 失败）", () => {
      const args = makeFixture(
        {
          "reports.rs":
            "#[tauri::command]\npub fn top_report(db: State<'_, DbState>, top_n: Option<i64>) -> String {\n    todo!()\n}\n",
        },
        'invoke<string>("top_report", { top_n: 5 })\n',
      );
      const r = run(args);
      expect(r.status).toBe(1);
      expect(r.output).toContain("topN");
      expect(r.output).toContain("top_n");
    });

    it("字符串里的 // 不吞掉其后的真实调用（掩码须识别字面量，不把 URL 当注释）", () => {
      const args = makeFixture(
        { "alpha.rs": cmd("alpha_one") },
        'const url = "http://example.com"; invoke<void>("ghost_after_url")\n',
      );
      const r = run(args);
      expect(r.status).toBe(1);
      expect(r.output).toContain("ghost_after_url");
    });

    it("字符串里的 /* 不吞掉其后的真实调用（掩码须识别字面量，不把未闭合 /* 当块注释）", () => {
      const args = makeFixture(
        { "alpha.rs": cmd("alpha_one") },
        ['const glob = "src/*.ts"', 'invoke<void>("ghost_after_glob")', ""].join("\n"),
      );
      const r = run(args);
      expect(r.status).toBe(1);
      expect(r.output).toContain("ghost_after_glob");
    });

    it("正则字面量里的 \\/\\/ 不吞掉其后的真实调用（正则体内斜杠带转义）", () => {
      const args = makeFixture(
        { "alpha.rs": cmd("alpha_one") },
        'const re = /^https:\\/\\//; invoke<void>("ghost_after_regex")\n',
      );
      const r = run(args);
      expect(r.status).toBe(1);
      expect(r.output).toContain("ghost_after_regex");
    });
  });
});
