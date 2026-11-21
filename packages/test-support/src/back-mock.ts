import { onBackButtonPress } from "@tauri-apps/api/app";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { vi, type Mock } from "vitest";

export type BackPressHandler = (payload?: { canGoBack?: boolean }) => void;

type RegisterBackHandler = (
  handler: BackPressHandler,
) => Promise<{ unregister: () => Promise<void> }>;

export const mockOnBackButtonPress = vi.mocked(
  onBackButtonPress,
) as unknown as Mock<RegisterBackHandler>;

export function captureBackHandler(): (payload?: { canGoBack?: boolean }) => void {
  let handler: BackPressHandler | null = null;
  mockOnBackButtonPress.mockImplementation((h) => {
    handler = h;
    return Promise.resolve({ unregister: () => Promise.resolve() });
  });
  return (payload) => handler?.({ canGoBack: false, ...payload });
}

export function captureBackRegistration(): () => {
  handler: BackPressHandler;
  unregister: Mock;
} | null {
  let captured: { handler: BackPressHandler; unregister: Mock } | null = null;
  mockOnBackButtonPress.mockImplementation((h) => {
    const unregister = vi.fn().mockResolvedValue(undefined);
    captured = { handler: h, unregister };
    return Promise.resolve({ unregister: () => unregister() });
  });
  return () => captured;
}

export function mockWindowDestroy(): Mock<() => Promise<void>> {
  const destroy = vi.fn().mockResolvedValue(undefined);
  vi.mocked(getCurrentWindow).mockReturnValue({ destroy } as never);
  return destroy;
}
