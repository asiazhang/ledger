import { computed, ref } from "vue";
import { useReferenceStore } from "@/stores/reference";
import { resolveMerchantRef } from "@/merchants/resolve-merchant";
import { t } from "@ledger/i18n";

export function useMerchantField(editingMerchantId: string | null = null) {
  const reference = useReferenceStore();

  const merchantRef = ref<string | null>(null);

  const merchantOptions = computed<{ label: string; value: string }[]>(() => {
    const base = reference.merchants.map((m) => ({ label: m.name, value: m.id }));
    if (editingMerchantId && !reference.merchantMap.has(editingMerchantId)) {
      base.unshift({ label: t("transactions.form.merchantDeleted"), value: editingMerchantId });
    }
    return base;
  });

  async function resolveMerchantId(): Promise<string | null> {
    return resolveMerchantRef(merchantRef.value, editingMerchantId);
  }

  return { merchantRef, merchantOptions, resolveMerchantId };
}
