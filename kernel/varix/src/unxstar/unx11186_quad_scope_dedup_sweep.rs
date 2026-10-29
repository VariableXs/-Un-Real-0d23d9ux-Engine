//! UNX-F11186 防重四范围终扫（全域唯一 + 跨域零侵入终验）（0x1f6b0 段）
//!
//! 纯功能模型：防重账分四个登记范围（scope 0..3），每范围 SLOTS 席。
//! 全场唯一性按「键」而非「席位」裁定——同一键无论落在哪个范围都只许
//! 出现一次；因此跨范围重复即为「跨域侵入」，与域内重号共用同一张键表。
//! 终扫要求四范围满席、键表无重、无裸席（owner=0）、终盘值可冻结且幂等。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1f6b0;
/// 范围号越界
pub const QS_ESCOPE: u32 = SEG_BASE + 0x01;
/// 键号越界
pub const QS_EKEY: u32 = SEG_BASE + 0x02;
/// 域内重号
pub const QS_EDUP: u32 = SEG_BASE + 0x03;
/// 跨域同号（全域重复 / 侵入）
pub const QS_EGLOBAL: u32 = SEG_BASE + 0x04;
/// 无主登记（owner=0）
pub const QS_EOWNER: u32 = SEG_BASE + 0x05;
/// 未满席即终扫
pub const QS_ESHORT: u32 = SEG_BASE + 0x06;
/// 已冻结 / 已终扫后再次动账
pub const QS_EFROZEN: u32 = SEG_BASE + 0x07;
/// 空白账（无任何登记）
pub const QS_EEMPTY: u32 = SEG_BASE + 0x08;

/// 登记范围数
pub const SCOPES: usize = 4;
/// 每范围席数
pub const SLOTS: usize = 16;
/// 全场席数
pub const TOTAL_SLOTS: usize = SCOPES * SLOTS;
/// 键域上界（键号 0..KEY_SPAN）
pub const KEY_SPAN: u32 = TOTAL_SLOTS as u32;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 一条登记：范围 / 席次 / 键 / 属主。
#[derive(Clone, Copy, Debug)]
pub struct Claim {
    pub scope: u8,
    pub slot: u8,
    pub key: u32,
    pub owner: u16,
}

/// 防重四范围终扫账。
#[derive(Clone, Debug)]
pub struct QuadSweep {
    claims: Vec<Claim>,
    finalized: Option<u64>,
    frozen: bool,
    pub accepts: u64,
    pub rejects: u64,
    pub rescans: u64,
}

impl QuadSweep {
    /// 空账。
    pub fn new() -> Self {
        QuadSweep {
            claims: Vec::new(),
            finalized: None,
            frozen: false,
            accepts: 0,
            rejects: 0,
            rescans: 0,
        }
    }

    /// 已登记席数。
    pub fn count(&self) -> usize {
        self.claims.len()
    }

    /// 某范围已登记席数。
    pub fn occupancy(&self, scope: usize) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.claims.len() {
            if self.claims[i].scope as usize == scope {
                n = n.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        n
    }

    /// 裸席数（属主为零）。
    pub fn bare(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.claims.len() {
            if self.claims[i].owner == 0 {
                n = n.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        n
    }

    /// 四范围席次表。
    pub fn scope_table(&self) -> [usize; SCOPES] {
        let mut t = [0usize; SCOPES];
        let mut s = 0usize;
        while s < SCOPES {
            t[s] = self.occupancy(s);
            s = s.wrapping_add(1);
        }
        t
    }

    /// 按键查找其登记位置。
    pub fn locate(&self, key: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.claims.len() {
            if self.claims[i].key == key {
                return Some(i);
            }
            i = i.wrapping_add(1);
        }
        None
    }

    /// 登记一键。先判冻结，再判范围/键域/属主，最后按「同键」与「席满」裁定。
    pub fn claim(&mut self, scope: usize, key: u32, owner: u16) -> Result<u64, u32> {
        if self.frozen {
            return self.fail(QS_EFROZEN);
        }
        if scope >= SCOPES {
            return self.fail(QS_ESCOPE);
        }
        if key >= KEY_SPAN {
            return self.fail(QS_EKEY);
        }
        if owner == 0 {
            return self.fail(QS_EOWNER);
        }
        if let Some(i) = self.locate(key) {
            // 同键先于席满裁定：域内同号与跨域侵入各有其码。
            let code = if self.claims[i].scope as usize == scope {
                QS_EDUP
            } else {
                QS_EGLOBAL
            };
            return self.fail(code);
        }
        let slot = self.occupancy(scope);
        if slot >= SLOTS {
            return self.fail(QS_EFULL);
        }
        self.claims.push(Claim {
            scope: scope as u8,
            slot: slot as u8,
            key,
            owner,
        });
        self.accepts = self.accepts.wrapping_add(1);
        Ok(self.digest())
    }

    /// 记账一次拒止。
    fn fail(&mut self, code: u32) -> Result<u64, u32> {
        self.rejects = self.rejects.wrapping_add(1);
        Err(code)
    }

    /// 全场重扫：返回重复键的对数（真扫，不做捷径判定）。
    pub fn duplicate_scan(&mut self) -> usize {
        self.rescans = self.rescans.wrapping_add(1);
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.claims.len() {
            let mut j = i.wrapping_add(1);
            while j < self.claims.len() {
                if self.claims[i].key == self.claims[j].key {
                    n = n.wrapping_add(1);
                }
                j = j.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        n
    }

    /// 跨域侵入键数：同键出现在两个及以上不同范围。
    pub fn intrusions(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.claims.len() {
            let a = self.claims[i];
            let mut hit_other = false;
            let mut j = 0usize;
            while j < self.claims.len() {
                let b = self.claims[j];
                if b.key == a.key && b.scope != a.scope {
                    hit_other = true;
                }
                j = j.wrapping_add(1);
            }
            if hit_other {
                n = n.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        n
    }

    /// 全域唯一：无任何重复键。
    pub fn unique_globally(&self) -> bool {
        let mut i = 0usize;
        while i < self.claims.len() {
            let mut j = i.wrapping_add(1);
            while j < self.claims.len() {
                if self.claims[i].key == self.claims[j].key {
                    return false;
                }
                j = j.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        true
    }

    /// 跨域零侵入。
    pub fn cross_scope_zero(&self) -> bool {
        self.intrusions() == 0
    }

    /// 四范围满席。
    pub fn full(&self) -> bool {
        let t = self.scope_table();
        let mut s = 0usize;
        while s < SCOPES {
            if t[s] != SLOTS {
                return false;
            }
            s = s.wrapping_add(1);
        }
        true
    }

    /// 终扫指纹。
    pub fn digest(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.claims.len() {
            let c = self.claims[i];
            h = (h ^ (c.scope as u64)).wrapping_mul(FNV_PRIME);
            h = (h ^ (c.slot as u64)).wrapping_mul(FNV_PRIME);
            h = (h ^ (c.key as u64)).wrapping_mul(FNV_PRIME);
            h = (h ^ (c.owner as u64)).wrapping_mul(FNV_PRIME);
            i = i.wrapping_add(1);
        }
        h = (h ^ self.count() as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.accepts).wrapping_mul(FNV_PRIME);
        h
    }

    /// 终扫：满席 + 全域唯一 + 零裸席，缺一即拒。
    pub fn finalize(&mut self) -> Result<u64, u32> {
        if self.finalized.is_some() {
            return self.fail(QS_EFROZEN);
        }
        if self.claims.is_empty() {
            return self.fail(QS_EEMPTY);
        }
        if self.count() != TOTAL_SLOTS {
            return self.fail(QS_ESHORT);
        }
        if !self.unique_globally() {
            return self.fail(QS_EGLOBAL);
        }
        if self.bare() != 0 {
            return self.fail(QS_EOWNER);
        }
        let v = self.digest();
        self.finalized = Some(v);
        Ok(v)
    }

    /// 冻结：锁定当下单值，幂等。
    pub fn freeze(&mut self) -> Option<u64> {
        if self.frozen {
            return self.finalized;
        }
        if self.finalized.is_none() {
            self.finalized = Some(self.digest());
        }
        self.frozen = true;
        self.finalized
    }

    /// 终扫值。
    pub fn final_value(&self) -> Option<u64> {
        self.finalized
    }
}

/// 范围席满（第 SLOTS+1 席登记）
pub const QS_EFULL: u32 = SEG_BASE + 0x09;

/// 建场：四范围各 SLOTS 席，键按 范围×SLOTS+席次 铺开，属主随范围。
pub fn build_sweep() -> (QuadSweep, bool) {
    let mut q = QuadSweep::new();
    let mut s = 0usize;
    while s < SCOPES {
        let mut i = 0usize;
        while i < SLOTS {
            let key = (s.wrapping_mul(SLOTS).wrapping_add(i)) as u32;
            let owner = 0x10u16.wrapping_add(s as u16);
            if q.claim(s, key, owner).is_err() {
                return (q, false);
            }
            i = i.wrapping_add(1);
        }
        s = s.wrapping_add(1);
    }
    let ok = q.finalize().is_ok();
    (q, ok)
}

/// 四类注入：范围越界 / 键域越界 / 域内重号 / 跨域同号 / 冻结后动账。
pub fn inject_four() -> [bool; 4] {
    // ① 范围号与键号越界
    let mut a0 = QuadSweep::new();
    let r0 = a0.claim(SCOPES, 0, 1).err() == Some(QS_ESCOPE)
        && a0.claim(SCOPES.wrapping_add(3), 1, 1).err() == Some(QS_ESCOPE)
        && a0.claim(0, KEY_SPAN, 1).err() == Some(QS_EKEY);

    // ② 域内重号：先登记一键，再在同范围回填同键
    let mut a1 = QuadSweep::new();
    let s1 = a1.claim(0, 3, 7).is_ok();
    let r1 = s1 && a1.claim(0, 3, 8).err() == Some(QS_EDUP);

    // ③ 跨域同号：同键落到另一范围即侵入
    let mut a2 = QuadSweep::new();
    let s2 = a2.claim(0, 5, 9).is_ok();
    let r2 = s2
        && a2.claim(1, 5, 10).err() == Some(QS_EGLOBAL)
        && a2.claim(3, 5, 9).err() == Some(QS_EGLOBAL);

    // ④ 冻结后登记与二次终扫
    let mut a3 = QuadSweep::new();
    let s3 = a3.claim(2, 9, 11).is_ok();
    let locked = a3.freeze().is_some();
    let r3 = s3
        && locked
        && a3.claim(2, 10, 11).err() == Some(QS_EFROZEN)
        && a3.finalize().err() == Some(QS_EFROZEN);

    [r0, r1, r2, r3]
}

/// 判据：UNX-F11186 四范围终扫。
pub fn run_unx11186_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11186");
    let (mut q, ok) = build_sweep();

    // QS1 建场满席
    s.add_dyn(
        "QS1-建场满席",
        ok && q.full() && q.count() == TOTAL_SLOTS,
        &format!("count={} full={} ok={}", q.count(), q.full(), ok),
    );

    // QS2 全域唯一
    let dups = q.duplicate_scan();
    s.add_dyn(
        "QS2-全域唯一",
        q.unique_globally() && dups == 0,
        &format!("dups={} rescans={}", dups, q.rescans),
    );

    // QS3 跨域零侵入
    s.add(
        "QS3-跨域零侵入",
        q.cross_scope_zero() && q.intrusions() == 0,
        "四范围键域两两不交，侵入键数为零",
    );

    // QS4 面表均衡
    let t = q.scope_table();
    let mut balanced = true;
    let mut i = 0usize;
    while i < SCOPES {
        if t[i] != SLOTS {
            balanced = false;
        }
        i = i.wrapping_add(1);
    }
    s.add_dyn(
        "QS4-面表均衡",
        balanced,
        &format!("t0={} t1={} t2={} t3={}", t[0], t[1], t[2], t[3]),
    );

    // QS5 终扫指纹
    let root = q.final_value();
    s.add(
        "QS5-终扫指纹",
        root == Some(q.digest()) && q.freeze() == root,
        "终扫值即全场指纹且冻结不改变它",
    );

    // QS6 冻结幂等
    let f1 = q.freeze();
    let f2 = q.freeze();
    s.add(
        "QS6-冻结幂等",
        f1 == f2 && f1.is_some(),
        "二次冻结返回同一单值",
    );

    // QS7 冻结后拒登记
    s.add(
        "QS7-冻结后拒登记",
        q.claim(0, 0, 1).err() == Some(QS_EFROZEN),
        "冻结账不再接受任何登记",
    );

    // QS8 空账拒终扫
    let mut e = QuadSweep::new();
    s.add(
        "QS8-空账拒终扫",
        e.finalize().err() == Some(QS_EEMPTY) && e.count() == 0,
        "空账不得终扫",
    );

    // QS9 未满席拒终扫
    let mut p = QuadSweep::new();
    let _ = p.claim(0, 1, 1);
    let _ = p.claim(1, 2, 1);
    let _ = p.claim(2, 3, 1);
    s.add_dyn(
        "QS9-未满席拒终扫",
        p.finalize().err() == Some(QS_ESHORT) && p.count() == 3,
        &format!("partial={} code=ESHORT", p.count()),
    );

    // QS10 无主拒止
    let mut o = QuadSweep::new();
    s.add(
        "QS10-无主拒止",
        o.claim(0, 0, 0).err() == Some(QS_EOWNER) && o.bare() == 0,
        "属主为零的登记一律拒止",
    );

    // QS11 域内重号拒止
    let mut d = QuadSweep::new();
    let d0 = d.claim(1, 7, 3).is_ok();
    s.add(
        "QS11-域内重号拒止",
        d0 && d.claim(1, 7, 3).err() == Some(QS_EDUP) && d.count() == 1,
        "同范围同键只许一次",
    );

    // QS12 跨域同号拒止
    let mut g = QuadSweep::new();
    let g0 = g.claim(0, 9, 4).is_ok();
    s.add(
        "QS12-跨域同号拒止",
        g0
            && g.claim(2, 9, 4).err() == Some(QS_EGLOBAL)
            && g.intrusions() == 0
            && g.count() == 1,
        "同键换范围亦拒，侵入零留存",
    );

    // QS13 席满拒止
    let mut f = QuadSweep::new();
    let mut i = 0usize;
    while i < SLOTS {
        let _ = f.claim(0, i as u32, 5);
        i = i.wrapping_add(1);
    }
    s.add_dyn(
        "QS13-席满拒止",
        f.claim(0, 60, 5).err() == Some(QS_EFULL) && f.occupancy(0) == SLOTS,
        &format!("scope0={} code=EFULL", f.occupancy(0)),
    );

    // QS14 注入四拒 + 终盘明细（新场重算，不复用已污染实例）
    let hits = inject_four();
    let mut all = true;
    let mut k = 0usize;
    while k < hits.len() {
        if !hits[k] {
            all = false;
        }
        k = k.wrapping_add(1);
    }
    let (mut fin, ok2) = build_sweep();
    let root2 = fin.freeze().unwrap_or(0);
    s.add_dyn(
        "QS14-注入四拒与终盘明细",
        all && ok2 && fin.rejects == 0 && fin.accepts == TOTAL_SLOTS as u64 && root2 != 0,
        &format!(
            "inject={}{}{}{} acc={} rej={} root={:#x}",
            hits[0] as u8, hits[1] as u8, hits[2] as u8, hits[3] as u8,
            fin.accepts, fin.rejects, root2
        ),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f11186_sweep_flow() {
        let (mut q, ok) = build_sweep();
        assert!(ok);
        assert_eq!(q.count(), TOTAL_SLOTS);
        assert!(q.full());
        assert!(q.unique_globally());
        assert!(q.cross_scope_zero());
        let root = q.final_value();
        assert_eq!(root, Some(q.digest()));
        assert_eq!(q.freeze(), root);
        assert_eq!(q.freeze(), root);
        assert_eq!(q.claim(0, 0, 1), Err(QS_EFROZEN));
        assert_eq!(q.intrusions(), 0);
        assert_eq!(q.bare(), 0);
    }

    #[test]
    fn f11186_dedup_semantics() {
        let mut q = QuadSweep::new();
        assert!(q.claim(0, 1, 1).is_ok());
        assert_eq!(q.claim(0, 1, 2), Err(QS_EDUP));
        assert_eq!(q.claim(2, 1, 1), Err(QS_EGLOBAL));
        assert_eq!(q.claim(SCOPES, 2, 1), Err(QS_ESCOPE));
        assert_eq!(q.claim(1, KEY_SPAN, 1), Err(QS_EKEY));
        assert_eq!(q.claim(1, 2, 0), Err(QS_EOWNER));
        assert_eq!(q.count(), 1);
        assert_eq!(q.rejects, 5);
        assert_eq!(q.accepts, 1);
        assert!(q.unique_globally());
    }

    #[test]
    fn f11186_checks_green() {
        let cs = run_unx11186_checks();
        assert!(cs.all_passed());
    }
}
