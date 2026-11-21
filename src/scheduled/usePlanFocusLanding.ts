import { onMounted } from "vue";

export function usePlanFocusLanding(options: {
  focusPlanId: () => string | null | undefined;
  openDetail: (planId: string) => void;
  onConsumed: () => void;
}): void {
  onMounted(() => {
    const planId = options.focusPlanId();
    if (!planId) return;
    options.openDetail(planId);
    options.onConsumed();
  });
}
