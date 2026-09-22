// 应用壳测试装配薄壳（issue #1152）：「目录级测试薄壳」词条的 setup 侧形态——
// 只承载根包应用壳特有的测试装配，通用能力一律住 @ledger/test-support。
//
// invoke 测试接缝的 store 层参考预热（refreshReferenceStores）需要根包的参考
// store（@/stores/reference）；接缝本体已包化且不得反向依赖应用壳，故经
// registerReferenceRefresher 注册注入。本文件由 app project 的 setupFiles 装载，
// 每个测试文件先于用例求值执行（包化后模块注册表按文件隔离，注册随之每文件生效）。
import { beforeEach } from "vitest";
import { registerReferenceRefresher } from "@ledger/test-support/invoke-mock";
import { amountPrivacyEnabled } from "@ledger/money";
import { useReferenceStore } from "@/stores/reference";

registerReferenceRefresher(() => useReferenceStore().refresh());

// 金额隐私开关复位（清理四件套同责）：它是展示格式化层的模块级单点 ref，不随
// 测试壳的本地存储清空自愈——在此每测复位，应用壳项目全部测试文件统一生效，
// 测试文件不再手搓复位样板（清理四件套「测试文件内出现同类样板即回潮」）。
beforeEach(() => {
  amountPrivacyEnabled.value = false;
});
