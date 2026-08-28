<script setup lang="ts">
import { h, onMounted, ref } from 'vue'
import {
  NCard,
  NButton,
  NDataTable,
  NForm,
  NFormItem,
  NInput,
  NDatePicker,
  NModal,
  NSelect,
  NSpace,
  NDescriptions,
  NDescriptionsItem,
  useMessage,
  type DataTableColumns,
} from 'naive-ui'
import { formatAmount } from '@/types'
import { yuanToCents, centsToYuan } from '@/utils/money'
import type { ItemInput, ItemWithDailyCost } from '@/types'
import { useReferenceStore } from '@/stores/reference'
import { useAppStore } from '@/stores/app'
import { useItemsStore } from '@/stores/items'

const reference = useReferenceStore()
const app = useAppStore()
const itemsStore = useItemsStore()
const message = useMessage()

// —— 创建表单（最小入口：名称 / 购买日期 / 总成本 / 币种 / 备注） ——
const name = ref('')
const purchaseDate = ref(todayStr())
const costYuan = ref('')
const currencyCode = ref('CNY')
const note = ref('')

/** 本地时区今天（YYYY-MM-DD），作为购买日期默认值。 */
function todayStr(): string {
  const d = new Date()
  const month = String(d.getMonth() + 1).padStart(2, '0')
  const day = String(d.getDate()).padStart(2, '0')
  return `${d.getFullYear()}-${month}-${day}`
}

const currencyOptions = () =>
  reference.currencies.map((c) => ({ label: `${c.name} (${c.code})`, value: c.code }))

async function create() {
  if (!name.value.trim()) {
    message.warning('请输入物品名称')
    return
  }
  const costCents = yuanToCents(costYuan.value)
  if (costCents === null || costCents <= 0) {
    message.warning('请输入大于 0 的总成本')
    return
  }
  const input: ItemInput = {
    name: name.value.trim(),
    purchase_date: purchaseDate.value,
    total_cost_cents: costCents,
    currency_code: currencyCode.value,
    note: note.value.trim() || null,
  }
  try {
    await itemsStore.create(input)
    message.success('已创建物品')
    name.value = ''
    costYuan.value = ''
    note.value = ''
  } catch (e) {
    message.error(`创建失败: ${e}`)
  }
}

// —— 编辑（issue #117）：按 id 修改 名称 / 购买日期 / 总成本 / 备注；币种不可改 ——
const editing = ref<ItemWithDailyCost | null>(null)
const editName = ref('')
const editPurchaseDate = ref('')
const editCostYuan = ref('')
const editNote = ref('')

function openEdit(row: ItemWithDailyCost) {
  editing.value = row
  editName.value = row.name
  editPurchaseDate.value = row.purchase_date
  editCostYuan.value = String(centsToYuan(row.total_cost_cents))
  editNote.value = row.note ?? ''
}

function closeEdit() {
  editing.value = null
}

async function saveEdit() {
  if (!editing.value) return
  if (!editName.value.trim()) {
    message.warning('请输入物品名称')
    return
  }
  if (!editPurchaseDate.value) {
    message.warning('请选择购买日期')
    return
  }
  const costCents = yuanToCents(editCostYuan.value)
  if (costCents === null || costCents <= 0) {
    message.warning('请输入大于 0 的总成本')
    return
  }
  const input: ItemInput = {
    name: editName.value.trim(),
    purchase_date: editPurchaseDate.value,
    total_cost_cents: costCents,
    currency_code: editing.value.currency_code,
    note: editNote.value.trim() || null,
  }
  try {
    await itemsStore.update(editing.value.id, input)
    message.success('已保存')
    closeEdit()
  } catch (e) {
    message.error(`保存失败: ${e}`)
  }
}

// —— 详情（issue #117）：成本分解 = 分子（总成本 − 残值） ÷ 已用天数 = 每天成本 ——
const detail = ref<ItemWithDailyCost | null>(null)

function openDetail(row: ItemWithDailyCost) {
  detail.value = row
}

function detailAmount(cents: number): string {
  return detail.value
    ? formatAmount(cents, reference.getCurrency(detail.value.currency_code))
    : ''
}

// —— 物品列表 ——
const columns: DataTableColumns<ItemWithDailyCost> = [
  { title: '名称', key: 'name' },
  { title: '购买日期', key: 'purchase_date' },
  {
    title: '总成本',
    key: 'total_cost_cents',
    render: (row) =>
      formatAmount(row.total_cost_cents, reference.getCurrency(row.currency_code)),
  },
  { title: '已用天数', key: 'used_days' },
  {
    title: '每天成本',
    key: 'per_day_cents',
    render: (row) =>
      formatAmount(row.per_day_cents, reference.getCurrency(row.currency_code)),
  },
  {
    title: '操作',
    key: 'actions',
    render: (row) =>
      h(NSpace, { size: 4 }, () => [
        h(NButton, { size: 'tiny', onClick: () => openDetail(row) }, () => '详情'),
        h(NButton, { size: 'tiny', onClick: () => openEdit(row) }, () => '编辑'),
      ]),
  },
]

onMounted(() => {
  // 物品 store self-init + ledger:changed 信号兜底；mounted 重拉覆盖错误重试
  void itemsStore.refresh().catch(() => {
    /* 失败信号已由 status 承载 */
  })
})
</script>

<template>
  <NSpace vertical :size="16">
    <NCard title="新增物品" size="small">
      <NForm label-placement="left" :show-feedback="false" inline size="small">
        <NFormItem label="名称">
          <NInput v-model:value="name" placeholder="物品名称" style="width: 160px" />
        </NFormItem>
        <NFormItem label="购买日期">
          <NDatePicker
            v-model:formatted-value="purchaseDate"
            type="date"
            value-format="yyyy-MM-dd"
            style="width: 140px"
          />
        </NFormItem>
        <NFormItem label="总成本">
          <NInput v-model:value="costYuan" placeholder="总成本（元）" style="width: 120px" />
        </NFormItem>
        <NFormItem label="币种">
          <NSelect
            v-model:value="currencyCode"
            :options="currencyOptions()"
            style="width: 130px"
          />
        </NFormItem>
        <NFormItem label="备注">
          <NInput v-model:value="note" placeholder="品牌 / 型号 / 渠道（可选）" style="width: 200px" />
        </NFormItem>
        <NButton type="primary" @click="create">创建</NButton>
      </NForm>
    </NCard>

    <NCard title="物品列表" size="small">
      <NDataTable :columns="columns" :data="itemsStore.items" :bordered="false" size="small" />
    </NCard>

    <!-- 编辑弹窗（issue #117）：币种不可改，沿用行内币种 -->
    <NModal
      :show="editing !== null"
      preset="card"
      title="编辑物品"
      style="width: 440px"
      data-testid="item-edit-modal"
      @update:show="(v: boolean) => (v ? undefined : closeEdit())"
    >
      <NForm v-if="editing" label-placement="left" :show-feedback="false" size="small">
        <NFormItem label="名称">
          <NInput v-model:value="editName" placeholder="物品名称" />
        </NFormItem>
        <NFormItem label="购买日期">
          <NDatePicker
            v-model:formatted-value="editPurchaseDate"
            type="date"
            value-format="yyyy-MM-dd"
            style="width: 160px"
          />
        </NFormItem>
        <NFormItem label="总成本">
          <NInput v-model:value="editCostYuan" placeholder="总成本（元）" style="width: 160px" />
        </NFormItem>
        <NFormItem label="币种">
          <span>{{ editing.currency_code }}（不可修改）</span>
        </NFormItem>
        <NFormItem label="备注">
          <NInput v-model:value="editNote" placeholder="品牌 / 型号 / 渠道（可选）" />
        </NFormItem>
        <NSpace justify="end">
          <NButton @click="closeEdit">取消</NButton>
          <NButton type="primary" @click="saveEdit">保存</NButton>
        </NSpace>
      </NForm>
    </NModal>

    <!-- 详情弹窗（issue #117）：展示成本分解 = 分子 ÷ 已用天数 = 每天成本 -->
    <NModal
      :show="detail !== null"
      preset="card"
      title="物品详情"
      style="width: 480px"
      data-testid="item-detail-modal"
      @update:show="(v: boolean) => (v ? undefined : (detail = null))"
    >
      <NDescriptions v-if="detail" :column="1" size="small" label-placement="left" bordered>
        <NDescriptionsItem label="名称">{{ detail.name }}</NDescriptionsItem>
        <NDescriptionsItem label="状态">
          {{ detail.status === 'in_use' ? '在用' : '已处置' }}
        </NDescriptionsItem>
        <NDescriptionsItem label="购买日期">{{ detail.purchase_date }}</NDescriptionsItem>
        <NDescriptionsItem label="总成本">
          {{ detailAmount(detail.total_cost_cents) }}（{{ detail.currency_code }}）
        </NDescriptionsItem>
        <NDescriptionsItem label="本位币折算">
          {{ formatAmount(detail.cost_native_cents, reference.getCurrency(app.defaultCurrency)) }}
          （{{ app.defaultCurrency }}）
        </NDescriptionsItem>
        <NDescriptionsItem label="备注">{{ detail.note ?? '—' }}</NDescriptionsItem>
        <NDescriptionsItem label="已用天数">
          {{ detail.used_days }} 天（含购买当日）
        </NDescriptionsItem>
        <NDescriptionsItem label="每天成本分解">
          {{ detailAmount(detail.numerator_cents) }} ÷ {{ detail.used_days }} 天 =
          {{ detailAmount(detail.per_day_cents) }}/天
        </NDescriptionsItem>
      </NDescriptions>
    </NModal>
  </NSpace>
</template>
