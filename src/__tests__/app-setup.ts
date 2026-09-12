// 应用壳测试装配薄壳（issue #1152）：「目录级测试薄壳」词条的 setup 侧形态——
// 只承载根包应用壳特有的测试装配，通用能力一律住 @ledger/test-support。
//
// invoke 测试接缝的 store 层参考预热（refreshReferenceStores）需要根包的参考
// store（@/stores/reference）；接缝本体已包化且不得反向依赖应用壳，故经
// registerReferenceRefresher 注册注入。本文件由 app project 的 setupFiles 装载，
// 每个测试文件先于用例求值执行（包化后模块注册表按文件隔离，注册随之每文件生效）。
import { registerReferenceRefresher } from '@ledger/test-support/invoke-mock'
import { useReferenceStore } from '@/stores/reference'

registerReferenceRefresher(() => useReferenceStore().refresh())
