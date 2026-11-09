/**
 * Web 参照前端唯一的进程内事件总线（原 `@tauri-apps/api/event` 构建期替身，
 * ADR-0143 决策 5；#2083 切轨后 mock 后端是唯一后端，本模块即唯一实现）。
 * listen 签名与原事件通道对齐（`listen<T>(event, handler)`），返回真实
 * unlisten 函数。
 *
 * 替身写路径的失效信号（`ledger:changed`，ADR-0012 契约；价格类写入
 * `ledger:prices-changed`，ADR-0031）经 `emitMockEvent` 回送到本总线的全部
 * 监听者——参考数据 store 与价格消费方的「事件驱动重拉」因此成立。
 * 进度类事件（汇率同步 / 更新下载 / 行情同步等）不模拟：同步类命令在替身
 * 里答常量态，无真实进度可报。
 */

/** 事件回调载荷形状（与原通道 `Event` 投影同形：事件名 + 注册序号 + payload）。 */
export interface BridgeEvent<T> {
  event: string;
  id: number;
  payload: T;
}

export type EventCallback<T> = (event: BridgeEvent<T>) => void;

export type UnlistenFn = () => void;

interface Registration<T> {
  event: string;
  handler: EventCallback<T>;
}

let nextListenerId = 1;
const registrations = new Map<number, Registration<unknown>>();

export function listen<T>(event: string, handler: EventCallback<T>): Promise<UnlistenFn> {
  const id = nextListenerId++;
  registrations.set(id, { event, handler: handler as EventCallback<unknown> });
  return Promise.resolve(() => {
    registrations.delete(id);
  });
}

/** 替身内部的事件发射出口：把事件分发给全部同名监听者（同步派发）。 */
export function emitMockEvent(event: string, payload?: unknown): void {
  // 快照派发：分发中注销/新注册不进入本轮（对齐原通道「注册完成前到达的事件
  // 会丢失」的既有取舍语义，见 push-first-list 订阅注记）
  for (const reg of Array.from(registrations.values())) {
    if (reg.event !== event) continue;
    // 事件形状投影：id 为监听注册序号（生产 handler 只消费 payload 或忽略）
    reg.handler({ event, id: nextListenerId++, payload });
  }
}
