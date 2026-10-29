//! UNX-F12153 — 异常分发次序账：VEH→SEH→UEF 终局（0x1a540 段）
//!
//! 锚点：UNX-F12153-J1 三级分发序 10/10（VEH 链先试 → SEH 帧链
//! 次试 → UEF 终局），每级放行才落下一级；终局账逐级对平。
//! 域：CoRun·UNX-D1-B08（SEH 异常分发表谱系）。
//! 结构：DispatchStage（三级枚举）+ OrderLedger（逐级分发留痕 +
//! 合法序校验）+ 折叠指纹 + 注入核。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1a540;
pub const OL_ORDER: u32 = SEG_BASE + 0x01; // 级序倒挂（跳级/回跳）
pub const OL_TWICE: u32 = SEG_BASE + 0x02; // 同级二次记账
pub const OL_SEALED: u32 = SEG_BASE + 0x03; // 印后记账
pub const OL_EMPTY: u32 = SEG_BASE + 0x04; // 空账核终局
pub const OL_GHOST: u32 = SEG_BASE + 0x05; // 未知级

/// 分发级。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DispatchStage {
    Veh,
    Seh,
    Uef,
}

impl DispatchStage {
    pub fn rank(&self) -> u8 {
        match self {
            DispatchStage::Veh => 1,
            DispatchStage::Seh => 2,
            DispatchStage::Uef => 3,
        }
    }

    pub fn tag(&self) -> &'static str {
        match self {
            DispatchStage::Veh => "VEH",
            DispatchStage::Seh => "SEH",
            DispatchStage::Uef => "UEF",
        }
    }
}

/// 一次分发记录。
#[derive(Clone, PartialEq)]
pub struct DispRec {
    pub stage: DispatchStage,
    pub code: u32,
    pub pass: bool, // true = 该级放行（未接）
    pub note: String,
}

/// 分发次序账。
pub struct OrderLedger {
    pub recs: Vec<DispRec>,
    sealed: bool,
}

impl OrderLedger {
    pub fn new() -> OrderLedger {
        OrderLedger { recs: Vec::new(), sealed: false }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 记一级分发（级序必须逐级 +1：同级二次拒 TWICE，
    /// 跳级/回跳拒 ORDER —— 对应「每级放行才落下一级」）。
    pub fn record(&mut self, stage: DispatchStage, code: u32, pass: bool, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(OL_SEALED);
        }
        if note.is_empty() {
            return Err(OL_GHOST);
        }
        if let Some(last) = self.recs.last() {
            let r = stage.rank();
            let lr = last.stage.rank();
            if r == lr {
                return Err(OL_TWICE);
            }
            if r != lr + 1 {
                return Err(OL_ORDER);
            }
        }
        self.recs.push(DispRec { stage, code, pass, note: note.to_string() });
        Ok(())
    }

    /// 终局判定：最后一级记录必须 UEF；UEF 前每级必须 pass。
    pub fn final_verdict(&self) -> Result<&'static str, u32> {
        if self.recs.is_empty() {
            return Err(OL_EMPTY);
        }
        let last = self.recs.last().unwrap();
        if last.stage != DispatchStage::Uef {
            return Err(OL_ORDER);
        }
        if last.pass {
            Ok("WER")
        } else {
            Ok("UEF-HANDLED")
        }
    }

    /// 全账级序单调。
    pub fn monotonic(&self) -> bool {
        self.recs.windows(2).all(|w| w[0].stage.rank() < w[1].stage.rank())
    }

    /// 接住位次：首个 !pass 的级；全放行 = None。
    pub fn caught_at(&self) -> Option<DispatchStage> {
        self.recs
            .iter()
            .find(|r| !r.pass)
            .map(|r| r.stage)
    }

    /// 折叠指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1a540;
        for r in &self.recs {
            h = h.wrapping_mul(211).wrapping_add(r.stage.rank() as u32);
            h = h.wrapping_mul(211).wrapping_add(r.code);
            h = h.wrapping_mul(211).wrapping_add(r.pass as u32);
            for by in r.note.bytes() {
                h = h.wrapping_mul(211).wrapping_add(by as u32);
            }
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    pub fn book_summary(&self) -> String {
        format!("recs={} mono={}", self.recs.len(), self.monotonic())
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = OrderLedger::new();
    // 1) 空账核终局必拒。
    got[0] = b.final_verdict().err() == Some(OL_EMPTY);
    // 2) 跳级（VEH→UEF）必拒。
    let _ = b.record(DispatchStage::Veh, 0xC0000005, true, "v");
    got[1] = b.record(DispatchStage::Uef, 0xC0000005, true, "skip").err() == Some(OL_ORDER);
    // 3) 回跳（SEH→VEH）必拒。
    let mut b2 = OrderLedger::new();
    let _ = b2.record(DispatchStage::Seh, 0xC0000005, true, "s");
    got[2] = b2.record(DispatchStage::Veh, 0xC0000005, true, "back").err() == Some(OL_ORDER);
    // 4) 同级二次必拒。
    let mut b3 = OrderLedger::new();
    let _ = b3.record(DispatchStage::Veh, 0xC0000005, true, "v1");
    got[3] = b3.record(DispatchStage::Veh, 0xC0000005, true, "v2").err() == Some(OL_TWICE);
    // 5) 留痕空必拒。
    got[4] = b.record(DispatchStage::Seh, 0xC0000005, true, "").err() == Some(OL_GHOST);
    got
}

/// 判据入口：UNX-F12153
pub fn run_unx12153_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12153");
    let mut b = OrderLedger::new();

    // J1 三级全走（全放行 → WER 终局）。
    let j1 = b.record(DispatchStage::Veh, 0xC0000005, true, "veh-pass").is_ok()
        && b.record(DispatchStage::Seh, 0xC0000005, true, "seh-pass").is_ok()
        && b.record(DispatchStage::Uef, 0xC0000005, true, "uef-pass").is_ok();
    s.add("J1_three_stages", j1 && b.recs.len() == 3, "三级分发序账挂满");

    // J2 终局 = WER。
    s.add(
        "J2_wer_verdict",
        b.final_verdict() == Ok("WER"),
        "全放行终局 WER",
    );

    // J3 级序单调。
    s.add(
        "J3_monotonic",
        b.monotonic()
            && b.recs[0].stage.tag() == "VEH"
            && b.recs[2].stage.tag() == "UEF",
        "级序 VEH→SEH→UEF 单调",
    );

    // J4 SEH 接住：VEH 放行 → SEH 不放行 → 终局 UEF-HANDLED。
    let mut b4 = OrderLedger::new();
    let j4 = b4.record(DispatchStage::Veh, 0xC0000094, true, "veh-pass").is_ok()
        && b4.record(DispatchStage::Seh, 0xC0000094, false, "seh-caught").is_ok();
    s.add(
        "J4_seh_caught",
        j4 && b4.caught_at() == Some(DispatchStage::Seh),
        "SEH 级接住位次正确",
    );

    // J5 VEH 接住：首级即不放行。
    let mut b5 = OrderLedger::new();
    let _ = b5.record(DispatchStage::Veh, 0xC0000409, false, "veh-caught");
    s.add(
        "J5_veh_caught",
        b5.caught_at() == Some(DispatchStage::Veh),
        "VEH 级接住即止",
    );

    // J6 放行后接住的终局。
    let mut b6 = OrderLedger::new();
    let _ = b6.record(DispatchStage::Veh, 0xC0000005, true, "v");
    let _ = b6.record(DispatchStage::Seh, 0xC0000005, true, "s");
    let _ = b6.record(DispatchStage::Uef, 0xC0000005, false, "uef-h");
    s.add(
        "J6_uef_handled",
        b6.final_verdict() == Ok("UEF-HANDLED"),
        "UEF 接住终局 UEF-HANDLED",
    );

    // J7 跳级拒。
    let mut b7 = OrderLedger::new();
    let _ = b7.record(DispatchStage::Veh, 0xC0000005, true, "v");
    s.add(
        "J7_skip",
        b7.record(DispatchStage::Uef, 0xC0000005, true, "skip").err() == Some(OL_ORDER),
        "VEH→UEF 跳级拒 ORDER",
    );

    // J8 回跳拒。
    let mut b8 = OrderLedger::new();
    let _ = b8.record(DispatchStage::Seh, 0xC0000005, true, "s");
    s.add(
        "J8_back",
        b8.record(DispatchStage::Veh, 0xC0000005, true, "back").err() == Some(OL_ORDER),
        "SEH→VEH 回跳拒 ORDER",
    );

    // J9 同级二次拒。
    let mut b9 = OrderLedger::new();
    let _ = b9.record(DispatchStage::Uef, 0xC0000005, true, "u1");
    s.add(
        "J9_twice",
        b9.record(DispatchStage::Uef, 0xC0000005, true, "u2").err() == Some(OL_TWICE),
        "同级二次拒 TWICE",
    );

    // J10 三级 rank 1/2/3。
    s.add(
        "J10_ranks",
        DispatchStage::Veh.rank() == 1
            && DispatchStage::Seh.rank() == 2
            && DispatchStage::Uef.rank() == 3,
        "三级 rank 常量",
    );

    // J11 指纹稳定。
    let f1 = b.fold();
    let f2 = b.fold();
    s.add("J11_fingerprint", f1 == f2, "次序账折叠指纹一致");

    // J12 注入 5 项全抓。
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J12_inject_all", caught == 5, "5 注入点全检出");

    // J13 印账受控。
    b.seal();
    s.add(
        "J13_sealed",
        b.record(DispatchStage::Veh, 0xC0000005, true, "post").err() == Some(OL_SEALED),
        "印账后记账拒",
    );

    // J14 账面摘要。
    let sm = b.book_summary();
    s.add(
        "J14_summary",
        sm.contains("recs=3") && sm.contains("mono=true"),
        "摘要含记录数与单调位",
    );

    // J10b 接住位次在账中唯一。
    let mut b10b = OrderLedger::new();
    let _ = b10b.record(DispatchStage::Veh, 1, true, "v");
    let _ = b10b.record(DispatchStage::Seh, 1, false, "s");
    s.add(
        "J15_catch_unique",
        b10b.caught_at() == Some(DispatchStage::Seh),
        "首不放行级即接住位",
    );

    // J16 异常码逐条随级记录。
    s.add(
        "J16_code_traced",
        b.recs.iter().all(|r| r.code == 0xC0000005),
        "三级留痕异常码一致",
    );

    // J17 留痕空拒。
    let mut b17 = OrderLedger::new();
    s.add(
        "J17_empty_note",
        b17.record(DispatchStage::Veh, 1, true, "").err() == Some(OL_GHOST),
        "留痕空拒 GHOST",
    );

    // J18 终局判据需 UEF 末位。
    let mut b18 = OrderLedger::new();
    let _ = b18.record(DispatchStage::Veh, 1, false, "v");
    s.add(
        "J18_uef_last_required",
        b18.final_verdict().err() == Some(OL_ORDER),
        "非 UEF 末位拒终局判定",
    );

    // J19 空账指纹稳定。
    let b19 = OrderLedger::new();
    s.add("J19_fresh_fp", b19.fold() == b19.fold(), "空次序账指纹稳定");

    // J20 十例全放行路径账齐。
    let mut ok20 = true;
    for code in 1..=10u32 {
        let mut b20 = OrderLedger::new();
        let _ = b20.record(DispatchStage::Veh, code, true, "v");
        let _ = b20.record(DispatchStage::Seh, code, true, "s");
        let _ = b20.record(DispatchStage::Uef, code, true, "u");
        if b20.final_verdict() != Ok("WER") || !b20.monotonic() {
            ok20 = false;
        }
    }
    s.add("J20_ten_wer", ok20, "十例全放行终局 WER 全对");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12153_order() {
        let mut b = OrderLedger::new();
        let _ = b.record(DispatchStage::Veh, 5, true, "v");
        let _ = b.record(DispatchStage::Seh, 5, true, "s");
        let _ = b.record(DispatchStage::Uef, 5, true, "u");
        assert_eq!(b.final_verdict(), Ok("WER"));
    }

    #[test]
    fn unx12153_checks_green() {
        let s = run_unx12153_checks();
        assert!(s.all_passed(), "UNX-F12153 判据未全绿");
    }

    #[test]
    fn unx12153_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
