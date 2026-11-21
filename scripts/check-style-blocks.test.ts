import { afterAll, describe, expect, it } from "bun:test";
import { mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { STYLE_BLOCK_WHITELIST } from "../scripts/check-style-blocks.ts";
import { gateScript, runGateScript } from "./run-gate-script.test-helper.ts";

const script = gateScript("check-style-blocks.ts");
const run = (args: string[], cwd?: string, scriptPath = script) =>
  runGateScript(scriptPath, args, { cwd });

const tempDirs: string[] = [];
afterAll(() => {
  for (const dir of tempDirs) rmSync(dir, { recursive: true, force: true });
});

const STUB_WITH_STYLE = [
  "<template>",
  '  <div class="stub" />',
  "</template>",
  "",
  "<style scoped>",
  ".stub {",
  "  color: red;",
  "}",
  "</style>",
  "",
].join("\n");

function makeFixture(omit?: string): string {
  const root = realpathSync(mkdtempSync(join(tmpdir(), "check-style-blocks-")));
  tempDirs.push(root);
  for (const rel of STYLE_BLOCK_WHITELIST) {
    if (rel === omit) continue;
    const abs = join(root, rel);
    mkdirSync(join(abs, ".."), { recursive: true });
    writeFileSync(abs, STUB_WITH_STYLE);
  }
  return root;
}

function makeScriptMutant(needle: string, replacement: string): string {
  const source = readFileSync(script, "utf8");
  if (!source.includes(needle)) {
    throw new Error(`脚本变体锚点未命中：${needle.trim()}——脚本格式漂移，请同步本测试`);
  }
  const dir = mkdtempSync(join(process.cwd(), "node_modules", "check-style-blocks-mutant-"));
  tempDirs.push(dir);
  writeFileSync(
    join(dir, "gate-primitives.ts"),
    readFileSync(join(process.cwd(), "scripts", "gate-primitives.ts"), "utf8"),
  );
  const mutant = join(dir, "check-style-blocks.ts");
  writeFileSync(mutant, source.replace(needle, replacement));
  return mutant;
}

function makeWhitelistMutant(entry: string): string {
  return makeScriptMutant(`  "${entry}",\n`, "");
}

describe("check-style-blocks（样式块守门）", () => {
  it("真实仓库默认通过：白名单外零 <style> 块且全条目可达（现行仓库绿）", () => {
    const r = run([]);
    expect(r.status).toBe(0);
    expect(r.output).toContain("样式块守门");
  });

  it("夹具全量桩（每条目文件带 <style> 块）时通过", () => {
    const root = makeFixture();
    const r = run([root], root);
    expect(r.status).toBe(0);
    expect(r.output).toContain("样式块守门");
  });

  it("白名单条目指向的文件缺失即红：陈目报出路径（清单漂移 fail loud，#1360）", () => {
    const omitted = STYLE_BLOCK_WHITELIST[0];
    const root = makeFixture(omitted);
    const r = run([root], root);
    expect(r.status).toBe(1);
    expect(r.output).toContain("不可达");
    expect(r.output).toContain(omitted);
    expect(r.output.match(/不可达/g)).toHaveLength(1);
  });

  it("删除白名单条目即红：文件仍带 <style> 块时回潮分支拦截（只减不增，#1360）", () => {
    const entry = STYLE_BLOCK_WHITELIST[STYLE_BLOCK_WHITELIST.length - 1];
    const root = makeFixture();
    const r = run([root], root, makeWhitelistMutant(entry));
    expect(r.status).toBe(1);
    expect(r.output).toContain("新增 <style> 块");
    expect(r.output).toContain(entry);
  });
});

describe("check-style-blocks css.ts CJS 形态字面串扫描（#2189 翻车根因机械化）", () => {
  it("真实仓库默认通过：src 下 *.css.ts 零 CJS 形态字面串", () => {
    const r = run([]);
    expect(r.status).toBe(0);
    expect(r.output).toContain("css.ts 零 CJS 形态字面串");
  });

  it("注释里的 global.css / require( / exports.x 即红（mlly CJS_RE 原样命中，含注释）", () => {
    const root = makeFixture();
    const rel = "src/stub.css.ts";
    writeFileSync(
      join(root, rel),
      [
        'import { globalStyle } from "@vanilla-extract/css";',
        "",
        "/** 引用全局基座：仍在 global.css（注释里的字面串也命中）。 */",
        'globalStyle(`.stub-a`, { color: "red" });',
      ].join("\n"),
    );
    const r = run([root], root);
    expect(r.status).toBe(1);
    expect(r.output).toContain("CJS 形态字面串");
    expect(r.output).toContain(rel);
  });

  it("删掉探测正则接线（CJS_LOOKALIKE_RE）即红：命中形态不再被拦截", () => {
    const mutant = makeScriptMutant("if (CJS_LOOKALIKE_RE.test(line)) {", "if (false) {");
    const root = makeFixture();
    const rel = "src/stub.css.ts";
    writeFileSync(
      join(root, rel),
      [
        'import { globalStyle } from "@vanilla-extract/css";',
        "",
        "/** 引用全局基座：仍在 global.css */",
        'globalStyle(`.stub-a`, { color: "red" });',
      ].join("\n"),
    );
    const r = run([root], root, mutant);
    expect(r.status).toBe(0);
    expect(r.output).toContain("样式块守门");
  });
});
