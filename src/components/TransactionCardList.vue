<script setup lang="ts">
import { NButton, NTag } from 'naive-ui'
import type { Transaction } from '@/types'
import { formatAmount } from '@/utils/money'
import { useReferenceStore } from '@/stores/reference'
import { useAppStore } from '@/stores/app'
import { kindSemanticColor } from '@/theme/semantic-colors'
import { t } from '@/i18n'
import MerchantLink from '@/components/MerchantLink.vue'
import SourceLink from '@/components/SourceLink.vue'
import AmountCell from '@/components/AmountCell.vue'
import { kindLabel, KIND_TAG_TYPE, displayAmountCents, renderAccountCell } from '@/components/transaction-columns'
import {
  TRANSACTION_CARD_LIST_CLASS,
  TRANSACTION_CARD_CLASS,
  CARD_MENU_CLASS,
} from './transaction-card-list.css.ts'

/**
 * 移动档交易卡片列表（issue #846 / ADR-0088 决策 9 断点双渲染）：
 * 交易列表与搜索结果共享的移动档列表形态（第二消费方同构复用），纯展示组件、
 * 不持列表状态——数据与翻页归调用方视图（请求发起与列表数据归视图，ADR-0030
 * 决策 6），过滤语义零变化由调用方沿用既有 TransactionFilter 接缝保证。
 *
 * 字段按票⑥范围：日期、类型（表格列同源派生标签）、分类/商户（单源解析）、
 * 账户（转账行「转出 → 转入」双向链接，renderAccountCell 同一渲染）、金额
 * （formatAmount + kindSemanticColor 口径不变——金额隐私模式在格式化层收口，
 * 隐藏数字不隐藏形状与方向，语义色保留）；来源行保留链接语义（SourceLink 复用）。
 *
 * 触控语义：
 * - 卡片「⋯」与桌面右键共用同一 RowContextMenu open 入口（openRowMenu 可选——
 *   传入才渲染，搜索结果只读不传，与桌面表格操作列同构）；
 * - 整卡点击 = 行激活（activateRow 可选，交易页 = 编辑或只读详情；refund 两类都不开放
 *   的判定归调用方，ADR-0106 决策 10 / #1048）；
 *   卡内链接、「⋯」与金额触发器阻断冒泡，不连带整卡激活；
 * - 金额全文点按查看归 AmountCell（触控轴气泡，悬停一击可达同规）。
 *
 * 桌面档不渲染本组件（调用方按窗口分级分支），桌面表格一字不动。
 */

const props = defineProps<{
  /** 当前页行集（调用方视图持有） */
  rows: Transaction[]
  /** 卡片「⋯」打开回调（与行右键同一 RowContextMenu open 入口）；缺省不渲染「⋯」（搜索结果只读） */
  openRowMenu?: (event: MouseEvent, row: Transaction) => void
  /** 整卡点击回调（交易页 = 编辑或只读详情，判定归调用方）；缺省整卡点击无动作 */
  activateRow?: (row: Transaction) => void
}>()

const reference = useReferenceStore()
const app = useAppStore()

/** 账户单元格（转账双向 / 单账户）与表格列同一渲染函数；经功能组件进模板。 */
const AccountCell = (cellProps: { row: Transaction }) => renderAccountCell(cellProps.row)
AccountCell.props = { row: { type: Object, required: true } }

/** 分类路径：与表格分类列同一单源解析（未知 id 回退 '-'）。 */
function categoryText(row: Transaction): string {
  return row.category_id ? reference.categoryPath(row.category_id) || '-' : '-'
}

/** 分类/商户合并行是否渲染缺省「-」（两者皆空，与表格两列各自 '-' 的口径一致）。 */
function hasMeta(row: Transaction): boolean {
  return row.category_id !== null || row.merchant_id !== null
}

/** 金额文案与语义色（formatAmount / kindSemanticColor 口径不变，归其单点）；
 * 转换行金额读展示口径单点（转出金额，与表格金额列同源）。 */
function amountText(row: Transaction): string {
  return formatAmount(displayAmountCents(row), reference.getCurrency(row.currency_code))
}
function amountColor(row: Transaction): string {
  return kindSemanticColor(row.kind, app.theme)
}

/** 整卡点击 = 行激活：交互元素（链接/按钮/金额触发器）冒泡已在各行阻断，这里只收空地点击。 */
function onCardClick(row: Transaction): void {
  props.activateRow?.(row)
}
</script>

<template>
  <div :class="TRANSACTION_CARD_LIST_CLASS">
    <div
      v-for="row in rows"
      :key="row.id"
      :class="TRANSACTION_CARD_CLASS"
      @click="onCardClick(row)"
    >
      <div class="transaction-card-head">
        <span class="transaction-card-date">{{ row.date }}</span>
        <NTag :type="KIND_TAG_TYPE[row.kind]">{{ kindLabel(reference, row) }}</NTag>
        <NButton
          v-if="openRowMenu"
          size="tiny"
          quaternary
          :class="CARD_MENU_CLASS"
          class="row-actions-btn touch-hit-area"
          :aria-label="t('transactions.menu.actions')"
          @click.stop="openRowMenu($event, row)"
        >
          ⋯
        </NButton>
      </div>
      <!-- 分类/商户行（链接语义保留：商户可点击下钻）；整行阻断冒泡不触发整卡编辑 -->
      <div class="transaction-card-row" @click.stop>
        <template v-if="hasMeta(row)">
          <span v-if="row.category_id">{{ categoryText(row) }}</span>
          <span v-if="row.category_id && row.merchant_id" class="transaction-card-join">·</span>
          <MerchantLink v-if="row.merchant_id" :merchant-id="row.merchant_id" />
        </template>
        <span v-else class="transaction-card-empty">-</span>
      </div>
      <!-- 账户行：转账「转出 → 转入」双向链接与表格同一渲染（阻断冒泡） -->
      <div class="transaction-card-row" @click.stop>
        <AccountCell :row="row" />
      </div>
      <!-- 两腿标的行（ADR-0099 / issue #979）：转换行显示「A → B」——A（转出标的）
           经来源列同一 SourceLink 渲染（security_transactions.instrument_id 反查），
           B 显示转入标的代码；无来源（防御）时退化为转入标的单腿。
           转换行不重复渲染下方来源行（同一转出标的） -->
      <div
        v-if="row.kind === 'convert' && row.convert"
        class="transaction-card-row"
        @click.stop
      >
        <SourceLink v-if="row.source" :source="row.source" />
        <span v-if="row.source" class="transaction-card-join">→</span>
        <span>{{ row.convert.to_symbol }}</span>
      </div>
      <!-- 来源行：保留链接语义（来源列词条）；无来源不占行 -->
      <div v-else-if="row.source" class="transaction-card-row" @click.stop>
        <SourceLink :source="row.source" />
      </div>
      <!-- 金额行：AmountCell 承载语义色与触控轴全文点按（阻断冒泡） -->
      <div class="transaction-card-amount" @click.stop>
        <AmountCell :text="amountText(row)" :color="amountColor(row)" />
      </div>
    </div>
  </div>
</template>
