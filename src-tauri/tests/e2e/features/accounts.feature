Feature: 账户管理
  用户需要创建和管理账户

  Scenario: 创建账户并查看余额
    When 创建账户 "现金" 类型 "cash" 币种 "CNY" 初始余额 10000
    Then 账户列表应包含 1 条记录
    And "现金" 账户余额应为 10000

  Scenario: 软删除不影响其他账户
    When 创建账户 "账户A" 类型 "cash" 币种 "CNY" 初始余额 0
    And 创建账户 "账户B" 类型 "cash" 币种 "CNY" 初始余额 0
    And 删除账户 "账户A"
    Then 账户列表应包含 1 条记录
    And 账户列表应包含 "账户B"
    And 账户列表不应包含 "账户A"

  Scenario: 退款计入账户余额
    Given 存在账户 "现金" 类型 "cash" 币种 "CNY"
    When 创建交易 类型 "expense" 金额 1000 到账户 "现金" 日期 "2026-05-01"
    And 关联上一笔交易创建退款 金额 300 日期 "2026-05-02"
    Then "现金" 账户余额应为 -700

  Scenario: 转账双侧计入余额
    Given 存在账户 "A账户" 类型 "cash" 币种 "CNY"
    And 存在账户 "B账户" 类型 "cash" 币种 "CNY"
    When 创建转账 金额 2000 从 "A账户" 到 "B账户" 日期 "2026-05-03"
    Then "A账户" 账户余额应为 -2000
    And "B账户" 账户余额应为 2000

  Scenario: 分红计入账户余额
    Given 存在账户 "证券账户" 类型 "investment" 币种 "CNY"
    When 创建交易 类型 "dividend" 金额 60 到账户 "证券账户" 日期 "2026-05-04"
    Then "证券账户" 账户余额应为 60
