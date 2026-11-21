import type { AppBuildInfo, BaseCurrencyState, LogLevelState } from "@ledger/types";
import { db, reqBool, str } from "./state";

export function setAutoExecutionEnabled(args: Record<string, unknown>): void {
  reqBool(args, "enabled");
}

export function setAutoUpdateEnabled(args: Record<string, unknown>): void {
  reqBool(args, "enabled");
}

export function getBaseCurrency(): BaseCurrencyState {
  return { code: db().settings.base_currency };
}

export function setBaseCurrency(args: Record<string, unknown>): BaseCurrencyState {
  const code = str(args, "code");
  if (!db().currencies.some((c) => c.code === code)) {
    throw new Error(`替身后端：未知币种 ${code}`);
  }
  db().settings.base_currency = code;
  return { code };
}

export function getLogLevel(): LogLevelState {
  return { level: db().settings.log_level };
}

export function setLogLevel(args: Record<string, unknown>): void {
  db().settings.log_level = str(args, "level");
}

export const MOCK_APP_VERSION = "0.10.0";

export function appBuildInfo(): AppBuildInfo {
  return { version: MOCK_APP_VERSION, platform: "macos", arch: "aarch64" };
}

export function getAiPrompt(): string {
  return (
    "# Ledger API 入口提示词（浏览器开发模式替身）\n\n" +
    "替身后端不提供本地 HTTP API，AI 记账与数据迁移链路在 mock 模式不可用；" +
    "本字面量仅钉 get_ai_prompt 命令形状。"
  );
}

export function openLogDir(): void {}

export function logFrontendError(args: Record<string, unknown>): void {
  str(args, "message");
}

export function revealInFileManager(args: Record<string, unknown>): void {
  str(args, "path");
}

export function restartApp(): void {}
