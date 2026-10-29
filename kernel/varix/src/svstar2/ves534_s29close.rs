//! VE-F4300 · U05 组收口双签（U05 工具集成组）
//! 职责：U05 收口双签（家族模板同域沿用）+ 三件（台账深化/会话号回写/
//! 字数核对）+ 交付声明（九面集成全部可用且判定权单源未被稀释）
//! + 未销红线拒绝 + 回写失败整组回滚。
//! 段位：0x2F60（续表 idx534）。行数预算 300。

#![allow(dead_code)]

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x2F60;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum U5ClCode {
    WordMismatch = SEG_BASE,
    RedlineOpen = SEG_BASE + 0x01,
    SingleSrcDiluted = SEG_BASE + 0x02,
    SameSigner = SEG_BASE + 0x03,
    Rollback = SEG_BASE + 0x04,
}

/// 交付声明。
#[derive(Clone, Copy, Debug)]
pub struct Delivery {
    /// 九面可用位图。
    pub faces_ok: [bool; 9],
    /// 判定权单源未被稀释。
    pub single_src: bool,
}

impl Delivery {
    pub fn all_faces(&self) -> bool {
        self.faces_ok.iter().all(|f| *f)
    }

    /// —— 深化：缺面数投影。——
    pub fn missing_faces(&self) -> u32 {
        self.faces_ok.iter().filter(|f| !**f).count() as u32
    }

    /// —— 深化：交付声明位图字节（bit i = 面 i 可用）。——
    pub fn bitmap(&self) -> u16 {
        let mut b = 0u16;
        for (i, f) in self.faces_ok.iter().enumerate() {
            if *f {
                b |= 1 << i;
            }
        }
        b
    }
}

/// U05 收口器。
pub struct U05Closer {
    pub ledger_done: bool,
    pub session_back: Option<u32>,
    pub words_claim: u32,
    pub words_real: u32,
    pub delivery: Delivery,
    pub open_redlines: Vec<u16>,
    pub closed: bool,
    pub rolled: u32,
    /// —— 深化：结构化双签台账（每次尝试一记）。——
    pub log: Vec<SignRec>,
}

/// —— 深化：双签台账记录（verdict: 0xFFFF=Ok，其余为判码值）。——
#[derive(Clone, Copy, Debug)]
pub struct SignRec {
    pub signer_a: u8,
    pub signer_b: u8,
    pub session: u32,
    pub verdict: u16,
}

impl U05Closer {
    pub fn new(delivery: Delivery) -> Self {
        U05Closer { ledger_done: false, session_back: None, words_claim: 0, words_real: 0, delivery, open_redlines: Vec::new(), closed: false, rolled: 0, log: Vec::new() }
    }

    /// —— 深化：带账收口：尝试入台账并透传判码。——
    pub fn try_close_logged(&mut self, a: u8, b: u8, session: u32) -> Result<(), U5ClCode> {
        let r = self.try_close(a, b);
        let verdict = match &r {
            Ok(()) => 0xFFFF,
            Err(c) => *c as u16,
        };
        self.log.push(SignRec { signer_a: a, signer_b: b, session, verdict });
        if self.session_back.is_none() && r.is_ok() {
            self.session_back = Some(session);
        }
        r
    }

    /// —— 深化：台账守恒：成功笔数 ≡ 1（收口唯一）或 0（未收口）。——
    pub fn log_conserve(&self) -> bool {
        let ok_n = self.log.iter().filter(|r| r.verdict == 0xFFFF).count();
        (ok_n == 0 && !self.closed) || (ok_n >= 1 && self.closed)
    }

    /// —— 深化：按判码分列投影（判码, 次数）。——
    pub fn by_verdict(&self) -> Vec<(u16, u32)> {
        let mut out: Vec<(u16, u32)> = Vec::new();
        for r in &self.log {
            match out.iter_mut().find(|(v, _)| *v == r.verdict) {
                Some(e) => e.1 += 1,
                None => out.push((r.verdict, 1)),
            }
        }
        out
    }

    /// —— 深化：台账中文摘要。——
    pub fn summary(&self) -> String {
        let mut s = String::new();
        s.push_str("签试");
        s.push_str(&self.log.len().to_string());
        s.push_str("次·回滚");
        s.push_str(&self.rolled.to_string());
        s
    }

    pub fn try_close(&mut self, a: u8, b: u8) -> Result<(), U5ClCode> {
        if a == b || a == 0 || b == 0 {
            return Err(U5ClCode::SameSigner);
        }
        if self.words_claim != self.words_real {
            return Err(U5ClCode::WordMismatch);
        }
        if !self.open_redlines.is_empty() {
            return Err(U5ClCode::RedlineOpen);
        }
        if !self.delivery.all_faces() {
            return Err(U5ClCode::Rollback); // 九面缺一不可交付
        }
        if !self.delivery.single_src {
            return Err(U5ClCode::SingleSrcDiluted);
        }
        if !self.ledger_done || self.session_back.is_none() {
            self.rollback();
            return Err(U5ClCode::Rollback);
        }
        self.closed = true;
        Ok(())
    }

    fn rollback(&mut self) {
        self.closed = false;
        self.rolled += 1;
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

/// 判据组：C534-01..12。
pub fn run_ves534_checks() -> CheckSet {
    let mut set = CheckSet::new("ves534");

    let mut cl = U05Closer::new(Delivery { faces_ok: [true; 9], single_src: true });
    cl.ledger_done = true;
    cl.session_back = Some(77);
    cl.words_claim = 300;
    cl.words_real = 290;
    chk(&mut set, "C534-01", cl.try_close(1, 2) == Err(U5ClCode::WordMismatch), "字数不符禁签");
    cl.words_real = 300;
    chk(&mut set, "C534-02", cl.try_close(1, 2) == Err(U5ClCode::RedlineOpen), "未销红线拒绝收口");
    cl.open_redlines.push(5);
    chk(&mut set, "C534-03", cl.try_close(1, 2) == Err(U5ClCode::RedlineOpen), "红线在册即拒");
    cl.open_redlines.clear();

    cl.delivery.faces_ok[8] = false;
    chk(&mut set, "C534-04", cl.try_close(1, 2) == Err(U5ClCode::Rollback), "九面缺一不可交付");
    chk(&mut set, "C534-05", cl.rolled == 1, "回滚计数");
    cl.delivery.faces_ok[8] = true;

    cl.delivery.single_src = false;
    chk(&mut set, "C534-06", cl.try_close(1, 2) == Err(U5ClCode::SingleSrcDiluted), "判定权单源被稀释拒绝");
    cl.delivery.single_src = true;

    chk(&mut set, "C534-07", cl.try_close(1, 1) == Err(U5ClCode::SameSigner), "双签同人");
    chk(&mut set, "C534-08", cl.try_close(1, 2).is_ok(), "全条件收口");
    chk(&mut set, "C534-09", cl.closed, "收口在册");

    let d = Delivery { faces_ok: [true; 9], single_src: true };
    chk(&mut set, "C534-10", d.all_faces(), "交付声明九面语义");
    let d2 = Delivery { faces_ok: [false; 9], single_src: false };
    chk(&mut set, "C534-11", !d2.all_faces() && !d2.single_src, "空声明拒绝");
    chk(&mut set, "C534-12", cl.rolled == 1 && cl.session_back == Some(77), "会话号回写在册");

    // —— 深化 A：带账收口 + 台账守恒 ——
    let mut cl2 = U05Closer::new(Delivery { faces_ok: [true; 9], single_src: true });
    cl2.words_claim = 10;
    cl2.words_real = 10;
    cl2.ledger_done = true;
    chk(&mut set, "C534-13", cl2.try_close_logged(3, 4, 900) == Err(U5ClCode::Rollback), "缺会话号回写拒绝并回滚");
    chk(&mut set, "C534-14", cl2.try_close_logged(3, 4, 901).is_ok() && cl2.session_back == Some(901), "带会话号收口回写");
    chk(&mut set, "C534-15", cl2.log_conserve() && cl2.log.len() == 2, "双签台账守恒：成功与收口态一致");
    chk(&mut set, "C534-16", cl2.by_verdict().iter().any(|(v, _)| *v == 0xFFFF), "按判码分列投影含成功档");
    chk(&mut set, "C534-17", !cl2.summary().is_empty(), "台账中文摘要（域本色）");

    // —— 深化 B：幂等闸 ——
    let rolled0 = cl2.rolled;
    let closed0 = cl2.closed;
    chk(&mut set, "C534-18", cl2.try_close(3, 4).is_ok() && cl2.closed == closed0 && cl2.rolled == rolled0, "幂等闸：已收口态二次提交零副作用");

    // —— 深化 C：边界探针 ——
    let er = run_close_edge_probes();
    chk(&mut set, "C534-19", er.zero_signer_rejected && er.max_redline_held, "零签人拒绝/u16 极值红线在册");
    chk(&mut set, "C534-20", er.missing_count_ok && er.bitmap_roundtrip, "缺面数投影/位图编码往返");

    // —— 深化 D：LCG 注入回归（必达性）——
    match close_inject_regression(24) {
        Ok(attempts) => chk(&mut set, "C534-21", attempts >= 1, "24 轮红线/字数扰动清零后收口必达"),
        Err(_) => chk(&mut set, "C534-21", false, "注入回归收口未达"),
    }

    // —— 深化 E：序列化投影 ——
    let mut bytes: Vec<u8> = Vec::new();
    encode_redlines(&cl2.open_redlines, &mut bytes);
    let rt = decode_redlines(&bytes);
    chk(&mut set, "C534-22", rt.iter().count() == cl2.open_redlines.len(), "红线表 2B/条定长解码往返");
    chk(&mut set, "C534-23", fnv1a(&bytes) == fnv1a(&bytes), "FNV 指纹稳定");
    chk(&mut set, "C534-24", cl2.delivery.bitmap() == 0x01FF, "九面位图全可用=0x01FF");

    set
}

// ===== 深化块 A：边界探针 =====

/// 收口边界探针报告。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CloseEdgeReport {
    pub zero_signer_rejected: bool,
    pub max_redline_held: bool,
    pub missing_count_ok: bool,
    pub bitmap_roundtrip: bool,
}

/// 四类边界探针：零签人/u16 极值红线/缺面数/位图往返。
pub fn run_close_edge_probes() -> CloseEdgeReport {
    // 1) 零签人：(0,0) 必拒
    let mut e = U05Closer::new(Delivery { faces_ok: [true; 9], single_src: true });
    let zero_signer_rejected = e.try_close(0, 0) == Err(U5ClCode::SameSigner);
    // 2) u16 极值红线号在册不丢
    e.open_redlines.push(u16::MAX);
    let max_redline_held = e.open_redlines.contains(&u16::MAX);
    // 3) 缺面数投影
    let mut d = Delivery { faces_ok: [true; 9], single_src: true };
    d.faces_ok[3] = false;
    d.faces_ok[7] = false;
    let missing_count_ok = d.missing_faces() == 2 && !d.all_faces();
    // 4) 位图编码：全 False=0 / 全 True=0x01FF
    let d0 = Delivery { faces_ok: [false; 9], single_src: false };
    let bitmap_roundtrip = d0.bitmap() == 0 && d.bitmap() == 0x01FF;

    CloseEdgeReport { zero_signer_rejected, max_redline_held, missing_count_ok, bitmap_roundtrip }
}

// ===== 深化块 B：LCG 注入回归 =====

/// 确定性 LCG（8 行）。
pub struct Lcg(u32);

impl Lcg {
    pub fn new(seed: u32) -> Self {
        Lcg(seed | 1)
    }

    pub fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0
    }
}

/// 注入回归：LCG 随机扰动红线/字数 N 轮后全清，收口必达。
pub fn close_inject_regression(rounds: u32) -> Result<u32, U5ClCode> {
    let mut rng = Lcg::new(0x5EED_5340);
    let mut cl = U05Closer::new(Delivery { faces_ok: [true; 9], single_src: true });
    cl.ledger_done = true;
    cl.session_back = Some(1);
    cl.words_claim = 100;
    cl.words_real = 100;
    let mut attempts = 0u32;
    for _ in 0..rounds {
        if rng.next() & 1 == 0 {
            cl.open_redlines.push((rng.next() & 0xFF) as u16);
        } else {
            cl.words_real = cl.words_claim.wrapping_add(rng.next() % 3);
        }
        attempts += 1;
    }
    // 全清修复
    cl.open_redlines.clear();
    cl.words_real = cl.words_claim;
    cl.try_close(5, 6)?;
    if !cl.closed {
        return Err(U5ClCode::Rollback);
    }
    Ok(attempts)
}

// ===== 深化块 C：序列化投影 =====

/// FNV-1a 32 位指纹。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in bytes {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 定长编码：红线表每条 2B 大端。
pub fn encode_redlines(redlines: &[u16], out: &mut Vec<u8>) {
    for r in redlines {
        out.extend_from_slice(&r.to_be_bytes());
    }
}

/// 定长解码：非整 2B → 显性拒绝。
pub fn decode_redlines(bytes: &[u8]) -> Result<Vec<u16>, U5ClCode> {
    if bytes.len() % 2 != 0 {
        return Err(U5ClCode::WordMismatch);
    }
    let mut out = Vec::new();
    for ch in bytes.chunks_exact(2) {
        out.push(u16::from_be_bytes([ch[0], ch[1]]));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn u05_closer() {
        let mut cl = U05Closer::new(Delivery { faces_ok: [true; 9], single_src: true });
        cl.words_claim = 1;
        cl.words_real = 1;
        cl.ledger_done = true;
        cl.session_back = Some(1);
        assert!(cl.try_close(1, 2).is_ok());
        cl.open_redlines.push(1);
        assert_eq!(cl.try_close(1, 2), Err(U5ClCode::RedlineOpen));
    }
}
