import type { Book, BookListInfo } from "@ledger/types";
import { nextId, str } from "./state";

/**
 * 账本注册表域替身（issue #1966 / spec #1964 / ADR-0143 决策 1）：环境壳不
 * 模拟真实注册表——恒为**单账本常量态**：清单恒一本（可变更、无回退，切换器
 * 与备份/数据位置卡片的活动账本取数可用）；新建/切换/改名/移除答形状正确
 * 的常量回执（Book：id/name/dir）而不真正改注册表——多账本世界的登记、
 * 活动指针与原位重引导都不模拟，浏览器开发模式只服务单账本界面迭代。
 */

/** 单账本常量：mock 模式的唯一注册账本（活动账本即它）；跨账本投资汇总的
 *  books 行同源取用（issue #1976，单行 included）。 */
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
  // 答新建回执（会话内递增 id + 目录推导），但不落注册表（常量态）
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
  // 单账本即活动账本（真后端必拒）；mock 保流程可导航直答 void，注册表不变
  str(args, "id");
}
