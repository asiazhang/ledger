#!/usr/bin/env bun
// e2e 步骤库覆盖守门（issue #1510 / 父 spec #1494）：逐 e2e 目标核对「feature
// 步骤行 ↔ 目标注册面」的全等覆盖，并保证每个 feature 至少被一个 e2e 目标绑定。
//
// 为什么是静态守门而不是编译期校验（决策与实测见 #1494 评论）：
// rstest-bdd 0.6.0 的 compile-time-validation 只挂在 `#[scenario]` 属性路径，
// `scenarios!` 生成路径不调用校验；而全量改用 `#[scenario]` 意味着 453 个场景 →
// 453 个手工绑定函数，且新增场景会静默漏跑（代价高于收益）。故保留 `scenarios!`
// 的零维护绑定，把「步骤漏注册 / 模式错配 / 场景无绑定」的兜底前移到本守门。
//
// 口径（逐 e2e 目标）：
//   ① 绑定面：自目标源码派生——cucumber 目标（`filter_run("<dir>")`，目录递归）
//      与 rstest-bdd 目标（`scenarios!("<path>")`，文件或目录）；
//   ② 注册面：目标 `#[path = "…"] mod …;` 纳入的步骤文件（含嵌套 `mod x;` 子模块）
//      里的注册模式，两种形态都认：cucumber `#[given/when/then(expr = "…")]` 与
//      rstest-bdd `#[rstest_bdd_macros::given/when/then("…")]`；
//   ③ 判定：被绑 feature 的每条步骤行在同目标的注册面内**恰好一次**匹配；占位符
//      按语义转匹配式（cucumber `{string}`/`{int}`/`{float}`/`{word}`；rstest-bdd
//      `{<名>:string|整数|浮点}`），`And`/`But` 继承前一关键字（与 Gherkin 同义）；
//   ④ 全覆盖：`tests/e2e/features/**` 每个 feature 至少被一个 e2e 目标绑定——
//      删掉绑定（如 `scenarios!`）即红，补上 #1495 AC4 的「0 场景且退出码 0」缺口。
//
// 与 #1496 的边界：#1496 管「测试目标是否在调度面（不漏跑目标）」；本门管
// 「feature ↔ 目标 ↔ 步骤注册」的对应关系，两者互补、各自独立。
//
// 迁移期语义：同一 feature 可同时被 cucumber 目标与新目标绑定（过渡期双轨），
// 只要**每个绑定它的目标**都覆盖其全部步骤即可；收口删掉 cucumber 目标后同一
// 口径继续成立，无需改脚本。
//
// 已知边界（不静默放过）：`Scenario Outline` 的 `<占位符>` 替换未纳入口径——
// 出现即按问题报出（当前仓零 Outline），需扩展时按报错信息补实现。
//
// 运行时 = Bun（ADR-0083）；Rust 侧注释掩码复用结构守门的 `maskNonCode`
// （注释掩去、字面量保留），不新建第二份词法器。

import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { dirname, join, relative, resolve, sep } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { maskNonCode } from "./check-structure.ts";

/** 步骤关键字（Gherkin 小写形态，用于注册面与判定面比对）。 */
export type StepKeyword = "given" | "when" | "then";

/** 一条注册模式（注册面元素）。 */
interface StepPattern {
  keyword: StepKeyword;
  pattern: string;
  /** 注册所属运行器：同一函数双注册时两种形态各归其运行器，互不算重复。 */
  runner: Runner;
  file: string;
  line: number;
}

/** e2e 运行器：cucumber 属性面与 rstest-bdd 属性面各自独立注册表。 */
export type Runner = "cucumber" | "rstest-bdd";

/** 一个 e2e 测试目标的绑定面与注册面。 */
interface E2eTarget {
  file: string;
  /** 目标声明的绑定形态所对应的运行器（决定注册面取哪一族属性）。 */
  runners: Runner[];
  boundFeatures: string[];
  patterns: StepPattern[];
}

/** 一条 feature 步骤行（判定面元素）。 */
interface FeatureStep {
  keyword: StepKeyword;
  text: string;
  line: number;
}

/** 仓库相对路径（正斜杠，跨平台输出稳定）。 */
const rel = (root: string, path: string): string => relative(root, path).split(sep).join("/");

const read = (path: string): string => readFileSync(path, "utf8");

/** 注释掩码后的 Rust 源（保字面量：注册模式住在字符串字面量里）。 */
const maskedRust = (path: string): string => maskNonCode(read(path), true);

/** Gherkin 注释掩码：整行以 `#` 开头的行掩为空白（行数保持，报错行号不变）。 */
const maskedFeature = (path: string): string =>
  read(path)
    .split("\n")
    .map((line) => (line.trimStart().startsWith("#") ? "" : line))
    .join("\n");

/** 目录递归枚举（返回排序后的绝对路径），按后缀过滤。 */
function walk(dir: string, suffix: string): string[] {
  if (!existsSync(dir)) return [];
  const out: string[] = [];
  for (const entry of readdirSync(dir, { withFileTypes: true }).sort((a, b) =>
    a.name.localeCompare(b.name),
  )) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) out.push(...walk(path, suffix));
    else if (entry.name.endsWith(suffix)) out.push(path);
  }
  return out;
}

/** 依据 `mod x;` 声明定位子模块文件（`dir/x.rs` 或 `dir/x/mod.rs`）。 */
function resolveModuleFile(declaringFile: string, moduleName: string): string | null {
  const stem = declaringFile.slice(0, -".rs".length);
  const base = declaringFile.endsWith(`${sep}mod.rs`) ? dirname(declaringFile) : stem;
  for (const candidate of [join(base, `${moduleName}.rs`), join(base, moduleName, "mod.rs")]) {
    if (existsSync(candidate) && statSync(candidate).isFile()) return candidate;
  }
  return null;
}

/**
 * 目标编译进的步骤文件集合：`#[path = "…"]` 显式模块 + 目标文件与各步骤文件里的
 * 嵌套 `mod x;`（`e2e/scheduled_steps.rs` 形态），递归去重。
 */
function collectStepFiles(targetFile: string, problems: string[]): string[] {
  const collected = new Set<string>();
  const queue: string[] = [];
  const targetDir = dirname(targetFile);
  for (const m of maskedRust(targetFile).matchAll(
    /#\[path\s*=\s*"([^"]+)"\]\s*(?:pub\s+)?mod\s+([a-z_][a-z0-9_]*)\s*;/g,
  )) {
    const path = resolve(targetDir, m[1]);
    if (!existsSync(path)) {
      problems.push(`路径模块缺失：${rel(process.cwd(), targetFile)} 声明 ${m[1]}，文件不存在`);
      continue;
    }
    collected.add(path);
    queue.push(path);
  }
  queue.push(targetFile);
  while (queue.length > 0) {
    const current = queue.shift()!;
    for (const m of maskedRust(current).matchAll(/^(?:pub\s+)?mod\s+([a-z_][a-z0-9_]*)\s*;/gm)) {
      const nested = resolveModuleFile(current, m[1]);
      if (nested === null) continue; // 内联模块（`mod x { … }`）不含步骤注册——本仓零命中
      if (collected.has(nested)) continue;
      collected.add(nested);
      queue.push(nested);
    }
  }
  return [...collected].sort();
}

/** 从目标源码派生绑定面：cucumber `filter_run("<dir>")` 与 rstest `scenarios!("<path>")`。
 *  绑定路径按 **crate 根**（含 `tests/` 的目录）解析——与 cucumber `filter_run` 和
 *  rstest-bdd `scenarios!` 的 manifest-relative 语义一致，不随目标文件位置漂移。 */
function collectBoundFeatures(
  targetFile: string,
  crateRoot: string,
  problems: string[],
): { features: string[]; runners: Runner[] } {
  const masked = maskedRust(targetFile);
  const bound = new Set<string>();
  const runners = new Set<Runner>();
  const expand = (raw: string): void => {
    const path = resolve(crateRoot, raw);
    if (!existsSync(path)) {
      problems.push(`绑定路径缺失：${rel(process.cwd(), targetFile)} 绑定 ${raw}，路径不存在`);
      return;
    }
    if (statSync(path).isDirectory()) {
      for (const file of walk(path, ".feature")) bound.add(file);
      return;
    }
    bound.add(path);
  };
  for (const m of masked.matchAll(/filter_run\(\s*"([^"]+)"/g)) {
    runners.add("cucumber");
    expand(m[1]);
  }
  for (const m of masked.matchAll(/scenarios!\(\s*"([^"]+)"/g)) {
    runners.add("rstest-bdd");
    expand(m[1]);
  }
  return { features: [...bound].sort(), runners: [...runners].sort() };
}

/** 注册面：cucumber `expr = "…"` 与 rstest-bdd 直接字符串两种属性形态。 */
function collectPatterns(stepFiles: string[], problems: string[]): StepPattern[] {
  const patterns: StepPattern[] = [];
  const seen = new Map<string, StepPattern>();
  for (const file of stepFiles) {
    const masked = maskedRust(file);
    const lineOf = (index: number): number => masked.slice(0, index).split("\n").length;
    for (const m of masked.matchAll(
      /#\[(given|when|then)\(\s*(?:expr\s*=\s*)?"((?:[^"\\]|\\.)*)"\s*\)\]/g,
    )) {
      const pattern: StepPattern = {
        keyword: m[1] as StepKeyword,
        pattern: unescapeRustString(m[2]),
        runner: "cucumber",
        file,
        line: lineOf(m.index ?? 0),
      };
      patterns.push(pattern);
    }
    for (const m of masked.matchAll(
      /#\[rstest_bdd_macros::(given|when|then)\(\s*(?:expr\s*=\s*)?"((?:[^"\\]|\\.)*)"\s*\)\]/g,
    )) {
      const pattern: StepPattern = {
        keyword: m[1] as StepKeyword,
        pattern: unescapeRustString(m[2]),
        runner: "rstest-bdd",
        file,
        line: lineOf(m.index ?? 0),
      };
      patterns.push(pattern);
    }
    // `regex = "…"` 形态（本仓零命中）：按原生正则处理，避免误当字面量匹配。
    for (const m of masked.matchAll(
      /#\[(given|when|then)\(\s*regex\s*=\s*"((?:[^"\\]|\\.)*)"\s*\)\]/g,
    )) {
      problems.push(
        `未支持的注册形态：${rel(process.cwd(), file)}:${lineOf(m.index ?? 0)} 使用 regex = "…"` +
          `（守门口径只覆盖 expr / rstest-bdd 直接字符串，需扩展后再生效）`,
      );
    }
    for (const pattern of patterns) {
      if (pattern.file !== file) continue;
      const key = `${pattern.runner}\u0000${pattern.keyword}\u0000${pattern.pattern}`;
      const previous = seen.get(key);
      if (previous === undefined) {
        seen.set(key, pattern);
        continue;
      }
      problems.push(
        `同一注册重复：${pattern.keyword} "${pattern.pattern}" 同时定义于 ` +
          `${rel(process.cwd(), previous.file)}:${previous.line} 与 ` +
          `${rel(process.cwd(), pattern.file)}:${pattern.line}（歧义注册）`,
      );
    }
  }
  return patterns;
}

/** Rust 字符串字面量的最小反转义（本仓注册模式只用到 `\"` 与 `\\`）。 */
function unescapeRustString(raw: string): string {
  return raw.replace(/\\(["\\])/g, "$1");
}

/** 占位符 → 匹配式：cucumber 表达式与 rstest-bdd 类型提示两种形态共用一套语义。 */
function placeholderMatcher(
  token: string,
  typed: string,
  file: string,
  line: number,
  problems: string[],
): string {
  if (typed !== "") return typedMatcher(token, typed, file, line, problems);
  switch (token) {
    case "string":
      return '"[^"]*"';
    case "int":
      return "-?\\d+";
    case "float":
      return "-?\\d+(?:\\.\\d+)?";
    case "word":
      return "\\S+";
    default:
      // 匿名 `{}` 与未知命名占位符：保守按「任意非空」处理并留痕，不静默按字面量匹配。
      problems.push(
        `未知占位符：${rel(process.cwd(), file)}:${line} 的 {${token}} 不在守门口径内` +
          `（已按「任意非空」处理，需确认后补类型）`,
      );
      return ".+";
  }
}

/** rstest-bdd 类型提示 → 匹配式（string / 整数族 / 浮点族）。 */
function typedMatcher(
  token: string,
  typed: string,
  file: string,
  line: number,
  problems: string[],
): string {
  switch (typed) {
    case "string":
      return '"[^"]*"';
    case "f32":
    case "f64":
      return "-?\\d+(?:\\.\\d+)?";
    case "i8":
    case "i16":
    case "i32":
    case "i64":
    case "isize":
    case "u8":
    case "u16":
    case "u32":
    case "u64":
    case "usize":
      return "-?\\d+";
    default:
      problems.push(
        `未知类型提示：${rel(process.cwd(), file)}:${line} 的 {${token}:${typed}} 不在守门口径内`,
      );
      return ".+";
  }
}

/** 注册模式文本 → 锚定正则（模式里的字面量按字面匹配，占位符按语义匹配）。 */
function patternToRegExp(pattern: StepPattern, problems: string[]): RegExp {
  let out = "";
  let index = 0;
  while (index < pattern.pattern.length) {
    const char = pattern.pattern[index];
    if (char === "{") {
      const end = pattern.pattern.indexOf("}", index);
      if (end === -1) {
        out += "\\{";
        index += 1;
        continue;
      }
      const body = pattern.pattern.slice(index + 1, end);
      const [token, typed = ""] = body.split(":");
      out += placeholderMatcher(token, typed, pattern.file, pattern.line, problems);
      index = end + 1;
      continue;
    }
    out += char.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
    index += 1;
  }
  return new RegExp(`^${out}$`);
}

/** 解析 feature 的步骤行（`And`/`But` 继承前一关键字；Outline 占位符按已知边界报出）。 */
function collectFeatureSteps(featureFile: string, problems: string[]): FeatureStep[] {
  const steps: FeatureStep[] = [];
  const masked = maskedFeature(featureFile);
  let inherited: StepKeyword | null = null;
  masked.split("\n").forEach((line, index) => {
    const match = line.match(/^\s*(Given|When|Then|And|But)\s+(.*)$/);
    if (line.match(/^\s*(Feature|Background|Scenario|Scenario Outline|Examples)\s*:/)) {
      inherited = null;
      if (line.match(/^\s*(Scenario Outline|Examples)\s*:/)) {
        problems.push(
          `已知边界：${rel(process.cwd(), featureFile)}:${index + 1} 出现 Scenario Outline / Examples` +
            `（守门口径未覆盖 <占位符> 替换，需扩展守门）`,
        );
      }
      return;
    }
    if (match === null) return;
    const raw = match[1];
    if (raw === "And" || raw === "But") {
      if (inherited === null) {
        problems.push(
          `步骤关键字无法继承：${rel(process.cwd(), featureFile)}:${index + 1} 的 ${raw}` +
            ` 之前没有 Given/When/Then（Gherkin 语义要求先出现主关键字）`,
        );
        return;
      }
      steps.push({ keyword: inherited, text: match[2].trim(), line: index + 1 });
      return;
    }
    inherited = raw.toLowerCase() as StepKeyword;
    steps.push({ keyword: inherited, text: match[2].trim(), line: index + 1 });
  });
  return steps;
}

/** 全仓发现：tests/*.rs 里声明了绑定面的目标即 e2e 目标。 */
function discoverTargets(testsDir: string, crateRoot: string, problems: string[]): E2eTarget[] {
  const targets: E2eTarget[] = [];
  for (const entry of readdirSync(testsDir, { withFileTypes: true }).sort((a, b) =>
    a.name.localeCompare(b.name),
  )) {
    if (!entry.isFile() || !entry.name.endsWith(".rs")) continue;
    const file = join(testsDir, entry.name);
    const binding = collectBoundFeatures(file, crateRoot, problems);
    if (binding.features.length === 0) continue;
    const patterns = collectPatterns(collectStepFiles(file, problems), problems);
    targets.push({ file, runners: binding.runners, boundFeatures: binding.features, patterns });
  }
  return targets;
}

/** 主判定：未覆盖 / 歧义 / 无目标绑定三类问题。 */
export function checkCoverage(srcTauriDir: string): {
  findings: string[];
  problems: string[];
  targets: { file: string; features: number; patterns: number; steps: number }[];
  stats: { features: number; steps: number; patterns: number };
} {
  const problems: string[] = [];
  const findings: string[] = [];
  const testsDir = join(srcTauriDir, "tests");
  const featuresDir = join(testsDir, "e2e", "features");
  if (!existsSync(testsDir)) {
    return {
      findings,
      problems: [`tests 目录不存在：${testsDir}`],
      targets: [],
      stats: { features: 0, steps: 0, patterns: 0 },
    };
  }

  const targets = discoverTargets(testsDir, srcTauriDir, problems);
  const boundAnywhere = new Set<string>();
  const perTarget: { file: string; features: number; patterns: number; steps: number }[] = [];
  let stepLines = 0;
  let patternCount = 0;

  for (const target of targets) {
    // 注册面按目标的运行器取族：双注册文件里两族属性共存，各目标只消费自己那族
    //（cucumber 目标读 `#[given(expr = …)]`，rstest-bdd 目标读 `#[rstest_bdd_macros::…]`）。
    const runnerPatterns = target.patterns.filter((pattern) =>
      target.runners.includes(pattern.runner),
    );
    const matchers = runnerPatterns.map((pattern) => ({
      pattern,
      regex: patternToRegExp(pattern, problems),
    }));
    patternCount += runnerPatterns.length;
    let targetSteps = 0;
    for (const feature of target.boundFeatures) {
      boundAnywhere.add(feature);
      for (const step of collectFeatureSteps(feature, problems)) {
        targetSteps += 1;
        const hits = matchers.filter(
          (candidate) =>
            candidate.pattern.keyword === step.keyword && candidate.regex.test(step.text),
        );
        const where = `${rel(srcTauriDir, feature)}:${step.line}`;
        const label = `${step.keyword[0].toUpperCase()}${step.keyword.slice(1)} ${step.text}`;
        if (hits.length === 0) {
          findings.push(
            `未覆盖 [${rel(srcTauriDir, target.file)}] ${where} ${label}（该目标注册面无匹配模式）`,
          );
        } else if (hits.length > 1) {
          findings.push(
            `歧义 [${rel(srcTauriDir, target.file)}] ${where} ${label}（命中 ${hits.length} 条：` +
              hits
                .map((hit) => `${rel(srcTauriDir, hit.pattern.file)}:${hit.pattern.line}`)
                .join(" / ") +
              "）",
          );
        }
      }
    }
    stepLines += targetSteps;
    perTarget.push({
      file: target.file,
      features: target.boundFeatures.length,
      patterns: runnerPatterns.length,
      steps: targetSteps,
    });
  }

  for (const feature of walk(featuresDir, ".feature")) {
    if (!boundAnywhere.has(feature)) {
      findings.push(`无目标绑定：${rel(srcTauriDir, feature)}（没有任何 e2e 目标绑定该 feature）`);
    }
  }

  return {
    findings,
    problems,
    targets: perTarget,
    stats: {
      features: new Set(targets.flatMap((target) => target.boundFeatures)).size,
      steps: stepLines,
      patterns: patternCount,
    },
  };
}

function main(): void {
  // 默认住址在 main 内求值：vitest 转换下 import.meta.url 非 file: scheme
  //（run-gate-script.test-helper.ts 头注同款），夹具经位置参数传 src-tauri 目录。
  const srcTauriDir = process.argv[2]
    ? resolve(process.argv[2])
    : join(fileURLToPath(import.meta.url), "..", "..", "src-tauri");
  const { findings, problems, targets, stats } = checkCoverage(srcTauriDir);

  if (problems.length > 0 || findings.length > 0) {
    const lines = [...problems, ...findings];
    console.error(
      `✗ e2e 步骤库覆盖守门：发现 ${lines.length} 处问题` +
        `（口径：feature 步骤行 ↔ 目标注册面全等；issue #1510 / spec #1494）\n` +
        lines.map((line) => `  · ${line}`).join("\n"),
    );
    process.exit(1);
  }

  const detail = targets
    .map(
      (target) =>
        `    · ${rel(srcTauriDir, target.file)}：绑定 feature ${target.features} ·` +
        ` 注册 ${target.patterns} · 步骤 ${target.steps}`,
    )
    .join("\n");
  console.log(
    `✓ e2e 步骤库覆盖守门：目标 ${targets.length} 个 · 绑定 feature ${stats.features} 个 ·` +
      ` 步骤行 ${stats.steps} 条 · 注册 ${stats.patterns} 条 · 未覆盖 0 · 歧义 0 · 无绑定 0\n${detail}`,
  );
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main();
}
