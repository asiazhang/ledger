import type { Book, BookListInfo } from "@ledger/types";
import { nextId, str } from "./state";

export const MOCK_BOOK: Book = { id: "mock-book", name: "Mock 账本", dir: "/mock/data/mock-book" };

function requireBook(id: string): Book {
  if (id !== MOCK_BOOK.id) throw new Error(`替身后端：账本不存在 ${id}`);
  return { ...MOCK_BOOK };
}

export function listBooks(): BookListInfo {
  return {
    books: [{ ...MOCK_BOOK }],
    active_id: MOCK_BOOK.id,
    mutable: true,
    fallback_reason: null,
  };
}

export function createBook(args: Record<string, unknown>): Book {
  const id = nextId("mock-book");
  return { id, name: str(args, "name"), dir: `/mock/data/${id}` };
}

export function switchBook(args: Record<string, unknown>): Book {
  return requireBook(str(args, "id"));
}

export function renameBook(args: Record<string, unknown>): Book {
  const book = requireBook(str(args, "id"));
  return { ...book, name: str(args, "name") };
}

export function removeBook(args: Record<string, unknown>): void {
  str(args, "id");
}
