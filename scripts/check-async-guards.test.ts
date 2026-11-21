import { afterAll, describe, expect, it } from "bun:test";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  EPOCH_SEAM_FILE,
  TOAST_BASELINE,
  TOAST_WINDOW_LINES,
} from "../scripts/check-async-guards.ts";
import { gateScript, runGateScript } from "./run-gate-script.test-helper.ts";

const script = gateScript("check-async-guards.ts");
const run = (args: string[]) => runGateScript(script, args);

const tempDirs: string[] = [];
afterAll(() => {
  for (const dir of tempDirs) rmSync(dir, { recursive: true, force: true });
});

const TOAST_LINE = "message.error(t('app.msg.saveFailed', { message: errorMessage(e) }))";

function toastStub(count: number): string {
  return Array.from({ length: count }, () => TOAST_LINE).join("\n") + (count > 0 ? "\n" : "");
}

function makeFixture(overrides: Record<string, string> = {}, withSeam = true): string[] {
  const root = mkdtempSync(join(tmpdir(), "check-async-guards-"));
  tempDirs.push(root);
  for (const [relPath, count] of Object.entries(TOAST_BASELINE)) {
    const abs = join(root, relPath);
    mkdirSync(join(abs, ".."), { recursive: true });
    writeFileSync(abs, toastStub(count));
  }
  if (withSeam) {
    const seam = join(root, EPOCH_SEAM_FILE);
    mkdirSync(join(seam, ".."), { recursive: true });
    writeFileSync(seam, "export {}\n");
  }
  for (const [relPath, content] of Object.entries(overrides)) {
    const file = join(root, relPath);
    mkdirSync(join(file, ".."), { recursive: true });
    writeFileSync(file, content);
  }
  return [root];
}

describe("check-async-guards（前端异步守门）", () => {
  it("真实仓库默认通过：手搓序号零（接缝外）且 toast 基线全等", () => {
    const r = run([]);
    expect(r.status).toBe(0);
    expect(r.output).toContain("异步守门");
  });

  it("夹具全基线桩（每文件恰好基线计数）时通过", () => {
    const r = run(makeFixture());
    expect(r.status).toBe(0);
    expect(r.output).toContain("异步守门");
  });

  it("空扫描根拒绝假绿（零源文件即红）", () => {
    const dir = mkdtempSync(join(tmpdir(), "check-async-guards-empty-"));
    tempDirs.push(dir);
    const r = run([dir]);
    expect(r.status).toBe(1);
    expect(r.output).toContain("空集");
  });

  it("接缝住址不可达即红（搬迁未同步 EPOCH_SEAM_FILE，拒绝白名单静默失效）", () => {
    const r = run(makeFixture({}, false));
    expect(r.status).toBe(1);
    expect(r.output).toContain("住址不可达");
    expect(r.output).toContain(EPOCH_SEAM_FILE);
  });

  describe("规则 1：手搓竞态纪元（硬零容忍，唯一合法住址 @ledger/latest-wins）", () => {
    it("let fetchSeq = 0 即红，定位文件与行号（ADR-0087 删除即变红锚点：删除规则 1 判定 → 本用例红）", () => {
      const r = run(
        makeFixture({
          "src/views/BadView.ts": `export function reload() {\n  let fetchSeq = 0\n  fetchSeq++\n}`,
        }),
      );
      expect(r.status).toBe(1);
      expect(r.output).toContain("手搓竞态纪元");
      expect(r.output).toContain("src/views/BadView.ts:2");
    });

    it("裸名 let seq = 0（接缝外）同样红（#1678 前的加宽面）", () => {
      const r = run(makeFixture({ "src/composables/useOther.ts": "let seq = 0\nexport {} \n" }));
      expect(r.status).toBe(1);
      expect(r.output).toContain("手搓竞态纪元");
      expect(r.output).toContain("src/composables/useOther.ts:1");
    });

    it("别名面：let epoch = 0 / let generation = 0（接缝外）即红（#1678 按概念执法，新别名不再漏检）", () => {
      const epochHit = run(
        makeFixture({ "src/composables/useOther.ts": "let epoch = 0\nexport {}\n" }),
      );
      expect(epochHit.status).toBe(1);
      expect(epochHit.output).toContain("手搓竞态纪元");
      expect(epochHit.output).toContain("src/composables/useOther.ts:1");

      const generationHit = run(
        makeFixture({ "src/stores/useOtherStore.ts": "let generation = 0\nexport {}\n" }),
      );
      expect(generationHit.status).toBe(1);
      expect(generationHit.output).toContain("手搓竞态纪元");
      expect(generationHit.output).toContain("src/stores/useOtherStore.ts:1");
    });

    it("唯一合法住址：共享 module 本体内 let epoch = 0 绿（#1678 豁免随包）", () => {
      const r = run(
        makeFixture({
          [EPOCH_SEAM_FILE]:
            "export function createLatestWins() {\n  let epoch = 0\n  const bind = () => ({ isStale: () => false });\n  return { begin: bind };\n}\n",
        }),
      );
      expect(r.status).toBe(0);
    });

    it("消费点经共享 module 消费（无手搓计数器）绿", () => {
      const r = run(
        makeFixture({
          "src/views/ConsumeView.ts": [
            'import { createLatestWins } from "@ledger/latest-wins";',
            "const wins = createLatestWins();",
            "export async function load() {",
            "  const token = wins.begin();",
            "  if (token.isStale()) return;",
            "}",
            "",
          ].join("\n"),
        }),
      );
      expect(r.status).toBe(0);
    });

    it("注释行不误报：// let fetchSeq = 0 绿", () => {
      const r = run(
        makeFixture({
          "src/views/CommentView.ts": "// let fetchSeq = 0\nexport {}\n",
        }),
      );
      expect(r.status).toBe(0);
    });
  });

  describe("规则 2：catch 直弹 toast 基线冻结（只减不增，全等校验）", () => {
    it("基线文件回潮（4 → 5）即红，报新增行号", () => {
      const r = run(
        makeFixture({
          "src/views/ItemsView.vue": toastStub(5),
        }),
      );
      expect(r.status).toBe(1);
      expect(r.output).toContain("回潮");
      expect(r.output).toContain("src/views/ItemsView.vue");
      expect(r.output).toContain("5");
    });

    it("基线收缩未同步（4 → 3）即红，提示下调基线并附格式漂移排查提示", () => {
      const r = run(
        makeFixture({
          "src/views/ItemsView.vue": toastStub(3),
        }),
      );
      expect(r.status).toBe(1);
      expect(r.output).toContain("基线待收缩");
      expect(r.output).toContain("src/views/ItemsView.vue");
      expect(r.output).toContain("多行化");
    });

    it("基线条目清零即红，提示删除条目（基线只减不增、不挂陈目）", () => {
      const r = run(
        makeFixture({
          "src/views/PoliciesView.vue": "export {}\n",
        }),
      );
      expect(r.status).toBe(1);
      expect(r.output).toContain("基线待收缩");
      expect(r.output).toContain("src/views/PoliciesView.vue");
    });

    it("基线外文件新增直弹 toast 即红", () => {
      const r = run(
        makeFixture({
          "src/components/NewWidget.vue": `${TOAST_LINE}\n`,
        }),
      );
      expect(r.status).toBe(1);
      expect(r.output).toContain("回潮");
      expect(r.output).toContain("src/components/NewWidget.vue");
    });

    it("多行直弹 toast（t( 与 errorMessage 折行）同样红——#1467 检测面扩宽", () => {
      const r = run(
        makeFixture({
          "src/components/CompactCatch.vue": [
            "} catch (e) {",
            "  message.error(",
            "    t('app.msg.saveFailed', {",
            "      message: errorMessage(e),",
            "    }),",
            "  )",
            "}",
            "",
          ].join("\n"),
        }),
      );
      expect(r.status).toBe(1);
      expect(r.output).toContain("回潮");
      expect(r.output).toContain("src/components/CompactCatch.vue");
      expect(r.output).toContain("行 2");
    });

    it("errorMessage 恰在窗口内最远一行即命中、出窗一格即不识别（钉死最大跨度边界）", () => {
      const make = (fillerCount: number) => {
        const gap = Array.from({ length: fillerCount }, (_, i) => `const filler${i} = ${i}`);
        return (
          ["message.error(", "  t('app.msg.saved'),"].join("\n") +
          "\n" +
          gap.join("\n") +
          "\nconst late = errorMessage(e)\n"
        );
      };
      const inside = run(
        makeFixture({ "src/components/EdgeInside.vue": make(TOAST_WINDOW_LINES - 1) }),
      );
      expect(inside.status).toBe(1);
      expect(inside.output).toContain("回潮");
      expect(inside.output).toContain("src/components/EdgeInside.vue");

      const outside = run(
        makeFixture({ "src/components/FarWidget.vue": make(TOAST_WINDOW_LINES) }),
      );
      expect(outside.status).toBe(0);
    });

    it("注释行不误报：真实 4 处 + 注释 1 行仍与基线全等（绿）", () => {
      const r = run(
        makeFixture({
          "src/views/ItemsView.vue": `${toastStub(4)}// ${TOAST_LINE}\n`,
        }),
      );
      expect(r.status).toBe(0);
    });

    it("基线条目指向的文件缺失即红（清单漂移 fail loud）", () => {
      const dir = mkdtempSync(join(tmpdir(), "check-async-guards-missing-"));
      tempDirs.push(dir);
      for (const [relPath, count] of Object.entries(TOAST_BASELINE)) {
        if (relPath === "src/views/PoliciesView.vue") continue;
        const abs = join(dir, relPath);
        mkdirSync(join(abs, ".."), { recursive: true });
        writeFileSync(abs, toastStub(count));
      }
      const r = run([dir]);
      expect(r.status).toBe(1);
      expect(r.output).toContain("src/views/PoliciesView.vue");
    });
  });

  describe("扫描根自证：磁盘 packages 子包源码目录与 SCAN_ROOTS 一致（#1467）", () => {
    it("未登记的子包 src 目录落盘即红（堵新包静默逃逸扫描）", () => {
      const r = run(
        makeFixture({
          "packages/newpkg/src/index.ts": "export {}\n",
        }),
      );
      expect(r.status).toBe(1);
      expect(r.output).toContain("未登记扫描根");
      expect(r.output).toContain("packages/newpkg/src");
    });
  });
});
