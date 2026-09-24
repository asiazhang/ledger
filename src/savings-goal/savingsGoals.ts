import { defineStore } from "pinia";
import { computed, ref } from "vue";
import { api } from "@ledger/api";
import { createPushFirstList } from "@/composables/push-first-list";
import type { SavingsGoalInput, SavingsGoalProgress, SavingsGoalUpdateInput } from "@ledger/types";

/**
 * 储蓄目标（SavingsGoal）领域 store（spec #1750 / issue #1751 / ADR-0133）。
 *
 * 目标与进度是独立领域快照（不是可选值字典），拥有自己的单一来源 store；
 * 进度读数 = 专属账户余额（后端余额缓存口径），本店不自算任何进度。
 *
 * 快照生命周期（self-init / `ledger:changed` 失效重拉 / stale-while-revalidate
 * 整体替换 / 在途合并 / status / version）内化在 push-first 工厂单点
 * （`createPushFirstList`，ADR-0123）；创建成功后主动重拉一次，后端同步发出的
 * `ledger:changed` 信号兜底（与实物资产 store 同款语义）。
 */
export const useSavingsGoalsStore = defineStore("savingsGoals", () => {
  const goals = ref<SavingsGoalProgress[]>([]);

  const { status, version, refresh, invalidate } = createPushFirstList(
    () => api.savingsGoalProgress(),
    (snapshot) => {
      goals.value = snapshot;
    },
  );

  /**
   * 目标绑定账户 id 集（issue #1752）：账户身份由绑定派生（ADR-0133 决策 2）
   * 的单一选择器——账户侧名称只读判定（#1752）与账户页分组（#1755）同源消费，
   * 派生不外泄到视图层逐行下探。
   */
  const goalAccountIds = computed(() => new Set(goals.value.map((p) => p.goal.account_id)));

  /** 进行中目标（issue #1754）：默认列表只留进行中（active）——归档是收纳态，
   *  退出默认列表但不删除；快照整体替换由 push-first 工厂承接，分组是纯派生。 */
  const activeGoals = computed(() => goals.value.filter((p) => p.goal.status === "active"));

  /** 已归档目标（issue #1754）：归档列表可查——历史与流水保留，账户原样。 */
  const archivedGoals = computed(() => goals.value.filter((p) => p.goal.status === "archived"));

  /** 创建目标（名称、目标金额、可选截止日期）：写入成功即返回目标 id——不因
   *  重拉失败反转为「保存失败」（数据已落库，重复提交才是真错），重拉由
   *  `ledger:changed` 信号兜底，失败信号由 status 承载。 */
  async function create(input: SavingsGoalInput): Promise<string> {
    const id = await api.createSavingsGoal(input);
    await refresh().catch(() => {
      /* 重拉失败不阻断创建成功路径 */
    });
    return id;
  }

  /** 编辑目标（issue #1752）：四字段全量替换 + 改名联动——写入成功即完成，
   *  不因重拉失败反转为「保存失败」（与 create 同款语义），改名后的账户名
   *  随参考表重拉对账户列表与各下拉可见。 */
  async function update(id: string, input: SavingsGoalUpdateInput): Promise<void> {
    await api.updateSavingsGoal(id, input);
    await refresh().catch(() => {
      /* 重拉失败不阻断编辑成功路径 */
    });
  }

  /** 归档目标（issue #1754）：写入成功即完成，不因重拉失败反转为「归档失败」
   *  （与 create / update 同款语义），重拉由 ledger:changed 信号兜底。 */
  async function archive(id: string): Promise<void> {
    await api.archiveSavingsGoal(id);
    await refresh().catch(() => {
      /* 重拉失败不阻断归档成功路径 */
    });
  }

  /** 取消归档目标（issue #1754）：恢复 active 回默认列表。 */
  async function unarchive(id: string): Promise<void> {
    await api.unarchiveSavingsGoal(id);
    await refresh().catch(() => {
      /* 重拉失败不阻断取消归档成功路径 */
    });
  }

  /** 删除目标（issue #1754）：余额非零被后端码化拒绝（引导先转出），余额为零
   *  后端级联软删专属账户；写入成功即完成，重拉由 ledger:changed 信号兜底。 */
  async function remove(id: string): Promise<void> {
    await api.deleteSavingsGoal(id);
    await refresh().catch(() => {
      /* 重拉失败不阻断删除成功路径 */
    });
  }

  // push 生命周期（self-init 与 ledger:changed 订阅）由工厂内化（ADR-0123）。

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
