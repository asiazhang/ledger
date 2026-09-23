#!/usr/bin/env bun
// 守门家族共享原语库（issue #1680）：库归库、门归门——对「受守代码长什么样」不持
// 立场的纯机制住本模块；持立场的（测试豁免谓词、扫描边界扩展名表、豁免目录/文件
// 清单）是政策，留各门。此前原语住在最大守门脚本 check-structure.ts 内
// （#1625/#1634/#1637 三次收口把遍历与行号「收」进了以结构守门命名的巨型脚本），
// 兄弟脚本 import 的是一个门——本模块把库与门分离，改遍历语义不必再在两千行守门
// 逻辑里定位它，新增扫描类守门直接 import，不再抄第五份遍历。
//
// 闭集七件（#1680 五件 + #1742 增补平铺面两件）：
// - walkTextFiles / WalkedFile / WalkTextOptions —— 递归目录遍历（扩展名闭集过滤、
//   localeCompare 排序保证输出确定、rel 以 relBase 为前缀归一）；扫描哪些扩展名、
//   豁免哪些目录与文件属各守门政策，经 options 注入。
// - lineAt —— 命中下标 → 行号（1 起算），matchAll 逐命中定位 文件:行 的唯一实现。
// - maskNonCode —— Rust 词法掩码（注释与字符串/char 字面量掩为等长空白、保留换行
//   列位；keepLiterals=true 只掩注释），私有助手 rawStringOpenQuoteAt。
// - RUST_EXTENSIONS —— Rust 面扩展名闭集（全等副本收口单点，消费方直接 import）。
// - maskComments —— TS/Vue 注释掩码（自 ts-comment-mask.ts 并入：消费方
//   check-frontend-structure.ts 与 check-commands.ts 恰 2 门，达 ≥2 门准入判据）。
// #1742 增补（平铺 readdir 与内联 endsWith 的收口/留门判定票，逐处留痕见票）：
// - readDirEntries / DirEntry —— 只读一层目录列举（localeCompare 排序 + dirent 类型
//   投影，不跟随目录符号链接，与 walkTextFiles 同语义；与递归遍历形状不同构，单层
//   语义单独单点）。
// - hasExtension —— 点扩展名闭集匹配（walkTextFiles 内部同款）；残余内联
//   endsWith(".rs")（test-exec ×3、check-structure ×1）换库 RUST_EXTENSIONS 消费。
//
// 消费方（守门家族全量，#1680 T2 收口后）：check-structure、check-infra-dml、
// check-eastmoney-residue、check-background-services、test-exec、check-commands、
// check-frontend-structure、check-async-guards、check-test-support、check-test-stubs、
// check-i18n-keys、check-dialog-forms、check-style-blocks，与原语级测试
// gate-primitives.test.ts。掩码与遍历规则改动只动本文件，消费者随引用自动跟随。
//
// 边界（#1466/#1433 划界声明，自 ts-comment-mask.ts 头部迁入）：maskComments 只
// 承载 TS/Vue 注释掩码。面向 Rust 源码的词法掩码，本模块 maskNonCode 是 TS 侧
// 载体，与 Rust 侧唯一实现 `src-tauri/src/test_support/scan.rs` 的
// `mask_non_code` 是同一条词法规则的两个运行时载体（双源登记，规则改动必须两侧
// 同步）；防漂移断言消费共享语料夹具 `scripts/fixtures/rust-mask-corpus.rs`
// （gate-primitives.test.ts 与 Rust 测试双侧消费，任一侧单独改规则即红）。
//
// TypeScript 化 + Bun 运行时（issue #734 / ADR-0083）：类型经 tsconfig.scripts.json
// 门槛检查。本模块是库不是门：不被 check.sh 直接调用、不进守门挂载登记（#1682
// 的登记闭集 = 可执行门脚本）。

import { readdirSync } from "node:fs";
import { join } from "node:path";

/** 遍历收集的文本面文件：绝对路径（读文件用）+ 相对路径（报文定位用，`/` 分隔） */
export interface WalkedFile {
  abs: string;
  rel: string;
}

/** 文本面遍历政策（各守门自己的豁免面，经参数注入，不在共享单点内） */
export interface WalkTextOptions {
  /** 收集的扩展名闭集（含点，如 ".rs"）；按文件名最后一个点之后的后缀匹配 */
  extensions: ReadonlySet<string>;
  /** 目录名剪枝（整目录豁免，如 node_modules / target / docs） */
  skipDirs?: ReadonlySet<string>;
  /** 相对路径剪枝（整文件豁免，如 CHANGELOG.md） */
  skipFiles?: ReadonlySet<string>;
}

/**
 * 递归收集目录下的文本面文件（守门家族共享单点，issue #1625）：扩展名闭集过滤、
 * 目录名 localeCompare 排序保证输出确定、rel 以 relBase 为前缀 `/` 分隔归一。
 * 目录判定按 readdir dirent（不跟随目录符号链接；此语义随 #1680 收口成为家族
 * 统一行为——原自持遍历中 statSync 跟随符号链接的形态一并归一，本仓扫描树无
 * 目录符号链接，实害为零、留痕在案）。扫描哪些扩展名、豁免哪些目录与文件属
 * 各守门政策，经 options 注入。
 */
export function walkTextFiles(
  dir: string,
  relBase: string,
  options: WalkTextOptions,
): WalkedFile[] {
  const out: WalkedFile[] = [];
  const walk = (current: string, rel: string): void => {
    for (const entry of readdirSync(current, { withFileTypes: true }).sort((a, b) =>
      a.name.localeCompare(b.name),
    )) {
      const entryAbs = join(current, entry.name);
      const entryRel = rel ? `${rel}/${entry.name}` : entry.name;
      if (entry.isDirectory()) {
        if (!options.skipDirs?.has(entry.name)) walk(entryAbs, entryRel);
        continue;
      }
      if (!hasExtension(entry.name, options.extensions)) continue;
      if (options.skipFiles?.has(entryRel)) continue;
      out.push({ abs: entryAbs, rel: entryRel });
    }
  };
  walk(dir, relBase);
  return out;
}

/** 单层目录列举条目：dirent 类型投影（一层列举的读取面，政策过滤留消费侧） */
export interface DirEntry {
  name: string;
  isDirectory: boolean;
  isFile: boolean;
}

/**
 * 只读一层的目录列举（守门家族共享单点，issue #1742）：平铺 readdir 与递归遍历
 * 形状不同构，单层语义在此统一——localeCompare 排序保证输出确定（与 walkTextFiles
 * 同款家族统一行为；直接消费列举序的站点原 plain sort / readdir 原序已实树逐目录
 * 对拍全等（packages/、crates/、migrations/、locales/*），列举序被下游再排序吸收
 * 的站点（check-structure diskModuleKeys）对拍不参与、结果与列举序无关，#1742 留痕）、
 * dirent 类型判定不跟随目录符号链接（同 walkTextFiles）。
 * 列举哪些、怎么过滤属各守门政策，消费侧自持；目录不存在按 readdirSync 原样抛错
 * （调用侧的 existsSync 守卫各自保留，不静默空集）。
 */
export function readDirEntries(dir: string): DirEntry[] {
  return readdirSync(dir, { withFileTypes: true })
    .sort((a, b) => a.name.localeCompare(b.name))
    .map((entry) => ({
      name: entry.name,
      isDirectory: entry.isDirectory(),
      isFile: entry.isFile(),
    }));
}

/**
 * 命中行定位：匹配下标 → 行号（1 起算，数命中点之前的换行，undefined 按文首计）。
 * 守门家族共享单点（issue #1625）：matchAll 逐命中定位 文件:行 的唯一实现，
 * check-infra-dml 的 scanDml 与 check-eastmoney-residue 的 scanResidue 消费。
 */
export function lineAt(text: string, index: number | undefined): number {
  return (text.slice(0, index ?? 0).match(/\n/g)?.length ?? 0) + 1;
}

/** 若 i 起是 Rust 原始字符串前缀，返回其后开引号下标；否则 null。
 *  覆盖 r"…" / r#"…" 与字节变体 br"…" / br#"…"，# 数任意；前一字符为
 *  标识符成分时是普通名字（如 for），不误伤。 */
function rawStringOpenQuoteAt(text: string, i: number): number | null {
  const prev = i > 0 ? text[i - 1] : "";
  if (/[A-Za-z0-9_]/.test(prev)) return null;
  let j = i;
  if (text[j] === "b" && text[j + 1] === "r") j += 2;
  else if (text[j] === "r") j += 1;
  else return null;
  while (text[j] === "#") j++;
  return text[j] === '"' ? j : null;
}

/**
 * 掩码 Rust 源文本中的注释与字符串/char 字面量：内容替换为等长空白
 * （保留换行与列位，行号不变），使依赖扫描只落在真实代码上。
 * 处理形态：行注释（//、///、//!）、块注释（/* .. *&#47;，可嵌套）、
 * 普通字符串（含转义）、原始字符串 r"…" / r#"…" / r##"…" 及其字节变体
 * br"…" / br#"…" / br##"…"（# 数任意；'\u{…}' 转义不按字面量识别——与
 * Rust 侧一致，见下）、
 * char 字面量（'a'、'\n'、'\\'、'\''）；生命周期标注（'a）按非字面量处理。
 * `keepLiterals=true` 时保留字符串/char 字面量内容、只掩码注释——用于靶形态
 * 落在字符串里的扫描（原生事务语句 `execute("BEGIN")`，issue #1014；码化错误
 * 构造点第一参数与 `const NAME: &str` 值、命令注解腿整文掩码，两份弱形态注释
 * 处理器随 #1741 收口并入）。
 *
 * **双源登记**（issue #1433）：本函数与 Rust 侧唯一实现
 * `src-tauri/src/test_support/scan.rs` 的 `mask_non_code` 是同一条词法掩码规则
 * 的两个运行时载体，规则改动必须两侧同步；防漂移断言消费共享语料夹具
 * `scripts/fixtures/rust-mask-corpus.rs`（gate-primitives.test.ts 与 Rust 测试
 * 双侧消费，任一侧单独改规则即红）。TS 侧消费面：check-structure、check-infra-dml、
 * check-background-services、test-exec、check-test-support（#1680 起第三份实现
 * 收口至此）、check-commands（命令注解腿整文掩码）、check-i18n-keys（码化构造
 * 点提取腿，二者均为 #1741 弱形态副本收口），消费名单由 gate-primitives.test.ts
 * 锁定；keepLiterals 形态无 Rust 侧
 * 对应物（scan.rs 头注登记在案的不对称），由 keepLiterals 语料期望单侧锁定。
 */
export function maskNonCode(text: string, keepLiterals = false): string {
  const out = text.split("");
  const n = text.length;
  const blank = (from: number, to: number): void => {
    for (let k = from; k < to && k < n; k++) if (out[k] !== "\n") out[k] = " ";
  };
  let i = 0;
  while (i < n) {
    const c = text[i];
    if (c === "/" && text[i + 1] === "/") {
      // 行注释（含 /// 与 //!）到行尾
      const end = text.indexOf("\n", i);
      const stop = end === -1 ? n : end;
      blank(i, stop);
      i = stop;
    } else if (c === "/" && text[i + 1] === "*") {
      // 块注释，Rust 可嵌套
      let depth = 1;
      let j = i + 2;
      while (j < n && depth > 0) {
        if (text[j] === "/" && text[j + 1] === "*") {
          depth++;
          j += 2;
        } else if (text[j] === "*" && text[j + 1] === "/") {
          depth--;
          j += 2;
        } else {
          j++;
        }
      }
      blank(i, j);
      i = j;
    } else if (c === '"') {
      // 普通字符串：跳过转义对
      let j = i + 1;
      while (j < n) {
        if (text[j] === "\\") j += 2;
        else if (text[j] === '"') {
          j++;
          break;
        } else j++;
      }
      if (!keepLiterals) blank(i, j);
      i = j;
    } else if (c === "r" || c === "b") {
      // 原始字符串 r"…" / r#"…" / r##"…" 与字节变体 br"…" / br#"…" / br##"…"
      const open = rawStringOpenQuoteAt(text, i);
      if (open === null) {
        i++;
        continue;
      }
      const prefixEnd = c === "b" ? i + 2 : i + 1;
      const hashes = open - prefixEnd;
      const close = '"' + "#".repeat(hashes);
      const end = text.indexOf(close, open + 1);
      const stop = end === -1 ? n : end + close.length;
      if (!keepLiterals) blank(i, stop);
      i = stop;
    } else if (c === "'") {
      // char 字面量 vs 生命周期：有闭引号为字面量，否则是生命周期标注（'a）
      let j = i + 1;
      if (text[j] === "\\") {
        j++;
        if (text[j] === "{") {
          const e = text.indexOf("}", j);
          j = e === -1 ? n : e + 1;
        } else {
          j++;
        }
      } else {
        j++;
      }
      if (text[j] === "'") {
        const stop = j + 1;
        if (!keepLiterals) blank(i, stop);
        i = stop;
      } else {
        i++;
      }
    } else {
      i++;
    }
  }
  return out.join("");
}

/**
 * 点扩展名闭集匹配：取名字最后一个点之后的后缀对集合判定（与 walkTextFiles 的
 * 遍历内匹配同款，issue #1742）。对形如 ".rs" 的点扩展名与 endsWith 全等（裸扩展
 * 名 ".rs"、多点 "a.b.rs"、"a.rs.txt" 等边缘形态由 gate-primitives.test.ts 全等表
 * 钉住）——残余内联 endsWith(".rs") 收口至此 + RUST_EXTENSIONS。
 */
export function hasExtension(name: string, extensions: ReadonlySet<string>): boolean {
  const dot = name.lastIndexOf(".");
  return dot !== -1 && extensions.has(name.slice(dot));
}

/** Rust 面扩展名闭集（walkTextFiles 消费参数；全等副本收口单点，issue #1680） */
export const RUST_EXTENSIONS: ReadonlySet<string> = new Set([".rs"]);

/** 若 text[start] 起是字符串/模板字面量，返回结束引号后的下标；未闭合则返回文末。
 *  否则返回 -1。只用于跳过字面量内容以识别注释起点，字面量本身不掩码。 */
function stringLiteralEnd(text: string, start: number): number {
  const quote = text[start];
  if (quote !== "'" && quote !== '"' && quote !== "`") return -1;
  let i = start + 1;
  while (i < text.length) {
    if (text[i] === "\\") {
      i += 2;
      continue;
    }
    if (text[i] === quote) return i + 1;
    i++;
  }
  return text.length;
}

/** 掩码 TS/Vue 源文本中的注释（行注释与块注释）：内容替换为等长空白（保留换行与
 *  列位），使 import / invoke 扫描只落在真实代码上。字面量内容不掩码——import 说明符
 *  本身是字符串、invoke 命令名也是字符串，掩码会连靶一起抹掉；但识别注释起点前先
 *  跳过字符串/模板字面量，否则字面量里的 // 或 /* 会被误当注释、吞掉同行与前后的
 *  真实代码（把真实 import 或 invoke 藏出检测面，issue #1471）。正则字面量体内未转义
 *  的 / 会终止字面量，故体内 / 必为 \/，紧邻的反斜杠即「正则内斜杠」信号——带转义的
 *  / 不作注释起点，正则字面量的 \/\/ 形态同样不吞其后的真实代码。正则字面量整体词法
 *  不做（残余：字符类内相邻斜杠如 /[//]/、模板 ${} 插值——本仓零现役命中，评审兜底）；
 *  字符串内的伪 import 仍靠关键词上下文排除。
 *  单一维护点（issue #1481）：check-frontend-structure.ts 与 check-commands.ts 消费
 *  本出口，掩码规则改动只动本文件。 */
export function maskComments(text: string): string {
  const out = text.split("");
  const n = text.length;
  const blank = (from: number, to: number): void => {
    for (let k = from; k < to && k < n; k++) if (out[k] !== "\n") out[k] = " ";
  };
  let i = 0;
  while (i < n) {
    // text[i - 1] === '\\'：正则字面量体内的转义斜杠，不是注释起点（issue #1471）。
    if (text[i] === "/" && text[i - 1] !== "\\" && text[i + 1] === "/") {
      const stop = text.indexOf("\n", i) === -1 ? n : text.indexOf("\n", i);
      blank(i, stop);
      i = stop;
    } else if (text[i] === "/" && text[i - 1] !== "\\" && text[i + 1] === "*") {
      const end = text.indexOf("*/", i + 2);
      const stop = end === -1 ? n : end + 2;
      blank(i, stop);
      i = stop;
    } else {
      const literalEnd = stringLiteralEnd(text, i);
      i = literalEnd === -1 ? i + 1 : literalEnd;
    }
  }
  return out.join("");
}
