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
        date: "2026-02-01",
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
    expect(involvingBank.items.map((t) => t.id).sort()).toEqual(["mock-txn-002", "mock-txn-003"]);
  });

  it("list_transactions 分页：items 切片 + total 恒为过滤全集", async () => {
    const page1 = await invoke<TransactionListResult>("list_transactions", {
      filter: { page: 1, page_size: 2 },
    });
    expect(page1.items).toHaveLength(2);
    expect(page1.total).toBe(6);
    expect(page1.items[0]?.id).toBe("mock-txn-003");

    const page2 = await invoke<TransactionListResult>("list_transactions", {
      filter: { page: 2, page_size: 2 },
    });
    // 同日 2026-01-08 两行按 created_at 倒序：006 后建在前
    expect(page2.items.map((t) => t.id)).toEqual(["mock-txn-006", "mock-txn-005"]);
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
    expect(hidden.total).toBe(6);
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

describe("余额校准：adjust_account_balance（黑洞转账，ADR-0026）", () => {
  /** 人算基线：cash = 期初 50_000 − 006 支出 1_200 = 48_800；
   * bank = 1_000_000 + 002 收入 1_200_000 − 003 转出 300_000 = 1_900_000。 */
  it("调高：落黑洞→账户转账，余额到目标，恰发一次 ledger:changed", async () => {
    const changed = captureLedgerChanged();

    const txId = await invoke<string>("adjust_account_balance", {
      id: "mock-acc-cash",
      input: { target_balance_cents: 60_000, date: "2026-01-30" },
    });

    expect(txId).toBeTruthy();
    const balances =
      await invoke<Array<{ account: { id: string }; balance_cents: number }>>(
        "list_account_balances",
      );
    expect(balances.find((b) => b.account.id === "mock-acc-cash")?.balance_cents).toBe(60_000);

    // 转账行：差额 11_200（60_000 − 48_800），方向黑洞→cash，note 缺省「余额调整」
    const list = await invoke<TransactionListResult>("list_transactions");
    const row = list.items.find((t) => t.id === txId);
    expect(row).toMatchObject({
      kind: "transfer",
      amount_cents: 11_200,
      account_id: expect.any(String), // 黑洞账户 id（下一用例断名字）
      to_account_id: "mock-acc-cash",
      note: "余额调整",
      date: "2026-01-30",
    });
    expect(changed()).toBe(1);
  });

  it("调低：账户→黑洞方向，差额绝对值入账（bank 1_900_000 → 1_800_000）", async () => {
    const txId = await invoke<string>("adjust_account_balance", {
      id: "mock-acc-bank",
      input: { target_balance_cents: 1_800_000, date: "2026-01-30" },
    });

    const list = await invoke<TransactionListResult>("list_transactions");
    expect(list.items.find((t) => t.id === txId)).toMatchObject({
      kind: "transfer",
      amount_cents: 100_000,
      account_id: "mock-acc-bank",
      to_account_id: expect.any(String), // 黑洞账户 id
    });
    const balances =
      await invoke<Array<{ account: { id: string }; balance_cents: number }>>(
        "list_account_balances",
      );
    expect(balances.find((b) => b.account.id === "mock-acc-bank")?.balance_cents).toBe(1_800_000);
  });

  it("黑洞账户按币种即建且复用：两次校准的转账行引用同一黑洞账户", async () => {
    const first = await invoke<string>("adjust_account_balance", {
      id: "mock-acc-cash",
      input: { target_balance_cents: 60_000, date: "2026-01-30" },
    });
    const second = await invoke<string>("adjust_account_balance", {
      id: "mock-acc-bank",
      input: { target_balance_cents: 1_800_000, date: "2026-01-30" },
    });

    // 两笔转账的黑洞端是同一 id（即建后复用）；type 不可见（隐藏账户不进账户列表，
    // 见下一用例），经交易行引用断言
    const list =
      await invoke<Array<{ id: string; account_id: string | null; to_account_id: string | null }>>(
        "list_transactions",
      );
    const firstRow = list.items.find((t) => t.id === first);
    const secondRow = list.items.find((t) => t.id === second);
    const firstHole = firstRow?.account_id; // 调高：黑洞→cash（黑洞是转出端）
    const secondHole = secondRow?.to_account_id; // 调低：bank→黑洞（黑洞是转入端）
    expect(firstHole).toBeTruthy();
    expect(secondHole).toBe(firstHole);
  });

  it("隐藏账户不进账户面：校准即建黑洞后 list_accounts / list_account_balances 均不含它", async () => {
    await invoke<string>("adjust_account_balance", {
      id: "mock-acc-cash",
      input: { target_balance_cents: 60_000, date: "2026-01-30" },
    });

    const accounts = await invoke<Array<{ id: string; name: string }>>("list_accounts");
    expect(accounts.filter((a) => a.name === "无(CNY)")).toHaveLength(0);

    const balances = await invoke<Array<{ account: { name: string } }>>("list_account_balances");
    expect(balances.filter((b) => b.account.name === "无(CNY)")).toHaveLength(0);
  });

  it("目标等于当前余额报错（不落零额转账）", async () => {
    await expect(
      invoke<string>("adjust_account_balance", {
        id: "mock-acc-cash",
        input: { target_balance_cents: 48_800, date: "2026-01-30" }, // 人算：当前余额
      }),
    ).rejects.toThrow(/无需调整/);
  });

  it("不存在的账户以存在性校验拒绝", async () => {
    await expect(
      invoke<string>("adjust_account_balance", {
        id: "mock-acc-none",
        input: { target_balance_cents: 1, date: "2026-01-30" },
      }),
    ).rejects.toThrow(/mock-acc-none/);
  });
});

describe("余额缓存审计：audit_balance_cache（mock 余额口径即唯一口径，答无漂移）", () => {
  it("夹具种子态：5 账户全检、无漂移、不修复", async () => {
    await expect(invoke("audit_balance_cache")).resolves.toEqual({
      accounts_checked: 5,
      drifts: [],
      repaired: false,
    });
  });

  it("检查面含隐藏账户：校准即建黑洞后 checked +1", async () => {
    await invoke<string>("adjust_account_balance", {
      id: "mock-acc-cash",
      input: { target_balance_cents: 60_000, date: "2026-01-30" },
    });
    await expect(invoke("audit_balance_cache")).resolves.toMatchObject({
      accounts_checked: 6,
      drifts: [],
      repaired: false,
    });
  });
});

describe("商户关联交易计数：list_merchant_transaction_counts（毛笔数口径）", () => {
  /** 人算：京东被 005/006 引用 = 2；美团被 001/004 引用 = 2；已删商户无引用 = 0。
   * 序按名字典序：京东 < 已删商户 < 美团。 */
  it("含软删商户全字典行、无引用计 0、未删流水逐笔计 1", async () => {
    await expect(invoke("list_merchant_transaction_counts")).resolves.toEqual([
      { merchant_id: "mock-mch-jd", transaction_count: 2 },
      { merchant_id: "mock-mch-del", transaction_count: 0 },
      { merchant_id: "mock-mch-meituan", transaction_count: 2 },
    ]);
  });

  it("计数随写入实时推导：记一笔美团支出后美团 +1", async () => {
    await invoke<string>("create_transaction", {
      input: {
        kind: "expense",
        amount_cents: 1_000,
        currency_code: "CNY",
        account_id: "mock-acc-cash",
        merchant_id: "mock-mch-meituan",
        date: "2026-02-10",
      },
    });
    const counts = await invoke<Array<{ merchant_id: string; transaction_count: number }>>(
      "list_merchant_transaction_counts",
    );
    expect(counts.find((c) => c.merchant_id === "mock-mch-meituan")?.transaction_count).toBe(3);
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
    expect((await invoke<TransactionListResult>("list_transactions")).total).toBe(7);

    installMockBackend();

    expect((await invoke<TransactionListResult>("list_transactions")).total).toBe(6);
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
