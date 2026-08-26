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

  // store 键为历史契约（useRefundForm/useInvestmentForm 仍在解构它），实为 reference store
  return { store: reference, accountOptions, currencyOptions }
}
