import { computed } from 'vue'
import { useAppStore } from '@/stores/app'

export function useFormShared() {
  const store = useAppStore()

  const accountOptions = computed(() =>
    store.accounts.map((a) => ({ label: a.name, value: a.id })),
  )
  const currencyOptions = computed(() =>
    store.currencies.map((c) => ({ label: `${c.name} (${c.code})`, value: c.code })),
  )

  return { store, accountOptions, currencyOptions }
}
