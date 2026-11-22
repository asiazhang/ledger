import { api } from "@ledger/api";
import { useReferenceStore } from "@/stores/reference";

export async function resolveInsurerRef(selected: string | null): Promise<string | null> {
  if (!selected) return null;
  const reference = useReferenceStore();
  if (reference.insurerMap.has(selected)) return selected;
  const name = selected.trim();
  if (!name) return null;
  const existing = reference.insurerByName.get(name);
  if (existing) return existing.id;
  try {
    return await api.createInsurer({ name });
  } catch (e) {
    try {
      await reference.refresh();
    } catch {}
    const retry = reference.insurerByName.get(name);
    if (retry) return retry.id;
    throw e;
  }
}
