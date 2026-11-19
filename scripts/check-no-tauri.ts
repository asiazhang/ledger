#!/usr/bin/env bun
// 全仓无 tauri 守门（issue #2083 / ADR-0145 决策 4/7 切轨后置门）：清单形状面，与命令型门
// `cargo tree --workspace -i tauri`（check.sh / CI backend-lint 同挂，辖**解析后的
// 依赖全图**，含传递边）互补。本门辖「声明形状」：
//
// 面① Cargo 清单（src-app/Cargo.toml + src-app/crates/*/Cargo.toml）：注释掩码后
//   出现 `tauri` 词即红（TOML 注释掩码后）——覆盖 [dependencies] / [dev-dependencies] /
//   [build-dependencies] / [features] / [workspace.dependencies] 全段；tauri
//   只经清单进入依赖图，清单零声明即图零可达（与 cargo tree 门双保险：彼门
//   辖解析结果，本门辖声明文本，漂移任一侧即红）。
//
// 前端死路径面（原面②：src/ + packages/ 的 .ts/.vue + package.json 的
// @tauri-apps 扫描）随 Web 参照前端整体退役删除（#2171/#2172）——扫描根
// 不复存在，面归 Cargo 清单单一面。
//
// 判定语义（fail loud）：扫描面至少扫到一个文件（拒绝空集假绿，与 check-structure
// 同款哨兵）；注释掩码：Cargo 清单自持 TOML 行注释掩码（`#` 行注释，字符串感知）
// ——注释里的历史提及时过境迁不误报，声明与字面命中才算。Cargo 清单掩码后
// **全文**匹配（含字符串值：description 提及 tauri 同样视为残留——切轨后不应有
// 任何需要提及 tauri 的清单内容）。
// TypeScript 化 + Bun 运行时（issue #734 / ADR-0083）：类型经 tsconfig.scripts.json
// 门槛检查；调用方式 `bun scripts/check-no-tauri.ts`。
// 默认校验本仓库；测试可传位置参数指向夹具仓库根：`bun scripts/check-no-tauri.ts [repo-root]`。
// 挂载登记归 scripts/gate-mounts.ts（check.sh + CI 双宿主，issue #1682）。
// 包装测试 scripts/check-no-tauri.test.ts。

import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

/** Cargo 清单出现即红的词（词边界：tauri-plugin-* / tauri-build 同命中）。 */
const TAURI_WORD = /\btauri\b/;

/** 扫描根（相对仓库根）：Rust workspace 根与成员 crate 清单。 */
export const CARGO_MANIFEST_GLOB_DIRS = ["src-app", "src-app/crates"] as const;

/** TOML 注释掩码（逐行，不跨行）：行内首个 `#` 起到行尾换空格——TOML 的 `#`
 *  只在字符串字面量外成注释；清单内的注释提及（历史门退役注记等）不算声明。 */
function maskTomlComments(text: string): string {
  return text
    .split("\n")
    .map((line) => {
      const chars = line.split("");
      let inString = false;
      let quote = "";
      for (let i = 0; i < chars.length; i++) {
        const c = chars[i] ?? "";
        if (inString) {
          if (c === "\\") {
            i += 1;
            continue;
          }
          if (c === quote) inString = false;
          continue;
        }
        if (c === '"' || c === "'") {
          inString = true;
          quote = c;
        } else if (c === "#") {
          for (let k = i; k < chars.length; k++) chars[k] = " ";
          break;
        }
      }
      return chars.join("");
    })
    .join("\n");
}

/** 仓库内全部 Cargo.toml 清单（workspace 根 + crates 成员；夹具根同构）。 */
function cargoManifests(root: string): string[] {
  const out: string[] = [];
  for (const dir of CARGO_MANIFEST_GLOB_DIRS) {
    const abs = join(root, dir, "Cargo.toml");
    if (existsSync(abs)) out.push(abs);
  }
  const cratesDir = join(root, "src-app", "crates");
  if (existsSync(cratesDir)) {
    for (const name of readdirSync(cratesDir)) {
      const manifest = join(cratesDir, name, "Cargo.toml");
      if (statSync(join(cratesDir, name)).isDirectory() && existsSync(manifest)) out.push(manifest);
    }
  }
  return out.sort();
}

/** 单条命中：文件（相对仓库根）+ 行号（1 起算）+ 行原文。 */
export interface TauriHit {
  file: string;
  line: number;
  text: string;
}

/** Cargo 清单注释掩码后 `tauri` 词扫描。 */
export function scanCargoManifests(root: string): TauriHit[] {
  const hits: TauriHit[] = [];
  for (const abs of cargoManifests(root)) {
    const rel = abs.slice(resolve(root).length + 1);
    const masked = maskTomlComments(readFileSync(abs, "utf8"));
    const rawLines = masked.split("\n");
    for (let i = 0; i < rawLines.length; i++) {
      if (TAURI_WORD.test(rawLines[i] ?? "")) {
        hits.push({ file: rel, line: i + 1, text: (rawLines[i] ?? "").trim() });
      }
    }
  }
  return hits;
}

function main(): void {
  const repoRoot = process.argv[2] ?? fileURLToPath(new URL("..", import.meta.url));
  const problems: string[] = [];
  const cargoHits = scanCargoManifests(repoRoot);

  if (cargoManifests(repoRoot).length === 0) {
    problems.push("✗ 全仓无 tauri 守门：Cargo 清单扫描集为空（src-app 根缺失？）——拒绝空集假绿");
  }
  for (const hit of cargoHits) {
    problems.push(
      `✗ 全仓无 tauri 守门（Cargo 清单）：${hit.file}:${hit.line} 出现 tauri 声明\n` +
        `    ${hit.text}\n` +
        "    Tauri 壳已随 #2083 切轨退役（ADR-0145 决策 4/7）——清单零声明即依赖图\n" +
        "    零可达；命令型门 `cargo tree --workspace -i tauri` 辖解析结果，本门辖声明文本",
    );
  }

  if (problems.length > 0) {
    for (const p of problems) console.error(p);
    console.error(
      `❌ 全仓无 tauri 守门失败：${problems.length} 处残留（Cargo 清单 ${cargoHits.length}；` +
        `issue #2083 / ADR-0145）`,
    );
    process.exit(1);
  }
  console.log(
    `✓ 全仓无 tauri 守门：Cargo 清单 ${cargoManifests(repoRoot).length} 份零 tauri 声明（#2083 / ADR-0145）`,
  );
}

// 仅直接运行时执行 main；被测试/其他工具 import 时只取导出的扫描函数。
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main();
}
