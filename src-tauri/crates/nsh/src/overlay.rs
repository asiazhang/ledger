//! Overlay Suppression 注册表（issue #2021；旧壳 ADR-0035 语义的原生壳重建）。
//!
//! 语义沿旧壳裁决：**「弹层是否打开」显式上报，不做 DOM/视图树推断**——弹层
//! 在实例作用域持有 token，随自身 show 状态上报开/关（幂等）；快捷键处理器
//! 派发前读 [`has_open_overlay`] 判闸（翻页与周期步进两族快捷键共用同一闸门，
//! ADR-0140/0141 抑制三闸门之弹层闸；可编辑焦点闸由引擎 keymap 的
//! `When::Auto` 承担——文本编辑键在输入焦点期间不派发；桌面立桩无触控轴）。
//!
//! 与旧壳注册表（#845 修订后的有序栈）同形：打开序 = z 序，本票只消费
//! 「有弹层」判定；「关最上层」出口（closeTopOverlay）随系统返回桥接票接入，
//! 栈序在此先立。
//!
//! 失效模式反转（ADR-0035）：漏上报的症状是「弹层开着时快捷键误触发」——
//! 当场可见（headless 抑制测试钉住：删除 keymap 处理器中的闸门检查即红），
//! 不再是静默永久失效。
//!
//! 测试豁免（ADR-0060）：本模块测试经 crate 根 cfg(test) 整体放行六件套。

use std::collections::BTreeSet;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

/// 打开中的弹层 token 集（有序栈：BTreeSet 序 = token 序 ≈ 打开序；token 由
/// 进程级单调计数器发放）。锁毒化时取回内值——集合本身不含不变量，毒化不影
/// 响正确性。
static OPEN_OVERLAYS: Mutex<BTreeSet<u64>> = Mutex::new(BTreeSet::new());

/// token 发放计数器（进程级单调；每弹层实例一个，禁止共享——同类弹层多实例
/// 并存时共享 token 会互相污染开关状态，ADR-0035 接线要点）。
static NEXT_TOKEN: AtomicU64 = AtomicU64::new(0);

/// 弹层实例的开合上报句柄（实例作用域持有，随实例消亡自动撤销——卸载兜底，
/// 与旧壳 useOverlayReporting 的作用域销毁撤销同形）。
#[derive(Debug)]
pub struct OverlayToken {
    id: u64,
}

impl OverlayToken {
    /// 发放一个新 token（每弹层实例一次；不上报即不拦快捷键）。
    pub fn new() -> Self {
        let id = NEXT_TOKEN.fetch_add(1, Ordering::Relaxed);
        Self { id }
    }

    /// 上报开/关（幂等：重复上报无副作用，双通道重复安全——ADR-0035）。
    pub fn report(&self, open: bool) {
        let mut slot = open_overlays_slot();
        if open {
            slot.insert(self.id);
        } else {
            slot.remove(&self.id);
        }
    }
}

impl Default for OverlayToken {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for OverlayToken {
    /// 实例消亡（组件卸载 / 视图重建）时撤销上报：弹层不可能再开着。
    fn drop(&mut self) {
        open_overlays_slot().remove(&self.id);
    }
}

/// 是否有任一弹层打开（快捷键闸门的唯一判定入口；两族快捷键共用）。
pub fn has_open_overlay() -> bool {
    !open_overlays_slot().is_empty()
}

/// 注册表槽位守卫（毒化取回内值，见 [`OPEN_OVERLAYS`] 注）。
fn open_overlays_slot() -> std::sync::MutexGuard<'static, BTreeSet<u64>> {
    OPEN_OVERLAYS.lock().unwrap_or_else(|p| p.into_inner())
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    /// 串行锁：四个用例共用进程级全局注册表（OPEN_OVERLAYS），并行执行时
    /// 「未上报不拦」前置断言会被另一用例的在册 token 击穿（首发现于 #2032
    /// 期间：lib 测试默认并行、偶发 report_open_and_close_is_idempotent 红，
    /// 旧代码同样可复现）。与 tests.rs / tests/app_headless.rs 的 SERIAL 同形。
    static SERIAL: Mutex<()> = Mutex::new(());

    #[test]
    fn report_open_and_close_is_idempotent() {
        let _guard = SERIAL.lock().unwrap();
        let token = OverlayToken::new();
        assert!(!has_open_overlay(), "未上报不拦快捷键");
        token.report(true);
        assert!(has_open_overlay());
        token.report(true);
        assert!(has_open_overlay(), "重复上报开关无副作用");
        token.report(false);
        assert!(!has_open_overlay());
        token.report(false);
        assert!(!has_open_overlay(), "重复上报关闭无副作用");
    }

    #[test]
    fn multiple_tokens_require_all_closed() {
        let _guard = SERIAL.lock().unwrap();
        let a = OverlayToken::new();
        let b = OverlayToken::new();
        a.report(true);
        b.report(true);
        assert!(has_open_overlay());
        a.report(false);
        assert!(has_open_overlay(), "同类弹层多实例并存：关一个还有另一个");
        b.report(false);
        assert!(!has_open_overlay());
    }

    #[test]
    fn drop_withdraws_the_report() {
        let _guard = SERIAL.lock().unwrap();
        let token = OverlayToken::new();
        token.report(true);
        assert!(has_open_overlay());
        drop(token);
        assert!(!has_open_overlay(), "实例消亡自动撤销（卸载兜底）");
    }

    #[test]
    fn tokens_are_per_instance_and_never_collide() {
        let _guard = SERIAL.lock().unwrap();
        let a = OverlayToken::new();
        let b = OverlayToken::new();
        assert_ne!(
            a.id, b.id,
            "每实例一个 token，禁止共享（ADR-0035 接线要点）"
        );
        a.report(true);
        b.report(false);
        assert!(has_open_overlay(), "另一 token 的关闭不撤销本 token 的打开");
    }
}
