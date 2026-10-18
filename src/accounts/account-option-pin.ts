import { t } from "@ledger/i18n";
import type { Account } from "@ledger/types";

/** 账户下拉选项形状（与参考 store 的候选面投影、各表单既有选项同形）。 */
export interface AccountOption {
  label: string;
  value: string;
}

/**
 * 编辑面补值：当前已选值是归档账户 → 把它补回选项头部并标「已归档」
 * （spec #1952 / ADR-0142 决策 4 的「共享小工具」）。
 *
 * 为什么需要：归档账户已从全部**新选择面**排除（参考 store 的 selectable* 派生），
 * 但编辑既有流水 / 既有计划 / 已筛选会话值时，值本身仍可能指向归档账户。
 * 不补值的后果是 select 露出裸 uuid，或被静默改挂到别的账户——两者都不可接受。
 * 补值是**只补不选**：值原样保留（保持选中），只在选项里把可读名字 + 标记补回来。
 *
 * 形态照既有先例（`useMerchantField` 的软删商户兜底、`useCategoryForm` 的软删保单
 * 兜底、`useInvestmentForm` 的 pinInstrumentOption）：缺则 `unshift` 到头部保证可见，
 * 已在则去重不重复。判定条件是「值 ∈ 账户字典且 `is_archived`」——非归档、未选、
 * 字典查不到时**原数组原样返回**（不复制、零开销），故每条下拉零额外分配。
 *
 * 术语（ADR-0142 决策 7）：标记用「已归档」，不写「隐藏」（那在本仓库指黑洞占位
 * 与金额掩码），也不沿用储蓄目标的「取消归档」。
 */
export function pinArchivedAccountOption(
  options: AccountOption[],
  currentId: string | null | undefined,
  accountMap: ReadonlyMap<string, Account>,
): AccountOption[] {
  if (!currentId) return options;
  const account = accountMap.get(currentId);
  if (!account?.is_archived) return options;
  if (options.some((o) => o.value === currentId)) return options;
  return [
    { label: `${account.name}${t("accounts.archived.suffix")}`, value: currentId },
    ...options,
  ];
}
