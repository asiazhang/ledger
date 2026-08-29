Feature: 标的搜索统一模糊语义
  投资表单/实盘面板的标的下拉为远程搜索（issue #199）：list_instruments 的
  search 参数按统一模糊搜索语义匹配（issue #195，ADR-0027）——输入按空白
  切词、词条 AND；每词条命中 = 「代码 · 名称」的原文连续子串（大小写不敏感）
  ∨ 该文本拼音首字母串的子序列（大小写不敏感）。

  Scenario: 拼音首字母整串命中（多音字修正 银行→yh）
    Given 存在标的 "600519" 名称 "招商银行" 币种 "CNY"
    And 存在标的 "000002" 名称 "万科物业" 币种 "CNY"
    When 搜索标的 "zsyh"
    Then 标的搜索命中 1 条 总数 1
    And 标的搜索首个结果代码为 "600519"

  Scenario: 拼音首字母子序列跳字命中
    Given 存在标的 "000002" 名称 "万科物业" 币种 "CNY"
    When 搜索标的 "wy"
    Then 标的搜索命中 1 条 总数 1
    When 搜索标的 "yw"
    Then 标的搜索命中 0 条 总数 0

  Scenario: 原文子串与大小写不敏感
    Given 存在标的 "600519" 名称 "招商银行" 币种 "CNY"
    When 搜索标的 "招商"
    Then 标的搜索命中 1 条 总数 1
    When 搜索标的 "ZSYH"
    Then 标的搜索命中 1 条 总数 1

  Scenario: 多词条 AND 组合（词条分别命中代码与名称）
    Given 存在标的 "600519" 名称 "招商银行" 币种 "CNY"
    When 搜索标的 "600 zs"
    Then 标的搜索命中 1 条 总数 1
    When 搜索标的 "600 wy"
    Then 标的搜索命中 0 条 总数 0

  Scenario: 无名称标的退化为裸代码匹配
    Given 存在标的 "NVDA" 名称 "" 币种 "USD"
    When 搜索标的 "nvd"
    Then 标的搜索命中 1 条 总数 1
