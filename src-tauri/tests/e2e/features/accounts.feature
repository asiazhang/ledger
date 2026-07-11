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
