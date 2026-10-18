import { afterAll, describe, expect, it } from "vitest";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { gateScript, runGateScript } from "./run-gate-script.test-helper.ts";

// 被测对象是仓库工具脚本 scripts/check-versions.ts（版本元数据一致性守门，
// issue #1957 / ADR-0022 修订注记）。脚本以 Bun 运行时执行（ADR-0083）：
// runGateScript 以 spawnSync('bun') 与门槛调用同款拉起，测的就是门槛路径。
// 按测试决策只测外部可观察结果——进程退出码与输出，通过位置参数把目标指向
// 临时夹具目录（形状同构仓库根：三份元数据文件）。
const script = gateScript("check-versions.ts");
const run = (args: string[] = []) => runGateScript(script, args);

const tempDirs: string[] = [];
afterAll(() => {
  for (const dir of tempDirs) rmSync(dir, { recursive: true, force: true });
});

/** 三份元数据的最小可解析形态；值为 null 表示刻意省略该文件。 */
const DEFAULT_FILES: Record<string, string | null> = {
  "package.json": JSON.stringify({ name: "ledger", version: "1.2.3" }, null, 2),
  "src-tauri/tauri.conf.json": JSON.stringify(
    { productName: "OpenLedger", version: "1.2.3" },
    null,
    2,
  ),
  "src-tauri/Cargo.toml": [
    "[package]",
    'name = "tauri-app"',
    'version = "1.2.3"',
    "",
    "[dependencies]",
    'serde = { version = "1" }',
  ].join("\n"),
};

/** 建夹具目录（形状同构仓库根）；值为 null 表示刻意省略该文件。 */
function makeFixture(files: Record<string, string | null> = {}): string {
  const dir = mkdtempSync(join(tmpdir(), "check-versions-"));
  tempDirs.push(dir);
  const all: Record<string, string | null> = { ...DEFAULT_FILES, ...files };
  for (const [name, content] of Object.entries(all)) {
    if (content === null) continue;
    mkdirSync(dirname(join(dir, name)), { recursive: true });
    writeFileSync(join(dir, name), content);
  }
  return dir;
}

describe("check-versions（版本元数据一致性守门，issue #1957）", () => {
  it("默认校验本仓（门槛路径）：三处版本一致全绿", () => {
    const r = run();
    expect(r.status).toBe(0);
    expect(r.output).toContain("版本元数据一致");
    expect(r.output).toContain("package.json / src-tauri/tauri.conf.json / src-tauri/Cargo.toml");
  });

  it("夹具三处一致全绿（位置参数指向夹具根）", () => {
    const r = run([makeFixture()]);
    expect(r.status).toBe(0);
    expect(r.output).toContain("1.2.3");
  });

  it("tauri.conf.json 漂移即红，并列出三处取值", () => {
    const dir = makeFixture({
      "src-tauri/tauri.conf.json": JSON.stringify({ version: "1.2.4" }, null, 2),
    });
    const r = run([dir]);
    expect(r.status).toBe(1);
    expect(r.output).toContain("版本号不一致");
    expect(r.output).toContain("src-tauri/tauri.conf.json=1.2.4");
    expect(r.output).toContain("package.json=1.2.3");
  });

  it("Cargo.toml 漂移即红（覆盖 cargo 解析腿：依赖区 version 不参与判定）", () => {
    const dir = makeFixture({
      "src-tauri/Cargo.toml": [
        "[package]",
        'name = "tauri-app"',
        'version = "1.2.0"',
        "",
        "[dependencies]",
        'serde = { version = "1.2.3" }',
      ].join("\n"),
    });
    const r = run([dir]);
    expect(r.status).toBe(1);
    expect(r.output).toContain("src-tauri/Cargo.toml=1.2.0");
  });

  it("文件缺失即红并指名缺失文件（fail loud，不静默跳过）", () => {
    const dir = makeFixture({ "src-tauri/tauri.conf.json": null });
    const r = run([dir]);
    expect(r.status).toBe(1);
    expect(r.output).toContain("src-tauri/tauri.conf.json：文件不存在");
  });

  it("version 字段读不出即红（不按空值放行）", () => {
    const dir = makeFixture({
      "package.json": JSON.stringify({ name: "ledger" }, null, 2),
    });
    const r = run([dir]);
    expect(r.status).toBe(1);
    expect(r.output).toContain("package.json：读不出顶层 version 字段");
  });
});
