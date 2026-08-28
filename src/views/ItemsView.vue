<script setup lang="ts">
import { h, computed, onMounted, ref } from 'vue'
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
  NPopconfirm,
  useMessage,
  type DataTableColumns,
} from 'naive-ui'
import { formatAmount } from '@/types'
import { yuanToCents, centsToYuan } from '@/utils/money'
import { todayStr } from '@/utils/date'
import type { ItemDailyCost, ItemDisposeInput, ItemInput, ItemWithDailyCost, Transaction } from '@/types'
import { api } from '@/api'
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

const currencyOptions = () =>
  reference.currencies.map((c) => ({ label: `${c.name} (${c.code})`, value: c.code }))

// —— 关联购买交易（issue #119）：可选关联一笔 expense 交易，自动带出购买日期与基础成本 ——
// 后端校验交易存在且为 expense 并以交易值覆盖落库；物品侧仅存溯源指针，无「交易→物品」反向引用。
const expenseTxs = ref<Transaction[]>([])
const linkTxId = ref<string | null>(null)
const editLinkTxId = ref<string | null>(null)
/** 编辑弹窗打开时物品的既有关联：维持原关联 → 手动编辑照常生效；换关 → 后端重新带出覆盖。 */
const editOrigLink = ref<string | null>(null)

const linkTxOptions = () =>
  expenseTxs.value.map((t) => ({
    label: `${t.date} · ${formatAmount(t.amount_cents, reference.getCurrency(t.currency_code))}${t.note ? ` · ${t.note}` : ''}`,
    value: t.id,
  }))

function findLinkedTx(txId: string | null): Transaction | undefined {
  return expenseTxs.value.find((t) => t.id === txId)
}

/** 创建表单：选中关联交易后自动带出日期/成本/币种（带出期间禁用手改，与后端口径一致）。 */
function applyLinkedTx(txId: string | null) {
  const tx = findLinkedTx(txId)
  if (!tx) return
  purchaseDate.value = tx.date
  costYuan.value = String(centsToYuan(tx.amount_cents))
  currencyCode.value = tx.currency_code
}

/** 编辑弹窗：换关时自动带出日期/成本；与后端约定一致，换关即重新带出覆盖。 */
function applyLinkedTxToEdit(txId: string | null) {
  const tx = findLinkedTx(txId)
  if (!tx) return
  editPurchaseDate.value = tx.date
  editCostYuan.value = String(centsToYuan(tx.amount_cents))
}

/** 换关中（选中了与原关联不同的交易）：日期/成本将被后端带出覆盖，禁用手改。 */
const editRelinking = computed(() => editLinkTxId.value !== editOrigLink.value)

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
    purchase_transaction_id: linkTxId.value,
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
  editLinkTxId.value = row.purchase_transaction_id
  editOrigLink.value = row.purchase_transaction_id
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
    purchase_transaction_id: editLinkTxId.value,
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

// —— 详情自选参考日重算（issue #121）：选择参考日 → 后端重算三元组覆盖展示；
// 清空 → 回缺省目标日（在用今天/已处置处置日）。null = 未重算（展示列表快照）。 ——
const detailRefDate = ref<string | null>(null)
const detailCost = ref<ItemDailyCost | null>(null)

/** 详情成本三元组展示值：重算结果优先，未重算/重算失败回落列表快照。 */
const detailCostView = computed(() => ({
  days: detailCost.value?.used_days ?? detail.value?.used_days ?? 0,
  numeratorCents: detailCost.value?.numerator_cents ?? detail.value?.numerator_cents ?? 0,
  perDayCents: detailCost.value?.per_day_cents ?? detail.value?.per_day_cents ?? 0,
}))

async function recalcDetail(date: string | null) {
  if (!detail.value) return
  detailRefDate.value = date
  try {
    detailCost.value = await api.calculateItemCost(detail.value.id, date)
  } catch (e) {
    message.error(`重算失败: ${e}`)
  }
}

function openDetail(row: ItemWithDailyCost) {
  detail.value = row
  // 换行重置：参考日与重算结果不跨物品残留
  detailRefDate.value = null
  detailCost.value = null
}

function detailAmount(cents: number): string {
  return detail.value
    ? formatAmount(cents, reference.getCurrency(detail.value.currency_code))
    : ''
}

// —— 处置（issue #120）：置 disposed 并记录处置日期（必填）与可选残值；
// 已处置物品再次处置 = 修正处置信息 ——
const disposing = ref<ItemWithDailyCost | null>(null)
const disposeDate = ref('')
const disposeResidualYuan = ref('')

function openDispose(row: ItemWithDailyCost) {
  disposing.value = row
  disposeDate.value = row.disposal_date ?? todayStr()
  disposeResidualYuan.value =
    row.residual_value_cents != null ? String(centsToYuan(row.residual_value_cents)) : ''
}

function closeDispose() {
  disposing.value = null
}

async function confirmDispose() {
  if (!disposing.value) return
  if (!disposeDate.value) {
    message.warning('请选择处置日期')
    return
  }
  let residualCents: number | null = null
  if (disposeResidualYuan.value.trim()) {
    const cents = yuanToCents(disposeResidualYuan.value)
    if (cents === null || cents < 0) {
      message.warning('残值需为不小于 0 的金额')
      return
    }
    residualCents = cents
  }
  const input: ItemDisposeInput = {
    disposal_date: disposeDate.value,
    residual_value_cents: residualCents,
  }
  try {
    await itemsStore.dispose(disposing.value.id, input)
    message.success(disposing.value.status === 'in_use' ? '已处置' : '已更新处置信息')
    closeDispose()
  } catch (e) {
    message.error(`处置失败: ${e}`)
  }
}

// —— 软删除（issue #118）：二次确认后 is_deleted=1，列表自动过滤 ——
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
    title: '状态',
    key: 'status',
    render: (row) => (row.status === 'in_use' ? '在用' : `已处置 ${row.disposal_date ?? ''}`),
  },
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
    width: 200,
    render: (row) =>
      h(NSpace, { size: 4 }, () => [
        h(NButton, { size: 'tiny', onClick: () => openDetail(row) }, () => '详情'),
        h(NButton, { size: 'tiny', onClick: () => openEdit(row) }, () => '编辑'),
        h(
          NButton,
          { size: 'tiny', onClick: () => openDispose(row) },
          () => (row.status === 'in_use' ? '处置' : '处置信息'),
        ),
        h(
          NPopconfirm,
          { onPositiveClick: () => removeItem(row.id) },
          {
            default: () => '不再跟踪该物品，从列表移除？',
            trigger: () =>
              h(NButton, { size: 'tiny', type: 'error', quaternary: true }, () => '删除'),
          },
        ),
      ]),
  },
]

onMounted(() => {
  // 物品 store self-init + ledger:changed 信号兜底；mounted 重拉覆盖错误重试
  void itemsStore.refresh().catch(() => {
    /* 失败信号已由 status 承载 */
  })
  // 关联购买交易候选：支出交易，倒序取最近 100 笔（MVP 取舍：更早的交易不在
  // 候选内；加载失败不阻塞创建，可手填日期/成本）
  api
    .listTransactions({ kind: 'expense', limit: 100 })
    .then((r) => (expenseTxs.value = r.items))
    .catch(() => {
      /* 候选为空，创建退化为手填 */
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
            :disabled="!!linkTxId"
            type="date"
            value-format="yyyy-MM-dd"
            style="width: 140px"
          />
        </NFormItem>
        <NFormItem label="总成本">
          <NInput
            v-model:value="costYuan"
            :disabled="!!linkTxId"
            placeholder="总成本（元）"
            style="width: 120px"
          />
        </NFormItem>
        <NFormItem label="币种">
          <NSelect
            v-model:value="currencyCode"
            :options="currencyOptions()"
            :disabled="!!linkTxId"
            style="width: 130px"
          />
        </NFormItem>
        <NFormItem label="关联购买交易">
          <NSelect
            v-model:value="linkTxId"
            :options="linkTxOptions()"
            placeholder="关联购买交易"
            clearable
            style="width: 240px"
            @update:value="applyLinkedTx"
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
            :disabled="editRelinking"
            type="date"
            value-format="yyyy-MM-dd"
            style="width: 160px"
          />
        </NFormItem>
        <NFormItem label="总成本">
          <NInput
            v-model:value="editCostYuan"
            :disabled="editRelinking"
            placeholder="总成本（元）"
            style="width: 160px"
          />
        </NFormItem>
        <NFormItem label="关联购买交易">
          <NSelect
            v-model:value="editLinkTxId"
            :options="linkTxOptions()"
            placeholder="关联购买交易"
            @update:value="applyLinkedTxToEdit"
          />
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

    <!-- 处置弹窗（issue #120）：处置日期必填，残值可选；已处置物品可修正处置信息 -->
    <NModal
      :show="disposing !== null"
      preset="card"
      :title="disposing?.status === 'in_use' ? '处置物品' : '处置信息'"
      style="width: 400px"
      data-testid="item-dispose-modal"
      @update:show="(v: boolean) => (v ? undefined : closeDispose())"
    >
      <NForm v-if="disposing" label-placement="left" :show-feedback="false" size="small">
        <NFormItem label="物品">
          <span>{{ disposing.name }}</span>
        </NFormItem>
        <NFormItem label="处置日期">
          <NDatePicker
            v-model:formatted-value="disposeDate"
            type="date"
            value-format="yyyy-MM-dd"
            style="width: 160px"
          />
        </NFormItem>
        <NFormItem label="残值">
          <NInput
            v-model:value="disposeResidualYuan"
            placeholder="残值（元，可选）"
            style="width: 160px"
          />
        </NFormItem>
        <NSpace justify="end">
          <NButton @click="closeDispose">取消</NButton>
          <NButton type="primary" data-testid="item-dispose-confirm" @click="confirmDispose">
            {{ disposing.status === 'in_use' ? '确认处置' : '保存' }}
          </NButton>
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
        <NDescriptionsItem v-if="detail.status === 'disposed'" label="处置日期">
          {{ detail.disposal_date }}
        </NDescriptionsItem>
        <NDescriptionsItem v-if="detail.status === 'disposed'" label="残值">
          {{
            detail.residual_value_cents != null
              ? detailAmount(detail.residual_value_cents)
              : '—'
          }}
        </NDescriptionsItem>
        <NDescriptionsItem label="总成本">
          {{ detailAmount(detail.total_cost_cents) }}（{{ detail.currency_code }}）
        </NDescriptionsItem>
        <NDescriptionsItem label="本位币折算">
          {{ formatAmount(detail.cost_native_cents, reference.getCurrency(app.defaultCurrency)) }}
          （{{ app.defaultCurrency }}）
        </NDescriptionsItem>
        <NDescriptionsItem label="备注">{{ detail.note ?? '—' }}</NDescriptionsItem>
        <NDescriptionsItem label="关联购买交易">
          {{ detail.purchase_transaction_id ? '已关联（溯源）' : '—' }}
        </NDescriptionsItem>
        <NDescriptionsItem label="参考日">
          <NDatePicker
            :formatted-value="detailRefDate"
            clearable
            type="date"
            value-format="yyyy-MM-dd"
            placeholder="自选参考日"
            style="width: 160px"
            @update:formatted-value="recalcDetail"
          />
        </NDescriptionsItem>
        <NDescriptionsItem label="已用天数">
          {{ detailCostView.days }} 天（含购买当日）
        </NDescriptionsItem>
        <NDescriptionsItem label="每天成本分解">
          {{ detailAmount(detailCostView.numeratorCents) }} ÷ {{ detailCostView.days }} 天 =
          {{ detailAmount(detailCostView.perDayCents) }}/天
        </NDescriptionsItem>
      </NDescriptions>
    </NModal>
  </NSpace>
</template>
