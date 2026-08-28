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
 * 与各表单提交端 `new Date(ts).toISOString().slice(0, 10)` 同一口径：
 * 回填不改往返无损，散落多处的日期转换统一收口在此。 */
export function utcMidnightTimestamp(date: string): number {
  return new Date(`${date}T00:00:00Z`).getTime()
}
