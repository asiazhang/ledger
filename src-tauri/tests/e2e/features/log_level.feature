Feature: 日志等级配置持久化（spec #611）
  后端把日志等级持久化到 app_settings（logging.level），缺 key / 缺表回默认 info；
  闭集五档（error/warn/info/debug/trace），set→get 回读一致，闭集外档位被拒。

  Scenario: 未配置时持久化档位回默认 info（缺 key）
    Then 持久化日志档位应为 "info"

  Scenario: set→get 回读一致（合法档位持久化并读回）
    When 写入持久化日志档位 "debug"
    Then 持久化日志档位应为 "debug"

  Scenario: 闭集外档位被拒且未落库（错误码化）
    When 尝试写入非法日志档位 "verbose"
    Then 应返回错误码 "settings.log-level-invalid"
    And 持久化日志档位应为 "info"

  Scenario: 旧版本备份缺 app_settings 表回默认（表缺失自愈兑底）
    When 移除 app_settings 表
    Then 持久化日志档位应为 "info"
