import { api } from '@/api'
import { useReferenceStore } from '@/stores/reference'

/**
 * 商户解析（「输入即建 + 重名兜底」的单一权威，ADR-0051 决策 7 复用商户字典）：
 * tag 下拉的值可能是既有商户 id，也可能是用户输入的新名字；保存时单点收口——
 * 1. 空 → null（无商户）；
 * 2. 选中既有商户（value 为 id，merchantMap 含软删显示映射）→ 原样携带
 *    （编辑路径维持历史引用，同 Writer 接缝的 existing_merchant_id 语义）；
 *    `editingMerchantId` 提供时对同 id 再加一层兜底（id 已超出会话缓存的极端情形，
 *    订阅编辑路径专用，issue #206）；
 * 3. 输入文本精确命中在用商户名 → 按名复用（同一商户全库同名一致，软删名不命中）；
 * 4. 未命中 → `create_merchant` 即建；重名错误（store 陈旧竞态）先强制重拉
 *    按名复用，仍失败才向上抛。
 *
 * 消费方：计划表单（useScheduledPlanForm，issue #190/#206）与保单表单
 * （PolicyFormModal，issue #360）——第三处出现前不得再复制本函数。
 */
export async function resolveMerchantRef(
  selected: string | null,
  editingMerchantId: string | null = null,
): Promise<string | null> {
  if (!selected) return null
  const reference = useReferenceStore()
  if (reference.merchantMap.has(selected)) return selected
  if (editingMerchantId && selected === editingMerchantId) return selected
  const name = selected.trim()
  if (!name) return null
  const existing = reference.merchantByName.get(name)
  if (existing) return existing.id
  try {
    return await api.createMerchant({ name })
  } catch (e) {
    // 重名兜底（store 陈旧竞态）：强制重拉后按名复用；重拉失败不影响原错误上抛
    try {
      await reference.refresh()
    } catch {
      /* 保留原 create 错误 */
    }
    const retry = reference.merchantByName.get(name)
    if (retry) return retry.id
    throw e
  }
}
