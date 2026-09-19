<script setup lang="ts">
import { computed } from "vue";
import { NButton, NIcon } from "naive-ui";
import { InformationCircleOutline } from "@vicons/ionicons5";
import { t } from "@ledger/i18n";
import ConceptTipHost from "@/investment/ConceptTipHost.vue";
import {
  CONCEPT_SCOPE_KEY_SUFFIX,
  type ConceptKey,
  type ConceptScope,
} from "@/investment/concept-tips";
import { conceptLabel } from "./concept-label.css.ts";

/**
 * 投资域口径说明标签（issue #1369）：标签 + 常驻 `ⓘ`，指针轴悬停即现
 * （裸 NTooltip）、触控轴点按弹出（经 AppPopover 入弹层注册表），两轴文案同源。
 *
 * 入参是**概念键**（`ConceptKey` 闭集，拼错即编译期报错）而不是文案字符串：tip
 * 一律按 `investments.concepts.<concept>Tip` 现取（ADR-0049 文案按域归口），调用方
 * 给不出第二份措辞——「文案唯一源」由此成为结构约束而非评审纪律。标签仍由调用方
 * 传入（本表位命名空间），因为同一口径在不同表位的展示词可以不同（合计三卡
 * 「总市值」与持仓表「市值」是同一口径、两个标签）；aria 由标签拼出，读屏用户听到
 * 的是他在屏幕上看到的那个词。
 *
 * 作用域变体（`scope`）对**语境相关**的挂点是必须项：同一份概念文案在持仓页是
 * **过滤后子集**、在首页是**全量**、在跨账本页是**逐本折算合并**，作用域句因此不能
 * 写进概念文案本身，否则必有一处失真。单行值不随筛选变化、或口径本身自带「不随
 * 筛选收窄」属性时（资金加权收益率按完整历史计算）不挂变体——所以 prop 可选，而
 * 带合计/全账语义的消费方（PortfolioStatsCards）在自己的 props 上把它声明为必填。
 *
 * 拼接经 i18n 模板（`concepts.tipTemplate`）而非字符串相加：中文句号后不接空格、
 * 英文句号后要接空格，分隔是语言事实，属于文案资源（ADR-0049）。
 *
 * 与 ADR-0035 的关系：commit bb2c2239 曾裁定「不建 AppTooltip 封装（Speculative
 * Generality）」，其前提是单站点、单一悬停语义。本组件封装的是**双轴形态 + 文案
 * 单源**，站点数已达个位数，该裁定不再适用；ADR-0035 正文（弹层注册表枚举）不变——
 * 触控轴仍走注册过的 AppPopover，裸 NTooltip 仍是注册表外的悬停件。
 * 组件落域内而非 ui-kit：ui-kit 是成员闭集（ADR-0118 决策 5），双轴件先例
 * AmountCell 同样住域内（src/transaction/AmountCell.vue）。
 */

const props = defineProps<{
  /** 展示标签（取自本表位命名空间的现有键，如 holdings.columns.cost） */
  label: string;
  /** 概念键：tip 取 `investments.concepts.<concept>Tip`（闭集，见 concept-tips.ts） */
  concept: ConceptKey;
  /** 作用域变体句；口径不随页面语境变化时省略 */
  scope?: ConceptScope;
  /** `ⓘ` 触发器的 data-testid（省略即不挂测试钩子） */
  testId?: string;
}>();

const ariaLabel = computed(() => t("investments.concepts.tipAria", { concept: props.label }));

const tip = computed(() => {
  const body = t(`investments.concepts.${props.concept}Tip`);
  if (!props.scope) return body;
  const suffix = CONCEPT_SCOPE_KEY_SUFFIX[props.scope];
  return t("investments.concepts.tipTemplate", {
    body,
    scope: t(`investments.concepts.scope${suffix}`),
  });
});
</script>

<template>
  <span :class="conceptLabel">
    <!-- 概念名自成一个元素：与触发器之间的纯空白节点被编译器去除，标签文案不因
         加图标而多出空格（消费方 .text() 逐字口径不变，合计三卡先例） -->
    <span>{{ label }}</span>
    <ConceptTipHost :text="tip">
      <template #default="{ isTouch }">
        <NButton
          text
          :class="isTouch ? 'touch-hit-area' : undefined"
          :style="isTouch ? { '--touch-hit-inset': '-10px -10px' } : undefined"
          :aria-label="ariaLabel"
          :data-testid="testId ? `${testId}-info` : undefined"
        >
          <NIcon :size="14" color="var(--n-label-text-color, #999)">
            <InformationCircleOutline />
          </NIcon>
        </NButton>
      </template>
    </ConceptTipHost>
  </span>
</template>
