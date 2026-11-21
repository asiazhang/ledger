import { describe, it, expect } from "vitest";
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";

const SRC_ROOT = join(import.meta.dirname, "..");

function collectFiles(dir: string, acc: string[] = []): string[] {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const full = join(dir, entry.name);
    if (entry.isDirectory()) collectFiles(full, acc);
    else if (entry.name.endsWith(".ts") || entry.name.endsWith(".vue")) acc.push(full);
  }
  return acc;
}

function importsConfirmFromPluginDialog(code: string): boolean {
  const moduleSpec = ["@tauri-apps", "plugin-dialog"].join("/");
  const staticRe = /import\s*\{[^}]*\bconfirm\b[^}]*\}\s*from\s*['"]([\w@/-]+)['"]/g;
  // 动态：import 该模块后解构 confirm 属潜在旁路，一并拦截
  // （测试文件对 open 的动态引入不含 confirm，不误伤）。
  const dynamicMatch = code.match(/import\(\s*['"]([\w@/-]+)['"]\s*\)(?:\s*\.\s*then[^;]*)?;/g);
  if (dynamicMatch) {
    for (const m of dynamicMatch) {
      if (m.includes(moduleSpec) && /\bconfirm\b/.test(m)) return true;
    }
  }
  for (const m of code.matchAll(staticRe)) {
    if (m[1] === moduleSpec) return true;
  }
  return false;
}

describe("原生 confirm 清零（issue #652 / ADR-0078 决策 1）", () => {
  it("src/ 全树不再从 @tauri-apps/plugin-dialog 引入 confirm（open/save 不受影响）", () => {
    const offenders = collectFiles(SRC_ROOT)
      .map((path) => ({ path, code: readFileSync(path, "utf8") }))
      .filter(({ code }) => importsConfirmFromPluginDialog(code))
      .map(({ path }) => path);
    expect(offenders, `以下文件仍从 plugin-dialog 引入 confirm：${offenders.join(", ")}`).toEqual(
      [],
    );
  });
});
