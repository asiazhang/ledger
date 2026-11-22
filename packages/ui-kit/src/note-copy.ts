import { useMessage, type MessageApi } from "naive-ui";
import { t } from "@ledger/i18n";
import { errorMessage } from "@ledger/utils/errors";

export async function writeNoteToClipboard(note: string): Promise<string | null> {
  try {
    await navigator.clipboard.writeText(note);
    return null;
  } catch (e) {
    return errorMessage(e);
  }
}

export async function copyNoteWithToast(message: MessageApi, note: string): Promise<void> {
  const error = await writeNoteToClipboard(note);
  if (error === null) message.success(t("transactions.list.copyNoteOk"));
  else message.error(t("transactions.list.copyNoteFailed", { msg: error }));
}

export function useCopyNote(): (note: string) => Promise<void> {
  const message = useMessage();
  return (note) => copyNoteWithToast(message, note);
}
