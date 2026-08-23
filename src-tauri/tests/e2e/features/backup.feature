Feature: 备份与恢复
  用户需要把账本备份为文件，并能从备份完整恢复

  Scenario: 备份生成包含数据库与元数据的 zip 包
    Given 存在账户 "现金" 类型 "cash" 币种 "CNY"
    When 创建交易 类型 "expense" 金额 1500 到账户 "现金" 日期 "2026-02-01" 备注 "午餐"
    And 备份数据库到临时文件
    Then 备份文件应存在
    And 备份包应包含 "ledger.db" 与 "backup.json"
    And 备份包内的数据库应包含 1 条交易

  Scenario: 从备份恢复完整还原数据
    Given 存在账户 "现金" 类型 "cash" 币种 "CNY"
    When 创建交易 类型 "expense" 金额 1500 到账户 "现金" 日期 "2026-02-01" 备注 "午餐"
    And 备份数据库到临时文件
    And 删除全部交易
    And 从备份恢复到临时数据库
    Then 恢复的数据库应包含 1 条交易

  Scenario: 拒绝恢复更高 schema 版本的备份
    Given 存在账户 "现金" 类型 "cash" 币种 "CNY"
    When 尝试从更高 schema 版本恢复
    Then 应返回错误 "更高版本"
