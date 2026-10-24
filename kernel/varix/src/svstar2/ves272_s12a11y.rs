//! ves272_s12a11y — VE-F4032 · RTL 无障碍
//! RTL 读屏导航序一致性（序单源复述）+ RTL 焦点镜像（焦点序镜像复述）+ 阿拉伯字形对比度语言门（复用语言门）+ RTL 切换控制扫描方向镜像（复用扫描参数）。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x6E40;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u16)]
pub enum RtlA11yCode {
    Ok = 0,
    OrderDiverge = SEG_BASE + 0x01,
    FocusDirWrong = SEG_BASE + 0x02,
    LanguageGateFail = SEG_BASE + 0x03,
    ScanDirWrong = SEG_BASE + 0x04,
    VoiceMissing = SEG_BASE + 0x05,
    ParamStale = SEG_BASE + 0x06,
    GateNotArabic = SEG_BASE + 0x07,
}

/// 序单源：视觉序 ↔ 读屏序 唯一真源，两侧派生一致。
#[derive(Clone)]
pub struct OrderSource {
    /// 视觉序（RTL 下为右→左的逻辑序）。
    pub visual: Vec<u32>,
    /// 读屏序（派生：RTL 下反转）。
    pub spoken: Vec<u32>,
    pub voice_narrated: bool,
}

impl OrderSource {
    /// 单源派生：spoken 恒由 visual 反转派生，禁止独立维护。
    pub fn derive_spoken(visual: &[u32]) -> Vec<u32> {
        let mut v = Vec::new();
        let n = visual.len();
        for i in 0..n {
            v.push(visual[n - 1 - i]);
        }
        v
    }
    pub fn from_visual(visual: &[u32]) -> Self {
        OrderSource { visual: visual.to_vec(), spoken: Self::derive_spoken(visual), voice_narrated: false }
    }
    /// 序读屏一致性断言：spoken 必须等于单源派生。
    pub fn consistent(&self) -> bool {
        self.spoken == Self::derive_spoken(&self.visual)
    }
    /// 读序复述单源：复述必须来自单源而非旁路。
    pub fn narrate(&mut self) -> Result<Vec<u32>, RtlA11yCode> {
        if !self.consistent() { return Err(RtlA11yCode::OrderDiverge); }
        self.voice_narrated = true;
        Ok(self.spoken.clone())
    }
}

/// 焦点镜像：RTL 焦点移动方向镜像（下一焦点 = 视觉左移），焦点序复用读屏焦点序。
#[derive(Clone)]
pub struct FocusMirror {
    pub order: Vec<u32>,
    pub idx: usize,
    pub rtl: bool,
}

impl FocusMirror {
    pub fn new(order: &[u32], rtl: bool) -> Self {
        FocusMirror { order: order.to_vec(), idx: 0, rtl }
    }
    /// RTL 下 next = 左移（镜像），LTR 下 next = 右移。
    pub fn next_focus(&mut self) -> Option<u32> {
        if self.order.is_empty() { return None; }
        if self.rtl {
            if self.idx == 0 { return None; }
            self.idx -= 1;
        } else {
            if self.idx + 1 >= self.order.len() { return None; }
            self.idx += 1;
        }
        Some(self.order[self.idx])
    }
    /// 焦点序镜像断言：RTL 序 == LTR 序反转。
    pub fn mirror_of(ltr_order: &[u32]) -> Vec<u32> {
        let mut v = Vec::new();
        let n = ltr_order.len();
        for i in 0..n { v.push(ltr_order[n - 1 - i]); }
        v
    }
    pub fn mirror_consistent(&self, ltr_order: &[u32]) -> bool {
        self.order == Self::mirror_of(ltr_order)
    }
}

/// 语言门：阿拉伯字形对比度检查仅在阿语（或 RTL 强相关字形族）生效——复用 S05 可读性阈值。
pub struct LanguageGate {
    pub threshold: f32,
}

impl LanguageGate {
    pub fn new(threshold: f32) -> Self { LanguageGate { threshold } }
    /// 门条件：字形族为阿拉伯族才进对比度检查；非阿语字形族走原语言门（断言）。
    pub fn applies(&self, family: &str) -> bool {
        family == "ar" || family == "arabic" || family == "fa" || family == "ur"
    }
    pub fn check(&self, family: &str, contrast: f32) -> Result<(), RtlA11yCode> {
        if !self.applies(family) { return Err(RtlA11yCode::GateNotArabic); }
        if contrast < self.threshold { return Err(RtlA11yCode::LanguageGateFail); }
        Ok(())
    }
}

/// 扫描方向镜像：RTL 切换控制扫描方向反转，扫描参数（步长/环绕/停顿）复用单源。
#[derive(Clone)]
pub struct ScanParams {
    pub step: u32,
    pub wrap: bool,
    pub dwell_ms: u32,
}

impl ScanParams {
    pub fn canonical() -> Self { ScanParams { step: 1, wrap: true, dwell_ms: 800 } }
}

#[derive(Clone)]
pub struct ScanMirror {
    pub params: ScanParams,
    pub rtl: bool,
    pub pos: i32,
    pub count: u32,
}

impl ScanMirror {
    pub fn new(count: u32, params: ScanParams, rtl: bool) -> Self {
        ScanMirror { params, rtl, pos: 0, count }
    }
    /// RTL 扫描方向镜像：next 在 LTR 下 +step，RTL 下 -step。
    pub fn step_scan(&mut self) -> Result<u32, RtlA11yCode> {
        if self.count == 0 { return Err(RtlA11yCode::ScanDirWrong); }
        let s = self.params.step as i32;
        let d = if self.rtl { -s } else { s };
        let mut np = self.pos + d;
        if self.params.wrap {
            let c = self.count as i32;
            np = ((np % c) + c) % c;
        } else if np < 0 || np >= self.count as i32 {
            return Ok(self.pos as u32);
        }
        self.pos = np;
        Ok(np as u32)
    }
    /// 参数复用断言：镜像扫描不得改写共享参数。
    pub fn params_unchanged(&self, canonical: &ScanParams) -> bool {
        self.params.step == canonical.step && self.params.wrap == canonical.wrap
            && self.params.dwell_ms == canonical.dwell_ms
    }
}

pub fn run_ves272_checks() -> CheckSet {
    let mut set = CheckSet::new("ves272_s12a11y");

    // 1) 序单源：spoken 由 visual 反转派生。
    let src = OrderSource::from_visual(&[1, 2, 3, 4]);
    set.add("C4032-01", src.spoken == [4, 3, 2, 1], "读屏序反转派生");

    // 2) 序读屏一致性成立。
    set.add("C4032-02", src.consistent(), "序一致");

    // 3) 旁路改写 spoken → 一致性断言红。
    let mut bad = OrderSource::from_visual(&[1, 2, 3]);
    bad.spoken = vec![1, 2, 3];
    set.add("C4032-03", !bad.consistent(), "旁路改写检出");

    // 4) 复述单源：narrate 返回单源 spoken 且置位。
    let mut n1 = OrderSource::from_visual(&[7, 8]);
    match n1.narrate() {
        Ok(v) => set.add("C4032-04", v == [8, 7] && n1.voice_narrated, "复述单源"),
        Err(_) => set.add("C4032-04", false, "复述失败"),
    }

    // 5) 不一致时复述拒。
    bad.spoken = vec![9];
    set.add("C4032-05", bad.narrate() == Err(RtlA11yCode::OrderDiverge), "不一致拒复述");

    // 6) 焦点序镜像：RTL 序 == LTR 反转。
    let fm = FocusMirror::new(&[10, 11, 12, 13], true);
    set.add("C4032-06", fm.mirror_consistent(&[10, 11, 12, 13]) && fm.order == [13, 12, 11, 10], "焦点序镜像");

    // 7) RTL next = 左移（序号递减）。
    let mut fm2 = FocusMirror::new(&[1, 2, 3], true);
    fm2.idx = 2;
    let a = fm2.next_focus();
    let b = fm2.next_focus();
    set.add("C4032-07", a == Some(2) && b == Some(1), "RTL 焦点左移");

    // 8) RTL 焦点到头返回 None（不环绕=移动方向断言）。
    let mut fm3 = FocusMirror::new(&[1, 2], true);
    set.add("C4032-08", fm3.next_focus() == None, "RTL 焦点左端止步");

    // 9) LTR next 正向。
    let mut fm4 = FocusMirror::new(&[5, 6, 7], false);
    fm4.idx = 1;
    set.add("C4032-09", fm4.next_focus() == Some(7), "LTR 焦点右移");

    // 10) 语言门：非阿语字形族不进门。
    let gate = LanguageGate::new(4.5);
    set.add("C4032-10", gate.check("en", 2.0) == Err(RtlA11yCode::GateNotArabic), "非阿语不进门");

    // 11) 阿语对比度不达 → 拒。
    set.add("C4032-11", gate.check("ar", 4.0) == Err(RtlA11yCode::LanguageGateFail), "阿语对比度拒");

    // 12) 阿语对比度达 → 放行。
    set.add("C4032-12", gate.check("ar", 4.6).is_ok() && gate.check("fa", 5.0).is_ok(), "阿语门放行");

    // 13) 语言门覆盖 fa/ur。
    set.add("C4032-13", gate.applies("fa") && gate.applies("ur") && !gate.applies("zh"), "门覆盖族表");

    // 14) 扫描方向镜像：RTL -step。
    let mut sm = ScanMirror::new(4, ScanParams::canonical(), true);
    sm.pos = 2;
    let p1 = sm.step_scan();
    set.add("C4032-14", p1 == Ok(1), "RTL 扫描反向");

    // 15) LTR 扫描 +step。
    let mut sm2 = ScanMirror::new(4, ScanParams::canonical(), false);
    let p2 = sm2.step_scan();
    set.add("C4032-15", p2 == Ok(1), "LTR 扫描正向");

    // 16) 环绕镜像：RTL 从 0 左移回绕到 3。
    let mut sm3 = ScanMirror::new(4, ScanParams::canonical(), true);
    let p3 = sm3.step_scan();
    set.add("C4032-16", p3 == Ok(3), "RTL 环绕回绕");

    // 17) 参数复用：镜像扫描不改写共享参数。
    let canon = ScanParams::canonical();
    set.add("C4032-17", sm.params_unchanged(&canon) && sm3.params_unchanged(&canon), "扫描参数复用");

    // 18) 参数漂移检出。
    let mut drifted = ScanParams::canonical();
    drifted.step = 3;
    let sm4 = ScanMirror::new(4, drifted, true);
    set.add("C4032-18", !sm4.params_unchanged(&canon), "参数漂移检出");

    // 19) 空序防护：空焦点序 / 零计数扫描拒。
    let mut fm9 = FocusMirror::new(&[], true);
    let mut sm9 = ScanMirror::new(0, ScanParams::canonical(), true);
    set.add("C4032-19", fm9.next_focus() == None && sm9.step_scan() == Err(RtlA11yCode::ScanDirWrong), "空序防护");

    // 20) 段位自查。
    set.add("C4032-20", SEG_BASE == 0x6E40, "段位 0x6E40");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn order_and_focus() {
        let src = OrderSource::from_visual(&[3, 1, 2]);
        assert!(src.consistent());
        let mut fm = FocusMirror::new(&[1, 2, 3], true);
        fm.idx = 1;
        assert_eq!(fm.next_focus(), Some(1));
    }
}
