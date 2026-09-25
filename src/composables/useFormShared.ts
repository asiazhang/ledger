import { computed } from "vue";
import type { Ref } from "vue";
import { useReferenceStore } from "@/stores/reference";
import { useAppStore } from "@/stores/app";

export function useFormShared() {
  const reference = useReferenceStore();

  const accountOptions = computed(() =>
    reference.accounts.map((a) => ({ label: a.name, value: a.id })),
  );
  const currencyOptions = computed(() =>
    reference.currencies.map((c) => ({ label: `${c.name} (${c.code})`, value: c.code })),
  );

  return { reference, accountOptions, currencyOptions };
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

/** 日期字符串（YYYY-MM-DD）→ UTC 午夜时间戳（编辑回填用，issue #178）。
 * 仅作时间戳承载形态，不做时区换算；提交端的日期转换（本地日历日语义）
 * 由 TransactionInput 装配器统一收口（issue #216）。 */
export function utcMidnightTimestamp(date: string): number {
  return new Date(`${date}T00:00:00Z`).getTime();
}
