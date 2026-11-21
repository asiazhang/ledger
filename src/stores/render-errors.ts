import { defineStore } from "pinia";
import { ref } from "vue";

export const useRenderErrorsStore = defineStore("render-errors", () => {
  const message = ref<string | null>(null);

  function report(text: string): void {
    message.value = text;
  }

  function dismiss(): void {
    message.value = null;
  }

  return { message, report, dismiss };
});
