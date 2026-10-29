//! UNX-F11103 LTP 对照面总对账（三态总账终核）（0x1f583 段）
//!
//! 锚点：UNX-F11103-J1 三态总账终核对平（10/10），豁免率终核在闸内（10/10）。
//! 域：CoRun·UNX-C4-B36（LTP 对照面）。
//!
//! 结构：三态格（TriState：pass/fail/skip 三计数，千分率算豁免）+ 对照面
//! （LtpFace：B30–B35 六批三态登记 → 总账申报 → 豁免率闸 → 终核）+ 冻结根
//! + 注入核（三态和不平/重号/豁免超闸/缺批四路必抓）。

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1f583 独占 0x20）。
pub const SEG_BASE: u32 = 0x1f583;
pub const LT_NOROW: u32 = SEG_BASE + 0x01; // 批号缺行
pub const LT_DUP: u32 = SEG_BASE + 0x02; // 批号重号
pub const LT_RANGE: u32 = SEG_BASE + 0x03; // 批号越段
pub const LT_UNBAL: u32 = SEG_BASE + 0x04; // 三态和不平
pub const LT_GATE: u32 = SEG_BASE + 0x05; // 豁免率超闸
pub const LT_ZERO: u32 = SEG_BASE + 0x06; // 分母为零
pub const LT_FROZEN: u32 = SEG_BASE + 0x07; // 冻结后写账
pub const LT_RED: u32 = SEG_BASE + 0x08; // 有失败项

/// 段内六批（B30–B35）。
pub const BATCH_FIRST: u32 = 30;
pub const BATCH_LAST: u32 = 35;
pub const ROWS: usize = 6;
pub const CAP: usize = 8;
/// 每批对照条数。
pub const PER_BATCH: u32 = 20;
/// 全局豁免率闸（千分率 30 = 3%）。
pub const GATE_PERMILLE: u32 = 30;
/// 对照面标（fold 种子）。
pub const LTP_TAG: u64 = 0x1f5_8383;
/// 指纹步进。
pub const FP_STEP: u64 = 0x100000001b3;

/// 三态格：通过与失败与豁免三计数。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TriState {
    pub pass: u32,
    pub fail: u32,
    pub skip: u32,
}

impl TriState {
    pub fn new(pass: u32, fail: u32, skip: u32) -> TriState {
        TriState { pass, fail, skip }
    }

    pub fn total(&self) -> u32 {
        self.pass.wrapping_add(self.fail).wrapping_add(self.skip)
    }

    /// 豁免千分率：分母为零时不判（None）。
    pub fn exempt_permille(&self) -> Option<u32> {
        let t = self.total();
        if t == 0 {
            None
        } else {
            Some(self.skip.wrapping_mul(1000) / t)
        }
    }

    /// 三态全绿：零失败。
    pub fn green(&self) -> bool {
        self.fail == 0
    }

    pub fn render(&self) -> String {
        format!("p{}|f{}|s{}", self.pass, self.fail, self.skip)
    }
}

/// 单批三态行。
#[derive(Clone, Debug, PartialEq)]
pub struct LtpBatch {
    pub no: u32,
    pub tri: TriState,
}

/// LTP 对照面：六批三态 + 总账申报 + 豁免闸。
#[derive(Clone, Debug)]
pub struct LtpFace {
    pub rows: Vec<LtpBatch>,
    pub gate: u32,
    pub declared_total: u32,
    pub declared_skip: u32,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl LtpFace {
    pub fn new(gate: u32) -> LtpFace {
        LtpFace {
            rows: Vec::new(),
            gate,
            declared_total: 0,
            declared_skip: 0,
            frozen: false,
            root: None,
        }
    }

    /// 批行登记：越段/重号/超容/三态和不为每批条数皆拒。
    pub fn add(&mut self, no: u32, tri: TriState) -> Result<(), u32> {
        if self.frozen {
            return Err(LT_FROZEN);
        }
        if no < BATCH_FIRST || no > BATCH_LAST {
            return Err(LT_RANGE);
        }
        if self.rows.iter().any(|r| r.no == no) {
            return Err(LT_DUP);
        }
        if self.rows.len() >= CAP {
            return Err(LT_UNBAL);
        }
        if tri.total() != PER_BATCH {
            return Err(LT_UNBAL);
        }
        self.rows.push(LtpBatch { no, tri });
        self.rows.sort_by_key(|r| r.no);
        Ok(())
    }

    /// 总账合计。
    pub fn sum(&self) -> TriState {
        let mut p = 0u32;
        let mut f = 0u32;
        let mut s = 0u32;
        let mut k = 0usize;
        while k < self.rows.len() {
            p = p.wrapping_add(self.rows[k].tri.pass);
            f = f.wrapping_add(self.rows[k].tri.fail);
            s = s.wrapping_add(self.rows[k].tri.skip);
            k += 1;
        }
        TriState::new(p, f, s)
    }

    /// 总账申报：申报总数与豁免数须与实算一致。
    pub fn declare(&mut self, total: u32, skip: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(LT_FROZEN);
        }
        let s = self.sum();
        if total != s.total() || skip != s.skip {
            return Err(LT_UNBAL);
        }
        self.declared_total = total;
        self.declared_skip = skip;
        Ok(())
    }

    /// 红批数（有失败项的批）。
    pub fn reds(&self) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.rows.len() {
            if !self.rows[k].tri.green() {
                n += 1;
            }
            k += 1;
        }
        n
    }

    /// 缺批点名。
    pub fn missing(&self) -> Vec<u32> {
        let mut m: Vec<u32> = Vec::new();
        let mut no = BATCH_FIRST;
        while no <= BATCH_LAST {
            if !self.rows.iter().any(|r| r.no == no) {
                m.push(no);
            }
            no += 1;
        }
        m
    }

    /// 全局豁免千分率（空面为 None）。
    pub fn exempt_permille(&self) -> Option<u32> {
        self.sum().exempt_permille()
    }

    /// 豁免率在闸内：有分母且千分率不超闸。
    pub fn in_gate(&self) -> bool {
        match self.exempt_permille() {
            Some(r) => r <= self.gate,
            None => false,
        }
    }

    /// 终核：六批齐、零红、总账对平、豁免在闸内。
    pub fn verdict(&self) -> bool {
        self.rows.len() == ROWS
            && self.missing().is_empty()
            && self.reds() == 0
            && self.declared_total == self.sum().total()
            && self.declared_total == ROWS as u32 * PER_BATCH
            && self.declared_skip == self.sum().skip
            && self.in_gate()
    }

    /// 冻结并落根。
    pub fn seal(&mut self) -> u64 {
        self.frozen = true;
        let r = self.fold();
        self.root = Some(r);
        r
    }

    /// 面级指纹：批号 + 三态 + 申报 + 闸值。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = LTP_TAG;
        let mut k = 0usize;
        while k < self.rows.len() {
            let r = &self.rows[k];
            h = (h ^ r.no as u64).wrapping_mul(FP_STEP);
            h = (h ^ r.tri.pass as u64).wrapping_mul(FP_STEP);
            h = (h ^ r.tri.fail as u64).wrapping_mul(FP_STEP);
            h = (h ^ r.tri.skip as u64).wrapping_mul(FP_STEP);
            k += 1;
        }
        h = (h ^ self.gate as u64).wrapping_mul(FP_STEP);
        if self.declared_total != 0 || self.declared_skip != 0 {
            h = (h ^ self.declared_total as u64).wrapping_mul(FP_STEP);
            h = (h ^ self.declared_skip as u64).wrapping_mul(FP_STEP);
        }
        h
    }
}

/// 批次三态样本：每批 20 条，仅 B33 豁免 1 条（全局 1/120 = 8‰）。
pub fn sample_tri(no: u32) -> TriState {
    if no == 33 {
        TriState::new(19, 0, 1)
    } else {
        TriState::new(20, 0, 0)
    }
}

/// 标准对照面：六批登记并申报总账。
pub fn build_face() -> (LtpFace, bool) {
    let mut f = LtpFace::new(GATE_PERMILLE);
    let mut ok = true;
    let mut no = BATCH_FIRST;
    while no <= BATCH_LAST {
        if f.add(no, sample_tri(no)).is_err() {
            ok = false;
        }
        no += 1;
    }
    let s = f.sum();
    if f.declare(s.total(), s.skip).is_err() {
        ok = false;
    }
    (f, ok)
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 三态和不平必拒（19 条充 20）
    let mut a = LtpFace::new(GATE_PERMILLE);
    got[0] = a.add(BATCH_FIRST, TriState::new(19, 0, 0)).err() == Some(LT_UNBAL);
    // 2) 重号必拒
    let mut b = LtpFace::new(GATE_PERMILLE);
    let _ = b.add(BATCH_FIRST, TriState::new(20, 0, 0));
    got[1] = b.add(BATCH_FIRST, TriState::new(20, 0, 0)).err() == Some(LT_DUP);
    // 3) 豁免超闸必检出（每批豁免 2 → 100‰）
    let mut c = LtpFace::new(GATE_PERMILLE);
    let mut no = BATCH_FIRST;
    while no <= BATCH_LAST {
        let _ = c.add(no, TriState::new(18, 0, 2));
        no += 1;
    }
    let s = c.sum();
    let _ = c.declare(s.total(), s.skip);
    got[2] = !c.in_gate() && c.exempt_permille() == Some(100);
    // 4) 缺批终核为假
    let mut d = LtpFace::new(GATE_PERMILLE);
    let mut n2 = BATCH_FIRST;
    while n2 < BATCH_LAST {
        let _ = d.add(n2, TriState::new(20, 0, 0));
        n2 += 1;
    }
    let s2 = d.sum();
    let _ = d.declare(s2.total(), s2.skip);
    got[3] = !d.verdict() && d.missing().len() == 1;
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx11103_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11103");

    let (mut face, built) = build_face();
    // J1 六批三态齐备
    s.add(
        "J1-六批三态齐",
        built && face.rows.len() == ROWS && face.missing().is_empty(),
        "B30–B35 六批三态逐批在册",
    );

    // J2 三态总账终核对平
    let sum = face.sum();
    s.add(
        "J2-三态总账对平",
        sum.total() == 120 && sum.pass == 119 && sum.skip == 1 && face.declared_total == 120,
        "120 条三态合计与申报对平",
    );

    // J3 零失败断言
    s.add("J3-零失败", face.reds() == 0 && sum.fail == 0, "六批失败计数全零");

    // J4 豁免率终核在闸内
    s.add(
        "J4-豁免在闸内",
        face.in_gate() && face.exempt_permille() == Some(8),
        "全局豁免 1/120 = 8‰，低于 30‰ 闸",
    );

    // J5 超闸必检出
    let j5 = {
        let mut f = LtpFace::new(GATE_PERMILLE);
        let mut no = BATCH_FIRST;
        while no <= BATCH_LAST {
            let _ = f.add(no, TriState::new(18, 0, 2));
            no += 1;
        }
        let t = f.sum();
        let _ = f.declare(t.total(), t.skip);
        !f.in_gate() && f.exempt_permille() == Some(100)
    };
    s.add("J5-超闸检出", j5, "豁免 100‰ 时闸判为假");

    // J6 三态和不平拒
    let j6 = {
        let mut f = LtpFace::new(GATE_PERMILLE);
        f.add(BATCH_FIRST, TriState::new(10, 5, 4)).err() == Some(LT_UNBAL)
    };
    s.add("J6-三态不平拒", j6, "每批三态和不为 20 必拒");

    // J7 越段与重号拒
    let j7 = {
        let mut f = LtpFace::new(GATE_PERMILLE);
        let r1 = f.add(BATCH_LAST + 1, TriState::new(20, 0, 0)).err() == Some(LT_RANGE);
        let _ = f.add(BATCH_FIRST, TriState::new(20, 0, 0));
        r1 && f.add(BATCH_FIRST, TriState::new(20, 0, 0)).err() == Some(LT_DUP)
    };
    s.add("J7-越段重号拒", j7, "越段批号与重号登记两路必拒");

    // J8 分母为零不判
    let j8 = {
        let z = TriState::new(0, 0, 0);
        z.exempt_permille().is_none() && !LtpFace::new(GATE_PERMILLE).in_gate()
    };
    s.add("J8-零分母不判", j8, "空面豁免率不可判且闸判为假");

    // J9 注入检出：不平 + 重号
    let inj = inject_four();
    s.add("J9-不平重号检出", inj[0] && inj[1], "三态不平与批号重号两路必抓");

    // J10 注入检出：超闸 + 缺批
    s.add("J10-超闸缺批检出", inj[2] && inj[3], "豁免超闸与缺批两路必抓");

    // J11 红批不判绿
    let j11 = {
        let mut f = LtpFace::new(GATE_PERMILLE);
        let mut no = BATCH_FIRST;
        while no <= BATCH_LAST {
            let t = if no == 32 { TriState::new(19, 1, 0) } else { TriState::new(20, 0, 0) };
            let _ = f.add(no, t);
            no += 1;
        }
        let t = f.sum();
        let _ = f.declare(t.total(), t.skip);
        !f.verdict() && f.reds() == 1
    };
    s.add("J11-红批不判绿", j11, "单批失败即终核为假");

    // J12 冻结拒写
    let root = face.seal();
    let j12 = face.add(30, TriState::new(20, 0, 0)).err() == Some(LT_FROZEN)
        && face.declare(1, 1).err() == Some(LT_FROZEN);
    s.add("J12-冻结拒写", j12, "封存后登记与申报两路必拒");

    // J13 冻结根稳定
    s.add(
        "J13-冻结根稳定",
        face.root == Some(root) && root == face.fold() && root != 0,
        "对照面冻结根可重算零漂移",
    );

    // J14 渲染面
    let j14 = face.rows[0].tri.render() != face.rows[3].tri.render()
        && face.rows[0].tri.render().contains("s0");
    s.add("J14-三态渲染", j14, "豁免批与零豁免批渲染可区分");

    // J15 动态明细
    s.add_dyn(
        "J15-对照明细",
        face.in_gate() && face.verdict(),
        &format!(
            "p={} f={} s={} rate={:?}",
            sum.pass,
            sum.fail,
            sum.skip,
            face.exempt_permille()
        ),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11103_ltp_flow() {
        let (mut f, built) = build_face();
        assert!(built);
        assert!(f.verdict());
        assert!(f.exempt_permille() == Some(8));
        assert!(f.in_gate());
        assert!(f.seal() != 0);
        assert!(f.add(30, TriState::new(20, 0, 0)).err() == Some(LT_FROZEN));
    }

    #[test]
    fn unx11103_checks_green() {
        let s = run_unx11103_checks();
        assert!(s.all_passed(), "UNX-F11103 判据未全绿");
    }
}
