#!/usr/bin/env bun
// 版本元数据一致性守门（issue #1957 / ADR-0022 修订注记）：应用版本在三个文件里
// 各写一次——`package.json`（前端元数据）、`src-tauri/tauri.conf.json`（安装包与
// 更新比较的真源）、根 `src-tauri/Cargo.toml`（壳层 `app_build_info` 经
// `CARGO_PKG_VERSION` 上报的值）。三处互不引用，任何一处漏改都会让用户在
// 「关于」页看到的版本、更新判断用的版本、安装包声明的版本变成三个数——本票
// 修的就是这个缺陷，本门把它钉死。
//
// 判定：三处相等即绿；不相等、文件缺失、字段读不出（fail loud，不静默跳过）
// 一律非零退出并逐条列出。`Cargo.lock` 不入判定面——它是 cargo 的产物，
// 随 `cargo build`/`cargo update` 自动跟随，纳入判定只会制造与真实元数据无关的假红。
//
// 扫描边界：只读三份元数据的顶层 version 字段（Cargo.toml 取 `[package]` 段内的
// 那一个），不解析依赖区、不比对依赖版本——依赖升级纪律归
// `docs/agents/dependency-upgrades.md`，与发布版本号是两件事。
//
// TypeScript 化 + Bun 运行时（issue #734 / ADR-0083）：类型经 tsconfig.scripts.json
// 门槛检查；调用方式 `bun scripts/check-versions.ts`。
// 默认校验本仓库；测试可传位置参数指向夹具根：`bun scripts/check-versions.ts [root]`。
// 挂载登记归 scripts/gate-mounts.ts（check.sh + CI 双宿主，issue #1682）。
// 包装测试 scripts/check-versions.test.ts。

import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

/** 一处版本来源的读取结果：读到即 `version`，读不到即 `problem`（两者互斥）。 */
export interface VersionSourceReading {
  /** 相对仓库根的路径（输出定位用） */
  file: string;
  version: string | null;
  problem: string | null;
}

/** 判定面清单（顺序即输出顺序）：三份元数据文件各一条。 */
export const VERSION_SOURCES: readonly string[] = [
  "package.json",
  "src-tauri/tauri.conf.json",
  "src-tauri/Cargo.toml",
];

/** 从 JSON 元数据（package.json / tauri.conf.json）读顶层 version。 */
function readJsonVersion(text: string): string | null {
  let parsed: unknown;
  try {
    parsed = JSON.parse(text);
  } catch {
    return null;
  }
  if (typeof parsed !== "object" || parsed === null) return null;
  const version = (parsed as { version?: unknown }).version;
  return typeof version === "string" && version !== "" ? version : null;
}

/** 从 Cargo.toml 读 `[package]` 段的 version（依赖区同名键不算）。 */
function readCargoPackageVersion(text: string): string | null {
  let inPackage = false;
  for (const raw of text.split("\n")) {
    const line = raw.trim();
    if (line.startsWith("[")) {
      inPackage = line === "[package]";
      continue;
    }
    if (!inPackage) continue;
    const match = /^version\s*=\s*"([^"]+)"/.exec(line);
    if (match?.[1] !== undefined) return match[1];
  }
  return null;
}

/** 读取全部版本来源：文件缺失与字段读不出各记一条 problem（fail loud）。 */
export function readVersionSources(root: string): VersionSourceReading[] {
  return VERSION_SOURCES.map((file) => {
    const path = join(root, file);
    if (!existsSync(path)) {
      return { file, version: null, problem: "文件不存在" };
    }
    const text = readFileSync(path, "utf8");
    const version = file.endsWith(".toml") ? readCargoPackageVersion(text) : readJsonVersion(text);
    return version === null
      ? { file, version: null, problem: "读不出顶层 version 字段" }
      : { file, version, problem: null };
  });
}

/** 一致性判定：读失败或版本不等即返回失败原因清单（空数组 = 通过）。 */
export function versionProblems(readings: readonly VersionSourceReading[]): string[] {
  const problems: string[] = [];
  for (const reading of readings) {
    if (reading.problem !== null) {
      problems.push(`${reading.file}：${reading.problem}`);
    }
  }
  const versions = new Set(readings.map((r) => r.version).filter((v): v is string => v !== null));
  if (versions.size > 1) {
    problems.push(
      `版本号不一致：${readings
        .map((r) => `${r.file}=${r.version ?? "（读不出）"}`)
        .join(
          " / ",
        )}（三处必须相等：任一文件漏改都会让「关于」页、更新比较与安装包声明各说一个数）`,
    );
  }
  return problems;
}

function main(): void {
  const root = process.argv[2] ?? fileURLToPath(new URL("..", import.meta.url));
  const readings = readVersionSources(root);
  const problems = versionProblems(readings);

  if (problems.length > 0) {
    for (const problem of problems) console.error(`版本元数据一致性：${problem}`);
    console.error(`❌ 版本元数据一致性检查失败：${problems.length} 处问题（见 issue #1957）`);
    process.exit(1);
  }

  const version = readings[0]?.version ?? "";
  console.log(
    `✓ 版本元数据一致：${version}（${VERSION_SOURCES.join(" / ")}）——` +
      `「关于」页、更新比较与安装包声明同源`,
  );
}

// 仅直接运行时执行 main；被测试 import 时只取导出的判定函数。
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main();
}
