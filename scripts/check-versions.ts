#!/usr/bin/env bun

import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

export interface VersionSourceReading {
  file: string;
  version: string | null;
  problem: string | null;
}

export const VERSION_SOURCES: readonly string[] = [
  "package.json",
  "src-tauri/tauri.conf.json",
  "src-tauri/Cargo.toml",
];

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

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main();
}
