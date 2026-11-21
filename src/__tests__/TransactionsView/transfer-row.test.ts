import { pushMock, makeTxn, mountView, setTxnDb } from "./common";
import { describe, it, expect, beforeEach } from "vitest";
import { mount, flushPromises } from "@vue/test-utils";
import { NSelect } from "naive-ui";
import type { Transaction } from "@ledger/types";

function accountDrillButtons(wrapper: ReturnType<typeof mount>) {
  return wrapper.findAll('button[data-tx-action="account-drill"]');
}

describe("TransactionsView 转账行双向账户名（issue #99）", () => {
  const mixedDb: Transaction[] = [
    makeTxn(1, "acc-1", { kind: "expense" }),
    makeTxn(2, "acc-2", { kind: "transfer", to_account_id: "acc-1" }),
    makeTxn(3, "acc-1", { kind: "income" }),
  ];

  beforeEach(() => {
    setTxnDb([...mixedDb]);
  });

  async function filterKind(wrapper: ReturnType<typeof mount>, k: string[] | null) {
    wrapper.findAllComponents(NSelect)[2].vm.$emit("update:value", k);
    await flushPromises();
  }

  it("转账行账户列显示「转出 → 转入」双向账户名，两个名字各自可点击、各自跳转对应账户", async () => {
    const wrapper = await mountView();
    await filterKind(wrapper, ["transfer"]);
    const links = accountDrillButtons(wrapper);
    expect(links.length).toBe(2);
    expect(links.map((l) => l.text().trim())).toEqual(["银行", "现金"]);
    expect(wrapper.text()).toContain("→");
    await links[0].trigger("click");
    expect(pushMock).toHaveBeenLastCalledWith({
      name: "transactions",
      query: { account: "acc-2" },
    });
    await links[1].trigger("click");
    expect(pushMock).toHaveBeenLastCalledWith({
      name: "transactions",
      query: { account: "acc-1" },
    });
  });

  it("非转账行账户列仍显示单个主账户名（可点击，带 title 提示）", async () => {
    const wrapper = await mountView();
    await filterKind(wrapper, ["income"]);
    const links = accountDrillButtons(wrapper);
    expect(links.length).toBe(1);
    expect(links[0].text().trim()).toBe("现金");
    expect(links[0].attributes("title")).toBe("查看该账户的交易");
    await links[0].trigger("click");
    expect(pushMock).toHaveBeenLastCalledWith({
      name: "transactions",
      query: { account: "acc-1" },
    });
  });
});
