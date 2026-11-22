import { afterAll, describe, expect, it } from "bun:test";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { gateScript, runGateScript } from "./run-gate-script.test-helper.ts";

const script = gateScript("check-eastmoney-residue.ts");
const run = (args: string[] = []) => runGateScript(script, args);

const tempDirs: string[] = [];
afterAll(() => {
  for (const dir of tempDirs) rmSync(dir, { recursive: true, force: true });
});

const RETIRED_FX_HOST_POOL = [
  "// 测试夹具：已退役的东财 FX 日 K 主机池（#1551 换 ECB）",
  "pub const FX_KLINE_HOSTS: &[&str] = &[",
  '    "https://push2his.eastmoney.com",',
  "];",
].join("\n");

const DEFAULT_FILES: Record<string, string> = {
  "scripts/check-eastmoney-residue.ts": "// 守门本体（豁免自证对象）",
  "scripts/check-eastmoney-residue.test.ts": "// 包装测试（豁免自证对象）",
  "src/lib.ts": "export const ok = 1;",
};

function makeFixture(files: Record<string, string | null> = {}): string {
  const dir = mkdtempSync(join(tmpdir(), "check-eastmoney-residue-"));
  tempDirs.push(dir);
  const all: Record<string, string | null> = { ...DEFAULT_FILES, ...files };
  for (const [name, content] of Object.entries(all)) {
    if (content === null) continue;
    mkdirSync(dirname(join(dir, name)), { recursive: true });
    writeFileSync(join(dir, name), content);
  }
  return dir;
}

describe("check-eastmoney-residue（东财行情面端点零残留守门，issue #1572 / ADR-0130 判据①）", () => {
  it("默认扫描本仓（门槛路径）：行情面零残留全绿，已登记例外清单为空", () => {
    const r = run();
    expect(r.status).toBe(0);
    expect(r.output).toContain("东财行情面零残留守门通过");
    expect(r.output).toContain("已登记例外 0 条");
  });

  it("负向判据：未登记文件出现东财端点域名即红并定位到 文件:行", () => {
    const dir = makeFixture({
      "src-tauri/crates/market-sync/src/rogue.rs": [
        "// 测试夹具：重新引入东财通道（应被守门拦下）",
        'pub const HOSTS: &[&str] = &["https://push2.eastmoney.com"];',
        "const REFERER = 'https://fund.eastmoney.com/';",
      ].join("\n"),
    });
    const r = run([dir]);
    expect(r.status).toBe(1);
    expect(r.output).toContain("src-tauri/crates/market-sync/src/rogue.rs:2");
    expect(r.output).toContain("eastmoney.com");
    expect(r.output).toContain("ADR-0130");
  });

  it("dfcfw 域名同在禁令闭集：东财自有域任一出现即红", () => {
    const dir = makeFixture({
      "src/rogue.ts": "export const IMG = 'https://dfcfw.eastmoney.com/x.png';",
    });
    const r = run([dir]);
    expect(r.status).toBe(1);
    expect(r.output).toContain("src/rogue.ts:1");
  });

  it("收紧后的判据：原 FX 腿主机池文件（http.rs）再出现东财域名即未登记残留红", () => {
    const dir = makeFixture({
      "src-tauri/crates/market-sync/src/http.rs": RETIRED_FX_HOST_POOL,
    });
    const r = run([dir]);
    expect(r.status).toBe(1);
    expect(r.output).toContain("src-tauri/crates/market-sync/src/http.rs:3");
    expect(r.output).toContain("eastmoney.com");
    expect(r.output).toContain("ADR-0130");
  });

  it("豁免自证：守门本体或包装测试缺失即红，不放任守门空转", () => {
    const noScript = makeFixture({ "scripts/check-eastmoney-residue.ts": null });
    const noScriptRun = run([noScript]);
    expect(noScriptRun.status).toBe(1);
    expect(noScriptRun.output).toContain("豁免清单漂移");

    const noTest = makeFixture({ "scripts/check-eastmoney-residue.test.ts": null });
    expect(run([noTest]).status).toBe(1);
  });

  it("docs/ 历史叙述豁免：档案内的端点 URL 引证不计数", () => {
    const dir = makeFixture({
      "docs/research/note.md": "历史档案：https://push2his.eastmoney.com 已于 2026-09 实测失效。",
    });
    const r = run([dir]);
    expect(r.status).toBe(0);
    expect(r.output).toContain("东财行情面零残留守门通过");
  });
});
