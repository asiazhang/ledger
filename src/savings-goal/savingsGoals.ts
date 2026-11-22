import { defineStore } from "pinia";
import { computed, ref } from "vue";
import { api } from "@ledger/api";
import { createPushFirstList } from "@/composables/push-first-list";
import type { SavingsGoalInput, SavingsGoalProgress, SavingsGoalUpdateInput } from "@ledger/types";

export const useSavingsGoalsStore = defineStore("savingsGoals", () => {
  const goals = ref<SavingsGoalProgress[]>([]);

  const { status, version, refresh, invalidate } = createPushFirstList(
    () => api.savingsGoalProgress(),
    (snapshot) => {
      goals.value = snapshot;
    },
  );

  const goalAccountIds = computed(() => new Set(goals.value.map((p) => p.goal.account_id)));

  const activeGoals = computed(() => goals.value.filter((p) => p.goal.status === "active"));

  const archivedGoals = computed(() => goals.value.filter((p) => p.goal.status === "archived"));

  async function create(input: SavingsGoalInput): Promise<string> {
    const id = await api.createSavingsGoal(input);
    await refresh().catch(() => {});
    return id;
  }

  async function update(id: string, input: SavingsGoalUpdateInput): Promise<void> {
    await api.updateSavingsGoal(id, input);
    await refresh().catch(() => {});
  }

  async function archive(id: string): Promise<void> {
    await api.archiveSavingsGoal(id);
    await refresh().catch(() => {});
  }

  async function unarchive(id: string): Promise<void> {
    await api.unarchiveSavingsGoal(id);
    await refresh().catch(() => {});
  }

  async function remove(id: string): Promise<void> {
    await api.deleteSavingsGoal(id);
    await refresh().catch(() => {});
  }

  return {
    goals,
    goalAccountIds,
    activeGoals,
    archivedGoals,
    status,
    version,
    refresh,
    invalidate,
    create,
    update,
    archive,
    unarchive,
    remove,
  };
});
