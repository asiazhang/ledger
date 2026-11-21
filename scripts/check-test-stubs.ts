#!/usr/bin/env bun

import { existsSync, readFileSync } from "node:fs";
import { join, relative, resolve, sep } from "node:path";
import { pathToFileURL } from "node:url";
import { lineAt, walkTextFiles } from "./gate-primitives.ts";

const testsDir = resolve(process.argv[2] ?? join("src", "__tests__"));
const seamHomeDir = resolve(process.argv[3] ?? join("packages", "test-support", "src"));
const helperPath = join(seamHomeDir, "reference-stubs.ts");
const packagesDir = resolve(seamHomeDir, "..", "..");

function fail(message: string): never {
  console.error(`✗ 测试桩守门：${message}`);
  process.exit(1);
}

function extractCommands(): string[] {
  if (!existsSync(helperPath)) {
    fail(`参考数据桩助手缺失：${helperPath}（issue #725 治理的桩单一来源）`);
  }
  const helperSource = readFileSync(helperPath, "utf8");
  const registryMatch = helperSource.match(/REFERENCE_DEFAULTS[^=]*=\s*\{([\s\S]*?)\n\}/);
  if (!registryMatch) {
    fail(`助手 ${helperPath} 中找不到 REFERENCE_DEFAULTS 登记处，守门清单无从提取`);
  }
  return [...registryMatch[1].matchAll(/^\s*(list_[a-z_]+):/gm)].map((m) => m[1]);
}

const TS_EXTENSIONS: ReadonlySet<string> = new Set([".ts"]);
function walk(dir: string, keepName: (name: string) => boolean): string[] {
  if (!existsSync(dir)) return [];
  return walkTextFiles(dir, "", { extensions: TS_EXTENSIONS })
    .filter((f) => {
      const name = f.rel.split("/").pop() ?? "";
      return name !== "invoke-seam.test.ts" && keepName(name);
    })
    .map((f) => f.abs);
}

interface ScanTarget {
  abs: string;
  rel: string;
  zone: "app" | "pkg-tests" | "seam-home";
}

function collectScanTargets(): ScanTarget[] {
  const out: ScanTarget[] = [];
  for (const abs of walk(testsDir, () => true)) {
    out.push({ abs, rel: relative(testsDir, abs), zone: "app" });
  }
  if (seamHomeDir !== testsDir) {
    for (const abs of walk(seamHomeDir, () => true)) {
      out.push({ abs, rel: relative(seamHomeDir, abs), zone: "seam-home" });
    }
  }
  for (const abs of walk(packagesDir, (name) => name.endsWith(".test.ts"))) {
    if (resolve(abs).startsWith(seamHomeDir + sep)) continue;
    out.push({ abs, rel: relative(packagesDir, abs), zone: "pkg-tests" });
  }
  return out;
}

function findHandWiredReferenceStubs(rel: string, source: string, commands: string[]): string[] {
  const wirings = commands.map((cmd) => {
    const q = `['"\`]${cmd}['"\`]`;
    return {
      cmd,
      re: new RegExp(
        `if\\s*\\(\\s*cmd\\s*===\\s*${q}\\s*\\)` +
          `|cmd\\s*===\\s*${q}\\s*\\?` +
          `|case\\s+${q}\\s*:`,
      ),
    };
  });
  const hits: string[] = [];
  source.split("\n").forEach((line, i) => {
    for (const { cmd, re } of wirings) {
      if (re.test(line)) {
        hits.push(
          `  ${rel}:${i + 1}  手搓参考数据桩（${cmd}）——改走唯一接缝 wireInvokeSeam（@ledger/test-support/invoke-mock.ts）`,
        );
      }
    }
  });
  return hits;
}

interface CallbackUnit {
  start: number;
  end: number;
  masks: Array<[number, number]>;
}

function extractCallbackUnits(source: string): CallbackUnit[] {
  const units: CallbackUnit[] = [];
  const re = /\.mockImplementation\s*\(/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(source))) {
    const start = m.index + m[0].length;
    let i = start;
    let depth = 1;
    const masks: Array<[number, number]> = [];
    while (i < source.length && depth > 0) {
      const c = source[i];
      if (c === "/" && source[i + 1] === "/") {
        const s = i;
        while (i < source.length && source[i] !== "\n") i++;
        masks.push([s, i]);
      } else if (c === "/" && source[i + 1] === "*") {
        const s = i;
        i += 2;
        while (i < source.length && !(source[i] === "*" && source[i + 1] === "/")) i++;
        i = Math.min(i + 2, source.length);
        masks.push([s, i]);
      } else if (c === '"' || c === "'" || c === "`") {
        const s = i;
        const quote = c;
        i++;
        while (i < source.length && source[i] !== quote) {
          if (source[i] === "\\") i++;
          i++;
        }
        i = Math.min(i + 1, source.length);
        masks.push([s, i]);
      } else {
        if (c === "(") depth++;
        else if (c === ")") depth--;
        i++;
      }
    }
    if (depth !== 0) continue;
    units.push({ start, end: i - 1, masks });
  }
  return units;
}

function findDuplicateWiring(rel: string, source: string, units: CallbackUnit[]): string[] {
  const hits: string[] = [];
  const IF_WIRING = /\bif\s*\(\s*cmd\s*===\s*(['"`])([^'"`]+)\1\s*\)/g;
  const isLive = (at: number, len: number, masks: Array<[number, number]>): boolean =>
    !masks.some(([ms, me]) => ms < at + len && me > at && (ms < at || me > at + len));
  for (const unit of units) {
    const localMasks = unit.masks.map(
      ([s, e]) => [s - unit.start, Math.min(e, unit.end) - unit.start] as [number, number],
    );
    let text = source.slice(unit.start, unit.end);
    for (const nested of units) {
      if (nested === unit) continue;
      if (nested.start >= unit.start && nested.end <= unit.end) {
        const s = nested.start - unit.start;
        const e = nested.end - unit.start;
        text = text.slice(0, s) + text.slice(s, e).replace(/[^\n]/g, " ") + text.slice(e);
      }
    }
    const byCmd = new Map<string, number[]>();
    let m: RegExpExecArray | null;
    IF_WIRING.lastIndex = 0;
    while ((m = IF_WIRING.exec(text))) {
      if (!isLive(m.index, m[0].length, localMasks)) continue;
      if (!byCmd.has(m[2])) byCmd.set(m[2], []);
      byCmd.get(m[2])!.push(lineAt(source, unit.start + m.index));
    }
    for (const [cmd, lines] of byCmd) {
      if (lines.length > 1) {
        hits.push(
          `  ${rel}:${lines[0]}  同回调重复桩（${cmd} ×${lines.length}，行 ${lines.join("、")}）——if 链先命中短路，后一条永不生效`,
        );
      }
    }
  }
  return hits;
}

const DISPATCH_FEATURE = new RegExp(
  [
    "\\bif\\s*\\(\\s*cmd\\s*===",
    "\\bcmd\\s*===\\s*['\"`][^'\"`\\n]*['\"`]\\s*\\?",
    "\\bswitch\\s*\\(\\s*cmd\\s*\\)",
  ].join("|"),
  "g",
);

function findHandWrittenDispatchStub(rel: string, source: string, units: CallbackUnit[]): string[] {
  const hits: string[] = [];
  for (const unit of units) {
    const raw = source.slice(unit.start, unit.end);
    if (!/^\s*\(?\s*cmd\b/.test(raw)) continue;
    let text = raw;
    for (const nested of units) {
      if (nested === unit) continue;
      if (nested.start >= unit.start && nested.end <= unit.end) {
        const st = nested.start - unit.start;
        const en = nested.end - unit.start;
        text = text.slice(0, st) + text.slice(st, en).replace(/[^\n]/g, " ") + text.slice(en);
      }
    }
    const masks = unit.masks.map(
      ([ms, me]) => [ms - unit.start, Math.min(me, unit.end) - unit.start] as [number, number],
    );
    DISPATCH_FEATURE.lastIndex = 0;
    let m: RegExpExecArray | null;
    let dispatched = false;
    while ((m = DISPATCH_FEATURE.exec(text))) {
      const at = m.index;
      const len = m[0].length;
      const dead = masks.some(([ms, me]) => ms <= at && at + len <= me);
      if (!dead) {
        dispatched = true;
        break;
      }
    }
    if (dispatched) {
      hits.push(
        `  ${rel}:${lineAt(source, unit.start)}  手写 invoke 分发桩（mockImplementation 全量替换 + cmd 分发）——改走唯一接缝 wireInvokeSeam 两表布线，一次性覆盖用 mockImplementationOnce 委托`,
      );
    }
  }
  return hits;
}

const WRAPPER_DECL =
  /\b(?:function\s+|(?:const|let|var)\s+)(baseInvoke|stubInvoke|mockBaseCommands|invokeHandler)\b/g;

function findLocalWiringWrapper(rel: string, source: string): string[] {
  const hits: string[] = [];
  source.split("\n").forEach((line, i) => {
    const t = line.trim();
    if (t.startsWith("//") || t.startsWith("*") || t.startsWith("/*")) return;
    WRAPPER_DECL.lastIndex = 0;
    let m: RegExpExecArray | null;
    while ((m = WRAPPER_DECL.exec(line))) {
      hits.push(
        `  ${rel}:${i + 1}  本地布线包装定义（${m[1]}）——布线一律走唯一接缝 wireInvokeSeam`,
      );
    }
  });
  return hits;
}

const FACTORY_NAMES = [
  "makePlan",
  "makeSubscriptionPlan",
  "makeInstallmentPlan",
  "makeTransferPlan",
  "makeOccurrence",
  "makeFakeSink",
  "resetToastSink",
];
const FACTORY_DECL = new RegExp(
  `\\b(?:function\\s+|(?:const|let|var)\\s+)(${FACTORY_NAMES.join("|")})\\b`,
  "g",
);
const FACTORY_WHITELIST = new Set([
  "app:factories.ts",
  "seam-home:plan-factories.ts",
  `app:${join("TransactionsView", "common.ts")}`,
  "seam-home:toast-sink.ts",
]);

function findFactoryDefinition(rel: string, source: string): string[] {
  const hits: string[] = [];
  source.split("\n").forEach((line, i) => {
    const t = line.trim();
    if (t.startsWith("//") || t.startsWith("*") || t.startsWith("/*")) return;
    FACTORY_DECL.lastIndex = 0;
    let m: RegExpExecArray | null;
    while ((m = FACTORY_DECL.exec(line))) {
      hits.push(
        `  ${rel}:${i + 1}  共享测试替身/领域数据工厂本地定义（${m[1]}）——唯一定义点在共享工厂层或共享测试支持包，消费共享出口而非本地定义`,
      );
    }
  });
  return hits;
}

function main(): void {
  const commands = extractCommands();
  if (commands.length === 0) {
    fail(`助手 ${helperPath} 的 REFERENCE_DEFAULTS 登记处提不出任何 list_* 命令（清单漂移？）`);
  }

  let handWired = 0;
  let duplicated = 0;
  let dispatchStubs = 0;
  let wiringWrappers = 0;
  let factoryDefs = 0;
  const violations: string[] = [];
  for (const target of collectScanTargets()) {
    const source = readFileSync(target.abs, "utf8");
    const inSeamHome = target.zone === "seam-home";
    const units = extractCallbackUnits(source);
    const rule1 = inSeamHome ? [] : findHandWiredReferenceStubs(target.rel, source, commands);
    const rule2 = findDuplicateWiring(target.rel, source, units);
    const rule3a = inSeamHome ? [] : findHandWrittenDispatchStub(target.rel, source, units);
    const rule3b = inSeamHome ? [] : findLocalWiringWrapper(target.rel, source);
    const rule4 = FACTORY_WHITELIST.has(`${target.zone}:${target.rel.split(sep).join("/")}`)
      ? []
      : findFactoryDefinition(target.rel, source);
    handWired += rule1.length;
    duplicated += rule2.length;
    dispatchStubs += rule3a.length;
    wiringWrappers += rule3b.length;
    factoryDefs += rule4.length;
    violations.push(...rule1, ...rule2, ...rule3a, ...rule3b, ...rule4);
  }

  if (violations.length > 0) {
    console.error(
      `✗ 测试桩守门：发现 ${handWired} 处手搓参考数据桩、${duplicated} 处同回调重复桩、${dispatchStubs} 处手写 invoke 分发桩、${wiringWrappers} 处本地布线包装、${factoryDefs} 处共享测试替身/领域数据工厂本地定义（登记处命令：${commands.join(" ")}）\n` +
        violations.join("\n") +
        `\ninvoke 布线唯一接缝：wireInvokeSeam（${relative(process.cwd(), join(seamHomeDir, "invoke-mock.ts"))}，issue #746/#750/#1152，ADR-0085）`,
    );
    process.exit(1);
  }

  console.log(
    `✅ 测试桩守门通过（登记处 ${commands.length} 条命令；同回调重复 0、手写分发桩 0、本地布线包装 0、共享测试替身/领域数据工厂本地定义 0；testsDir=${relative(process.cwd(), testsDir)}、seamHome=${relative(process.cwd(), seamHomeDir)}、包内测试纳管）`,
  );
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main();
}
