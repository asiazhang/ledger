import type { EventCallback, UnlistenFn } from "@tauri-apps/api/event";

/**
 * `@tauri-apps/api/event` 构建期模块替身（issue #1962 / ADR-0143 决策 5）：
 * 进程内事件总线替代 IPC 事件通道。生产代码零改动——listen 签名与真模块对齐
 * （`listen<T>(event, handler)`），返回真实 unlisten 函数。
 *
 * 替身写路径的失效信号（`ledger:changed`，ADR-0012 契约）经 `emitMockEvent`
 * 回送到本总线的全部监听者——参考数据 store 的「事件驱动重拉」因此成立。
 * 进度类事件（汇率同步 / 更新下载 / 行情同步等）不模拟：对应写入路径在替身
 * 里未实现，未命中报错兜住缺口（ADR-0143 决策 3）。
 */

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
  // 快照派发：分发中注销/新注册不进入本轮（对齐真通道「注册完成前到达的事件
  // 会丢失」的既有取舍语义，见 push-first-list 订阅注记）
  for (const reg of Array.from(registrations.values())) {
    if (reg.event !== event) continue;
    // tauri Event 形状投影：id 为监听注册序号（生产 handler 只消费 payload 或忽略）
    reg.handler({ event, id: nextListenerId++, payload });
  }
}
