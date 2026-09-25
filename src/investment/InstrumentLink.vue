<script setup lang="ts">
import { computed } from "vue";
import { useRouter } from "vue-router";
import { useAppStore } from "@/stores/app";
import { accentColor } from "@ledger/theme/overrides";
import { t } from "@ledger/i18n";
import {
  INSTRUMENT_LINK_CLASS,
  INSTRUMENT_PLACEHOLDER_CLASS,
} from "@/investment/instrument-link.css";

/**
 * 可点击标的代码（持仓下钻，ADR-0135 决策 6 / ADR-0107 决策 4 修订）：点击跳投资页
 * 明细页签按标的过滤——持仓页签行携带 accountId（`/investments?tab=detail&account=&instrument=`，
 * 该账户该标的的投资交易历史）；不携带 accountId（已清仓标的形态）落同一明细页签、
 * 不带账户，可见该标的全部历史交易（含卖出）。
 *
 * 视觉与交互同 AccountLink / MerchantLink 先例：主题强调色文字、hover 提亮 +
 * 下划线 + 微亮背景；用真实 <button> 保证键盘可达（Tab 聚焦 + Enter 触发）。
 *
 * 标的不在参考数据字典（无 instrumentMap 可查），组件不校验 id 存在性——标的字典
 * 无软删（被流水引用的标的不可删），跳转恒可达、不提供落空的跳转（决策 5）。
 */
const props = defineProps<{
  /** 目标标的 id（跳转载荷核心） */
  instrumentId: string;
  /** 展示文本（标的代码）；空值渲染纯文本「-」，不可点击 */
  label: string | null;
  /** 可选同游账户 id：在场时跳转载荷带 ?account=（现仅持仓页签行场景） */
  accountId?: string | null;
}>();

const router = useRouter();
const app = useAppStore();

// 强调色与 AccountLink / MerchantLink 同源：@ledger/theme accentColor 选择器按主题解析
// （值源：overrides common 单一来源）。
const accent = computed(() => accentColor(app.theme));

// 持仓下钻改址（ADR-0135 决策 6 / ADR-0107 决策 4 修订）：落投资页明细页签
// ?tab=detail&account=&instrument=——交易页 ?instrument= 维度随投资 kind 行迁出
// 主列表而退役（ADR-0107 修订注记），标的历史交易在明细页签重获呈现面。
function go() {
  router.push({
    name: "investments",
    query: props.accountId
      ? { tab: "detail", account: props.accountId, instrument: props.instrumentId }
      : { tab: "detail", instrument: props.instrumentId },
  });
}
</script>

<template>
  <button
    v-if="label"
    type="button"
    :class="INSTRUMENT_LINK_CLASS"
    :title="t('common.link.viewInstrument')"
    :style="{ color: accent.base, '--accent-hover': accent.hover }"
    @click="go"
  >
    {{ label }}
  </button>
  <span v-else :class="INSTRUMENT_PLACEHOLDER_CLASS">-</span>
</template>
