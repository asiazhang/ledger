import { afterAll, beforeEach, describe, expect, it } from "bun:test";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { scanCargoManifests } from "./check-no-tauri.ts";

// 全仓无 tauri 守门的包装测试（删除即变红，ADR-0087 断言强度）：夹具仓库根
// 按扫描根同构摆放，断言「Cargo 清单 tauri 声明」命中即红、清除后零命中；
// 注释提及不误报（掩码语义）；空集拒绝假绿。前端死路径面（原面②）已随
// Web 参照前端退役删除（#2171/#2172），本测试只辖 Cargo 清单面。

let fixtureRoot = "";

beforeEach(() => {
  fixtureRoot = mkdtempSync(join("/tmp", "check-no-tauri-"));
  mkdirSync(join(fixtureRoot, "src-app/crates/infra"), { recursive: true });
  writeFileSync(
    join(fixtureRoot, "src-app/Cargo.toml"),
    `[package]\nname = "nsh-app"\n# 注释提及 tauri 不算（掩码语义）\n`,
  );
  writeFileSync(
    join(fixtureRoot, "src-app/crates/infra/Cargo.toml"),
    `[package]\nname = "ledger-infra"\n`,
  );
});

afterAll(() => {
  rmSync(fixtureRoot, { recursive: true, force: true });
});

function write(rel: string, content: string): void {
  const abs = join(fixtureRoot, rel);
  mkdirSync(abs.slice(0, abs.lastIndexOf("/")), { recursive: true });
  writeFileSync(abs, content);
}

describe("全仓无 tauri 守门（#2083）", () => {
  it("Cargo 清单 tauri 声明命中即红，注释提及不误报", () => {
    expect(scanCargoManifests(fixtureRoot)).toEqual([]);
    write(
      "src-app/crates/infra/Cargo.toml",
      `[package]\nname = "ledger-infra"\ntauri = { version = "2", optional = true }\ntauri-emit = ["dep:tauri"]\n`,
    );
    const hits = scanCargoManifests(fixtureRoot);
    expect(hits.length).toBe(2);
    expect(hits[0]?.file).toBe("src-app/crates/infra/Cargo.toml");
    expect(hits[0]?.line).toBe(3);
    // 注释行不算（TOML 注释掩码）
    write(
      "src-app/crates/infra/Cargo.toml",
      `[package]\nname = "ledger-infra"\n# tauri-emit 门已退役\n`,
    );
    expect(scanCargoManifests(fixtureRoot)).toEqual([]);
  });

  it("真实仓库零命中（守门在当前树必须全绿）", () => {
    // 直接对真实仓库跑扫描（bun test cwd = 仓库根）：任何残留即本测试红
    //（与 CI 门同判，删除即变红的负例已在上方用例覆盖）。
    expect(scanCargoManifests("."), "真实仓库存在 tauri 残留").toEqual([]);
  });
});
