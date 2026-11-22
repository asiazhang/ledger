import { useRestoreFromFile } from "@/backup/useRestoreFromFile";

export function useFailureRestore() {
  const flow = useRestoreFromFile({
    pickTitleKey: "startupFailure.restorePickTitle",
  });

  const { restoring, restoreIntent, restoreSeq, closeRestore, confirmRestore } = flow;

  return {
    restoring,
    restoreIntent,
    restoreSeq,
    closeRestore,
    confirmRestore,
    pickRestoreFromFailure: flow.pickRestore,
  };
}
