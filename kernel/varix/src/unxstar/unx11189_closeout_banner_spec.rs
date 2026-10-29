//! UNX-F11189 域收官标志条规格核验（F11200 三断言预检）（0x1f6e0 段）
//!
//! 纯功能模型：域收官标志条由六条规格句组成，每句有「应为」与「实有」两侧。
//! 六句之上再压三条断言：满条（判据 800）、满账（行数 240,000）、已封（锚与
//! 状态）。三断言的「定义」在预检末尾冻结——冻结之后任何一条规格的应为值
//! 被改动即视为定义漂移，直接拒止；冻结点之前的缺句、重句、零值句同样拒止。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1f6e0;
/// 规格句号越界
pub const BC_ECLAUSE: u32 = SEG_BASE + 0x01;
/// 重复定义
pub const BC_EDUP: u32 = SEG_BASE + 0x02;
/// 应为值非法（零值）
pub const BC_ENEED: u32 = SEG_BASE + 0x03;
/// 缺句即冻结断言
pub const BC_ESHORT: u32 = SEG_BASE + 0x04;
/// 实有值未供
pub const BC_EMISS: u32 = SEG_BASE + 0x05;
/// 实有值与应为不符
pub const BC_EMISMATCH: u32 = SEG_BASE + 0x06;
/// 定义漂移
pub const BC_EDRIFT: u32 = SEG_BASE + 0x07;
/// 断言未冻结 / 已冻结后再改
pub const BC_EFROZEN: u32 = SEG_BASE + 0x08;

/// 规格句数。
pub const CLAUSES: usize = 6;
/// 句名：域判据 / 域行数 / 批数 / 卡数 / 锚 / 状态。
pub const CLAUSE_NAMES: [&str; CLAUSES] = ["criteria", "lines", "batches", "cards", "anchor", "state"];
/// 域判据满条。
pub const DOMAIN_CRITERIA: u64 = 800;
/// 域满账行数。
pub const DOMAIN_LINES: u64 = 240_000;
/// 域批数。
pub const DOMAIN_BATCHES: u64 = 40;
/// 域卡数。
pub const DOMAIN_CARDS: u64 = 800;
/// 状态「已完成」的编码。
pub const STATE_DONE: u64 = 1;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 一条规格句：应为 / 实有。
#[derive(Clone, Copy, Debug)]
pub struct Clause {
    pub id: u8,
    pub need: u64,
    pub have: Option<u64>,
}

/// 收官标志条。
#[derive(Clone, Debug)]
pub struct Banner {
    clauses: Vec<Clause>,
    defs_locked: Option<u64>,
    frozen: bool,
    pub accepts: u64,
    pub rejects: u64,
}

impl Banner {
    /// 空白标志条。
    pub fn new() -> Banner {
        Banner {
            clauses: Vec::new(),
            defs_locked: None,
            frozen: false,
            accepts: 0,
            rejects: 0,
        }
    }

    /// 已定义句数。
    pub fn len(&self) -> usize {
        self.clauses.len()
    }

    /// 是否空白。
    pub fn is_empty(&self) -> bool {
        self.clauses.is_empty()
    }

    /// 取某句定义。
    pub fn clause(&self, id: usize) -> Option<Clause> {
        let mut i = 0usize;
        while i < self.clauses.len() {
            if self.clauses[i].id as usize == id {
                return Some(self.clauses[i]);
            }
            i = i.wrapping_add(1);
        }
        None
    }

    /// 定义一条规格句：冻结→句号→重复→零值。
    pub fn define(&mut self, id: usize, need: u64) -> Result<u64, u32> {
        if self.frozen || self.defs_locked.is_some() {
            return self.fail(BC_EFROZEN);
        }
        if id >= CLAUSES {
            return self.fail(BC_ECLAUSE);
        }
        if self.clause(id).is_some() {
            return self.fail(BC_EDUP);
        }
        if need == 0 {
            return self.fail(BC_ENEED);
        }
        self.clauses.push(Clause {
            id: id as u8,
            need,
            have: None,
        });
        self.accepts = self.accepts.wrapping_add(1);
        Ok(self.need_fold())
    }

    /// 供一条实有值：未定义句拒。
    pub fn supply(&mut self, id: usize, have: u64) -> Result<u64, u32> {
        if self.frozen {
            return self.fail(BC_EFROZEN);
        }
        let mut i = 0usize;
        while i < self.clauses.len() {
            if self.clauses[i].id as usize == id {
                self.clauses[i].have = Some(have);
                self.accepts = self.accepts.wrapping_add(1);
                return Ok(self.have_fold());
            }
            i = i.wrapping_add(1);
        }
        self.fail(BC_EMISS)
    }

    /// 记账一次拒止。
    fn fail(&mut self, code: u32) -> Result<u64, u32> {
        self.rejects = self.rejects.wrapping_add(1);
        Err(code)
    }

    /// 应为侧指纹（断言定义的载体）。
    pub fn need_fold(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.clauses.len() {
            h = (h ^ self.clauses[i].id as u64).wrapping_mul(FNV_PRIME);
            h = (h ^ self.clauses[i].need).wrapping_mul(FNV_PRIME);
            i = i.wrapping_add(1);
        }
        (h ^ self.clauses.len() as u64).wrapping_mul(FNV_PRIME)
    }

    /// 实有侧指纹。
    pub fn have_fold(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.clauses.len() {
            let v = match self.clauses[i].have {
                Some(v) => v,
                None => 0,
            };
            h = (h ^ self.clauses[i].id as u64).wrapping_mul(FNV_PRIME);
            h = (h ^ v).wrapping_mul(FNV_PRIME);
            i = i.wrapping_add(1);
        }
        (h ^ self.clauses.len() as u64).wrapping_mul(FNV_PRIME)
    }

    /// 缺句数。
    pub fn missing(&self) -> usize {
        CLAUSES.saturating_sub(self.clauses.len())
    }

    /// 预检：六句齐、实有全供、逐句相等。
    pub fn precheck(&mut self) -> Result<u64, u32> {
        if self.clauses.len() != CLAUSES {
            return self.fail(BC_ESHORT);
        }
        let mut i = 0usize;
        while i < CLAUSES {
            let c = match self.clause(i) {
                Some(v) => v,
                None => return self.fail(BC_ESHORT),
            };
            let have = match c.have {
                Some(v) => v,
                None => return self.fail(BC_EMISS),
            };
            if have != c.need {
                return self.fail(BC_EMISMATCH);
            }
            i = i.wrapping_add(1);
        }
        Ok(self.have_fold())
    }

    /// 冻结三断言的「定义」：六句齐才许冻，重复调用返回同一值。
    pub fn freeze_assertions(&mut self) -> Result<u64, u32> {
        if let Some(v) = self.defs_locked {
            return Ok(v);
        }
        if self.frozen {
            return self.fail(BC_EFROZEN);
        }
        if self.clauses.len() != CLAUSES {
            return self.fail(BC_ESHORT);
        }
        let v = self.need_fold();
        self.defs_locked = Some(v);
        Ok(v)
    }

    /// 定义冻结值。
    pub fn defs_value(&self) -> Option<u64> {
        self.defs_locked
    }

    /// 核对定义未漂移：须已冻结，且当前应为侧指纹与冻结值一致。
    pub fn verify_defs(&mut self) -> Result<u64, u32> {
        let locked = match self.defs_locked {
            Some(v) => v,
            None => return self.fail(BC_EASRT),
        };
        if self.need_fold() != locked {
            return self.fail(BC_EDRIFT);
        }
        Ok(locked)
    }

    /// 三条断言的就位情况。
    pub fn three_assertions(&self) -> [bool; 3] {
        let c0 = self.clause(0);
        let c1 = self.clause(1);
        let c2 = self.clause(2);
        let c3 = self.clause(3);
        let c4 = self.clause(4);
        let c5 = self.clause(5);
        let a1 = matches!(c0, Some(c) if c.need == DOMAIN_CRITERIA)
            && matches!(c3, Some(c) if c.need == DOMAIN_CARDS);
        let a2 = matches!(c1, Some(c) if c.need == DOMAIN_LINES)
            && matches!(c2, Some(c) if c.need == DOMAIN_BATCHES);
        let a3 = matches!(c4, Some(c) if c.need != 0)
            && matches!(c5, Some(c) if c.need == STATE_DONE);
        [a1, a2, a3]
    }

    /// 渲染标志条正文：每句「应为/实有」成对。
    pub fn banner_line(&self) -> String {
        let mut out = String::new();
        out.push_str("UNX-F11189");
        let mut i = 0usize;
        while i < CLAUSES {
            out.push(' ');
            match CLAUSE_NAMES.get(i) {
                Some(n) => out.push_str(n),
                None => out.push_str("?"),
            }
            out.push('=');
            match self.clause(i) {
                Some(c) => {
                    push_u64(&mut out, c.need);
                    out.push('/');
                    push_u64(&mut out, c.have.unwrap_or(0));
                }
                None => out.push_str("0/0"),
            }
            i = i.wrapping_add(1);
        }
        out
    }

    /// 定稿冻结：断言定义已冻且定义未漂移才锁值。
    pub fn freeze(&mut self) -> Option<u64> {
        if self.frozen {
            return self.defs_locked;
        }
        let locked = self.defs_locked?;
        if self.need_fold() != locked {
            return None;
        }
        self.frozen = true;
        Some(locked)
    }

    /// 是否已定稿。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }
}

/// 无符号数追加到字符串（不经 format，避免中间分配）。
fn push_u64(out: &mut String, v: u64) {
    let mut stack = [0u8; 20];
    let mut n = 0usize;
    let mut x = v;
    if x == 0 {
        out.push('0');
        return;
    }
    while x > 0 {
        if let Some(slot) = stack.get_mut(n) {
            *slot = b'0'.wrapping_add((x % 10) as u8);
        }
        n = n.wrapping_add(1);
        x /= 10;
    }
    let mut j = n;
    while j > 0 {
        j = j.wrapping_sub(1);
        if let Some(d) = stack.get(j) {
            out.push(*d as char);
        }
    }
}

/// 标志条规范锚：批号 + 六句应为值的指纹。
pub fn canonical_anchor() -> u64 {
    let mut h = FNV_OFF;
    h = (h ^ 11189u64).wrapping_mul(FNV_PRIME);
    let needs = [DOMAIN_CRITERIA, DOMAIN_LINES, DOMAIN_BATCHES, DOMAIN_CARDS, 0u64, STATE_DONE];
    let mut i = 0usize;
    while i < needs.len() {
        if let Some(v) = needs.get(i) {
            // 锚句自身不参与，用批号代替其位置
            h = (h ^ if i == 4 { 11189 } else { *v }).wrapping_mul(FNV_PRIME);
        }
        i = i.wrapping_add(1);
    }
    (h ^ CLAUSES as u64).wrapping_mul(FNV_PRIME)
}

/// 断言未冻结即核对定义
pub const BC_EASRT: u32 = SEG_BASE + 0x09;

/// 建条：六句按序定义、逐句供值、预检、冻断言定义、定稿。
pub fn build_banner() -> (Banner, bool) {
    let mut b = Banner::new();
    let needs: [u64; CLAUSES] = [
        DOMAIN_CRITERIA,
        DOMAIN_LINES,
        DOMAIN_BATCHES,
        DOMAIN_CARDS,
        canonical_anchor(),
        STATE_DONE,
    ];
    let mut i = 0usize;
    while i < CLAUSES {
        let v = match needs.get(i) {
            Some(x) => *x,
            None => return (b, false),
        };
        if b.define(i, v).is_err() {
            return (b, false);
        }
        i = i.wrapping_add(1);
    }
    let mut j = 0usize;
    while j < CLAUSES {
        let v = match needs.get(j) {
            Some(x) => *x,
            None => return (b, false),
        };
        if b.supply(j, v).is_err() {
            return (b, false);
        }
        j = j.wrapping_add(1);
    }
    let pre = b.precheck().is_ok();
    let locked = b.freeze_assertions().is_ok();
    let done = b.freeze().is_some();
    (b, pre && locked && done)
}

/// 四类注入：句号越界 / 重句与零值 / 缺句与值不符与未供 / 冻后改定义。
pub fn inject_four() -> [bool; 4] {
    // ① 句号越界
    let mut a0 = Banner::new();
    let r0 = a0.define(CLAUSES, 1).err() == Some(BC_ECLAUSE)
        && a0.define(CLAUSES.wrapping_add(5), 1).err() == Some(BC_ECLAUSE);

    // ② 重句与零值句
    let mut a1 = Banner::new();
    let s1 = a1.define(0, DOMAIN_CRITERIA).is_ok();
    let r1 = s1
        && a1.define(0, DOMAIN_CRITERIA).err() == Some(BC_EDUP)
        && a1.define(1, 0).err() == Some(BC_ENEED);

    // ③ 缺句即冻断言 / 未供值 / 值不符
    let mut a2 = Banner::new();
    let _ = a2.define(0, DOMAIN_CRITERIA);
    let short = a2.freeze_assertions().err() == Some(BC_ESHORT);
    let mut a3 = Banner::new();
    let mut i = 0usize;
    while i < CLAUSES {
        let _ = a3.define(i, 1);
        i = i.wrapping_add(1);
    }
    let unfed = a3.precheck().err() == Some(BC_EMISS);
    let mut a4 = Banner::new();
    let _ = a4.define(0, DOMAIN_CRITERIA);
    let miss = a4.supply(3, 1).err() == Some(BC_EMISS);
    let r2 = short && unfed && miss;

    // ④ 冻后改定义 + 未冻断言核对
    let (mut a5, ok5) = build_banner();
    let drift_guard = a5.define(0, 1).err() == Some(BC_EFROZEN)
        && a5.supply(0, 1).err() == Some(BC_EFROZEN);
    let mut a6 = Banner::new();
    let _ = a6.define(0, DOMAIN_CRITERIA);
    let unsealed = a6.verify_defs().err() == Some(BC_EASRT);
    let r3 = ok5 && drift_guard && unsealed;

    [r0, r1, r2, r3]
}

/// 判据：UNX-F11189 收官标志条规格核验。
pub fn run_unx11189_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11189");
    let (mut b, ok) = build_banner();

    // B1 六句齐备
    s.add_dyn(
        "B1-六句齐备",
        ok && b.len() == CLAUSES && b.missing() == 0,
        &format!("clauses={}/{} missing={}", b.len(), CLAUSES, b.missing()),
    );

    // B2 应当值逐句正确
    let spec: [u64; CLAUSES] = [
        DOMAIN_CRITERIA,
        DOMAIN_LINES,
        DOMAIN_BATCHES,
        DOMAIN_CARDS,
        canonical_anchor(),
        STATE_DONE,
    ];
    let mut spec_ok = true;
    let mut i = 0usize;
    while i < CLAUSES {
        let want = match spec.get(i) {
            Some(v) => *v,
            None => 0,
        };
        match b.clause(i) {
            Some(c) => {
                if c.need != want {
                    spec_ok = false;
                }
            }
            None => spec_ok = false,
        }
        i = i.wrapping_add(1);
    }
    s.add("B2-应当值逐句正确", spec_ok, "判据/行数/批数/卡数/锚/状态六句应与规格一致");

    // B3 实有全供且相等
    let pre = b.precheck();
    s.add_dyn("B3-实有全供相等", pre.is_ok(), &format!("have_fold={:#x}", b.have_fold()));

    // B4 三断言就位
    let tri = b.three_assertions();
    s.add_dyn(
        "B4-三断言就位",
        tri[0] && tri[1] && tri[2],
        &format!("A1={} A2={} A3={}", tri[0], tri[1], tri[2]),
    );

    // B5 断言定义已冻且幂等
    let d1 = b.freeze_assertions();
    let d2 = b.freeze_assertions();
    s.add(
        "B5-断言定义冻结幂等",
        d1.is_ok() && d1 == d2 && b.defs_value() == d1.ok(),
        "重复冻结断言定义返回同一值",
    );

    // B6 定义未漂移
    let vd = b.verify_defs();
    s.add_dyn(
        "B6-定义未漂移",
        vd.is_ok() && vd.ok() == b.defs_value(),
        &format!("defs={:#x}", b.defs_value().unwrap_or(0)),
    );

    // B7 定稿冻结幂等
    let f1 = b.freeze();
    let f2 = b.freeze();
    s.add("B7-定稿冻结幂等", f1.is_some() && f1 == f2 && b.is_frozen(), "定稿只锁一次单值");

    // B8 定稿后拒改
    s.add(
        "B8-定稿后拒改",
        b.define(0, 1).err() == Some(BC_EFROZEN) && b.supply(0, 1).err() == Some(BC_EFROZEN),
        "定稿后定义与实有皆不可再写",
    );

    // B9 缺句拒冻断言
    let mut h = Banner::new();
    let _ = h.define(0, DOMAIN_CRITERIA);
    let _ = h.define(1, DOMAIN_LINES);
    s.add_dyn(
        "B9-缺句拒冻断言",
        h.freeze_assertions().err() == Some(BC_ESHORT) && h.missing() == CLAUSES - 2,
        &format!("missing={}", h.missing()),
    );

    // B10 值不符拒预检
    let mut m = Banner::new();
    let mut i = 0usize;
    while i < CLAUSES {
        let _ = m.define(i, 1);
        let _ = m.supply(i, if i == 1 { 2 } else { 1 });
        i = i.wrapping_add(1);
    }
    s.add(
        "B10-值不符拒预检",
        m.precheck().err() == Some(BC_EMISMATCH),
        "任一句实有偏离应为即拒",
    );

    // B11 未供值拒预检
    let mut u = Banner::new();
    let mut i = 0usize;
    while i < CLAUSES {
        let _ = u.define(i, 1);
        i = i.wrapping_add(1);
    }
    s.add("B11-未供值拒预检", u.precheck().err() == Some(BC_EMISS), "实有侧缺供即拒");

    // B12 句号越界与重句拒止
    let mut g = Banner::new();
    let g0 = g.define(0, DOMAIN_CRITERIA).is_ok();
    s.add(
        "B12-越界与重句拒止",
        g.define(CLAUSES, 1).err() == Some(BC_ECLAUSE)
            && g0
            && g.define(0, DOMAIN_CRITERIA).err() == Some(BC_EDUP),
        "句号越界与重句各按其码拒止",
    );

    // B13 注入四拒 + 正文渲染（新条重算）
    let hits = inject_four();
    let mut all = true;
    let mut k = 0usize;
    while k < hits.len() {
        if !hits[k] {
            all = false;
        }
        k = k.wrapping_add(1);
    }
    let (mut fresh, ok2) = build_banner();
    let line = fresh.banner_line();
    let root = fresh.freeze().unwrap_or(0);
    s.add_dyn(
        "B13-注入四拒与正文",
        all && ok2 && fresh.rejects == 0 && fresh.accepts == (CLAUSES as u64) * 2 && root != 0,
        &format!(
            "inject={}{}{}{} acc={} rej={} | {}",
            hits[0] as u8, hits[1] as u8, hits[2] as u8, hits[3] as u8,
            fresh.accepts, fresh.rejects, line
        ),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f11189_banner_flow() {
        let (mut b, ok) = build_banner();
        assert!(ok);
        assert_eq!(b.len(), CLAUSES);
        assert!(b.precheck().is_ok());
        let tri = b.three_assertions();
        assert!(tri[0] && tri[1] && tri[2]);
        let defs = b.freeze_assertions();
        assert!(defs.is_ok());
        assert_eq!(defs, b.freeze_assertions());
        assert!(b.verify_defs().is_ok());
        let v = b.freeze();
        assert!(v.is_some());
        assert_eq!(b.freeze(), v);
        assert_eq!(b.define(0, 1), Err(BC_EFROZEN));
        let line = b.banner_line();
        assert!(line.starts_with("UNX-F11189"));
        assert!(line.contains("criteria=800/800"));
    }

    #[test]
    fn f11189_spec_gates() {
        let mut b = Banner::new();
        assert_eq!(b.define(CLAUSES, 1), Err(BC_ECLAUSE));
        assert_eq!(b.define(0, 0), Err(BC_ENEED));
        assert!(b.define(0, DOMAIN_CRITERIA).is_ok());
        assert_eq!(b.define(0, DOMAIN_CRITERIA), Err(BC_EDUP));
        assert_eq!(b.supply(3, 1), Err(BC_EMISS));
        assert!(b.supply(0, DOMAIN_CRITERIA).is_ok());
        assert_eq!(b.verify_defs(), Err(BC_EASRT));
        assert_eq!(b.freeze_assertions(), Err(BC_ESHORT));
        assert_eq!(b.rejects, 6);
    }

    #[test]
    fn f11189_checks_green() {
        let cs = run_unx11189_checks();
        assert!(cs.all_passed());
    }
}
