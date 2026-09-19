import { describe, it, expect, beforeEach } from "vitest";
import { deferred } from "@ledger/test-support/deferred";
import { wireInvokeSeam } from "@ledger/test-support/invoke-mock";
import { captureListenHandlers, type CapturedListener } from "@ledger/test-support/listen-mock";
import { flushPromises } from "@vue/test-utils";
import { usePhysicalAssetsStore } from "@/physical-asset/physicalAssets";
import { makePhysicalAsset, makePhysicalAssetList } from "./factories";
import type {
  PhysicalAsset,
  PhysicalAssetDisposeInput,
  PhysicalAssetInput,
  PhysicalAssetList,
  PhysicalAssetUpdateInput,
  PhysicalAssetValuationInput,
} from "@ledger/types";

function baseAsset(over: Partial<PhysicalAsset> = {}): PhysicalAsset {
  return makePhysicalAsset({ id: "asset-1", ...over });
}

const createInput: PhysicalAssetInput = {
  name: "代步车",
  purchase_date: "2023-05-01",
  purchase_price_cents: 12_000_000_00,
  purchase_currency_code: "CNY",
  initial_valuation_cents: 8_000_000_00,
  initial_valuation_currency_code: "CNY",
  initial_valuation_date: null,
};

/** 捕获 ledger:changed 监听处理器（store 创建时注册） */
let handlers: CapturedListener[];

beforeEach(() => {
  handlers = captureListenHandlers();
});

/**
 * 机制断言（self-init / SWR / 在途合并 / 事件重拉 / status-version）已收口到
 * push-first-list.test.ts 工厂单点（ADR-0123 决策 6）；本文件只留领域动作断言
 * （写命令调用、写后触发重拉、筛选参数与同源快照拆分等本店特有行为）。
 */

describe("usePhysicalAssetsStore", () => {
  it("列表与在持合计同批就位：后端同源快照拆分落位（含折算币种）", async () => {
    const asset = baseAsset();
    wireInvokeSeam({
      defaults: {
        list_physical_assets: makePhysicalAssetList({
          assets: [asset],
          holding_total_native_cents: 5_000_000,
        }),
      },
    });
    const store = usePhysicalAssetsStore();
    await flushPromises();
    expect(store.assets).toHaveLength(1);
    expect(store.assets[0].name).toBe("客厅油画");
    expect(store.holdingTotalNativeCents).toBe(5_000_000);
    expect(store.nativeCurrency).toBe("CNY");
  });

  it("create 成功后立即重拉并返回 id（建档后列表与合计随之更新）", async () => {
    let listCalls = 0;
    wireInvokeSeam({
      overrides: {
        list_physical_assets: () => {
          listCalls++;
          return Promise.resolve(
            listCalls > 1
              ? makePhysicalAssetList({
                  assets: [
                    baseAsset({
                      id: "new-1",
                      name: "代步车",
                      current_valuation_cents: 8_000_000_00,
                      current_valuation_native_cents: 8_000_000_00,
                    }),
                  ],
                  holding_total_native_cents: 8_000_000_00,
                })
              : makePhysicalAssetList(),
          );
        },
        create_physical_asset: (args) => {
          expect(args).toMatchObject({ input: createInput });
          return "new-1";
        },
      },
    });
    const store = usePhysicalAssetsStore();
    await flushPromises();
    expect(store.assets).toHaveLength(0);
    const id = await store.create(createInput);
    expect(id).toBe("new-1");
    await flushPromises();
    expect(store.assets).toHaveLength(1);
    expect(store.holdingTotalNativeCents).toBe(8_000_000_00);
  });

  it("create 失败不重拉、错误上抛（由调用方 toast 展示）", async () => {
    wireInvokeSeam({
      defaults: { list_physical_assets: makePhysicalAssetList() },
      overrides: {
        create_physical_asset: () => Promise.reject(new Error("资产名称不能为空")),
      },
    });
    const store = usePhysicalAssetsStore();
    await flushPromises();
    await expect(store.create({ ...createInput, name: "" })).rejects.toThrow("资产名称不能为空");
    expect(store.version).toBe(1);
  });

  it("updateValuation 成功后立即重拉（当前估值变为最新一条，issue #467 T2）", async () => {
    const valuationInput: PhysicalAssetValuationInput = {
      amount_cents: 6_000_000_00,
      currency_code: "CNY",
      valuation_date: null,
    };
    let listCalls = 0;
    wireInvokeSeam({
      overrides: {
        list_physical_assets: () => {
          listCalls++;
          return Promise.resolve(
            listCalls > 1
              ? makePhysicalAssetList({
                  assets: [
                    baseAsset({
                      current_valuation_cents: 6_000_000_00,
                      current_valuation_native_cents: 6_000_000_00,
                    }),
                  ],
                  holding_total_native_cents: 6_000_000_00,
                })
              : makePhysicalAssetList({ assets: [baseAsset()] }),
          );
        },
        update_physical_asset_valuation: (args) => {
          expect(args).toMatchObject({ id: "asset-1", input: valuationInput });
        },
      },
    });
    const store = usePhysicalAssetsStore();
    await flushPromises();
    await store.updateValuation("asset-1", valuationInput);
    await flushPromises();
    expect(store.assets[0].current_valuation_cents).toBe(6_000_000_00);
    expect(store.version).toBe(2);
  });

  it("updateValuation 失败不重拉、错误上抛（未来日期守卫由后端报错）", async () => {
    wireInvokeSeam({
      defaults: { list_physical_assets: makePhysicalAssetList({ assets: [baseAsset()] }) },
      overrides: {
        update_physical_asset_valuation: () =>
          Promise.reject(new Error("估值日期 9999-12-31 不能是未来")),
      },
    });
    const store = usePhysicalAssetsStore();
    await flushPromises();
    await expect(
      store.updateValuation("asset-1", {
        amount_cents: 1,
        currency_code: "CNY",
        valuation_date: "9999-12-31",
      }),
    ).rejects.toThrow("不能是未来");
    expect(store.version).toBe(1);
  });

  it("update 成功后立即重拉（编辑名称 / 购买信息读回一致，issue #467 T2）", async () => {
    const updateInput: PhysicalAssetUpdateInput = {
      name: "家用代步车",
      purchase_date: "2023-06-01",
      purchase_price_cents: 11_000_000_00,
      purchase_currency_code: "CNY",
    };
    let listCalls = 0;
    wireInvokeSeam({
      overrides: {
        list_physical_assets: () => {
          listCalls++;
          return Promise.resolve(
            listCalls > 1
              ? makePhysicalAssetList({ assets: [baseAsset({ name: "家用代步车" })] })
              : makePhysicalAssetList({ assets: [baseAsset({ name: "代步车" })] }),
          );
        },
        update_physical_asset: (args) => {
          expect(args).toMatchObject({ id: "asset-1", input: updateInput });
        },
      },
    });
    const store = usePhysicalAssetsStore();
    await flushPromises();
    await store.update("asset-1", updateInput);
    await flushPromises();
    expect(store.assets[0].name).toBe("家用代步车");
    expect(store.version).toBe(2);
  });

  it("update 失败不重拉、错误上抛（由调用方 toast 展示）", async () => {
    wireInvokeSeam({
      defaults: { list_physical_assets: makePhysicalAssetList({ assets: [baseAsset()] }) },
      overrides: {
        update_physical_asset: () => Promise.reject(new Error("资产名称不能为空")),
      },
    });
    const store = usePhysicalAssetsStore();
    await flushPromises();
    await expect(
      store.update("asset-1", {
        name: "",
        purchase_date: null,
        purchase_price_cents: null,
        purchase_currency_code: null,
      }),
    ).rejects.toThrow("资产名称不能为空");
    expect(store.version).toBe(1);
  });

  it("dispose 成功后立即重拉（处置流：资产退出默认列表，issue #468 T3）", async () => {
    const disposeInput: PhysicalAssetDisposeInput = {
      disposal_date: "2026-08-01",
      disposal_price_cents: 60_000_000_00,
      disposal_currency_code: "CNY",
    };
    let listCalls = 0;
    wireInvokeSeam({
      overrides: {
        list_physical_assets: () => {
          listCalls++;
          return Promise.resolve(
            listCalls > 1
              ? makePhysicalAssetList({ assets: [], holding_total_native_cents: 0 })
              : makePhysicalAssetList({ assets: [baseAsset()] }),
          );
        },
        dispose_physical_asset: (args) => {
          expect(args).toMatchObject({ id: "asset-1", input: disposeInput });
        },
      },
    });
    const store = usePhysicalAssetsStore();
    await flushPromises();
    await store.dispose("asset-1", disposeInput);
    await flushPromises();
    expect(store.assets).toHaveLength(0);
    expect(store.holdingTotalNativeCents).toBe(0);
    expect(store.version).toBe(2);
  });

  it("dispose 失败不重拉、错误上抛（缺处置日期守卫由后端报错）", async () => {
    wireInvokeSeam({
      defaults: { list_physical_assets: makePhysicalAssetList({ assets: [baseAsset()] }) },
      overrides: {
        dispose_physical_asset: () => Promise.reject(new Error("处置日期不能为空")),
      },
    });
    const store = usePhysicalAssetsStore();
    await flushPromises();
    await expect(
      store.dispose("asset-1", {
        disposal_date: null,
        disposal_price_cents: null,
        disposal_currency_code: null,
      }),
    ).rejects.toThrow("处置日期不能为空");
    expect(store.version).toBe(1);
  });

  it("remove 成功后立即重拉（软删过滤：资产退出列表与合计，issue #468 T3）", async () => {
    let listCalls = 0;
    wireInvokeSeam({
      overrides: {
        list_physical_assets: () => {
          listCalls++;
          return Promise.resolve(
            listCalls > 1
              ? makePhysicalAssetList({ assets: [], holding_total_native_cents: 0 })
              : makePhysicalAssetList({ assets: [baseAsset()] }),
          );
        },
        delete_physical_asset: (args) => {
          expect(args).toMatchObject({ id: "asset-1" });
        },
      },
    });
    const store = usePhysicalAssetsStore();
    await flushPromises();
    await store.remove("asset-1");
    await flushPromises();
    expect(store.assets).toHaveLength(0);
    expect(store.holdingTotalNativeCents).toBe(0);
    expect(store.version).toBe(2);
  });

  it("setStatusFilter 切换状态筛选并重拉（已处置筛选回看档案，默认在持）", async () => {
    const holding = [baseAsset()];
    const disposed = [
      baseAsset({
        id: "asset-9",
        name: "旧车",
        status: "disposed",
        current_valuation_native_cents: null,
      }),
    ];
    const seenStatus: string[] = [];
    wireInvokeSeam({
      overrides: {
        list_physical_assets: (args) => {
          const status = (args as { status: string | null }).status;
          seenStatus.push(status ?? "holding");
          return status === "disposed"
            ? makePhysicalAssetList({ assets: disposed, holding_total_native_cents: 5_000_000 })
            : makePhysicalAssetList({ assets: holding, holding_total_native_cents: 5_000_000 });
        },
      },
    });
    const store = usePhysicalAssetsStore();
    await flushPromises();
    expect(seenStatus[0]).toBe("holding");
    expect(store.statusFilter).toBe("holding");
    await store.setStatusFilter("disposed");
    await flushPromises();
    expect(store.statusFilter).toBe("disposed");
    expect(store.assets[0].name).toBe("旧车");
    expect(store.assets[0].status).toBe("disposed");
    // 在持合计口径与筛选无关（回看已处置时合计不变）
    expect(store.holdingTotalNativeCents).toBe(5_000_000);
    // ledger:changed 重拉沿用当前筛选，不回退默认口径
    handlers.forEach((h) => h({ event: "ledger:changed", payload: null }));
    await flushPromises();
    expect(seenStatus[seenStatus.length - 1]).toBe("disposed");
  });

  it("筛选切换落在旧筛选在途重拉期间：作废旧纪元、按新筛选新拉，旧结果不落位（issue #1381）", async () => {
    const holding = [baseAsset()];
    const disposed = [
      baseAsset({
        id: "asset-9",
        name: "旧车",
        status: "disposed",
        current_valuation_native_cents: null,
      }),
    ];
    const seenStatus: string[] = [];
    const stale = deferred<PhysicalAssetList>();
    const fresh = deferred<PhysicalAssetList>();
    let listCalls = 0;
    wireInvokeSeam({
      overrides: {
        list_physical_assets: (args) => {
          const status = (args as { status: string | null }).status;
          seenStatus.push(status ?? "holding");
          listCalls++;
          if (listCalls === 1) {
            return Promise.resolve(makePhysicalAssetList({ assets: holding }));
          }
          // ledger:changed 按旧筛选发起的重拉，保持到测试节奏才完结
          if (listCalls === 2) return stale.promise;
          return fresh.promise;
        },
      },
    });
    const store = usePhysicalAssetsStore();
    await flushPromises();

    // 旧筛选（holding）在途重拉期间切换筛选
    handlers.forEach((h) => h({ event: "ledger:changed", payload: null }));
    await flushPromises();
    expect(seenStatus).toEqual(["holding", "holding"]);

    const switching = store.setStatusFilter("disposed");
    // 作废在途：立即按新筛选新拉，不合并进旧筛选的在途
    expect(seenStatus).toEqual(["holding", "holding", "disposed"]);
    expect(store.statusFilter).toBe("disposed");

    // 旧纪元结果迟到：不落位（载荷不同于当前展示，误落位即被晾出；
    //  修复前新筛选合并进旧在途，旧结果在此落旧筛选数据）
    stale.resolve(
      makePhysicalAssetList({ assets: [baseAsset({ id: "asset-2", name: "在途期间新建的资产" })] }),
    );
    await flushPromises();
    expect(store.assets[0].id).toBe("asset-1");
    expect(store.assets[0].name).toBe("客厅油画");

    // 新纪元结果：按新筛选落位
    fresh.resolve(makePhysicalAssetList({ assets: disposed }));
    await switching;
    expect(store.assets[0].name).toBe("旧车");
    expect(store.assets[0].status).toBe("disposed");
  });

  it("写入成功后重拉失败不反转写动作成败：动作正常返回，失败信号由 status 承载（ADR-0123 决策 3）", async () => {
    let listCalls = 0;
    wireInvokeSeam({
      overrides: {
        list_physical_assets: () => {
          listCalls++;
          return listCalls === 1
            ? Promise.resolve(makePhysicalAssetList({ assets: [baseAsset()] }))
            : Promise.reject(new Error("重拉失败"));
        },
        delete_physical_asset: (args) => {
          expect(args).toMatchObject({ id: "asset-1" });
        },
      },
    });
    const store = usePhysicalAssetsStore();
    await flushPromises();

    // 已落库的删除不因重拉失败误报「删除失败」，动作正常返回
    await expect(store.remove("asset-1")).resolves.toBeUndefined();
    expect(store.status).toBe("error");
  });
});
