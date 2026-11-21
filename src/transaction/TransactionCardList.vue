<script setup lang="ts">
import { NButton, NTag } from "naive-ui";
import type { Transaction, TransactionPurchase } from "@ledger/types";
import { useReferenceStore } from "@/stores/reference";
import { useAppStore } from "@/stores/app";
import { kindSemanticColor } from "@ledger/theme/semantic-colors";
import { t } from "@ledger/i18n";
import MerchantLink from "@/merchants/MerchantLink.vue";
import SourceLink from "@/transaction/SourceLink.vue";
import AmountCell from "@/transaction/AmountCell.vue";
import {
  kindLabel,
  KIND_TAG_TYPE,
  displayAmountText,
  renderAccountCell,
  renderPurchaseLine,
} from "@/transaction/transaction-columns";
import {
  TRANSACTION_CARD_LIST_CLASS,
  TRANSACTION_CARD_CLASS,
  CARD_MENU_CLASS,
} from "./transaction-card-list.css.ts";

const props = defineProps<{
  rows: Transaction[];
  openRowMenu?: (event: MouseEvent, row: Transaction) => void;
  activateRow?: (row: Transaction) => void;
}>();

const reference = useReferenceStore();
const app = useAppStore();

const AccountCell = (cellProps: { row: Transaction }) => renderAccountCell(cellProps.row);
AccountCell.props = { row: { type: Object, required: true } };

const PurchaseLine = (cellProps: { item: TransactionPurchase }) =>
  renderPurchaseLine(cellProps.item);
PurchaseLine.props = { item: { type: Object, required: true } };

function categoryText(row: Transaction): string {
  return row.category_id ? reference.categoryPath(row.category_id) || "-" : "-";
}

function hasMeta(row: Transaction): boolean {
  return row.category_id !== null || row.merchant_id !== null;
}

function amountText(row: Transaction): string {
  return displayAmountText(reference, row);
}
function amountColor(row: Transaction): string {
  return kindSemanticColor(row.kind, app.theme);
}

function onCardClick(row: Transaction): void {
  props.activateRow?.(row);
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
      <div v-if="row.kind === 'convert' && row.convert" class="transaction-card-row" @click.stop>
        <SourceLink v-if="row.source" :source="row.source" />
        <span v-if="row.source" class="transaction-card-join">→</span>
        <span>{{ row.convert.to_symbol }}</span>
      </div>
      <!-- 来源行：保留链接语义（来源列词条）；无来源不占行 -->
      <div v-else-if="row.source" class="transaction-card-row" @click.stop>
        <SourceLink :source="row.source" />
      </div>
      <!-- 订单号不在卡片呈现（#1928 / ADR-0138 修订注记：撤行尾徽章，出口在详情订单区） -->
      <!-- 购买项块（issue #1884 / ADR-0138 决策 15）：卡内逐行列出购买项（商品名 + 共 N 件，
           列表侧渲染单点全文形态）；一卡一笔交易不变、金额仍只出现一次。行文本无交互，
           点击冒泡归整卡激活（详情是长商品名的全文出口）；无购买项不占行（零变化） -->
      <template v-if="row.purchases.length > 0">
        <div
          v-for="(item, i) in row.purchases"
          :key="i"
          class="transaction-card-row transaction-card-purchase"
        >
          <PurchaseLine :item="item" />
        </div>
      </template>
      <!-- 金额行：AmountCell 承载语义色与触控轴全文点按（阻断冒泡） -->
      <div class="transaction-card-amount" @click.stop>
        <AmountCell :text="amountText(row)" :color="amountColor(row)" />
      </div>
    </div>
  </div>
</template>
