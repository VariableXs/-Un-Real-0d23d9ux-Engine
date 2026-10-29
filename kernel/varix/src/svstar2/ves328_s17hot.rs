//! ves328_s17hot — VE-F4088 · T05 语言切换热更新
//! 运行时切语言不重启：文本重绑定+布局重排+资源热换三步 + 原子切换协议（任一步失败整回滚——绝不半切）+ 半切检测自动回滚 + 骨架占位异步重排 + 动效无障碍（偏好减弱直切）+ 读屏即时跟随。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x9540;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HuCode {
    Ok = 0,
    RebindFail = SEG_BASE + 0x01,
    RelayoutFail = SEG_BASE + 0x02,
    SwapFail = SEG_BASE + 0x03,
    HalfDetected = SEG_BASE + 0x04,
    NotPrepared = SEG_BASE + 0x05,
}

/// 热更新三步。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HotStep { Rebind = 0, Relayout = 1, ResSwap = 2 }

impl HotStep {
    pub const ALL: [HotStep; 3] = [HotStep::Rebind, HotStep::Relayout, HotStep::ResSwap];
    pub fn name(self) -> &'static str {
        match self { HotStep::Rebind => "rebind", HotStep::Relayout => "relayout", HotStep::ResSwap => "res-swap" }
    }
}

/// 切换事务：目标语言 + 三步完成位图 + 原语言快照（回滚锚点）。
#[derive(Clone)]
pub struct SwitchTxn {
    pub from_lang: &'static str,
    pub to_lang: &'static str,
    pub step_done: [bool; 3],
    pub rolled_back: bool,
    pub committed: bool,
    /// 半切检测：三步全成或全无——否则半切。
    pub half_cut: bool,
}

impl SwitchTxn {
    pub fn new(from: &'static str, to: &'static str) -> SwitchTxn {
        SwitchTxn { from_lang: from, to_lang: to, step_done: [false; 3], rolled_back: false, committed: false, half_cut: false }
    }

    pub fn progress(&self) -> u8 { self.step_done.iter().filter(|s| **s).count() as u8 }

    /// 原子判定：commit 前 0 步或 3 步，rollback 后允许任意残留（已回滚不是半切）。
    pub fn atomic_invariant(&self) -> bool {
        if self.rolled_back { return true; }
        let p = self.progress();
        p == 0 || p == 3
    }
}

/// 热更新器。
pub struct HotUpdater {
    /// 当前生效语言。
    pub live_lang: &'static str,
    /// 已绑定文本键数（重绑口径）。
    pub bound_keys: usize,
    /// 待重排视口数。
    pub viewports: usize,
    /// 重排超时阈值 ms。
    pub relayout_timeout_ms: u32,
    /// 骨架占位激活（超时降级）。
    pub skeleton_on: bool,
    /// 动效减弱偏好（true=直切无动画）。
    pub reduce_motion: bool,
    /// 读屏跟随语言（域本色硬要求：切换后必须等于 live）。
    pub screen_reader_lang: &'static str,
    /// 回滚记录。
    pub rollbacks: Vec<(&'static str, HotStep)>,
}

impl HotUpdater {
    pub fn new(live: &'static str, viewports: usize) -> Self {
        HotUpdater {
            live_lang: live, bound_keys: 0, viewports, relayout_timeout_ms: 400,
            skeleton_on: false, reduce_motion: false, screen_reader_lang: live,
            rollbacks: Vec::new(),
        }
    }

    /// 步一：文本重绑定（引擎宕/键缺 → RebindFail）。
    pub fn rebind(&mut self, ok: bool) -> Result<(), HuCode> {
        if !ok { return Err(HuCode::RebindFail); }
        self.bound_keys += 1;
        Ok(())
    }

    /// 步二：布局重排（超时 → 骨架占位 + 异步完成仍算过；引擎异常 → RelayoutFail）。
    pub fn relayout(&mut self, elapsed_ms: u32, engine_ok: bool) -> Result<(), HuCode> {
        if !engine_ok { return Err(HuCode::RelayoutFail); }
        if elapsed_ms > self.relayout_timeout_ms {
            self.skeleton_on = true; // 骨架占位，异步完成
        }
        self.viewports = 0;
        Ok(())
    }

    /// 步三：资源热换。
    pub fn res_swap(&mut self, ok: bool) -> Result<(), HuCode> {
        if !ok { return Err(HuCode::SwapFail); }
        Ok(())
    }

    /// 原子切换协议：按序执行三步；任一步失败 → 整回滚 + 显性通告；成功 → 提交 + 读屏跟随。
    pub fn switch(&mut self, txn: &mut SwitchTxn, rebind_ok: bool, relayout_ms: u32, relayout_ok: bool, swap_ok: bool) -> Result<(), HuCode> {
        if txn.from_lang != self.live_lang || txn.committed || txn.rolled_back { return Err(HuCode::NotPrepared); }
        for (i, step) in HotStep::ALL.iter().enumerate() {
            let r = match step {
                HotStep::Rebind => self.rebind(rebind_ok),
                HotStep::Relayout => self.relayout(relayout_ms, relayout_ok),
                HotStep::ResSwap => self.res_swap(swap_ok),
            };
            match r {
                Ok(()) => txn.step_done[i] = true,
                Err(e) => return self.rollback(txn, e),
            }
        }
        txn.committed = true;
        self.live_lang = txn.to_lang;
        self.screen_reader_lang = txn.to_lang; // 读屏即时跟随（硬要求）
        Ok(())
    }

    /// 整回滚：三步位图清零 + 记录失败步 + 读屏同步回原语言。
    pub fn rollback(&mut self, txn: &mut SwitchTxn, cause: HuCode) -> Result<(), HuCode> {
        let fail_step = match cause {
            HuCode::RebindFail => HotStep::Rebind,
            HuCode::RelayoutFail => HotStep::Relayout,
            HuCode::SwapFail => HotStep::ResSwap,
            _ => HotStep::Rebind,
        };
        self.rollbacks.push((txn.to_lang, fail_step));
        txn.step_done = [false; 3];
        txn.rolled_back = true;
        self.bound_keys = 0;
        self.viewports = self.viewports.max(1);
        self.screen_reader_lang = txn.from_lang;
        self.skeleton_on = false;
        Err(cause)
    }

    /// 半切检测：外部注入的状态扫描——进度既非 0 也非 3 且未回滚未提交 → 自动回滚。
    pub fn half_cut_scan(&mut self, txn: &mut SwitchTxn) -> bool {
        if !txn.committed && !txn.rolled_back && !txn.atomic_invariant() {
            txn.half_cut = true;
            let _ = self.rollback(txn, HuCode::HalfDetected);
            return true;
        }
        false
    }

    /// 动效无障碍：偏好减弱 → 直切（无切换动画）。
    pub fn animation_ms(&self) -> u32 {
        if self.reduce_motion { 0 } else { 240 }
    }
}

/// 异步完成器：骨架占位下重排异步收尾。
pub struct AsyncRelayout { pub pending: Vec<&'static str> }

impl AsyncRelayout {
    pub fn new() -> Self { AsyncRelayout { pending: Vec::new() } }
    pub fn schedule(&mut self, vp: &'static str) { self.pending.push(vp); }
    pub fn complete_all(&mut self) -> usize { let n = self.pending.len(); self.pending.clear(); n }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves328_checks() -> CheckSet {
    let mut set = CheckSet::new("ves328");

    // C4088-01 三步名称单源。
    chk(&mut set, "C4088-01",
        HotStep::ALL[0].name() == "rebind" && HotStep::ALL[1].name() == "relayout" && HotStep::ALL[2].name() == "res-swap",
        "三步单源命名");

    // C4088-02 原子不变式：0/3 步合法，1/2 步半切。
    let mut t0 = SwitchTxn::new("zh", "ja");
    chk(&mut set, "C4088-02", t0.atomic_invariant(), "0 步合法");

    // C4088-03 成功切换：三步全成 + 读屏跟随。
    let mut hu = HotUpdater::new("zh", 8);
    let mut t1 = SwitchTxn::new("zh", "ja");
    chk(&mut set, "C4088-03", hu.switch(&mut t1, true, 100, true, true).is_ok()
        && hu.live_lang == "ja" && hu.screen_reader_lang == "ja", "切换成功 + 读屏跟随");

    // C4088-04 提交后原子不变式成立。
    chk(&mut set, "C4088-04", t1.committed && t1.atomic_invariant(), "3 步提交合法");

    // C4088-05 步三失败 → 整回滚：位图清零 + live 不动 + 读屏回原。
    let mut hu2 = HotUpdater::new("zh", 8);
    let mut t2 = SwitchTxn::new("zh", "ko");
    let r = hu2.switch(&mut t2, true, 100, true, false);
    chk(&mut set, "C4088-05", r == Err(HuCode::SwapFail) && t2.rolled_back
        && t2.step_done == [false; 3] && hu2.live_lang == "zh" && hu2.screen_reader_lang == "zh",
        "SwapFail 整回滚 绝不半切");

    // C4088-06 回滚留痕（目标语言+失败步）。
    chk(&mut set, "C4088-06", hu2.rollbacks == vec![("ko", HotStep::ResSwap)], "回滚记录 ko@res-swap");

    // C4088-07 步一失败同样整回滚。
    let mut t3 = SwitchTxn::new("zh", "de");
    let r3 = hu2.switch(&mut t3, false, 100, true, true);
    chk(&mut set, "C4088-07", r3 == Err(HuCode::RebindFail) && hu2.live_lang == "zh", "RebindFail 整回滚");

    // C4088-08 已回滚事务不可重放（NotPrepared）。
    chk(&mut set, "C4088-08", hu2.switch(&mut t2, true, 100, true, true) == Err(HuCode::NotPrepared), "回滚事务拒重放");

    // C4088-09 重排超时 → 骨架占位激活。
    let mut hu3 = HotUpdater::new("zh", 4);
    let _ = hu3.relayout(900, true);
    chk(&mut set, "C4088-09", hu3.skeleton_on && hu3.viewports == 0, "超时 900>400 骨架占位");

    // C4088-10 半切检测：注入 2 步态 → 自动回滚。
    let mut hu4 = HotUpdater::new("zh", 2);
    let mut t4 = SwitchTxn::new("zh", "fr");
    t4.step_done = [true, true, false];
    chk(&mut set, "C4088-10", hu4.half_cut_scan(&mut t4) && t4.half_cut && t4.rolled_back, "半切扫描 → 自动回滚");

    // C4088-11 动效无障碍：偏好减弱直切。
    hu4.reduce_motion = true;
    chk(&mut set, "C4088-11", hu4.animation_ms() == 0, "reduce-motion → 0ms 直切");

    // C4088-12 异步重排收尾。
    let mut ar = AsyncRelayout::new();
    ar.schedule("vp-main");
    ar.schedule("vp-side");
    chk(&mut set, "C4088-12", ar.complete_all() == 2 && ar.pending.is_empty(), "骨架下异步收尾 2 视口");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves328_smoke() {
        let set = run_ves328_checks();
        assert!(set.failed_count() == 0, "ves328 red: {}", set.red_items_text());
    }

    #[test]
    fn ves328_atomic() {
        let mut hu = HotUpdater::new("zh", 1);
        let mut t = SwitchTxn::new("zh", "en");
        assert!(hu.switch(&mut t, true, 50, true, true).is_ok());
        assert_eq!(hu.live_lang, "en");
        let mut t2 = SwitchTxn::new("en", "zh");
        assert_eq!(hu.switch(&mut t2, true, 50, false, true), Err(HuCode::RelayoutFail));
        assert!(t2.rolled_back);
        assert_eq!(hu.live_lang, "en");
    }
}
