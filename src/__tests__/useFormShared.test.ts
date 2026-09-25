import { describe, it, expect, beforeEach } from "vitest";
import { ref } from "vue";
import { wireInvokeSeam } from "@ledger/test-support/invoke-mock";
import { useAccountCurrency } from "@/composables/useFormShared";
import { useReferenceStore } from "@/stores/reference";
import { useAppStore } from "@/stores/app";
import type { Account } from "@ledger/types";

const usdAccount: Account = {
  id: "acc-usd",
  name: "美元户",
  type: "bank",
  currency_code: "USD",
  initial_balance_cents: 0,
  created_at: "2026-01-01T00:00:00Z",
  updated_at: "2026-01-01T00:00:00Z",
  version: 1,
  device_id: "test",
  is_deleted: false,
  is_hidden: false,
};

beforeEach(() => {
  // 参考字典命令由接缝内建规范夹具兜底（acc-1 = CNY）；USD 户按需覆写。
  wireInvokeSeam();
});
describe("useFormShared.useAccountCurrency（币种随账户推导，#1775 / ADR-0134 决策 5）", () => {
  it("未选账户（null）：回落展示币种偏好", async () => {
    await useReferenceStore().refresh();
    const accountId = ref<string | null>(null);
    const currencyCode = useAccountCurrency(accountId);
    // 钉死字面量而非读实现（app store 缺省偏好即 CNY）：期望与被测实现解耦（ADR-0087 断言强度）
    expect(currencyCode.value).toBe("CNY");
  });

  it("选中账户：账户币种；随 accountId 变化联动", async () => {
    wireInvokeSeam({ overrides: { list_accounts: [usdAccount] } });
    await useReferenceStore().refresh();
    const accountId = ref<string | null>(null);
    const currencyCode = useAccountCurrency(accountId);
    accountId.value = "acc-usd";
    expect(currencyCode.value).toBe("USD");
    accountId.value = null;
    expect(currencyCode.value).toBe("CNY");
  });

  it("账户未选时回落值随展示币种偏好变化", async () => {
    await useReferenceStore().refresh();
    const accountId = ref<string | null>(null);
    const currencyCode = useAccountCurrency(accountId);
    useAppStore().setDefaultCurrency("HKD");
    expect(currencyCode.value).toBe("HKD");
  });
});
