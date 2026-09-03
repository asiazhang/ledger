<script setup lang="ts">
import { h, computed, onMounted, ref } from 'vue'
import { NCard, NButton, NDataTable, NSpace, NTag, type DataTableColumns } from 'naive-ui'
import { formatAmount } from '@/types'
import { t } from '@/i18n'
import PhysicalAssetFormModal from '@/components/PhysicalAssetFormModal.vue'
import PhysicalAssetValuationModal from '@/components/PhysicalAssetValuationModal.vue'
import { usePhysicalAssetsStore } from '@/stores/physicalAssets'
import { useReferenceStore } from '@/stores/reference'
import type { PhysicalAsset } from '@/types'

/**
 * 实物资产视图（issue #466 建档列表 / issue #467 T2 更新估值与编辑 / spec #465 /
 * ADR-0064）：列表 + 在持合计卡 + 建档入口 + 行操作（编辑 / 更新估值）。
 *
 * 「更多」页实物资产页签的装载体（ADR-0055 低频视图收纳，先例保单页签）；
 * 数据全部来自 `usePhysicalAssetsStore`（self-init + `ledger:changed` 静默重拉），
 * 本组件只做展示与入口接线，零业务逻辑。列表默认只看在持资产（已处置 /
 * 软删不进默认口径，处置与筛选由 T3 承接）；当前估值折本位币展示消费
 * 后端同源折算（Amount 接缝当期汇率，缺汇率后端整体报错上抛）。
 */
const physicalAssetsStore = usePhysicalAssetsStore()
const reference = useReferenceStore()

const formShow = ref(false)
/** 编辑目标（null = 新建模式，T2：同一建档弹窗双模式）。 */
const editingAsset = ref<PhysicalAsset | null>(null)
const valuationShow = ref(false)
/** 更新估值目标（T2：追加历史行入口）。 */
const valuationAsset = ref<PhysicalAsset | null>(null)

function openCreate() {
  editingAsset.value = null
  formShow.value = true
}

function openEdit(asset: PhysicalAsset) {
  editingAsset.value = asset
  formShow.value = true
}

function openUpdateValuation(asset: PhysicalAsset) {
  valuationAsset.value = asset
  valuationShow.value = true
}

/** 在持估值合计（折本位币，后端同源快照；金额格式化走统一 formatAmount）。 */
const holdingTotalText = computed(() =>
  formatAmount(
    physicalAssetsStore.holdingTotalNativeCents,
    reference.getCurrency(physicalAssetsStore.nativeCurrency),
  ),
)

const columns: DataTableColumns<PhysicalAsset> = [
  { title: () => t('physicalAssets.columns.name'), key: 'name' },
  {
    // 当前估值（折本位币）：在持行显示折算值；估值金额以原币种为权威数字，
    // 展示统一走本位币口径（与合计卡同口径，跨币种可比）
    title: () => t('physicalAssets.columns.valuation'),
    key: 'current_valuation_native_cents',
    render: (row) =>
      row.current_valuation_native_cents !== null
        ? formatAmount(row.current_valuation_native_cents, reference.getCurrency(row.native_currency))
        : '—',
  },
  {
    title: () => t('physicalAssets.columns.valuationDate'),
    key: 'current_valuation_date',
    width: 120,
    render: (row) => row.current_valuation_date,
  },
  {
    title: () => t('physicalAssets.columns.status'),
    key: 'status',
    width: 90,
    render: (row) =>
      h(
        NTag,
        { size: 'small', type: row.status === 'holding' ? 'success' : 'default', bordered: false },
        () =>
          row.status === 'holding'
            ? t('physicalAssets.status.holding')
            : t('physicalAssets.status.disposed'),
      ),
  },
  {
    // 行操作（T2）：编辑档案（名称 / 购买信息）与更新估值（追加历史行）
    title: () => t('physicalAssets.columns.actions'),
    key: 'actions',
    width: 170,
    render: (row) =>
      h(NSpace, { size: 4, wrap: false }, () => [
        h(
          NButton,
          {
            size: 'tiny',
            quaternary: true,
            type: 'primary',
            'data-testid': 'physical-asset-update-valuation',
            onClick: () => openUpdateValuation(row),
          },
          () => t('physicalAssets.actions.updateValuation'),
        ),
        h(
          NButton,
          {
            size: 'tiny',
            quaternary: true,
            'data-testid': 'physical-asset-edit',
            onClick: () => openEdit(row),
          },
          () => t('physicalAssets.actions.edit'),
        ),
      ]),
  },
]

const listTitle = computed(() => t('physicalAssets.listTitle'))
const totalLabel = computed(() => t('physicalAssets.holdingTotal'))

onMounted(() => {
  // store self-init + ledger:changed 信号兜底；mounted 重拉覆盖错误重试
  void physicalAssetsStore.refresh().catch(() => {
    /* 失败信号已由 status 承载 */
  })
})
</script>

<template>
  <NSpace vertical :size="16">
    <NCard size="small">
      <NSpace justify="space-between" align="center">
        <span>
          {{ totalLabel }}：<strong data-testid="physical-asset-holding-total">{{ holdingTotalText }}</strong>
        </span>
        <NButton type="primary" data-testid="physical-asset-new" @click="openCreate">
          {{ t('physicalAssets.newButton') }}
        </NButton>
      </NSpace>
    </NCard>

    <NCard :title="listTitle" size="small">
      <NDataTable
        :columns="columns"
        :data="physicalAssetsStore.assets"
        :bordered="false"
        size="small"
      >
        <template #empty>
          <span data-testid="physical-asset-empty-guide">{{ t('physicalAssets.emptyGuide') }}</span>
        </template>
      </NDataTable>
    </NCard>

    <!-- 新建 / 编辑弹窗（同一建档表单双模式，编辑态无估值字段，T2） -->
    <PhysicalAssetFormModal
      :show="formShow"
      :editing="editingAsset"
      @update:show="formShow = $event"
    />
    <!-- 更新估值弹窗（T2：追加历史行，旧值保留） -->
    <PhysicalAssetValuationModal
      :show="valuationShow"
      :asset="valuationAsset"
      @update:show="valuationShow = $event"
    />
  </NSpace>
</template>
