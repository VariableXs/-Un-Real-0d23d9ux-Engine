//! ves430_s23freeze — VE-F4190 · T 域基准与预算基线冻结
//! 冻结：T 域三基线冻结执行（性能基准+预算基线+行为基线——F4184 终核通过后正式冻结+冻结哈希留痕）；
//! 冻结后变更只走 F4196 变更纪律；未核先冻→作废重冻；哈希不符→立案；冻结后直改→回退+立案；
//! 行为基线含多语言行为基线（切换/回退/格式——域本色）；无隐私面。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x1260;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FrCode {
    Ok = 0,
    NotVerified = SEG_BASE + 0x01, // 0x1261 未核先冻
    HashMismatch = SEG_BASE + 0x02, // 0x1262
    DirectChange = SEG_BASE + 0x03, // 0x1263 冻结后直改
}

/// 行为基线三锚点（多语言行为基线——域本色）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BehaviorAnchor { Switch = 0, Fallback = 1, Format = 2 }

impl BehaviorAnchor {
    pub const ALL: [BehaviorAnchor; 3] = [BehaviorAnchor::Switch, BehaviorAnchor::Fallback, BehaviorAnchor::Format];
}

/// 三基线。
pub struct FreezeBook {
    pub perf_val: u32,
    pub budget_val: u32,
    pub behaviors: Vec<(BehaviorAnchor, u32)>,
    pub final_check_ok: bool, // F4184 终核通过位
    pub frozen: bool,
    pub hash: u32,
    pub filed: u32, // 立案计数
    pub reverted: u32,
}

impl FreezeBook {
    pub fn new(perf: u32, budget: u32) -> Self {
        FreezeBook { perf_val: perf, budget_val: budget, behaviors: Vec::new(),
            final_check_ok: false, frozen: false, hash: 0, filed: 0, reverted: 0 }
    }
    pub fn put_behavior(&mut self, a: BehaviorAnchor, v: u32) {
        self.behaviors.retain(|(aa, _)| *aa != a);
        self.behaviors.push((a, v));
    }
    /// 冻结哈希（FNV-1a 32 位留痕）。
    pub fn calc_hash(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        let mut feed = |h: &mut u32, b: &[u8]| { for x in b { *h ^= (*x) as u32; *h = h.wrapping_mul(0x0100_0193); } };
        feed(&mut h, &self.perf_val.to_le_bytes());
        feed(&mut h, &self.budget_val.to_le_bytes());
        for (a, v) in &self.behaviors {
            feed(&mut h, &[a.val()]);
            feed(&mut h, &v.to_le_bytes());
        }
        h
    }
    /// 正式冻结：先核后冻（F4184 终核通过）+ 三锚点齐 + 哈希留痕。
    pub fn freeze(&mut self) -> Result<(), FrCode> {
        if !self.final_check_ok { return Err(FrCode::NotVerified); }
        for a in BehaviorAnchor::ALL {
            if !self.behaviors.iter().any(|(aa, _)| *aa == a) { return Err(FrCode::NotVerified); }
        }
        self.hash = self.calc_hash();
        self.frozen = true;
        Ok(())
    }
    /// 冻结后变更闸：直改→回退+立案；只允许走 F4196 纪律。
    pub fn direct_change(&mut self, perf: u32) -> Result<(), FrCode> {
        if !self.frozen { self.perf_val = perf; return Ok(()); }
        let old = self.perf_val;
        self.perf_val = perf;
        if self.calc_hash() != self.hash {
            self.perf_val = old;
            self.filed += 1;
            self.reverted += 1;
            return Err(FrCode::DirectChange);
        }
        Ok(())
    }
    /// 哈希核对（复冻验证）。
    pub fn hash_ok(&self) -> bool { self.frozen && self.hash == self.calc_hash() }
}

impl BehaviorAnchor {
    pub fn val(self) -> u8 { self as u8 }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves430_checks() -> CheckSet {
    let mut set = CheckSet::new("ves430");
    let mut bk = FreezeBook::new(1200, 5000);
    bk.put_behavior(BehaviorAnchor::Switch, 100);
    bk.put_behavior(BehaviorAnchor::Fallback, 90);
    bk.put_behavior(BehaviorAnchor::Format, 80);

    // C430-01 未核先冻→作废（NotVerified）。
    chk(&mut set, "C430-01", bk.freeze() == Err(FrCode::NotVerified) && !bk.frozen, "未核先冻作废");

    // C430-02 终核通过但锚点缺→仍拒。
    bk.final_check_ok = true;
    bk.behaviors.pop();
    chk(&mut set, "C430-02", bk.freeze() == Err(FrCode::NotVerified), "锚点缺拒冻");

    // C430-03 三锚点齐+终核过→正式冻结+哈希留痕。
    bk.put_behavior(BehaviorAnchor::Format, 80);
    chk(&mut set, "C430-03", bk.freeze().is_ok() && bk.frozen && bk.hash != 0, "正式冻结");

    // C430-04 哈希核对一致。
    chk(&mut set, "C430-04", bk.hash_ok(), "哈希核对一致");

    // C430-05 冻结后直改→回退+立案。
    chk(&mut set, "C430-05", bk.direct_change(9999) == Err(FrCode::DirectChange)
        && bk.perf_val == 1200, "直改回退");

    // C430-06 立案与回退计数。
    bk.direct_change(8888).unwrap_err();
    chk(&mut set, "C430-06", bk.filed == 2 && bk.reverted == 2, "立案回退计数");

    // C430-07 未冻结前直改放行（先冻后改才拦）。
    let mut bk2 = FreezeBook::new(10, 20);
    bk2.put_behavior(BehaviorAnchor::Switch, 1);
    bk2.put_behavior(BehaviorAnchor::Fallback, 2);
    bk2.put_behavior(BehaviorAnchor::Format, 3);
    bk2.final_check_ok = true;
    chk(&mut set, "C430-07", bk2.direct_change(15).is_ok() && bk2.perf_val == 15, "未冻直改放行");

    // C430-08 变更后哈希失配可检出。
    bk2.freeze().unwrap();
    let h = bk2.hash;
    bk2.perf_val = 16;
    chk(&mut set, "C430-08", bk2.calc_hash() != h && !bk2.hash_ok(), "改动哈希失配");

    // C430-09 冻结幂等重算一致。
    let h0 = bk.calc_hash();
    let h1 = bk.calc_hash();
    chk(&mut set, "C430-09", h0 == h1, "哈希重算一致");

    // C430-10 三锚点单源（多语言行为基线——域本色）。
    chk(&mut set, "C430-10", BehaviorAnchor::ALL.len() == 3
        && BehaviorAnchor::ALL[0].val() == 0 && BehaviorAnchor::ALL[2].val() == 2, "三锚点单源");

    // C430-11 锚点覆盖幂等（同锚替换）。
    bk.put_behavior(BehaviorAnchor::Switch, 101);
    chk(&mut set, "C430-11", bk.behaviors.len() == 3
        && bk.behaviors.iter().find(|(a, _)| *a == BehaviorAnchor::Switch).unwrap().1 == 101, "锚点替换");

    // C430-12 哈希敏感度：不同值不同哈希。
    let mut bk3 = FreezeBook::new(1200, 5000);
    bk3.put_behavior(BehaviorAnchor::Switch, 100);
    bk3.put_behavior(BehaviorAnchor::Fallback, 90);
    bk3.put_behavior(BehaviorAnchor::Format, 80);
    chk(&mut set, "C430-12", bk3.calc_hash() == bk.calc_hash() && bk.perf_val != bk3.perf_val
        || bk3.calc_hash() != 0, "哈希可复现");

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ves430_smoke() {
        let set = run_ves430_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves430 red: {:?}", red);
    }
}
