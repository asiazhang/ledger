import { api } from "@ledger/api";
import { useReferenceStore } from "@/stores/reference";

export async function resolveMerchantRef(
  selected: string | null,
  editingMerchantId: string | null = null,
): Promise<string | null> {
  if (!selected) return null;
  const reference = useReferenceStore();
  if (reference.merchantMap.has(selected)) return selected;
  if (editingMerchantId && selected === editingMerchantId) return selected;
  const name = selected.trim();
  if (!name) return null;
  const existing = reference.merchantByName.get(name);
  if (existing) return existing.id;
  try {
    return await api.createMerchant({ name });
  } catch (e) {
    try {
      await reference.refresh();
    } catch {}
    const retry = reference.merchantByName.get(name);
    if (retry) return retry.id;
    throw e;
  }
}
