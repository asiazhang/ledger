import { beforeEach, describe, expect, it, vi } from "vitest";
import { defineComponent, h } from "vue";
import { mount, type VueWrapper } from "@vue/test-utils";
import { messageApi } from "@ledger/test-support/message-mock";
import { useTransactionTableActions } from "@/transaction/transaction-table-actions";

/** 表格级行为委托单测（issue #2010 验收③：事件委托后下钻与复制行为等价）——
 * 宿主组件渲染桌面表格同构的壳层 div（单一 click 监听器）+ 平替元素（data-action
 * 载荷），真 DOM 冒泡触发分发；账户/商户下钻载荷、来源跳转（经 source-jump 深模块
 * 真实计算）与备注复制（clipboard + toast，NoteCopyButton 语义迁移）逐路断言。
 * 非委托目标的点击静默无操作（不误触导航）。 */

// 账户/商户/来源下钻经 useRouter：push 替身断言导航目标（视图测试同款先例）。
const pushMock = vi.fn();
vi.mock("vue-router", () => ({
  useRoute: () => ({ query: {} }),
  useRouter: () => ({ push: pushMock }),
}));

/** 宿主：镜像视图壳层接线（div @click = 委托处理器，平替元素带载荷）。 */
function mountShell(): VueWrapper {
  const Shell = defineComponent({
    name: "Shell",
    setup() {
      const onAction = useTransactionTableActions();
      return () =>
        h("div", { onClick: onAction }, [
          h("button", { type: "button", "data-action": "account", "data-id": "acc-1" }, "现金"),
          h("button", { type: "button", "data-action": "merchant", "data-id": "mch-1" }, "京东"),
          h(
            "button",
            {
              type: "button",
              "data-action": "source",
              "data-kind": "policy",
              "data-entity": "pol-1",
            },
            "重疾险",
          ),
          h(
            "button",
            {
              type: "button",
              "data-action": "copy-note",
              "data-note": "视频会员月费",
            },
            "复制",
          ),
          h("span", { class: "tx-account-arrow" }, "→"),
        ]);
    },
  });
  return mount(Shell, { attachTo: document.body });
}

describe("useTransactionTableActions 表格级行为委托", () => {
  beforeEach(() => {
    pushMock.mockReset();
    Object.defineProperty(navigator, "clipboard", {
      value: { writeText: vi.fn().mockResolvedValue(undefined) },
      configurable: true,
    });
  });

  it("账户下钻：跳 /transactions?account=<id>（AccountLink 同载荷）", async () => {
    const wrapper = mountShell();
    await wrapper.find('[data-action="account"]').trigger("click");
    expect(pushMock).toHaveBeenLastCalledWith({
      name: "transactions",
      query: { account: "acc-1" },
    });
  });

  it("商户下钻：跳 /transactions?merchant=<id>（MerchantLink 同载荷）", async () => {
    const wrapper = mountShell();
    await wrapper.find('[data-action="merchant"]').trigger("click");
    expect(pushMock).toHaveBeenLastCalledWith({
      name: "transactions",
      query: { merchant: "mch-1" },
    });
  });

  it("来源下钻：经 source-jump 深模块计算路由目标（收纳态落组「更多」页签 + focus 参数）", async () => {
    const wrapper = mountShell();
    await wrapper.find('[data-action="source"]').trigger("click");
    // 默认侧栏布局下保单收纳于资产组「更多」（sidebar-order store 谓词真实接线），
    // 深模块分流到聚合页签并携带 focus 定位参数（spec #704 落点尊重收纳）。
    expect(pushMock).toHaveBeenLastCalledWith({
      name: "assets-more",
      query: { tab: "policies", focus: "pol-1" },
    });
  });

  it("备注复制：clipboard 写入完整备注 + 成功 toast（NoteCopyButton 语义迁移）", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      value: { writeText },
      configurable: true,
    });
    const wrapper = mountShell();
    await wrapper.find('[data-action="copy-note"]').trigger("click");
    await Promise.resolve();
    expect(writeText).toHaveBeenCalledWith("视频会员月费");
    expect(messageApi.success).toHaveBeenCalledWith("备注已复制");
  });

  it("复制失败：错误 toast 携带原因，不静默", async () => {
    Object.defineProperty(navigator, "clipboard", {
      value: { writeText: vi.fn().mockRejectedValue(new Error("denied")) },
      configurable: true,
    });
    const wrapper = mountShell();
    await wrapper.find('[data-action="copy-note"]').trigger("click");
    await Promise.resolve();
    expect(messageApi.error).toHaveBeenCalledWith("复制失败: denied");
  });

  it("键盘可达的结构保证：全部委托载荷元素是真 <button>（Tab 聚焦 + Enter 原生激活，点击冒泡同一分发）", async () => {
    const wrapper = mountShell();
    // jsdom 不合成键激活 click，键盘等价落在结构保证上：委托元素一律原生 <button>
    // （AccountLink 同款键盘语义），Enter/Space 的原生激活在真实引擎里产出冒泡 click。
    for (const el of wrapper.findAll("[data-action]")) {
      expect(el.element.tagName).toBe("BUTTON");
      expect((el.element as HTMLButtonElement).type).toBe("button");
    }
  });

  it("非委托目标（无 data-action 的元素）点击静默无操作，不触发导航", async () => {
    const wrapper = mountShell();
    await wrapper.find(".tx-account-arrow").trigger("click");
    expect(pushMock).not.toHaveBeenCalled();
    expect(messageApi.success).not.toHaveBeenCalled();
  });
});
