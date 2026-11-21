import { beforeEach, describe, expect, it } from "vitest";
import { installMockBackend } from "./index";
import { invoke } from "./tauri/core";
import type { Book, BookListInfo } from "@ledger/types";

beforeEach(() => {
  installMockBackend();
});

describe("账本注册表：单账本常量态", () => {
  it("list_books 答单账本清单：可变更、无回退", async () => {
    const info = await invoke<BookListInfo>("list_books");
    expect(info.books).toHaveLength(1);
    expect(info.books[0]).toMatchObject({ name: expect.any(String), dir: expect.any(String) });
    expect(info.active_id).toBe(info.books[0].id);
    expect(info.mutable).toBe(true);
    expect(info.fallback_reason).toBeNull();
  });

  it("create_book 收名答新建账本回执（形状：id/name/dir），注册表保持单账本", async () => {
    const book = await invoke<Book>("create_book", { name: "新账本" });
    expect(book.name).toBe("新账本");
    expect(book.id.length).toBeGreaterThan(0);
    expect(book.dir.length).toBeGreaterThan(0);
    const info = await invoke<BookListInfo>("list_books");
    expect(info.books).toHaveLength(1);
  });

  it("switch_book 对清单内账本答目标账本，未知 id 报错（存在性校验）", async () => {
    const { books } = await invoke<BookListInfo>("list_books");
    const book = await invoke<Book>("switch_book", { id: books[0].id });
    expect(book.id).toBe(books[0].id);

    await expect(invoke<Book>("switch_book", { id: "no-such-book" })).rejects.toThrow(
      /no-such-book/,
    );
  });

  it("rename_book 收新名答改名回执，未知 id 报错", async () => {
    const { books } = await invoke<BookListInfo>("list_books");
    const book = await invoke<Book>("rename_book", { id: books[0].id, name: "改名后" });
    expect(book.name).toBe("改名后");
    expect(book.id).toBe(books[0].id);

    await expect(invoke<Book>("rename_book", { id: "no-such-book", name: "x" })).rejects.toThrow(
      /no-such-book/,
    );
  });

  it("remove_book 答 void（单账本即活动账本，不模拟移除）", async () => {
    const { books } = await invoke<BookListInfo>("list_books");
    await expect(invoke<void>("remove_book", { id: books[0].id })).resolves.toBeUndefined();
    const info = await invoke<BookListInfo>("list_books");
    expect(info.books).toHaveLength(1);
  });

  it("create_book 缺 name 以含参数名的错误拒绝", async () => {
    await expect(invoke<Book>("create_book", {})).rejects.toThrow(/参数 name/);
  });
});
