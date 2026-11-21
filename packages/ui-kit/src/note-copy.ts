import { useMessage, type MessageApi } from "naive-ui";
import { t } from "@ledger/i18n";
import { errorMessage } from "@ledger/utils/errors";

/**
 * 备注复制统一出口（issue #2189 表级事件委托收口）：剪贴板写入与成功/失败
 * toast 反馈的单一实现点——桌面表格的轻量复制按钮（列渲染函数经表级事件委托
 * 调用）与 NoteCopyButton 组件共用本函数，不出现第二套剪贴板/提示口径。
 *
 * toast 实例由消费方传入（视图 setup 期 useMessage()；组件消费方经 useMessage()
 * 自取，见 NoteCopyButton）：复制动作语义归本函数，消息实例归调用环境。
 */

/** 写剪贴板：成功 resolve(null)，失败 resolve(错误文案)（不抛错，错误信息随 toast 呈现）。 */
export async function writeNoteToClipboard(note: string): Promise<string | null> {
  try {
    await navigator.clipboard.writeText(note);
    return null;
  } catch (e) {
    return errorMessage(e);
  }
}

/** 复制完整备注 + 统一成功/失败 toast（与既有 NoteCopyButton 反馈文案同一 i18n 键）。 */
export async function copyNoteWithToast(message: MessageApi, note: string): Promise<void> {
  const error = await writeNoteToClipboard(note);
  if (error === null) message.success(t("transactions.list.copyNoteOk"));
  else message.error(t("transactions.list.copyNoteFailed", { msg: error }));
}

/** 便捷消费：组件内自取消息实例后委托统一出口（NoteCopyButton 等组件消费方）。 */
export function useCopyNote(): (note: string) => Promise<void> {
  const message = useMessage();
  return (note) => copyNoteWithToast(message, note);
}
