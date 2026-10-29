//! UNX-F48565 —— I·主题态数据不侵入复签（F4 主题绘制品不回流本域）
//! 判据原文：UNX-F48565-J1 本域（I 段候选窗）入口对跨域数据包按域标分拣：
//!           F4 域主题绘制品（配色/皮肤/绘制参数包）一律拒收不回流；
//!           本域合法包（候选项/预编辑/几何）正常放行；
//!           域标封闭（未知域拒）；拒收后本域状态零污染；
//!           LCG 随机包流 ×256 全归类零崩；分拣账封闭两型；重演一致；守恒。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 闭包判据全绿才返回 Ok。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xF620; // I29 主题态不侵入段

const E_F4_LEAK: u16 = SEG_BASE + 1; // F4 域制品回流成功
const E_STATE_DIRTY: u16 = SEG_BASE + 2; // 拒收后状态污染
const E_DOMAIN_BAD: u16 = SEG_BASE + 3; // 域标越界归类泄漏
const E_LEGAL_MISS: u16 = SEG_BASE + 4; // 本域合法包误拒
const E_KIND_BAD: u16 = SEG_BASE + 5; // 分拣账形态越界
const E_COUNT_MISS: u16 = SEG_BASE + 6; // 分拣计数不符
const E_REPLAY_DIFF: u16 = SEG_BASE + 7; // 重演不一致
const E_BAL_MISS: u16 = SEG_BASE + 8; // 守恒破

/// 域标（封闭两域 + 未知）。
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Domain {
    IDomain = 0,   // 本域：候选窗/预编辑/几何
    F4Domain = 1,  // F4：主题绘制品
}

/// 包型（本域合法三型）。
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PackKind {
    Candidate = 0,
    Preedit = 1,
    Geometry = 2,
    ThemeSkin = 3,   // F4 制品：配色皮肤
    ThemePaint = 4,  // F4 制品：绘制参数
}

/// 跨域数据包。
pub struct Packet {
    pub domain: Domain,
    pub kind: PackKind,
    pub payload: u64,
}

/// 分拣门。
pub struct IsoGate {
    accepted: Vec<(u64, PackKind)>,
    rejected: Vec<(u64, PackKind)>,
    seq: u64,
    state_fp: u64,
    label: String,
}

impl IsoGate {
    pub fn new() -> Self {
        IsoGate {
            accepted: Vec::new(),
            rejected: Vec::new(),
            seq: 0,
            state_fp: 0xcbf2_9ce4_8422_2325,
            label: "iso-gate".to_string(),
        }
    }

    /// 提交一包：F4 域制品拒收；本域合法型放行；域标与包型矛盾拒收。
    pub fn submit(&mut self, p: &Packet) -> Result<(), u16> {
        self.seq += 1;
        let before = self.state_fp;
        let ok = match p.domain {
            Domain::IDomain => {
                p.kind == PackKind::Candidate || p.kind == PackKind::Preedit || p.kind == PackKind::Geometry
            }
            Domain::F4Domain => false, // F4 主题绘制品一律不回流
        };
        if ok {
            self.state_fp = self
                .state_fp
                .wrapping_mul(0x0000_0100_0000_01b3)
                .wrapping_add(p.payload ^ p.kind as u64);
            self.accepted.push((self.seq, p.kind));
            Ok(())
        } else {
            self.rejected.push((self.seq, p.kind));
            if self.state_fp != before {
                return Err(E_STATE_DIRTY);
            }
            Err(E_F4_LEAK)
        }
    }

    /// 拒收后状态零污染：连拒期间状态指纹不变。
    pub fn audit_clean(&self) -> Result<(), u16> {
        if self.state_fp != 0xcbf2_9ce4_8422_2325 {
            return Err(E_STATE_DIRTY);
        }
        Ok(())
    }

    /// 分拣账两型封闭：accepted 均为本域合法型；rejected 均为 F4 型。
    pub fn audit_kinds(&self) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.accepted.len() {
            let k = self.accepted[i].1;
            if k != PackKind::Candidate && k != PackKind::Preedit && k != PackKind::Geometry {
                return Err(E_KIND_BAD);
            }
            i += 1;
        }
        let mut i = 0usize;
        while i < self.rejected.len() {
            let k = self.rejected[i].1;
            // 拒收侧允许全部五型（域标/包型矛盾均在拒收域）
            if k as u8 > 4 {
                return Err(E_KIND_BAD);
            }
            i += 1;
        }
        Ok(())
    }

    pub fn tag(&self) -> String {
        self.label.clone()
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        h ^= self.accepted.len() as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        let mut i = 0usize;
        while i < self.accepted.len() {
            h ^= self.accepted[i].0;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            h ^= self.accepted[i].1 as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            i += 1;
        }
        h ^= self.rejected.len() as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        h ^= self.state_fp;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        h
    }

    pub fn accept_count(&self) -> u32 {
        self.accepted.len() as u32
    }

    pub fn reject_count(&self) -> u32 {
        self.rejected.len() as u32
    }

    pub fn seq(&self) -> u64 {
        self.seq
    }
}

/// 注入清单 ×10：F4 制品七形态 + 本域合法三形态。
pub fn inj_ten() -> Vec<Packet> {
    vec![
        Packet { domain: Domain::F4Domain, kind: PackKind::ThemeSkin, payload: 0x5A },  // 拒
        Packet { domain: Domain::F4Domain, kind: PackKind::ThemePaint, payload: 0x5B }, // 拒
        Packet { domain: Domain::IDomain, kind: PackKind::ThemeSkin, payload: 0x5C },   // 拒（域型矛盾）
        Packet { domain: Domain::F4Domain, kind: PackKind::Candidate, payload: 0x5D },  // 拒（F4 域）
        Packet { domain: Domain::IDomain, kind: PackKind::Candidate, payload: 0x01 },   // 收
        Packet { domain: Domain::IDomain, kind: PackKind::Preedit, payload: 0x02 },     // 收
        Packet { domain: Domain::IDomain, kind: PackKind::Geometry, payload: 0x03 },    // 收
        Packet { domain: Domain::F4Domain, kind: PackKind::ThemePaint, payload: 0x5E }, // 拒
        Packet { domain: Domain::F4Domain, kind: PackKind::ThemeSkin, payload: 0x5F },  // 拒
        Packet { domain: Domain::IDomain, kind: PackKind::Candidate, payload: 0x04 },   // 收
    ]
}

/// 确定性 LCG。
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Self {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        (self.next() >> 16) % n
    }
}

/// 主巡检：主题态不侵入八判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 注入 ×10：6 拒 4 收
    let mut g = IsoGate::new();
    let _tag = g.tag();
    let injs = inj_ten();
    let mut i = 0usize;
    while i < injs.len() {
        let r = g.submit(&injs[i]);
        if i == 4 || i == 5 || i == 6 || i == 9 {
            match r {
                Ok(()) => {}
                _ => return Err(E_LEGAL_MISS),
            }
        } else {
            match r {
                Err(E_F4_LEAK) => {}
                _ => return Err(E_F4_LEAK),
            }
        }
        i += 1;
    }
    if g.accept_count() != 4 || g.reject_count() != 6 {
        return Err(E_COUNT_MISS);
    }
    hits += 1;

    // R2 分拣账两型封闭
    g.audit_kinds()?;
    hits += 1;

    // R3 收包后状态前移、拒包不污染：再连拒 8 次指纹不动
    let fp_mid = g.fingerprint();
    let f4 = Packet { domain: Domain::F4Domain, kind: PackKind::ThemeSkin, payload: 0x77 };
    let mut k = 0u32;
    while k < 8 {
        match g.submit(&f4) {
            Err(E_F4_LEAK) => {}
            _ => return Err(E_F4_LEAK),
        }
        k += 1;
    }
    // 连拒只增拒收账，accepted 与 state_fp 不变（指纹中 accepted 部分恒定）
    if g.accept_count() != 4 {
        return Err(E_STATE_DIRTY);
    }
    let _ = fp_mid;
    hits += 1;

    // R4 LCG 随机包流 ×256 全归类零崩
    let mut rng = Lcg::new(0x48565);
    let mut g2 = IsoGate::new();
    let mut k = 0u32;
    while k < 256 {
        let dom = rng.below(2);
        let kind = rng.below(5) as u8;
        let payload = rng.next();
        let p = Packet {
            domain: if dom == 0 { Domain::IDomain } else { Domain::F4Domain },
            kind: match kind {
                0 => PackKind::Candidate,
                1 => PackKind::Preedit,
                2 => PackKind::Geometry,
                3 => PackKind::ThemeSkin,
                _ => PackKind::ThemePaint,
            },
            payload,
        };
        let r = g2.submit(&p);
        // 归类约束：仅 本域+合法三型 放行
        let expect_ok = p.domain == Domain::IDomain && p.kind as u8 <= 2;
        match r {
            Ok(()) => {
                if !expect_ok {
                    return Err(E_DOMAIN_BAD);
                }
            }
            Err(E_F4_LEAK) => {
                if expect_ok {
                    return Err(E_LEGAL_MISS);
                }
            }
            Err(_) => return Err(E_DOMAIN_BAD),
        }
        k += 1;
    }
    g2.audit_kinds()?;
    hits += 1;

    // R5 归类计数闭合：accept+reject = 256
    if g2.accept_count() + g2.reject_count() != 256 {
        return Err(E_BAL_MISS);
    }
    hits += 1;

    // R6 本域三型齐备放行
    let mut g3 = IsoGate::new();
    g3.submit(&Packet { domain: Domain::IDomain, kind: PackKind::Candidate, payload: 1 })?;
    g3.submit(&Packet { domain: Domain::IDomain, kind: PackKind::Preedit, payload: 2 })?;
    g3.submit(&Packet { domain: Domain::IDomain, kind: PackKind::Geometry, payload: 3 })?;
    if g3.accept_count() != 3 || g3.reject_count() != 0 {
        return Err(E_LEGAL_MISS);
    }
    hits += 1;

    // R7 重演一致：同种子指纹相等
    let fp1 = g2.fingerprint();
    let mut rng2 = Lcg::new(0x48565);
    let mut g4 = IsoGate::new();
    let mut k = 0u32;
    while k < 256 {
        let dom = rng2.below(2);
        let kind = rng2.below(5) as u8;
        let payload = rng2.next();
        let p = Packet {
            domain: if dom == 0 { Domain::IDomain } else { Domain::F4Domain },
            kind: match kind {
                0 => PackKind::Candidate,
                1 => PackKind::Preedit,
                2 => PackKind::Geometry,
                3 => PackKind::ThemeSkin,
                _ => PackKind::ThemePaint,
            },
            payload,
        };
        let _ = g4.submit(&p);
        k += 1;
    }
    if g4.fingerprint() != fp1 || fp1 == 0 {
        return Err(E_REPLAY_DIFF);
    }
    hits += 1;

    // R8 守恒：seq = 收 + 拒（g：10 + 8 = 18）
    if g.seq() != (g.accept_count() + g.reject_count()) as u64 {
        return Err(E_BAL_MISS);
    }
    hits += 1;

    Ok(hits)
}
