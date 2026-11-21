import { describe, expect, it } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { defineComponent, h, nextTick } from "vue";
import AppPopover from "../AppPopover.vue";
import { hasOpenOverlay, resetOverlays } from "../overlayRegistry";

const ControlledHost = defineComponent({
  props: { show: { type: Boolean, default: false } },
  components: { AppPopover },
  setup(props) {
    return () =>
      h(
        AppPopover,
        { trigger: "manual", show: props.show, x: 10, y: 10 },
        {
          default: () => h("div", { class: "app-popover-marker" }, "内容"),
        },
      );
  },
});

describe("AppPopover（薄封装 NPopover + 弹层注册表上报）", () => {
  it("受控 show 置开时内容经 teleport 渲染，并上报弹层注册表；置关后撤销", async () => {
    resetOverlays();
    const wrapper = mount(ControlledHost, { props: { show: false } });
    await flushPromises();
    expect(hasOpenOverlay()).toBe(false);

    await wrapper.setProps({ show: true });
    await flushPromises();
    await nextTick();
    expect(document.body.querySelector(".app-popover-marker")).not.toBeNull();
    expect(hasOpenOverlay()).toBe(true);

    await wrapper.setProps({ show: false });
    await flushPromises();
    expect(hasOpenOverlay()).toBe(false);
  });
});
