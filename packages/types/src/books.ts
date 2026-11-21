export interface Book {
  id: string;
  name: string;
  dir: string;
}

export interface BookListInfo {
  books: Book[];
  active_id: string | null;
  mutable: boolean;
  fallback_reason: string | null;
}
