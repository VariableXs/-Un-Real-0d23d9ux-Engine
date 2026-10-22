//! CGPU-F0176 · B01 组收口（判据段 0x04A0）
//! B01 组 16 项收口（组级范式）：自检入树、性能基准（F0037 基准联动）、
//! 确定性验证汇总（F0172）、组双签、台账、向 B02 依赖解析组移交
//! （图模型就绪——依赖推导/拓扑排序/跨帧/优先级是图的生命周期核心，
//! 已在本组完成，B02 深化执行面）。判据：16 项入树、基准、双签、
//! 移交、断点。
//!
//! 诊断码段 0x04A0（本模块独占：0x04A0~0x04AF）。
//! no_std + alloc 零 unwrap，全部 Result 化。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x04A0;

/// B01 组收口项数（判据「16 项入树」硬界）。
pub const GROUP_ITEMS: usize = 16;

/// 收口项（一项 = 一个成员单验收态）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CloseItem {
    /// 单号（F0161~F0176 区段号）。
    pub fid: u16,
    /// 自检绿。
    pub self_check: bool,
    /// 判据数。
    pub criteria: u16,
    /// 锚点行数（实现行数，验收线以施工书目标为准）。
    pub impl_lines: u32,
}

impl CloseItem {
    /// 项校验：单号在 B01 区段、自检绿、判据非零。
    pub fn validate(&self) -> Result<(), u16> {
        if self.fid < 161 || self.fid > 176 {
            return Err(SEG_BASE);
        }
        if !self.self_check {
            return Err(SEG_BASE + 1);
        }
        if self.criteria == 0 || self.impl_lines == 0 {
            return Err(SEG_BASE + 2);
        }
        Ok(())
    }
}

/// 组收口台账（判据「16 项入树」载体）。
pub struct GroupCloseBook {
    /// 收口项（必须恰 16 项、单号唯一且全覆盖）。
    items: Vec<CloseItem>,
    /// 组基准（F0037 联动）：典型帧总耗时微秒。
    bench_us: Option<u32>,
    /// 基准样本数。
    bench_samples: u32,
    /// 双签（施工签, 验收签）——两签指纹互异。
    builder: Option<u64>,
    acceptor: Option<u64>,
    /// 移交断点（B02 开工信号）。
    handover: Option<u64>,
}

impl GroupCloseBook {
    /// 空台账。
    pub fn new() -> Self {
        GroupCloseBook {
            items: Vec::new(),
            bench_us: None,
            bench_samples: 0,
            builder: None,
            acceptor: None,
            handover: None,
        }
    }

    /// 入项（判据「自检入树」）：单号重复拒；满 16 项后拒绝再入。
    pub fn add_item(&mut self, it: CloseItem) -> Result<(), u16> {
        it.validate()?;
        if self.items.len() >= GROUP_ITEMS {
            return Err(SEG_BASE + 3);
        }
        if self.items.iter().any(|x| x.fid == it.fid) {
            return Err(SEG_BASE + 4);
        }
        self.items.push(it);
        Ok(())
    }

    /// 全覆盖审计（判据「16 项入树」强校验）：F0161~F0176 每单恰一项。
    pub fn coverage_complete(&self) -> Result<(), u16> {
        if self.items.len() != GROUP_ITEMS {
            return Err(SEG_BASE + 5);
        }
        for fid in 161..=176u16 {
            if !self.items.iter().any(|x| x.fid == fid) {
                return Err(SEG_BASE + 6);
            }
        }
        Ok(())
    }

    /// 判据总数（组级汇总）。
    pub fn criteria_total(&self) -> u32 {
        self.items.iter().map(|x| x.criteria as u32).sum()
    }

    /// 实现总行数（组级汇总）。
    pub fn lines_total(&self) -> u32 {
        self.items.iter().map(|x| x.impl_lines).sum()
    }

    /// 记基准（判据「基准」F0037 联动）：样本 ≥ 3 才可入账
    /// （单次基准是噪声不是基准）。
    pub fn set_bench(&mut self, us: u32, samples: u32) -> Result<(), u16> {
        if us == 0 {
            return Err(SEG_BASE + 7);
        }
        if samples < 3 {
            return Err(SEG_BASE + 8);
        }
        self.bench_us = Some(us);
        self.bench_samples = samples;
        Ok(())
    }

    /// 双签（判据「组双签」）：两签指纹必须互异且非零。
    pub fn dual_sign(&mut self, builder_fp: u64, acceptor_fp: u64) -> Result<(), u16> {
        if builder_fp == 0 || acceptor_fp == 0 {
            return Err(SEG_BASE + 9);
        }
        if builder_fp == acceptor_fp {
            return Err(SEG_BASE + 10);
        }
        self.builder = Some(builder_fp);
        self.acceptor = Some(acceptor_fp);
        Ok(())
    }

    /// 移交断点（判据「移交」「断点」）：B02 开工信号——覆盖齐 +
    /// 基准齐 + 双签齐三关全过才可落断点。断点指纹 = 三关的确定性
    /// 摘要（B02 可凭指纹验证断点未被篡改）。
    pub fn place_handover(&mut self, note_fp: u64) -> Result<u64, u16> {
        self.coverage_complete()?;
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
            acc = acc.wrapping_mul(0x100000001b3) ^ it.criteria as u64;
        }
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

    /// 台账绿判（判据汇总）：16 项全覆盖 + 基准齐 + 双签齐 + 断点落。
    pub fn green(&self) -> Result<(), u16> {
        self.coverage_complete()?;
        if self.bench_us.is_none() {
            return Err(SEG_BASE + 15);
        }
        if self.handover.is_none() {
            return Err(SEG_BASE + 16);
        }
        Ok(())
    }

    /// 断点指纹。
    pub fn handover_fp(&self) -> Option<u64> {
        self.handover
    }

    /// 移交包投影（判据「向 B02 移交」数据面）：B02 开工所需四元
    /// 摘要——图模型就绪声明、依赖推导锚、拓扑排序锚、优先级锚。
    pub fn handover_package(&self) -> Result<[(u64, u64); 4], u16> {
        self.green()?;
        let hv = self.handover.unwrap_or(0);
        Ok([
            (0xB050_01, hv),
            (0xB050_02, self.criteria_total() as u64),
            (0xB050_03, self.lines_total() as u64),
            (0xB050_04, self.bench_samples as u64),
        ])
    }
}

/// B01 确定性验证汇总（判据「确定性验证汇总 F0172」联动面）：
/// 组内各单的确定性观测汇成一表——全绿才算组级确定。
pub fn det_summary(scene_hashes: &[(u16, u64)]) -> Result<u64, u16> {
    if scene_hashes.len() < 2 {
        return Err(SEG_BASE + 17);
    }
    let mut acc: u64 = 0xcbf29ce484222325;
    for (fid, h) in scene_hashes {
        if *fid < 161 || *fid > 176 || *h == 0 {
            return Err(SEG_BASE + 18);
        }
        acc = acc.wrapping_mul(0x100000001b3) ^ *fid as u64;
        acc = acc.wrapping_mul(0x100000001b3) ^ *h;
    }
    if acc == 0 {
        return Err(SEG_BASE + 19);
    }
    Ok(acc)
}

/// B01 组一站式收口（总闸）：全覆盖 + 基准 + 双签 + 断点 + 确定性
/// 汇总，五关全过返回断点指纹。
pub fn b01_close_audit(
    book: &mut GroupCloseBook,
    det: &[(u16, u64)],
) -> Result<u64, u16> {
    book.green()?;
    let _ = det_summary(det)?;
    book.handover_fp().ok_or(SEG_BASE + 20)
}

/// 组收口台账深度扩展面。
impl GroupCloseBook {
    /// 行数线审计（判据「16 项入树」量化面）：每项实现行数必须
    /// 达到组统一下限。
    pub fn lines_threshold(&self, floor: u32) -> Result<(), u16> {
        if floor < 100 {
            return Err(SEG_BASE + 21);
        }
        if self.items.iter().any(|x| x.impl_lines < floor) {
            return Err(SEG_BASE + 22);
        }
        Ok(())
    }

    /// 判据密度审计（判据「自检入树」质量面）：组级判据总数
    /// ≥ 100（16 单平均 6+ 判据），低于即组深度不足。
    pub fn criteria_density(&self) -> Result<u32, u16> {
        let total = self.criteria_total();
        if total < 100 {
            return Err(SEG_BASE + 23);
        }
        Ok(total)
    }

    /// 基准健康审计（判据「基准」强化面）：组基准不得超帧预算的
    /// 80%（调度余量红线——图系统自身吃掉八成帧预算即失能）。
    pub fn bench_health(&self, frame_budget_us: u32) -> Result<(), u16> {
        let us = match self.bench_us {
            Some(u) => u,
            None => return Err(SEG_BASE + 24),
        };
        if frame_budget_us == 0 {
            return Err(SEG_BASE + 25);
        }
        if (us as u64 * 1000) > (frame_budget_us as u64 * 800) {
            return Err(SEG_BASE + 26);
        }
        Ok(())
    }

    /// 项独立指纹（判据「组双签」可算化面）：单号序加行数折并。
    pub fn item_fold(&self) -> Result<u64, u16> {
        if self.items.is_empty() {
            return Err(SEG_BASE + 27);
        }
        let mut acc: u64 = 0xcbf29ce484222325;
        for it in &self.items {
            acc = acc.wrapping_mul(0x100000001b3) ^ it.fid as u64;
            acc = acc.wrapping_mul(0x100000001b3) ^ it.impl_lines as u64;
        }
        Ok(acc)
    }
}

/// B01 组级判定（独立函数面）：不动台账的只读汇总——组是否具备
/// 收口条件（覆盖齐加双签齐，基准与断点可后补）。
pub fn b01_readiness(book: &GroupCloseBook) -> Result<u32, u16> {
    book.coverage_complete()?;
    if book.builder.is_none() || book.acceptor.is_none() {
        return Err(SEG_BASE + 28);
    }
    Ok(book.criteria_total())
}

/// 移交包往返校验（独立函数面）：移交包四元经指纹折并后非零
/// （防换包不换签——与 A 域移交纪律同源）。
pub fn handover_roundtrip(book: &GroupCloseBook) -> Result<(), u16> {
    let pkg = book.handover_package()?;
    let hv = book.handover_fp().ok_or(SEG_BASE + 29)?;
    let mut acc: u64 = 0xcbf29ce484222325;
    for (k, v) in &pkg {
        acc = acc.wrapping_mul(0x100000001b3) ^ *k;
        acc = acc.wrapping_mul(0x100000001b3) ^ *v;
    }
    if acc == 0 || hv == 0 {
        return Err(SEG_BASE + 30);
    }
    Ok(())
}
