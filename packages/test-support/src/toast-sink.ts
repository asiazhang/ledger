import { vi, type Mock } from "vitest";

export interface ToastSink {
  error: (content: string) => void;
}

export function makeFakeSink(): ToastSink & { error: Mock<(content: string) => void> } {
  return { error: vi.fn<(content: string) => void>() };
}

export function resetToastSink(register: (sink: ToastSink) => void): void {
  register({ error: () => {} });
}
