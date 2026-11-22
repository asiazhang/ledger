import { beforeAll, describe, expect, it } from "vitest";
import { setAdapter, removeAdapter } from "@vanilla-extract/css/adapter";
import { setFileScope, endFileScope } from "@vanilla-extract/css/fileScope";

let cssModule: typeof import("../app-modal.css.ts") | null = null;
type CapturedBlock = { type: string; selector?: string; rule?: Record<string, unknown> };
let capturedBlocks: CapturedBlock[] = [];

beforeAll(async () => {
  const captured: CapturedBlock[] = [];
  const adapter: Parameters<typeof setAdapter>[0] = {
    appendCss: (block) => captured.push(block as CapturedBlock),
    registerClassName: () => {},
    registerComposition: () => {},
    markCompositionUsed: () => {},
    onEndFileScope: () => {},
    getIdentOption: () => "short",
  };
  setAdapter(adapter);
  setFileScope("packages/ui-kit/src/app-modal.css.ts");
  try {
    cssModule = await import("../app-modal.css.ts");
  } finally {
    endFileScope();
    removeAdapter();
  }
  capturedBlocks = captured;
});

function blockOf(fragment: string): CapturedBlock {
  const block = capturedBlocks.find((b) => b.selector?.includes(fragment));
  expect(block, `应存在选择器含 ${fragment} 的 CSS 块`).toBeDefined();
  return block as CapturedBlock;
}

describe("弹窗移动档 CSS 产出物（issue #844，收口 app-modal.css.ts）", () => {
  it("全部规则收口在移动钩子类作用域下（桌面档零渗透）", () => {
    expect(cssModule).not.toBeNull();
    expect(cssModule!.MOBILE_CARD_CLASS).toBe("app-modal-mobile-card");
    expect(capturedBlocks.length).toBeGreaterThan(0);
    for (const block of capturedBlocks) {
      expect(
        block.selector?.startsWith(`.${cssModule!.MOBILE_CARD_CLASS}`) ?? false,
        `选择器应以其移动钩子类打头：${block.selector}`,
      ).toBe(true);
    }
  });

  it("卡片内容滚动：内容区纵向滚动且子项不压缩（lg 表格详情类可读性方案）", () => {
    const content = blockOf(".n-card-content");
    expect(content.rule).toMatchObject({ overflowY: "auto", display: "flex", minHeight: 0 });
    const shrink = blockOf(".n-card-content > *");
    expect(shrink.rule).toMatchObject({ flexShrink: 0 });
  });

  it("标签上置：左置标签表单翻为上下堆叠且标签居左（窄屏不再挤压输入宽度）", () => {
    const flip = blockOf(".n-form-item.n-form-item--left-labelled");
    expect(flip.rule).toMatchObject({
      gridTemplateAreas: '"label" "blank" "feedback"',
      gridTemplateColumns: "minmax(0, 100%)",
      alignItems: "stretch",
    });
    const align = blockOf(".n-form-item--left-labelled .n-form-item-label");
    expect(align.rule).toMatchObject({ textAlign: "left" });
  });

  it("左置标签详情表：标签列不逐字折行（窄屏内容列挤压的收口）", () => {
    const nowrap = blockOf(".n-descriptions--left-label-placement .n-descriptions-table-header");
    expect(nowrap.rule).toMatchObject({ whiteSpace: "nowrap" });
  });

  it("按钮行底部固定：表单与其节奏容器撑满、末块（按钮行）推至底部", () => {
    expect(blockOf("form.n-form:not(.n-form--inline)").rule).toMatchObject({
      display: "flex",
      flexGrow: 1,
    });
    expect(blockOf("form.n-form:not(.n-form--inline) > .n-space").rule).toMatchObject({
      flexGrow: 1,
    });
    expect(blockOf("form.n-form:not(.n-form--inline) > .n-space > :last-child").rule).toMatchObject(
      { marginTop: "auto" },
    );
    expect(blockOf(".n-card-content > .n-space > :last-child").rule).toMatchObject({
      marginTop: "auto",
    });
  });
});
