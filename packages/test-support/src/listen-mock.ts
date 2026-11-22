import { listen } from "@tauri-apps/api/event";
import { vi, type Mock } from "vitest";

export type CapturedListener = (evt?: { event?: string; payload?: unknown }) => void;

export type AppListenHandler = (event: string, handler: CapturedListener) => Promise<() => void>;

export const mockListen = vi.mocked(listen) as unknown as Mock<AppListenHandler>;

export function captureListenHandlers(): CapturedListener[] {
  const handlers: CapturedListener[] = [];
  mockListen.mockImplementation((_event, handler) => {
    handlers.push(handler);
    return Promise.resolve(() => {});
  });
  return handlers;
}

export function captureLastListener(): () => CapturedListener | null {
  let handler: CapturedListener | null = null;
  mockListen.mockImplementation((_event, h) => {
    handler = h;
    return Promise.resolve(() => {});
  });
  return () => handler;
}
