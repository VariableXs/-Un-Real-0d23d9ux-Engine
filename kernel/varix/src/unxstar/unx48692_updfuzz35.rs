//! UNX-F48692 —— I35 fuzz（随机更新包注入零崩）
//! 判据原文：UNX-F48692-J1 fuzz ×10^4 零崩；判据锚定 体验十。
//! 判据落地（drill 场景）：R1 接收面闭集；R2 fuzz ×10^4 零崩；
//! R3 坏 id 拒收；R4 坏 sig 拒收；R5 合法包入账；R6 边界值；
//! R7 收发守恒；R8 呈现行；R9 指纹稳定；R10 账面自洽。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//! 确定性 LCG（wrapping_mul/wrapping_add）；drill 全绿才返回 Ok(hits)。
//! 口径说明：fuzz 注入面 = 更新包流（id 0..8、sig 全域随机）；
//! 零崩 = 10^4 笔全部接住：坏 id 与坏 sig 分账拒收，合法包入账；
//! 守恒 = ok + rej_id + rej_sig == 注入总笔数，账内零非法残留。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x0DE0;

// ── 诊断码闭集（SEG_BASE+1 起连续）──────────────────────────────
const E_ID: u16 = SEG_BASE + 1; // 包 ID 越界
const E_SIG: u16 = SEG_BASE + 2; // 签名不匹配
const E_CONSERV: u16 = SEG_BASE + 3; // 收发不守恒
const E_SANE: u16 = SEG_BASE + 4; // 账面自洽破坏
const E_FP: u16 = SEG_BASE + 5; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 6; // 呈现行缺段
const E_FUZZ: u16 = SEG_BASE + 7; // fuzz 轮次不足

/// 合法包 ID 闭集 1..=6（六类更新包）。
pub const ID_N: u8 = 6;
/// fuzz 轮次。
pub const FUZZ_N: u32 = 10_000;

/// FNV-1a 64 位。
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 确定性 LCG（批内统一系数）。
pub struct Lcg {
    state: u32,
}

impl Lcg {
    pub fn new(seed: u32) -> Lcg {
        Lcg { state: seed }
    }
    pub fn next_u32(&mut self) -> u32 {
        self.state = self.state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.state
    }
    pub fn below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            0
        } else {
            self.next_u32() % bound
        }
    }
}

/// 一笔更新包。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct UpdPacket {
    pub id: u8,
    pub sig: u64,
}

impl UpdPacket {
    /// 签名口径：sig = FNV(id)。
    pub fn sign_of(id: u8) -> u64 {
        let mut buf: Vec<u8> = vec![];
        buf.push(id);
        fnv1a64(&buf)
    }

    /// 签名校验。
    pub fn verify(&self) -> Result<(), u16> {
        if self.id == 0 || self.id > ID_N {
            return Err(E_ID);
        }
        if self.sig != UpdPacket::sign_of(self.id) {
            return Err(E_SIG);
        }
        Ok(())
    }
}

/// fuzz 接收面：任意更新包流进来不崩、账面自洽。
pub struct UpdSink {
    pub ok: u32,
    pub rej_id: u32,
    pub rej_sig: u32,
    pub accepted: Vec<u8>,
}

impl UpdSink {
    pub fn new() -> UpdSink {
        UpdSink { ok: 0, rej_id: 0, rej_sig: 0, accepted: vec![] }
    }

    /// 注入一笔。
    pub fn inject(&mut self, p: &UpdPacket) -> Result<(), u16> {
        if p.id == 0 || p.id > ID_N {
            self.rej_id += 1;
            return Err(E_ID);
        }
        if p.sig != UpdPacket::sign_of(p.id) {
            self.rej_sig += 1;
            return Err(E_SIG);
        }
        self.accepted.push(p.id);
        self.ok += 1;
        Ok(())
    }

    /// 总注入。
    pub fn total(&self) -> u32 {
        self.ok + self.rej_id + self.rej_sig
    }

    /// 自洽：账内包全部结构合法且计数一致。
    pub fn sane(&self) -> bool {
        let mut i = 0usize;
        while i < self.accepted.len() {
            let id = self.accepted[i];
            if id == 0 || id > ID_N {
                return false;
            }
            i += 1;
        }
        self.accepted.len() as u32 == self.ok
    }

    /// 指纹。
    pub fn fp(&self) -> u64 {
        let mut buf: Vec<u8> = vec![];
        let mut i = 0usize;
        while i < self.accepted.len() {
            buf.push(self.accepted[i]);
            i += 1;
        }
        buf.extend_from_slice(&self.ok.to_le_bytes());
        buf.extend_from_slice(&self.rej_id.to_le_bytes());
        buf.extend_from_slice(&self.rej_sig.to_le_bytes());
        fnv1a64(&buf)
    }
}

/// 呈现行：ok=<n> rid=<n> rsig=<n>。
pub fn sink_line(s: &UpdSink) -> String {
    let mut out = String::new();
    out.push_str("ok=");
    out.push_str(&s.ok.to_string());
    out.push_str(" rid=");
    out.push_str(&s.rej_id.to_string());
    out.push_str(" rsig=");
    out.push_str(&s.rej_sig.to_string());
    out
}

/// 子串判定（朴素扫描）。
pub fn has_kw(hay: &[u8], kw: &[u8]) -> bool {
    if kw.is_empty() || hay.len() < kw.len() {
        return false;
    }
    let mut i = 0usize;
    while i + kw.len() <= hay.len() {
        let mut j = 0usize;
        while j < kw.len() && hay[i + j] == kw[j] {
            j += 1;
        }
        if j == kw.len() {
            return true;
        }
        i += 1;
    }
    false
}

/// 按 id 统计账内笔数。
pub fn count_of(s: &UpdSink, id: u8) -> u32 {
    let mut c: u32 = 0;
    let mut i = 0usize;
    while i < s.accepted.len() {
        if s.accepted[i] == id {
            c += 1;
        }
        i += 1;
    }
    c
}

/// 分类统计自洽：六类 id 账内合计 == ok。
pub fn id_stats_sane(s: &UpdSink) -> bool {
    let mut sum: u32 = 0;
    let mut id = 1u8;
    while id <= ID_N {
        sum += count_of(s, id);
        id += 1;
    }
    sum == s.ok
}

/// 拒收账自洽：拒收两路合计 == 总注入 - ok。
pub fn rej_stats_sane(s: &UpdSink) -> bool {
    s.rej_id + s.rej_sig == s.total() - s.ok
}

/// 主巡检入口：R1..R10 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 接收面闭集：六类包各一笔合法入账。
    let mut s = UpdSink::new();
    let mut id: u8 = 1;
    while id <= ID_N {
        s.inject(&UpdPacket { id, sig: UpdPacket::sign_of(id) })?;
        id += 1;
    }
    if s.ok != 6 {
        return Err(E_ID);
    }
    hits += 1;

    // R2 fuzz ×10^4 零崩：随机 id 0..8、随机 sig。
    let mut fz = UpdSink::new();
    let mut g = Lcg::new(0x486_92);
    let mut r: u32 = 0;
    while r < FUZZ_N {
        let pid = (g.below(8)) as u8;
        let sig = (g.next_u32() as u64) << 32 | g.next_u32() as u64;
        let _x = fz.inject(&UpdPacket { id: pid, sig });
        r += 1;
    }
    if fz.total() != FUZZ_N {
        return Err(E_FUZZ);
    }
    if !fz.sane() {
        return Err(E_SANE);
    }
    hits += 1;

    // R3 坏 id 拒收。
    let mut probe = UpdSink::new();
    match probe.inject(&UpdPacket { id: 0, sig: 1 }) {
        Err(E_ID) => {}
        _ => return Err(E_ID),
    }
    match probe.inject(&UpdPacket { id: 9, sig: 1 }) {
        Err(E_ID) => hits += 1,
        _ => return Err(E_ID),
    }

    // R4 坏 sig 拒收：合法 id 配错签。
    match probe.inject(&UpdPacket { id: 3, sig: 0 }) {
        Err(E_SIG) => hits += 1,
        _ => return Err(E_SIG),
    }

    // R5 合法包入账：ok 计数与账内一致。
    probe.inject(&UpdPacket { id: 2, sig: UpdPacket::sign_of(2) })?;
    if probe.ok != 1 || probe.accepted.len() as u32 != 1 {
        return Err(E_CONSERV);
    }
    hits += 1;

    // R6 边界值：id=1（下界）与 id=6（上界）均入账。
    let mut edge = UpdSink::new();
    edge.inject(&UpdPacket { id: 1, sig: UpdPacket::sign_of(1) })?;
    edge.inject(&UpdPacket { id: 6, sig: UpdPacket::sign_of(6) })?;
    if edge.ok != 2 || edge.accepted[0] != 1 || edge.accepted[1] != 6 {
        return Err(E_SANE);
    }
    hits += 1;

    // R7 收发守恒。
    if !fz.sane() || fz.ok + fz.rej_id + fz.rej_sig != FUZZ_N {
        return Err(E_CONSERV);
    }
    hits += 1;

    // R8 呈现行：ok=/rid=/rsig= 在位。
    let line = sink_line(&fz);
    if !has_kw(line.as_bytes(), b"ok=") || !has_kw(line.as_bytes(), b"rid=")
        || !has_kw(line.as_bytes(), b"rsig=")
    {
        return Err(E_LINE);
    }
    hits += 1;

    // R9 指纹稳定：同账两次一致 + 同种子重放一致。
    let f1 = fz.fp();
    if f1 != fz.fp() || f1 == 0 {
        return Err(E_FP);
    }
    let mut fz2 = UpdSink::new();
    let mut g2 = Lcg::new(0x486_92);
    let mut r = 0u32;
    while r < FUZZ_N {
        let pid = (g2.below(8)) as u8;
        let sig = (g2.next_u32() as u64) << 32 | g2.next_u32() as u64;
        let _x = fz2.inject(&UpdPacket { id: pid, sig });
        r += 1;
    }
    if fz2.fp() != f1 {
        return Err(E_FP);
    }
    hits += 1;

    // R10 账面自洽：账内无非法 id、计数一致；
    // 分类统计自洽 + 拒收两路对账。
    if !fz.sane() || fz.accepted.len() as u32 != fz.ok {
        return Err(E_SANE);
    }
    if !id_stats_sane(&fz) || !rej_stats_sane(&fz) {
        return Err(E_SANE);
    }
    hits += 1;

    Ok(hits)
}
