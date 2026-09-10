<script setup lang="ts">
import { computed } from 'vue'
import { NButton, NSpace } from 'naive-ui'
import AppPopconfirm from '@/components/AppPopconfirm.vue'
import { MOBILE_TOUCH_TARGET_STYLE } from '@/components/mobile-cells'
import type { ScheduledPlanRowAction } from '@/composables/useScheduledPlanList'

/**
 * 行操作渲染组件（ADR-0041 决策 7 / spec #520）：无状态展示组件——
 * 统一行操作描述符数组进、渲染出；AppPopconfirm 二次确认分支、测试锚点
 * （op-${key}-${rowId}）与空占位「—」只此一份。
 *
 * 组件不识计划形态、不设插槽；描述符数组由适配器组装——分期与转账直接透传
 * 清单模块 rowActions 产出，订阅在透传基础上、于详情动作之后插入自建编辑
 * 描述符（插入位置与编辑语义是 ADR-0041 决策 7 显式保留的真差异）。
 *
 * 弹层纯度（ADR-0035）：确认弹层承载上移至此（组件有实例作用域，弹层注册表
 * 上报照常接线）；清单模块仍不引用任何组件、不接弹层注册表。
 */
const props = defineProps<{
  /** 行操作描述符数组（清单模块 rowActions 或适配器按形态组装）。 */
  actions: ScheduledPlanRowAction[]
  /** 行主键：测试锚点 `op-${key}-${rowId}` 的来源。 */
  rowId: string
  /** 移动档变体（issue #848 / ADR-0088 决策 11 票⑧）：动作纵排堆叠 + ≥48px
   *  触控目标（生命周期操作一击可达）；描述符闭集、确认分支、测试锚点两档
   *  共用。缺省桌面档渲染一字不动（回归红线）。 */
  mobile?: boolean
}>()

/** 仅渲染可用动作；全不可用时空占位「—」。 */
const visibleActions = computed(() => props.actions.filter((a) => a.available))
</script>

<template>
  <NSpace v-if="visibleActions.length" :size="mobile ? 2 : 4" :vertical="mobile">
    <template v-for="a in visibleActions" :key="a.key">
      <AppPopconfirm v-if="a.confirm" :on-positive-click="a.run">
        <template #default>{{ a.confirm }}</template>
        <template #trigger>
          <NButton
            :size="mobile ? 'small' : 'tiny'"
            type="error"
            quaternary
            :style="mobile ? MOBILE_TOUCH_TARGET_STYLE : undefined"
            :data-testid="`op-${a.key}-${rowId}`"
          >
            {{ a.label }}
          </NButton>
        </template>
      </AppPopconfirm>
      <NButton
        v-else
        :size="mobile ? 'small' : 'tiny'"
        :style="mobile ? MOBILE_TOUCH_TARGET_STYLE : undefined"
        :data-testid="`op-${a.key}-${rowId}`"
        @click="a.run"
      >
        {{ a.label }}
      </NButton>
    </template>
  </NSpace>
  <span v-else>—</span>
</template>
