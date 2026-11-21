import { describe, it, expect } from "vitest";
import { nextTick } from "vue";

import { amountPrivacyEnabled } from "@ledger/money";
import { applyLocale } from "@ledger/i18n";
import { useAppStore } from "@/stores/app";
import { useReferenceStore } from "@/stores/reference";
import { mountView, setAccountDb, setTxnDb, setMerchantDb, makeTxn } from "./common";
import { makeAccount, makeMerchant } from "../factories";

async function togglePrivacy(enabled: boolean): Promise<void> {
  amountPrivacyEnabled.value = enabled;
  await nextTick();
}

describe("TransactionsView 展示行预计算：参考上下文变化即时更新（issue #2188）", () => {
  it("金额隐私模式切换：当前页全部金额格即时切换掩码/真实数字，无需重新请求", async () => {
    const wrapper = await mountView();
    await togglePrivacy(true);
    try {
      const amountCells = wrapper.findAll(".amount-cell");
      expect(amountCells.length).toBeGreaterThan(0);
      for (const cell of amountCells) expect(cell.text()).toBe("••••");
      await togglePrivacy(false);
      for (const cell of wrapper.findAll(".amount-cell")) expect(cell.text()).not.toBe("••••");
    } finally {
      amountPrivacyEnabled.value = false;
    }
  });

  it("语言切换：表格行内类型标签（借贷派生）即时随 locale 更新", async () => {
    setAccountDb([
      makeAccount({ id: "acc-cash", name: "现金", type: "cash" }),
      makeAccount({ id: "acc-recv", name: "借出·张三", type: "receivable" }),
    ]);
    await useReferenceStore().refresh();
    setTxnDb([makeTxn(1, "acc-cash", { kind: "transfer", to_account_id: "acc-recv" })]);
    const wrapper = await mountView();
    const kindCell = wrapper.find('td[data-col-key="kind"]');
    expect(kindCell.text()).toBe("借出");
    await applyLocale("en-US");
    await nextTick();
    expect(wrapper.find('td[data-col-key="kind"]').text()).toBe("Lend out");
    await applyLocale("zh-CN");
  });

  it("外观主题切换：金额语义色即时换色（暗 ↔ 亮往返一致）", async () => {
    const wrapper = await mountView();
    const app = useAppStore();
    const colorOf = () => (wrapper.find(".amount-cell").element as HTMLElement).style.color;
    const dark = colorOf();
    app.setTheme("light");
    await nextTick();
    const light = colorOf();
    expect(light).not.toBe(dark);
    app.setTheme("dark");
    await nextTick();
    expect(colorOf()).toBe(dark);
  });

  it("参考数据失效重拉：商户名经 store refresh 同一出口即时更新，不重新请求列表", async () => {
    setTxnDb([makeTxn(1, "acc-1", { merchant_id: "mch-1" })]);
    const wrapper = await mountView();
    expect(wrapper.find('td[data-col-key="merchant_id"]').text()).toBe("京东");
    setMerchantDb([makeMerchant({ id: "mch-1", name: "京东自营" })]);
    const reference = useReferenceStore();
    const listCountBefore = wrapper.findAll(".n-data-table-tbody .n-data-table-tr").length;
    await reference.refresh();
    await nextTick();
    expect(wrapper.find('td[data-col-key="merchant_id"]').text()).toBe("京东自营");
    expect(wrapper.findAll(".n-data-table-tbody .n-data-table-tr").length).toBe(listCountBefore);
  });
});
