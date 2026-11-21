import { computed, onMounted, ref } from "vue";
import { useMessage } from "naive-ui";
import { api } from "@ledger/api";
import { t } from "@ledger/i18n";
import { errorMessage } from "@ledger/utils/errors";
import { useLoadable } from "@ledger/loadable";
import { restartAppShortly } from "@/backup/restart";
import { useAppDialog } from "@/composables/useAppDialog";
import { useModalIntent } from "@ledger/modal-intent";
import type { Book } from "@ledger/types";

export type BookNameIntent =
  | {
      mode: "create";
    }
  | {
      mode: "rename";
      book: Book;
    };

export function useBookSwitcher() {
  const message = useMessage();
  const dialog = useAppDialog();

  const books = ref<Book[]>([]);
  const activeId = ref<string | null>(null);
  const mutable = ref(true);
  const fallbackReason = ref<string | null>(null);
  const switching = ref(false);

  const { loading, error, run: runList } = useLoadable(() => api.listBooks());
  const loadFailed = computed(() => error.value !== null);

  const activeBook = computed(() => books.value.find((b) => b.id === activeId.value) ?? null);

  async function refresh(): Promise<void> {
    const info = await runList();
    if (info === null) return;
    books.value = info.books;
    activeId.value = info.active_id;
    mutable.value = info.mutable;
    fallbackReason.value = info.fallback_reason;
  }

  onMounted(() => {
    void refresh();
  });

  async function createBook(name: string): Promise<boolean> {
    try {
      const book = await api.createBook(name);
      message.success(t("books.toasts.created", { name: book.name }));
      await refresh();
      return true;
    } catch (e) {
      message.error(errorMessage(e));
      return false;
    }
  }

  async function renameBook(id: string, name: string): Promise<boolean> {
    try {
      const book = await api.renameBook(id, name);
      message.success(t("books.toasts.renamed", { name: book.name }));
      await refresh();
      return true;
    } catch (e) {
      message.error(errorMessage(e));
      return false;
    }
  }

  async function removeBook(book: Book): Promise<boolean> {
    try {
      await api.removeBook(book.id);
      message.success(t("books.toasts.removed", { name: book.name }));
      await refresh();
      return true;
    } catch (e) {
      message.error(errorMessage(e));
      return false;
    }
  }

  function requestSwitch(book: Book): void {
    if (book.id === activeId.value || !mutable.value || switching.value) return;
    dialog.info({
      title: t("books.switchDialog.title"),
      content: t("books.switchDialog.content", { name: book.name }),
      positiveText: t("books.switchDialog.confirm"),
      negativeText: t("books.switchDialog.cancel"),
      maskClosable: false,
      onPositiveClick: () => {
        void performSwitch(book);
      },
    });
  }

  async function performSwitch(book: Book): Promise<void> {
    switching.value = true;
    try {
      await api.switchBook(book.id);
      message.success(t("books.toasts.switching", { name: book.name }));
      closePanel();
      restartAppShortly();
    } catch (e) {
      message.error(errorMessage(e));
    } finally {
      switching.value = false;
    }
  }

  const panelShow = ref(false);

  function closePanel(): void {
    panelShow.value = false;
  }

  const nameIntent = useModalIntent<BookNameIntent>();
  const nameDraft = ref("");
  const nameBusy = ref(false);

  const nameModalTitle = computed(() =>
    nameIntent.intent.value?.mode === "rename"
      ? t("books.renameModal.title")
      : t("books.createModal.title"),
  );
  const nameModalConfirmText = computed(() =>
    nameIntent.intent.value?.mode === "rename"
      ? t("books.renameModal.confirm")
      : t("books.createModal.confirm"),
  );

  function openCreate(): void {
    nameDraft.value = "";
    nameIntent.open({ mode: "create" });
  }

  function openRename(book: Book): void {
    nameDraft.value = book.name;
    nameIntent.open({ mode: "rename", book });
  }

  async function submitName(): Promise<void> {
    const intent = nameIntent.intent.value;
    const name = nameDraft.value.trim();
    if (!intent || !name || nameBusy.value) return;
    nameBusy.value = true;
    const ok =
      intent.mode === "create" ? await createBook(name) : await renameBook(intent.book.id, name);
    nameBusy.value = false;
    if (ok) nameIntent.close();
  }

  return {
    books,
    activeId,
    activeBook,
    mutable,
    fallbackReason,
    loading,
    loadFailed,
    refresh,
    requestSwitch,
    panelShow,
    closePanel,
    nameIntent,
    nameDraft,
    nameBusy,
    nameModalTitle,
    nameModalConfirmText,
    openCreate,
    openRename,
    submitName,
    removeBook,
  };
}
