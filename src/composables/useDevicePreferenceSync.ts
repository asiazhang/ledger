import { watch } from "vue";
import { useAppStore } from "@/stores/app";
import { api } from "@ledger/api";

/**
 * 设备偏好镜像推送：前端 localStorage 设备偏好（应用设置 store）的唯一推送出口，
 * 由应用根组件（App.vue）挂载一次——启动（immediate）与变更时把镜像推给后端
 * 运行时消费。真源永远在本机 localStorage，后端只持镜像（备份目录先例，
 * ADR-0016 决策 3）。
 *
 * - 备份目录（issue #125）：供调度线程与退出兜底消费；未配置传空串即静默跳过。
 * - 自动执行开关（issue #308 / ADR-0042）：设备级偏好、默认关，供定时计划追补
 *   调度读取；未推送（新机器/恢复备份后）即保持默认关——自动化不随账本迁移。
 * - 自动检查更新开关（issue #1431 / ADR-0124 决策 8）：设备级偏好、默认开，供
 *   启动延迟静默检查发起前判定；未推送即按开启兜底（提醒侧另有「已提醒版本」
 *   记忆兜底不骚扰）。
 */
export function useDevicePreferenceSync() {
  const store = useAppStore();

  watch(
    () => store.backupDir,
    (dir) => {
      void api.setAutoBackupDir(dir);
    },
    { immediate: true },
  );

  watch(
    () => store.autoExecutionEnabled,
    (enabled) => {
      void api.setAutoExecutionEnabled(enabled);
    },
    { immediate: true },
  );
  watch(
    () => store.autoUpdateEnabled,
    (enabled) => {
      void api.setAutoUpdateEnabled(enabled);
    },
    { immediate: true },
  );
}
