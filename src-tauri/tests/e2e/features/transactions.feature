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
