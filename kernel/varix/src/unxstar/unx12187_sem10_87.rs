//! UNX-F12187 — SpinCount 语义与初始化参数（0x1a980 段）
//!
//! 锚点：UNX-F12187-J1 自旋参数归一 10/10（0=不自旋、负值=4 倍
//! 处理器数、正值封顶上限），自旋账逐档留痕全档回放一致。
//! 域：CoRun·UNX-D1-B10（临界区语义谱系）。
//! 结构：SpinPolicy（参数归一核）+ SpinLed（自旋账）+ 折叠指纹 +
//! 注入核。
//!
//! NT 语义：RtlInitializeCriticalSectionAndSpinCount 的 SpinCount
//! 参数三档——0 表示进锁不自旋；负值（高位语义）按 4 倍处理器数
//! 折算；正值直接生效但封顶 MAX。自旋发生在争用路径前段：空转
//! SpinCount 次再落等待块，超上限的自旋是浪费 CPU，必须封顶。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1a980;
pub const SP_SEALED: u32 = SEG_BASE + 0x01; // 印后改参
pub const SP_CAP: u32 = SEG_BASE + 0x02; // 超上限封顶
pub const SP_NEG: u32 = SEG_BASE + 0x03; // 负值归一档
pub const SP_ZERO: u32 = SEG_BASE + 0x04; // 零自旋档
pub const SP_EMPTY: u32 = SEG_BASE + 0x05; // 空账回放

/// 自旋计数封顶上限（本模型语义常量）。
pub const MAX_SPIN: u32 = 0x1_0000;

/// SpinPolicy：SpinCount 参数归一核。
#[derive(Clone, Copy, PartialEq)]
pub struct SpinPolicy {
    pub cpus: u32,
}

impl SpinPolicy {
    pub fn new(cpus: u32) -> SpinPolicy {
        SpinPolicy { cpus: if cpus == 0 { 1 } else { cpus } }
    }

    /// 归一：0 → 零自旋档；负值 → 4×处理器数；正值 → min(值, MAX)。
    /// 返回 (归一值, 档位码)。
    pub fn normalize(&self, raw: i32) -> (u32, u32) {
        if raw == 0 {
            (0, SP_ZERO)
        } else if raw < 0 {
            // 负值档：4 倍处理器数，同样受封顶纪律约束。
            let v = 4u32.wrapping_mul(self.cpus);
            (v.min(MAX_SPIN), SP_NEG)
        } else if raw as u32 > MAX_SPIN {
            (MAX_SPIN, SP_CAP)
        } else {
            (raw as u32, 0)
        }
    }
}

/// 自旋账：逐档留痕。
#[derive(Clone, PartialEq)]
pub struct SpinRec {
    pub raw: i32,
    pub effective: u32,
    pub tag: String,
}

impl SpinRec {
    /// 记录自洽：tag 非空、effective 与 raw 归一关系成立。
    pub fn ok(&self, p: &SpinPolicy) -> bool {
        let (want, _) = p.normalize(self.raw);
        !self.tag.is_empty() && want == self.effective
    }

    pub fn render(&self) -> String {
        format!("raw={} eff={}", self.raw, self.effective)
    }
}

/// 自旋账容器。
pub struct SpinLed {
    pub policy: SpinPolicy,
    pub recs: Vec<SpinRec>,
    sealed: bool,
}

impl SpinLed {
    pub fn new(cpus: u32) -> SpinLed {
        SpinLed { policy: SpinPolicy::new(cpus), recs: Vec::new(), sealed: false }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 记一档参数。
    pub fn record(&mut self, raw: i32, tag: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(SP_SEALED);
        }
        if tag.is_empty() {
            return Err(SP_EMPTY);
        }
        let (eff, code) = self.policy.normalize(raw);
        let seq = self.recs.len() as u32 + 1;
        self.recs.push(SpinRec { raw, effective: eff, tag: tag.to_string() });
        Ok(if code == 0 { seq } else { code })
    }

    /// 空转模拟：按生效自旋数逐次递减，返回实际空转步数。
    /// 零自旋档直接返回 0；步数受封顶保护不超 MAX。
    pub fn spin_burn(&self, raw: i32) -> u32 {
        let (eff, _) = self.policy.normalize(raw);
        let mut steps: u32 = 0;
        let mut left = eff;
        while left > 0 && steps < MAX_SPIN {
            left -= 1;
            steps += 1;
        }
        steps
    }

    /// 全账回放自洽。
    pub fn replay(&self) -> bool {
        !self.recs.is_empty() && self.recs.iter().all(|r| r.ok(&self.policy))
    }

    /// 零自旋档计数。
    pub fn zero_count(&self) -> usize {
        self.recs.iter().filter(|r| r.effective == 0).count()
    }

    /// 负值归一档计数。
    pub fn neg_count(&self) -> usize {
        self.recs.iter().filter(|r| r.raw < 0).count()
    }

    /// 折叠指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1a980;
        h = h.wrapping_mul(211).wrapping_add(self.policy.cpus);
        for r in &self.recs {
            h = h.wrapping_mul(211).wrapping_add(self.policy.normalize(r.raw).0);
            for by in r.tag.bytes() {
                h = h.wrapping_mul(211).wrapping_add(by as u32);
            }
        }
        h
    }

    /// 印账冻结。
    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 账面摘要。
    pub fn book_summary(&self) -> String {
        format!(
            "cpus={} recs={} zero={}",
            self.policy.cpus, self.recs.len(), self.zero_count()
        )
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = SpinLed::new(8);
    // 1) 负值必归一为 4 倍处理器数。
    b.record(-1, "neg");
    got[0] = b.recs[0].effective == 32;
    // 2) 超上限正值必封顶。
    let _ = b.record(MAX_SPIN as i32 + 7, "cap");
    got[1] = b.recs[1].effective == MAX_SPIN;
    // 3) 零参必落零自旋档。
    let _ = b.record(0, "zero");
    got[2] = b.recs[2].effective == 0;
    // 4) 空 tag 记录必拒。
    got[3] = b.record(16, "").err() == Some(SP_EMPTY);
    // 5) 印后改参必拒。
    b.seal();
    got[4] = b.record(4, "post").err() == Some(SP_SEALED);
    got
}

/// 判据入口：UNX-F12187
pub fn run_unx12187_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12187");
    let p = SpinPolicy::new(8);

    // J1 归一核：零参落零自旋档。
    s.add(
        "J1_zero_tier",
        p.normalize(0) == (0, SP_ZERO),
        "0=不自旋语义钉死",
    );

    // J2 归一核：负值 = 4 倍处理器数。
    s.add(
        "J2_neg_tier",
        p.normalize(-1) == (32, SP_NEG) && p.normalize(-99) == (32, SP_NEG),
        "负值按 4×8=32 折算",
    );

    // J3 归一核：正值直通。
    s.add(
        "J3_pos_pass",
        p.normalize(64) == (64, 0) && p.normalize(1) == (1, 0),
        "正值未超上限直通",
    );

    // J4 归一核：超上限封顶。
    s.add(
        "J4_cap",
        p.normalize(MAX_SPIN as i32 + 1) == (MAX_SPIN, SP_CAP),
        "超上限封顶 MAX",
    );

    // J5 零处理器数防御（按 1 计）。
    s.add(
        "J5_zero_cpus",
        SpinPolicy::new(0).normalize(-1).0 == 4,
        "cpus=0 按 1 归一：4×1=4",
    );

    // J6 空转模拟：零自旋档零步。
    let mut b = SpinLed::new(8);
    s.add(
        "J6_burn_zero",
        b.spin_burn(0) == 0,
        "零自旋档空转 0 步",
    );

    // J7 空转模拟：负值档按 32 步。
    s.add(
        "J7_burn_neg",
        b.spin_burn(-5) == 32,
        "负值档空转 32 步",
    );

    // J8 空转模拟：封顶档不超 MAX。
    s.add(
        "J8_burn_cap",
        b.spin_burn(MAX_SPIN as i32 * 2) == MAX_SPIN,
        "封顶档空转恰 MAX 步",
    );

    // J9 记账五档。
    let j9 = b.record(0, "z").is_ok()
        && b.record(-1, "n").is_ok()
        && b.record(16, "p").is_ok()
        && b.record(MAX_SPIN as i32 + 9, "c").is_ok()
        && b.record(4, "q").is_ok();
    s.add("J9_records", j9 && b.recs.len() == 5, "五档参数全记账");

    // J10 全账回放自洽。
    s.add("J10_replay", b.replay(), "自旋账逐档过记录核");

    // J11 档位计数。
    s.add(
        "J11_counts",
        b.zero_count() == 1 && b.neg_count() == 1,
        "零档 1 条、负值档 1 条",
    );

    // J12 记录渲染。
    let r = b.recs[0].render();
    s.add(
        "J12_render",
        r == "raw=0 eff=0",
        "记录渲染 raw/eff 对平",
    );

    // J13 指纹稳定。
    let f1 = b.fold();
    let f2 = b.fold();
    s.add("J13_fingerprint", f1 == f2, "自旋账折叠指纹一致");

    // J14 注入 5 项全抓。
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J14_inject_all", caught == 5, "5 注入点全检出");

    // J15 印账受控。
    b.seal();
    s.add(
        "J15_sealed",
        b.record(8, "post").err() == Some(SP_SEALED),
        "印账后改参拒",
    );

    // J16 印账不改指纹。
    let fb = b.fold();
    b.seal();
    s.add("J16_seal_fp", b.fold() == fb, "印账不改指纹");

    // J17 空 tag 记录拒。
    let mut b17 = SpinLed::new(4);
    s.add(
        "J17_empty_tag",
        b17.record(4, "").err() == Some(SP_EMPTY) && b17.recs.is_empty(),
        "空 tag 拒且不入账",
    );

    // J18 空账指纹稳定。
    let b18 = SpinLed::new(4);
    s.add("J18_fresh_fp", b18.fold() == b18.fold(), "空自旋账指纹稳定");

    // J19 空账回放判不自洽。
    s.add("J19_empty_replay", !b18.replay(), "空账回放判不自洽");

    // J20 账面摘要。
    let sm = b.book_summary();
    s.add(
        "J20_summary",
        sm.contains("cpus=8") && sm.contains("recs=5") && sm.contains("zero=1"),
        "摘要含处理器数/条数/零档",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12187_normalize() {
        let p = SpinPolicy::new(8);
        assert_eq!(p.normalize(0), (0, SP_ZERO));
        assert_eq!(p.normalize(-1), (32, SP_NEG));
        assert_eq!(p.normalize(16), (16, 0));
        assert_eq!(p.normalize(MAX_SPIN as i32 + 1), (MAX_SPIN, SP_CAP));
    }

    #[test]
    fn unx12187_checks_green() {
        let s = run_unx12187_checks();
        assert!(s.all_passed(), "UNX-F12187 判据未全绿");
    }

    #[test]
    fn unx12187_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
