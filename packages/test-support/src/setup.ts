import { vi, beforeEach, afterEach } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { enableAutoUnmount } from "@vue/test-utils";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { setFileScope } from "@vanilla-extract/css/fileScope";
import { mockInvoke, unexpectedInvoke } from "./invoke-mock";
import { mockListen } from "./listen-mock";
import { mockOnBackButtonPress } from "./back-mock";
import { fakeMatchMedia, resetFakeMedia } from "./media-mock";
import { messageApi, resetMessageApi } from "./message-mock";

setFileScope("@ledger/test-support/setup.ts");

if (typeof localStorage === "undefined" || localStorage === null) {
  const store: Record<string, string> = {};
  const mockStorage = {
    getItem: (key: string) => store[key] ?? null,
    setItem: (key: string, value: string) => {
      store[key] = value;
    },
    removeItem: (key: string) => {
      delete store[key];
    },
    clear: () => {
      Object.keys(store).forEach((k) => delete store[k]);
    },
    get length() {
      return Object.keys(store).length;
    },
    key: (index: number) => Object.keys(store)[index] ?? null,
  };
  Object.defineProperty(globalThis, "localStorage", { value: mockStorage });
}

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn().mockResolvedValue(vi.fn()),
}));

vi.mock("@tauri-apps/api/app", () => ({
  onBackButtonPress: vi.fn(),
}));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: vi.fn(),
}));

if (typeof window !== "undefined" && !window.matchMedia) {
  Object.defineProperty(window, "matchMedia", {
    writable: true,
    value: fakeMatchMedia,
  });
}

vi.mock("@vue/test-utils", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@vue/test-utils")>();
  let registered = false;
  return {
    ...actual,
    enableAutoUnmount: (hook: (cb: () => void) => void) => {
      if (registered) return;
      registered = true;
      actual.enableAutoUnmount(hook);
    },
  };
});

vi.mock("naive-ui", async (importOriginal) => {
  const actual = await importOriginal<typeof import("naive-ui")>();
  return {
    ...actual,
    useMessage: () => messageApi,
  };
});

beforeEach(() => {
  mockInvoke.mockReset();
  mockInvoke.mockImplementation(unexpectedInvoke as typeof invoke);
  mockListen.mockReset();
  mockListen.mockResolvedValue(vi.fn());
  mockOnBackButtonPress.mockReset();
  mockOnBackButtonPress.mockResolvedValue({ unregister: () => Promise.resolve() });
  vi.mocked(getCurrentWindow).mockReset();
  setActivePinia(createPinia());
  localStorage.clear();
  resetMessageApi();
  resetFakeMedia();
});

enableAutoUnmount(afterEach);

afterEach(() => {
  if (typeof document !== "undefined") document.body.innerHTML = "";
});
