import {
  ArchiveOutline,
  ArrowUndoOutline,
  CreateOutline,
  SwapHorizontalOutline,
  TrashOutline,
} from "@vicons/ionicons5";
import type { DropdownOption } from "naive-ui";
import { t } from "@ledger/i18n";
import { errorOptionProps, renderRowMenuIcon } from "@/components/row-menu-common";

/**
 * 账户行菜单选项组装：纯函数收口，菜单形状可独立测试（与交易行菜单
 * buildRowMenuOptions 同一模式）。
 *
 * 两种形态（spec #1952 / ADR-0142 决策 2「已归档行的动作集收窄为恢复」）：
 * - 未归档行（{@link buildAccountRowMenuOptions}）：编辑（CreateOutline）/ 调整余额
 *   （SwapHorizontalOutline，调整即与黑洞账户的转账，ADR-0026）/ 归档（ArchiveOutline）/
 *   分隔线 / 删除（TrashOutline，着主题 error 色）；
 * - 已归档行（{@link buildArchivedAccountRowMenuOptions}）：只有「恢复」——编辑、
 *   调整余额、删除都要先恢复，避免在一张看不见全貌的卡上改钱。
 *
 * 术语：动作叫「归档 / 恢复」（ADR-0142 决策 7）；不叫「隐藏」（那在本仓库指
 * 黑洞占位与金额掩码），也不沿用储蓄目标的「取消归档」。
 *
 * `errorColor`：当前主题的 error 色（组件经 useThemeVars 取值传入）。
 */
export function buildAccountRowMenuOptions(opts: { errorColor?: string } = {}): DropdownOption[] {
  return [
    { label: t("accounts.menu.edit"), key: "edit", icon: renderRowMenuIcon(CreateOutline) },
    {
      label: t("accounts.menu.adjustBalance"),
      key: "adjust-balance",
      icon: renderRowMenuIcon(SwapHorizontalOutline),
    },
    {
      label: t("accounts.menu.archive"),
      key: "archive",
      icon: renderRowMenuIcon(ArchiveOutline),
    },
    { type: "divider", key: "menu-divider" },
    {
      label: t("accounts.menu.delete"),
      key: "delete",
      icon: renderRowMenuIcon(TrashOutline),
      ...errorOptionProps(opts.errorColor),
    },
  ];
}

/**
 * 已归档账户行菜单选项：单一「恢复」项（回主表与选择面）。独立函数而非
 * 未归档形态的参数分支——两种形态各自单一职责，与储蓄目标「归档列独立列集」
 * 同形；不接 `errorColor`（恢复不是危险动作），签名留 `_opts` 只为与上者对称，
 * 调用方传同款入参不用分支。
 */
export function buildArchivedAccountRowMenuOptions(
  _opts: { errorColor?: string } = {},
): DropdownOption[] {
  return [
    {
      label: t("accounts.menu.restore"),
      key: "restore",
      icon: renderRowMenuIcon(ArrowUndoOutline),
    },
  ];
}
