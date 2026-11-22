import { beforeAll, describe, expect, it } from "vitest";
import { setAdapter, removeAdapter } from "@vanilla-extract/css/adapter";
import { setFileScope, endFileScope } from "@vanilla-extract/css/fileScope";

let cssModule: typeof import("@/settings/settings-layout.css.ts") | null = null;
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
  setFileScope("src/settings/settings-layout.css.ts");
  try {
    cssModule = await import("@/settings/settings-layout.css.ts");
  } finally {
    endFileScope();
    removeAdapter();
  }
  capturedBlocks = captured;
});

function blockExact(selector: string): CapturedBlock {
  const block = capturedBlocks.find((b) => b.selector === selector);
  expect(block, `应存在选择器为 ${selector} 的 CSS 块`).toBeDefined();
  return block as CapturedBlock;
}

describe("设置页动态卡片布局 CSS（issue #651 修订，收口 settings-layout.css.ts）", () => {
  it("设置列吃满内容区可用宽度：不再有 720px / 1280px 上限", () => {
    expect(cssModule).not.toBeNull();
    expect(cssModule!.SETTINGS_COLUMN_CLASS).toBe("settings-column");
    expect(cssModule!.SETTINGS_CARD_STACK_CLASS).toBe("settings-card-stack");
    const column = blockExact(".settings-column").rule;
    expect(column).toMatchObject({ width: "100%" });
    expect(column).not.toHaveProperty("maxWidth");
  });

  it("卡片单列堆叠且随窗口铺满：不做分栏，行距保持 16px", () => {
    expect(blockExact(".settings-card-stack").rule).toMatchObject({
      display: "grid",
      gridTemplateColumns: "minmax(0, 1fr)",
      gap: "16px",
      width: "100%",
    });
  });
});
