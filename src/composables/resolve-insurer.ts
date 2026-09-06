import { api } from '@/api'
import { useReferenceStore } from '@/stores/reference'

/**
 * 保司解析（「输入即建 + 重名兜底」，issue #713 / ADR-0082）：保单表单选择器的
 * 保存时单点收口——tag 下拉的值可能是既有保司 id，也可能是用户输入的新名字：
 * 1. 空 → null（保司必填，调用方自行拦截空值并提示）；
 * 2. 选中既有保司（value 为 id，insurerMap 含软删显示映射）→ 原样携带
 *    （编辑路径维持历史引用，同 Writer 接缝的 existing_merchant_id 语义）；
 * 3. 输入文本精确命中在用保司名 → 按名复用（同一保司全库同名一致，软删名不命中）；
 * 4. 未命中 → `create_insurer` 即建；重名错误（store 陈旧竞态）先强制重拉
 *    按名复用，仍失败才向上抛。
 *
 * 消费方：保单表单（PolicyFormModal，issue #713 换轨：即建目标从商户换为保司）。
 * 语义与 `resolve-merchant` 同构；两字典语义分离（ADR-0082），不合并为参数化接缝
 * ——商户/保司是两套字典，合并会让类型面与错误文案在调用点失去领域信息。
 */
export async function resolveInsurerRef(selected: string | null): Promise<string | null> {
  if (!selected) return null
  const reference = useReferenceStore()
  if (reference.insurerMap.has(selected)) return selected
  const name = selected.trim()
  if (!name) return null
  const existing = reference.insurerByName.get(name)
  if (existing) return existing.id
  try {
    return await api.createInsurer({ name })
  } catch (e) {
    // 重名兜底（store 陈旧竞态）：强制重拉后按名复用；重拉失败不影响原错误上抛
    try {
      await reference.refresh()
    } catch {
      /* 保留原 create 错误 */
    }
    const retry = reference.insurerByName.get(name)
    if (retry) return retry.id
    throw e
  }
}
