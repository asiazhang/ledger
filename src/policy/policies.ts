import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { api } from '@ledger/api'
import { createPushFirstList } from '@/composables/push-first-list'
import type { Policy, PolicyInput, PolicyStats } from '@ledger/types'

/**
 * 保单（Policy）领域 store（issue #360 / ADR-0051）。
 *
 * 保单是消费型保险合同的静态档案（保险分域词汇表 `Policy`），**不进**
 * `useReferenceStore`（不是可选值字典），拥有自己的单一来源 store。
 *
 * 清单生命周期（self-init / `ledger:changed` 失效重拉 / stale-while-revalidate
 * 整体替换 / 在途合并 / status / version）内化在 push-first 工厂单点
 * （`createPushFirstList`，ADR-0123）；本店只留快照落位（列表 + 统计同批）
 * 与领域动作。
 *
 * 列表只含未删除保单（软删不进列表）；到期状态不在此持久化或推导，
 * 展示层由保障期间即时推导（可推导的状态不落库）。
 *
 * 逐保单视角统计（issue #363）：与列表同生命周期重拉（同一批 IPC 写入都触发
 * `ledger:changed`），`statsById` 供视图按保单 id 取统计行——实时推导不落库，
 * 前端不做二次聚合。
 */
export const usePoliciesStore = defineStore('policies', () => {
  const policies = ref<Policy[]>([])
  const stats = ref<PolicyStats[]>([])

  /** 保单 id → 统计行（视图按行取统计的单点查法）。 */
  const statsById = computed(() => new Map(stats.value.map((s) => [s.policy_id, s])))

  const { status, version, refresh } = createPushFirstList(
    () => Promise.all([api.listPolicies(), api.listPolicyStats()]),
    ([list, statsList]) => {
      policies.value = list
      stats.value = statsList
    },
  )

  /** 创建保单：成功后立即重拉（后端同时发 ledger:changed，事件侧重拉被在途合并）；
   *  重拉失败不反转写动作成败（已落库，失败信号由 status 承载，ADR-0123 决策 3）。 */
  async function create(input: PolicyInput): Promise<string> {
    const id = await api.createPolicy(input)
    await refresh().catch(() => {
      /* 重拉失败不阻断建档成功路径 */
    })
    return id
  }

  /** 按 id 编辑保单静态要素：成功后立即重拉（同 create）。 */
  async function update(id: string, input: PolicyInput): Promise<void> {
    await api.updatePolicy(id, input)
    await refresh().catch(() => {
      /* 重拉失败不阻断编辑成功路径 */
    })
  }

  /** 软删除保单（后端打 is_deleted=1，不物理移除、引用不置空）：成功后立即重拉。 */
  async function remove(id: string): Promise<void> {
    await api.deletePolicy(id)
    await refresh().catch(() => {
      /* 重拉失败不阻断删除成功路径 */
    })
  }

  // push 生命周期（self-init 与 ledger:changed 订阅）由工厂内化（ADR-0123）。

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
  }
})
