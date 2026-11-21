import { describe, it, expect, beforeEach } from "vitest";
import { wireInvokeSeam } from "@ledger/test-support/invoke-mock";
import { nextTick } from "vue";
import { flushPromises } from "@vue/test-utils";
import { mountFlushed, mountWithDialog } from "@ledger/test-support/mount";
import AddInstrumentModal from "@/investment/AddInstrumentModal.vue";
import ManualPriceModal from "@/investment/ManualPriceModal.vue";
import InstrumentBrowser from "@/investment/InstrumentBrowser.vue";
import { makeInstrument } from "../factories";

const mockInstruments = [
  makeInstrument({ id: "inst-1" }),
  makeInstrument({ id: "inst-2", symbol: "000001", name: "平安银行", market: "sz" }),
];

beforeEach(async () => {
  const seam = wireInvokeSeam({
    defaults: {
      list_instruments: { items: mockInstruments, total: mockInstruments.length },
    },
    refreshReferenceStores: true,
  });
  await seam.ready;
});

function modalCard(): HTMLElement {
  const card = document.body.querySelector<HTMLElement>(".n-card");
  expect(card, "弹窗卡片（NCard）应存在").not.toBeNull();
  return card!;
}

function expectCardSizeMd(card: HTMLElement) {
  expect(card.style.width).toBe("480px");
  expect(card.classList.contains("n-card--bordered")).toBe(false);
}

function mountBrowser() {
  return mountWithDialog(InstrumentBrowser);
}

async function clickToolbarButton(wrapper: ReturnType<typeof mountBrowser>, testid: string) {
  await wrapper.find(`[data-testid="${testid}"]`).trigger("click");
  await nextTick();
  await flushPromises();
}

describe("投资弹窗族排版统一（issue #638）", () => {
  it("添加投资标的弹窗（独立挂载）归 md 档且默认无边框", async () => {
    await mountFlushed(AddInstrumentModal, { props: { show: true } });
    expectCardSizeMd(modalCard());
  });

  it("手动报价弹窗归 md 档且默认无边框", async () => {
    await mountFlushed(ManualPriceModal, {
      props: { show: true, instrument: makeInstrument({ id: "inst-quote-1" }) },
    });
    expectCardSizeMd(modalCard());
  });

  it("添加投资标的弹窗（经标的页工具栏入口打开）归 md 档且默认无边框", async () => {
    const wrapper = mountBrowser();
    await flushPromises();
    await clickToolbarButton(wrapper, "add-instrument");
    expectCardSizeMd(modalCard());
  });

  it("全量同步确认/进度弹窗已随全量同步退役删除（issue #698）", async () => {
    const wrapper = mountBrowser();
    await flushPromises();
    expect(wrapper.find('[data-testid="full-sync"]').exists()).toBe(false);
  });
});
