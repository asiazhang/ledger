/**
 * 「最新胜出」竞态纪元守卫（issue #1401）：参数化异步读取的作废在途单点。
 *
 * 语义：每次发起先 `start()` 开启新纪元并取得纪元号，结果落位前以 `isCurrent`
 * 校验该纪元是否仍是最新；纪元被后续 `start` 推进后，迟到的旧纪元结果一律作废、
 * 不落位（`useLoadable` 序号守卫与 ADR-0123 决策 4 修订注 `invalidate` 的同型语义）。
 *
 * 为什么不收编进 Loadable（ADR-0040 决策 6「新代码一律走 Loadable」的例外，同
 * `push-first-list.ts` 的 `epoch` 先例 #1381）：标的远程搜索按词汇表「刻意静默不
 * 收编」只有自己的 searching 标志、失败静默清空候选；且调用方要区分「被取代」与
 * 「失败」——被取代不得触碰结果、失败才清空候选，`useLoadable.run()` 的返回值不
 * 承载这一区分（两者同回 null）。故纪元簿记收口于此机制单点，调用方只消费成对
 * API、不自写序号（机制本体自有簿记，非调用点手搓守卫）。
 *
 * 守门边界：本形态不在 `check-async-guards` 规则 1 的 `let <名>seq = 0` 检测面
 * （纪元按名 `epoch`），与 `push-first-list.ts` 同款；是否将本模块登记为守门的第二
 * 合法住址属守门契约变更（#1381 评审已同款留痕待另议）。
 */

export interface LatestWinsGuard {
  /** 开启新纪元并返回纪元号：此前发起的在途请求随即过期。 */
  start(): number;
  /** 该纪元是否仍是最新——已被后续 `start` 取代（过期）时返回 false，结果不得落位。 */
  isCurrent(epoch: number): boolean;
}

export function createLatestWinsGuard(): LatestWinsGuard {
  let epoch = 0;
  return {
    start(): number {
      epoch += 1;
      return epoch;
    },
    isCurrent(candidate: number): boolean {
      return candidate === epoch;
    },
  };
}
