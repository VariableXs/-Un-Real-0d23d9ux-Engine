//! cga154_fuzzsched.rs —— CGPU-F0154 · fuzz 长期化配置
//!
//! fuzz 的长期运行配置：词法/路径/图像/命令流四族 fuzz 的持续运行
//! 配额（每夜时长分配）、语料库治理（案例去重与退休）、长期覆盖率
//! 追踪、CI 集成面、报告生成面。
//! 设计立场：配额按「历史产出弹性」分配——哪族 fuzz 单位时间出
//! 有效案例多，哪族分得多；语料退休是防膨胀的关键（只进不出
//! 的语料库会让种子质量被稀释）。
//! 诊断码段 0x0390（本席 WB1022 分配表，全域唯一）。
//! no_std + alloc 零 unwrap、零 panic 路径。

use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x0390;

/// fuzz 族（闭集：判据「四族」）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FuzzFamily {
    /// 词法 fuzz。
    Lexical,
    /// 路径 fuzz。
    Path,
    /// 图像 fuzz。
    Image,
    /// 命令流 fuzz。
    CmdStream,
}

impl FuzzFamily {
    /// 全族序。
    pub const ALL: [FuzzFamily; 4] = [
        FuzzFamily::Lexical,
        FuzzFamily::Path,
        FuzzFamily::Image,
        FuzzFamily::CmdStream,
    ];

    /// 序号。
    pub fn rank(self) -> u8 {
        match self {
            FuzzFamily::Lexical => 0,
            FuzzFamily::Path => 1,
            FuzzFamily::Image => 2,
            FuzzFamily::CmdStream => 3,
        }
    }
}

/// 单族每夜配额（分钟）与产出弹性。
#[derive(Clone, Copy, Debug)]
pub struct FamilyQuota {
    /// 族。
    pub family: FuzzFamily,
    /// 分配分钟数。
    pub minutes: u32,
    /// 历史有效案例产出（个/小时）——弹性依据。
    pub yield_per_hour: u32,
}

impl FamilyQuota {
    /// 自检：时长界（单族每夜 10~480 分钟）。
    pub fn validate(&self) -> Result<(), u16> {
        if self.minutes < 10 || self.minutes > 480 {
            return Err(SEG_BASE + 1);
        }
        Ok(())
    }
}

/// 每夜配额表（判据「配额配置」）。
pub struct NightQuota {
    /// 四族配额。
    pub quotas: [FamilyQuota; 4],
    /// 每夜总预算（分钟）。
    pub night_budget: u32,
    /// 冻结签章。
    pub sealed: Option<u64>,
}

impl NightQuota {
    /// 弹性分配（判据「配额配置」核心算法）：按产出弹性占比
    /// 分配总预算，保底每族 10 分钟；Unknown 弹性按保底计。
    pub fn allocate(night_budget: u32, yields: [u32; 4]) -> Result<Self, u16> {
        if night_budget < 40 {
            return Err(SEG_BASE + 2);
        }
        let floor = 10u32;
        let free = night_budget - 40;
        let total: u32 = yields.iter().sum();
        let mut quotas = [FamilyQuota {
            family: FuzzFamily::Lexical,
            minutes: floor,
            yield_per_hour: 0,
        }; 4];
        for i in 0..4 {
            quotas[i].family = FuzzFamily::ALL[i];
            quotas[i].yield_per_hour = yields[i];
            quotas[i].minutes = if total == 0 {
                floor + free / 4
            } else {
                floor + (free * yields[i]) / total
            };
            quotas[i].validate()?;
        }
        Ok(NightQuota {
            quotas,
            night_budget,
            sealed: None,
        })
    }

    /// 分配合计不得超过总预算（四舍五入溢出防御）。
    pub fn budget_closed(&self) -> Result<(), u16> {
        let sum: u32 = self.quotas.iter().map(|q| q.minutes).sum();
        if sum > self.night_budget {
            return Err(SEG_BASE + 3);
        }
        Ok(())
    }

    /// 冻结。
    pub fn seal(&mut self, canon: &[u8]) -> Result<u64, u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 4);
        }
        self.budget_closed()?;
        let mut fp: u64 = 0xcbf29ce484222325;
        for q in &self.quotas {
            fp = fp.wrapping_mul(0x100000001b3) ^ (q.minutes as u64);
        }
        for &b in canon {
            fp = fp.wrapping_mul(0x100000001b3) ^ (b as u64);
        }
        self.sealed = Some(fp);
        Ok(fp)
    }
}

/// 语料案例。
#[derive(Clone, Debug)]
pub struct CorpusCase {
    /// 案例指纹。
    pub case_fp: u64,
    /// 所属族。
    pub family: FuzzFamily,
    /// 引发覆盖增量的位图指纹（0 = 无新覆盖）。
    pub cover_fp: u64,
    /// 最近命中 tick（种子被用到的最后时刻）。
    pub last_hit: u32,
    /// 退休票数：连续 N 轮治理无新贡献即退休。
    pub retire_votes: u8,
}

/// 语料库（判据「语料治理」）。
pub struct Corpus {
    /// 全部案例。
    cases: Vec<CorpusCase>,
    /// 案例容量上限（超限强制退休最冷）。
    cap: usize,
    /// 退休阈值（连续无贡献轮数）。
    retire_after: u8,
    /// 退休累计数。
    retired_total: u32,
    /// 去重拦截累计数。
    dup_blocked: u32,
}

impl Corpus {
    /// 空库。
    pub fn new(cap: usize, retire_after: u8) -> Result<Self, u16> {
        if cap < 16 {
            return Err(SEG_BASE + 5);
        }
        if retire_after == 0 {
            return Err(SEG_BASE + 6);
        }
        Ok(Corpus {
            cases: Vec::new(),
            cap,
            retire_after,
            retired_total: 0,
            dup_blocked: 0,
        })
    }

    /// 库内案例数。
    pub fn len(&self) -> usize {
        self.cases.len()
    }

    /// 空判。
    pub fn is_empty(&self) -> bool {
        self.cases.is_empty()
    }

    /// 退休累计。
    pub fn retired_count(&self) -> u32 {
        self.retired_total
    }

    /// 去重拦截累计。
    pub fn dup_count(&self) -> u32 {
        self.dup_blocked
    }

    /// 收案（判据「语料治理」入口）：同指纹去重；满库先强制
    /// 退休最冷案例（last_hit 最小且票满）再收。
    pub fn admit(
        &mut self,
        family: FuzzFamily,
        case_fp: u64,
        cover_fp: u64,
        now: u32,
    ) -> Result<bool, u16> {
        if case_fp == 0 {
            return Err(SEG_BASE + 7);
        }
        if self.cases.iter().any(|c| c.case_fp == case_fp) {
            self.dup_blocked = self.dup_blocked.saturating_add(1);
            return Ok(false);
        }
        if self.cases.len() >= self.cap {
            self.force_retire_coldest()?;
        }
        self.cases.push(CorpusCase {
            case_fp,
            family,
            cover_fp,
            last_hit: now,
            retire_votes: 0,
        });
        Ok(true)
    }

    /// 强制退休最冷一个（票满优先，其次 last_hit 最小）。
    fn force_retire_coldest(&mut self) -> Result<(), u16> {
        let mut pick: Option<usize> = None;
        for (i, c) in self.cases.iter().enumerate() {
            match pick {
                None => pick = Some(i),
                Some(p) => {
                    let better = c.retire_votes > self.cases[p].retire_votes
                        || (c.retire_votes == self.cases[p].retire_votes
                            && c.last_hit < self.cases[p].last_hit);
                    if better {
                        pick = Some(i);
                    }
                }
            }
        }
        match pick {
            Some(i) => {
                self.cases.remove(i);
                self.retired_total = self.retired_total.saturating_add(1);
                Ok(())
            }
            None => Err(SEG_BASE + 8),
        }
    }

    /// 治理轮（判据「语料治理」周期面）：对本轮未被命中的案例
    /// 记一票，票满退休；被命中案例清票并刷新 last_hit。
    pub fn govern(&mut self, hit_fps: &[u64], now: u32) -> Result<u32, u16> {
        let mut removed = 0u32;
        let mut i = 0;
        while i < self.cases.len() {
            let hit = hit_fps.contains(&self.cases[i].case_fp);
            if hit {
                self.cases[i].retire_votes = 0;
                self.cases[i].last_hit = now;
                i += 1;
            } else {
                self.cases[i].retire_votes += 1;
                if self.cases[i].retire_votes >= self.retire_after {
                    self.cases.remove(i);
                    self.retired_total = self.retired_total.saturating_add(1);
                    removed += 1;
                } else {
                    i += 1;
                }
            }
        }
        Ok(removed)
    }

    /// 覆盖追踪（判据「覆盖追踪」）：全库覆盖指纹的并集聚合
    /// （异或折叠——位图指纹级的粗并集，供趋势用）。
    pub fn coverage_fold(&self) -> u64 {
        self.cases.iter().fold(0u64, |a, c| a ^ c.cover_fp)
    }

    /// 族分布普查。
    pub fn family_census(&self) -> [usize; 4] {
        let mut out = [0usize; 4];
        for c in &self.cases {
            out[c.family.rank() as usize] += 1;
        }
        out
    }
}

/// 夜报（判据「报告」）：单夜配额执行与产出投影。
#[derive(Clone, Debug)]
pub struct NightReport {
    /// 族。
    pub family: FuzzFamily,
    /// 计划分钟。
    pub planned_min: u32,
    /// 实际分钟。
    pub actual_min: u32,
    /// 有效案例数。
    pub new_cases: u32,
    /// 覆盖增量（折叠指纹）。
    pub cover_gain: u64,
}

impl NightReport {
    /// 自检：实际不超计划（超时 = 配额失效）。
    pub fn validate(&self) -> Result<(), u16> {
        if self.actual_min > self.planned_min {
            return Err(SEG_BASE + 9);
        }
        Ok(())
    }
}

/// CI 集成检查（判据「CI 集成」）：夜报全量自检 + 零产出族
/// 预警（连续零产出的族说明配置或语料出了问题）。
pub fn ci_night_audit(reports: &[NightReport; 4]) -> Result<(), u16> {
    for r in reports {
        r.validate()?;
    }
    for (i, r) in reports.iter().enumerate() {
        if r.new_cases == 0 && r.actual_min > 0 && reports[i].planned_min >= 10 {
            // 零产出不是错误但必须显性（由调用方看报告预警位），
            // 此处仅当「跑了很久却零产出」才立案。
            if r.actual_min >= 60 {
                return Err(SEG_BASE + 10);
            }
        }
    }
    Ok(())
}

/// 配额执行审计（判据「配额配置」闭环）：夜报计划必须与冻结
/// 配额表一致（改配额不换签 = 配置漂移）。
pub fn quota_consistency(q: &NightQuota, reports: &[NightReport; 4]) -> Result<(), u16> {
    if q.sealed.is_none() {
        return Err(SEG_BASE + 11);
    }
    for i in 0..4 {
        if reports[i].family != q.quotas[i].family {
            return Err(SEG_BASE + 12);
        }
        if reports[i].planned_min != q.quotas[i].minutes {
            return Err(SEG_BASE + 13);
        }
    }
    Ok(())
}
