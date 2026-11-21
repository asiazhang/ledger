import { vi } from "vitest";

export const messageApi = {
  success: vi.fn(),
  warning: vi.fn(),
  error: vi.fn(),
  info: vi.fn(),
  loading: vi.fn(),
  destroyAll: vi.fn(),
};

export function resetMessageApi(): void {
  messageApi.success.mockReset();
  messageApi.warning.mockReset();
  messageApi.error.mockReset();
  messageApi.info.mockReset();
  messageApi.loading.mockReset();
  messageApi.destroyAll.mockReset();
}

export function messageCalls(): Array<{ method: string; text: string }> {
  return (["success", "warning", "error", "info", "loading"] as const).flatMap((method) =>
    messageApi[method].mock.calls.map((c) => ({ method, text: String(c[0]) })),
  );
}
