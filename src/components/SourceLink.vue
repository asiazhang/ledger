<script setup lang="ts">
import { computed, type Component } from 'vue'
import { useRouter } from 'vue-router'
import { NIcon, NTag } from 'naive-ui'
import {
  CalendarClearOutline,
  CubeOutline,
  RepeatOutline,
  ShieldCheckmarkOutline,
  SwapHorizontalOutline,
  TrendingUpOutline,
} from '@vicons/ionicons5'
import { resolveSourceJumpTarget, type TransactionSourceKind } from '@/components/source-jump'
import { useAppStore } from '@/stores/app'
import { useSidebarOrderStore } from '@/stores/sidebar-order'
import { darkOverrides, lightOverrides } from '@/theme/overrides'
import { t } from '@/i18n'
import type { TransactionSource } from '@/types'

/**
 * 来源列单元格（spec #704 / issue #706，词汇表「来源列」「实体定位参数（focus 参数）」）：
 * 来源类型图标 + 实体名 + 可空状态标注。点击经来源跳转深模块计算路由目标
 * （`resolveSourceJumpTarget`：落点分流、计划页签叠加、focus 统一装配收口单点），
 * 组件只注入收纳谓词（sidebar-order store 的 `isViewContained`）——路由细节不在此处。
 *
 * 可点击裁决（词汇表「不可点击范围仅软删保单」）：`status = deleted` 的软删保单
 * 不在列表、无详情面——名称 +「已删除」标注、不可点击（不提供落空的跳转）；
 * 其余状态（取消的计划/已处置物品）可点击、名称旁带标注。
 *
 * 视觉与交互同 MerchantLink/AccountLink 先例（真 <button> + 主题强调色 + 省略号）；
 * 来源类型全称收进悬停 tooltip（词汇表「列形态」）。
 */

/** 来源类型 → 图标（六类闭集穷尽映射，编译器强制补行；与侧栏视图图标同源惯例：
 * 保单=盾、物品=立方、标的=走势、订阅=循环、转账=双向、分期=日历）。 */
const KIND_ICONS = {
  installmentPlan: CalendarClearOutline,
  subscription: RepeatOutline,
  scheduledTransfer: SwapHorizontalOutline,
  policy: ShieldCheckmarkOutline,
  item: CubeOutline,
  instrument: TrendingUpOutline,
} satisfies Record<TransactionSourceKind, Component>

const props = defineProps<{
  /** 行来源对象（列表/搜索命令填充，无来源行不渲染本组件） */
  source: TransactionSource
}>()

const router = useRouter()
const app = useAppStore()
const sidebarOrder = useSidebarOrderStore()

/** 状态标注文案（status 为空则无标注）。 */
const statusLabel = computed(() =>
  props.source.status ? t(`transactions.source.status.${props.source.status}`) : null,
)

/** 展示名：计划来源无备注时后端回空串，按来源类型名兜底（图标旁仍有可读名称，
 *  文案随界面语言；spec #704 / issue #707，计划名口径：备注即名）。 */
const displayName = computed(
  () => props.source.display_name || t(`transactions.source.kind.${props.source.kind}`),
)

/** 软删保单不可点击（不提供落空的跳转）；其余来源可点击。 */
const clickable = computed(() => props.source.status !== 'deleted')

// 强调色与 MerchantLink/AccountLink 同源：theme/overrides.ts 单一来源，按当前主题取值。
const accent = computed(() => {
  const common = app.theme === 'dark' ? darkOverrides.common : lightOverrides.common
  return {
    base: common?.primaryColor ?? '#F59E0B',
    hover: common?.primaryColorHover ?? '#FBBF24',
  }
})

function go() {
  if (!clickable.value) return
  // 收纳谓词注入（深模块约定）：只问本次跳转的目标视图
  router.push(
    resolveSourceJumpTarget(props.source.kind, props.source.entity_id, (v) =>
      sidebarOrder.isViewContained(v),
    ),
  )
}
</script>

<template>
  <span class="source-cell" :title="t(`transactions.source.kind.${source.kind}`)">
    <NIcon :component="KIND_ICONS[source.kind]" class="source-cell-icon" />
    <button
      v-if="clickable"
      type="button"
      class="source-link"
      :style="{ color: accent.base, '--accent-hover': accent.hover }"
      @click="go"
    >
      {{ displayName }}
    </button>
    <span v-else class="source-name">{{ displayName }}</span>
    <NTag v-if="statusLabel" size="small" :bordered="false">{{ statusLabel }}</NTag>
  </span>
</template>

<style scoped>
.source-cell {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  max-width: 100%;
  /* 图标随内容着色后降低存在感：类型是扫读线索不是主信息 */
  color: inherit;
  opacity: 0.92;
}
.source-cell-icon {
  flex: none;
  opacity: 0.65;
}
.source-link {
  /* 文本按钮：继承单元格字号，无边框无背景（MerchantLink 同款） */
  border: none;
  padding: 0;
  background: none;
  font: inherit;
  cursor: pointer;
  border-radius: 4px;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.source-link:hover {
  color: var(--accent-hover);
  background: rgba(255, 255, 255, 0.06);
  text-decoration: underline;
}
.source-link:focus-visible {
  outline: 2px solid var(--accent-hover);
  outline-offset: 2px;
}
.source-name {
  /* 不可点击名称（软删保单）：继承色纯文本，同样省略号兜底 */
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
</style>
