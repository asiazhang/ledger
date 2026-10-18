import { describe, it, expect, vi, beforeEach } from "vitest";
import { mockInvoke, wireInvokeSeam } from "@ledger/test-support/invoke-mock";
import { useTransferForm } from "@/transaction/useTransferForm";
import { useReferenceStore } from "@/stores/reference";
import type { Account, Transaction } from "@ledger/types";

const mockAccounts: Account[] = [
  {
    id: "acc-1",
    name: "现金",
    type: "cash",
    currency_code: "CNY",
    initial_balance_cents: 0,
    created_at: "2026-01-01T00:00:00Z",
    is_hidden: false,
    is_archived: false,
    updated_at: "2026-01-01T00:00:00Z",
    version: 1,
    device_id: "test",
    is_deleted: false,
  },
  {
    id: "acc-2",
    name: "银行",
    type: "bank",
    currency_code: "CNY",
    initial_balance_cents: 0,
    created_at: "2026-01-01T00:00:00Z",
    is_hidden: false,
    is_archived: false,
    updated_at: "2026-01-01T00:00:00Z",
    version: 1,
    device_id: "test",
    is_deleted: false,
  },
];

/** 参考命令覆写集（beforeEach 与用例级覆写共享）：账户保留本文件夹具，其余三表空集。 */
const REFERENCE_OVERRIDES = {
  list_accounts: mockAccounts,
};

describe("useTransferForm", () => {
  beforeEach(() => {
    wireInvokeSeam({ overrides: { ...REFERENCE_OVERRIDES } });
  });

  it("submit 校验：转出转入账户相同时提示警告（不调用写入命令）", async () => {
    const form = useTransferForm();
    form.accountId.value = "acc-1";
    form.toAccountId.value = "acc-1";
    form.amountText.value = "100";

    await expect(form.submit()).resolves.toBeUndefined();
    expect(mockInvoke.mock.calls.filter(([cmd]) => cmd === "create_transaction")).toHaveLength(0);
  });

  it("submit 调用 api.createTransaction（创建路径）", async () => {
    wireInvokeSeam({ overrides: { ...REFERENCE_OVERRIDES, create_transaction: "new-txn-id" } });
    const form = useTransferForm();
    form.accountId.value = "acc-1";
    form.toAccountId.value = "acc-2";
    form.amountText.value = "200";

    await form.submit();

    // 提交路由：创建命令 + 正确 kind；wire 字段形状由装配器测试承担（issue #216）
    expect(mockInvoke).toHaveBeenCalledWith("create_transaction", {
      input: expect.objectContaining({ kind: "transfer" }),
    });
  });

  describe("编辑模式（issue #178）", () => {
    const editingTx: Transaction = {
      id: "txn-100",
      kind: "transfer",
      amount_cents: 50000,
      currency_code: "CNY",
      amount_native_cents: 50000,
      fx_rate_used: null,
      fx_rate_source: null,
      source_order_no: null,
      account_id: "acc-1",
      to_account_id: "acc-2",
      funding_account_id: null,
      fundings: [],
      purchases: [],
      category_id: null,
      merchant_id: null,
      policy_id: null,
      source: null,
      convert: null,
      refund_of_transaction_id: null,
      note: "房租",
      date: "2026-03-01",
      created_at: "2026-03-01T00:00:00Z",
      updated_at: "2026-03-01T00:00:00Z",
      version: 3,
      device_id: "test",
      is_deleted: false,
    };

    it("创建时按既有交易回填全部业务字段", () => {
      const form = useTransferForm({ editing: () => editingTx });
      expect(form.amountText.value).toBe("500");
      expect(form.currencyCode.value).toBe("CNY");
      expect(form.accountId.value).toBe("acc-1");
      expect(form.toAccountId.value).toBe("acc-2");
      expect(form.note.value).toBe("房租");
      expect(new Date(form.date.value).toISOString().slice(0, 10)).toBe("2026-03-01");
    });

    it("submit 走更新命令：同形入参（无幂等键）+ 交易 id，成功触发 onUpdated", async () => {
      wireInvokeSeam({ overrides: { ...REFERENCE_OVERRIDES, update_transaction: undefined } });
      const onCreated = vi.fn();
      const onUpdated = vi.fn();
      const form = useTransferForm({
        onCreated,
        onUpdated,
        editing: () => ({ ...editingTx, source_order_no: "JD-9001" }),
      });
      // 用户改转入账户与金额
      form.toAccountId.value = "acc-1";
      form.accountId.value = "acc-2";
      form.amountText.value = "300";

      await form.submit();

      // 提交路由：更新命令携带交易 id + 用户改动字段交接装配结果；
      // 金额/日期转换与占位字段由装配器测试承担（issue #216）
      expect(mockInvoke).toHaveBeenCalledWith("update_transaction", {
        id: "txn-100",
        input: expect.objectContaining({
          kind: "transfer",
          account_id: "acc-2",
          to_account_id: "acc-1",
          funding_account_id: null,
          note: "房租",
          // 来源订单号随编辑原样透传（#1862）：表单无输入位，静默清除即摧毁订单锚点
          source_order_no: "JD-9001",
        }),
      });
      expect(mockInvoke.mock.calls.filter(([cmd]) => cmd === "create_transaction")).toHaveLength(0);
      expect(onUpdated).toHaveBeenCalledTimes(1);
      expect(onCreated).not.toHaveBeenCalled();
    });

    it("提交失败不重置已填内容且不触发 onUpdated", async () => {
      wireInvokeSeam({
        overrides: {
          ...REFERENCE_OVERRIDES,
          update_transaction: () => Promise.reject("转账必须指定目标账户"),
        },
      });
      const onUpdated = vi.fn();
      const form = useTransferForm({ onUpdated, editing: () => editingTx });
      form.toAccountId.value = null;

      await expect(form.submit()).resolves.toBeUndefined();

      expect(onUpdated).not.toHaveBeenCalled();
      expect(form.amountText.value).toBe("500");
      expect(form.note.value).toBe("房租");
    });
  });

  describe("币种一致性联动（issue #1770 / ADR-0134 决策 5）", () => {
    const usdAccount: Account = {
      ...mockAccounts[0],
      id: "acc-usd",
      name: "美元户",
      currency_code: "USD",
    };

    async function formWithUsd() {
      wireInvokeSeam({ overrides: { list_accounts: [...mockAccounts, usdAccount] } });
      await useReferenceStore().refresh();
      return useTransferForm();
    }

    it("币种随转出账户推导：未选退展示币种偏好，选 USD 户变 USD（先例：useInvestmentForm）", async () => {
      const form = await formWithUsd();
      expect(form.currencyCode.value).toBe("CNY");
      form.accountId.value = "acc-usd";
      expect(form.currencyCode.value).toBe("USD");
    });

    it("转入候选随转出币种收窄；未选转出前不收窄", async () => {
      const form = await formWithUsd();
      expect(form.toAccountOptions.value.map((o) => o.value)).toEqual([
        "acc-1",
        "acc-2",
        "acc-usd",
      ]);
      form.accountId.value = "acc-usd";
      expect(form.toAccountOptions.value.map((o) => o.value)).toEqual(["acc-usd"]);
    });

    it("存量脏行编辑回填两端异币种：toEndCurrencyError 即时置位，submit 静默中止不发命令", async () => {
      wireInvokeSeam({ overrides: { list_accounts: [...mockAccounts, usdAccount] } });
      await useReferenceStore().refresh();
      const onUpdated = vi.fn();
      const dirty: Transaction = {
        id: "txn-dirty",
        kind: "transfer",
        amount_cents: 50000,
        currency_code: "CNY",
        amount_native_cents: 50000,
        fx_rate_used: null,
        fx_rate_source: null,
        source_order_no: null,
        account_id: "acc-1",
        to_account_id: "acc-usd",
        funding_account_id: null,
        fundings: [],
        purchases: [],
        category_id: null,
        merchant_id: null,
        policy_id: null,
        source: null,
        convert: null,
        refund_of_transaction_id: null,
        note: "房租",
        date: "2026-03-01",
        created_at: "2026-03-01T00:00:00Z",
        updated_at: "2026-03-01T00:00:00Z",
        version: 3,
        device_id: "test",
        is_deleted: false,
      };
      const form = useTransferForm({ editing: () => dirty, onUpdated });
      expect(form.toEndCurrencyError.value).toBe(true);

      await form.submit();

      expect(onUpdated).not.toHaveBeenCalled();
      expect(mockInvoke.mock.calls.filter(([cmd]) => cmd === "update_transaction")).toHaveLength(0);
    });
  });

  describe("账户归档（spec #1952 / ADR-0142 决策 4）：选择面排除 + 编辑面补值", () => {
    /** 归档账户（任一类型皆可归档，ADR-0142 决策 6）：转出/转入两端各一 */
    const archivedFrom: Account = {
      ...mockAccounts[0],
      id: "acc-arch-from",
      name: "旧储蓄卡",
      type: "bank",
      is_archived: true,
    };
    const archivedTo: Account = {
      ...mockAccounts[1],
      id: "acc-arch-to",
      name: "旧工资卡",
      is_archived: true,
    };

    async function formWithArchived() {
      wireInvokeSeam({ overrides: { list_accounts: [...mockAccounts, archivedFrom, archivedTo] } });
      await useReferenceStore().refresh();
      return useTransferForm();
    }

    /** 编辑一笔挂归档账户的转账（两端分别指向 archivedFrom / archivedTo） */
    function editingTransfer(accountId: string, toAccountId: string): Transaction {
      return {
        ...editingTx,
        account_id: accountId,
        to_account_id: toAccountId,
      };
    }

    const editingTx: Transaction = {
      id: "txn-arch",
      kind: "transfer",
      amount_cents: 50000,
      currency_code: "CNY",
      amount_native_cents: 50000,
      fx_rate_used: null,
      fx_rate_source: null,
      source_order_no: null,
      account_id: "acc-1",
      to_account_id: "acc-2",
      funding_account_id: null,
      fundings: [],
      purchases: [],
      category_id: null,
      merchant_id: null,
      policy_id: null,
      source: null,
      convert: null,
      refund_of_transaction_id: null,
      note: "房租",
      date: "2026-03-01",
      created_at: "2026-03-01T00:00:00Z",
      updated_at: "2026-03-01T00:00:00Z",
      version: 3,
      device_id: "test",
      is_deleted: false,
    };

    it("新建：两端候选都不含归档账户", async () => {
      const form = await formWithArchived();
      expect(form.accountOptions.value.map((o) => o.value)).toEqual(["acc-1", "acc-2"]);
      expect(form.toAccountOptions.value.map((o) => o.value)).toEqual(["acc-1", "acc-2"]);
    });

    it("新建：转出账户已补值也不从转出侧漏进转入候选（补值只补已存值，不扩新选择面）", async () => {
      wireInvokeSeam({
        overrides: { list_accounts: [...mockAccounts, archivedFrom, archivedTo] },
      });
      await useReferenceStore().refresh();
      // 模拟「编辑后清空转入值再选新转入」：转出是被补值的归档账户
      const form = useTransferForm({
        editing: () => editingTransfer("acc-arch-from", "acc-2"),
      });
      form.toAccountId.value = null;
      // 转出侧补值可见（已存值），转入侧新选择面不含它
      expect(form.accountOptions.value[0]).toEqual({
        label: "旧储蓄卡（已归档）",
        value: "acc-arch-from",
      });
      expect(form.toAccountOptions.value.map((o) => o.value)).toEqual(["acc-1", "acc-2"]);
    });

    it("编辑：转出端已存值是归档账户 → 补回选项、标「已归档」、保持选中，提交原样携带不静默改挂", async () => {
      wireInvokeSeam({ overrides: { list_accounts: [...mockAccounts, archivedFrom, archivedTo] } });
      await useReferenceStore().refresh();
      const form = useTransferForm({
        editing: () => editingTransfer("acc-arch-from", "acc-2"),
      });
      expect(form.accountId.value).toBe("acc-arch-from");
      expect(form.accountOptions.value[0]).toEqual({
        label: "旧储蓄卡（已归档）",
        value: "acc-arch-from",
      });

      await form.submit();

      expect(mockInvoke).toHaveBeenCalledWith("update_transaction", {
        id: "txn-arch",
        input: expect.objectContaining({
          account_id: "acc-arch-from",
          to_account_id: "acc-2",
        }),
      });
    });

    it("编辑：转入端已存值是归档账户 → 补回转入候选并标「已归档」，值保持不变", async () => {
      wireInvokeSeam({ overrides: { list_accounts: [...mockAccounts, archivedFrom, archivedTo] } });
      await useReferenceStore().refresh();
      const form = useTransferForm({
        editing: () => editingTransfer("acc-1", "acc-arch-to"),
      });
      expect(form.toAccountId.value).toBe("acc-arch-to");
      expect(form.toAccountOptions.value).toContainEqual({
        label: "旧工资卡（已归档）",
        value: "acc-arch-to",
      });
      // 转出侧在用账户不受影响（补值只动已存值那一侧）
      expect(form.accountOptions.value.map((o) => o.value)).toEqual(["acc-1", "acc-2"]);
    });
  });
});
