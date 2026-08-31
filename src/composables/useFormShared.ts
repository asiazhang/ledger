import { computed } from 'vue'
import { useReferenceStore } from '@/stores/reference'

export function useFormShared() {
  const reference = useReferenceStore()

  const accountOptions = computed(() =>
    reference.accounts.map((a) => ({ label: a.name, value: a.id })),
  )
  const currencyOptions = computed(() =>
    reference.currencies.map((c) => ({ label: `${c.name} (${c.code})`, value: c.code })),
  )

  return { reference, accountOptions, currencyOptions }
}

/** 日期字符串（YYYY-MM-DD）→ UTC 午夜时间戳（编辑回填用，issue #178）。
 * 仅作时间戳承载形态，不做时区换算；提交端的日期转换（本地日历日语义）
 * 由 TransactionInput 装配器统一收口（issue #216）。 */
export function utcMidnightTimestamp(date: string): number {
  return new Date(`${date}T00:00:00Z`).getTime()
}
