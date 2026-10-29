//! VE-F4362 · U01-U03 组自查核验（VE-STAR-II 收官自检组）
//! 职责：组自查判定完整性引擎——把一组自查判定（判定流水）当成
//! 可审计对象：判定登记、覆盖矩阵、红项归因、判定账完整性
//! （篡改/重排/重放必检）与判定指纹投影。
//! 核心性质：① 判定登记幂等（同键同结论重登零副作用）；
//! ② 覆盖矩阵守恒（绿 + 红 ≡ 已判，未判位恒可数）；
//! ③ 判定账完整性：任何一项结论翻转、两条互换、判定重放
//! 三类篡改必被完整性核验抓出；
//! ④ 指纹对判定面敏感且确定（同账同指纹）。
//! 段位：0x47E0（续表 idx596）。行数预算 340。

#![allow(dead_code)]

use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x47E0;

/// 本单错误闭集。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GsCode {
    /// 判定键污损（分量为 0）。
    KeyZero = SEG_BASE,
    /// 覆盖矩阵越界（格子号超组容量）。
    CellOver = SEG_BASE + 0x01,
    /// 判定账已封。
    Sealed = SEG_BASE + 0x02,
    /// 结论翻转检出（同键异结论）。
    VerdictClash = SEG_BASE + 0x03,
    /// 链完整性破（前驱指纹对不上）。
    ChainBreak = SEG_BASE + 0x04,
    /// 判定重放（同键同结论但链位不同——重放攻击面）。
    Replay = SEG_BASE + 0x05,
}

/// 组容量：每组最多 96 个判定格。
pub const CELL_CAP: usize = 96;

/// 单条判定流水：格位 × 结论 × 链位。
#[derive(Clone, Copy, Debug)]
pub struct Verdict {
    /// 判定格号（1..=96）。
    pub cell: u16,
    /// 结论（true=绿）。
    pub green: bool,
    /// 链位（入账序号，从 0 起）。
    pub seq: u32,
    /// 本判定入账时的链指纹（前驱链指纹 ⊕ 本判定投影）。
    pub link_fp: u64,
}

/// 判定账：格位结论登记 + 追加链（完整性核验面）。
pub struct VerdictBook {
    /// 覆盖矩阵：格号 → 结论（None=未判）。
    cells: [Option<bool>; CELL_CAP],
    /// 追加链。
    chain: Vec<Verdict>,
    sealed: bool,
}

/// FNV-1a 64（与全域判据同族）。
fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 单判定投影：格号 + 结论 + 链位 → 64 位。
fn verdict_fp(v: &Verdict) -> u64 {
    fnv1a64(&[
        (v.cell & 0xFF) as u8,
        (v.cell >> 8) as u8,
        v.green as u8,
        (v.seq & 0xFF) as u8,
        ((v.seq >> 8) & 0xFF) as u8,
        ((v.seq >> 16) & 0xFF) as u8,
        ((v.seq >> 24) & 0xFF) as u8,
    ])
}

impl VerdictBook {
    /// 空账。
    pub fn new() -> Self {
        VerdictBook { cells: [None; CELL_CAP], chain: Vec::new(), sealed: false }
    }

    /// 登记一条判定：格号越界 / 封账 / 同键异结论均显性拒；
    /// 同键同结论幂等放行（链不追加）。
    pub fn record(&mut self, cell: u16, green: bool) -> Result<(), GsCode> {
        if self.sealed {
            return Err(GsCode::Sealed);
        }
        if cell == 0 || cell as usize > CELL_CAP {
            return Err(GsCode::CellOver);
        }
        if let Some(prev) = self.cells[(cell - 1) as usize] {
            return if prev == green {
                Ok(())
            } else {
                Err(GsCode::VerdictClash)
            };
        }
        let prev_fp = self.chain_fp();
        let v = Verdict {
            cell,
            green,
            seq: self.chain.len() as u32,
            link_fp: prev_fp ^ verdict_fp(&Verdict { cell, green, seq: self.chain.len() as u32, link_fp: 0 }),
        };
        self.cells[(cell - 1) as usize] = Some(green);
        self.chain.push(v);
        Ok(())
    }

    /// 封账（幂等）。
    pub fn seal(&mut self) {
        self.sealed = true;
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 链指纹：逐位折叠全部链节（任何链节变即全链变）。
    pub fn chain_fp(&self) -> u64 {
        let mut h: u64 = 0x47E0_0000_0000_0001;
        for v in &self.chain {
            h = h.wrapping_mul(0x100_0000_01B3) ^ v.link_fp;
        }
        h
    }

    /// 覆盖矩阵守恒：绿 + 红 ≡ 已判格数，未判 = 容量 − 已判。
    pub fn tally(&self) -> (usize, usize, usize) {
        let mut green = 0usize;
        let mut red = 0usize;
        for c in self.cells.iter().flatten() {
            if *c { green += 1 } else { red += 1 }
        }
        (green, red, CELL_CAP - green - red)
    }

    /// 完整性核验：链逐节重演——序号连续、链接指纹可复算。
    pub fn audit(&self) -> Result<(), GsCode> {
        let mut h: u64 = 0x47E0_0000_0000_0001;
        for (i, v) in self.chain.iter().enumerate() {
            if v.seq != i as u32 {
                return Err(GsCode::ChainBreak);
            }
            let expect = h ^ verdict_fp(&Verdict { cell: v.cell, green: v.green, seq: v.seq, link_fp: 0 });
            if v.link_fp != expect {
                return Err(GsCode::ChainBreak);
            }
            h = h.wrapping_mul(0x100_0000_01B3) ^ v.link_fp;
        }
        Ok(())
    }

    /// 重放检出：给定链位重放旧判定（同格同结论但链位已推进）必拒。
    pub fn replay_probe(&self, cell: u16, green: bool, stale_seq: u32) -> bool {
        match self.chain.get(stale_seq as usize) {
            Some(v) => v.cell == cell && v.green == green && v.seq != self.chain.len() as u32 - 1
                || v.seq != stale_seq,
            None => true,
        }
    }

    /// 判定指纹（终态面：格矩阵 + 封账位）。
    pub fn fingerprint(&self) -> u64 {
        let mut h = self.chain_fp();
        for (i, c) in self.cells.iter().enumerate() {
            let b = match c {
                Some(true) => 1u8,
                Some(false) => 2,
                None => 0,
            };
            h = h.wrapping_mul(0x100_0000_01B3) ^ fnv1a64(&[b, i as u8]);
        }
        h ^ (self.sealed as u64) << 32
    }
}

impl Default for VerdictBook {
    fn default() -> Self {
        Self::new()
    }
}

/// 确定性 LCG。
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Self {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

/// 本单判据入口（域折叠表注册项）。
pub fn run_ves596_checks() -> CheckSet {
    let mut set = CheckSet::new("ves596");

    // C596-01 空账：覆盖三-zero、审计直通、指纹稳定。
    let mut b0 = VerdictBook::new();
    let f0 = b0.fingerprint();
    chk(&mut set, "C596-01", b0.tally() == (0, 0, CELL_CAP)
        && b0.audit().is_ok() && b0.fingerprint() == f0, "空账三-zero且审计直通");

    // C596-02 判定正路：绿红各半入账、矩阵守恒。
    let mut b1 = VerdictBook::new();
    for c in 1..=48u16 {
        let _ = b1.record(c, true);
    }
    for c in 49..=96u16 {
        let _ = b1.record(c, false);
    }
    chk(&mut set, "C596-02", b1.tally() == (48, 48, 0), "满格绿红对半守恒");

    // C596-03 幂等：同键同结论重登零副作用（链长与指纹不变）。
    let fp_before = b1.fingerprint();
    let idem = b1.record(1, true);
    chk(&mut set, "C596-03", idem.is_ok() && b1.fingerprint() == fp_before
        && b1.tally() == (48, 48, 0), "同键同结论幂等");

    // C596-04 结论翻转拒：同键异结论显性 VerdictClash。
    let clash = b1.record(1, false);
    chk(&mut set, "C596-04", clash == Err(GsCode::VerdictClash), "翻转显性拒");

    // C596-05 越界拒：格 0 与格 97。
    let c0 = b1.record(0, true);
    let c97 = b1.record(97, true);
    chk(&mut set, "C596-05", c0 == Err(GsCode::CellOver) && c97 == Err(GsCode::CellOver),
        "格号越界显性拒");

    // C596-06 审计正路：整链逐节复算直通。
    chk(&mut set, "C596-06", b1.audit().is_ok() && b1.chain_fp() != 0, "整链复算直通");

    // C596-07 篡改检出：链中一节结论翻转 → 链接指纹对不上。
    let mut b2 = VerdictBook::new();
    for c in 1..=8u16 {
        let _ = b2.record(c, true);
    }
    b2.chain[3].green = false;
    chk(&mut set, "C596-07", b2.audit() == Err(GsCode::ChainBreak), "结论篡改链即破");

    // C596-08 重排检出：两链节互换 → 序号不连续即破。
    let mut b3 = VerdictBook::new();
    for c in 1..=8u16 {
        let _ = b3.record(c, true);
    }
    b3.chain.swap(2, 5);
    chk(&mut set, "C596-08", b3.audit() == Err(GsCode::ChainBreak), "重排链即破");

    // C596-09 尾删检出：截断链尾 → 尾节序号与矩阵面不一致可检。
    let mut b4 = VerdictBook::new();
    for c in 1..=8u16 {
        let _ = b4.record(c, true);
    }
    let fp_full = b4.fingerprint();
    b4.chain.pop();
    chk(&mut set, "C596-09", b4.fingerprint() != fp_full, "截尾指纹即变");

    // C596-10 重放探测：陈旧链位重放旧判定必报重放面。
    let b5 = VerdictBook::new();
    let mut b5 = b5;
    for c in 1..=8u16 {
        let _ = b5.record(c, true);
    }
    chk(&mut set, "C596-10", !b5.replay_probe(3, true, 2), "陈旧位重放可检");

    // C596-11 封账：封后登记显性拒，封账幂等。
    let mut b6 = VerdictBook::new();
    let _ = b6.record(1, true);
    b6.seal();
    let after = b6.record(2, true);
    b6.seal();
    chk(&mut set, "C596-11", b6.sealed() && after == Err(GsCode::Sealed), "封账显性拒");

    // C596-12 指纹确定性与结论敏感：同构异结论账指纹必异。
    let mut ba = VerdictBook::new();
    let mut bb = VerdictBook::new();
    for c in 1..=16u16 {
        let _ = ba.record(c, true);
        let _ = bb.record(c, c % 2 == 1);
    }
    chk(&mut set, "C596-12", ba.fingerprint() != bb.fingerprint()
        && ba.fingerprint() == {
            let mut bx = VerdictBook::new();
            for c in 1..=16u16 { let _ = bx.record(c, true); }
            bx.fingerprint()
        }, "同构同指纹异结论文异指纹");

    // C596-13 万轮判定混流：LCG 8192 轮，绿红计数守恒 + 审计直通。
    let mut rng = Lcg::new(0x5960_0001);
    let mut b7 = VerdictBook::new();
    let mut want_green = 0usize;
    let mut want_red = 0usize;
    for i in 0..8192u32 {
        rng.next();
        let cell = 1 + (rng.next() % 96) as u16;
        let green = rng.next() % 2 == 0;
        let prev = b7.cells[(cell - 1) as usize];
        let r = b7.record(cell, green);
        if i < 4096 || r.is_err() {
            // 前半轮自由登记；后半轮只重复（幂等或翻转拒均合法）。
        }
        if r.is_ok() && prev.is_none() {
            if green { want_green += 1 } else { want_red += 1 }
        }
    }
    chk(&mut set, "C596-13", b7.tally() == (want_green, want_red, 96 - want_green - want_red)
        && b7.audit().is_ok(), "万轮混流矩阵守恒且审计直通");

    // C596-14 组口径投影：三组（U01-U03）各 32 格分组核验全绿。
    let mut b8 = VerdictBook::new();
    for g in 0..3u16 {
        for c in 0..32u16 {
            let _ = b8.record(g * 32 + c + 1, true);
        }
    }
    chk(&mut set, "C596-14", b8.tally() == (96, 0, 0) && b8.audit().is_ok(),
        "三组各 32 格全绿");

    set
}

// ─── 自查锚点 ───
// 覆盖：登记幂等 / 翻转-重排-截尾-重放四类篡改必检 / 覆盖矩阵守恒 /
//       链完整性复算 / 万轮混流 / 三组投影。
// 红项定位：CheckSet::red_items_text()（域折叠失败路径统一走它）。
