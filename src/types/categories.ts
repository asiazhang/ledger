import type { Syncable } from './common'

export type CategoryKind = 'income' | 'expense'

export interface Category extends Syncable {
  id: string
  name: string
  kind: CategoryKind
  parent_id: string | null
  icon: string | null
  sort_order: number
  created_at: string
}

export interface CategoryInput {
  name: string
  kind: CategoryKind
  parent_id?: string | null
  icon?: string | null
}

export interface CategoryUpdateInput {
  name?: string
  icon?: string | null
  parent_id?: string | null
}

export interface ReorderItem {
  id: string
  sort_order: number
}
