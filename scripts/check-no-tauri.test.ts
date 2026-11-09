import { afterAll, beforeEach, describe, expect, it } from "bun:test";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { scanCargoManifests, scanFrontendSources, scanPackageJsons } from "./check-no-tauri.ts";

// 全仓无 tauri 守门的包装测试（删除即变红，ADR-0087 断言强度）：夹具仓库根
// 按 SCAN 集合同构摆放，断言「tauri 声明 / @tauri-apps 引用 / tauri 依赖键」
// 各面命中即红、清除后零命中；注释提及不误报（掩码语义）；空集拒绝假绿。

let fixtureRoot = "";

beforeEach(() => {
  fixtureRoot = mkdtempSync(join("/tmp", "check-no-tauri-"));
  mkdirSync(join(fixtureRoot, "src-app/crates/infra"), { recursive: true });
  mkdirSync(join(fixtureRoot, "src"), { recursive: true });
  mkdirSync(join(fixtureRoot, "packages/api/src"), { recursive: true });
  writeFileSync(
    join(fixtureRoot, "src-app/Cargo.toml"),
    `[package]\nname = "nsh-app"\n# 注释提及 tauri 不算（掩码语义）\n`,
  );
  writeFileSync(
    join(fixtureRoot, "src-app/crates/infra/Cargo.toml"),
    `[package]\nname = "ledger-infra"\n`,
  );
  writeFileSync(join(fixtureRoot, "package.json"), `{\n  "name": "ledger"\n}\n`);
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
  it("面①：Cargo 清单 tauri 声明命中即红，注释提及不误报", () => {
    expect(scanCargoManifests(fixtureRoot)).toEqual([]);
    write(
      "src-app/crates/infra/Cargo.toml",
      `[package]\nname = "ledger-infra"\ntauri = { version = "2", optional = true }\ntauri-emit = ["dep:tauri"]\n`,
    );
    const hits = scanCargoManifests(fixtureRoot);
    expect(hits.length).toBe(2);
    expect(hits[0]?.file).toBe("src-app/crates/infra/Cargo.toml");
    expect(hits[0]?.line).toBe(3);
    // 注释行不算（maskNonCode 掩注释）
    write(
      "src-app/crates/infra/Cargo.toml",
      `[package]\nname = "ledger-infra"\n# tauri-emit 门已退役\n`,
    );
    expect(scanCargoManifests(fixtureRoot)).toEqual([]);
  });

  it("面②源码：前端 @tauri-apps 引用命中即红，注释提及不误报", () => {
    expect(scanFrontendSources(fixtureRoot)).toEqual([]);
    write("src/composables/useX.ts", `import { invoke } from "@tauri-apps/api/core";\n`);
    write(
      "packages/api/src/index.ts",
      `// 历史：曾从 "@tauri-apps/api/core" 引入（注释不算）\nexport {};\n`,
    );
    const hits = scanFrontendSources(fixtureRoot);
    expect(hits.length).toBe(1);
    expect(hits[0]?.file).toBe("src/composables/useX.ts");
  });

  it("面②清单：package.json tauri 依赖键 / 脚本键命中即红", () => {
    expect(scanPackageJsons(fixtureRoot)).toEqual([]);
    write(
      "package.json",
      `{\n  "name": "ledger",\n  "@tauri-apps/api": "^2",\n  "scripts": {},\n  "tauri": "tauri"\n}\n`,
    );
    const hits = scanPackageJsons(fixtureRoot);
    expect(hits.length).toBe(2);
    // 值里出现 tauri 词不算（键形态判定）
    write("package.json", `{\n  "name": "ledger",\n  "description": "not tauri app"\n}\n`);
    expect(scanPackageJsons(fixtureRoot)).toEqual([]);
  });

  it("真实仓库零命中（守门在当前树必须全绿）", () => {
    // 直接对真实仓库跑三面扫描（bun test cwd = 仓库根）：任何残留即本测试红
    //（与 CI 门同判，删除即变红的面级负例已在上方用例覆盖）。
    const hits = [
      ...scanCargoManifests("."),
      ...scanFrontendSources("."),
      ...scanPackageJsons("."),
    ];
    expect(hits, `真实仓库存在 tauri 残留：${JSON.stringify(hits, null, 2)}`).toEqual([]);
  });
});
