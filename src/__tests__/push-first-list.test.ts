import { describe, it, expect, beforeEach } from "vitest";
import { deferred } from "@ledger/test-support/deferred";
import {
  captureListenHandlers,
  mockListen,
  type CapturedListener,
} from "@ledger/test-support/listen-mock";
import { flushPromises } from "@vue/test-utils";
import { ref } from "vue";
import { createPushFirstList } from "@/composables/push-first-list";

let handlers: CapturedListener[];

beforeEach(() => {
  handlers = captureListenHandlers();
});

describe("createPushFirstList（push-first 清单生命周期工厂，ADR-0123）", () => {
  it("self-init：创建即同步置 loading 并发起一次加载，成功后 apply 落位、version=1、status=ready", async () => {
    const first = deferred<string[]>();
    const sink = ref<string[]>([]);
    let loadCalls = 0;
    const list = createPushFirstList(
      () => {
        loadCalls++;
        return first.promise;
      },
      (snapshot) => {
        sink.value = snapshot;
      },
    );

    expect(loadCalls).toBe(1);
    expect(list.status.value).toBe("loading");
    expect(list.version.value).toBe(0);
    expect(sink.value).toEqual([]);

    first.resolve(["a"]);
    await flushPromises();
    expect(sink.value).toEqual(["a"]);
    expect(list.version.value).toBe(1);
    expect(list.status.value).toBe("ready");
  });

  it("SWR 整体替换：重拉在途期间旧快照保留（不二次 apply），成功后整体替换并 version 自增", async () => {
    const first = deferred<string[]>();
    const second = deferred<string[]>();
    const sink = ref<string[]>(["seed"]);
    const applied: string[][] = [];
    let loadCalls = 0;
    const list = createPushFirstList<string[]>(
      () => {
        loadCalls++;
        return loadCalls === 1 ? first.promise : second.promise;
      },
      (snapshot) => {
        applied.push(snapshot);
        sink.value = snapshot;
      },
    );
    first.resolve(["v1"]);
    await flushPromises();
    expect(sink.value).toEqual(["v1"]);

    const reloading = list.refresh();
    expect(list.status.value).toBe("loading");
    second.resolve(["v2"]);
    await reloading;
    expect(applied).toEqual([["v1"], ["v2"]]);
    expect(sink.value).toEqual(["v2"]);
    expect(list.version.value).toBe(2);
    expect(list.status.value).toBe("ready");
  });

  it("失败语义：load 拒绝 → status=error、不落位、version 不变，refresh 向调用方上抛", async () => {
    const first = deferred<string[]>();
    const sink = ref<string[]>([]);
    const list = createPushFirstList(
      () => first.promise,
      (snapshot) => {
        sink.value = snapshot;
      },
    );
    first.reject(new Error("boom"));
    await flushPromises();
    expect(list.status.value).toBe("error");
    expect(sink.value).toEqual([]);
    expect(list.version.value).toBe(0);

    const failing = deferred<string[]>();
    let loadCalls = 0;
    const again = createPushFirstList<string[]>(
      () => {
        loadCalls++;
        return loadCalls === 1 ? Promise.resolve(["x"]) : failing.promise;
      },
      () => {},
    );
    await flushPromises();
    failing.reject(new Error("boom"));
    await expect(again.refresh()).rejects.toThrow("boom");
    expect(again.status.value).toBe("error");
  });

  it("失败后 refresh 可恢复：error → loading → ready，version 续增", async () => {
    let loadCalls = 0;
    const sink = ref<string[]>([]);
    const list = createPushFirstList<string[]>(
      () => {
        loadCalls++;
        return loadCalls === 1 ? Promise.reject(new Error("boom")) : Promise.resolve(["ok"]);
      },
      (snapshot) => {
        sink.value = snapshot;
      },
    );
    await flushPromises();
    expect(list.status.value).toBe("error");

    await list.refresh();
    expect(list.status.value).toBe("ready");
    expect(sink.value).toEqual(["ok"]);
    expect(list.version.value).toBe(1);
  });

  it("部分失败不落位：快照内任一子加载失败 → 整体失败、apply 未被调用", async () => {
    const ok = deferred<string[]>();
    const bad = deferred<number>();
    const sink = ref<{ a: string[]; b: number } | null>(null);
    const list = createPushFirstList(
      () => Promise.all([ok.promise, bad.promise]).then(([a, b]) => ({ a, b })),
      (snapshot) => {
        sink.value = snapshot;
      },
    );
    ok.resolve(["a"]);
    bad.reject(new Error("stats boom"));
    await flushPromises();
    expect(list.status.value).toBe("error");
    expect(sink.value).toBeNull();
    expect(list.version.value).toBe(0);
  });

  it("在途合并：并发 refresh 共享同一次加载（load 恰一次），全部同解", async () => {
    const pending = deferred<string[]>();
    let loadCalls = 0;
    const sink = ref<string[]>([]);
    const list = createPushFirstList<string[]>(
      () => {
        loadCalls++;
        return loadCalls === 1 ? Promise.resolve(["init"]) : pending.promise;
      },
      (snapshot) => {
        sink.value = snapshot;
      },
    );
    await flushPromises();

    const p1 = list.refresh();
    const p2 = list.refresh();
    const p3 = list.refresh();
    expect(loadCalls).toBe(2);
    pending.resolve(["fresh"]);
    await Promise.all([p1, p2, p3]);
    expect(sink.value).toEqual(["fresh"]);
    expect(list.version.value).toBe(2);
  });

  it("ledger:changed 订阅与事件重拉：每实例注册一次，事件到达即静默重拉（在途期间事件被合并）", async () => {
    const sink = ref<string[]>([]);
    let loadCalls = 0;
    const holder: { pending: ReturnType<typeof deferred<string[]>> | null } = { pending: null };
    const list = createPushFirstList<string[]>(
      () => {
        loadCalls++;
        if (loadCalls === 1) return Promise.resolve(["init"]);
        holder.pending = deferred<string[]>();
        return holder.pending.promise;
      },
      (snapshot) => {
        sink.value = snapshot;
      },
    );
    await flushPromises();
    expect(sink.value).toEqual(["init"]);

    expect(mockListen).toHaveBeenCalledWith("ledger:changed", expect.any(Function));
    expect(handlers.length).toBe(1);

    handlers.forEach((h) => h({ event: "ledger:changed", payload: null }));
    await flushPromises();
    expect(loadCalls).toBe(2);
    expect(sink.value).toEqual(["init"]);
    expect(list.status.value).toBe("loading");

    handlers.forEach((h) => h({ event: "ledger:changed", payload: null }));
    expect(loadCalls).toBe(2);

    holder.pending?.resolve(["fresh"]);
    await flushPromises();
    expect(sink.value).toEqual(["fresh"]);
    expect(list.version.value).toBe(2);

    createPushFirstList<string[]>(
      () => Promise.resolve([]),
      () => {},
    );
    expect(handlers.length).toBe(2);
  });

  it("invalidate 作废在途：迟到结果不落位（不 apply、version 不变），loading 收尾（ADR-0040 invalidate 先例）", async () => {
    const first = deferred<string[]>();
    const sink = ref<string[]>([]);
    let loadCalls = 0;
    const list = createPushFirstList<string[]>(
      () => {
        loadCalls++;
        return loadCalls === 1 ? Promise.resolve(["init"]) : first.promise;
      },
      (snapshot) => {
        sink.value = snapshot;
      },
    );
    await flushPromises();
    expect(sink.value).toEqual(["init"]);

    const reloading = list.refresh();
    expect(list.status.value).toBe("loading");
    list.invalidate();
    expect(list.status.value).toBe("ready");

    first.resolve(["late"]);
    await reloading;
    expect(sink.value).toEqual(["init"]);
    expect(list.version.value).toBe(1);
    expect(list.status.value).toBe("ready");
  });

  it("invalidate 后 refresh 不合并进旧纪元在途：立即新拉，新结果正常落位", async () => {
    const stale = deferred<string[]>();
    const fresh = deferred<string[]>();
    const sink = ref<string[]>([]);
    let loadCalls = 0;
    const list = createPushFirstList<string[]>(
      () => {
        loadCalls++;
        if (loadCalls === 1) return Promise.resolve(["init"]);
        if (loadCalls === 2) return stale.promise;
        return fresh.promise;
      },
      (snapshot) => {
        sink.value = snapshot;
      },
    );
    await flushPromises();

    void list.refresh();
    expect(loadCalls).toBe(2);

    list.invalidate();
    const reloading = list.refresh();
    expect(loadCalls).toBe(3);
    expect(list.status.value).toBe("loading");

    stale.resolve(["stale"]);
    await flushPromises();
    expect(sink.value).toEqual(["init"]);
    expect(list.status.value).toBe("loading");

    fresh.resolve(["fresh"]);
    await reloading;
    expect(sink.value).toEqual(["fresh"]);
    expect(list.version.value).toBe(2);
    expect(list.status.value).toBe("ready");
  });

  it("旧纪元加载失败同样作废：不置 error、对旧调用方静默 resolve；新纪元失败照常 error", async () => {
    const stale = deferred<string[]>();
    const fresh = deferred<string[]>();
    const sink = ref<string[]>([]);
    let loadCalls = 0;
    const list = createPushFirstList<string[]>(
      () => {
        loadCalls++;
        if (loadCalls === 1) return Promise.resolve(["init"]);
        if (loadCalls === 2) return stale.promise;
        return fresh.promise;
      },
      (snapshot) => {
        sink.value = snapshot;
      },
    );
    await flushPromises();

    void list.refresh();
    list.invalidate();
    const reloading = list.refresh();

    stale.reject(new Error("stale boom"));
    await flushPromises();
    expect(list.status.value).toBe("loading");
    expect(sink.value).toEqual(["init"]);

    fresh.reject(new Error("fresh boom"));
    await expect(reloading).rejects.toThrow("fresh boom");
    expect(list.status.value).toBe("error");
    expect(list.version.value).toBe(1);
  });

  it("invalidate 的 loading 收尾：初载在途（无成功快照）时回 idle，error 不动", async () => {
    const first = deferred<string[]>();
    let loadCalls = 0;
    const sink = ref<string[]>([]);
    const list = createPushFirstList<string[]>(
      () => {
        loadCalls++;
        return loadCalls === 1 ? first.promise : Promise.reject(new Error("boom"));
      },
      (snapshot) => {
        sink.value = snapshot;
      },
    );
    list.invalidate();
    expect(list.status.value).toBe("idle");
    first.resolve(["late"]);
    await flushPromises();
    expect(sink.value).toEqual([]);

    await list.refresh().catch(() => {});
    expect(list.status.value).toBe("error");
    list.invalidate();
    expect(list.status.value).toBe("error");
  });
});
