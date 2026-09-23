import { afterAll, describe, expect, it } from "vitest";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { lineAt, maskComments, maskNonCode, walkTextFiles } from "./gate-primitives.ts";

// 被测对象是守门家族共享原语库 scripts/gate-primitives.ts（issue #1680 库归库、门归门）：
// 原语级测试面归库自身，掩码与遍历的正确性不再借结构守门的通过来间接证明。
// - Rust 词法掩码双源防漂移语料与 lineAt / walkTextFiles 用例自 check-structure.test.ts
//   迁入（#1433 / #1625「直测导出共享面」先例随库迁移，删除即变红判据不变：
//   从库删掉一个导出 → 消费它的门 spawnSync 启动即崩 → 包装测试红 + 类型检查红）；
// - TS/Vue 注释掩码用例自 ts-comment-mask.test.ts 迁入（#1481，模块已并入本库）。
// （仓库根 = vitest 进程 cwd；原 repoRoot 助手 has-command-line.test-helper 已随
//  守门挂载登记收编、消费清零而退役，issue #1682）

const read = (rel: string): string => readFileSync(join(process.cwd(), ...rel.split("/")), "utf8");
const tempDirs: string[] = [];
afterAll(() => {
  for (const dir of tempDirs) rmSync(dir, { recursive: true, force: true });
});

describe("maskNonCode 双源防漂移语料（issue #1433）", () => {
  // 与 Rust 侧唯一实现（src-tauri/src/test_support/scan.rs 的 mask_non_code）
  // 消费同一夹具：语料输入 + 期望输出双文件共享，任一侧单独改词法规则即
  // 本测或 Rust 语料测试红。（vitest 下取进程 cwd = 仓库根定位夹具，同上款先例）
  const corpusPath = join(process.cwd(), "scripts", "fixtures", "rust-mask-corpus.rs");
  const expectedPath = join(process.cwd(), "scripts", "fixtures", "rust-mask-corpus.expected.txt");

  it("掩码输出与共享语料期望全等（与 Rust 侧 mask_non_code 同规）", () => {
    const corpus = readFileSync(corpusPath, "utf8");
    const expected = readFileSync(expectedPath, "utf8");
    expect(maskNonCode(corpus)).toBe(expected);
  });

  it("keepLiterals=true 与共享语料期望全等（TS 侧单侧锁定：Rust 侧无 keepLiterals 是 scan.rs 头注登记在案的不对称）", () => {
    const corpus = readFileSync(corpusPath, "utf8");
    const keepExpected = readFileSync(
      join(process.cwd(), "scripts", "fixtures", "rust-mask-corpus.keepLiterals.expected.txt"),
      "utf8",
    );
    expect(maskNonCode(corpus, true)).toBe(keepExpected);
  });

  it("keepLiterals=true 只掩注释、保留字符串字面量（TS 侧扩展形态，语料外的直接断言）", () => {
    const src = '// comment\nlet s = "keep";\n';
    expect(maskNonCode(src, true)).toBe('          \nlet s = "keep";\n');
  });
});

describe("守门家族共享扫描单点：命中行定位 lineAt + 文本面遍历 walkTextFiles（issue #1625）", () => {
  // 守门家族共享面（供各守门直接消费）：行号定位与目录遍历收口单点，
  // 扩展名闭集与豁免面属各守门政策经参数注入。

  it("lineAt：按命中点之前的换行数计行（1 起算），index undefined 按文首计", () => {
    const source = ["const a = 1;", "", "const url = 'https://x.example';"].join("\n");
    const m = /x\.example/.exec(source);
    expect(m).not.toBeNull();
    expect(lineAt(source, m?.index)).toBe(3);
    expect(lineAt("abc def", undefined)).toBe(1);
    expect(lineAt("abc def", 0)).toBe(1);
  });

  it("walkTextFiles：扩展名闭集过滤 + 目录名剪枝 + 相对路径剪枝，排序确定", () => {
    const dir = mkdtempSync(join(tmpdir(), "walk-text-files-"));
    tempDirs.push(dir);
    mkdirSync(join(dir, "src", "nested"), { recursive: true });
    mkdirSync(join(dir, "node_modules"), { recursive: true });
    writeFileSync(join(dir, "CHANGELOG.md"), "# history\n");
    writeFileSync(join(dir, "README"), "无扩展名文件不参与文本面");
    writeFileSync(join(dir, "src", "b.ts"), "export const b = 1;\n");
    writeFileSync(join(dir, "src", "skip-me.ts"), "export const s = 1;\n");
    writeFileSync(join(dir, "src", "nested", "a.rs"), "pub fn a() {}\n");
    writeFileSync(join(dir, "src", "nested", "a.rs.bak"), "junk");
    writeFileSync(join(dir, "node_modules", "dep.ts"), "export const dep = 1;\n");

    const options = {
      extensions: new Set([".ts", ".rs", ".md"]),
      skipDirs: new Set(["node_modules"]),
      skipFiles: new Set(["src/skip-me.ts"]),
    };
    const files = walkTextFiles(dir, "", options);
    expect(files.map((f) => f.rel)).toEqual(["CHANGELOG.md", "src/b.ts", "src/nested/a.rs"]);
    // abs 与 rel 指向同一文件（读文件用 abs、报文定位用 rel）
    expect(files[2].abs).toBe(join(dir, "src", "nested", "a.rs"));
    // 排序确定性：同一目录重复调用输出全等
    expect(walkTextFiles(dir, "", options).map((f) => f.rel)).toEqual(files.map((f) => f.rel));
  });

  it("walkTextFiles：relBase 前缀归一 rel 路径（扫描根 ≠ 仓库根的守门形态）", () => {
    const dir = mkdtempSync(join(tmpdir(), "walk-text-files-relbase-"));
    tempDirs.push(dir);
    // check-infra-dml 形态：扫描根 = join(srcTauri, INFRA_SRC_REL)，relBase 同值——
    // rel 不随夹具所在绝对路径漂移，始终以声明前缀归一。
    const scanRoot = join(dir, "crates", "infra", "src");
    mkdirSync(join(scanRoot, "db"), { recursive: true });
    writeFileSync(join(scanRoot, "db", "migrate.rs"), "pub fn migrate() {}\n");

    const files = walkTextFiles(scanRoot, "crates/infra/src", {
      extensions: new Set([".rs"]),
    });
    expect(files.map((f) => f.rel)).toEqual(["crates/infra/src/db/migrate.rs"]);
  });
});

describe("maskComments（TS/Vue 注释掩码库语义）", () => {
  it("行注释与块注释掩为等长空白：保留换行与列位（行号稳定）", () => {
    const src = "a() // note\nx /* blk */ y\n";
    const out = maskComments(src);
    expect(out).toHaveLength(src.length);
    // 换行位置逐列不变
    expect([...out].map((c) => c === "\n")).toEqual([...src].map((c) => c === "\n"));
    const [line0, line1] = out.split("\n");
    expect(line0?.trimEnd()).toBe("a()"); // 行注释内容不再出现
    expect(line1?.startsWith("x ")).toBe(true); // 块注释前代码保留
    expect(line1?.endsWith(" y")).toBe(true); // 块注释后代码保留
    expect(line1).not.toContain("blk");
  });

  it("字符串与模板字面量内的 // /* 不误当注释（否则吞掉同行真实代码）", () => {
    const src = 'const s = "http://x"\nconst t = `a /* b */ c`\n';
    expect(maskComments(src)).toBe(src);
  });

  it("正则体内转义的斜杠序列不触发注释掩码，其后的真实代码保留", () => {
    const src = "const r = /a\\/\\/b/\nconst after = 1\n";
    expect(maskComments(src)).toBe(src);
  });
});

// 「删除即变红」负向判据（ADR-0087 断言强度 / 接线型守门）：本票的核心价值是
// maskComments 只有一份实现、两个门禁脚本消费同一出口。任一消费者删掉共享模块
// import 并回退为本地副本（或共享模块定义被移除），下面的断言至少一条变红；
// 只在真实仓行为层（exit code / output）保持绿无法单独锚定「同一出口」这条接线。
describe("单源收敛：maskComments 只有一份实现（issue #1481，#1680 并入 gate-primitives）", () => {
  const consumers = ["check-frontend-structure.ts", "check-commands.ts"] as const;

  for (const file of consumers) {
    it(`${file} 从共享库消费 maskComments 且不本地重定义`, () => {
      const src = read(`scripts/${file}`);
      expect(src).toMatch(
        /import\s*\{[^}]*\bmaskComments\b[^}]*\}\s*from\s*['"]\.\/gate-primitives\.ts['"]/,
      );
      // 本地重定义（含函数表达式 / 箭头函数副本）同样即红
      expect(src).not.toMatch(/(?:function|const|let|var)\s+maskComments\b/);
    });
  }

  it("共享库导出 maskComments（两消费者导入的同一出口）", () => {
    expect(read("scripts/gate-primitives.ts")).toMatch(/export\s+function\s+maskComments\b/);
  });
});

// Rust 词法掩码单源收敛（#1680）：maskNonCode 的 TS 消费名单锁定——任一消费方摘掉
// 库 import、回退本地副本（或新脚本绕开库自建第四份实现），下面的断言即红；
// 语料期望只锁规则本身，名单把「谁在消费同一实现」也钉住，防分歧潜伏。
describe("单源收敛：Rust 词法掩码只有一份实现（#1680）", () => {
  const consumers = [
    "check-structure.ts",
    "check-infra-dml.ts",
    "check-background-services.ts",
    "test-exec.ts",
    "check-test-support.ts",
  ] as const;

  for (const file of consumers) {
    it(`${file} 从共享库消费 maskNonCode 且不本地重定义`, () => {
      const src = read(`scripts/${file}`);
      expect(src).toMatch(
        /import\s*\{[^}]*\bmaskNonCode\b[^}]*\}\s*from\s*['"]\.\/gate-primitives\.ts['"]/,
      );
      // 本地重定义（含函数表达式 / 箭头函数副本）同样即红
      expect(src).not.toMatch(/(?:function|const|let|var)\s+maskNonCode\b/);
    });
  }
});
