import { computed } from "vue";
import type { Ref } from "vue";
import { useReferenceStore } from "@/stores/reference";
import { useAppStore } from "@/stores/app";
import { pinArchivedAccountOption, type AccountOption } from "@/accounts/account-option-pin";

export function useFormShared() {
  const reference = useReferenceStore();

  /**
   * 通用账户下拉候选面：消费参考数据的**可选择账户**单一派生
   * （= 账户去归档，spec #1952 / ADR-0142 决策 4），归档账户不进新选择。
   * 编辑面「已选值恰是归档账户」的补值不在这里做——用 {@link usePinnedAccountOptions}
   * （选项面收窄与补值分开：补值只在有已存值的地方生效）。
   */
  const accountOptions = computed<AccountOption[]>(() => reference.selectableAccountOptions);
  const currencyOptions = computed(() =>
    reference.currencies.map((c) => ({ label: `${c.name} (${c.code})`, value: c.code })),
  );

  return { reference, accountOptions, currencyOptions };
}

/**
 * 账户选项 + 编辑面补值（spec #1952 / ADR-0142 决策 4 的共享小工具，纯函数见
 * `@/accounts/account-option-pin`）：在当前已选值是归档账户时把它补回选项头部并标
 * 「已归档」，其余情形原数组原样返回（零开销）。
 *
 * `baseOptions` 是「新选择面」——已是收窄后的候选（可选择账户派生，可按币种/侧别/
 * 类别再过滤）。补值在收窄**之后**做，故过滤规则（同币种、同方向侧别等）不会把已存值
 * 弄丢：编辑存量行时值本身仍可能不满足过滤（存量脏行），也能读得出来、不被静默改挂。
 */
export function usePinnedAccountOptions(
  accountId: Ref<string | null | undefined>,
  baseOptions: Ref<AccountOption[]>,
) {
  const reference = useReferenceStore();
  return computed(() =>
    pinArchivedAccountOption(baseOptions.value, accountId.value, reference.accountMap),
  );
}
/**
 * 交易币种随所选账户推导（ADR-0134 决策 5 的共享推导单点，issue #1775）：
 * `所选账户币种 ?? 展示币种偏好`。此前该 computed 在投资 / 收支 / 转账 / 计划四个
 * 表单 composable 逐字复制，收进本接缝四处消费同一份推导；账户未选回落「新表单
 * 预选币种」（展示币种偏好，见核心交易域 DefaultCurrency）。各表单的币种语义
 * （记账币种由哪端账户决定）仍归各表单注释，本工厂只承载机械推导。
 */
export function useAccountCurrency(accountId: Ref<string | null>) {
  const reference = useReferenceStore();
  const app = useAppStore();
  return computed(() => {
    const account = accountId.value == null ? undefined : reference.accountMap.get(accountId.value);
    return account?.currency_code ?? app.defaultCurrency;
  });
}

/**
 * 币种随已持有对象推导（ADR-0134 决策 5 共享推导单点的持对象变体，#1775 / #1821）：
 * `所持对象币种 ?? 展示币种偏好`。与 useAccountCurrency（按账户 id 查账户字典）
 * 互补：推导源已是手中对象、无 id 可解析时用本变体——先例：退款表单的币种继承
 * 目标是原支出交易本身（退款继承原支出账户/币种，后端 Writer 守卫照跑，ADR-0134
 * 决策 5），强套按 id 查账户的工厂会歪语义。各表单的币种语义（币种随哪个对象
 * 继承）仍归各表单注释，本工厂只承载机械推导。
 */
export function useEntityCurrency(source: Ref<{ currency_code: string } | null | undefined>) {
  const app = useAppStore();
  return computed(() => source.value?.currency_code ?? app.defaultCurrency);
}

/** 日期字符串（YYYY-MM-DD）→ UTC 午夜时间戳（编辑回填用，issue #178）。
 * 仅作时间戳承载形态，不做时区换算；提交端的日期转换（本地日历日语义）
 * 由 TransactionInput 装配器统一收口（issue #216）。 */
export function utcMidnightTimestamp(date: string): number {
  return new Date(`${date}T00:00:00Z`).getTime();
}
