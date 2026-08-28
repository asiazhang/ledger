<script setup lang="ts">
import { onMounted, ref, h } from 'vue'
import {
  NCard,
  NButton,
  NDataTable,
  NForm,
  NFormItem,
  NInput,
  NDatePicker,
  NSelect,
  NSpace,
  NPopconfirm,
  useMessage,
  type DataTableColumns,
} from 'naive-ui'
import { formatAmount } from '@/types'
import { yuanToCents } from '@/utils/money'
import type { ItemInput, ItemWithDailyCost } from '@/types'
import { useReferenceStore } from '@/stores/reference'
import { useItemsStore } from '@/stores/items'

const reference = useReferenceStore()
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

async function removeItem(id: string) {
  try {
    await itemsStore.remove(id)
    message.success('已删除')
  } catch (e) {
    message.error(`删除失败: ${e}`)
  }
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
    width: 90,
    render: (row) =>
      h(
        NPopconfirm,
        { onPositiveClick: () => removeItem(row.id) },
        {
          default: () => '不再跟踪该物品，从列表移除？',
          trigger: () =>
            h(NButton, { size: 'tiny', type: 'error', quaternary: true }, () => '删除'),
        },
      ),
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
  </NSpace>
</template>
