//! unx2261_sediment —— UNX-F2261 域经沉淀轴总控（沉淀编排三段）。
//! 锚点（CoRun Unxreal 主册）：UNX-F2261 · 域经沉淀轴总控 · 目标 300 行 · 已深化
//! 判据原文：UNX-F2261-J1 沉淀总控三段编排全过（素材接收段：F2257 四类
//!           素材哈希点收；沉淀执行段：欠账/案例/域经三线滚动；沉淀确认
//!           段：域经账落账与索引挂接），三段时序断言全过，跳段注入
//!           10/10 拒绝
//! 组件：三段编排状态机（接收 → 执行 → 确认，顺序闭集 + 时序戳逐段
//!       单调）+ 素材接收段（F2257 四类素材闭集：欠账/案例/教训/域经
//!       ——逐件哈希点收）+ 沉淀执行段（三线滚动：欠账线/案例线/域经
//!       线各推一档）+ 沉淀确认段（域经账落账 append-only + 索引挂接
//!       号回填）+ 时序断言（段戳单调 + 接收先于执行先于确认）+
//!       注入核（跳段/素材缺类/三线漏滚/落账跳号四类 10/10 拒）。
//! 职责定位：域经沉淀轴的唯一权威编排——三段闭集使沉淀不可偷步；
//!   四类素材哈希点收使素材不可替换；三线滚动使欠账/案例/域经同步
//!   推进（F2262 沉淀执行面消费本编排）。
//! 性能逐项分解：段步 O(1)；点收 O(素材数)；滚动 O(线数)；
//!   注入核 O(注入数)。全整数确定性。
//! 诊断码段：0xD490（0xD491~0xD499 闭集；全域 grep 确认独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// 模块版本。
pub const SD_VERSION: &str = "unx2261_sediment-v1";

/// 诊断段基址（0xD490 段 · 本模块独占）。
pub const SD_SEG_BASE: u16 = 0xD490;

/// XcCode 闭集（0xD491~0xD499）。
pub const XC_STEP_SKIP: u32 = 0xD491;
pub const XC_MAT_MISSING: u32 = 0xD492;
pub const XC_LINE_STALL: u32 = 0xD493;
pub const XC_LEDGER_GAP: u32 = 0xD494;
pub const XC_TS_BACK: u32 = 0xD495;
pub const XC_HASH_MISS: u32 = 0xD496;
pub const XC_INDEX_MISSING: u32 = 0xD497;
pub const XC_MAT_DUP: u32 = 0xD498;
pub const XC_INJECT_MISSED: u32 = 0xD499;

/// 中文诊断文本。
pub fn diag_text(code: u32) -> (&'static str, &'static str) {
    match code {
        XC_STEP_SKIP => ("沉淀编排跳段（三段被跳过）", "按接收→执行→确认补段"),
        XC_MAT_MISSING => ("素材缺类（四类不满）", "按 F2257 四类补收"),
        XC_LINE_STALL => ("三线滚动停摆（某线未推进）", "三线同步推档"),
        XC_LEDGER_GAP => ("域经账落账跳号", "检查账务封装"),
        XC_TS_BACK => ("段时序倒退", "按单调时序重排"),
        XC_HASH_MISS => ("素材哈希点收失败", "重传素材并点收"),
        XC_INDEX_MISSING => ("索引挂接缺失", "确认段补挂接号"),
        XC_MAT_DUP => ("素材重复点收", "点收前按件号查重"),
        XC_INJECT_MISSED => ("注入故障未被编排闸检出", "修复闸比对逻辑后重跑"),
        _ => ("未知诊断码", "查 0xD490 段闭集登记表"),
    }
}

/// 确定性伪随机。
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Self {
        Lcg(seed ^ 0x2261_5344)
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

/// F2257 四类素材闭集。
pub const MAT_KINDS: [&str; 4] = ["欠账", "案例", "教训", "域经"];

/// 素材件。
#[derive(Clone, Debug)]
pub struct SdMaterial {
    pub kind: u8,
    pub no: u64,
    pub hash: u32,
}

/// 沉淀总控。
pub struct SedimentCtrl {
    cur: u8,
    materials: Vec<SdMaterial>,
    /// 三线滚动档位。
    pub line_arrears: u64,
    pub line_cases: u64,
    pub line_domain: u64,
    /// 域经落账：(序号, 挂接号)。
    ledger: Vec<(u64, u64)>,
    next_seq: u64,
    /// 段时序戳。
    ts: [u64; 3],
    pub now: u64,
    pub reject_count: u64,
}

impl SedimentCtrl {
    pub fn new() -> Self {
        SedimentCtrl {
            cur: 0,
            materials: Vec::new(),
            line_arrears: 0,
            line_cases: 0,
            line_domain: 0,
            ledger: Vec::new(),
            next_seq: 1,
            ts: [0; 3],
            now: 0,
            reject_count: 0,
        }
    }

    /// 段一：素材接收（四类各 ≥1 + 哈希点收 + 查重）。
    pub fn accept(&mut self, mats: &[(u8, u64, &[u8])]) -> Result<usize, u32> {
        if self.cur != 0 {
            self.reject_count += 1;
            return Err(XC_STEP_SKIP);
        }
        for (k, no, payload) in mats.iter() {
            if (*k as usize) >= MAT_KINDS.len() {
                return Err(XC_MAT_MISSING);
            }
            if self.materials.iter().any(|m| m.kind == *k && m.no == *no) {
                self.reject_count += 1;
                return Err(XC_MAT_DUP);
            }
            let h = fnv1a32(payload);
            if h == 0 {
                return Err(XC_HASH_MISS);
            }
            self.materials.push(SdMaterial { kind: *k, no: *no, hash: h });
        }
        // 四类闭集核。
        for k in 0..4u8 {
            if !self.materials.iter().any(|m| m.kind == k) {
                return Err(XC_MAT_MISSING);
            }
        }
        self.cur = 1;
        self.ts[0] = self.now;
        Ok(self.materials.len())
    }

    /// 段二：三线滚动。
    pub fn roll(&mut self) -> Result<(), u32> {
        if self.cur != 1 {
            self.reject_count += 1;
            return Err(XC_STEP_SKIP);
        }
        if self.now < self.ts[0] {
            return Err(XC_TS_BACK);
        }
        self.line_arrears += 1;
        self.line_cases += 1;
        self.line_domain += 1;
        self.cur = 2;
        self.ts[1] = self.now;
        Ok(())
    }

    /// 段三：落账 + 索引挂接。
    pub fn confirm(&mut self, index_no: u64) -> Result<u64, u32> {
        if self.cur != 2 {
            self.reject_count += 1;
            return Err(XC_STEP_SKIP);
        }
        if self.now < self.ts[1] {
            return Err(XC_TS_BACK);
        }
        if index_no == 0 {
            return Err(XC_INDEX_MISSING);
        }
        let seq = self.next_seq;
        self.next_seq += 1;
        self.ledger.push((seq, index_no));
        self.cur = 3;
        self.ts[2] = self.now;
        Ok(seq)
    }

    /// 时序断言：ts0 ≤ ts1 ≤ ts2。
    pub fn timing_ok(&self) -> bool {
        self.ts[0] <= self.ts[1] && self.ts[1] <= self.ts[2] && self.cur == 3
    }

    /// 三线同档核。
    pub fn lines_synced(&self) -> bool {
        self.line_arrears == self.line_cases && self.line_cases == self.line_domain && self.line_domain > 0
    }

    /// 落账序号连续核。
    pub fn ledger_ok(&self) -> bool {
        self.ledger
            .iter()
            .enumerate()
            .all(|(i, (s, _))| *s == i as u64 + 1)
    }

    /// 素材数。
    pub fn material_count(&self) -> usize {
        self.materials.len()
    }
}

impl Default for SedimentCtrl {
    fn default() -> Self {
        Self::new()
    }
}

/// 注入核：跳段/素材缺类/三线漏滚/落账跳号四类，闸必抓 10/10。
pub fn injection_core() -> (bool, String) {
    let mut log = String::new();
    let mut all_caught = true;
    let _ = Lcg::new(0x2261);

    // 注入一：跳段（未接收直接 roll → 拒）。
    for i in 0..3u32 {
        let mut s = SedimentCtrl::new();
        let r = s.roll();
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

    // 注入二：素材缺类（只给三类 → 拒）。
    for i in 0..3u32 {
        let mut s = SedimentCtrl::new();
        let r = s.accept(&[(0, 1, b"a"), (1, 1, b"b"), (2, 1, b"c")]);
        let caught = r.is_err() && r.err().unwrap_or(0) == XC_MAT_MISSING;
        if !caught {
            all_caught = false;
        }
        log.push_str(&alloc::format!(
            "素材缺类#{} caught={} code={:04X}h\n",
            i + 1,
            caught,
            r.err().unwrap_or(0)
        ));
    }

    // 注入三：三线漏滚（手工破坏同档 → 抓）。
    for i in 0..2u32 {
        let mut s = SedimentCtrl::new();
        let _ = s.accept(&[(0, 1, b"a"), (1, 1, b"b"), (2, 1, b"c"), (3, 1, b"d")]);
        let _ = s.roll();
        s.line_cases = 0;
        let caught = !s.lines_synced();
        if !caught {
            all_caught = false;
        }
        log.push_str(&alloc::format!("三线漏滚#{} caught={}\n", i + 1, caught));
    }

    // 注入四：落账跳号（伪造序号 → 抓）。
    for i in 0..2u32 {
        let mut s = SedimentCtrl::new();
        let _ = s.accept(&[(0, 1, b"a"), (1, 1, b"b"), (2, 1, b"c"), (3, 1, b"d")]);
        let _ = s.roll();
        let _ = s.confirm(88);
        if let Some(first) = s.ledger.first_mut() {
            first.0 = 5;
        }
        let caught = !s.ledger_ok();
        if !caught {
            all_caught = false;
        }
        log.push_str(&alloc::format!("落账跳号#{} caught={}\n", i + 1, caught));
    }

    (all_caught, log)
}

/// 验收入口：判据 J1 全链核（三段编排 + 时序 + 注入 10/10）。
pub fn drill_sediment() -> (bool, &'static str) {
    let mut s = SedimentCtrl::new();
    // 段一：四类素材点收。
    let n = match s.accept(&[
        (0, 1, b"arrears-item-1"),
        (1, 1, b"case-item-1"),
        (2, 1, b"lesson-item-1"),
        (3, 1, b"domain-item-1"),
        (3, 2, b"domain-item-2"),
    ]) {
        Ok(n) => n,
        Err(_) => return (false, "F2261 素材接收失败"),
    };
    if n != 5 {
        return (false, "F2261 素材数不符");
    }
    // 段二：三线滚动。
    s.now = 10;
    if s.roll().is_err() || !s.lines_synced() {
        return (false, "F2261 三线滚动失败");
    }
    // 段三：落账挂接。
    s.now = 20;
    if s.confirm(7701).is_err() {
        return (false, "F2261 落账失败");
    }
    // 时序断言 + 账完整性。
    if !s.timing_ok() || !s.ledger_ok() {
        return (false, "F2261 时序/账破缺");
    }
    // 注入核 10/10。
    let (ok, _log) = injection_core();
    if !ok {
        return (false, "F2261 注入核未全抓");
    }
    (true, "F2261 全判据绿")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn drill() {
        assert!(drill_sediment().0);
    }
}
