<script setup lang="ts">
import { ref } from "vue";
import { NButton, NIcon } from "naive-ui";
import { AddOutline } from "@vicons/ionicons5";
import type { CreateTransactionKind } from "@ledger/types";
import { t } from "@ledger/i18n";
import AppPopover from "@ledger/ui-kit/AppPopover.vue";
import {
  CREATE_FAB_CLASS,
  CREATE_FAB_SHEET_CLASS,
  CREATE_FAB_OPTION_CLASS,
} from "./create-fab.css.ts";

/**
 * 记一笔悬浮按钮（Create FAB，issue #846 / ADR-0088 决策 5 / 词汇表「记一笔悬浮按钮」）：
 * 移动档交易页右下的记一笔常驻入口，与顶栏按钮（桌面档）、记一笔快捷键（指针轴
 * 裸键）同族并列的第三形态入口——触控轴移动档下快捷键不绑，本按钮是唯一记一笔入口。
 *
 * 二击进表单、零表单内部改造：点开大号类型选择（由调用方传入的可用类型清单，
 * 交易页创建闭集单源 availableCreateKinds：支出/收入/转账，ADR-0135 决策 5 /
 * issue #1782 收窄后买卖不在交易页入口；refund 不在入口、借贷变体是桌面下拉专属），
 * 经弹窗意图编排的记一笔意图（携带类型，openCreate → useTransactionModalState.open）
 * 进入对应表单；意图编排归调用方视图，本组件只报选中类型。
 *
 * 类型清单由调用方传入（创建闭集判定源，见 utils/create-entry-kinds）：本组件不自行
 * 维护第二份类型清单（一处生效两处）。
 *
 * 类型选择是独立轻弹层：经 AppPopover 入弹层注册表（ADR-0035，开合期间快捷键
 * 照常抑制）；受控开合——选中即关闭，点外部关闭（非模态家族）与系统返回「关
 * 最上层」照常（useOverlayReporting 关闭请求中继）。
 */

const props = defineProps<{
  /** 可选新建类型（清单序即渲染序）：创建闭集判定源 availableCreateKinds 的投影。 */
  kinds: readonly CreateTransactionKind[];
}>();

const emit = defineEmits<{
  /** 选中一个可创建类型（清单见 kinds prop，创建闭集判定源过滤） */
  select: [kind: CreateTransactionKind];
}>();

const show = ref(false);

/** 选中即关闭并上报（表单由调用方经既有意图编排开启）。 */
function pick(kind: CreateTransactionKind): void {
  show.value = false;
  emit("select", kind);
}
</script>

<template>
  <AppPopover trigger="click" placement="top-end" :show="show" @update:show="show = $event">
    <template #trigger>
      <NButton
        :class="CREATE_FAB_CLASS"
        circle
        type="primary"
        :aria-label="t('transactions.create.button')"
      >
        <NIcon :component="AddOutline" />
      </NButton>
    </template>
    <div :class="CREATE_FAB_SHEET_CLASS" role="menu" :aria-label="t('transactions.create.title')">
      <NButton
        v-for="kind in props.kinds"
        :key="kind"
        :class="CREATE_FAB_OPTION_CLASS"
        size="large"
        role="menuitem"
        block
        @click="pick(kind)"
      >
        {{ t(`transactions.kind.${kind}`) }}
      </NButton>
    </div>
  </AppPopover>
</template>
