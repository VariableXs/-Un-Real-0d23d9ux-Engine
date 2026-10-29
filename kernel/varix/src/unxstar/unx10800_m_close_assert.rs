//! UNX-F10800 M 型总收口断言（B20 聚合+段账对平）（0x1bc80 段）
//!
//! 锚点：UNX-F10800-J1 M 型总收口对平——前段 B20 五锚（步距 20 升序）
//! 全数登记翻绿、段账三轴（登记数/绿数/步距+段基址单调）对平、封账后
//! 写账必拒、五类注入坏账必抓。
//! 域：CoRun·UNX-C3（错误矩阵面谱系）。
//!
//! 结构：锚点登记册（升序+步距+段基址单调+翻绿位独立可翻）+ 对平核
//! （登记数锚五、绿数对平、指纹折叠封存）+ 注入核（重号/跳步/带红封账/
//! 封账后写账/计数欠额五类必抓）+ 正反双样本（正样本五锚放行封账，
//! 反样本各坏形必返回对应诊断码）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1bc80 独占 0x20）。
pub const SEG_BASE: u32 = 0x1bc80;
pub const MC_DUP_ID: u32 = SEG_BASE + 0x01; // 锚点重号
pub const MC_STRIDE: u32 = SEG_BASE + 0x02; // 步距/段基址单调断裂
pub const MC_RED_SEAL: u32 = SEG_BASE + 0x03; // 带红封账（绿数不齐）
pub const MC_SEALED: u32 = SEG_BASE + 0x04; // 封账后写账
pub const MC_COUNT: u32 = SEG_BASE + 0x05; // 段账计数欠额/超容
pub const MC_EMPTY: u32 = SEG_BASE + 0x06; // 空锚名
pub const MC_MISS: u32 = SEG_BASE + 0x07; // 翻绿指向未登记锚号

/// 前段 B20 首锚号（M 型总收口的聚合对象）。
pub const ANCHOR_FIRST: u32 = 10701;
/// 锚点步距（批内锚点号面按 20 递进）。
pub const ANCHOR_STRIDE: u32 = 20;
/// 应登记锚数（B20 五锚）。
pub const ANCHOR_COUNT: usize = 5;
/// 登记册容量（先定后用：五锚 + 余量 2）。
pub const LEDGER_CAP: usize = 7;

/// 单个锚点条目。
#[derive(Clone)]
pub struct Anchor {
    pub id: u32,
    pub seg: u32,
    pub label: String,
    pub green: bool,
}

/// 锚点登记册：升序登记、翻绿位可翻、封账冻结。
pub struct AnchorLedger {
    pub entries: Vec<Anchor>,
    pub sealed: bool,
    pub note: String,
}

/// 锚点段基址抽样（随号递增的合成段值，单调核用）。
pub fn seg_of(k: u32) -> u32 {
    SEG_BASE - 0x40 * (ANCHOR_COUNT as u32 - k)
}

/// 锚号步距核：第 k 个锚号必须恰为 ANCHOR_FIRST + STRIDE*k。
pub fn stride_ok(id: u32, k: u32) -> bool {
    id == ANCHOR_FIRST + ANCHOR_STRIDE * k
}

/// 全账逐行渲染（对平审计明细）。
pub fn audit_rows(led: &AnchorLedger) -> Vec<String> {
    let mut rows: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < led.entries.len() {
        rows.push(render_anchor(&led.entries[i]));
        i += 1;
    }
    rows
}

/// 账内错位枚举：步距断裂与带红锚号的 id 列表（空表=全对平）。
pub fn mismatches(led: &AnchorLedger) -> Vec<u32> {
    let mut bad: Vec<u32> = Vec::new();
    let mut i = 0usize;
    while i < led.entries.len() {
        let a = &led.entries[i];
        if !stride_ok(a.id, i as u32) || !a.green {
            bad.push(a.id);
        }
        i += 1;
    }
    bad
}

impl AnchorLedger {
    pub fn new(note: &str) -> AnchorLedger {
        AnchorLedger { entries: Vec::new(), sealed: false, note: note.to_string() }
    }

    /// 登记锚点：封账/空名/重号/步距断裂/段基址倒退/超容均拒。
    pub fn enroll(&mut self, id: u32, seg: u32, label: &str, green: bool) -> Result<(), u32> {
        if self.sealed {
            return Err(MC_SEALED);
        }
        if label.is_empty() {
            return Err(MC_EMPTY);
        }
        if self.entries.iter().any(|a| a.id == id) {
            return Err(MC_DUP_ID);
        }
        match self.entries.last() {
            Some(last) => {
                if id != last.id + ANCHOR_STRIDE || seg <= last.seg {
                    return Err(MC_STRIDE);
                }
            }
            None => {
                if id != ANCHOR_FIRST {
                    return Err(MC_STRIDE);
                }
            }
        }
        if self.entries.len() >= LEDGER_CAP {
            return Err(MC_COUNT);
        }
        self.entries.push(Anchor { id, seg, label: label.to_string(), green });
        Ok(())
    }

    /// 翻绿位翻转：封账后必拒；指向未登记锚号必拒。
    pub fn set_green(&mut self, id: u32, green: bool) -> Result<(), u32> {
        if self.sealed {
            return Err(MC_SEALED);
        }
        let mut hit = false;
        for a in &mut self.entries {
            if a.id == id {
                a.green = green;
                hit = true;
            }
        }
        if hit {
            Ok(())
        } else {
            Err(MC_MISS)
        }
    }

    /// 段账对平：登记数恰达锚数且绿数对平。
    pub fn reconcile(&self) -> Result<(usize, usize), u32> {
        let greens = self.entries.iter().filter(|a| a.green).count();
        if self.entries.len() != ANCHOR_COUNT {
            return Err(MC_COUNT);
        }
        if greens != ANCHOR_COUNT {
            return Err(MC_RED_SEAL);
        }
        Ok((self.entries.len(), greens))
    }

    /// 封账：带红必拒，成功返回折叠指纹并冻结。
    pub fn seal(&mut self) -> Result<u32, u32> {
        if self.sealed {
            return Err(MC_SEALED);
        }
        if self.entries.iter().any(|a| !a.green) {
            return Err(MC_RED_SEAL);
        }
        self.sealed = true;
        Ok(self.fold())
    }

    /// 折叠指纹（序敏感，封账前后稳定）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        h = h.wrapping_mul(31).wrapping_add(self.entries.len() as u32);
        for a in &self.entries {
            h = h.wrapping_mul(31).wrapping_add(a.id);
            h = h.wrapping_mul(31).wrapping_add(a.seg);
            h = h.wrapping_mul(31).wrapping_add(a.green as u32);
            for b in a.label.bytes() {
                h = h.wrapping_mul(31).wrapping_add(b as u32);
            }
        }
        h
    }
}

/// 权威正账：B20 五锚（步距 20、段基址递增）全绿登记。
pub fn canonical() -> AnchorLedger {
    let mut led = AnchorLedger::new("M 型总收口");
    let mut k = 0u32;
    while k < ANCHOR_COUNT as u32 {
        let id = ANCHOR_FIRST + ANCHOR_STRIDE * k;
        let _ = led.enroll(id, seg_of(k), "B20 锚", true);
        k += 1;
    }
    led
}

/// 注入核：五类坏账必被抓（重号/跳步/带红封账/封账后写账/计数欠额）。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 重号必拒
    let mut l1 = AnchorLedger::new("inj-dup");
    let _ = l1.enroll(ANCHOR_FIRST, seg_of(0), "甲", true);
    got[0] = l1.enroll(ANCHOR_FIRST, seg_of(1), "乙", true).err() == Some(MC_DUP_ID);
    // 2) 跳步必拒（步距 20 断裂）
    let mut l2 = AnchorLedger::new("inj-stride");
    let _ = l2.enroll(ANCHOR_FIRST, seg_of(0), "甲", true);
    got[1] = l2
        .enroll(ANCHOR_FIRST + ANCHOR_STRIDE + 1, seg_of(1), "乙", true)
        .err() == Some(MC_STRIDE);
    // 3) 带红封账必拒
    let mut l3 = AnchorLedger::new("inj-red");
    let _ = l3.enroll(ANCHOR_FIRST, seg_of(0), "甲", false);
    got[2] = l3.seal().err() == Some(MC_RED_SEAL);
    // 4) 封账后写账必抓
    let mut l4 = canonical();
    let _ = l4.seal();
    got[3] = l4.enroll(10801, SEG_BASE, "迟", true).err() == Some(MC_SEALED);
    // 5) 计数欠额：只登四锚 → 对平必不平
    let mut l5 = AnchorLedger::new("inj-short");
    let mut k = 0u32;
    while k < (ANCHOR_COUNT as u32) - 1 {
        let id = ANCHOR_FIRST + ANCHOR_STRIDE * k;
        let _ = l5.enroll(id, seg_of(k), "锚", true);
        k += 1;
    }
    got[4] = l5.reconcile().err() == Some(MC_COUNT);
    got
}

/// 单锚渲染行（对平明细用）。
pub fn render_anchor(a: &Anchor) -> String {
    let mark = if a.green { "G" } else { "R" };
    format!("A{}|{:#x}|{}|{}", a.id, a.seg, mark, a.label)
}

/// M 型总收口判据（对应卡验收 J 判据）。
pub fn run_unx10800_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10800");
    let inj = inject_five();

    // J1 正路径：五锚全绿登记且段账对平（错位枚举为空）
    let mut led = canonical();
    let rec = led.reconcile();
    let j1 = rec.is_ok() && mismatches(&led).is_empty();
    s.add("J1-五锚对平", j1, "登记数=5 绿数=5 且零错位");

    // J2 步距与段基址双单调：ids 恰为 10701+20k，seg 递增
    let j2 = {
        let mut ok = led.entries.len() == ANCHOR_COUNT;
        let mut i = 0usize;
        while i < led.entries.len() {
            if !stride_ok(led.entries[i].id, i as u32) {
                ok = false;
            }
            if i > 0 && led.entries[i].seg <= led.entries[i - 1].seg {
                ok = false;
            }
            i += 1;
        }
        ok
    };
    s.add("J2-步距段基对平", j2, "锚号步距 20 且段基址单调");

    // J3 封账指纹稳定 + 二次封账必拒
    let fp = led.fold();
    let seal1 = led.seal();
    let j3 = seal1.is_ok() && led.fold() == fp && led.seal().err() == Some(MC_SEALED);
    s.add("J3-封账指纹稳定", j3, "封账前后指纹一致且二次封账拒");

    // J4 封账后写账必拒（正账复现）
    let j4 = led.enroll(10801, SEG_BASE, "迟", true).err() == Some(MC_SEALED);
    s.add("J4-封账拒写", j4, "封账后登记必拒");

    // J5 注入1 重号必抓
    s.add("J5-重号注入检出", inj[0], "重号登记必拒");

    // J6 注入2 跳步必抓
    s.add("J6-跳步注入检出", inj[1], "步距断裂必拒");

    // J7 注入3 带红封账必抓
    s.add("J7-带红封账检出", inj[2], "绿数不齐封账必拒");

    // J8 注入4 封账后写账必抓
    s.add("J8-封账后写检出", inj[3], "封账后写账必抓");

    // J9 注入5 计数欠额必抓
    s.add("J9-欠额注入检出", inj[4], "少登一锚对平必不平");

    // J10 空名拒收
    let j10 = {
        let mut l = AnchorLedger::new("empty");
        l.enroll(ANCHOR_FIRST, seg_of(0), "", true).err() == Some(MC_EMPTY)
    };
    s.add("J10-空名拒收", j10, "空锚名必拒");

    // J11 翻绿缺号必拒（未登记锚号）
    let j11 = {
        let mut l = canonical();
        l.set_green(99999, true).err() == Some(MC_MISS)
    };
    s.add("J11-翻绿缺号", j11, "未登记锚号翻绿必拒");

    // J12 容量边界：填满 LEDGER_CAP 后第 CAP+1 条必拒且原账不破
    let j12 = {
        let mut l = AnchorLedger::new("cap");
        let mut filled = true;
        let mut k = 0u32;
        while k < LEDGER_CAP as u32 {
            let sg = SEG_BASE - 0x40 * (LEDGER_CAP as u32 - k);
            if l.enroll(ANCHOR_FIRST + ANCHOR_STRIDE * k, sg, "x", true).is_err() {
                filled = false;
            }
            k += 1;
        }
        let over_id = ANCHOR_FIRST + ANCHOR_STRIDE * LEDGER_CAP as u32;
        filled && l.enroll(over_id, SEG_BASE, "over", true).err() == Some(MC_COUNT)
    };
    s.add("J12-容量边界", j12, "超容登记必拒");

    // J13 (dyn) 指纹落账：权威账指纹非零且稳定
    let fp2 = canonical().fold();
    s.add_dyn("J13-指纹落账", fp2 != 0 && fp2 == canonical().fold(), &format!("fold={:#x}", fp2));

    // J14 (dyn) 对平明细：五锚逐行渲染计数
    let led2 = canonical();
    let (n, g) = led2.reconcile().ok().unwrap_or((0, 0));
    let rows = audit_rows(&led2);
    let first = if rows.is_empty() { String::from("-") } else { rows[0].clone() };
    s.add_dyn(
        "J14-对平明细",
        n == ANCHOR_COUNT && g == ANCHOR_COUNT && rows.len() == ANCHOR_COUNT,
        &format!("n={} g={} first={}", n, g, first),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10800_ledger_flow() {
        let mut l = AnchorLedger::new("t");
        assert!(l.enroll(ANCHOR_FIRST, seg_of(0), "锚一", true).is_ok());
        assert!(l.enroll(ANCHOR_FIRST, seg_of(1), "重号", true).err() == Some(MC_DUP_ID));
        assert!(
            l.enroll(ANCHOR_FIRST + ANCHOR_STRIDE + 1, seg_of(1), "跳步", true).err()
                == Some(MC_STRIDE)
        );
        let _ = l.enroll(ANCHOR_FIRST + ANCHOR_STRIDE, seg_of(1), "锚二", false);
        assert!(l.reconcile().err() == Some(MC_COUNT));
        assert!(l.seal().err() == Some(MC_RED_SEAL));
        assert!(l.set_green(ANCHOR_FIRST + ANCHOR_STRIDE, true).is_ok());
        assert!(l.set_green(99999, true).err() == Some(MC_MISS));
        assert!(l.seal().is_ok());
        assert!(l.enroll(9999, SEG_BASE, "迟", true).err() == Some(MC_SEALED));
    }

    #[test]
    fn unx10800_checks_green() {
        let s = run_unx10800_checks();
        assert!(s.all_passed(), "UNX-F10800 判据未全绿");
    }
}
