import type { Syncable } from "./common";

export interface Merchant extends Syncable {
  id: string;
  name: string;
  is_deleted: boolean;
}

export interface MerchantInput {
  name: string;
}

export interface MerchantTransactionCount {
  merchant_id: string;
  transaction_count: number;
}

export interface MerchantUpdateInput {
  name?: string;
}
