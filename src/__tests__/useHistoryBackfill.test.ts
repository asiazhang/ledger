import { describe, it, expect, beforeEach } from "vitest";
import {
  captureListenHandlers,
  mockListen,
  type CapturedListener,
} from "@ledger/test-support/listen-mock";
import {
  HISTORY_BACKFILL_PROGRESS_EVENT,
  resetHistoryBackfillForTest,
  useHistoryBackfill,
} from "@/investment/useHistoryBackfill";

function fire(handlers: CapturedListener[], payload: unknown): void {
  for (const handler of handlers) handler({ event: HISTORY_BACKFILL_PROGRESS_EVENT, payload });
}

describe("useHistoryBackfill 价格历史后台补全（静默计数接缝）", () => {
  let handlers: CapturedListener[];

  beforeEach(() => {
    mockListen.mockReset();
    resetHistoryBackfillForTest();
    handlers = captureListenHandlers();
  });

  it("订阅独立事件名：后台补全事件与手动同步进度事件互不串台", () => {
    useHistoryBackfill();
    expect(mockListen).toHaveBeenCalledWith(HISTORY_BACKFILL_PROGRESS_EVENT, expect.any(Function));
    expect(HISTORY_BACKFILL_PROGRESS_EVENT).toBe("ledger:history-backfill-progress");
    expect(HISTORY_BACKFILL_PROGRESS_EVENT).not.toBe("ledger:instrument-sync-progress");
  });

  it("进度事件更新计数：done < total 期间持续展示", () => {
    const { progress } = useHistoryBackfill();
    expect(progress.value).toBeNull();

    fire(handlers, { done: 0, total: 228 });
    expect(progress.value).toEqual({ done: 0, total: 228 });

    fire(handlers, { done: 137, total: 228 });
    expect(progress.value).toEqual({ done: 137, total: 228 });
  });

  it("终态静默收起：done ≥ total 计数归空，不残留完成态", () => {
    const { progress } = useHistoryBackfill();
    fire(handlers, { done: 227, total: 228 });
    expect(progress.value).toEqual({ done: 227, total: 228 });

    fire(handlers, { done: 228, total: 228 });
    expect(progress.value).toBeNull();
  });

  it("下一轮（新自然日窗口）从 done=0 重新点亮", () => {
    const { progress } = useHistoryBackfill();
    fire(handlers, { done: 5, total: 5 });
    expect(progress.value).toBeNull();
    fire(handlers, { done: 0, total: 3 });
    expect(progress.value).toEqual({ done: 0, total: 3 });
  });

  it("事件恒为标的级两字段：多余字段不影响计数（原页级明细已随换源退役）", () => {
    const { progress } = useHistoryBackfill();
    fire(handlers, {
      done: 3,
      total: 228,
      fund: { code: "110022", page: 3, pages: 25 },
    } as never);
    expect(progress.value).toEqual({ done: 3, total: 228 });
    fire(handlers, { done: 4, total: 228 });
    expect(progress.value).toEqual({ done: 4, total: 228 });
  });

  it("载荷形状异常（脏数据/NaN）整体忽略", () => {
    const { progress } = useHistoryBackfill();
    fire(handlers, { done: Number.NaN, total: 228 });
    fire(handlers, { done: "3" });
    fire(handlers, undefined);
    expect(progress.value).toBeNull();
  });
});
