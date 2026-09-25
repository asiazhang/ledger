import { describe, it, expect } from "vitest";
import { setActivePinia, createPinia } from "pinia";
import { mockInvoke } from "@ledger/test-support/invoke-mock";
import {
  parseHideInvestmentRelated,
  useTransactionViewPreferencesStore,
} from "@/transaction/transaction-view-preferences";
import {
  VIEW_STATE_KEYS,
  getSavedHideInvestmentRelated,
  saveSidebarOrders,
} from "@ledger/utils/view-state";

// 交易页视图偏好 store 接口测试（issue #1811 / ADR-0136 决策 1）。
// 偏好是设备级持久裁决（「视图偏好」词条）：具名 Pinia store 是唯一读写方，
// localStorage（ViewState 家族）承载、不入账本 SQLite、不进备份、不参与多端同步。
// 「重启」惯用法 = setActivePinia(createPinia())：store 首次实例化即启动读路径
// （读 view_state:hide_investment_related 经解析防御），新 pinia = 新一次启动。

/** 重启：换新 pinia，下一次 useTransactionViewPreferencesStore() 即新一次启动读路径 */
function reboot() {
  setActivePinia(createPinia());
}

describe("parseHideInvestmentRelated 解析防御（布尔偏好：脏值整体回默认关，ADR-0136）", () => {
  it.each([null, undefined, "true", "false", 1, 0, {}, { value: true }, [], "on"])(
    "脏值 %p 回默认关",
    (raw) => {
      expect(parseHideInvestmentRelated(raw)).toBe(false);
    },
  );

  it("仅布尔值合法：true 为开，false 为关", () => {
    expect(parseHideInvestmentRelated(true)).toBe(true);
    expect(parseHideInvestmentRelated(false)).toBe(false);
  });
});

describe("store 读路径：默认关 + 跨启动保持（issue #1811 验收判据）", () => {
  it("无记录启动：默认关", () => {
    reboot();
    expect(useTransactionViewPreferencesStore().hideInvestmentRelated).toBe(false);
  });

  it("存储脏值启动：整体回默认关，不产生非法状态", () => {
    for (const dirty of ['"true"', "1", "null", '{"v":true}']) {
      localStorage.setItem(VIEW_STATE_KEYS.hideInvestmentRelated, dirty);
      reboot();
      expect(useTransactionViewPreferencesStore().hideInvestmentRelated).toBe(false);
    }
  });

  it("开启后重启仍在：偏好跨启动保持（仅本机设备，无记录才回默认关）", () => {
    reboot();
    useTransactionViewPreferencesStore().setHideInvestmentRelated(true);
    reboot();
    expect(useTransactionViewPreferencesStore().hideInvestmentRelated).toBe(true);
    expect(getSavedHideInvestmentRelated()).toBe(true);
  });
});

describe("store 写路径：点选即写 + 默认态无记录（ADR-0116 家族先例同构）", () => {
  it("开启即写：内存态与 localStorage 同步", () => {
    reboot();
    useTransactionViewPreferencesStore().setHideInvestmentRelated(true);
    expect(useTransactionViewPreferencesStore().hideInvestmentRelated).toBe(true);
    expect(localStorage.getItem(VIEW_STATE_KEYS.hideInvestmentRelated)).toBe("true");
  });

  it("关闭即删记录：默认态 = 无记录，下次启动回默认关", () => {
    reboot();
    const store = useTransactionViewPreferencesStore();
    store.setHideInvestmentRelated(true);
    store.setHideInvestmentRelated(false);
    expect(store.hideInvestmentRelated).toBe(false);
    expect(localStorage.getItem(VIEW_STATE_KEYS.hideInvestmentRelated)).toBeNull();
  });

  it("同值重复设置 no-op：目标态与当前态相同不产生存储写入", () => {
    reboot();
    const store = useTransactionViewPreferencesStore();
    store.setHideInvestmentRelated(true);
    store.setHideInvestmentRelated(true);
    expect(localStorage.getItem(VIEW_STATE_KEYS.hideInvestmentRelated)).toBe("true");
    store.setHideInvestmentRelated(false);
    store.setHideInvestmentRelated(false);
    expect(localStorage.getItem(VIEW_STATE_KEYS.hideInvestmentRelated)).toBeNull();
  });
});

describe("边界：零后端调用、不触碰其他 ViewState 存储（不入 SQLite / 备份 / 同步）", () => {
  it("读 / 写偏好不调用任何后端命令（不入账本 SQLite、不参与多端同步）", () => {
    reboot();
    const store = useTransactionViewPreferencesStore();
    store.setHideInvestmentRelated(true);
    expect(store.hideInvestmentRelated).toBe(true);
    store.setHideInvestmentRelated(false);
    expect(store.hideInvestmentRelated).toBe(false);
    expect(mockInvoke).not.toHaveBeenCalled();
  });

  it("写偏好不改写其他视图状态存储（同族隔离）", () => {
    const orders = { bookkeeping: ["transactions", "accounts", "budget"] };
    saveSidebarOrders(orders);
    reboot();
    useTransactionViewPreferencesStore().setHideInvestmentRelated(true);
    expect(JSON.parse(localStorage.getItem(VIEW_STATE_KEYS.sidebarOrder) as string)).toEqual(
      orders,
    );
  });
});
