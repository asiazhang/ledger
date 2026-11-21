import { describe, it, expect } from "vitest";
import { nextTick } from "vue";

import { amountPrivacyEnabled } from "@ledger/money";
import { applyLocale } from "@ledger/i18n";
import { useAppStore } from "@/stores/app";
import { useReferenceStore } from "@/stores/reference";
import { mountView, setAccountDb, setTxnDb, setMerchantDb, makeTxn } from "./common";
import { makeAccount, makeMerchant } from "../factories";

// 交易主列表展示行预计算接线（issue #2188）：表格展示文本与颜色随参考上下文
// 变化即时更新——金额隐私模式 / 界面语言 / 外观主题 / 参考数据失效重拉四轴的
// 表格级可观察断言。预计算在数据到达或上下文变化时经视图 computed 整体重派生，
// 表格 patch 期只读预计算字段（删除接线即计数守门 ① 变红，归
// src/__tests__/perf/render-gate.test.ts；本文件对准用户可观察结果）。

/** 隐私开关是展示格式化层模块级单点（不随本地存储清空自愈），用例结尾复原。 */
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
    // 借贷标签由预计算产出（kindLabel 单源）：换入借贷账户集并重拉参考数据
    //（lending.test.ts 同款先例），切 en-US 后行内标签即时为英文。
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
    // 还原语言状态（单例，避免污染其他测试文件）
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
    // 交易行挂商户：表格商户列渲染 MerchantLink（merchantMap 单源）
    setTxnDb([makeTxn(1, "acc-1", { merchant_id: "mch-1" })]);
    const wrapper = await mountView();
    expect(wrapper.find('td[data-col-key="merchant_id"]').text()).toBe("京东");
    // 改写商户字典后走失效重拉同一出口（ledger:changed 事件订阅调 refresh）：
    // 表格商户名即时刷新（行内组件与预计算记录同源消费 merchantMap）。
    setMerchantDb([makeMerchant({ id: "mch-1", name: "京东自营" })]);
    const reference = useReferenceStore();
    const listCountBefore = wrapper.findAll(".n-data-table-tbody .n-data-table-tr").length;
    await reference.refresh();
    await nextTick();
    expect(wrapper.find('td[data-col-key="merchant_id"]').text()).toBe("京东自营");
    // 行集不变（重拉只刷新字典，不重发列表请求）
    expect(wrapper.findAll(".n-data-table-tbody .n-data-table-tr").length).toBe(listCountBefore);
  });
});
