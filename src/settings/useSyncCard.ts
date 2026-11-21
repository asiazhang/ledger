import { computed, onMounted, ref } from "vue";
import { useMessage } from "naive-ui";
import { api } from "@ledger/api";
import { t } from "@ledger/i18n";
import { formatIsoMinute } from "@ledger/utils/datetime";
import { restartAppShortly } from "@/backup/restart";
import { useLoadable } from "@ledger/loadable";
import {
  CUSTOM_VENDOR_ID,
  findVendorPreset,
  matchVendorByEndpoint,
  vendorOptions,
  vendorPrefill,
  vendorTierKey,
  type S3VendorPrefill,
} from "@ledger/utils/s3-vendors";
import type {
  ParkedOpInfo,
  SyncChannelConfig,
  SyncChannelConfigInput,
  SyncCheckpointInfo,
  SyncStatus,
} from "@ledger/types";

export function useSyncCard() {
  const message = useMessage();

  const status = ref<SyncStatus | null>(null);
  const passphrase = ref("");
  const parkedOps = ref<ParkedOpInfo[]>([]);

  const statusLoad = useLoadable(async () => {
    const next = await api.getSyncStatus();
    status.value = next;
    await refreshParkedOps();
    return true;
  });
  const statusLoading = statusLoad.loading;

  async function refreshStatus(): Promise<void> {
    await statusLoad.run();
  }

  async function refreshParkedOps(): Promise<void> {
    if (!status.value || status.value.parked_count === 0) {
      parkedOps.value = [];
      return;
    }
    try {
      parkedOps.value = await api.getParkedOps();
    } catch (e) {
      console.warn("挂起明细拉取失败", e);
      parkedOps.value = [];
    }
  }

  const lastSyncText = computed(() =>
    status.value?.last_sync_at
      ? formatIsoMinute(status.value.last_sync_at)
      : t("settings.data.sync.neverSynced"),
  );

  const syncLoad = useLoadable(async () => {
    const report = await api.syncNow(passphrase.value || undefined);
    message.success(
      t("settings.data.sync.syncOk", {
        uploaded: report.uploaded_ops,
        applied: report.applied,
        parked: report.parked,
      }),
    );
    await refreshStatus();
    if (report.parked > 0) {
      message.warning(t("settings.data.sync.parkedToast", { count: report.parked }));
    }
    return true;
  });
  const syncing = syncLoad.loading;

  async function syncNow(): Promise<void> {
    await syncLoad.run();
  }

  type ChannelForm = Pick<
    SyncChannelConfig,
    | "space_id"
    | "endpoint"
    | "region"
    | "bucket"
    | "prefix"
    | "access_key"
    | "secret_key"
    | "path_style"
  >;

  const form = ref<ChannelForm>({
    space_id: "default",
    endpoint: "",
    region: "",
    bucket: "",
    prefix: "",
    access_key: "",
    secret_key: "",
    path_style: false,
  });

  const secretKeyInput = ref("");

  const secretKeyPlaceholder = computed(() =>
    form.value.secret_key
      ? t("settings.data.sync.secretKeySavedPlaceholder")
      : t("settings.data.sync.secretKeyPlaceholder"),
  );

  const selectedVendor = ref<string>(CUSTOM_VENDOR_ID);

  const selectedVendorPreset = computed(() => findVendorPreset(selectedVendor.value));

  const vendorSelectOptions = computed(() =>
    vendorOptions().map((option) => ({
      value: option.id,
      label: option.custom
        ? t("settings.data.sync.vendorCustom")
        : t("settings.data.sync.vendorOption", {
            name: option.name,
            tier: t(vendorTierKey(option.verified)),
          }),
    })),
  );

  function onVendorChange(vendorId: string): void {
    selectedVendor.value = vendorId;
    const prefill = vendorPrefill(vendorId);
    if (prefill) applyPrefill(prefill);
  }

  function applyVendorRegion(region: string): void {
    const prefill = vendorPrefill(selectedVendor.value, region);
    if (prefill) applyPrefill(prefill);
  }

  function applyPrefill(prefill: S3VendorPrefill): void {
    form.value.endpoint = prefill.endpoint;
    form.value.region = prefill.region;
    form.value.path_style = prefill.pathStyle;
  }

  const channelLoad = useLoadable(async () => {
    const config = await api.getSyncChannelConfig();
    form.value = {
      space_id: config.configured ? config.space_id : "default",
      endpoint: config.endpoint,
      region: config.region,
      bucket: config.bucket,
      prefix: config.prefix,
      access_key: config.access_key,
      secret_key: config.secret_key,
      path_style: config.path_style,
    };
    secretKeyInput.value = "";
    selectedVendor.value = matchVendorByEndpoint(form.value.endpoint);
    return true;
  });

  async function refreshChannelConfig(): Promise<void> {
    await channelLoad.run();
  }

  function channelPayload(): SyncChannelConfigInput {
    return {
      space_id: form.value.space_id.trim() || undefined,
      endpoint: form.value.endpoint,
      region: form.value.region,
      bucket: form.value.bucket,
      prefix: form.value.prefix,
      access_key: form.value.access_key,
      secret_key: secretKeyInput.value !== "" ? secretKeyInput.value : form.value.secret_key,
      path_style: form.value.path_style,
    };
  }

  const saveLoad = useLoadable(async () => {
    await api.setSyncChannelConfig(channelPayload());
    message.success(t("settings.data.sync.saveOk"));
    await Promise.all([refreshStatus(), refreshChannelConfig()]);
    return true;
  });
  const saving = saveLoad.loading;

  async function saveChannel(): Promise<void> {
    await saveLoad.run();
  }

  const probe = useLoadable(async () => {
    await api.testSyncChannelConnection(channelPayload());
    return true;
  });
  const testing = probe.loading;

  async function testConnection(): Promise<void> {
    if (await probe.run()) {
      message.success(t("settings.data.sync.testOk"));
    }
  }

  function formatSizeMb(bytes: number): string {
    return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
  }

  const publishLoad = useLoadable(async () => {
    const result = await api.publishSyncCheckpoint(passphrase.value || undefined);
    message.success(
      t("settings.data.sync.publishOk", {
        generation: result.generation,
        size: formatSizeMb(result.size),
      }),
    );
    if (result.plaintext_mode) {
      message.warning(t("settings.data.sync.publishPlaintextToast"));
    }
    return true;
  });
  const publishing = publishLoad.loading;

  async function publishCheckpoint(): Promise<void> {
    await publishLoad.run();
  }

  const bootstrapShow = ref(false);
  const bootstrapPassphrase = ref("");
  const checkpointInfo = ref<SyncCheckpointInfo | null>(null);

  const precheck = useLoadable(() => api.getSyncChannelCheckpoint(), { silent: true });
  const prechecking = precheck.loading;
  const precheckError = precheck.error;

  async function openBootstrap(): Promise<void> {
    bootstrapShow.value = true;
    bootstrapPassphrase.value = "";
    checkpointInfo.value = null;
    checkpointInfo.value = await precheck.run();
  }

  const bootstrapLoad = useLoadable(
    async () => await api.bootstrapSyncFromChannel(bootstrapPassphrase.value || undefined),
  );
  const bootstrapping = bootstrapLoad.loading;

  async function confirmBootstrap(): Promise<void> {
    if (!checkpointInfo.value || bootstrapping.value) return;
    const outcome = await bootstrapLoad.run();
    if (outcome === null) return;
    bootstrapShow.value = false;
    message.success(
      t("settings.data.sync.bootstrapOk", {
        generation: outcome.generation,
        size: formatSizeMb(outcome.size),
        reencrypted: outcome.reencrypted ? t("settings.data.sync.bootstrapReencrypted") : "",
      }),
    );
    restartAppShortly();
  }

  onMounted(async () => {
    await Promise.all([refreshStatus(), refreshChannelConfig()]);
  });

  return {
    status,
    statusLoading,
    lastSyncText,
    refreshStatus,
    parkedOps,
    passphrase,
    syncing,
    syncNow,
    form,
    secretKeyInput,
    secretKeyPlaceholder,
    selectedVendor,
    selectedVendorPreset,
    vendorSelectOptions,
    onVendorChange,
    applyVendorRegion,
    channelPayload,
    refreshChannelConfig,
    saving,
    saveChannel,
    testing,
    testConnection,
    publishing,
    publishCheckpoint,
    bootstrapShow,
    prechecking,
    precheckError,
    checkpointInfo,
    bootstrapPassphrase,
    bootstrapping,
    openBootstrap,
    confirmBootstrap,
  };
}
