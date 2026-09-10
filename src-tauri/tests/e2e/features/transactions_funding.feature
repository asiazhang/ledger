Feature: 交易出资账户——列表两端展示与按卡过滤
  直扣买卖的资金流在账本里可见、可检索（ADR-0096 / issue #937）

  Scenario: 直扣申购——出资账户余额下降、投资账户不动、列表两端展示、按卡过滤命中
    Given 存在账户 "银行卡" 类型 "bank" 币种 "CNY" 初始余额 10000
    And 存在账户 "基金户" 类型 "investment" 币种 "CNY"
    And 存在基金标的 "000123" 名称 "某混合基金"
    When 按确认单出资账户申购基金 "000123" 份额 5000 金额 5000 手续费 0 到投资账户 "基金户" 出资账户 "银行卡"
    Then "银行卡" 账户余额应为 5000
    And "基金户" 账户余额应为 0
    And 该买入 account_id 应匹配账户 "基金户"
    And 该买入 funding_account_id 应匹配账户 "银行卡"
    And 分页查询 涉及账户 "银行卡" page 1 page_size 10 应返回 1 条 total 1
    And 分页查询 涉及账户 "基金户" page 1 page_size 10 应返回 1 条 total 1
