import { describe, it, expect, vi, beforeEach } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { defineComponent } from "vue";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import {
  PRICES_CHANGED_EVENT,
  usePricesChanged,
} from "@/composables/usePricesChanged";

const mockListen = vi.mocked(listen);

/** 承载 composable 生命周期的宿主组件。 */
function mountHost(callback: () => void) {
  const Host = defineComponent({
    setup() {
      usePricesChanged(callback);
      return () => null;
    },
  });
  return mount(Host);
}

beforeEach(() => {
  mockListen.mockReset();
});

describe("usePricesChanged 价格失效信号订阅基座（issue #237 / ADR-0031）", () => {
  it("订阅 ledger:prices-changed，注册的是调用方回调", async () => {
    let registered: (() => void) | undefined;
    mockListen.mockImplementation(async (_event: string, handler: never) => {
      registered = handler;
      return vi.fn() as unknown as UnlistenFn;
    });

    mountHost(() => {});

    expect(mockListen).toHaveBeenCalledTimes(1);
    expect(mockListen).toHaveBeenCalledWith(
      PRICES_CHANGED_EVENT,
      expect.any(Function),
    );
    // 常量单点定义：事件名即 ADR-0031 的 ledger:prices-changed
    expect(PRICES_CHANGED_EVENT).toBe("ledger:prices-changed");
    expect(registered).toBeTypeOf("function");
  });

  it("信号到达时调用回调（重拉自身数据由调用方承载）", async () => {
    let fire: () => void = () => {};
    mockListen.mockImplementation(async (_event: string, handler: never) => {
      fire = handler;
      return vi.fn() as unknown as UnlistenFn;
    });

    let pulls = 0;
    mountHost(() => {
      pulls += 1;
    });

    fire();
    fire();
    expect(pulls).toBe(2);
  });

  it("卸载时注销监听", async () => {
    const unlisten = vi.fn();
    mockListen.mockResolvedValue(unlisten as unknown as UnlistenFn);

    const wrapper = mountHost(() => {});
    await flushPromises(); // listen 注册异步落定
    wrapper.unmount();
    expect(unlisten).toHaveBeenCalledTimes(1);
  });

  it("注册在途时卸载：落定后立即注销，不留悬挂监听", async () => {
    let release!: (fn: UnlistenFn) => void;
    const unlisten = vi.fn();
    mockListen.mockImplementation(
      () =>
        new Promise<UnlistenFn>((resolve) => {
          release = resolve;
        }),
    );

    const wrapper = mountHost(() => {});
    wrapper.unmount();
    expect(unlisten).not.toHaveBeenCalled();

    // 卸载后注册才落定：应立即注销，而非挂到已卸载组件上
    release(unlisten as unknown as UnlistenFn);
    await flushPromises();
    expect(unlisten).toHaveBeenCalledTimes(1);
  });

  it("订阅注册失败不抛错（失败路径静默，与备份信号订阅先例一致）", async () => {
    mockListen.mockRejectedValue(new Error("注册失败"));

    const wrapper = mountHost(() => {});
    // 注册失败时卸载不得因 unlisten 未就绪而报错
    expect(() => wrapper.unmount()).not.toThrow();
  });
});
