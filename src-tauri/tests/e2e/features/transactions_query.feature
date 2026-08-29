Feature: 交易管理
  用户需要创建、查询和管理交易

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
