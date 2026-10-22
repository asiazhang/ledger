import { beforeEach, describe, expect, it } from "vitest";
import { installMockBackend } from "./index";
import { invoke } from "./tauri/core";
import { listen } from "./tauri/event";

/**
 * 交易读扩展替身行为测试（issue #1968 / spec #1964 / ADR-0143）：搜索、批量创建
 * 与订单汇总。接缝 = tauri 命令面（`installMockBackend` + `invoke`），删除任一
 * 登记行即未命中报错、本文件变红（接线负向判据，ADR-0087 断言强度）。
 *
 * 期望值为独立人算字面量。夹具未删交易 11 笔（同 reports.test.ts 头注），其中
 * 005/006 为同单 SO-88 京东两行（2026-01-08）、101–105 为投资夹具（#1970，2026-02）。
 *
 * search_transactions 载荷是扁平 camelCase（api 层直传四个筛选参数，不走 obj()
 * 嵌套惯例）。替身从简口径（transactions.ts 头注）：匹配面只做备注/商户名
 * （账户名/购买项名称不模拟）、排序 date DESC + created_at DESC + id DESC。
 */

beforeEach(() => {
  installMockBackend(); // 重装即重播种：每用例从夹具种子起步
});

/** 订阅 ledger:changed 并返回触发计数。 */
function captureLedgerChanged(): () => number {
  let count = 0;
  void listen("ledger:changed", () => {
    count++;
  });
  return () => count;
}

describe("交易搜索：search_transactions（关键字 + 筛选 AND 组合，服务端分页）", () => {
  it("商户名关键字命中该商户的全部交易（人算：美团 001 expense + 004 refund，date 倒序）", async () => {
    const result = await invoke<{ items: Array<{ id: string }>; total: number }>(
      "search_transactions",
      {
        query: "美团",
        page: 1,
        pageSize: 20,
        amountMinCents: null,
        amountMaxCents: null,
        dateFrom: null,
        dateTo: null,
      },
    );
    expect(result.items.map((t) => t.id)).toEqual(["mock-txn-004", "mock-txn-001"]);
    expect(result.total).toBe(2);
  });

  it("备注子串关键字命中（「午餐」只命中 001）", async () => {
    const result = await invoke<{ items: Array<{ id: string }>; total: number }>(
      "search_transactions",
      {
        query: "午餐",
        page: 1,
        pageSize: 20,
        amountMinCents: null,
        amountMaxCents: null,
        dateFrom: null,
        dateTo: null,
      },
    );
    expect(result.items.map((t) => t.id)).toEqual(["mock-txn-001"]);
  });

  it("多词条 AND 组合：「美团 午餐」只命中同时带两词的 001（004 无「午餐」被 AND 掉）", async () => {
    const result = await invoke<{ items: Array<{ id: string }>; total: number }>(
      "search_transactions",
      {
        query: "美团 午餐",
        page: 1,
        pageSize: 20,
        amountMinCents: null,
        amountMaxCents: null,
        dateFrom: null,
        dateTo: null,
      },
    );
    expect(result.items.map((t) => t.id)).toEqual(["mock-txn-001"]);
    expect(result.total).toBe(1);
  });

  it("空关键字 + 无筛选维持空结果（既有语义）", async () => {
    await expect(
      invoke("search_transactions", {
        query: "",
        page: 1,
        pageSize: 20,
        amountMinCents: null,
        amountMaxCents: null,
        dateFrom: null,
        dateTo: null,
      }),
    ).resolves.toEqual({ items: [], total: 0 });
  });

  it("金额区间仅筛选路径（无关键字）：[2000,10000] 命中 9 笔（1200/300000 等出界）", async () => {
    const result = await invoke<{ items: Array<{ id: string }>; total: number }>(
      "search_transactions",
      {
        query: "",
        page: 1,
        pageSize: 20,
        amountMinCents: 2000,
        amountMaxCents: 10000,
        dateFrom: null,
        dateTo: null,
      },
    );
    // 人算：005 8_800、001 2_500、102 卖出 3_000、105 分红 8_500 入界 +
    // 定时会员历史 301..305（2_500 × 5，#1973）入界；006 1_200、003 300_000、
    // 101 205_200、103 19_800、201/202 1_000、306 600_000、307 500_000 出界，date 倒序
    expect(result.items.map((t) => t.id)).toEqual([
      "mock-txn-105",
      "mock-txn-102",
      "mock-txn-305",
      "mock-txn-005",
      "mock-txn-001",
      "mock-txn-304",
      "mock-txn-303",
      "mock-txn-302",
      "mock-txn-301",
    ]);
    expect(result.total).toBe(9);
  });

  it("关键字与日期区间 AND：京东 + 到 01-08 含边界命中两行，收到 01-07 即空", async () => {
    const hit = await invoke<{ items: Array<{ id: string }>; total: number }>(
      "search_transactions",
      {
        query: "京东",
        page: 1,
        pageSize: 20,
        amountMinCents: null,
        amountMaxCents: null,
        dateFrom: "2026-01-01",
        dateTo: "2026-01-08",
      },
    );
    expect(hit.items.map((t) => t.id).sort()).toEqual(["mock-txn-005", "mock-txn-006"]);

    const miss = await invoke<{ items: unknown[]; total: number }>("search_transactions", {
      query: "京东",
      page: 1,
      pageSize: 20,
      amountMinCents: null,
      amountMaxCents: null,
      dateFrom: "2026-01-01",
      dateTo: "2026-01-07",
    });
    expect(miss.total).toBe(0);
  });

  it("服务端分页：当月 8 笔命中、pageSize=2 翻页 total 恒为全集", async () => {
    // 人算：2026-01 = 原有 6 笔 + 会员历史 305（01-10）+ 分期历史 202（01-15，#1973）
    const page1 = await invoke<{ items: Array<{ id: string }>; total: number }>(
      "search_transactions",
      {
        query: "",
        page: 1,
        pageSize: 2,
        amountMinCents: null,
        amountMaxCents: null,
        dateFrom: "2026-01-01",
        dateTo: "2026-01-31",
      },
    );
    expect(page1.items.map((t) => t.id)).toEqual(["mock-txn-003", "mock-txn-202"]);
    expect(page1.total).toBe(8);

    const page2 = await invoke<{ items: Array<{ id: string }>; total: number }>(
      "search_transactions",
      {
        query: "",
        page: 2,
        pageSize: 2,
        amountMinCents: null,
        amountMaxCents: null,
        dateFrom: "2026-01-01",
        dateTo: "2026-01-31",
      },
    );
    // 同日 2026-01-10 两行按 created_at 倒序：002（09:00）在 305（08:00）前
    expect(page2.items.map((t) => t.id)).toEqual(["mock-txn-002", "mock-txn-305"]);
  });

  it("软删商户名仍可搜（历史引用口径）", async () => {
    await invoke<string>("create_transaction", {
      input: {
        kind: "expense",
        amount_cents: 3_300,
        currency_code: "CNY",
        account_id: "mock-acc-cash",
        merchant_id: "mock-mch-del",
        date: "2026-01-20",
      },
    });
    const result = await invoke<{ items: unknown[]; total: number }>("search_transactions", {
      query: "已删商户",
      page: 1,
      pageSize: 20,
      amountMinCents: null,
      amountMaxCents: null,
      dateFrom: null,
      dateTo: null,
    });
    expect(result.total).toBe(1);
  });

  it("读命令不发 ledger:changed", async () => {
    const changed = captureLedgerChanged();
    await invoke("search_transactions", {
      query: "美团",
      page: 1,
      pageSize: 20,
      amountMinCents: null,
      amountMaxCents: null,
      dateFrom: null,
      dateTo: null,
    });
    expect(changed()).toBe(0);
  });
});

describe("批量创建：create_transactions（逐笔记内存，ledger:changed 只发一次）", () => {
  it("两笔批量落内存：逐条结果带回 id，list_transactions total +2，恰发一次 ledger:changed", async () => {
    const changed = captureLedgerChanged();

    const results = await invoke<
      Array<{ success: boolean; duplicate: boolean; id: string | null; error: string | null }>
    >("create_transactions", {
      inputs: [
        {
          kind: "expense",
          amount_cents: 5_600,
          currency_code: "CNY",
          account_id: "mock-acc-cash",
          category_id: "mock-cat-transport",
          date: "2026-03-01",
        },
        {
          kind: "income",
          amount_cents: 8_800,
          currency_code: "CNY",
          account_id: "mock-acc-bank",
          date: "2026-03-02",
        },
      ],
    });

    expect(results).toHaveLength(2);
    for (const r of results) {
      expect(r.success).toBe(true);
      expect(r.duplicate).toBe(false);
      expect(r.id).toBeTruthy();
      expect(r.error).toBeNull();
    }

    const list = await invoke<{ total: number }>("list_transactions");
    expect(list.total).toBe(22); // 夹具 20（含 #1970 投资 5、#1973 定时 9）+ 新 2（人算）
    expect(changed()).toBe(1); // 批量只发一次（专门断言）
  });

  it("批量内商户即建随批一次信号：merchant_name 未命中即建、不重复建", async () => {
    const changed = captureLedgerChanged();
    await invoke("create_transactions", {
      inputs: [
        {
          kind: "expense",
          amount_cents: 1_000,
          currency_code: "CNY",
          account_id: "mock-acc-cash",
          date: "2026-03-03",
          merchant_name: "批发商户",
        },
        {
          kind: "expense",
          amount_cents: 2_000,
          currency_code: "CNY",
          account_id: "mock-acc-cash",
          date: "2026-03-04",
          merchant_name: "批发商户",
        },
      ],
    });
    const merchants = await invoke<Array<{ name: string }>>("list_merchants");
    expect(merchants.filter((m) => m.name === "批发商户")).toHaveLength(1);
    expect(changed()).toBe(1);
  });
});

describe("订单汇总：get_transaction_order_summary（同单行集只读投影）", () => {
  it("夹具同单 SO-88：2 行 / 直和 10_000 / 唯一币种 / 逐账户出资构成 / 明细日期升序", async () => {
    const summary = await invoke<{
      source_order_no: string;
      row_count: number;
      total_amount_cents: number;
      currency_code: string | null;
      accounts: Array<{ account_id: string; amount_cents: number }>;
      items: Array<{ id: string }>;
    }>("get_transaction_order_summary", { sourceOrderNo: "SO-88" });
    expect(summary.source_order_no).toBe("SO-88");
    expect(summary.row_count).toBe(2);
    expect(summary.total_amount_cents).toBe(10_000); // 8_800 + 1_200（人算直和）
    expect(summary.currency_code).toBe("CNY");
    expect(summary.accounts).toEqual([
      { account_id: "mock-acc-ewallet", amount_cents: 8_800 },
      { account_id: "mock-acc-cash", amount_cents: 1_200 },
    ]);
    expect(summary.items.map((t) => t.id)).toEqual(["mock-txn-005", "mock-txn-006"]);
  });

  it("不存在订单号：零行汇总不报错（空态渲染契约）", async () => {
    await expect(
      invoke("get_transaction_order_summary", { sourceOrderNo: "SO-NONE" }),
    ).resolves.toEqual({
      source_order_no: "SO-NONE",
      row_count: 0,
      total_amount_cents: 0,
      currency_code: null,
      accounts: [],
      items: [],
    });
  });

  it("混合币种订单 currency_code 为 null（不静默混算）", async () => {
    await invoke<string>("create_transaction", {
      input: {
        kind: "expense",
        amount_cents: 500,
        currency_code: "USD",
        account_id: "mock-acc-cash",
        date: "2026-01-09",
        source_order_no: "SO-88",
      },
    });
    const summary = await invoke<{
      currency_code: string | null;
      total_amount_cents: number;
      row_count: number;
    }>("get_transaction_order_summary", { sourceOrderNo: "SO-88" });
    expect(summary.currency_code).toBeNull();
    expect(summary.row_count).toBe(3);
    expect(summary.total_amount_cents).toBe(10_500); // 原币直和不折算（人算）
  });

  it("分解行逐出资项计入、同账户合并（ADR-0138 出资构成口径）", async () => {
    await invoke<string>("create_transaction", {
      input: {
        kind: "expense",
        amount_cents: 900,
        currency_code: "CNY",
        date: "2026-01-10",
        source_order_no: "SO-99",
        funding: [
          { account_id: "mock-acc-cash", amount_cents: 500, label: "定金" },
          { account_id: "mock-acc-bank", amount_cents: 400 },
        ],
      },
    });
    await invoke<string>("create_transaction", {
      input: {
        kind: "expense",
        amount_cents: 200,
        currency_code: "CNY",
        account_id: "mock-acc-bank",
        date: "2026-01-11",
        source_order_no: "SO-99",
      },
    });
    const summary = await invoke<{
      accounts: Array<{ account_id: string; amount_cents: number }>;
      total_amount_cents: number;
    }>("get_transaction_order_summary", { sourceOrderNo: "SO-99" });
    expect(summary.accounts).toEqual([
      { account_id: "mock-acc-cash", amount_cents: 500 },
      { account_id: "mock-acc-bank", amount_cents: 600 }, // 400 分解 + 200 单出资合并（人算）
    ]);
    expect(summary.total_amount_cents).toBe(1_100);
  });

  it("软删行不进订单汇总（行集与聚合同口径）", async () => {
    await invoke<void>("delete_transaction", { id: "mock-txn-006" });
    const summary = await invoke<{
      row_count: number;
      total_amount_cents: number;
      accounts: unknown[];
    }>("get_transaction_order_summary", { sourceOrderNo: "SO-88" });
    expect(summary.row_count).toBe(1);
    expect(summary.total_amount_cents).toBe(8_800);
    expect(summary.accounts).toEqual([{ account_id: "mock-acc-ewallet", amount_cents: 8_800 }]);
  });

  it("读命令不发 ledger:changed", async () => {
    const changed = captureLedgerChanged();
    await invoke("get_transaction_order_summary", { sourceOrderNo: "SO-88" });
    expect(changed()).toBe(0);
  });
});
