/**
 * 手动完结的 Promise 替身（issue #1393）：测试按用例节奏 resolve / reject，
 * 用于构造异步时序（竞态、乱序到达、阈值前后等场景）的唯一定义点。
 *
 * 上收自各测试文件的本地定义（八处同构副本：push-first-list / globalBusy /
 * useLoadable 三处含 reject，GlobalBusyBar / physical-assets-store /
 * useRealizedPnl / useInvestmentForm / InstrumentBrowser 五处仅 resolve，
 * 形状已漂移）：统一形状为 `{ promise, resolve, reject }`——只需 resolve
 * 的用例不调用 reject 即可；泛型默认 `unknown`，调用点显式传参收窄
 * （如 `deferred<string[]>()`）。
 */
export function deferred<T = unknown>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}
