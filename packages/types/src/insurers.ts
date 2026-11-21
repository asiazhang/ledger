import type { Syncable } from "./common";

export interface Insurer extends Syncable {
  id: string;
  name: string;
  is_deleted: boolean;
}

export interface InsurerInput {
  name: string;
}

export interface InsurerUpdateInput {
  name?: string;
}
