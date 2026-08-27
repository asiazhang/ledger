Feature: 首页财务全貌——净资产跨币种合计（issue #142）
  dashboard_overview 只读聚合命令返回本位币净资产及其两个组成：
  非投资账户余额合计与持仓市值合计。账户侧沿用 account_flow 余额口径，
  币种折算复用 convert_to_native；从未录价的持仓按空值语义处理，不以零计入。

  Scenario: 多币种账户与多持仓折本位币合计净资产
    Given 存在汇率 "USD" 兑 "CNY" 为 7.2
    And 存在标的 "NVDA" 币种 "USD"
    And 存在标的 "AAPL" 币种 "USD"
    And 标的 "NVDA" 现价 15000 币种 "USD"
    When 创建账户 "现金" 类型 "cash" 币种 "CNY" 初始余额 100000
    And 创建账户 "美元存款" 类型 "bank" 币种 "USD" 初始余额 20000
    And 创建账户 "美股券商" 类型 "investment" 币种 "USD" 初始余额 0
    And 已买入 标的 "NVDA" 数量 2 单价 10000 到账户 "美股券商"
    And 已买入 标的 "AAPL" 数量 1 单价 5000 到账户 "美股券商"
    # AAPL 从未录价：市值按空值语义跳过，不以零计入合计
    When 查询净资产总览
    Then 非投资账户余额合计应为 244000
    And 持仓市值合计应为 216000
    And 净资产应为 460000

  Scenario: 缺汇率的币种让错误上抛并带中文错误信息
    Given 存在账户 "现金" 类型 "cash" 币种 "CNY"
    And 存在账户 "日元现金" 类型 "bank" 币种 "JPY"
    When 查询净资产总览
    Then 应返回错误 "汇率"

  Scenario: 仅默认币种账户时两组成均为零
    Given 存在账户 "现金" 类型 "cash" 币种 "CNY"
    When 查询净资产总览
    Then 非投资账户余额合计应为 0
    And 持仓市值合计应为 0
    And 净资产应为 0
