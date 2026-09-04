<script setup lang="ts">
import { errorMessage } from '@/utils/errors'
import { h, computed, onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import {
  NCard,
  NButton,
  NDataTable,
  NForm,
  NFormItem,
  NInput,
  NAlert,
  NSpace,
  NDescriptions,
  NDescriptionsItem,
  useMessage,
  type DataTableColumns,
} from 'naive-ui'
import { formatAmount } from '@/types'
import { yuanToCents, centsToYuan } from '@/utils/money'
import { todayStr } from '@/utils/date'
import type { ItemDailyCost, ItemDisposeInput, ItemInput, ItemWithDailyCost, Transaction } from '@/types'
import { api } from '@/api'
import AppModal from '@/components/AppModal.vue'
import AppDatePicker from '@/components/AppDatePicker.vue'
import AppPopconfirm from '@/components/AppPopconfirm.vue'
import PinyinSelect from '@/components/PinyinSelect.vue'
import { useModalIntent } from '@/composables/useModalIntent'
import { useReferenceStore } from '@/stores/reference'
import { useAppStore } from '@/stores/app'
import { useItemsStore } from '@/stores/items'
import { t } from '@/i18n'

const reference = useReferenceStore()
const app = useAppStore()
const itemsStore = useItemsStore()
const message = useMessage()
const router = useRouter()

// —— 创建唯一入口提示（issue #207，ADR-0025）：物品只能经交易右键「加入物品」创建，
// 本页不提供手动新增表单；提示条常驻顶部并一键跳转交易页。——
function goTransactions() {
  router.push({ name: 'transactions' })
}

// —— 关联购买交易候选（issue #119）：编辑弹窗换关仍需候选列表（交易右键创建入口不经此页）——
// 后端校验交易存在且为 expense 并以交易值覆盖落库；物品侧仅存溯源指针，无「交易→物品」反向引用。
const expenseTxs = ref<Transaction[]>([])
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

/** 编辑弹窗：换关时自动带出日期/成本；与后端约定一致，换关即重新带出覆盖。 */
function applyLinkedTxToEdit(txId: string | null) {
  const tx = findLinkedTx(txId)
  if (!tx) return
  editPurchaseDate.value = tx.date
  editCostYuan.value = String(centsToYuan(tx.amount_cents))
}

/** 换关中（选中了与原关联不同的交易）：日期/成本将被后端带出覆盖，禁用手改。 */
const editRelinking = computed(() => editLinkTxId.value !== editOrigLink.value)

// —— 编辑（issue #117）：按 id 修改 名称 / 购买日期 / 总成本 / 备注；币种不可改 ——
// 开启/目标/关闭编排归弹窗意图工厂 ModalIntent（ADR-0072）：意图闭集单成员
// （携带目标物品行），显示由「意图非空」派生、序号随开启递增驱动表单重建、
// 关闭清回 null 终态。现状无序号守卫，迁移为缺陷修复（本票唯一声明的行为
// 变化）：守卫从无到有，「弹窗开着时目标行被替换回填旧行」缺陷消亡，
// 同目标重开重回填等边缘语义细化同归此类；此外等价。

/** 编辑物品弹窗意图（单成员闭集）：携带目标物品行。 */
interface ItemEditIntent {
  row: ItemWithDailyCost
}

const {
  intent: editIntent,
  seq: editSeq,
  open: openEditIntent,
  close: closeEdit,
} = useModalIntent<ItemEditIntent>()

const editName = ref('')
const editPurchaseDate = ref('')
const editCostYuan = ref('')
const editNote = ref('')

function openEdit(row: ItemWithDailyCost) {
  editName.value = row.name
  editPurchaseDate.value = row.purchase_date
  editCostYuan.value = String(centsToYuan(row.total_cost_cents))
  editNote.value = row.note ?? ''
  editLinkTxId.value = row.purchase_transaction_id
  editOrigLink.value = row.purchase_transaction_id
  openEditIntent({ row })
}

async function saveEdit() {
  if (!editIntent.value) return
  if (!editName.value.trim()) {
    message.warning(t('items.msg.nameRequired'))
    return
  }
  if (!editPurchaseDate.value) {
    message.warning(t('items.msg.dateRequired'))
    return
  }
  const costCents = yuanToCents(editCostYuan.value)
  if (costCents === null || costCents <= 0) {
    message.warning(t('items.msg.costInvalid'))
    return
  }
  const input: ItemInput = {
    name: editName.value.trim(),
    purchase_date: editPurchaseDate.value,
    total_cost_cents: costCents,
    currency_code: editIntent.value.row.currency_code,
    note: editNote.value.trim() || null,
    purchase_transaction_id: editLinkTxId.value,
  }
  try {
    await itemsStore.update(editIntent.value.row.id, input)
    message.success(t('items.msg.saved'))
    closeEdit()
  } catch (e) {
    message.error(t('items.msg.saveFailed', { msg: errorMessage(e) }))
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
    message.error(t('items.msg.recalcFailed', { msg: errorMessage(e) }))
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
    message.warning(t('items.msg.disposeDateRequired'))
    return
  }
  let residualCents: number | null = null
  if (disposeResidualYuan.value.trim()) {
    const cents = yuanToCents(disposeResidualYuan.value)
    if (cents === null || cents < 0) {
      message.warning(t('items.msg.residualInvalid'))
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
    message.success(
      disposing.value.status === 'in_use'
        ? t('items.msg.disposed')
        : t('items.msg.disposeUpdated'),
    )
    closeDispose()
  } catch (e) {
    message.error(t('items.msg.disposeFailed', { msg: errorMessage(e) }))
  }
}

// —— 软删除（issue #118）：二次确认后 is_deleted=1，列表自动过滤 ——
async function removeItem(id: string) {
  try {
    await itemsStore.remove(id)
    message.success(t('items.msg.deleted'))
  } catch (e) {
    message.error(t('items.msg.deleteFailed', { msg: errorMessage(e) }))
  }
}

// —— 物品列表 ——
const columns: DataTableColumns<ItemWithDailyCost> = [
  { title: () => t('items.columns.name'), key: 'name' },
  { title: () => t('items.columns.purchaseDate'), key: 'purchase_date' },
  {
    title: () => t('items.columns.status'),
    key: 'status',
    render: (row) =>
      row.status === 'in_use'
        ? t('items.status.inUse')
        : t('items.status.disposedOn', { date: row.disposal_date ?? '' }),
  },
  {
    title: () => t('items.columns.totalCost'),
    key: 'total_cost_cents',
    render: (row) =>
      formatAmount(row.total_cost_cents, reference.getCurrency(row.currency_code)),
  },
  { title: () => t('items.columns.usedDays'), key: 'used_days' },
  {
    title: () => t('items.columns.perDayCost'),
    key: 'per_day_cents',
    render: (row) =>
      formatAmount(row.per_day_cents, reference.getCurrency(row.currency_code)),
  },
  {
    title: () => t('items.columns.actions'),
    key: 'actions',
    width: 200,
    render: (row) =>
      h(NSpace, { size: 4 }, () => [
        h(NButton, { size: 'tiny', onClick: () => openDetail(row) }, () => t('items.rowActions.detail')),
        h(NButton, { size: 'tiny', onClick: () => openEdit(row) }, () => t('items.rowActions.edit')),
        h(
          NButton,
          { size: 'tiny', onClick: () => openDispose(row) },
          () =>
            row.status === 'in_use'
              ? t('items.rowActions.dispose')
              : t('items.rowActions.disposeInfo'),
        ),
        h(
          AppPopconfirm,
          { onPositiveClick: () => removeItem(row.id) },
          {
            default: () => t('items.deleteConfirm'),
            trigger: () =>
              h(NButton, { size: 'tiny', type: 'error', quaternary: true }, () =>
                t('items.rowActions.delete'),
              ),
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
  // 候选内；加载失败不阻塞编辑弹窗换关）
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
    <!-- 创建唯一入口提示（issue #207）：本页不提供手动新增，物品只能经交易右键「加入物品」创建 -->
    <NAlert type="info" :show-icon="true" data-testid="item-create-hint">
      {{ t('items.createHint') }}
      <NButton
        size="tiny"
        type="primary"
        secondary
        style="margin-left: 8px"
        data-testid="item-go-transactions"
        @click="goTransactions"
      >
        {{ t('items.goTransactions') }}
      </NButton>
    </NAlert>

    <NCard :title="t('items.listTitle')" size="small">
      <NDataTable :columns="columns" :data="itemsStore.items" :bordered="false" size="small">
        <template #empty>
          <span data-testid="item-empty-guide">
            {{ t('items.emptyGuide') }}
          </span>
        </template>
      </NDataTable>
    </NCard>

    <!-- 编辑弹窗（issue #117）：币种不可改，沿用行内币种。
         显示由「意图非空」派生（无独立 show 布尔），关闭（✕ / ESC / 取消 / 提交成功）
         统一经工厂清回 null 终态；序号作表单 key 强制重建（ADR-0072）。 -->
    <AppModal
      :show="editIntent !== null"
      preset="card"
      :title="t('items.edit.title')"
      style="width: 440px"
      data-testid="item-edit-modal"
      @update:show="(v: boolean) => (v ? undefined : closeEdit())"
    >
      <NForm
        v-if="editIntent"
        :key="editSeq"
        label-placement="left"
        :show-feedback="false"
        size="small"
      >
        <NFormItem :label="t('items.edit.label.name')">
          <NInput v-model:value="editName" :placeholder="t('items.edit.placeholder.name')" />
        </NFormItem>
        <NFormItem :label="t('items.edit.label.purchaseDate')">
          <AppDatePicker
            v-model:formatted-value="editPurchaseDate"
            :disabled="editRelinking"
            type="date"
            value-format="yyyy-MM-dd"
            style="width: 160px"
          />
        </NFormItem>
        <NFormItem :label="t('items.edit.label.totalCost')">
          <NInput
            v-model:value="editCostYuan"
            :disabled="editRelinking"
            :placeholder="t('items.edit.placeholder.totalCost')"
            style="width: 160px"
          />
        </NFormItem>
        <NFormItem :label="t('items.edit.label.linkedTx')">
          <PinyinSelect
            v-model:value="editLinkTxId"
            :options="linkTxOptions()"
            :placeholder="t('items.edit.placeholder.linkedTx')"
            @update:value="applyLinkedTxToEdit"
          />
        </NFormItem>
        <NFormItem :label="t('items.edit.label.currency')">
          <span>{{ editIntent.row.currency_code }}{{ t('items.edit.currencyFixed') }}</span>
        </NFormItem>
        <NFormItem :label="t('items.edit.label.note')">
          <NInput v-model:value="editNote" :placeholder="t('items.edit.placeholder.note')" />
        </NFormItem>
        <NSpace justify="end">
          <NButton @click="closeEdit">{{ t('items.rowActions.cancel') }}</NButton>
          <NButton type="primary" @click="saveEdit">{{ t('items.rowActions.save') }}</NButton>
        </NSpace>
      </NForm>
    </AppModal>

    <!-- 处置弹窗（issue #120）：处置日期必填，残值可选；已处置物品可修正处置信息 -->
    <AppModal
      :show="disposing !== null"
      preset="card"
      :title="disposing?.status === 'in_use' ? t('items.dispose.titleInUse') : t('items.dispose.titleInfo')"
      style="width: 400px"
      data-testid="item-dispose-modal"
      @update:show="(v: boolean) => (v ? undefined : closeDispose())"
    >
      <NForm v-if="disposing" label-placement="left" :show-feedback="false" size="small">
        <NFormItem :label="t('items.dispose.label.item')">
          <span>{{ disposing.name }}</span>
        </NFormItem>
        <NFormItem :label="t('items.dispose.label.date')">
          <AppDatePicker
            v-model:formatted-value="disposeDate"
            type="date"
            value-format="yyyy-MM-dd"
            style="width: 160px"
          />
        </NFormItem>
        <NFormItem :label="t('items.dispose.label.residual')">
          <NInput
            v-model:value="disposeResidualYuan"
            :placeholder="t('items.dispose.residualPlaceholder')"
            style="width: 160px"
          />
        </NFormItem>
        <NSpace justify="end">
          <NButton @click="closeDispose">{{ t('items.rowActions.cancel') }}</NButton>
          <NButton
            type="primary"
            data-testid="item-dispose-confirm"
            @click="confirmDispose"
          >
            {{ disposing.status === 'in_use' ? t('items.dispose.confirm') : t('items.rowActions.save') }}
          </NButton>
        </NSpace>
      </NForm>
    </AppModal>

    <!-- 详情弹窗（issue #117）：展示成本分解 = 分子 ÷ 已用天数 = 每天成本 -->
    <AppModal
      :show="detail !== null"
      preset="card"
      :title="t('items.detail.title')"
      style="width: 480px"
      data-testid="item-detail-modal"
      @update:show="(v: boolean) => (v ? undefined : (detail = null))"
    >
      <NDescriptions v-if="detail" :column="1" size="small" label-placement="left" bordered>
        <NDescriptionsItem :label="t('items.detail.label.name')">{{ detail.name }}</NDescriptionsItem>
        <NDescriptionsItem :label="t('items.detail.label.status')">
          {{ detail.status === 'in_use' ? t('items.status.inUse') : t('items.status.disposed') }}
        </NDescriptionsItem>
        <NDescriptionsItem :label="t('items.detail.label.purchaseDate')">{{ detail.purchase_date }}</NDescriptionsItem>
        <NDescriptionsItem v-if="detail.status === 'disposed'" :label="t('items.detail.label.disposalDate')">
          {{ detail.disposal_date }}
        </NDescriptionsItem>
        <NDescriptionsItem v-if="detail.status === 'disposed'" :label="t('items.detail.label.residual')">
          {{
            detail.residual_value_cents != null
              ? detailAmount(detail.residual_value_cents)
              : '—'
          }}
        </NDescriptionsItem>
        <NDescriptionsItem :label="t('items.detail.label.totalCost')">
          {{ detailAmount(detail.total_cost_cents) }}{{ t('items.currencySuffix', { code: detail.currency_code }) }}
        </NDescriptionsItem>
        <NDescriptionsItem :label="t('items.detail.label.native')">
          {{ formatAmount(detail.cost_native_cents, reference.getCurrency(app.defaultCurrency)) }}{{ t('items.currencySuffix', { code: app.defaultCurrency }) }}
        </NDescriptionsItem>
        <NDescriptionsItem :label="t('items.detail.label.note')">{{ detail.note ?? '—' }}</NDescriptionsItem>
        <NDescriptionsItem :label="t('items.detail.label.linkedTx')">
          {{ detail.purchase_transaction_id ? t('items.detail.linkedYes') : '—' }}
        </NDescriptionsItem>
        <NDescriptionsItem :label="t('items.detail.label.refDate')">
          <AppDatePicker
            :formatted-value="detailRefDate"
            clearable
            type="date"
            value-format="yyyy-MM-dd"
            :placeholder="t('items.detail.refDatePlaceholder')"
            style="width: 160px"
            @update:formatted-value="recalcDetail"
          />
        </NDescriptionsItem>
        <NDescriptionsItem :label="t('items.detail.label.usedDays')">
          {{ t('items.detail.usedDays', { n: detailCostView.days }) }}
        </NDescriptionsItem>
        <NDescriptionsItem :label="t('items.detail.label.perDayBreakdown')">
          {{
            t('items.detail.breakdown', {
              cost: detailAmount(detailCostView.numeratorCents),
              days: detailCostView.days,
              perDay: detailAmount(detailCostView.perDayCents),
            })
          }}
        </NDescriptionsItem>
      </NDescriptions>
    </AppModal>
  </NSpace>
</template>
