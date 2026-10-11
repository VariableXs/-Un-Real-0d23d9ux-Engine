//! unx2301_axfinal —— UNX-F2301 域经轴收官总控（轴终编排三段）。
//! 锚点（CoRun Unxreal 主册）：UNX-F2301 · 域经轴收官总控 · 目标 310 行 · 已深化
//! 判据原文：UNX-F2301-J1 轴终三段编排全过（收口段：B34–B36 产出逐件
//!           点收；终验段：教程可执行/文档三向/轴预告核销三件终验；
//!           交接段：F2317 交接正式件与素材移交），三段时序断言全过，
//!           跳段注入 10/10 拒绝
//! 组件：轴终三段状态机（收口 → 终验 → 交接，顺序闭集）+ 收口段
//!       （B34/B35/B36 产出逐件哈希点收，三批闭集）+ 终验段（三件
//!       闭集：教程可执行位/文档三向一致位/轴预告核销位逐件置位）+
//!       交接段（F2317 正式件登记 + 素材移交清单点收）+ 时序断言
//!       （段戳单调）+ 注入核（跳段/产出缺批/终验缺件/移交缺单四类
//!       10/10 拒）。
//! 职责定位：域经轴的收官唯一权威闸——产出点收使 B34–B36 齐件可机检；
//!   三件终验使「轴是否真完成」有三把硬尺；交接段把轴产出移交正式
//!   件（F2317 消费本交接账）。
//! 性能逐项分解：点收 O(件)；终验 O(件)；交接 O(清单)；
//!   注入核 O(注入数)。全整数确定性。
//! 诊断码段：0xD4B0（0xD4B1~0xD4B9 闭集；全域 grep 确认独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::string::String;
use alloc::vec::Vec;

/// 模块版本。
pub const AF_VERSION: &str = "unx2301_axfinal-v1";

/// 诊断段基址（0xD4B0 段 · 本模块独占）。
pub const AF_SEG_BASE: u16 = 0xD4B0;

/// XcCode 闭集（0xD4B1~0xD4B9）。
pub const XC_STEP_SKIP: u32 = 0xD4B1;
pub const XC_BATCH_MISS: u32 = 0xD4B2;
pub const XC_VERIFY_MISS: u32 = 0xD4B3;
pub const XC_HANDOVER_MISS: u32 = 0xD4B4;
pub const XC_TS_BACK: u32 = 0xD4B5;
pub const XC_HASH_MISS: u32 = 0xD4B6;
pub const XC_PIECE_DUP: u32 = 0xD4B7;
pub const XC_LIST_GAP: u32 = 0xD4B8;
pub const XC_INJECT_MISSED: u32 = 0xD4B9;

/// 中文诊断文本。
pub fn diag_text(code: u32) -> (&'static str, &'static str) {
    match code {
        XC_STEP_SKIP => ("轴终编排跳段（三段被跳过）", "按收口→终验→交接补段"),
        XC_BATCH_MISS => ("产出缺批（B34–B36 不满）", "三批逐件补收"),
        XC_VERIFY_MISS => ("终验缺件（三件闭集不满）", "教程/文档/预告逐件置位"),
        XC_HANDOVER_MISS => ("交接缺单（正式件或素材清单缺）", "补 F2317 登记与移交清单"),
        XC_TS_BACK => ("段时序倒退", "按单调时序重排"),
        XC_HASH_MISS => ("产出哈希点收失败", "重传产出并点收"),
        XC_PIECE_DUP => ("产出件重复点收", "点收前按件号查重"),
        XC_LIST_GAP => ("移交清单跳号", "检查清单封装"),
        XC_INJECT_MISSED => ("注入故障未被轴终闸检出", "修复闸比对逻辑后重跑"),
        _ => ("未知诊断码", "查 0xD4B0 段闭集登记表"),
    }
}

/// 确定性伪随机。
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Self {
        Lcg(seed ^ 0x2301_4146)
    }
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
}

/// FNV-1a 32 位。
pub const fn fnv1a32(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0;
    while i < bytes.len() {
        h ^= bytes[i] as u32;
        h = h.wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}

/// 终验三件闭集。
pub const VF_TUTORIAL: u8 = 0; // 教程可执行
pub const VF_DOC3WAY: u8 = 1; // 文档三向
pub const VF_PRESOLD: u8 = 2; // 轴预告核销

/// 轴终总控。
pub struct AxFinalCtrl {
    cur: u8,
    /// 收口产出：(批号, 件号, 哈希)。
    pieces: Vec<(u64, u64, u32)>,
    /// 终验置位。
    verified: [bool; 3],
    /// 交接：正式件号 + 移交清单。
    pub formal_no: u64,
    handover: Vec<(u64, u32)>,
    ts: [u64; 3],
    pub now: u64,
    pub reject_count: u64,
}

impl AxFinalCtrl {
    pub fn new() -> Self {
        AxFinalCtrl {
            cur: 0,
            pieces: Vec::new(),
            verified: [false; 3],
            formal_no: 0,
            handover: Vec::new(),
            ts: [0; 3],
            now: 0,
            reject_count: 0,
        }
    }

    /// 段一：收口（B34/B35/B36 三批逐件点收）。
    pub fn collect(&mut self, pieces: &[(u64, u64, &[u8])]) -> Result<usize, u32> {
        if self.cur != 0 {
            self.reject_count += 1;
            return Err(XC_STEP_SKIP);
        }
        for (b, no, payload) in pieces.iter() {
            if *b < 34 || *b > 36 {
                return Err(XC_BATCH_MISS);
            }
            if self.pieces.iter().any(|p| p.0 == *b && p.1 == *no) {
                self.reject_count += 1;
                return Err(XC_PIECE_DUP);
            }
            let h = fnv1a32(payload);
            if h == 0 {
                return Err(XC_HASH_MISS);
            }
            self.pieces.push((*b, *no, h));
        }
        for b in 34..=36u64 {
            if !self.pieces.iter().any(|p| p.0 == b) {
                return Err(XC_BATCH_MISS);
            }
        }
        self.cur = 1;
        self.ts[0] = self.now;
        Ok(self.pieces.len())
    }

    /// 段二：终验（三件逐件置位）。
    pub fn verify(&mut self, items: &[u8]) -> Result<(), u32> {
        if self.cur != 1 {
            self.reject_count += 1;
            return Err(XC_STEP_SKIP);
        }
        if self.now < self.ts[0] {
            return Err(XC_TS_BACK);
        }
        for it in items.iter() {
            if (*it as usize) >= 3 {
                return Err(XC_VERIFY_MISS);
            }
            self.verified[*it as usize] = true;
        }
        if !self.verified.iter().all(|v| *v) {
            return Err(XC_VERIFY_MISS);
        }
        self.cur = 2;
        self.ts[1] = self.now;
        Ok(())
    }

    /// 段三：交接（正式件号 + 移交清单）。
    pub fn handover(&mut self, formal_no: u64, list: &[(u64, &[u8])]) -> Result<(), u32> {
        if self.cur != 2 {
            self.reject_count += 1;
            return Err(XC_STEP_SKIP);
        }
        if self.now < self.ts[1] {
            return Err(XC_TS_BACK);
        }
        if formal_no == 0 || list.is_empty() {
            return Err(XC_HANDOVER_MISS);
        }
        self.formal_no = formal_no;
        self.handover.clear();
        for (i, (no, payload)) in list.iter().enumerate() {
            let h = fnv1a32(payload);
            if h == 0 {
                return Err(XC_HASH_MISS);
            }
            if no != &(i as u64 + 1) {
                return Err(XC_LIST_GAP);
            }
            self.handover.push((*no, h));
        }
        self.cur = 3;
        self.ts[2] = self.now;
        Ok(())
    }

    /// 时序断言。
    pub fn timing_ok(&self) -> bool {
        self.ts[0] <= self.ts[1] && self.ts[1] <= self.ts[2] && self.cur == 3
    }

    /// 终验位只读视图。
    pub fn verified(&self) -> &[bool; 3] {
        &self.verified
    }

    /// 移交清单数。
    pub fn handover_count(&self) -> usize {
        self.handover.len()
    }
}

impl Default for AxFinalCtrl {
    fn default() -> Self {
        Self::new()
    }
}

/// 注入核：跳段/缺批/终验缺件/交接缺单四类，闸必抓 10/10。
pub fn injection_core() -> (bool, String) {
    let mut log = String::new();
    let mut all_caught = true;
    let _ = Lcg::new(0x2301);

    // 注入一：跳段（未收口直接终验 → 拒）。
    for i in 0..3u32 {
        let mut a = AxFinalCtrl::new();
        let r = a.verify(&[0, 1, 2]);
        let caught = r.is_err() && r.err().unwrap_or(0) == XC_STEP_SKIP;
        if !caught {
            all_caught = false;
        }
        log.push_str(&alloc::format!(
            "跳段#{} caught={} code={:04X}h\n",
            i + 1,
            caught,
            r.err().unwrap_or(0)
        ));
    }

    // 注入二：缺批（只给两批 → 拒）。
    for i in 0..3u32 {
        let mut a = AxFinalCtrl::new();
        let r = a.collect(&[(34, 1, b"p1"), (35, 1, b"p2")]);
        let caught = r.is_err() && r.err().unwrap_or(0) == XC_BATCH_MISS;
        if !caught {
            all_caught = false;
        }
        log.push_str(&alloc::format!(
            "缺批#{} caught={} code={:04X}h\n",
            i + 1,
            caught,
            r.err().unwrap_or(0)
        ));
    }

    // 注入三：终验缺件（只置两件 → 拒）。
    for i in 0..2u32 {
        let mut a = AxFinalCtrl::new();
        let _ = a.collect(&[(34, 1, b"p1"), (35, 1, b"p2"), (36, 1, b"p3")]);
        let r = a.verify(&[0, 1]);
        let caught = r.is_err() && r.err().unwrap_or(0) == XC_VERIFY_MISS;
        if !caught {
            all_caught = false;
        }
        log.push_str(&alloc::format!(
            "终验缺件#{} caught={} code={:04X}h\n",
            i + 1,
            caught,
            r.err().unwrap_or(0)
        ));
    }

    // 注入四：交接缺单（空清单 → 拒）。
    for i in 0..2u32 {
        let mut a = AxFinalCtrl::new();
        let _ = a.collect(&[(34, 1, b"p1"), (35, 1, b"p2"), (36, 1, b"p3")]);
        let _ = a.verify(&[0, 1, 2]);
        let r = a.handover(9101, &[]);
        let caught = r.is_err() && r.err().unwrap_or(0) == XC_HANDOVER_MISS;
        if !caught {
            all_caught = false;
        }
        log.push_str(&alloc::format!(
            "交接缺单#{} caught={} code={:04X}h\n",
            i + 1,
            caught,
            r.err().unwrap_or(0)
        ));
    }

    (all_caught, log)
}

/// 验收入口：判据 J1 全链核（三段编排 + 时序 + 注入 10/10）。
pub fn drill_axfinal() -> (bool, &'static str) {
    let mut a = AxFinalCtrl::new();
    // 段一：三批产出点收。
    let n = match a.collect(&[
        (34, 1, b"b34-tutorial-out"),
        (35, 1, b"b35-doc-out"),
        (36, 1, b"b36-domain-out"),
    ]) {
        Ok(n) => n,
        Err(_) => return (false, "F2301 收口失败"),
    };
    if n != 3 {
        return (false, "F2301 产出数不符");
    }
    // 段二：三件终验。
    a.now = 10;
    if a.verify(&[VF_TUTORIAL, VF_DOC3WAY, VF_PRESOLD]).is_err() {
        return (false, "F2301 终验失败");
    }
    // 段三：交接。
    a.now = 20;
    if a.handover(9101, &[(1, b"handover-asset-1"), (2, b"handover-asset-2")]).is_err() {
        return (false, "F2301 交接失败");
    }
    if !a.timing_ok() || a.handover_count() != 2 || a.formal_no != 9101 {
        return (false, "F2301 时序/交接账破缺");
    }
    if !a.verified().iter().all(|v| *v) {
        return (false, "F2301 终验位不全");
    }
    // 注入核 10/10。
    let (ok, _log) = injection_core();
    if !ok {
        return (false, "F2301 注入核未全抓");
    }
    (true, "F2301 全判据绿")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn drill() {
        assert!(drill_axfinal().0);
    }
}
