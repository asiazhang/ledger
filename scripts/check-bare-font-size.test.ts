import { afterAll, describe, expect, it } from "bun:test";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { gateScript, runGateScript } from "./run-gate-script.test-helper.ts";
import { REGISTERED_EXCEPTIONS } from "./check-bare-font-size.ts";

// 被测对象是仓库工具脚本 scripts/check-bare-font-size.ts（壳根包裸字号直调零残留
// 守门，issue #2108 / spec #2099 / ADR-0147——源码扫描，删除扫描规则即存量违规不再
// 被拦）。脚本以 Bun 运行时执行（ADR-0083）：runGateScript 以 spawnSync('bun') 与
// 门槛调用同款拉起，测的就是门槛路径。
// 按测试决策只测外部可观察结果——进程退出码与输出，通过位置参数把扫描根指向
// 临时夹具目录（形状同构 src-app/src：夹具根直接放 .rs 模块文件）。
const script = gateScript("check-bare-font-size.ts");
const run = (args: string[] = []) => runGateScript(script, args);

const tempDirs: string[] = [];
afterAll(() => {
  for (const dir of tempDirs) rmSync(dir, { recursive: true, force: true });
});

/** 违规直调样例：px() 参数是数字字面量（裸值）——要抓的回归 */
const BARE_LINE = '                .child(text("复制").text_size(px(12.0)).text_color(accent)),';

/** 建夹具目录（形状同构扫描根：.rs 模块平铺/嵌套均可） */
function makeFixture(files: Record<string, string>): string {
  const dir = mkdtempSync(join(tmpdir(), "check-bare-font-size-"));
  tempDirs.push(dir);
  for (const [name, content] of Object.entries(files)) {
    mkdirSync(dirname(join(dir, name)), { recursive: true });
    writeFileSync(join(dir, name), content);
  }
  return dir;
}

describe("check-bare-font-size（壳根包裸字号直调零残留守门，issue #2108）", () => {
  it("默认扫描本仓（门槛路径）：src-app/src 零裸字号直调全绿，例外清单为空", () => {
    const r = run();
    expect(r.status).toBe(0);
    expect(r.output).toContain("壳根包裸字号直调零残留守门通过");
    // 扫描面非空自证（拒绝空集假绿：扫描根漂移成空面必须红，不能静默绿）
    expect(r.output).toMatch(/扫描 [1-9][0-9]* 个文件/);
    // 零例外稳态（#2108 交付时点：批次票 #2101–#2107 已把 643 处存量清零）
    expect(r.output).toContain("已登记例外 0 条");
  });

  it("负向判据：text_size(px(<数字字面量>)) 直调即红并定位到 文件:行", () => {
    const dir = makeFixture({
      "rogue_view.rs": [
        "use nsh_core::prelude::*;",
        "pub fn cell() -> impl IntoElement {",
        '    text("金额")',
        BARE_LINE,
        "}",
      ].join("\n"),
    });
    const r = run([dir]);
    expect(r.status).toBe(1);
    expect(r.output).toContain("rogue_view.rs:4");
    expect(r.output).toContain("text_size(px(12.0)");
    expect(r.output).toContain("font_size");
  });

  it("合规形态过门：px(常量引用) 与 typed 预设（text_xs 等）不触发", () => {
    const dir = makeFixture({
      "compliant_view.rs": [
        "use crate::design;",
        "pub fn cell() -> impl IntoElement {",
        '    text("金额").text_size(px(design::font_size::CAPTION)).text_xs()',
        "}",
      ].join("\n"),
    });
    const r = run([dir]);
    expect(r.status).toBe(0);
    expect(r.output).toContain("壳根包裸字号直调零残留守门通过");
  });

  it("词法豁免：注释与字符串字面量里的形态不计数（设计文档引证、样例文本合法在场）", () => {
    const dir = makeFixture({
      "docs_and_strings.rs": [
        "//! 收口先例：把裸 `text_size(px(..))` 直调换到字阶常量（#2108）。",
        'pub const SAMPLE: &str = "text_size(px(12.0)) 只是展示文本，不是调用";',
      ].join("\n"),
    });
    const r = run([dir]);
    expect(r.status).toBe(0);
    expect(r.output).toContain("壳根包裸字号直调零残留守门通过");
  });

  it("例外登记严格相等校验在册：交付时点为零例外稳态（存量已随批次票清零）", () => {
    // 例外机制语义（与 check-eastmoney-residue / check-infra-dml 同款）：实际命中
    // ≠ 登记数即红、例外清零也红（条目应删除）、登记文件消失红。交付时点清单为
    // 空——真实仓任何命中都按未登记残留红（负向判据用例覆盖）；出现新的合法在
    // 场时在此登记并同步补「白名单生效→绿」端到端用例。
    expect(REGISTERED_EXCEPTIONS).toEqual([]);
  });

  it("扫描根不存在即红；扫描根无 .rs 文件（空面）即红，拒绝静默空转", () => {
    const r = run([join(tmpdir(), "check-bare-font-size-nonexistent-xyz")]);
    expect(r.status).toBe(1);
    expect(r.output).toContain("扫描根不存在");

    const empty = makeFixture({ "README.txt": "非 Rust 面，不进扫描" });
    const emptyRun = run([empty]);
    expect(emptyRun.status).toBe(1);
    expect(emptyRun.output).toContain("扫描面为空");
  });
});
