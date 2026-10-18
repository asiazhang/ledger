import { describe, it, expect, beforeEach } from "vitest";
import { ref } from "vue";
import { wireInvokeSeam } from "@ledger/test-support/invoke-mock";
import {
  useAccountCurrency,
  useEntityCurrency,
  useFormShared,
  usePinnedAccountOptions,
} from "@/composables/useFormShared";
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
  is_archived: false,
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

describe("useFormShared.usePinnedAccountOptions（编辑面补值：已选值是归档账户，spec #1952 / ADR-0142 决策 4）", () => {
  /** 归档账户行：可选项里已排除，但已存值必须能补回并带标记 */
  const archivedAccount: Account = {
    ...usdAccount,
    id: "acc-archived",
    name: "借出·张三",
    type: "receivable",
    is_archived: true,
  };

  async function setupArchived() {
    wireInvokeSeam({ overrides: { list_accounts: [usdAccount, archivedAccount] } });
    await useReferenceStore().refresh();
    return useFormShared();
  }

  it("当前值是归档账户：补回选项头部、标「已归档」、值保持不变（不静默改挂）", async () => {
    const { accountOptions } = await setupArchived();
    // 新选择面不含归档账户
    expect(accountOptions.value.map((o) => o.value)).toEqual(["acc-usd"]);
    const accountId = ref<string | null>("acc-archived");
    const pinned = usePinnedAccountOptions(accountId, accountOptions);
    expect(pinned.value).toEqual([
      { label: "借出·张三（已归档）", value: "acc-archived" },
      { label: "美元户", value: "acc-usd" },
    ]);
    expect(accountId.value).toBe("acc-archived");
  });

  it("当前值是在用账户 / 未选 / 未知 id：原数组原样返回（零开销，不复制不重复）", async () => {
    const { accountOptions } = await setupArchived();
    const active = ref<string | null>("acc-usd");
    const pinnedActive = usePinnedAccountOptions(active, accountOptions);
    expect(pinnedActive.value).toBe(accountOptions.value);

    const empty = ref<string | null>(null);
    const pinnedEmpty = usePinnedAccountOptions(empty, accountOptions);
    expect(pinnedEmpty.value).toBe(accountOptions.value);

    const unknown = ref<string | null>("acc-gone");
    const pinnedUnknown = usePinnedAccountOptions(unknown, accountOptions);
    expect(pinnedUnknown.value).toBe(accountOptions.value);
  });
});

describe("useFormShared.useEntityCurrency（币种随已持有对象推导，#1821 / ADR-0134 决策 5）", () => {
  it("未持对象（null）：回落展示币种偏好", async () => {
    await useReferenceStore().refresh();
    const source = ref<{ currency_code: string } | null>(null);
    const currencyCode = useEntityCurrency(source);
    // 钉死字面量而非读实现（app store 缺省偏好即 CNY）：期望与被测实现解耦（ADR-0087 断言强度）
    expect(currencyCode.value).toBe("CNY");
  });

  it("持有对象：取对象币种；随对象替换联动", async () => {
    await useReferenceStore().refresh();
    const source = ref<{ currency_code: string } | null>(null);
    const currencyCode = useEntityCurrency(source);
    source.value = { currency_code: "USD" };
    expect(currencyCode.value).toBe("USD");
    source.value = { currency_code: "JPY" };
    expect(currencyCode.value).toBe("JPY");
    source.value = null;
    expect(currencyCode.value).toBe("CNY");
  });

  it("未持对象时回落值随展示币种偏好变化", async () => {
    await useReferenceStore().refresh();
    const source = ref<{ currency_code: string } | null>(null);
    const currencyCode = useEntityCurrency(source);
    useAppStore().setDefaultCurrency("HKD");
    expect(currencyCode.value).toBe("HKD");
  });
});
