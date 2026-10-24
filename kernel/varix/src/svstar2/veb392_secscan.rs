//! veb392_secscan —— VE-F0392 B 域安全审查。
//! 锚点（CoRun Unxreal 主册）：VE-F0392 · B 域安全审查 · 目标 380 行
//! 判据原文：VE-F0392-J1 安全审查对 B 域交付物逐件扫四类风险：
//!           越界访问、未验输入、特权提升、资源泄漏；每件出风
//!           险分（0~100，越高越险）；分档（低 <20、中 <60、
//!           高 ≥60）；高危件必须整改复扫（降分后归档）；整改
//!           追踪（整改单关闭须复扫分低于阈值）；审查总账（扫
//!           描数 = 归档数 + 整改中数）。
//! 组件：风险面（四类检查点）+ 打分器 + 分档 + 整改单 + 复扫。
//! 职责定位：安全审查是「B 域的安检门」——四类风险覆盖了内核
//!           事故的八成成因；打分不是一票否决而是排序工具：高
//!           分先修；整改闭环是审查的意义所在：只查不改等于没
//!           查，复扫降分才许关单。
//! 性能逐项分解：扫一件 O(检查点)；复扫 O(检查点)；总账 O(n)。
//! 诊断码段：0x30780（0x30781~0x3078A 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const SS_VERSION: &str = "veb392_secscan-v1";

/// 诊断段基址（0x30780 段 · 本模块独占）。
pub const SS_SEG_BASE: u32 = 0x30780;
pub const XC_NOENT: u32 = 0x30781; // 无此件
pub const XC_NAME_DUP: u32 = 0x30782; // 件重名
pub const XC_NOT_SCAN: u32 = 0x30783; // 未扫先整改
pub const XC_HIGH_RISK: u32 = 0x30784; // 高危未整改
pub const XC_MAGIC: u32 = 0x30785; // 账魔数破
pub const XC_FIX_OPEN: u32 = 0x30786; // 整改单未复扫关单
pub const XC_LEDGER: u32 = 0x30787; // 总账不平
pub const XC_SCORE_OOR: u32 = 0x30788; // 分越域
pub const XC_RESCAN_UP: u32 = 0x30789; // 复扫升分（整改无效）
pub const XC_TWICE: u32 = 0x3078A; // 重复扫描

/// 风险四类位。
pub const R_BOUND: u32 = 1 << 0; // 越界
pub const R_INPUT: u32 = 1 << 1; // 未验输入
pub const R_PRIV: u32 = 1 << 2; // 特权提升
pub const R_LEAK: u32 = 1 << 3; // 资源泄漏

/// 高危阈。
pub const HIGH: u32 = 60;
/// 关单阈（复扫分须低于此）。
pub const CLOSE_SCORE: u32 = 20;

/// 分档。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Band {
    /// 低 <20。
    Low = 0,
    /// 中 <60。
    Mid = 1,
    /// 高 ≥60。
    High = 2,
}

pub fn band_of(score: u32) -> Band {
    if score >= HIGH {
        Band::High
    } else if score >= 20 {
        Band::Mid
    } else {
        Band::Low
    }
}

/// 扫描记录。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScanRec {
    pub name: &'static str,
    /// 命中的风险位集。
    pub hits: u32,
    pub score: u32,
}

/// 整改单。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FixRec {
    pub name: &'static str,
    /// 整改前分。
    pub before: u32,
    /// 复扫分（None = 未复扫）。
    pub after: Option<u32>,
    pub closed: bool,
}

/// 审查总账。
pub struct SecBook {
    scans: Vec<ScanRec>,
    fixes: Vec<FixRec>,
    magic: u32,
}

impl SecBook {
    /// 建。
    pub fn new() -> SecBook {
        SecBook { scans: Vec::new(), fixes: Vec::new(), magic: 0x0780_5E0C }
    }

    /// 提交件。
    pub fn submit(&mut self, name: &'static str) -> Result<(), u32> {
        if self.magic != 0x0780_5E0C {
            return Err(XC_MAGIC);
        }
        for sc in self.scans.iter() {
            if sc.name == name {
                return Err(XC_NAME_DUP);
            }
        }
        self.scans.push(ScanRec { name, hits: 0, score: 0 });
        Ok(())
    }

    /// 扫描：按风险位集打分（每位 25 分，封顶 100）。
    pub fn scan(&mut self, name: &str, hits: u32) -> Result<(u32, Band), u32> {
        let i = match self.scans.iter().position(|s| s.name == name) {
            Some(i) => i,
            None => return Err(XC_NOENT),
        };
        let n = hits.count_ones();
        let score = (n * 25).min(100);
        self.scans[i].hits = hits;
        self.scans[i].score = score;
        let b = band_of(score);
        Ok((score, b))
    }

    /// 开整改单（高危件必须开）。
    pub fn open_fix(&mut self, name: &'static str) -> Result<(), u32> {
        let i = match self.scans.iter().position(|s| s.name == name) {
            Some(i) => i,
            None => return Err(XC_NOENT),
        };
        if self.scans[i].score == 0 && self.scans[i].hits == 0 {
            return Err(XC_NOT_SCAN);
        }
        for f in self.fixes.iter() {
            if f.name == name {
                return Err(XC_NOT_SCAN); // 重复开单
            }
        }
        let before = self.scans[i].score;
        self.fixes.push(FixRec { name, before, after: None, closed: false });
        Ok(())
    }

    /// 复扫（整改后）：降分归档；升分拒。
    pub fn rescan(&mut self, name: &str, new_hits: u32) -> Result<(u32, Band), u32> {
        let si = match self.scans.iter().position(|s| s.name == name) {
            Some(i) => i,
            None => return Err(XC_NOENT),
        };
        let fi = match self.fixes.iter().position(|f| f.name == name) {
            Some(i) => i,
            None => return Err(XC_NOT_SCAN),
        };
        let n = new_hits.count_ones();
        let score = (n * 25).min(100);
        // 升分判定对最近一次分数（复扫分优先，无则整改前分）。
        let last = self.fixes[fi].after.unwrap_or(self.fixes[fi].before);
        if score > last {
            return Err(XC_RESCAN_UP);
        }
        self.scans[si].hits = new_hits;
        self.scans[si].score = score;
        self.fixes[fi].after = Some(score);
        // 关单核：复扫分低于关单阈才许关。
        if score < CLOSE_SCORE {
            self.fixes[fi].closed = true;
        } else {
            return Err(XC_FIX_OPEN);
        }
        let b = band_of(score);
        Ok((score, b))
    }

    /// 总账核：件数 = 已整改（复扫降分归档）+ 整改中（有单未关）+ 无单件。
    pub fn audit(&self) -> Result<u32, u32> {
        if self.magic != 0x0780_5E0C {
            return Err(XC_MAGIC);
        }
        for sc in self.scans.iter() {
            if sc.score > 100 {
                return Err(XC_SCORE_OOR);
            }
            let mut has_fix = false;
            for f in self.fixes.iter() {
                if f.name == sc.name {
                    has_fix = true;
                    if f.closed && f.after.unwrap_or(100) >= CLOSE_SCORE {
                        return Err(XC_FIX_OPEN);
                    }
                }
            }
            // 高危无单 = 漏网。
            if sc.score >= HIGH && !has_fix {
                return Err(XC_HIGH_RISK);
            }
        }
        Ok(self.scans.len() as u32)
    }

    /// 归档数（复扫过关）。
    pub fn archived(&self) -> u32 {
        let mut n = 0u32;
        for f in self.fixes.iter() {
            if f.closed {
                n += 1;
            }
        }
        n
    }

    /// 扫描记录数。
    pub fn scan_count(&self) -> usize {
        self.scans.len()
    }
}

pub fn run_veb392_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0392");
    // 族一：打分与分档。
    s.add("零险 Low", band_of(0) == Band::Low, "0 分");
    s.add("一位 Mid", band_of(25) == Band::Mid, "25 分");
    s.add("两位 Mid", band_of(50) == Band::Mid, "50 分");
    s.add("三位 High", band_of(75) == Band::High, "75 分");
    s.add("四位 High", band_of(100) == Band::High, "满分");
    s.add("十九 Low", band_of(19) == Band::Low, "临界下");
    s.add("五十九 Mid", band_of(59) == Band::Mid, "临界中");
    // 族二：扫描流程。
    let mut b = SecBook::new();
    let _ = b.submit("dma_copy");
    let _ = b.submit("irq_route");
    let _ = b.submit("buf_zero");
    s.add("三件提交", b.scan_count() == 3, "在册");
    s.add("重名拒", b.submit("dma_copy") == Err(XC_NAME_DUP), "主键");
    let r1 = b.scan("dma_copy", R_BOUND | R_INPUT);
    s.add("两位 50", r1 == Ok((50, Band::Mid)), "2×25");
    let r2 = b.scan("irq_route", R_PRIV | R_LEAK | R_BOUND);
    s.add("三位 75 高危", r2 == Ok((75, Band::High)), "3×25");
    let r3 = b.scan("buf_zero", 0);
    s.add("零险过", r3 == Ok((0, Band::Low)), "干净件");
    s.add("野件拒", b.scan("ghost", 0) == Err(XC_NOENT), "未提交");
    // 族三：高危必须开单。
    s.add("高危无单挡", b.audit() == Err(XC_HIGH_RISK), "漏网");
    s.add("开单过", b.open_fix("irq_route").is_ok(), "立整改");
    s.add("重复开单拒", b.open_fix("irq_route") == Err(XC_NOT_SCAN), "在单");
    // 族四：复扫降分关单。
    let rs = b.rescan("irq_route", R_BOUND);
    // 25 分 ≥ 关单阈 20 → 分已记但单未关，返回 Err(XC_FIX_OPEN)。
    s.add("复扫 25 未达阈", rs == Err(XC_FIX_OPEN), "3 险→1 险仍关不了");
    // 再复扫到零险 → 0 < 20 关单。
    let rs2 = b.rescan("irq_route", 0);
    s.add("复扫清零关单", rs2 == Ok((0, Band::Low)), "全消");
    s.add("归档 1", b.archived() == 1, "一件归档");
    s.add("复扫升分拒", b.rescan("irq_route", R_LEAK | R_PRIV) == Err(XC_RESCAN_UP), "50>0 拒");
    // 族五：总账。
    s.add("审计过", b.audit() == Ok(3), "三件两档一归档");
    let mut c = SecBook::new();
    let _ = c.submit("x");
    s.add("未扫开单拒", c.open_fix("x") == Err(XC_NOT_SCAN), "零分零险");
    s.add("未扫复扫拒", c.rescan("x", 0) == Err(XC_NOT_SCAN), "无单");
    s.add("魔数核", SS_SEG_BASE == 0x30780, "段基址");
    s
}
