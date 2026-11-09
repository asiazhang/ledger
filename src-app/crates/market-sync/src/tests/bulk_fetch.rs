//! 行情批量取数面（ADR-0121 / issue #1374 / ADR-0130 决策 2）：批量面载荷的
//! 覆盖面语义与跨同步记忆状态机。新浪 `f_` 面的报文解析、请求形态与被拦截处置
//! 见 `tests/sina_fund.rs`（取数单元自身的测试）；本文件只测取数面作为**编排
//! 接缝**的行为——载荷覆盖面判据与跨同步记忆。

use std::time::Instant;

use crate::bulk::{
    BULK_DISABLE_PERIOD, BULK_FAILURE_THRESHOLD, BulkFetchCircuit, BulkNavPoint, FundBatch,
    FundNameDictionary, FundNavTable,
};

/// 载荷覆盖面语义（issue #1565）：名称字典是覆盖面判据（有名称即被面收录），
/// 净值表是「面是否给出可采信的价格点」的判据——货基错位行只进名称字典。
#[test]
fn fund_batch_separates_coverage_from_price_points() {
    let batch = FundBatch {
        names: FundNameDictionary::from([
            ("000001".to_string(), "华夏成长混合A".to_string()),
            // 货基错位行：名称在场、净值表无它（万份收益不产出价格点）。
            ("000198".to_string(), "天弘余额宝货币".to_string()),
        ]),
        nav: FundNavTable::from([(
            "000001".to_string(),
            BulkNavPoint {
                date: "2026-09-18".into(),
                nav: 1.333,
            },
        )]),
    };

    assert!(batch.covers("000001"), "普通行被面收录");
    assert!(batch.covers("000198"), "货基行同被面收录（有名称）");
    assert!(!batch.covers("999999"), "面未返回的代码是缺口");
    assert_eq!(batch.name_of("000001"), Some("华夏成长混合A"));
    assert_eq!(batch.name_of("000198"), Some("天弘余额宝货币"));
    assert_eq!(batch.name_of("999999"), None);
    assert!(
        batch.nav_of("000001").is_some(),
        "普通行给出可采信的最新单位净值"
    );
    assert!(
        batch.nav_of("000198").is_none(),
        "货基行不给价格点（已在取数层判形为 MoneyYield）"
    );
}

// ---------------------------------------------------------------------------
// 跨同步记忆（ADR-0121 决策 3）：连续失败达阈值停用一个期限，到期先半开试一次
// ---------------------------------------------------------------------------

#[test]
fn circuit_opens_only_after_threshold_consecutive_failures() {
    let mut circuit = BulkFetchCircuit::new();
    let now = Instant::now();

    for _ in 1..BULK_FAILURE_THRESHOLD {
        circuit.record_failure(now);
        assert!(
            circuit.should_attempt(now),
            "未达阈值不停用（避免把偶发一次失败升级成长期停用）"
        );
    }
    circuit.record_failure(now);
    assert!(circuit.is_disabled(now), "达阈值即停用");
    assert!(!circuit.should_attempt(now), "停用期内不再撞批量面");
    assert!(!circuit.should_attempt(now + BULK_DISABLE_PERIOD / 2));
}

#[test]
fn circuit_success_resets_the_consecutive_failure_count() {
    let mut circuit = BulkFetchCircuit::new();
    let now = Instant::now();

    for _ in 1..BULK_FAILURE_THRESHOLD {
        circuit.record_failure(now);
    }
    circuit.record_success();
    assert_eq!(circuit.consecutive_failures(), 0);
    circuit.record_failure(now);
    assert!(
        circuit.should_attempt(now),
        "成功清零后重新计数，未达阈值不停用"
    );
}

#[test]
fn circuit_half_opens_once_after_the_disable_period_then_recovers_on_success() {
    let mut circuit = BulkFetchCircuit::new();
    let now = Instant::now();
    for _ in 0..BULK_FAILURE_THRESHOLD {
        circuit.record_failure(now);
    }

    let after_period = now + BULK_DISABLE_PERIOD;
    assert!(circuit.should_attempt(after_period), "停用到期先半开试一次");
    assert!(
        !circuit.should_attempt(after_period),
        "半开试探只放行一次，结果未回来之前不再放行"
    );
    assert!(
        !circuit.should_attempt(after_period + BULK_DISABLE_PERIOD / 2),
        "试探窗口同样是整个停用期限"
    );

    circuit.record_success();
    assert!(
        circuit.should_attempt(after_period),
        "试探成功即解除停用、恢复常态"
    );
}

#[test]
fn circuit_failed_half_open_trial_restarts_the_disable_period() {
    let mut circuit = BulkFetchCircuit::new();
    let now = Instant::now();
    for _ in 0..BULK_FAILURE_THRESHOLD {
        circuit.record_failure(now);
    }

    let after_period = now + BULK_DISABLE_PERIOD;
    assert!(circuit.should_attempt(after_period));
    circuit.record_failure(after_period);
    assert!(
        !circuit.should_attempt(after_period),
        "半开试探失败即重新停用一个期限"
    );
    assert!(circuit.should_attempt(after_period + BULK_DISABLE_PERIOD));
}
