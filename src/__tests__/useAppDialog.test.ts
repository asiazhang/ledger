import { describe, it, expect } from "vitest";
import { mount } from "@vue/test-utils";
import { NDialogProvider } from "naive-ui";
import { defineComponent, h } from "vue";
import { closeTopOverlay, hasOpenOverlay } from "@ledger/ui-kit/overlayRegistry";
import { useAppDialog } from "@/composables/useAppDialog";

describe("封装关闭通道（closeTopOverlay 消费面，issue #845）", () => {
  it("useAppDialog：closeTopOverlay 走 destroy()（删除确认等命令式对话框）", async () => {
    const Host = defineComponent({
      setup() {
        const dialog = useAppDialog();
        return () =>
          h("button", { onClick: () => dialog.warning({ title: "t", content: "c" }) }, "del");
      },
    });
    const wrapper = mount(NDialogProvider, { slots: { default: () => h(Host) } });
    await wrapper.find("button").trigger("click");
    expect(hasOpenOverlay()).toBe(true);

    expect(closeTopOverlay()).toBe(true);
    expect(hasOpenOverlay()).toBe(false);
  });
});
