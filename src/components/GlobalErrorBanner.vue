<script setup lang="ts">
import { NButton } from "naive-ui";
import { useThemeVars } from "naive-ui";
import { useRenderErrorsStore } from "@/stores/render-errors";
import { t } from "@ledger/i18n";
import { banner, text, closeButton } from "./global-error-banner.css.ts";

const store = useRenderErrorsStore();
const themeVars = useThemeVars();
</script>

<template>
  <Transition name="global-error-banner">
    <div
      v-if="store.message"
      :class="banner"
      role="alert"
      :style="{ '--error-color': themeVars.errorColor }"
    >
      <span :class="text">{{ t("common.globalErrorBanner.prefix") }}{{ store.message }}</span>
      <NButton
        size="tiny"
        quaternary
        :class="closeButton"
        :aria-label="t('common.globalErrorBanner.dismiss')"
        @click="store.dismiss()"
      >
        {{ t("common.globalErrorBanner.dismiss") }}
      </NButton>
    </div>
  </Transition>
</template>
