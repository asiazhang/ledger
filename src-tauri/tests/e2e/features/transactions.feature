Feature: 交易管理
  用户需要创建、查询和管理交易

  Scenario: 创建收入和支出交易
    Given 存在账户 "现金" 类型 "cash" 币种 "CNY"
    When 创建交易 类型 "income" 金额 5000 到账户 "现金" 日期 "2026-02-02"
    And 创建交易 类型 "expense" 金额 1500 到账户 "现金" 日期 "2026-02-01" 备注 "午餐"
    Then 交易列表应包含 2 条记录
    And 第 1 条交易类型应为 "income" 金额应为 5000
    And 第 2 条交易类型应为 "expense" 金额应为 1500 备注 "午餐"

  Scenario: 转账必须指定目标账户
    Given 存在账户 "A账户" 类型 "cash" 币种 "CNY"
    When 尝试创建转账 金额 3000 从账户 "A账户" 日期 "2026-03-01"
    Then 应返回错误 "转账必须指定目标账户"

  Scenario: 投资类交易（分红/拆股）MVP 未实现，经交易接口显式拒绝
    Given 存在账户 "证券账户" 类型 "investment" 币种 "CNY"
    When 尝试创建交易 类型 "dividend" 金额 60 到账户 "证券账户" 日期 "2026-05-04"
    Then 应返回错误 "暂不支持"
    When 尝试创建交易 类型 "split" 金额 0 到账户 "证券账户" 日期 "2026-05-05"
    Then 应返回错误 "暂不支持"

  Scenario: 创建转账交易
    Given 存在账户 "A账户" 类型 "cash" 币种 "CNY"
    And 存在账户 "B账户" 类型 "cash" 币种 "CNY"
    When 创建转账 金额 3000 从 "A账户" 到 "B账户" 日期 "2026-03-01"
    Then 交易列表应包含 1 条记录
    And 该转账类型应为 "transfer"
    And 该转账 account_id 应匹配账户 "A账户"
    And 该转账 to_account_id 应匹配账户 "B账户"

  Scenario: 退款关联原支出
    Given 存在账户 "现金" 类型 "cash" 币种 "CNY"
    When 创建交易 类型 "expense" 金额 1000 到账户 "现金" 日期 "2026-04-01"
    And 关联上一笔交易创建退款 金额 200 日期 "2026-04-05"
    Then 交易列表应包含 2 条记录
    And 退款交易的 refund_of 应指向原支出交易

  Scenario: 按 id 全字段替换交易并保留去重身份
    Given 存在账户 "现金" 类型 "cash" 币种 "CNY"
    When 创建交易 类型 "expense" 金额 500 到账户 "现金" 日期 "2026-02-02"
    And 修改最近交易 类型 "expense" 金额 900 日期 "2026-02-10" 备注 "修改"
    Then 交易列表应包含 1 条记录
    And 第 1 条交易类型应为 "expense" 金额应为 900 备注 "修改"
    And 第 1 条交易版本应为 2

  Scenario: 编辑已删除的交易返回明确错误
    Given 存在账户 "现金" 类型 "cash" 币种 "CNY"
    When 创建交易 类型 "expense" 金额 500 到账户 "现金" 日期 "2026-02-02"
    And 删除最近交易
    And 尝试修改已删除的交易 金额 900 日期 "2026-02-10"
    Then 应返回错误 "交易不存在"

  Scenario: 修改交易复用按 kind 校验（转账缺目标账户）
    Given 存在账户 "现金" 类型 "cash" 币种 "CNY"
    When 创建交易 类型 "expense" 金额 500 到账户 "现金" 日期 "2026-02-02"
    And 尝试修改最近交易为转账 金额 1000 日期 "2026-02-03"
    Then 应返回错误 "转账必须指定目标账户"

  Scenario: 修改不存在的交易返回明确错误
    Given 存在账户 "现金" 类型 "cash" 币种 "CNY"
    When 尝试修改不存在的交易 金额 100 日期 "2026-02-02"
    Then 应返回错误 "交易不存在"

  Scenario: 编辑买入交易全字段替换并重建持仓批次（issue #180）
    Given 存在账户 "证券账户" 类型 "investment" 币种 "CNY"
    And 存在标的 "600519" 币种 "CNY"
    When 买入标的 "600519" 数量 100 单价 1500 到投资账户 "证券账户"
    And 修改买入交易 "600519" 数量 200 单价 1600 手续费 500
    Then 交易列表应包含 1 条记录
    And 第 1 条交易类型应为 "buy" 金额应为 320500
    And 标的 "600519" 持仓数量应为 200
    And 该买入明细应为 标的 "600519" 数量 200 单价 1600 手续费 500

  Scenario: 已有部分卖出的买入编辑被拒（持仓批次一致性守卫，issue #180）
    Given 存在账户 "证券账户" 类型 "investment" 币种 "CNY"
    And 存在标的 "600519" 币种 "CNY"
    When 买入标的 "600519" 数量 100 单价 1500 到投资账户 "证券账户"
    And 卖出标的 "600519" 数量 40 单价 1600 从投资账户 "证券账户"
    And 尝试修改买入交易 "600519" 数量 200 单价 1800
    Then 应返回错误 "该买入交易已有部分卖出，无法修改"

  Scenario: 编辑卖出交易重建卖出匹配与持仓扣减（issue #180）
    Given 存在账户 "证券账户" 类型 "investment" 币种 "CNY"
    And 存在标的 "600519" 币种 "CNY"
    When 买入标的 "600519" 数量 100 单价 1500 到投资账户 "证券账户"
    And 卖出标的 "600519" 数量 40 单价 1600 从投资账户 "证券账户"
    And 修改卖出交易 "600519" 数量 60 单价 1600 手续费 200
    Then 交易列表应包含 2 条记录
    And 第 1 条交易类型应为 "sell" 金额应为 95800
    And 标的 "600519" 持仓数量应为 40
    And 该卖出明细应为 标的 "600519" 数量 60 单价 1600 手续费 200

  Scenario: 服务端分页返回当前页与过滤后总数
    Given 存在账户 "现金" 类型 "cash" 币种 "CNY"
    When 批量导入交易
      | kind    | 金额  | 币种 | 账户 | 转入账户 | 日期        |
      | expense | 100  | CNY  | 现金  |          | 2026-01-01 |
      | expense | 200  | CNY  | 现金  |          | 2026-01-02 |
      | expense | 300  | CNY  | 现金  |          | 2026-01-03 |
      | expense | 400  | CNY  | 现金  |          | 2026-01-04 |
      | expense | 500  | CNY  | 现金  |          | 2026-01-05 |
      | expense | 600  | CNY  | 现金  |          | 2026-01-06 |
      | expense | 700  | CNY  | 现金  |          | 2026-01-07 |
      | expense | 800  | CNY  | 现金  |          | 2026-01-08 |
      | expense | 900  | CNY  | 现金  |          | 2026-01-09 |
      | expense | 1000 | CNY  | 现金  |          | 2026-01-10 |
      | expense | 1100 | CNY  | 现金  |          | 2026-01-11 |
      | expense | 1200 | CNY  | 现金  |          | 2026-01-12 |
      | expense | 1300 | CNY  | 现金  |          | 2026-01-13 |
      | expense | 1400 | CNY  | 现金  |          | 2026-01-14 |
      | expense | 1500 | CNY  | 现金  |          | 2026-01-15 |
      | expense | 1600 | CNY  | 现金  |          | 2026-01-16 |
      | expense | 1700 | CNY  | 现金  |          | 2026-01-17 |
      | expense | 1800 | CNY  | 现金  |          | 2026-01-18 |
      | expense | 1900 | CNY  | 现金  |          | 2026-01-19 |
      | expense | 2000 | CNY  | 现金  |          | 2026-01-20 |
      | expense | 2100 | CNY  | 现金  |          | 2026-01-21 |
      | expense | 2200 | CNY  | 现金  |          | 2026-01-22 |
      | expense | 2300 | CNY  | 现金  |          | 2026-01-23 |
      | expense | 2400 | CNY  | 现金  |          | 2026-01-24 |
      | expense | 2500 | CNY  | 现金  |          | 2026-01-25 |
    Then 分页查询 page 1 page_size 10 应返回 10 条 total 25
    And 分页查询 page 2 page_size 10 应返回 10 条 total 25
    And 分页查询 page 3 page_size 10 应返回 5 条 total 25

  Scenario: 过滤与分页组合的 total 口径
    Given 存在账户 "现金" 类型 "cash" 币种 "CNY"
    And 存在账户 "银行" 类型 "bank" 币种 "CNY"
    When 批量导入交易
      | kind    | 金额  | 币种 | 账户 | 转入账户 | 日期        |
      | expense | 100  | CNY  | 现金  |          | 2026-02-01 |
      | expense | 200  | CNY  | 现金  |          | 2026-02-02 |
      | expense | 300  | CNY  | 现金  |          | 2026-02-03 |
      | expense | 400  | CNY  | 现金  |          | 2026-02-04 |
      | expense | 500  | CNY  | 现金  |          | 2026-02-05 |
      | expense | 600  | CNY  | 现金  |          | 2026-02-06 |
      | expense | 700  | CNY  | 现金  |          | 2026-02-07 |
      | income  | 1000 | CNY  | 银行  |          | 2026-02-08 |
    Then 分页查询 账户 "现金" page 1 page_size 5 应返回 5 条 total 7
    And 分页查询 账户 "现金" page 2 page_size 5 应返回 2 条 total 7
    And 分页查询 kind "expense" page 1 page_size 3 应返回 3 条 total 7
    And 分页查询 日期 "2026-02-02" 至 "2026-02-06" page 1 page_size 2 应返回 2 条 total 5

  Scenario: 涉及账户过滤命中普通交易与转账两侧
    Given 存在账户 "现金" 类型 "cash" 币种 "CNY"
    And 存在账户 "银行" 类型 "bank" 币种 "CNY"
    And 存在账户 "支付宝" 类型 "cash" 币种 "CNY"
    When 创建交易 类型 "expense" 金额 100 到账户 "现金" 日期 "2026-03-01"
    And 创建转账 金额 3000 从 "现金" 到 "银行" 日期 "2026-03-02"
    And 创建转账 金额 500 从 "银行" 到 "现金" 日期 "2026-03-03"
    And 创建交易 类型 "expense" 金额 700 到账户 "支付宝" 日期 "2026-03-04"
    Then 分页查询 涉及账户 "现金" page 1 page_size 10 应返回 3 条 total 3
    And 分页查询 涉及账户 "支付宝" page 1 page_size 10 应返回 1 条 total 1

  Scenario: 按商户过滤命中该商户全部交易（分页 total 口径）
    Given 存在账户 "现金" 类型 "cash" 币种 "CNY"
    And 存在商户 "京东"
    And 存在商户 "拼多多"
    When 创建交易 类型 "expense" 金额 100 到账户 "现金" 日期 "2026-04-01" 商户 "京东"
    And 创建交易 类型 "expense" 金额 200 到账户 "现金" 日期 "2026-04-02" 商户 "京东"
    And 创建交易 类型 "income" 金额 500 到账户 "现金" 日期 "2026-04-03" 商户 "拼多多"
    And 创建交易 类型 "expense" 金额 300 到账户 "现金" 日期 "2026-04-04"
    Then 分页查询 商户 "京东" page 1 page_size 10 应返回 2 条 total 2
    And 分页查询 商户 "京东" page 2 page_size 1 应返回 1 条 total 2
    And 分页查询 商户 "拼多多" page 1 page_size 1 应返回 1 条 total 1
    And 分页查询 page 1 page_size 10 应返回 4 条 total 4

  Scenario: 商户与账户/日期组合筛选；软删商户仍可过滤且出现在含软删商户列表
    Given 存在账户 "现金" 类型 "cash" 币种 "CNY"
    And 存在账户 "银行" 类型 "bank" 币种 "CNY"
    And 存在商户 "京东"
    When 创建交易 类型 "expense" 金额 100 到账户 "现金" 日期 "2026-05-01" 商户 "京东"
    And 创建交易 类型 "expense" 金额 200 到账户 "银行" 日期 "2026-05-02" 商户 "京东"
    And 创建交易 类型 "expense" 金额 300 到账户 "现金" 日期 "2026-05-03" 商户 "京东"
    Then 分页查询 商户 "京东" 涉及账户 "现金" page 1 page_size 10 应返回 2 条 total 2
    And 分页查询 商户 "京东" 日期 "2026-05-02" 至 "2026-05-30" page 1 page_size 10 应返回 2 条 total 2
    When 软删商户 "京东"
    Then 分页查询 商户 "京东" page 1 page_size 10 应返回 3 条 total 3
    And 商户列表应包含 0 条记录
    And 商户含软删列表应包含 1 条记录
    And 商户含软删列表应包含 "京东"

  Scenario: 同日期同时间戳批量导入翻页无重复无遗漏
    Given 存在账户 "现金" 类型 "cash" 币种 "CNY"
    When 批量导入 25 笔同日交易 日期 "2026-03-01" 到账户 "现金"
    Then 分页查询 page 1 page_size 10 应返回 10 条 total 25
    And 分页查询 page 2 page_size 10 应返回 10 条 total 25
    And 分页查询 page 3 page_size 10 应返回 5 条 total 25
    And 翻页 page_size 10 应覆盖全部 25 条无重复无遗漏

  Scenario: 缺省返回全部且 limit 取前 N 条行为不变
    Given 存在账户 "现金" 类型 "cash" 币种 "CNY"
    When 批量导入交易
      | kind    | 金额 | 币种 | 账户 | 转入账户 | 日期        |
      | expense | 100 | CNY  | 现金  |          | 2026-04-01 |
      | expense | 200 | CNY  | 现金  |          | 2026-04-02 |
      | expense | 300 | CNY  | 现金  |          | 2026-04-03 |
      | expense | 400 | CNY  | 现金  |          | 2026-04-04 |
      | expense | 500 | CNY  | 现金  |          | 2026-04-05 |
    Then 缺省查询 应返回 5 条 total 5
    And 读取 limit 3 应返回 3 条
    And 读取 limit 10 应返回 5 条

  Scenario: 空结果与超范围页码边界
    Given 存在账户 "现金" 类型 "cash" 币种 "CNY"
    When 批量导入交易
      | kind    | 金额 | 币种 | 账户 | 转入账户 | 日期        |
      | expense | 100 | CNY  | 现金  |          | 2026-05-01 |
      | expense | 200 | CNY  | 现金  |          | 2026-05-02 |
      | expense | 300 | CNY  | 现金  |          | 2026-05-03 |
    Then 分页查询 page 99 page_size 10 应返回 0 条 total 3
    And 分页查询 page 0 page_size 10 应返回 3 条 total 3
    And 分页查询 kind "income" page 1 page_size 10 应返回 0 条 total 0
