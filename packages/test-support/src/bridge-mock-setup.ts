import { beforeEach, vi } from "vitest";
import { invoke } from "@ledger/mock-backend/bridge/command";
import { getCurrentWindow } from "@ledger/mock-backend/bridge/window";
import { mockInvoke, unexpectedInvoke } from "./invoke-mock";
import { mockListen } from "./listen-mock";
import { mockOnBackButtonPress } from "./back-mock";

// 全局桥替身装配（issue #746 / ADR-0085；#2083 切轨后传输面 =
// @ledger/mock-backend/bridge/*）：按模块整体替换四件桥面，app 与 packages-dom
// 两个测试工程挂载本文件（vitest.config.ts setupFiles）；packages 工程不挂——
// mock-backend 包内测试要行使真实替身实现（全局模块 mock 会把它们打成空壳），
// 纯逻辑包不触桥面。

// Mock invoke - 所有测试共享同一 mock
vi.mock("@ledger/mock-backend/bridge/command", () => ({
  invoke: vi.fn(),
}));

// Mock 事件监听
vi.mock("@ledger/mock-backend/bridge/events", () => ({
  listen: vi.fn().mockResolvedValue(vi.fn()),
}));

// Mock 系统返回桥（issue #845）：注册/撤销与窗口销毁经同目录 back-mock 助手装配；
// 默认实现为「注册即成功、销毁为 no-op」。
vi.mock("@ledger/mock-backend/bridge/app", () => ({
  onBackButtonPress: vi.fn(),
}));
vi.mock("@ledger/mock-backend/bridge/window", () => ({
  getCurrentWindow: vi.fn(),
}));

beforeEach(() => {
  mockInvoke.mockReset();
  mockInvoke.mockImplementation(unexpectedInvoke as typeof invoke);
  mockListen.mockReset();
  mockListen.mockResolvedValue(vi.fn());
  mockOnBackButtonPress.mockReset();
  mockOnBackButtonPress.mockResolvedValue({ unregister: () => Promise.resolve() });
  vi.mocked(getCurrentWindow).mockReset();
});
