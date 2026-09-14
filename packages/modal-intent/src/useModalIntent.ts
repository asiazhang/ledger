import { readonly, ref } from 'vue'
import type { Ref } from 'vue'

/**
 * useModalIntent 弹窗意图编排通用工厂（ADR-0072，词汇表「ModalIntent（弹窗意图编排）」）：
 * 模态弹窗「开启 / 目标 / 关闭」编排的唯一形态——调用方声明意图闭集（判别联合，
 * 可携带目标载荷），工厂内化「意图非空即显示、目标随开启递增序号、关闭清回空终态」
 * 的完整编排，产出可观察的意图终态与序号。命名照 useTransactionFilter 先例
 * （工厂形态 composable，ADR-0030）。
 *
 * 接口四面：
 * - intent：`TIntent | null`，唯一事实源，显示由非空派生（无独立显示布尔），
 *   不存在「目标非空但已关闭」的中间态；
 * - seq：只读序号，随每次意图落位递增、关闭不重置——消费方表单重建与迟到回调
 *   过期判定的唯一凭据；
 * - open：纯同步，设意图 + 递增序号；落位的意图对象是工厂产出的全新对象
 *   （浅克隆快照，不依赖调用方每次传新字面量），同一目标重开亦重触发消费方响应
 *   （watch 消费开箱即用）；载荷内的目标行等引用按开启时快照内化，不随后续外
部变更漂移；
 * - close：意图清回 null 终态；关闭后副作用（列表刷新、提示等）留视图。
 *
 * 工厂零业务语义、零外部依赖（无 store、无 api、无组件），不接弹层注册表
 * （弹层纯度，ADR-0035——弹层开/关上报仍由封装组件承担）；异步取数类时序守卫
 * 属消费方适配器层（先例 TransactionModalState），不上浮本工厂。
 */

export interface UseModalIntentReturn<TIntent> {
  /** 当前意图（只读）：null = 关闭终态（弹窗不显示）；非空即「弹窗显示」。 */
  readonly intent: Readonly<Ref<TIntent | null>>
  /** 序号：随每次意图落位递增、关闭不重置。 */
  readonly seq: Readonly<Ref<number>>
  /** 开启意图（纯同步）：落位传入的全新意图对象并递增序号。 */
  open(intent: TIntent): void
  /** 关闭：意图清回 null 终态（关闭后副作用仍归视图）。 */
  close(): void
}

/**
 * 弹窗意图编排工厂：每次调用返回独立实例（意图与序号不串扰）。
 * TIntent 由调用方声明为意图闭集（判别联合）。
 */
export function useModalIntent<TIntent>(): UseModalIntentReturn<TIntent> {
  const intent = ref(null) as Ref<TIntent | null>
  const seq = ref(0)

  function open(next: TIntent) {
    // 浅克隆快照：意图落位的对象恒为工厂产出的全新对象（ADR-0072 决策 1），
    // 同载荷重开（同一引用）也因引用变化重触发 watch 消费。
    intent.value = { ...next }
    seq.value += 1
  }

  function close() {
    intent.value = null
  }

  return {
    // 泛型 TIntent 下 readonly() 的 DeepReadonly 无法结构赋值给 Readonly<Ref<TIntent | null>>，
    // 在接缝处显式收窄（消费方只读 .value，行为不变）。
    intent: readonly(intent) as Readonly<Ref<TIntent | null>>,
    seq: readonly(seq),
    open,
    close,
  }
}
