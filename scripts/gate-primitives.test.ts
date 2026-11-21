import { afterAll, describe, expect, it } from "bun:test";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  hasExtension,
  lineAt,
  maskComments,
  maskNonCode,
  readDirEntries,
  RUST_EXTENSIONS,
  walkTextFiles,
} from "./gate-primitives.ts";

const read = (rel: string): string => readFileSync(join(process.cwd(), ...rel.split("/")), "utf8");
const tempDirs: string[] = [];
afterAll(() => {
  for (const dir of tempDirs) rmSync(dir, { recursive: true, force: true });
});

describe("maskNonCode 双源防漂移语料（issue #1433）", () => {
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
    expect(files[2].abs).toBe(join(dir, "src", "nested", "a.rs"));
    expect(walkTextFiles(dir, "", options).map((f) => f.rel)).toEqual(files.map((f) => f.rel));
  });

  it("walkTextFiles：relBase 前缀归一 rel 路径（扫描根 ≠ 仓库根的守门形态）", () => {
    const dir = mkdtempSync(join(tmpdir(), "walk-text-files-relbase-"));
    tempDirs.push(dir);
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
    expect([...out].map((c) => c === "\n")).toEqual([...src].map((c) => c === "\n"));
    const [line0, line1] = out.split("\n");
    expect(line0?.trimEnd()).toBe("a()");
    expect(line1?.startsWith("x ")).toBe(true);
    expect(line1?.endsWith(" y")).toBe(true);
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

  it("除法歧义两侧：标识符后除法链不当正则，赋值号后才按正则识别", () => {
    const src = "const q = total / count / 2\nconst r = s.match(/x/g)\n";
    expect(maskComments(src)).toBe(src);
  });

  it("除法后接行注释：除法斜杠不吞注释起点，行注释照常掩码", () => {
    const src = "const q = total / // note\nconst after = 1\n";
    expect(maskComments(src)).toBe(`const q = total / ${" ".repeat(7)}\nconst after = 1\n`);
  });

  it("字符类内斜杠不闭合正则：[/] 形态整体识别，其后代码保留", () => {
    const src = "const seg = input.split(/[\\/]/)\nconst after = 1\n";
    expect(maskComments(src)).toBe(src);
  });

  it("正则后跟行注释：正则体跳过后注释起点仍被掩码计数", () => {
    const src = "const ok = /^\\d+$/.test(v) // trailing note\nconst after = 1\n";
    expect(maskComments(src)).toBe(
      `const ok = /^\\d+$/.test(v) ${" ".repeat(16)}\nconst after = 1\n`,
    );
  });

  it("未闭合正则候选不逃逸：换行前无闭合斜杠按普通字符处理", () => {
    const src = "const x = /abc\nconst after = 1\n";
    expect(maskComments(src)).toBe(src);
  });
});

describe("maskComments 正则字面量词法（#2211 毒化缺陷修复，ADR-0145 修订注记）", () => {
  it("test-exec.ts:135 形态：引号入正则体不启伪字符串，误计 854 行毒化消除", () => {
    const src =
      [
        "const quoted = text.match(/^(\"'])(.*)\\1$/);",
        "if (quoted !== null) return quoted[2];",
        "// real comment",
        "const after = 1;",
      ].join("\n") + "\n";
    const masked = maskComments(src);
    const lines = masked.split("\n");
    expect(lines[0]).toBe("const quoted = text.match(/^(\"'])(.*)\\1$/);");
    expect(lines[1]).toBe("if (quoted !== null) return quoted[2];");
    expect(lines[2]).toBe(" ".repeat(15));
    expect(lines[3]).toBe("const after = 1;");
  });

  it("棘轮失明洞闭合：正则体后的真注释行重新被计出（no-native-confirm 形态）", () => {
    const src =
      [
        "const staticRe = /import\\s*\\{[^}]*\\bconfirm\\b[^}]*\\}\\s*from\\s*[\"']([\\w@/-]+)[\"']/g;",
        "// first real comment",
        "// second real comment",
        "const done = 1;",
      ].join("\n") + "\n";
    const masked = maskComments(src);
    const lines = masked.split("\n");
    expect(lines[0]).toBe(src.split("\n")[0]);
    expect(lines[1]?.trimEnd()).toBe("");
    expect(lines[2]?.trimEnd()).toBe("");
    expect(lines[3]).toBe("const done = 1;");
  });

  it("行首正则与跨行前导：赋值换行与多行实参中的行首正则识别，其后真注释保留", () => {
    const src =
      [
        "const TEST_RESULT_RE =",
        "  /^test result: (?:ok|FAILED)\\. (\\d+) passed;$/gm;",
        "expect(",
        "  /no-such-book/,",
        ");",
        "// counted note",
      ].join("\n") + "\n";
    const masked = maskComments(src);
    const lines = masked.split("\n");
    expect(lines[0]).toBe("const TEST_RESULT_RE =");
    expect(lines[1]).toBe("  /^test result: (?:ok|FAILED)\\. (\\d+) passed;$/gm;");
    expect(lines[3]).toBe("  /no-such-book/,");
    expect(lines[5]?.trimEnd()).toBe("");
  });

  it("=> 与 return 前导的正则识别（仓内实证形态）", () => {
    const src =
      [
        "const axumLine = lines.find((l) => /^\\s*axum\\s*=/.test(l));",
        "return /mac/i.test(platform);",
        "const after = 1;",
      ].join("\n") + "\n";
    expect(maskComments(src)).toBe(src);
  });
});

describe("单源收敛：maskComments 只有一份实现（issue #1481，#1680 并入 gate-primitives）", () => {
  const consumers = ["check-frontend-structure.ts", "check-commands.ts"] as const;

  for (const file of consumers) {
    it(`${file} 从共享库消费 maskComments 且不本地重定义`, () => {
      const src = read(`scripts/${file}`);
      expect(src).toMatch(
        /import\s*\{[^}]*\bmaskComments\b[^}]*\}\s*from\s*['"]\.\/gate-primitives\.ts['"]/,
      );
      expect(src).not.toMatch(/(?:function|const|let|var)\s+maskComments\b/);
    });
  }

  it("共享库导出 maskComments（两消费者导入的同一出口）", () => {
    expect(read("scripts/gate-primitives.ts")).toMatch(/export\s+function\s+maskComments\b/);
  });
});

describe("一层目录列举 readDirEntries + 点扩展名匹配 hasExtension（issue #1742）", () => {
  it("readDirEntries：一层列举不递归，dirent 类型投影（文件/目录）", () => {
    const dir = mkdtempSync(join(tmpdir(), "read-dir-entries-"));
    tempDirs.push(dir);
    mkdirSync(join(dir, "subdir", "nested"), { recursive: true });
    writeFileSync(join(dir, "a.rs"), "pub fn a() {}\n");
    writeFileSync(join(dir, "subdir", "inner.rs"), "pub fn inner() {}\n");
    writeFileSync(join(dir, "subdir", "nested", "deep.rs"), "pub fn deep() {}\n");

    const entries = readDirEntries(dir);
    expect(entries.map((e) => e.name)).toEqual(["a.rs", "subdir"]);
    expect(entries[0]).toEqual({ name: "a.rs", isDirectory: false, isFile: true });
    expect(entries[1]).toEqual({ name: "subdir", isDirectory: true, isFile: false });
  });

  it("readDirEntries：localeCompare 排序（家族统一行为，与 walkTextFiles 同款）", () => {
    const dir = mkdtempSync(join(tmpdir(), "read-dir-entries-sort-"));
    tempDirs.push(dir);
    for (const name of ["B", "a", "c"]) writeFileSync(join(dir, name), "x");
    expect(readDirEntries(dir).map((e) => e.name)).toEqual(["a", "B", "c"]);
  });

  it("readDirEntries：目录符号链接不跟随（isFile/isDirectory 均 false，与 walkTextFiles 同语义）", () => {
    const dir = mkdtempSync(join(tmpdir(), "read-dir-entries-symlink-"));
    tempDirs.push(dir);
    mkdirSync(join(dir, "real"), { recursive: true });
    symlinkSync(join(dir, "real"), join(dir, "link"));

    const link = readDirEntries(dir).find((e) => e.name === "link");
    expect(link).toEqual({ name: "link", isDirectory: false, isFile: false });
  });

  it("readDirEntries：目录不存在按 readdirSync 原样抛错（不静默空集，调用侧 existsSync 守卫保留）", () => {
    expect(() => readDirEntries(join(tmpdir(), "read-dir-entries-missing-nope"))).toThrow();
  });

  it("hasExtension：点扩展名闭集匹配与 endsWith 全等（残余内联 endsWith 收口的等价性判据）", () => {
    const names = [".rs", "a.rs", "lib.rs", "rs", "a.b.rs", "a.RS", "a.rs.txt", "", ".r", "a."];
    for (const name of names) {
      expect(hasExtension(name, RUST_EXTENSIONS)).toBe(name.endsWith(".rs"));
    }
    expect(hasExtension("lib.rs", RUST_EXTENSIONS)).toBe(true);
    expect(hasExtension("main", RUST_EXTENSIONS)).toBe(false);
  });

  it("hasExtension：多扩展名集合成员判定", () => {
    const exts = new Set([".json", ".ts"]);
    expect(hasExtension("errors.json", exts)).toBe(true);
    expect(hasExtension("app.ts", exts)).toBe(true);
    expect(hasExtension("a.json5", exts)).toBe(false);
    expect(hasExtension("json", exts)).toBe(false);
  });
});

describe("单源收敛：Rust 词法掩码只有一份实现（#1680 / #1741）", () => {
  const consumers = [
    "check-structure.ts",
    "check-infra-dml.ts",
    "check-background-services.ts",
    "test-exec.ts",
    "check-test-support.ts",
    "check-commands.ts",
    "check-i18n-keys.ts",
  ] as const;

  for (const file of consumers) {
    it(`${file} 从共享库消费 maskNonCode 且不本地重定义`, () => {
      const src = read(`scripts/${file}`);
      expect(src).toMatch(
        /import\s*\{[^}]*\bmaskNonCode\b[^}]*\}\s*from\s*['"]\.\/gate-primitives\.ts['"]/,
      );
      expect(src).not.toMatch(/(?:function|const|let|var)\s+maskNonCode\b/);
    });
  }
});
