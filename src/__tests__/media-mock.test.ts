import { describe, expect, it } from "vitest";
import { defineComponent, h, nextTick } from "vue";
import { DEFAULT_MEDIA_STATE, resetFakeMedia, setFakeMedia } from "@ledger/test-support/media-mock";
import { useInputMode } from "@/composables/useInputMode";
import { useWindowTier } from "@ledger/window-tier";
import { mountFlushed } from "@ledger/test-support/mount";

function matches(query: string): boolean {
  return window.matchMedia(query).matches;
}

describe("媒体查询测试接缝：默认状态（全局壳层每测复位）", () => {
  it("默认桌面指针环境：宽视口命中 min-width、可悬停、精细主指针", () => {
    expect(DEFAULT_MEDIA_STATE).toEqual({ width: 1280, hover: "hover", pointer: "fine" });
    expect(matches("(min-width: 100px)")).toBe(true);
    expect(matches("(hover: hover)")).toBe(true);
    expect(matches("(pointer: fine)")).toBe(true);
  });

  it("媒体字段与默认态相反的查询一律不应答命中", () => {
    expect(matches("(hover: none)")).toBe(false);
    expect(matches("(pointer: coarse)")).toBe(false);
  });
});

describe("媒体查询测试接缝：可编程换档", () => {
  it("宽度应答：setFakeMedia 后 min-width / max-width 按视口宽求值", () => {
    setFakeMedia({ width: 400 });
    expect(matches("(min-width: 100px)")).toBe(true);
    expect(matches("(min-width: 500px)")).toBe(false);
    expect(matches("(max-width: 500px)")).toBe(true);
    expect(matches("(max-width: 399px)")).toBe(false);
  });

  it("hover / pointer 应答：设定后按值命中对应查询", () => {
    setFakeMedia({ hover: "none", pointer: "coarse" });
    expect(matches("(hover: none)")).toBe(true);
    expect(matches("(hover: hover)")).toBe(false);
    expect(matches("(pointer: coarse)")).toBe(true);
    expect(matches("(pointer: fine)")).toBe(false);
  });

  it("宽度比较按数值而非字符串（含小数边界）", () => {
    setFakeMedia({ width: 850 });
    expect(matches("(min-width: 849.98px)")).toBe(true);
    setFakeMedia({ width: 849 });
    expect(matches("(min-width: 849.98px)")).toBe(false);
  });

  it("not 前缀：内层已知查询取反", () => {
    setFakeMedia({ width: 400 });
    expect(matches("not (min-width: 300px)")).toBe(false);
    expect(matches("not (min-width: 500px)")).toBe(true);
  });

  it("and 组合：全部已知且全真才命中，一假即不命中", () => {
    setFakeMedia({ hover: "hover", pointer: "fine" });
    expect(matches("(hover: hover) and (pointer: fine)")).toBe(true);
    setFakeMedia({ pointer: "coarse" });
    expect(matches("(hover: hover) and (pointer: fine)")).toBe(false);
  });

  it("接缝不认识的查询一律不应答命中（外来消费者与既有全局桩行为一致）", () => {
    setFakeMedia({ width: 1280, hover: "hover", pointer: "fine" });
    expect(matches("screen")).toBe(false);
    expect(matches("(prefers-reduced-motion: reduce)")).toBe(false);
  });
});

describe("媒体查询测试接缝：change 派发（MediaQueryList 语义）", () => {
  it("重编程使 matches 翻转时，addEventListener 与 onchange 都收到事件", () => {
    setFakeMedia({ width: 1280 });
    const mql = window.matchMedia("(min-width: 1000px)");
    expect(mql.matches).toBe(true);
    const viaListener: MediaQueryListEvent[] = [];
    const viaOnchange: MediaQueryListEvent[] = [];
    mql.addEventListener("change", (e) => viaListener.push(e));
    mql.onchange = (e) => viaOnchange.push(e);

    setFakeMedia({ width: 400 });
    expect(mql.matches).toBe(false);
    expect(viaListener).toHaveLength(1);
    expect(viaListener[0].matches).toBe(false);
    expect(viaOnchange).toHaveLength(1);
  });

  it("重编程未翻转 matches 时不派发 change（浏览器同款语义）", () => {
    setFakeMedia({ width: 1280 });
    const mql = window.matchMedia("(min-width: 1000px)");
    let fires = 0;
    mql.addEventListener("change", () => {
      fires += 1;
    });

    setFakeMedia({ hover: "none" });
    expect(fires).toBe(0);
    setFakeMedia({ width: 2000 });
    expect(fires).toBe(0);
  });

  it("legacy addListener / removeListener 同样收发事件", () => {
    setFakeMedia({ width: 1280 });
    const mql = window.matchMedia("(min-width: 1000px)");
    let fires = 0;
    const listener = (): void => {
      fires += 1;
    };
    mql.addListener(listener);

    setFakeMedia({ width: 400 });
    expect(fires).toBe(1);

    mql.removeListener(listener);
    setFakeMedia({ width: 2000 });
    expect(fires).toBe(1);
  });
});

describe("媒体查询测试接缝：resetFakeMedia（显式复位，全局壳层同款）", () => {
  it("复位后状态回默认、此前的监听不再收到事件", () => {
    setFakeMedia({ width: 400 });
    const mql = window.matchMedia("(min-width: 1000px)");
    let fires = 0;
    mql.addEventListener("change", () => {
      fires += 1;
    });

    resetFakeMedia();
    expect(mql.matches).toBe(false);
    expect(matches("(min-width: 100px)")).toBe(true);
    setFakeMedia({ width: 400 });
    expect(fires).toBe(0);
  });
});

const AxisProbe = defineComponent({
  name: "AxisProbe",
  setup() {
    const tier = useWindowTier();
    const mode = useInputMode();
    return () => h("div", { "data-tier": tier.value, "data-mode": mode.value });
  },
});

describe("媒体查询测试接缝：消费示例（后续票照此换档）", () => {
  it("mount 前设定信号：组件按档位 / 轴渲染分支", async () => {
    setFakeMedia({ width: 360, hover: "none", pointer: "coarse" });
    const wrapper = await mountFlushed(AxisProbe);
    expect(wrapper.attributes("data-tier")).toBe("mobile");
    expect(wrapper.attributes("data-mode")).toBe("touch");
  });

  it("默认状态挂载：桌面档 + 指针轴（既有测试零迁移语义）", async () => {
    const wrapper = await mountFlushed(AxisProbe);
    expect(wrapper.attributes("data-tier")).toBe("desktop");
    expect(wrapper.attributes("data-mode")).toBe("pointer");
  });

  it("mount 后重编程：档位实时切换，交互轴实时换轴", async () => {
    const wrapper = await mountFlushed(AxisProbe);
    expect(wrapper.attributes("data-tier")).toBe("desktop");

    setFakeMedia({ width: 360 });
    await nextTick();
    expect(wrapper.attributes("data-tier")).toBe("mobile");

    setFakeMedia({ hover: "none", pointer: "coarse" });
    await nextTick();
    expect(wrapper.attributes("data-mode")).toBe("touch");
  });
});
