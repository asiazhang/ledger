#!/usr/bin/env bun

import { readFileSync } from "node:fs";
import { join } from "node:path";
import { pathToFileURL, fileURLToPath } from "node:url";
import {
  lineAt,
  maskComments,
  maskNonCode,
  RUST_EXTENSIONS,
  walkTextFiles,
} from "./gate-primitives.ts";

export interface ScanError {
  line: number;
  text: string;
}

export interface RustParam {
  name: string;
  injected: boolean;
}

export interface RustCommand {
  name: string;
  params: RustParam[];
}

export interface ScanResult {
  names: string[];
  commands: RustCommand[];
  errors: ScanError[];
}

function extractParamList(text: string): string | null {
  const start = text.indexOf("(");
  if (start === -1) return null;
  let depth = 0;
  for (let i = start; i < text.length; i++) {
    if (text[i] === "(") depth++;
    else if (text[i] === ")") {
      depth--;
      if (depth === 0) return text.slice(start + 1, i);
    }
  }
  return null;
}

function splitTopLevel(text: string): string[] {
  const pieces: string[] = [];
  let current = "";
  let angle = 0;
  let paren = 0;
  let bracket = 0;
  for (let i = 0; i < text.length; i++) {
    const c = text[i];
    if (c === "-" && text[i + 1] === ">") {
      current += "->";
      i++;
      continue;
    }
    if (c === "<") angle++;
    else if (c === ">") angle = Math.max(0, angle - 1);
    else if (c === "(") paren++;
    else if (c === ")") paren--;
    else if (c === "[") bracket++;
    else if (c === "]") bracket--;
    else if (c === "," && angle === 0 && paren === 0 && bracket === 0) {
      pieces.push(current);
      current = "";
      continue;
    }
    current += c;
  }
  pieces.push(current);
  return pieces.map((p) => p.trim()).filter((p) => p.length > 0);
}

const INJECTED_TYPES = new Set(["State", "AppHandle", "Window", "WebviewWindow"]);

function isInjectedType(typeText: string): boolean {
  const bare = typeText
    .replace(/\s+/g, " ")
    .trim()
    .replace(/^(?:&\s*mut\s+|&|mut\s+)+/, "")
    .replace(/^(?:tauri::|crate::|self::)+/, "");
  const m = bare.match(/^([A-Za-z_][A-Za-z0-9_]*)\s*(?:<.*)?$/s);
  return m !== null && INJECTED_TYPES.has(m[1]);
}

function parseParams(text: string): { params: RustParam[]; errors: string[] } {
  const params: RustParam[] = [];
  const errors: string[] = [];
  for (const piece of splitTopLevel(text)) {
    const m = piece.match(/^(?:mut\s+)?(?:r#)?([A-Za-z_][A-Za-z0-9_]*)\s*:\s*(.+)$/s);
    if (!m) {
      errors.push(`参数「${piece}」不是「参数名: 类型」形态，拒绝猜测`);
      continue;
    }
    params.push({ name: m[1], injected: isInjectedType(m[2]) });
  }
  return { params, errors };
}

export function toLowerCamelCase(name: string): string {
  const words = name.split(/[^A-Za-z0-9]+/).filter((w) => w.length > 0);
  if (words.length === 0) return "";
  return words
    .map((w, i) => (i === 0 ? w.toLowerCase() : w[0].toUpperCase() + w.slice(1).toLowerCase()))
    .join("");
}

export function scanRustSource(text: string): ScanResult {
  const names: string[] = [];
  const commands: RustCommand[] = [];
  const errors: ScanError[] = [];
  const lines = maskNonCode(text, true).split("\n");
  interface Pending {
    name: string;
    line: number;
    buffer: string;
  }
  const complete = (p: Pending): boolean => {
    const inner = extractParamList(p.buffer);
    if (inner === null) return false;
    const { params, errors: paramErrors } = parseParams(inner);
    for (const e of paramErrors) errors.push({ line: p.line, text: `命令 ${p.name}：${e}` });
    names.push(p.name);
    commands.push({ name: p.name, params });
    return true;
  };
  let armed = false;
  let pending: Pending | null = null;
  for (let i = 0; i < lines.length; i++) {
    const trimmed = lines[i].trim();
    if (pending) {
      pending.buffer += " " + trimmed;
      if (complete(pending)) pending = null;
      continue;
    }
    if (armed) {
      const m = trimmed.match(/^pub (?:async )?fn ([A-Za-z0-9_]+)/);
      if (m) {
        pending = { name: m[1], line: i + 1, buffer: trimmed.slice(m[0].length) };
        if (complete(pending)) pending = null;
      } else {
        errors.push({ line: i + 1, text: trimmed });
      }
      armed = false;
    } else if (trimmed === "#[tauri::command]") {
      armed = true;
    }
  }
  if (armed) {
    errors.push({ line: lines.length, text: "（文件以注解结尾，其后无 fn 定义）" });
  }
  if (pending) {
    errors.push({
      line: lines.length,
      text: `命令 ${pending.name} 参数列表括号不闭合（文件提前结束）`,
    });
  }
  return { names, commands, errors };
}

const TS_INVOKE_PATTERN = /\binvoke(?:<[^>]*>)?\(\s*(['"])([^'"]+)\1/g;

function matchInvokeCalls(masked: string): RegExpExecArray[] {
  return [...masked.matchAll(TS_INVOKE_PATTERN)];
}

export function scanTsSource(text: string): string[] {
  return matchInvokeCalls(maskComments(text)).map((m) => m[2]);
}

export interface TsInvokeCall {
  command: string;
  line: number;
  keys: string[];
  problems: string[];
}

function skipTrivia(text: string, start: number): number {
  let i = start;
  while (i < text.length) {
    if (/\s/.test(text[i])) {
      i++;
    } else if (text[i] === "/" && text[i + 1] === "/") {
      const nl = text.indexOf("\n", i);
      if (nl === -1) return text.length;
      i = nl + 1;
    } else if (text[i] === "/" && text[i + 1] === "*") {
      const end = text.indexOf("*/", i + 2);
      i = end === -1 ? text.length : end + 2;
    } else {
      return i;
    }
  }
  return i;
}

function skipString(text: string, start: number): number {
  const quote = text[start];
  let i = start + 1;
  while (i < text.length) {
    if (text[i] === "\\") {
      i += 2;
      continue;
    }
    if (text[i] === quote) return i + 1;
    if (quote === "`" && text[i] === "$" && text[i + 1] === "{") return -1;
    i++;
  }
  return -1;
}

function parseArgsObject(text: string, start: number): { keys: string[]; problems: string[] } {
  const keys: string[] = [];
  const problems: string[] = [];
  let i = skipTrivia(text, start);
  if (i >= text.length) {
    return { keys, problems: ["invoke 实参截断（文件提前结束），拒绝猜测"] };
  }
  if (text[i] === ")") return { keys, problems };
  if (text[i] === ",") i = skipTrivia(text, i + 1);
  if (i >= text.length || text[i] !== "{") {
    return { keys, problems: ["实参不是对象字面量，无法静态核对参数键名"] };
  }
  const entries: string[] = [];
  let current = "";
  let depth = 0;
  i++;
  while (i < text.length) {
    const c = text[i];
    if (c === "'" || c === '"' || c === "`") {
      const end = skipString(text, i);
      if (end === -1) {
        problems.push("实参含未闭合字符串或模板插值，无法静态核对参数键名");
        return { keys, problems };
      }
      current += text.slice(i, end);
      i = end;
      continue;
    }
    if (c === "/" && text[i + 1] === "/") {
      const nl = text.indexOf("\n", i);
      i = nl === -1 ? text.length : nl;
      continue;
    }
    if (c === "/" && text[i + 1] === "*") {
      const end = text.indexOf("*/", i + 2);
      i = end === -1 ? text.length : end + 2;
      continue;
    }
    if (c === "{" || c === "(" || c === "[") {
      depth++;
      current += c;
      i++;
      continue;
    }
    if (c === "}" || c === ")" || c === "]") {
      if (depth === 0) {
        if (c === "}") {
          entries.push(current);
          return { keys: finishEntries(entries, problems), problems };
        }
        problems.push(`实参对象以「${c}」意外闭合，无法静态核对参数键名`);
        return { keys, problems };
      }
      depth--;
      current += c;
      i++;
      continue;
    }
    if (c === "," && depth === 0) {
      entries.push(current);
      current = "";
      i++;
      continue;
    }
    current += c;
    i++;
  }
  problems.push("实参对象未闭合，无法静态核对参数键名");
  return { keys, problems };
}

function finishEntries(entries: string[], problems: string[]): string[] {
  const keys: string[] = [];
  for (const raw of entries) {
    const t = raw.trim();
    if (t === "") continue;
    if (t.startsWith("...")) {
      problems.push(`实参含展开语法「${t}」，无法静态核对参数键名`);
      continue;
    }
    if (t.startsWith("[")) {
      problems.push(`实参含计算键「${t}」，无法静态核对参数键名`);
      continue;
    }
    const m = t.match(/^([A-Za-z_$][A-Za-z0-9_$]*)\s*:/);
    if (m) {
      keys.push(m[1]);
      continue;
    }
    if (/^[A-Za-z_$][A-Za-z0-9_$]*$/.test(t)) {
      keys.push(t);
      continue;
    }
    problems.push(`实参键无法解析「${t}」，拒绝猜测`);
  }
  return keys;
}

export function scanTsInvokeCalls(text: string): TsInvokeCall[] {
  const calls: TsInvokeCall[] = [];
  for (const m of matchInvokeCalls(maskComments(text))) {
    const line = lineAt(text, m.index);
    const { keys, problems } = parseArgsObject(text, m.index + m[0].length);
    calls.push({ command: m[2], line, keys, problems });
  }
  return calls;
}

function collectRustFiles(dir: string): string[] {
  return walkTextFiles(dir, "", { extensions: RUST_EXTENSIONS }).map((f) => f.abs);
}

function main(): void {
  const repoRoot = fileURLToPath(new URL("..", import.meta.url));
  const commandsDir = process.argv[2] ?? join(repoRoot, "src-tauri", "src", "commands");
  const apiFile = process.argv[3] ?? join(repoRoot, "packages", "api", "src", "index.ts");

  const problems: string[] = [];
  const rustByName = new Map<string, string>();
  const rustKeys = new Map<string, string[]>();
  let rustParamTotal = 0;
  let expectedKeyTotal = 0;
  for (const file of collectRustFiles(commandsDir)) {
    const { commands, errors } = scanRustSource(readFileSync(file, "utf8"));
    for (const e of errors) {
      problems.push(
        `✗ 扫描器不认识的命令形态（${file}:${e.line}）：${e.text}\n` +
          `  扫描边界：只认裸 #[tauri::command] + 紧随 pub fn / pub async fn；` +
          `带参注解 / cfg 条件命令需同步扩展 src-tauri/build.rs 与本脚本的扫描规则（ADR-0047）`,
      );
    }
    for (const cmd of commands) {
      if (rustByName.has(cmd.name)) {
        problems.push(`✗ 命令名重复定义：${cmd.name}（${rustByName.get(cmd.name)} 与 ${file}）`);
      } else {
        rustByName.set(cmd.name, file);
      }
      const keys = cmd.params.filter((p) => !p.injected).map((p) => toLowerCamelCase(p.name));
      if (!rustKeys.has(cmd.name)) rustKeys.set(cmd.name, keys);
      rustParamTotal += cmd.params.length;
      expectedKeyTotal += keys.length;
    }
  }

  const tsText = readFileSync(apiFile, "utf8");
  const tsSet = new Set(scanTsSource(tsText));
  const tsCalls = scanTsInvokeCalls(tsText);

  if (rustByName.size === 0) {
    problems.push(
      "✗ 未在命令目录扫描到任何 #[tauri::command]——目录为空或路径指错，拒绝以空集假绿通过",
    );
  }
  if (tsSet.size === 0) {
    problems.push("✗ 未在 TS 调用面扫描到任何 invoke 调用——文件为空或路径指错，拒绝以空集假绿通过");
  }
  if (rustByName.size > 0 && rustParamTotal === 0) {
    problems.push("✗ 命令已扫到但未扫出任何参数——参数提取失灵信号，拒绝以空键集假绿通过");
  }

  const missingInTs = [...rustByName.keys()].filter((n) => !tsSet.has(n)).sort();
  const missingInRust = [...tsSet].filter((n) => !rustByName.has(n)).sort();
  if (missingInTs.length > 0) {
    problems.push(
      `✗ 仅在 Rust 注解侧（TS 调用面缺方法，packages/api/src/index.ts 补 invoke 方法）：\n` +
        missingInTs.map((n) => `  - ${n}`).join("\n"),
    );
  }
  if (missingInRust.length > 0) {
    problems.push(
      `✗ 仅在 TS 调用面（Rust 无此命令，删除调用或补后端命令）：\n` +
        missingInRust.map((n) => `  - ${n}`).join("\n"),
    );
  }

  const callsByCommand = new Map<string, TsInvokeCall[]>();
  for (const call of tsCalls) {
    if (!callsByCommand.has(call.command)) callsByCommand.set(call.command, []);
    callsByCommand.get(call.command)!.push(call);
    if (!rustByName.has(call.command) || !tsSet.has(call.command)) continue;
    for (const p of call.problems) {
      problems.push(`✗ invoke('${call.command}')（api 第 ${call.line} 行）：${p}`);
    }
    const expected = rustKeys.get(call.command) ?? [];
    const missing = expected.filter((k) => !call.keys.includes(k));
    const unknown = call.keys.filter((k) => !expected.includes(k));
    if (missing.length > 0) {
      problems.push(
        `✗ 参数键名失配：${call.command}（api 第 ${call.line} 行）实参缺键：` +
          `${missing.join(", ")}（Rust 参数键：${expected.join(", ") || "（无）"}；` +
          `Tauri 按参数名 lowerCamelCase 绑定，缺键绑定 None，#588 类回归）`,
      );
    }
    if (unknown.length > 0) {
      problems.push(
        `✗ 参数键名失配：${call.command}（api 第 ${call.line} 行）实参多键：` +
          `${unknown.join(", ")}（Rust 参数键：${expected.join(", ") || "（无）"}；` +
          `Tauri 静默丢弃未知键，snake_case 键静默绑 None，#588 类回归）`,
      );
    }
  }
  for (const [command, calls] of callsByCommand) {
    if (calls.some((c) => c.problems.length > 0)) continue;
    if (!rustByName.has(command) || !tsSet.has(command)) continue;
    const signatures = [...new Set(calls.map((c) => [...c.keys].sort().join(", ")))];
    if (signatures.length > 1) {
      problems.push(
        `✗ 参数键名不一致：${command} 的 ${calls.length} 处调用点键集不同（` +
          signatures.map((s) => `{ ${s} }`).join(" vs ") +
          `），多处调用点键集必须互相一致`,
      );
    }
  }

  if (problems.length > 0) {
    for (const p of problems) console.error(`命令注册一致性：${p}`);
    console.error(
      `❌ 命令注册一致性校验失败：${problems.length} 处问题` +
        `（命令单一来源 = #[tauri::command] 注解；命令名与参数键名两侧均须双向全等，` +
        `见 ADR-0047 与 issue #1398）`,
    );
    process.exit(1);
  }
  console.log(
    `✓ 命令注册一致性：Rust 注解 ${rustByName.size} ↔ TS 调用面 ${tsSet.size}，双向全等；` +
      `参数键名：${tsCalls.length} 处 invoke 调用点 ↔ 期望键 ${expectedKeyTotal} 个，双向全等`,
  );
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main();
}
