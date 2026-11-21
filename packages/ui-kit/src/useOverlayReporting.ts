import { computed, onScopeDispose, ref, useAttrs, watch } from "vue";
import type { Ref } from "vue";
import { createOverlayToken } from "./overlayRegistry";

export function useOverlayReporting(name: string): {
  onUpdateShow: (value: boolean) => void;
  resolvedShow: Ref<boolean>;
} {
  const attrs = useAttrs();
  const overlay = createOverlayToken(name, requestClose);
  const shadowShow = ref(false);

  watch(
    () => attrs.show,
    (value) => {
      if (value !== undefined) {
        shadowShow.value = Boolean(value);
        overlay.set(Boolean(value));
      }
    },
    { immediate: true },
  );

  const onUpdateShow = (value: boolean) => {
    shadowShow.value = value;
    overlay.set(value);
  };

  const resolvedShow = computed(() =>
    attrs.show !== undefined ? Boolean(attrs.show) : shadowShow.value,
  );

  function requestClose(): boolean {
    const handler = attrs["onUpdate:show"] as ((value: boolean) => void) | undefined;
    if (typeof handler === "function") {
      handler(false);
      return true;
    }
    if (attrs.show !== undefined) {
      return false;
    }
    shadowShow.value = false;
    overlay.set(false);
    return true;
  }

  onScopeDispose(() => overlay.set(false));

  return { onUpdateShow, resolvedShow };
}
