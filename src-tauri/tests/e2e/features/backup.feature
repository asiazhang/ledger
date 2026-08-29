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

  Scenario: 备份产物标记来源（issue #127）
    Given 存在账户 "现金" 类型 "cash" 币种 "CNY"
    When 创建交易 类型 "expense" 金额 1500 到账户 "现金" 日期 "2026-02-01" 备注 "午餐"
    And 备份数据库到临时文件
    Then 备份元数据来源应为 "manual"
    When 自动备份数据库到临时目录
    Then 自动备份元数据来源应为 "auto"

  Scenario: 拒绝恢复更高 schema 版本的备份
    Given 存在账户 "现金" 类型 "cash" 币种 "CNY"
    When 尝试从更高 schema 版本恢复
    Then 应返回错误 "更高版本"

  Scenario: 业务写入与删除成功后置脏（issue #126）
    Given 存在账户 "现金" 类型 "cash" 币种 "CNY"
    Then 自动备份脏标记应为假
    When 创建交易 类型 "expense" 金额 1500 到账户 "现金" 日期 "2026-02-01" 备注 "午餐"
    And 删除最近创建的交易
    Then 自动备份脏标记应为真

  Scenario: 从备份恢复后自动备份调度状态重置（issue #126）
    Given 存在账户 "现金" 类型 "cash" 币种 "CNY"
    When 创建交易 类型 "expense" 金额 1500 到账户 "现金" 日期 "2026-02-01" 备注 "午餐"
    And 备份数据库到临时文件
    And 从备份恢复到临时数据库
    Then 恢复的数据库应包含 1 条交易
    And 恢复的数据库自动备份状态应为「未脏且已重新计时」

  # ---- 连接层统一写入口（ADR-0032，spec #173 / issue #242）----

  Scenario: 设置写入不置脏（app_settings 豁免，经 settings 模块单点收口）
    Given 存在账户 "现金" 类型 "cash" 币种 "CNY"
    Then 自动备份脏标记应为假
    When 写入一项设置
    Then 自动备份脏标记应为假

  Scenario: 交易修改失败回滚不置脏（写入口闭包失败语义）
    Given 存在账户 "现金" 类型 "cash" 币种 "CNY"
    When 创建交易 类型 "expense" 金额 1500 到账户 "现金" 日期 "2026-02-01" 备注 "午餐"
    Then 自动备份脏标记应为真
    When 自动备份数据库到临时目录
    And 尝试把最近创建的交易修改为非法金额
    Then 应返回错误 "金额必须大于 0"
    And 自动备份脏标记应为假

  Scenario: 参考数据写入成功后置脏（账户/分类/商户写路径，issue #243 行为保持）
    Given 存在账户 "现金" 类型 "cash" 币种 "CNY"
    Then 自动备份脏标记应为假
    When 创建账户 "工资卡" 类型 "bank" 币种 "CNY"
    Then 自动备份脏标记应为真
    When 距离上次自动备份已过一天
    And 自动备份数据库到临时目录
    And 创建分类 "通勤" 类型 "expense"
    Then 自动备份脏标记应为真
    When 距离上次自动备份已过一天
    And 自动备份数据库到临时目录
    And 创建商户 "星巴克"
    Then 自动备份脏标记应为真
    When 距离上次自动备份已过一天
    And 自动备份数据库到临时目录
    And 删除分类 "通勤"
    Then 自动备份脏标记应为真

  Scenario: 余额调整提交后置脏、失败回滚不置脏（「COMMIT 后补调」由写入口承接，issue #243）
    Given 存在账户 "现金" 类型 "cash" 币种 "CNY"
    Then 自动备份脏标记应为假
    When 调整账户 "现金" 余额至 1000 日期 "2026-02-01"
    Then 自动备份脏标记应为真
    When 自动备份数据库到临时目录
    And 调整账户 "现金" 余额至 1000 日期 "2026-02-01"
    Then 应返回错误 "余额已等于目标值"
    And 自动备份脏标记应为假
