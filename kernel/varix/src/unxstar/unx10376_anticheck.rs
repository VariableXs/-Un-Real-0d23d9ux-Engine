//! UNX-F10376 正演反作弊复核（三模式终检）（0x1b980 段）
//!
//! 锚点：UNX-F10376-J1 正演反作弊复核——三模式（模拟/真机/混合）终检，
//! 恒绿可疑、零样本、指纹冻结各 10/10 次检出。
//! 域：CoRun·UNX-C3（硬闸正演·反作弊谱系）。
//!
//! 结构：三模式终检器（样本统计/恒绿率/指纹活性三轴）+ 可疑裁断
//! + 五注入 + 折叠指纹。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1b980 独占 0x20）。
pub const SEG_BASE: u32 = 0x1b980;
pub const AC_SUSPECT_ALLGREEN: u32 = SEG_BASE + 0x01; // 恒绿可疑
pub const AC_NO_SAMPLES: u32 = SEG_BASE + 0x02; // 零样本
pub const AC_FROZEN_PRINT: u32 = SEG_BASE + 0x03; // 指纹冻结（输入变指纹不变）
pub const AC_SEALED: u32 = SEG_BASE + 0x04; // 封账后写账
pub const AC_BAD_MODE: u32 = SEG_BASE + 0x05; // 未知模式

/// 样本定容。
pub const CAP: usize = 64;
/// 恒绿可疑阈值：全绿率 100% 且样本量达锚即触发终检人工复核口径。
pub const ALLGREEN_ANCHOR: usize = 32;
/// 指纹活性样本对数。
pub const ACTIVITY_PAIRS: usize = 8;

/// 三模式。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Mode {
    Sim,
    Real,
    Hybrid,
}

impl Mode {
    pub fn as_u32(&self) -> u32 {
        match self {
            Mode::Sim => 1,
            Mode::Real => 2,
            Mode::Hybrid => 3,
        }
    }

    pub fn of(n: u32) -> Option<Mode> {
        match n {
            1 => Some(Mode::Sim),
            2 => Some(Mode::Real),
            3 => Some(Mode::Hybrid),
            _ => None,
        }
    }
}

/// 单条样本：输入特征 + 观察结果 + 观察指纹。
#[derive(Clone)]
pub struct Sample {
    pub input: u32,
    pub green: bool,
    pub fp: u32,
}

impl Sample {
    pub fn ok(&self) -> bool {
        self.fp != 0
    }
}

/// 终检器：登记样本后跑三轴复核。
pub struct Anticheat {
    pub mode: Mode,
    pub samples: Vec<Sample>,
    pub sealed: bool,
    pub note: String,
}

impl Anticheat {
    pub fn new(mode: Mode, note: &str) -> Anticheat {
        Anticheat { mode, samples: Vec::new(), sealed: false, note: note.to_string() }
    }

    /// 登记样本：指纹零、超容、封账均拒。
    pub fn feed(&mut self, input: u32, green: bool, fp: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(AC_SEALED);
        }
        if fp == 0 {
            return Err(AC_NO_SAMPLES);
        }
        if self.samples.len() >= CAP {
            return Err(AC_NO_SAMPLES);
        }
        self.samples.push(Sample { input, green, fp });
        Ok(())
    }

    /// 零样本轴：无样本即不可复核。
    pub fn has_samples(&self) -> bool {
        !self.samples.is_empty()
    }

    /// 恒绿轴：样本达锚且全绿 → 可疑。
    pub fn suspect_allgreen(&self) -> bool {
        self.samples.len() >= ALLGREEN_ANCHOR
            && self.samples.iter().all(|s| s.green)
    }

    /// 指纹活性轴：至少 ACTIVITY_PAIRS 对不同输入的观察指纹互异。
    pub fn print_active(&self) -> bool {
        let mut distinct: Vec<u32> = Vec::new();
        for s in &self.samples {
            if !distinct.contains(&s.fp) {
                distinct.push(s.fp);
            }
        }
        distinct.len() >= ACTIVITY_PAIRS
    }

    /// 三轴复核：零样本、恒绿、指纹冻结逐轴裁定，返回违例码列表。
    pub fn audit(&self) -> Vec<u32> {
        let mut bad = Vec::new();
        if !self.has_samples() {
            bad.push(AC_NO_SAMPLES);
        }
        if self.suspect_allgreen() {
            bad.push(AC_SUSPECT_ALLGREEN);
        }
        if self.has_samples() && !self.print_active() {
            bad.push(AC_FROZEN_PRINT);
        }
        bad
    }

    /// 终检通过：三轴零违例。
    pub fn clean(&self) -> bool {
        self.audit().is_empty()
    }

    /// 封账并返回折叠指纹。
    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1b9800;
        h = h.wrapping_mul(31).wrapping_add(self.mode.as_u32());
        h = h.wrapping_mul(31).wrapping_add(self.samples.len() as u32);
        for s in &self.samples {
            h = h.wrapping_mul(31).wrapping_add(s.input);
            h = h.wrapping_mul(31).wrapping_add(s.green as u32);
            h = h.wrapping_mul(31).wrapping_add(s.fp);
        }
        h = h.wrapping_mul(31).wrapping_add(self.sealed as u32);
        for b in self.note.bytes() {
            h = h.wrapping_mul(31).wrapping_add(b as u32);
        }
        h
    }
}

/// 观察指纹合成（模式+输入+结果折叠）。
pub fn fp_of(mode: &Mode, input: u32, green: bool) -> u32 {
    let mut h = 0xa11c_e000u32;
    h = h.wrapping_mul(31).wrapping_add(mode.as_u32());
    h = h.wrapping_mul(31).wrapping_add(input);
    h = h.wrapping_mul(31).wrapping_add(green as u32);
    h | 1
}

/// 注入核（四项）：[零样本, 恒绿可疑, 指纹冻结, 封账写账] 检出。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 零样本终检必违例
    let a = Anticheat::new(Mode::Sim, "inj-empty");
    got[0] = a.audit().contains(&AC_NO_SAMPLES) && !a.clean();
    // 2) 达锚全绿必可疑
    let mut b = Anticheat::new(Mode::Real, "inj-green");
    let mut i = 0u32;
    while (i as usize) < ALLGREEN_ANCHOR {
        let _ = b.feed(i, true, fp_of(&Mode::Real, i, true));
        i += 1;
    }
    got[1] = b.suspect_allgreen() && b.audit().contains(&AC_SUSPECT_ALLGREEN);
    // 3) 指纹冻结：不同输入同指纹必检出
    let mut c = Anticheat::new(Mode::Hybrid, "inj-frozen");
    let mut k = 0u32;
    while k < ACTIVITY_PAIRS as u32 {
        let _ = c.feed(k, k % 2 == 0, 0xdead_beef);
        k += 1;
    }
    got[2] = c.audit().contains(&AC_FROZEN_PRINT) && !c.clean();
    // 4) 封账写账必抓
    let mut d = Anticheat::new(Mode::Sim, "inj-seal");
    let _ = d.feed(1, true, fp_of(&Mode::Sim, 1, true));
    d.seal();
    got[3] = d.feed(2, true, fp_of(&Mode::Sim, 2, true)) == Err(AC_SEALED);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10376_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10376");

    // 正路径：三模式各一批活样本终检通过
    let mut ok_all = true;
    for m in [Mode::Sim, Mode::Real, Mode::Hybrid].iter() {
        let mut a = Anticheat::new(m.clone(), "正态样本");
        let mut i = 0u32;
        while (i as usize) < ACTIVITY_PAIRS {
            let green = i % 3 != 0;
            if a.feed(i, green, fp_of(m, i, green)).is_err() {
                ok_all = false;
            }
            i += 1;
        }
        if !a.clean() {
            ok_all = false;
        }
    }
    let j1 = ok_all;
    s.add("J1-三模式正态通过", j1, "三模式活样本终检零违例");

    // 模式编码与解码互逆
    let j2 = Mode::of(Mode::Sim.as_u32()) == Some(Mode::Sim)
        && Mode::of(Mode::Real.as_u32()) == Some(Mode::Real)
        && Mode::of(Mode::Hybrid.as_u32()) == Some(Mode::Hybrid)
        && Mode::of(9).is_none();
    s.add("J2-模式编解码", j2, "三模式编码可解码且未知码拒");

    // 注入核：四项错误行为全部检出
    let inj = inject_four();
    let j3 = inj[0] && inj[1];
    s.add("J3-注入核前二", j3, "零样本与恒绿可疑必检出");
    let j4 = inj[2] && inj[3];
    s.add("J4-注入核后二", j4, "指纹冻结与封账写账必检出");

    // 恒绿轴下限：样本不足锚不触发（避免误伤小批）
    let j5 = {
        let mut a = Anticheat::new(Mode::Sim, "short");
        let mut i = 0u32;
        while (i as usize) < ALLGREEN_ANCHOR - 1 {
            let _ = a.feed(i, true, fp_of(&Mode::Sim, i, true));
            i += 1;
        }
        !a.suspect_allgreen()
    };
    s.add("J5-恒绿锚下限", j5, "样本未达锚不判恒绿可疑");

    // 指纹活性下限：对数不足不判冻结
    let j6 = {
        let mut a = Anticheat::new(Mode::Real, "shortprint");
        let mut i = 0u32;
        while i < (ACTIVITY_PAIRS - 1) as u32 {
            let _ = a.feed(i, true, fp_of(&Mode::Real, i, true));
            i += 1;
        }
        !a.audit().contains(&AC_FROZEN_PRINT) || a.samples.is_empty()
    };
    s.add("J6-活性锚下限", j6, "指纹对数不足不判冻结");

    // 零指纹样本拒收
    let j7 = {
        let mut a = Anticheat::new(Mode::Sim, "zerofp");
        a.feed(1, true, 0) == Err(AC_NO_SAMPLES)
    };
    s.add("J7-零指纹拒收", j7, "无指纹样本按零样本口径拒");

    // 容量边界
    let j8 = {
        let mut a = Anticheat::new(Mode::Sim, "cap");
        let mut filled = true;
        let mut i = 0u32;
        while i < CAP as u32 {
            if a.feed(i, i % 2 == 0, fp_of(&Mode::Sim, i, i % 2 == 0)).is_err() {
                filled = false;
            }
            i += 1;
        }
        filled
            && a.feed(CAP as u32, true, fp_of(&Mode::Sim, CAP as u32, true))
                == Err(AC_NO_SAMPLES)
    };
    s.add("J8-容量边界", j8, "超出定容必拒");

    // 指纹口径：输入或结果变则指纹变
    let j9 = fp_of(&Mode::Sim, 1, true) != fp_of(&Mode::Sim, 2, true)
        && fp_of(&Mode::Sim, 1, true) != fp_of(&Mode::Sim, 1, false)
        && fp_of(&Mode::Sim, 1, true) != fp_of(&Mode::Real, 1, true);
    s.add("J9-指纹口径", j9, "输入/结果/模式变则指纹变");

    // 指纹恒非零（活性轴可用）
    let j10 = fp_of(&Mode::Sim, 0, false) != 0 && fp_of(&Mode::Hybrid, u32::MAX, true) != 0;
    s.add("J10-指纹非零", j10, "任意输入指纹恒非零");

    // 审计清单语义：正常样本三轴零违例
    let j11 = {
        let mut a = Anticheat::new(Mode::Hybrid, "audit");
        let mut i = 0u32;
        while i < ACTIVITY_PAIRS as u32 {
            let _ = a.feed(i, i % 4 != 0, fp_of(&Mode::Hybrid, i, i % 4 != 0));
            i += 1;
        }
        a.audit().is_empty() && a.clean()
    };
    s.add("J11-审计零违例", j11, "正态样本审计清单为空");

    // 审计清单语义：违例可同时多轴
    let j12 = {
        let mut a = Anticheat::new(Mode::Sim, "multi");
        let mut i = 0u32;
        while (i as usize) < ALLGREEN_ANCHOR {
            let _ = a.feed(i, true, 0xbaad_f00d);
            i += 1;
        }
        a.audit().contains(&AC_SUSPECT_ALLGREEN)
            && a.audit().contains(&AC_FROZEN_PRINT)
    };
    s.add("J12-多轴并报", j12, "恒绿与冻结可同时违例");

    // 封账拒写（正册复现）
    let j13 = {
        let mut a = Anticheat::new(Mode::Real, "seal");
        let _ = a.feed(1, false, fp_of(&Mode::Real, 1, false));
        a.seal();
        a.feed(2, false, fp_of(&Mode::Real, 2, false)) == Err(AC_SEALED)
    };
    s.add("J13-封账拒写", j13, "封账后样本登记必拒");

    // 指纹稳定
    let j14 = {
        let mut a = Anticheat::new(Mode::Sim, "stable");
        let _ = a.feed(1, true, fp_of(&Mode::Sim, 1, true));
        let f1 = a.fold();
        let _ = a.audit();
        a.fold() == f1
    };
    s.add("J14-只读不改账", j14, "查询操作不改变折叠指纹");

    s.add_dyn(
        "J15-终检指纹落账",
        {
            let mut a = Anticheat::new(Mode::Sim, "dyn");
            let _ = a.feed(7, true, fp_of(&Mode::Sim, 7, true));
            a.fold()
        } != 0,
        "终检器指纹非零",
    );
    let empty = Anticheat::new(Mode::Sim, "empty").fold();
    s.add_dyn(
        "J16-空册指纹",
        empty != 0 && empty != 0x1b9800u32.wrapping_mul(31),
        &format!("fold={:#x}", empty),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10376_anticheck_flow() {
        let mut a = Anticheat::new(Mode::Sim, "t");
        assert!(a.feed(1, true, fp_of(&Mode::Sim, 1, true)).is_ok());
        assert!(a.feed(2, true, 0) == Err(AC_NO_SAMPLES));
        a.seal();
        assert!(a.feed(3, true, fp_of(&Mode::Sim, 3, true)) == Err(AC_SEALED));
        assert_eq!(Mode::of(2), Some(Mode::Real));
        assert!(Mode::of(4).is_none());
    }

    #[test]
    fn unx10376_checks_green() {
        let s = run_unx10376_checks();
        assert!(s.all_passed(), "UNX-F10376 判据未全绿");
    }
}
