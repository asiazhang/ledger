import { beforeEach, describe, expect, it, vi } from "vitest";
import { installMockBackend } from "./index";
import { invoke } from "./tauri/core";
import { emitMockEvent, listen } from "./tauri/event";
import type { Transaction, TransactionListResult } from "@ledger/types";

/**
 * 替身后端行为测试（issue #1962 / ADR-0143）：接缝 = 应用所见的 tauri 命令面
 * （`installMockBackend` + `invoke`）与事件面（`listen`）。断言对准命令契约的
 * 返回形态与可观察状态迁移（写改内存、读投影内存、写后必发 ledger:changed），
 * 不探内表——ledger 内部结构变化时本文件应零改动。
 *
 * 替身刻意不追行为等价（ADR-0143 决策 1）：此处只钉首批主路径闭环的契约形状，
 * 不复刻 Rust 侧领域规则。
 */

beforeEach(() => {
  installMockBackend(); // 重装即重播种：每用例从夹具种子起步
});

/** 订阅 ledger:changed 并返回触发计数（断言失效信号必发，ADR-0143 决策 3）。 */
function captureLedgerChanged(): () => number {
  let count = 0;
  void listen("ledger:changed", () => {
    count++;
  });
  return () => count;
}

async function allTransactions(): Promise<Transaction[]> {
  const result = await invoke<TransactionListResult>("list_transactions");
  return result.items;
}

describe("启动门替身（ADR-0143 决策 6）", () => {
  it("get_boot_status 答「已解锁」，主界面直接挂载", async () => {
    await expect(invoke("get_boot_status")).resolves.toEqual({
      phase: "ready",
      error_code: null,
    });
  });
});

describe("参考数据读：五字典夹具投影", () => {
  it("list_currencies / list_accounts 返回夹具种子", async () => {
    const currencies = await invoke<Array<{ code: string }>>("list_currencies");
    expect(currencies.map((c) => c.code)).toContain("CNY");

    const accounts = await invoke<Array<{ id: string; type: string }>>("list_accounts");
    expect(accounts.map((a) => a.id)).toContain("mock-acc-cash");
  });

  it("includeDeleted 缺省过滤软删行，true 返回含删全量（历史引用口径）", async () => {
    const active = await invoke<Array<{ id: string }>>("list_categories");
    expect(active.map((c) => c.id)).not.toContain("mock-cat-del");

    const all = await invoke<Array<{ id: string; is_deleted: boolean }>>("list_categories", {
      includeDeleted: true,
    });
    expect(all.filter((c) => c.id === "mock-cat-del").map((c) => c.is_deleted)).toEqual([true]);
  });
});

describe("参考数据写：改内存 + ledger:changed 必发", () => {
  it("create_category 落内存，list 反映；写后恰发一次失效信号", async () => {
    const changed = captureLedgerChanged();

    const id = await invoke<string>("create_category", {
      input: { name: "测试分类", kind: "expense" },
    });

    const all = await invoke<Array<{ id: string; name: string }>>("list_categories", {
      includeDeleted: true,
    });
    expect(all.find((c) => c.id === id)?.name).toBe("测试分类");
    expect(changed()).toBe(1);
  });

  it("delete_category 软删：在用列表不可见，含删全量 is_deleted 翻转", async () => {
    const id = await invoke<string>("create_category", {
      input: { name: "待删分类", kind: "income" },
    });
    await invoke<void>("delete_category", { id });

    const active = await invoke<Array<{ id: string }>>("list_categories");
    expect(active.map((c) => c.id)).not.toContain(id);
    const all = await invoke<Array<{ id: string; is_deleted: boolean }>>("list_categories", {
      includeDeleted: true,
    });
    expect(all.find((c) => c.id === id)?.is_deleted).toBe(true);
  });

  it("update_merchant 改名后 list 反映", async () => {
    const id = await invoke<string>("create_merchant", { input: { name: "旧名" } });
    await invoke<void>("update_merchant", { id, input: { name: "新名" } });

    const all = await invoke<Array<{ id: string; name: string }>>("list_merchants");
    expect(all.find((m) => m.id === id)?.name).toBe("新名");
  });

  it("create_account 落内存并投影余额（期初口径）", async () => {
    const changed = captureLedgerChanged();

    const id = await invoke<string>("create_account", {
      input: { name: "新钱包", type: "cash", currency_code: "CNY", initial_balance_cents: 9_900 },
    });

    const balances =
      await invoke<Array<{ account: { id: string }; balance_cents: number }>>(
        "list_account_balances",
      );
    expect(balances.find((b) => b.account.id === id)?.balance_cents).toBe(9_900);
    expect(changed()).toBe(1);
  });

  it("archive_account / unarchive_account 翻转归档标志", async () => {
    await invoke<void>("archive_account", { id: "mock-acc-cash" });
    let row = (await invoke<Array<{ id: string; is_archived: boolean }>>("list_accounts")).find(
      (a) => a.id === "mock-acc-cash",
    );
    expect(row?.is_archived).toBe(true);

    await invoke<void>("unarchive_account", { id: "mock-acc-cash" });
    row = (await invoke<Array<{ id: string; is_archived: boolean }>>("list_accounts")).find(
      (a) => a.id === "mock-acc-cash",
    );
    expect(row?.is_archived).toBe(false);
  });
});

describe("交易读写闭环（ADR-0143 决策 2）", () => {
  it("create_transaction 落内存，list_transactions 反映（date 倒序置顶）", async () => {
    const changed = captureLedgerChanged();

    const id = await invoke<string>("create_transaction", {
      input: {
        kind: "expense",
        amount_cents: 3_300,
        currency_code: "CNY",
        account_id: "mock-acc-cash",
        category_id: "mock-cat-food",
        date: "2026-03-01",
        note: "晚餐",
      },
    });

    const items = await allTransactions();
    expect(items[0]?.id).toBe(id);
    expect(items[0]?.amount_cents).toBe(3_300);
    expect(changed()).toBe(1);
  });

  it("create_transaction 携 merchant_name 未命中即建商户，下次按名命中复用", async () => {
    const id = await invoke<string>("create_transaction", {
      input: {
        kind: "expense",
        amount_cents: 1_000,
        currency_code: "CNY",
        account_id: "mock-acc-cash",
        date: "2026-02-02",
        merchant_name: "全新商户",
      },
    });

    const merchants = await invoke<Array<{ id: string; name: string }>>("list_merchants");
    const created = merchants.find((m) => m.name === "全新商户");
    expect(created).toBeDefined();
    expect((await allTransactions()).find((t) => t.id === id)?.merchant_id).toBe(created?.id);

    // 复用：同名不再新建
    await invoke<string>("create_transaction", {
      input: {
        kind: "expense",
        amount_cents: 2_000,
        currency_code: "CNY",
        account_id: "mock-acc-cash",
        date: "2026-02-03",
        merchant_name: "全新商户",
      },
    });
    const again = await invoke<Array<{ id: string; name: string }>>("list_merchants");
    expect(again.filter((m) => m.name === "全新商户")).toHaveLength(1);
  });

  it("update_transaction 全字段替换，id 与 created_at 保持", async () => {
    const before = (await allTransactions()).find((t) => t.id === "mock-txn-001");
    await invoke<void>("update_transaction", {
      id: "mock-txn-001",
      input: {
        kind: "expense",
        amount_cents: 9_999,
        currency_code: "CNY",
        account_id: "mock-acc-cash",
        date: "2026-01-05",
        note: "改过",
      },
    });

    const after = (await allTransactions()).find((t) => t.id === "mock-txn-001");
    expect(after?.amount_cents).toBe(9_999);
    expect(after?.note).toBe("改过");
    expect(after?.created_at).toBe(before?.created_at);
  });

  it("delete_transaction 软删后列表不再返回，total 同步收窄", async () => {
    const before = await invoke<TransactionListResult>("list_transactions");
    await invoke<void>("delete_transaction", { id: "mock-txn-001" });

    const after = await invoke<TransactionListResult>("list_transactions");
    expect(after.items.map((t) => t.id)).not.toContain("mock-txn-001");
    expect(after.total).toBe(before.total - 1);
  });

  it("list_transactions 过滤：kinds 集合、日期区间、涉及账户", async () => {
    const onlyTransfer = await invoke<TransactionListResult>("list_transactions", {
      filter: { kinds: ["transfer"] },
    });
    expect(onlyTransfer.items.map((t) => t.kind)).toEqual(["transfer"]);

    const janFirst = await invoke<TransactionListResult>("list_transactions", {
      filter: { from: "2026-01-06", to: "2026-01-06" },
    });
    expect(janFirst.items.map((t) => t.id)).toEqual(["mock-txn-004"]);

    const involvingBank = await invoke<TransactionListResult>("list_transactions", {
      filter: { involving_account_id: "mock-acc-bank" },
    });
    // 投资夹具行：buy/sell 出资工资卡、dividend 到账工资卡（issue #1970）；
    // 理赔流入到账工资卡（issue #1974 保单挂单）
    expect(involvingBank.items.map((t) => t.id).sort()).toEqual([
      "mock-txn-002",
      "mock-txn-003",
      "mock-txn-101",
      "mock-txn-102",
      "mock-txn-105",
      "mock-txn-203",
    ]);
  });

  it("instrument_id 过滤命中证券扩展行，convert 任一腿同算（ADR-0107）", async () => {
    const byEtf = await invoke<TransactionListResult>("list_transactions", {
      filter: { instrument_id: "mock-inst-etf" },
    });
    // buy 直接持有 + convert 转入腿，date 倒序
    expect(byEtf.items.map((t) => t.id)).toEqual(["mock-txn-103", "mock-txn-101"]);
    expect(byEtf.total).toBe(2);

    const byFund = await invoke<TransactionListResult>("list_transactions", {
      filter: { instrument_id: "mock-inst-fund" },
    });
    // sell 直接持有 + convert 转出腿
    expect(byFund.items.map((t) => t.id)).toEqual(["mock-txn-103", "mock-txn-102"]);

    const byHk = await invoke<TransactionListResult>("list_transactions", {
      filter: { instrument_id: "mock-inst-hk" },
    });
    expect(byHk.items.map((t) => t.id)).toEqual(["mock-txn-105"]); // dividend 到账行
  });

  it("list_transactions 分页：items 切片 + total 恒为过滤全集", async () => {
    const page1 = await invoke<TransactionListResult>("list_transactions", {
      filter: { page: 1, page_size: 2 },
    });
    expect(page1.items).toHaveLength(2);
    expect(page1.total).toBe(13); // 通用 4 + 投资 5 + 保单挂单 4（issue #1974 夹具）
    expect(page1.items[0]?.id).toBe("mock-txn-105");

    const page2 = await invoke<TransactionListResult>("list_transactions", {
      filter: { page: 2, page_size: 2 },
    });
    expect(page2.items[0]?.id).toBe("mock-txn-103");
  });

  it("hide_investment_related 排除任一端为投资账户的行", async () => {
    await invoke<string>("create_transaction", {
      input: {
        kind: "expense",
        amount_cents: 5_000,
        currency_code: "CNY",
        account_id: "mock-acc-invest",
        date: "2026-02-04",
      },
    });

    const hidden = await invoke<TransactionListResult>("list_transactions", {
      filter: { hide_investment_related: true },
    });
    expect(hidden.items.map((t) => t.account_id)).not.toContain("mock-acc-invest");
    // 全部 14 行（13 夹具 + 1 新建）隐藏 5 行（投资账户端 4 + 新建投资账户 expense）；
    // dividend 到账工资卡、保单挂单流水（非投资账户）不隐藏
    expect(hidden.total).toBe(9);
  });

  it("list_account_balances = 期初 + 流水直和（记一笔后余额反映）", async () => {
    const before =
      await invoke<Array<{ account: { id: string }; balance_cents: number }>>(
        "list_account_balances",
      );
    const cashBefore = before.find((b) => b.account.id === "mock-acc-cash")?.balance_cents ?? 0;

    await invoke<string>("create_transaction", {
      input: {
        kind: "expense",
        amount_cents: 1_234,
        currency_code: "CNY",
        account_id: "mock-acc-cash",
        date: "2026-02-05",
      },
    });

    const after =
      await invoke<Array<{ account: { id: string }; balance_cents: number }>>(
        "list_account_balances",
      );
    expect(after.find((b) => b.account.id === "mock-acc-cash")?.balance_cents).toBe(
      cashBefore - 1_234,
    );
  });
});

describe("会话状态：重装即重播种（刷新重置语义，ADR-0143 决策 6）", () => {
  it("installMockBackend 重装后写入丢弃、状态回夹具种子", async () => {
    await invoke<string>("create_transaction", {
      input: {
        kind: "expense",
        amount_cents: 7_777,
        currency_code: "CNY",
        account_id: "mock-acc-cash",
        date: "2026-02-06",
      },
    });
    expect((await invoke<TransactionListResult>("list_transactions")).total).toBe(14);

    installMockBackend();

    expect((await invoke<TransactionListResult>("list_transactions")).total).toBe(13);
  });
});

describe("未实现命令：含命令名报错（console + toast 双出口）", () => {
  it("invoke 未登记命令以含命令名的错误拒绝，console.error 与 toastError 双出口", async () => {
    const errSpy = vi.spyOn(console, "error").mockImplementation(() => {});
    const toast = vi.fn();
    // 经 loadable 注册接口挂假 sink：断言走公共接缝，不探模块私有 sink 状态
    const { registerToastSink } = await import("@ledger/loadable");
    registerToastSink({ error: toast });

    await expect(invoke("dashboard_overview")).rejects.toThrow(/dashboard_overview/);
    expect(errSpy).toHaveBeenCalledWith(expect.stringContaining("dashboard_overview"));
    expect(toast).toHaveBeenCalledWith(expect.stringContaining("dashboard_overview"));

    errSpy.mockRestore();
  });
});

describe("事件面：listen / emitMockEvent（tauri/event 替身）", () => {
  it("同名监听全部收到分发，unlisten 后自身不再收到", async () => {
    const seen: string[] = [];
    const unlisten = await listen("mock:event", (evt) => {
      seen.push(`a:${String(evt.payload)}`);
    });
    const second: string[] = [];
    await listen("mock:event", (evt) => {
      second.push(`b:${String(evt.payload)}`);
    });
    const other: number[] = [];
    await listen("mock:other", (evt) => {
      other.push(Number(evt.payload));
    });

    emitMockEvent("mock:event", "ping");
    expect(seen).toEqual(["a:ping"]);
    expect(second).toEqual(["b:ping"]);
    expect(other).toEqual([]);

    unlisten();
    emitMockEvent("mock:event", "pong");
    // 仅被撤销者停收，其余监听不受牵连
    expect(seen).toEqual(["a:ping"]);
    expect(second).toEqual(["b:ping", "b:pong"]);
  });

  it("Event 形状投影：event 名与 payload 按 tauri 契约携带", async () => {
    let received: { event: string; payload: unknown } | null = null;
    await listen("ledger:changed", (evt) => {
      received = { event: evt.event, payload: evt.payload };
    });
    emitMockEvent("ledger:changed");
    expect(received).toEqual({ event: "ledger:changed", payload: undefined });
  });
});
