import { beforeEach } from "vitest";
import { registerReferenceRefresher } from "@ledger/test-support/invoke-mock";
import { amountPrivacyEnabled } from "@ledger/money";
import { useReferenceStore } from "@/stores/reference";

registerReferenceRefresher(() => useReferenceStore().refresh());

beforeEach(() => {
  amountPrivacyEnabled.value = false;
});
