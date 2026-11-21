import { ref } from "vue";
import type { Ref } from "vue";
import { listen } from "@tauri-apps/api/event";
import { createLatestWins, type LatestWinsToken } from "@ledger/latest-wins";

type PushFirstListStatus = "idle" | "loading" | "ready" | "error";

export interface PushFirstList {
  status: Ref<PushFirstListStatus>;
  version: Ref<number>;
  refresh(): Promise<void>;
  invalidate(): void;
}

export function createPushFirstList<T>(
  load: () => Promise<T>,
  apply: (snapshot: T) => void,
): PushFirstList {
  const status = ref<PushFirstListStatus>("idle");
  const version = ref(0);

  let inFlight: Promise<void> | null = null;
  const wins = createLatestWins();

  async function reload(myToken: LatestWinsToken): Promise<void> {
    status.value = "loading";
    try {
      const snapshot = await load();
      if (myToken.isStale()) return;
      apply(snapshot);
      version.value += 1;
      status.value = "ready";
    } catch (e) {
      if (myToken.isStale()) return;
      status.value = "error";
      throw e;
    }
  }

  function refresh(): Promise<void> {
    if (inFlight) return inFlight;
    const myToken = wins.observe();
    const pending = reload(myToken).finally(() => {
      if (inFlight === pending) inFlight = null;
    });
    inFlight = pending;
    return pending;
  }

  function invalidate(): void {
    wins.invalidate();
    inFlight = null;
    if (status.value === "loading") {
      status.value = version.value > 0 ? "ready" : "idle";
    }
  }

  void refresh().catch(() => {});

  listen("ledger:changed", () => {
    void refresh().catch(() => {});
  }).catch(() => {});

  return { status, version, refresh, invalidate };
}
