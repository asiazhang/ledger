import { describe, it, expect, beforeEach } from "vitest";
import { flushPromises } from "@vue/test-utils";
import { mockInvoke } from "@ledger/test-support/invoke-mock";
import { captureListenHandlers, type CapturedListener } from "@ledger/test-support/listen-mock";
import { FX_SYNC_PROGRESS_EVENT, resetFxSyncForTest, useFxSync } from "@/settings/useFxSync";

/** 命令契约静态快照：一次正常落库报告（与 ExchangeRateSyncSettings.test.ts 同形）。 */
const REPORT = {
  full_backfilled: true,
  persist: {
    pairs: 10,
    points: 30,
    earliest: "2026-06-27",
    latest: "2026-09-18",
    manual_protected: 0,
  },
};

let progressHandlers: CapturedListener[] = [];

beforeEach(() => {
  resetFxSyncForTest();
  progressHandlers = captureListenHandlers();
});

/** 触发最近捕获的阶段事件监听器（tauri Event 载荷形状）。 */
function fireProgress(payload: unknown): void {
  expect(progressHandlers.length).toBeGreaterThan(0);
  progressHandlers.at(-1)!({ event: FX_SYNC_PROGRESS_EVENT, payload });
}

describe("useFxSync 汇率手动同步（设置页共用接缝，模块级单例）", () => {
  it("成功：sync 回报告，syncing 收尾，报告就位", async () => {
    mockInvoke.mockResolvedValue(REPORT);
    const { syncing, report, syncError, sync } = useFxSync();
    await expect(sync()).resolves.toEqual(REPORT);
    expect(syncing.value).toBe(false);
    expect(report.value).toEqual(REPORT);
    expect(syncError.value).toBeNull();
  });

  it("失败：sync 回空，错误按码本地化（数据源不可达）", async () => {
    mockInvoke.mockRejectedValue({
      kind: "Invalid",
      code: "fx.source-unreachable",
      message: "后端原文（与模板不同文）：连接超时",
    });
    const { syncing, report, syncError, sync } = useFxSync();
    await expect(sync()).resolves.toBeNull();
    expect(syncing.value).toBe(false);
    expect(report.value).toBeNull();
    expect(syncError.value).toContain("汇率数据源暂时不可达");
    expect(syncError.value).not.toContain("连接超时");
  });

  it("撞车：在途互斥码经 Loadable 就地呈现「已有汇率同步在进行」", async () => {
    mockInvoke.mockRejectedValue({
      kind: "Invalid",
      code: "fx.sync-in-progress",
      message: "后端原文（与模板不同文）：busy",
    });
    const { syncError, sync } = useFxSync();
    await expect(sync()).resolves.toBeNull();
    expect(syncError.value).toContain("已有汇率同步在进行");
    expect(syncError.value).not.toContain("busy");
  });

  it("新一次同步开始时清空旧报告（上次的结果不残留）", async () => {
    mockInvoke.mockResolvedValueOnce(REPORT);
    const { report, sync } = useFxSync();
    await sync();
    expect(report.value).toEqual(REPORT);

    let resolveSync!: (v: unknown) => void;
    mockInvoke.mockImplementationOnce(
      () =>
        new Promise((res) => {
          resolveSync = res as (v: unknown) => void;
        }),
    );
    const pending = sync();
    expect(report.value).toBeNull();
    resolveSync(REPORT);
    await pending;
    expect(report.value).toEqual(REPORT);
  });
});

describe("useFxSync 阶段文字（issue #1762）", () => {
  it("订阅后端阶段事件 ledger:fx-sync-progress（事件名常量单点）", () => {
    useFxSync();
    expect(FX_SYNC_PROGRESS_EVENT).toBe("ledger:fx-sync-progress");
    expect(progressHandlers.length).toBeGreaterThan(0);
  });

  it("在途期间消费阶段事件：读取 → 携带天数的写入", async () => {
    let resolveSync!: (v: unknown) => void;
    mockInvoke.mockImplementation(
      () =>
        new Promise((res) => {
          resolveSync = res as (v: unknown) => void;
        }),
    );
    const { stageText, sync } = useFxSync();
    const p = sync();
    expect(stageText.value).toBe("");

    fireProgress({ stage: "fetching", days_parsed: null });
    await flushPromises();
    expect(stageText.value).toContain("正在读取汇率文件");

    fireProgress({ stage: "persisting", days_parsed: 21 });
    await flushPromises();
    expect(stageText.value).toContain("21");
    expect(stageText.value).toContain("正在写入");

    resolveSync(REPORT);
    await p;
  });

  it("成功终态收起阶段文字，迟到事件不再复活", async () => {
    let resolveSync!: (v: unknown) => void;
    mockInvoke.mockImplementation(
      () =>
        new Promise((res) => {
          resolveSync = res as (v: unknown) => void;
        }),
    );
    const { stageText, report, sync } = useFxSync();
    const p = sync();
    fireProgress({ stage: "fetching", days_parsed: null });
    await flushPromises();
    expect(stageText.value).not.toBe("");

    resolveSync(REPORT);
    await p;
    expect(stageText.value).toBe("");
    expect(report.value).toEqual(REPORT);

    fireProgress({ stage: "persisting", days_parsed: 21 });
    await flushPromises();
    expect(stageText.value).toBe("");
  });

  it("失败终态同样收起阶段文字", async () => {
    let rejectSync!: (e: unknown) => void;
    mockInvoke.mockImplementation(
      () =>
        new Promise((_, rej) => {
          rejectSync = rej;
        }),
    );
    const { stageText, syncError, sync } = useFxSync();
    const p = sync();
    fireProgress({ stage: "fetching", days_parsed: null });
    await flushPromises();
    expect(stageText.value).not.toBe("");

    rejectSync(new Error("网络错误"));
    await p;
    expect(stageText.value).toBe("");
    expect(syncError.value).not.toBeNull();
  });

  it("无在途同步时阶段事件被忽略", async () => {
    const { stageText } = useFxSync();
    fireProgress({ stage: "fetching", days_parsed: null });
    await flushPromises();
    expect(stageText.value).toBe("");
  });

  it("载荷形状异常的阶段事件被忽略（防脏 payload 渲染）", async () => {
    let resolveSync!: (v: unknown) => void;
    mockInvoke.mockImplementation(
      () =>
        new Promise((res) => {
          resolveSync = res as (v: unknown) => void;
        }),
    );
    const { stageText, stage, parsedDays, sync } = useFxSync();
    const p = sync();
    fireProgress(undefined);
    fireProgress({ stage: "downloading", days_parsed: 3 });
    fireProgress({ stage: "persisting" });
    fireProgress({ stage: "persisting", days_parsed: Number.NaN });
    await flushPromises();
    expect(stage.value).toBeNull();
    expect(parsedDays.value).toBeNull();
    expect(stageText.value).toBe("");
    resolveSync(REPORT);
    await p;
  });

  it("重叠触发时先完成者不擦掉仍在途那路的阶段文字", async () => {
    const resolvers: Array<(v: unknown) => void> = [];
    mockInvoke.mockImplementation(
      () =>
        new Promise((res) => {
          resolvers.push(res as (v: unknown) => void);
        }),
    );
    const { stageText, syncing, sync } = useFxSync();
    const p1 = sync();
    const p2 = sync();
    expect(mockInvoke).toHaveBeenCalledTimes(2);

    fireProgress({ stage: "fetching", days_parsed: null });
    await flushPromises();
    expect(stageText.value).toContain("正在读取汇率文件");

    // 首路先完成（Loadable 最新胜出下作废）：不得擦掉仍在途那路的阶段文字
    resolvers[0](REPORT);
    await p1;
    expect(syncing.value).toBe(true);
    expect(stageText.value).toContain("正在读取汇率文件");

    fireProgress({ stage: "persisting", days_parsed: 21 });
    await flushPromises();
    expect(stageText.value).toContain("正在写入");

    resolvers[1](REPORT);
    await p2;
    expect(syncing.value).toBe(false);
    expect(stageText.value).toBe("");
  });

  it("切页模拟：重挂后的实例看到同一份在途与终态（syncing/阶段/报告零分叉）", async () => {
    let resolveSync!: (v: unknown) => void;
    mockInvoke.mockImplementation(
      () =>
        new Promise((res) => {
          resolveSync = res as (v: unknown) => void;
        }),
    );
    const first = useFxSync();
    const p = first.sync();

    // 切走再切回：新实例（重挂）看到同一份在途状态
    const second = useFxSync();
    expect(second.syncing.value).toBe(true);

    fireProgress({ stage: "fetching", days_parsed: null });
    await flushPromises();
    expect(second.stageText.value).toContain("正在读取汇率文件");
    expect(first.stageText.value).toBe(second.stageText.value);

    fireProgress({ stage: "persisting", days_parsed: 21 });
    await flushPromises();
    expect(second.stageText.value).toContain("21");

    resolveSync(REPORT);
    await p;
    expect(first.syncing.value).toBe(false);
    expect(second.syncing.value).toBe(false);
    expect(second.report.value).toEqual(REPORT);
    expect(second.stageText.value).toBe("");
  });
});
