import type { EventCallback, UnlistenFn } from "@tauri-apps/api/event";

interface Registration<T> {
  event: string;
  handler: EventCallback<T>;
}

let nextListenerId = 1;
const registrations = new Map<number, Registration<unknown>>();

export function listen<T>(event: string, handler: EventCallback<T>): Promise<UnlistenFn> {
  const id = nextListenerId++;
  registrations.set(id, { event, handler: handler as EventCallback<unknown> });
  return Promise.resolve(() => {
    registrations.delete(id);
  });
}

export function emitMockEvent(event: string, payload?: unknown): void {
  for (const reg of Array.from(registrations.values())) {
    if (reg.event !== event) continue;
    reg.handler({ event, id: nextListenerId++, payload });
  }
}
