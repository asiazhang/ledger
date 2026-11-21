import { invoke } from "@tauri-apps/api/core";
import { expect, vi, type Mock } from "vitest";
import { REFERENCE_DEFAULTS } from "./reference-stubs";

export type AppInvokeHandler = (cmd: string, args?: Record<string, unknown>) => Promise<unknown>;

export const mockInvoke = vi.mocked(invoke) as unknown as Mock<AppInvokeHandler>;

export function lastInvokeArgs(cmd: string): Record<string, unknown> {
  const call = mockInvoke.mock.calls.filter(([c]) => c === cmd).at(-1);
  expect(call, `应已调用 ${cmd}`).toBeTruthy();
  expect(call![1], `调用 ${cmd} 应携带对象 args`).toBeDefined();
  return call![1] as Record<string, unknown>;
}

let referenceRefresher: (() => Promise<unknown>) | null = null;

export function registerReferenceRefresher(refresh: () => Promise<unknown>): void {
  referenceRefresher = refresh;
}

export function unexpectedInvoke(cmd: string): Promise<never> {
  return Promise.reject(new Error(`unexpected invoke: ${cmd}`));
}

function resolveSeamFallback(
  cmd: string,
  defaults: Record<string, unknown> = {},
): Promise<unknown> {
  if (cmd in defaults) return Promise.resolve(defaults[cmd]);
  if (cmd in REFERENCE_DEFAULTS) return Promise.resolve(REFERENCE_DEFAULTS[cmd]);
  return unexpectedInvoke(cmd);
}

export type InvokeSeamStaticValue = string | number | boolean | null | undefined | object;

export type InvokeSeamOverride =
  | ((args?: Record<string, unknown>) => unknown)
  | InvokeSeamStaticValue;

export interface InvokeSeamOptions {
  defaults?: Record<string, InvokeSeamStaticValue>;
  overrides?: Record<string, InvokeSeamOverride>;
  refreshReferenceStores?: boolean;
}

export type InvokeSeamDispatcher = ((
  cmd: string,
  args?: Record<string, unknown>,
) => Promise<unknown>) & {
  ready?: Promise<void>;
};

function isThenable(value: unknown): value is Promise<unknown> {
  return typeof (value as Promise<unknown> | undefined)?.then === "function";
}

export function wireInvokeSeam(options: InvokeSeamOptions = {}): InvokeSeamDispatcher {
  const defaults = options.defaults ?? {};
  const overrides = options.overrides ?? {};
  const dispatch = ((cmd: string, args?: Record<string, unknown>): Promise<unknown> => {
    if (cmd in overrides) {
      const handler = overrides[cmd];
      if (typeof handler === "function") {
        const out = (handler as (a?: Record<string, unknown>) => unknown)(args);
        return isThenable(out) ? out : Promise.resolve(out);
      }
      return Promise.resolve(handler);
    }
    return resolveSeamFallback(cmd, defaults);
  }) as InvokeSeamDispatcher;
  mockInvoke.mockImplementation(dispatch as typeof invoke);
  if (options.refreshReferenceStores) {
    if (!referenceRefresher) {
      throw new Error(
        "refreshReferenceStores 需要参考 store 刷新器：应用壳测试工程由 app-setup 注册（registerReferenceRefresher）",
      );
    }
    dispatch.ready = referenceRefresher().then(() => undefined);
  }
  return dispatch;
}
