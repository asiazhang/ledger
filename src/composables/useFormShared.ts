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
