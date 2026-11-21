import { defineStore } from "pinia";
import { computed, ref } from "vue";
import { api } from "@ledger/api";
import { createPushFirstList } from "@/composables/push-first-list";
import type { Policy, PolicyInput, PolicyStats } from "@ledger/types";

export const usePoliciesStore = defineStore("policies", () => {
  const policies = ref<Policy[]>([]);
  const stats = ref<PolicyStats[]>([]);

  const statsById = computed(() => new Map(stats.value.map((s) => [s.policy_id, s])));

  const { status, version, refresh } = createPushFirstList(
    () => Promise.all([api.listPolicies(), api.listPolicyStats()]),
    ([list, statsList]) => {
      policies.value = list;
      stats.value = statsList;
    },
  );

  async function create(input: PolicyInput): Promise<string> {
    const id = await api.createPolicy(input);
    await refresh().catch(() => {});
    return id;
  }

  async function update(id: string, input: PolicyInput): Promise<void> {
    await api.updatePolicy(id, input);
    await refresh().catch(() => {});
  }

  async function remove(id: string): Promise<void> {
    await api.deletePolicy(id);
    await refresh().catch(() => {});
  }

  return {
    policies,
    stats,
    statsById,
    status,
    version,
    refresh,
    create,
    update,
    remove,
  };
});
