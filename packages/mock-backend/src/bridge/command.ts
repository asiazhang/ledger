import { toastError } from "@ledger/loadable";

/**
 * Web 参照前端唯一的 invoke 传输面（原 `@tauri-apps/api/core` 构建期替身，
 * ADR-0143 决策 5；#2083 切轨后 Tauri 壳退役——mock 后端是参照界面唯一后端，
 * 本模块即唯一实现，不再有「替身 vs 真模块」双轨）。
 *
 * invoke 不走任何 IPC，而是路由到 `installMockBackend()` 登记的命令表：
 * 已实现命令转发替身实现；未实现命令运行时报错（含命令名，console + toast
 * 双出口，与 invoke 测试接缝「未命中报错」同语义，ADR-0085）——开发中用到
 * 哪个补哪个。签名与原 IPC 通道对齐（`invoke<T>(cmd, args?)`），生产调用方类型
 * 面不变；`InvokeArgs` 按原 IPC 形态镜像（类型擦除，无运行时消费）。
 */

/** 镜像原 IPC 参数形态：参数对象（本应用全部命令均为对象形态）。 */
export interface InvokeArgs {
  [key: string]: unknown;
}

/** 命令实现：收到命令参数对象，返回值或 promise 均可（invoke 统一 await）。 */
export type MockCommandHandler = (args: Record<string, unknown>) => unknown;

const handlers = new Map<string, MockCommandHandler>();

/** 登记命令实现（installMockBackend 装配入口；同名重复登记即覆盖）。 */
export function registerMockCommand(name: string, handler: MockCommandHandler): void {
  handlers.set(name, handler);
}

export async function invoke<T>(cmd: string, args?: InvokeArgs): Promise<T> {
  const handler = handlers.get(cmd);
  if (handler === undefined) {
    // 双出口：console（开发者工具排查）+ toast（界面即时可见，经应用已注册的
    // 统一 sink；注册前 no-op）。错误原样上抛，调用方按既有错误通道消费。
    const message = `替身后端未实现命令: ${cmd}`;
    console.error(`[mock-backend] ${message}（浏览器开发模式，用到哪个补哪个）`);
    toastError(message);
    throw new Error(message);
  }
  return (await handler(args ?? {})) as T;
}
