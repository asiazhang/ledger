/**
 * 「最新胜出」竞态纪元共享 module（词汇表「最新胜出（Latest-Wins）」，issue #1678）：
 * 前端异步竞态裁决机制本体（竞态纪元）的唯一实现与唯一合法住址——单调推进的纪元
 * 计数器，发起时 `begin()` 开启新纪元并取得 token，产出落位前以 `token.isStale()`
 * 比对；纪元被后续 begin / invalidate 推进后，迟到的旧纪元产出（结果、失败与收尾
 * 动作）一律作废、不落位。
 *
 * 接口三面 + token 一面（token 制，#1678 定稿）：
 * - `begin()`：推进并取 token——「新的一次发起」语义（Loadable 每次 run、弹窗每次
 *   open、系统返回每次换档）；
 * - `observe()`：采样当前纪元不推进——「并入当前在途批次」语义（push-first-list
 *   refresh 在途合并：并发刷新共享同一裁决点，不互相作废）；
 * - `invalidate()`：推进作废全部在途——「显式作废在途」出口（ADR-0040 invalidate
 *   同名先例：清空/重置、关闭弹窗、作用域销毁）；
 * - token `isStale()`：产出落位前的唯一过期判据。
 *
 * 不做 run-wrapper：统一「返回结果」需发明第二套结果语义，且与 Loadable 错误通道
 * 重叠；过期后「做什么」（丢弃结果、不落意图、撤销迟到注册、不收旗标）属各消费点
 * 组合层，本模块只裁决「谁胜出、何时过期」。
 *
 * 消费点（五处，#1678 收敛，别名 seq/epoch/generation 随收敛退役）：Loadable 每次
 * run 与 invalidate（ADR-0040）；标的搜索与服务端分页列表（issue #1401 先例）；
 * push-first-list refresh 采样 / invalidate 出口（ADR-0123）；系统返回换档/销毁
 * （ADR-0088 决策 7）；交易弹窗编排代数（ADR-0045，last-open-wins）。
 *
 * 守门：竞态纪元唯一合法住址即本文件——scripts/check-async-guards.ts 规则 1 对
 * `let <名>(seq|epoch|generation) = 0` 形态执法并对本文件豁免；新消费点一律 import
 * 本 module，不在调用点手搓计数器。
 */

/** 纪元 token：一次发起（或采样）与纪元的绑定凭证，落位前的唯一过期判据。 */
export interface LatestWinsToken {
  /** 该 token 是否已过期——纪元被后续 begin / invalidate 推进后返回 true，产出不得落位。 */
  isStale(): boolean;
}

/** 最新胜出守卫：竞态纪元的推进与采样接口（工厂每次调用返回独立实例，互不串扰）。 */
export interface LatestWins {
  /** 开启新纪元并返回 token：此前发起的在途产出随即过期。 */
  begin(): LatestWinsToken;
  /** 采样当前纪元不推进：返回与当前纪元绑定的 token（在途合并语义——并发发起共享同一裁决点）。 */
  observe(): LatestWinsToken;
  /** 推进作废全部在途：已发 token 一并过期（ADR-0040 invalidate 同名先例）。 */
  invalidate(): void;
}

export function createLatestWins(): LatestWins {
  let epoch = 0;

  const bind = (): LatestWinsToken => {
    const mine = epoch;
    return { isStale: () => mine !== epoch };
  };

  return {
    begin(): LatestWinsToken {
      epoch += 1;
      return bind();
    },
    observe: bind,
    invalidate(): void {
      epoch += 1;
    },
  };
}
