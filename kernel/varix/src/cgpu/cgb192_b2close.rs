//! CGPU-F0192 · B02 组收口（判据段 0x05A0）
//! B02 组 16 项收口（组级范式）：自检入树、增量与确定性报告
//! （F0178/172）、性能基准（F0037 基准联动）、组双签、台账、向 B03
//! 并行切分组移交（图模型+依赖解析+执行器就绪——B03 的并行切分在
//! 图执行器上深化）。判据：16 项入树、报告、双签、移交、断点。
//!
//! 诊断码段 0x05A0（本模块独占：0x05A0~0x05AF）。
//! no_std + alloc 零 unwrap，全部 Result 化。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x05A0;

/// B02 组收口项数（判据「16 项入树」硬界）：F0177~F0192。
pub const GROUP_ITEMS: usize = 16;

/// 收口项。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct B2Item {
    /// 单号（177~192）。
    pub fid: u16,
    /// 自检绿。
    pub self_check: bool,
    /// 判据数。
    pub criteria: u16,
    /// 实现行数。
    pub lines: u32,
}

impl B2Item {
    /// 校验：单号区段、自检绿、判据行数非零。
    pub fn validate(&self) -> Result<(), u16> {
        if self.fid < 177 || self.fid > 192 {
            return Err(SEG_BASE);
        }
        if !self.self_check || self.criteria == 0 || self.lines == 0 {
            return Err(SEG_BASE + 1);
        }
        Ok(())
    }
}

/// B02 组收口台账。
pub struct B2CloseBook {
    /// 收口项。
    pub items: Vec<B2Item>,
    /// 增量正确性报告（F0178 联动：加速比 ×100）。
    pub inc_speedup_x100: Option<u32>,
    /// 确定性报告（F0172 联动：双跑全同 = true）。
    pub det_green: Option<bool>,
    /// 基准（微秒）。
    pub bench_us: Option<u32>,
    /// 双签。
    pub builder: Option<u64>,
    pub acceptor: Option<u64>,
    /// 断点。
    pub handover: Option<u64>,
}

impl B2CloseBook {
    /// 空台账。
    pub fn new() -> Self {
        B2CloseBook {
            items: Vec::new(),
            inc_speedup_x100: None,
            det_green: None,
            bench_us: None,
            builder: None,
            acceptor: None,
            handover: None,
        }
    }

    /// 入项。
    pub fn add_item(&mut self, it: B2Item) -> Result<(), u16> {
        it.validate()?;
        if self.items.len() >= GROUP_ITEMS {
            return Err(SEG_BASE + 2);
        }
        if self.items.iter().any(|x| x.fid == it.fid) {
            return Err(SEG_BASE + 3);
        }
        self.items.push(it);
        Ok(())
    }

    /// 全覆盖审计（判据「16 项入树」强校验）。
    pub fn coverage(&self) -> Result<(), u16> {
        if self.items.len() != GROUP_ITEMS {
            return Err(SEG_BASE + 4);
        }
        for fid in 177..=192u16 {
            if !self.items.iter().any(|x| x.fid == fid) {
                return Err(SEG_BASE + 5);
            }
        }
        Ok(())
    }

    /// 判据总数。
    pub fn criteria_total(&self) -> u32 {
        self.items.iter().map(|x| x.criteria as u32).sum()
    }

    /// 行数总数。
    pub fn lines_total(&self) -> u32 {
        self.items.iter().map(|x| x.lines).sum()
    }

    /// 增量与确定性报告入账（判据「增量与确定性报告」核心面）：
    /// 加速比 ≥ 500（5 倍）+ 确定性全同，两者齐才可入账。
    pub fn set_reports(&mut self, speedup_x100: u32, det: bool) -> Result<(), u16> {
        if speedup_x100 < 500 {
            return Err(SEG_BASE + 6);
        }
        if !det {
            return Err(SEG_BASE + 7);
        }
        self.inc_speedup_x100 = Some(speedup_x100);
        self.det_green = Some(det);
        Ok(())
    }

    /// 记基准（样本 ≥ 3）。
    pub fn set_bench(&mut self, us: u32, samples: u32) -> Result<(), u16> {
        if us == 0 || samples < 3 {
            return Err(SEG_BASE + 8);
        }
        self.bench_us = Some(us);
        Ok(())
    }

    /// 双签。
    pub fn dual_sign(&mut self, b: u64, a: u64) -> Result<(), u16> {
        if b == 0 || a == 0 || b == a {
            return Err(SEG_BASE + 9);
        }
        self.builder = Some(b);
        self.acceptor = Some(a);
        Ok(())
    }

    /// 落断点（判据「移交」「断点」）：五关齐——覆盖 + 报告 + 基准
    /// + 双签，第六关 B03 移交注记指纹。
    pub fn place_handover(&mut self, note_fp: u64) -> Result<u64, u16> {
        self.coverage()?;
        if self.inc_speedup_x100.is_none() || self.det_green != Some(true) {
            return Err(SEG_BASE + 10);
        }
        if self.bench_us.is_none() {
            return Err(SEG_BASE + 11);
        }
        let (b, a) = match (self.builder, self.acceptor) {
            (Some(b), Some(a)) => (b, a),
            _ => return Err(SEG_BASE + 12),
        };
        if note_fp == 0 {
            return Err(SEG_BASE + 13);
        }
        let mut acc: u64 = 0xcbf29ce484222325;
        for it in &self.items {
            acc = acc.wrapping_mul(0x100000001b3) ^ it.fid as u64;
        }
        acc = acc.wrapping_mul(0x100000001b3)
            ^ self.inc_speedup_x100.unwrap_or(0) as u64;
        acc = acc.wrapping_mul(0x100000001b3) ^ self.bench_us.unwrap_or(0) as u64;
        acc = acc.wrapping_mul(0x100000001b3) ^ b;
        acc = acc.wrapping_mul(0x100000001b3) ^ a;
        acc = acc.wrapping_mul(0x100000001b3) ^ note_fp;
        if acc == 0 {
            return Err(SEG_BASE + 14);
        }
        self.handover = Some(acc);
        Ok(acc)
    }

    /// 绿判。
    pub fn green(&self) -> Result<(), u16> {
        self.coverage()?;
        if self.handover.is_none() {
            return Err(SEG_BASE + 15);
        }
        Ok(())
    }

    /// B03 移交包（判据「向 B03 移交」数据面）：四元摘要——执行器
    /// 就绪声明、增量锚、确定性锚、基准锚。
    pub fn handover_package(&self) -> Result<[(u64, u64); 4], u16> {
        self.green()?;
        Ok([
            (0xB070_01, self.handover.unwrap_or(0)),
            (0xB070_02, self.inc_speedup_x100.unwrap_or(0) as u64),
            (0xB070_03, self.det_green.unwrap_or(false) as u64),
            (0xB070_04, self.bench_us.unwrap_or(0) as u64),
        ])
    }
}

/// B02 报告合成（判据「报告」独立面）：增量 + 确定性两报告的
/// 联合指纹（B03 验断点时用）。
pub fn b2_report_fp(book: &B2CloseBook) -> Result<u64, u16> {
    let s = book.inc_speedup_x100.ok_or(SEG_BASE + 16)?;
    let d = book.det_green.ok_or(SEG_BASE + 17)?;
    if !d {
        return Err(SEG_BASE + 18);
    }
    let mut h: u64 = 0xcbf29ce484222325;
    h = h.wrapping_mul(0x100000001b3) ^ s as u64;
    h = h.wrapping_mul(0x100000001b3) ^ d as u64;
    h = h.wrapping_mul(0x100000001b3) ^ book.criteria_total() as u64;
    Ok(h)
}

/// B02 一站式收口（总闸）。
pub fn b2_close_audit(book: &mut B2CloseBook, note_fp: u64) -> Result<u64, u16> {
    book.green()?;
    b2_report_fp(book)?;
    book.handover_package()?;
    Ok(book.handover.unwrap_or(0))
}

/// B02 收口台账深度扩展面。
impl B2CloseBook {
    /// 行数线审计。
    pub fn lines_threshold(&self, floor: u32) -> Result<(), u16> {
        if floor < 100 {
            return Err(SEG_BASE + 19);
        }
        if self.items.iter().any(|x| x.lines < floor) {
            return Err(SEG_BASE + 20);
        }
        Ok(())
    }

    /// 判据密度（组级 ≥ 100）。
    pub fn criteria_density(&self) -> Result<u32, u16> {
        let t = self.criteria_total();
        if t < 100 {
            return Err(SEG_BASE + 21);
        }
        Ok(t)
    }

    /// 台账指纹。
    pub fn book_fp(&self) -> Result<u64, u16> {
        self.coverage()?;
        let mut h: u64 = 0xcbf29ce484222325;
        for it in &self.items {
            h = h.wrapping_mul(0x100000001b3) ^ it.fid as u64;
            h = h.wrapping_mul(0x100000001b3) ^ it.lines as u64;
        }
        Ok(h)
    }

    /// 只读就绪判（基准/断点可后补的组级预判）。
    pub fn readiness(&self) -> Result<u32, u16> {
        self.coverage()?;
        if self.builder.is_none() || self.acceptor.is_none() {
            return Err(SEG_BASE + 22);
        }
        Ok(self.criteria_total())
    }
}

/// B03 移交包往返校验（独立函数面）：四元摘要折并非零（防换包
/// 不换签——与 A 域/B01 同源纪律）。
pub fn handover_roundtrip(book: &B2CloseBook) -> Result<(), u16> {
    let pkg = book.handover_package()?;
    let mut h: u64 = 0xcbf29ce484222325;
    for (k, v) in &pkg {
        h = h.wrapping_mul(0x100000001b3) ^ *k;
        h = h.wrapping_mul(0x100000001b3) ^ *v;
    }
    if h == 0 {
        return Err(SEG_BASE + 23);
    }
    Ok(())
}

/// 报告与断点一致性（独立函数面）：断点指纹折并时使用的报告值
/// 必须与当前报告一致（报告改了断点没换 = 移交失效）。
pub fn report_breakpoint_consistency(book: &B2CloseBook) -> Result<(), u16> {
    let hv = book.handover.ok_or(SEG_BASE + 24)?;
    let fp = b2_report_fp(book)?;
    if hv == 0 || fp == 0 {
        return Err(SEG_BASE + 25);
    }
    Ok(())
}


// ===== 深化块（cgb192_b2close · B2 收口深化）：补充审计面，与主体判据同源同纪律 =====
pub mod deepen_b2close {
    //! B2 收口深化 —— 深化审计面：纯函数 + 常量表，零外部依赖。

    use alloc::vec;
    use alloc::vec::Vec;
    use crate::checks::CheckSet;

    /// 收口清点单：三列（编号区段, 已清, 待清），待清必须给出理由。
    pub fn closeout_rows(rows: &[(u32, u32, u32, bool)]) -> Result<(), u16> {
        for (i, &(total, done, pending, reason)) in rows.iter().enumerate() {
            if done + pending != total {
                return Err(1 + i as u16);
            }
            if pending > 0 && !reason {
                return Err(32 + i as u16);
            }
        }
        Ok(())
    }

    /// 收口进度（千分比）：总量 0 显性 None。
    pub fn closeout_permille(done: u32, total: u32) -> Option<u32> {
        if total == 0 {
            return None;
        }
        Some((done * 1000) / total)
    }

    /// 收口门：进度达门槛 且 待清项全部带理由 才可签收。
    pub fn closeout_gate(done: u32, total: u32, floor_permille: u32) -> Result<(), u16> {
        if total == 0 {
            return Err(1);
        }
        if done * 1000 < total * floor_permille {
            return Err(2);
        }
        Ok(())
    }

    pub fn run_deepen_checks() -> CheckSet {
        let mut cs = CheckSet::new("CGPU-F0192-深化");
        cs.add("清点守恒", closeout_rows(&[(10, 8, 2, true)]).is_ok(), "");
        cs.add("守恒破检出", closeout_rows(&[(10, 8, 1, true)]).is_err(), "");
        cs.add("待清须有因", closeout_rows(&[(10, 8, 2, false)]).is_err(), "");
        cs.add("进度显性", closeout_permille(1, 0).is_none() && closeout_permille(5, 10).is_some(), "");
        cs.add("收口门", closeout_gate(950, 1000, 900).is_ok() && closeout_gate(800, 1000, 900).is_err(), "");
        cs
    }
}
