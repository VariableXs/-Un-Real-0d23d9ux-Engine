//! UNX-F11109 豁免账总核（域豁免零悬空）（0x1f589 段）
//!
//! 锚点：UNX-F11109-J1 豁免账逐例终核（10/10），ADR 挂账零悬空（10/10）。
//! 域：CoRun·UNX-C4-B36（豁免账）。
//!
//! 结构：豁免账（WaiverBook：ADR 先登记 → 豁免逐例挂 ADR → 退役 → 悬空
//! 断言）+ 单例（Waiver：编号 + ADR 引用 + 范围 + 生效位）+ 冻结根 +
//! 注入核（未登记 ADR/非法 ADR/重号/撤账悬空四路必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1f589 独占 0x20）。
pub const SEG_BASE: u32 = 0x1f589;
pub const WV_ID: u32 = SEG_BASE + 0x01; // 零号豁免
pub const WV_DUP: u32 = SEG_BASE + 0x02; // 豁免重号
pub const WV_ADR: u32 = SEG_BASE + 0x03; // ADR 名非法或重号
pub const WV_SCOPE: u32 = SEG_BASE + 0x04; // 豁免范围为零
pub const WV_DANGLING: u32 = SEG_BASE + 0x05; // ADR 未挂账
pub const WV_RETIRED: u32 = SEG_BASE + 0x06; // 重复退役
pub const WV_FROZEN: u32 = SEG_BASE + 0x07; // 冻结后写账
pub const WV_OVER: u32 = SEG_BASE + 0x08; // 超定容

/// 域豁免例数（8）。
pub const NEEDED: usize = 8;
/// 豁免账定容。
pub const CAP: usize = 12;
/// ADR 前缀。
pub const ADR_PREFIX: &str = "ADR-";
/// 豁免账标（fold 种子）。
pub const WV_TAG: u64 = 0x1f5_8989;
/// 指纹步进。
pub const FP_STEP: u64 = 0x100000001b3;

/// 单例豁免：编号 + ADR 引用 + 范围 + 生效位。
#[derive(Clone, Debug, PartialEq)]
pub struct Waiver {
    pub id: u32,
    pub adr: String,
    pub scope: u32,
    pub active: bool,
}

impl Waiver {
    pub fn render(&self) -> String {
        format!("W{}|{}|s{}|{}", self.id, self.adr, self.scope, self.active as u8)
    }
}

/// 豁免账：ADR 名册 + 豁免例册 + 冻结根。
#[derive(Clone, Debug)]
pub struct WaiverBook {
    pub adrs: Vec<String>,
    pub items: Vec<Waiver>,
    pub frozen: bool,
    pub root: Option<u64>,
}

/// ADR 名合法性：前缀 ADR- 且长度不短于 6。
pub fn adr_ok(adr: &str) -> bool {
    adr.starts_with(ADR_PREFIX) && adr.len() >= 6
}

impl WaiverBook {
    pub fn new() -> WaiverBook {
        WaiverBook { adrs: Vec::new(), items: Vec::new(), frozen: false, root: None }
    }

    /// ADR 登记：非法名与重名皆拒。
    pub fn register_adr(&mut self, adr: &str) -> Result<(), u32> {
        if self.frozen {
            return Err(WV_FROZEN);
        }
        if !adr_ok(adr) || self.adrs.iter().any(|a| a == adr) {
            return Err(WV_ADR);
        }
        self.adrs.push(adr.to_string());
        Ok(())
    }

    /// ADR 撤账：制造悬空面以验证断言（正样本不走此路）。
    pub fn revoke_adr(&mut self, adr: &str) -> Result<(), u32> {
        if self.frozen {
            return Err(WV_FROZEN);
        }
        let mut k = 0usize;
        while k < self.adrs.len() {
            if self.adrs[k] == adr {
                self.adrs.remove(k);
                return Ok(());
            }
            k += 1;
        }
        Err(WV_ADR)
    }

    /// 豁免挂账：零号/重号/零范围/ADR 未挂账/超容皆拒。
    pub fn file(&mut self, id: u32, adr: &str, scope: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(WV_FROZEN);
        }
        if id == 0 {
            return Err(WV_ID);
        }
        if self.items.iter().any(|w| w.id == id) {
            return Err(WV_DUP);
        }
        if scope == 0 {
            return Err(WV_SCOPE);
        }
        if !self.adrs.iter().any(|a| a == adr) {
            return Err(WV_DANGLING);
        }
        if self.items.len() >= CAP {
            return Err(WV_OVER);
        }
        self.items.push(Waiver { id, adr: adr.to_string(), scope, active: true });
        Ok(())
    }

    /// 退役：查无此例拒、重复退役拒。
    pub fn retire(&mut self, id: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(WV_FROZEN);
        }
        let mut k = 0usize;
        while k < self.items.len() {
            if self.items[k].id == id {
                if !self.items[k].active {
                    return Err(WV_RETIRED);
                }
                self.items[k].active = false;
                return Ok(());
            }
            k += 1;
        }
        Err(WV_ID)
    }

    /// 悬空枚举：引用已不在 ADR 名册的豁免例。
    pub fn dangling(&self) -> Vec<u32> {
        let mut v: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.items.len() {
            if !self.adrs.iter().any(|a| a == &self.items[k].adr) {
                v.push(self.items[k].id);
            }
            k += 1;
        }
        v
    }

    /// 生效例数。
    pub fn active_count(&self) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.items.len() {
            if self.items[k].active {
                n += 1;
            }
            k += 1;
        }
        n
    }

    /// 终核：八例在册、ADR 名册足额、零悬空。
    pub fn closed(&self) -> bool {
        self.items.len() == NEEDED && self.adrs.len() >= NEEDED && self.dangling().is_empty()
    }

    /// 冻结并落根。
    pub fn seal(&mut self) -> u64 {
        self.frozen = true;
        let r = self.fold();
        self.root = Some(r);
        r
    }

    /// 账级指纹：ADR 名册 + 豁免例链式入链。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = WV_TAG;
        let mut a = 0usize;
        while a < self.adrs.len() {
            let bytes = self.adrs[a].as_bytes();
            let mut b = 0usize;
            while b < bytes.len() {
                h = (h ^ bytes[b] as u64).wrapping_mul(FP_STEP);
                b += 1;
            }
            h = (h ^ 0xa5).wrapping_mul(FP_STEP);
            a += 1;
        }
        let mut k = 0usize;
        while k < self.items.len() {
            let w = &self.items[k];
            h = (h ^ w.id as u64).wrapping_mul(FP_STEP);
            h = (h ^ w.scope as u64).wrapping_mul(FP_STEP);
            h = (h ^ w.active as u64).wrapping_mul(FP_STEP);
            let bytes = w.adr.as_bytes();
            let mut b = 0usize;
            while b < bytes.len() {
                h = (h ^ bytes[b] as u64).wrapping_mul(FP_STEP);
                b += 1;
            }
            k += 1;
        }
        h
    }
}

/// ADR 名样本。
pub fn adr_name(seq: u32) -> String {
    format!("ADR-C4-{:03}", seq)
}

/// 标准账：8 条 ADR 登记 + 8 例豁免挂账。
pub fn build_book() -> (WaiverBook, bool) {
    let mut b = WaiverBook::new();
    let mut ok = true;
    let mut seq = 1u32;
    while seq <= NEEDED as u32 {
        if b.register_adr(&adr_name(seq)).is_err() {
            ok = false;
        }
        if b.file(seq, &adr_name(seq), 100u32.wrapping_add(seq)).is_err() {
            ok = false;
        }
        seq += 1;
    }
    (b, ok)
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 未登记 ADR 挂账必拒
    let mut a = WaiverBook::new();
    let _ = a.register_adr(&adr_name(1));
    got[0] = a.file(1, &adr_name(2), 9).err() == Some(WV_DANGLING);
    // 2) 非法 ADR 名必拒
    let mut b = WaiverBook::new();
    got[1] = b.register_adr("ADR").err() == Some(WV_ADR)
        && b.register_adr("X-123").err() == Some(WV_ADR);
    // 3) 豁免重号必拒
    let mut c = WaiverBook::new();
    let _ = c.register_adr(&adr_name(1));
    let _ = c.file(1, &adr_name(1), 3);
    got[2] = c.file(1, &adr_name(1), 4).err() == Some(WV_DUP);
    // 4) 撤 ADR 后悬空必检出
    let (mut d, _) = build_book();
    let _ = d.revoke_adr(&adr_name(4));
    got[3] = d.dangling().len() == 1 && d.dangling().contains(&4) && !d.closed();
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx11109_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11109");

    let (mut book, built) = build_book();
    // J1 豁免账逐例终核
    s.add(
        "J1-豁免逐例终核",
        built && book.items.len() == NEEDED && book.closed(),
        "8 例豁免逐例挂 ADR 且终核闭合",
    );

    // J2 ADR 挂账零悬空
    s.add(
        "J2-ADR 零悬空",
        book.dangling().is_empty() && book.adrs.len() == NEEDED,
        "ADR 名册足额且无悬空引用",
    );

    // J3 未登记 ADR 挂账拒
    let j3 = {
        let mut c = WaiverBook::new();
        let _ = c.register_adr(&adr_name(1));
        c.file(1, &adr_name(9), 5).err() == Some(WV_DANGLING)
    };
    s.add("J3-未挂 ADR 拒", j3, "未登记 ADR 的豁免不得入账");

    // J4 非法 ADR 名拒
    let j4 = {
        let mut c = WaiverBook::new();
        c.register_adr("ADR-").err() == Some(WV_ADR)
            && c.register_adr("adr-1").err() == Some(WV_ADR)
    };
    s.add("J4-非法 ADR 拒", j4, "过短或前缀不符的 ADR 名必拒");

    // J5 重号豁免拒
    let j5 = {
        let mut c = WaiverBook::new();
        let _ = c.register_adr(&adr_name(1));
        let _ = c.file(1, &adr_name(1), 1);
        c.file(1, &adr_name(1), 2).err() == Some(WV_DUP)
    };
    s.add("J5-重号豁免拒", j5, "同号二次挂账必拒");

    // J6 零范围与零号拒
    let j6 = {
        let mut c = WaiverBook::new();
        let _ = c.register_adr(&adr_name(1));
        let a = c.file(1, &adr_name(1), 0).err() == Some(WV_SCOPE);
        let b = c.file(0, &adr_name(1), 1).err() == Some(WV_ID);
        a && b && c.items.is_empty()
    };
    s.add("J6-零范围零号拒", j6, "零范围与零号豁免两路必拒");

    // J7 重复退役拒
    let j7 = {
        let mut c = WaiverBook::new();
        let _ = c.register_adr(&adr_name(1));
        let _ = c.file(1, &adr_name(1), 1);
        let _ = c.retire(1);
        c.retire(1).err() == Some(WV_RETIRED) && c.active_count() == 0
    };
    s.add("J7-重复退役拒", j7, "同例二次退役必拒且生效归零");

    // J8 撤 ADR 悬空检出
    let j8 = {
        let (mut c, _) = build_book();
        let _ = c.revoke_adr(&adr_name(6));
        c.dangling().len() == 1 && c.dangling().contains(&6) && !c.closed()
    };
    s.add("J8-撤账悬空检出", j8, "撤一条 ADR 即悬空点名且终核为假");

    // J9 注入检出：未挂 ADR + 非法 ADR
    let inj = inject_four();
    s.add("J9-未挂非法检出", inj[0] && inj[1], "未登记 ADR 与非法名两路必抓");

    // J10 注入检出：重号 + 撤账悬空
    s.add("J10-重号悬空检出", inj[2] && inj[3], "豁免重号与撤账悬空两路必抓");

    // J11 冻结拒写
    let root = book.seal();
    let j11 = book.register_adr("ADR-C4-999").err() == Some(WV_FROZEN)
        && book.file(99, &adr_name(1), 1).err() == Some(WV_FROZEN)
        && book.retire(1).err() == Some(WV_FROZEN)
        && book.revoke_adr(&adr_name(1)).err() == Some(WV_FROZEN);
    s.add("J11-冻结拒写", j11, "封存后 ADR/挂账/退役/撤账四路必拒");

    // J12 冻结根稳定
    s.add(
        "J12-冻结根稳定",
        book.root == Some(root) && root == book.fold() && root != 0,
        "豁免账根可重算零漂移",
    );

    // J13 动态明细
    s.add_dyn(
        "J13-豁免明细",
        book.closed() && book.fold() == root,
        &format!(
            "items={} adrs={} active={} root={:#x}",
            book.items.len(),
            book.adrs.len(),
            book.active_count(),
            root
        ),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11109_waiver_flow() {
        let (mut b, built) = build_book();
        assert!(built);
        assert!(b.closed());
        assert!(b.dangling().is_empty());
        assert!(b.active_count() == NEEDED);
        assert!(b.seal() != 0);
        assert!(b.retire(1).err() == Some(WV_FROZEN));
        assert!(adr_ok("ADR-C4-001"));
        assert!(!adr_ok("ADR-"));
    }

    #[test]
    fn unx11109_checks_green() {
        let s = run_unx11109_checks();
        assert!(s.all_passed(), "UNX-F11109 判据未全绿");
    }
}
