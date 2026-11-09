#!/usr/bin/env bun
// 全仓无 tauri 守门（issue #2083 / ADR-0145 决策 4/7 切轨后置门）：Tauri 壳退役
// 后的静态形状门——清单形状面 + 前端死路径面，与命令型门
// `cargo tree --workspace -i tauri`（check.sh / CI backend-lint 同挂，辖**解析后的
// 依赖全图**，含传递边）互补。本门辖「声明形状」：
//
// 面① Cargo 清单（src-app/Cargo.toml + src-app/crates/*/Cargo.toml）：注释掩码后
//   出现 `tauri` 词即红（TOML 注释掩码后）——覆盖 [dependencies] / [dev-dependencies] /
//   [build-dependencies] / [features] / [workspace.dependencies] 全段；tauri
//   只经清单进入依赖图，清单零声明即图零可达（与 cargo tree 门双保险：彼门
//   辖解析结果，本门辖声明文本，漂移任一侧即红）。
// 面② 前端死路径（src/ + packages/*/src/ 的 .ts/.vue + 全部 package.json）：
//   `@tauri-apps` 出现即红——import / 动态 import / vi.mock 模块替身声明 /
//   依赖键全是 tauri 壳专属死路径；Web 参照前端的唯一后端是 mock
//   （ADR-0145 决策 1 修订），tauri API 真模块已随壳退役、任何引用都不可解析。
//
// 判定语义（fail loud）：每面至少扫到一个文件（拒绝空集假绿，与 check-structure
// 同款哨兵）；注释掩码：TS/Vue 面 maskComments（gate-primitives 库）、Cargo 清单
// 自持 TOML 行注释掩码（`#` 行注释，字符串感知）——注释里的历史提及时过境迁
// 不误报，声明与字面命中才算。Cargo 清单掩码后**全文**匹配（含字符串值：
// description 提及 tauri 同样视为残留——切轨后不应有任何需要提及 tauri 的清单内容）。
// TypeScript 化 + Bun 运行时（issue #734 / ADR-0083）：类型经 tsconfig.scripts.json
// 门槛检查；调用方式 `bun scripts/check-no-tauri.ts`。
// 默认校验本仓库；测试可传位置参数指向夹具仓库根：`bun scripts/check-no-tauri.ts [repo-root]`。
// 挂载登记归 scripts/gate-mounts.ts（check.sh + CI 双宿主，issue #1682）。
// 包装测试 scripts/check-no-tauri.test.ts。

import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { maskComments } from "./gate-primitives.ts";

/** Cargo 清单出现即红的词（词边界：tauri-plugin-* / tauri-build 同命中）。 */
const TAURI_WORD = /\btauri\b/;

/** 前端源码出现即红的包作用域（import / vi.mock / 动态 import 皆经它解析）。 */
const TAURI_PACKAGE_SCOPE = /@tauri-apps/;

/** 面①扫描根（相对仓库根）：Rust workspace 根与成员 crate 清单。 */
export const CARGO_MANIFEST_GLOB_DIRS = ["src-app", "src-app/crates"] as const;

/** 面②前端源码扫描根（相对仓库根）。 */
export const FRONTEND_SCAN_ROOTS = ["src", "packages"] as const;

/** 前端源码扩展名（.vue 单文件组件的 script 块经文本级扫描覆盖）。 */
const FRONTEND_EXTENSIONS = new Set([".ts", ".tsx", ".vue"]);

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

/** 仓库内全部 package.json（根 + packages/* 成员；夹具根同构）。 */
function packageJsons(root: string): string[] {
  const out: string[] = [join(root, "package.json")];
  const packagesDir = join(root, "packages");
  if (existsSync(packagesDir)) {
    for (const name of readdirSync(packagesDir)) {
      const manifest = join(packagesDir, name, "package.json");
      if (statSync(join(packagesDir, name)).isDirectory() && existsSync(manifest)) {
        out.push(manifest);
      }
    }
  }
  return out.filter((p) => existsSync(p)).sort();
}

/** 前端源码文件清单（src/ 全树 + packages/<pkg>/src/ 全树）。 */
function frontendSources(root: string): string[] {
  const out: string[] = [];
  const walk = (dir: string): void => {
    if (!existsSync(dir)) return;
    for (const name of readdirSync(dir)) {
      if (name === "node_modules" || name === "dist") continue;
      const abs = join(dir, name);
      const stat = statSync(abs);
      if (stat.isDirectory()) walk(abs);
      else if (FRONTEND_EXTENSIONS.has(name.slice(name.lastIndexOf(".")))) out.push(abs);
    }
  };
  for (const rel of FRONTEND_SCAN_ROOTS) {
    const abs = join(root, rel);
    if (!existsSync(abs)) continue;
    if (rel === "packages") {
      for (const name of readdirSync(abs)) {
        const src = join(abs, name, "src");
        if (statSync(join(abs, name)).isDirectory()) walk(src);
      }
    } else {
      walk(abs);
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

/** 面①：Cargo 清单注释掩码后 `tauri` 词扫描。 */
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

/** 面②：前端源码 `@tauri-apps` 扫描（注释掩码后匹配，历史注释提及不误报）。 */
export function scanFrontendSources(root: string): TauriHit[] {
  const hits: TauriHit[] = [];
  for (const abs of frontendSources(root)) {
    const rel = abs.slice(resolve(root).length + 1);
    const masked = maskComments(readFileSync(abs, "utf8"));
    const rawLines = masked.split("\n");
    for (let i = 0; i < rawLines.length; i++) {
      if (TAURI_PACKAGE_SCOPE.test(rawLines[i] ?? "")) {
        hits.push({ file: rel, line: i + 1, text: (rawLines[i] ?? "").trim() });
      }
    }
  }
  return hits;
}

/** 面②清单侧：package.json 依赖键 / scripts 键的 `@tauri-apps` / `tauri` 扫描。 */
export function scanPackageJsons(root: string): TauriHit[] {
  const hits: TauriHit[] = [];
  for (const abs of packageJsons(root)) {
    const rel = abs.slice(resolve(root).length + 1);
    const rawLines = readFileSync(abs, "utf8").split("\n");
    for (let i = 0; i < rawLines.length; i++) {
      const line = rawLines[i] ?? "";
      // 依赖键 `"@tauri-apps/…"：` 与脚本键 `"tauri":`（键形态判定，值里的包名
      // 由面②源码扫描辖）。注释掩码不适用（JSON 无注释语法）。
      if (/^\s*"@tauri-apps\//.test(line) || /^\s*"tauri"\s*:/.test(line)) {
        hits.push({ file: rel, line: i + 1, text: line.trim() });
      }
    }
  }
  return hits;
}

function main(): void {
  const repoRoot = process.argv[2] ?? fileURLToPath(new URL("..", import.meta.url));
  const problems: string[] = [];
  const cargoHits = scanCargoManifests(repoRoot);
  const frontendHits = scanFrontendSources(repoRoot);
  const packageJsonHits = scanPackageJsons(repoRoot);

  if (cargoManifests(repoRoot).length === 0) {
    problems.push("✗ 全仓无 tauri 守门：Cargo 清单扫描集为空（src-app 根缺失？）——拒绝空集假绿");
  }
  for (const hit of cargoHits) {
    problems.push(
      `✗ 全仓无 tauri 守门（Cargo 清单）：${hit.file}:${hit.line} 出现 tauri 声明\n` +
        `    ${hit.text}\n` +
        "    Tauri 壳已随 #2083 切轨退役（ADR-0145 决策 4/7）——清单零声明即依赖图\n" +
        "    零可达；命令型门 `cargo tree --workspace -i tauri` 辖解析结果，本面辖声明文本",
    );
  }
  for (const hit of frontendHits) {
    problems.push(
      `✗ 全仓无 tauri 守门（前端死路径）：${hit.file}:${hit.line} 引用 @tauri-apps\n` +
        `    ${hit.text}\n` +
        "    Web 参照前端的唯一后端是 mock（ADR-0145 决策 1 修订）；tauri API 真模块\n" +
        "    已随壳退役，任何 import / vi.mock / 动态 import 都是不可解析的死路径",
    );
  }
  for (const hit of packageJsonHits) {
    problems.push(
      `✗ 全仓无 tauri 守门（package.json）：${hit.file}:${hit.line} 出现 tauri 依赖键或脚本\n` +
        `    ${hit.text}\n` +
        "    tauri CLI / @tauri-apps/* 依赖与 tauri 脚本随壳退役（#2083），删除即变绿",
    );
  }

  if (problems.length > 0) {
    for (const p of problems) console.error(p);
    console.error(
      `❌ 全仓无 tauri 守门失败：${problems.length} 处残留（Cargo 清单 ${cargoHits.length} + ` +
        `前端源码 ${frontendHits.length} + package.json ${packageJsonHits.length}；issue #2083 / ADR-0145）`,
    );
    process.exit(1);
  }
  console.log(
    `✓ 全仓无 tauri 守门：Cargo 清单 ${cargoManifests(repoRoot).length} 份零 tauri 声明 · ` +
      `前端源码 ${frontendSources(repoRoot).length} 个文件零 @tauri-apps 引用 · ` +
      `package.json ${packageJsons(repoRoot).length} 份零 tauri 依赖键/脚本（#2083 / ADR-0145）`,
  );
}

// 仅直接运行时执行 main；被测试/其他工具 import 时只取导出的扫描函数。
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main();
}
