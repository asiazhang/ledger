Feature: 定时交易引擎接入共享写入权威（native 折算）
  定时引擎生成的交易经 transaction::writer 落库：本位币金额由 convert_to_native 折算
  （而非硬编码 1:1），修复多币种启用后定时交易静默算错的隐患；
  分期 / 订阅 / 定时转账生成的类型与金额保持既有行为（issue #71）。

  Scenario: 非默认币种订阅执行后本位币金额经汇率折算
    Given 存在账户 "美股订阅" 类型 "cash" 币种 "USD"
    And 存在汇率 "USD" 兑 "CNY" 为 7.2
    When 创建订阅计划 金额 10000 币种 "USD" 账户 "美股订阅" 起始日期 "2026-01-15" 备注 "国际订阅"
    And 执行该计划第一期
    Then 该期次交易类型应为 "expense" 金额应为 10000
    And 该期次交易本位币金额应为 72000
    And 该期次状态应为 "completed"

  Scenario: 非默认币种缺汇率时执行失败且期次保持可重试
    Given 存在账户 "日元订阅" 类型 "cash" 币种 "JPY"
    When 创建订阅计划 金额 10000 币种 "JPY" 账户 "日元订阅" 起始日期 "2026-01-15"
    And 执行该计划第一期
    Then 执行应失败并提示 "汇率"
    And 该期次状态应为 "pending"
    And 期次未回填交易
    Given 存在汇率 "JPY" 兑 "CNY" 为 0.05
    When 重新执行该期次
    Then 该期次交易类型应为 "expense" 金额应为 10000
    And 该期次交易本位币金额应为 500
    And 该期次状态应为 "completed"

  Scenario: 分期计划各期金额与类型不回归
    Given 存在账户 "分期账户" 类型 "cash" 币种 "CNY"
    When 创建分期计划 总额 3100 期数 3 账户 "分期账户" 起始日期 "2026-01-15"
    And 依次执行全部期次
    Then 应生成 3 笔类型 "expense" 的交易 金额依次为 "1033,1033,1034"
    And 计划状态应为 "completed"

  Scenario: 定时转账账户映射不回归
    Given 存在账户 "账户A" 类型 "cash" 币种 "CNY"
    And 存在账户 "账户B" 类型 "cash" 币种 "CNY"
    When 创建定时转账计划 金额 50000 从 "账户A" 到 "账户B" 期数 3 起始日期 "2026-01-15"
    And 执行该计划第一期
    Then 该期次交易类型应为 "transfer" 金额应为 50000
    And 该期次交易转入账户应为 "账户B"
    And 该期次状态应为 "completed"
