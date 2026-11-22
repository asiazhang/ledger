import { toastError } from "@ledger/loadable";

export interface InvokeArgs {
  [key: string]: unknown;
}

export type MockCommandHandler = (args: Record<string, unknown>) => unknown;

const handlers = new Map<string, MockCommandHandler>();

export function registerMockCommand(name: string, handler: MockCommandHandler): void {
  handlers.set(name, handler);
}

export async function invoke<T>(cmd: string, args?: InvokeArgs): Promise<T> {
  const handler = handlers.get(cmd);
  if (handler === undefined) {
    const message = `替身后端未实现命令: ${cmd}`;
    console.error(`[mock-backend] ${message}（浏览器开发模式，用到哪个补哪个）`);
    toastError(message);
    throw new Error(message);
  }
  return (await handler(args ?? {})) as T;
}
