import { describe, expect, it } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";

describe("oxfmt 配置锚点（防 #25125 式静默回退）", () => {
  it(".oxfmtrc.json 存在、可解析且风格锚点未漂移", () => {
    const raw = readFileSync(join(process.cwd(), ".oxfmtrc.json"), "utf8");
    const config = JSON.parse(raw) as { printWidth?: number; sortPackageJson?: boolean };
    expect(config.sortPackageJson).toBe(false);
    expect(config.printWidth).toBe(100);
  });
});
