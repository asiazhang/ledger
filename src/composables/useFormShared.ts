import { computed } from "vue";
import type { Ref } from "vue";
import { useReferenceStore } from "@/stores/reference";
import { useAppStore } from "@/stores/app";
import { pinArchivedAccountOption, type AccountOption } from "@/accounts/account-option-pin";

export function useFormShared() {
  const reference = useReferenceStore();

  const accountOptions = computed<AccountOption[]>(() => reference.selectableAccountOptions);
  const currencyOptions = computed(() =>
    reference.currencies.map((c) => ({ label: `${c.name} (${c.code})`, value: c.code })),
  );

  return { reference, accountOptions, currencyOptions };
}

export function usePinnedAccountOptions(
  accountId: Ref<string | null | undefined>,
  baseOptions: Ref<AccountOption[]>,
) {
  const reference = useReferenceStore();
  return computed(() =>
    pinArchivedAccountOption(baseOptions.value, accountId.value, reference.accountMap),
  );
}
export function useAccountCurrency(accountId: Ref<string | null>) {
  const reference = useReferenceStore();
  const app = useAppStore();
  return computed(() => {
    const account = accountId.value == null ? undefined : reference.accountMap.get(accountId.value);
    return account?.currency_code ?? app.defaultCurrency;
  });
}

export function useEntityCurrency(source: Ref<{ currency_code: string } | null | undefined>) {
  const app = useAppStore();
  return computed(() => source.value?.currency_code ?? app.defaultCurrency);
}

export function utcMidnightTimestamp(date: string): number {
  return new Date(`${date}T00:00:00Z`).getTime();
}
