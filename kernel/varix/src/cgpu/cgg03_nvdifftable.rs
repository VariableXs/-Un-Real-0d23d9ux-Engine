//! cgg03_nvdifftable — CGPU-F0963 NVIDIA 驱动差异表（双驱动线已知问题治理）。
//!
//! 判据原文（施工书锚点 CGPU-F0963）：差异表：NVIDIA 驱动差异（Game Ready vs
//! Studio 双驱动线差异——已知问题清单；F07 差异表体系的 NVIDIA 侧接入兑现）；
//! 差异治理（版本演进 ADR 复用 F0899）；差异选路（问题驱动规避——全局选路引
//! 擎接入）；差异公开。判据：双驱动线、接入兑现、治理、选路、公开、判据。
//!
//! 职责定位：双驱动线已知问题表+版本区间命中+规避动作建议+ADR 治理账+公开
//! 报告单点。与 cgg02_nvprobe（F0962 能力探测专项）分工——彼探测"卡能什么"，
//! 此治理"驱动版本在什么组合下会坏、怎么绕"。选路建议只出单：全局选路引擎
//! （I 域）消费后按全局裁量落地。差异表一经发布冻结，演进走 ADR 账。诊断码
//! 段 0x3Fxx。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// 差异表版本（公开口径携带，对账用）。
pub const NV_DIFF_VERSION: &str = "G03-nvdifftable-v1";

/// 双驱动线闭集（判据"Game Ready vs Studio 双驱动线"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DriverLine {
    /// Game Ready——新游戏优化线（迭代快、验证窗口短）。
    GameReady,
    /// Studio——创作稳定线（迭代慢、验证窗口长）。
    Studio,
}

impl DriverLine {
    pub const fn zh(self) -> &'static str {
        match self {
            DriverLine::GameReady => "Game Ready 驱动线",
            DriverLine::Studio => "Studio 驱动线",
        }
    }
}

/// 驱动版本（NVIDIA 三段式：主.次.热修——如 566.36.03）。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct DriverVer {
    pub major: u16,
    pub minor: u16,
    pub hotfix: u16,
}

impl DriverVer {
    /// 解析 "566.36.03" 形态（段不足按 0 补；非数字段拒绝）。
    pub fn parse(s: &str) -> Result<Self, XcCode> {
        let mut parts = [0u16; 3];
        for (i, seg) in s.split('.').enumerate() {
            if i >= 3 {
                return Err(XcCode::NvVersionInvalid);
            }
            if seg.is_empty() {
                return Err(XcCode::NvVersionInvalid);
            }
            let mut v: u32 = 0;
            for c in seg.bytes() {
                if !c.is_ascii_digit() {
                    return Err(XcCode::NvVersionInvalid);
                }
                v = v * 10 + (c - b'0') as u32;
                if v > u16::MAX as u32 {
                    return Err(XcCode::NvVersionInvalid);
                }
            }
            parts[i] = v as u16;
        }
        Ok(DriverVer {
            major: parts[0],
            minor: parts[1],
            hotfix: parts[2],
        })
    }

    /// 单调键（区间比较用）。
    pub const fn key(self) -> u32 {
        ((self.major as u32) << 16) | ((self.minor as u32) << 8) | self.hotfix as u32
    }
}

/// 症状闭集（已知问题的现象归类——公开报告与归因共用口径）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Symptom {
    /// 帧率抖动/卡顿。
    Stutter,
    /// 黑屏/闪屏。
    BlackScreen,
    /// 显存泄漏。
    VramLeak,
    /// 频率不上（降锁频）。
    Downclock,
    /// 覆盖层/叠加渲染失效。
    OverlayFail,
    /// 多屏拓扑切换异常。
    TopologyGlitch,
}

impl Symptom {
    pub const fn zh(self) -> &'static str {
        match self {
            Symptom::Stutter => "帧率抖动",
            Symptom::BlackScreen => "黑屏闪屏",
            Symptom::VramLeak => "显存泄漏",
            Symptom::Downclock => "频率不上",
            Symptom::OverlayFail => "覆盖层失效",
            Symptom::TopologyGlitch => "多屏拓扑异常",
        }
    }
}

/// 规避动作闭集（选路建议单——全局选路引擎消费；动作语义冻结）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Workaround {
    /// 切换驱动线（Game↔Studio）。
    SwitchLine,
    /// 回滚到指定版本（问题版本区间的上一稳定点）。
    Rollback,
    /// 升级到修复版（问题已被新版本收敛）。
    Upgrade,
    /// 关闭特性位（按 feature 名绕过——能力位消费方按名清位）。
    DisableFeature,
    /// 无需动作（仅留痕观测）。
    Observe,
}

impl Workaround {
    pub const fn zh(self) -> &'static str {
        match self {
            Workaround::SwitchLine => "切换驱动线",
            Workaround::Rollback => "回滚驱动版本",
            Workaround::Upgrade => "升级驱动到修复版",
            Workaround::DisableFeature => "关闭相关特性位绕过",
            Workaround::Observe => "留痕观测（不阻断）",
        }
    }
}

/// 已知问题条目（差异表行）。
#[derive(Clone, Copy, Debug)]
pub struct DriverIssue {
    /// 驱动线（双线差异——条目只挂一线）。
    pub line: DriverLine,
    /// 影响版本区间下界（含）。
    pub ver_lo: DriverVer,
    /// 影响版本区间上界（含）。
    pub ver_hi: DriverVer,
    /// 症状。
    pub symptom: Symptom,
    /// 受影响特性名（DisableFeature 规避的清位对象；空=整机级症状）。
    pub feature: &'static str,
    /// 规避动作。
    pub fix: Workaround,
    /// 是否已被更新版本收敛（收敛条目选路时降级为 Observe——历史留档）。
    pub resolved: bool,
}

impl DriverIssue {
    /// 版本区间命中（闭区间）。
    pub const fn hits(&self, v: DriverVer) -> bool {
        v.key() >= self.ver_lo.key() && v.key() <= self.ver_hi.key()
    }

    /// 条目合法性：区间序正确（lo ≤ hi）。
    pub const fn valid(&self) -> bool {
        self.ver_lo.key() <= self.ver_hi.key()
    }
}

/// ADR 治理账行（差异表演进——F0899 ADR 模式复用声明）。
#[derive(Clone, Debug)]
pub struct DiffAdr {
    /// 申请序号。
    pub seq: u32,
    /// 变更类型（true=新增条目，false=标记收敛/撤条）。
    pub add_issue: bool,
    /// 变更对象（新增时携带）。
    pub issue: Option<DriverIssue>,
    /// 收敛标记的对象下标（撤条时用）。
    pub resolve_idx: Option<usize>,
    /// 理由留痕。
    pub reason: &'static str,
    /// 审批状态（本模块只记 submitted；批准由 ADR 流程另行落账）。
    pub approved: bool,
}

/// 诊断码（0x3Fxx 段独占）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum XcCode {
    /// 0x3F01 驱动线闭集外。
    NvLineUnknown,
    /// 0x3F02 驱动版本解析失败（非数字段/段超限）。
    NvVersionInvalid,
    /// 0x3F03 命中条目已收敛（resolved——选路降级为 Observe，不阻断）。
    NvIssueStale,
    /// 0x3F04 差异表变更未过 ADR（已记账未批准）。
    NvAdrRequired,
    /// 0x3F05 已发布版本冻结（直接改表被拒——演进走 ADR）。
    NvTableFrozenImmutable,
    /// 0x3F06 表项非法（版本区间倒序——lo > hi）。
    NvIssueInvalid,
    /// 0x3F07 命中问题但条目无合法规避动作（表配置缺陷——显性立案）。
    NvNoAdvice,
}

impl XcCode {
    /// 诊断详情（闭集逐码中文说明）。
    pub const fn detail(self) -> &'static str {
        match self {
            XcCode::NvLineUnknown => "驱动线闭集外：只认 Game Ready 与 Studio 双线",
            XcCode::NvVersionInvalid => "驱动版本解析失败：期望 主.次.热修 三段数字",
            XcCode::NvIssueStale => "命中条目已被新版本收敛：仅留痕观测不阻断",
            XcCode::NvAdrRequired => "差异表变更申请已记账但 ADR 未批准：维持发布版",
            XcCode::NvTableFrozenImmutable => "差异表已发布冻结：演进必须走 ADR 账",
            XcCode::NvIssueInvalid => "表项非法：版本区间倒序（下界>上界）",
            XcCode::NvNoAdvice => "命中问题但条目无规避动作：表配置缺陷显性立案",
        }
    }
}

/// 选路建议单（判据"差异选路"的出口——全局选路引擎消费）。
#[derive(Clone, Copy, Debug)]
pub struct RouteAdvice {
    /// 命中的症状。
    pub symptom: Symptom,
    /// 规避动作。
    pub action: Workaround,
    /// 特性名（DisableFeature 时清位对象；其余为空串语义——用空 &str 表示）。
    pub feature: &'static str,
    /// 是否阻断级（未收敛的真问题=阻断；收敛/观测=留痕级）。
    pub blocking: bool,
}

/// 差异表本体（已知问题清单 + 治理账）。
pub struct NvDiffTable {
    /// 已知问题清单（发布序）。
    issues: Vec<DriverIssue>,
    /// 发布冻结（false=起草中，true=已发布——演进走 ADR）。
    published: bool,
    /// ADR 账。
    pub adr: Vec<DiffAdr>,
    /// ADR 序号。
    adr_seq: u32,
    /// 立案计数。
    pub cases: u32,
}

impl NvDiffTable {
    /// 空表起草。
    pub fn draft() -> Self {
        NvDiffTable {
            issues: Vec::new(),
            published: false,
            adr: Vec::new(),
            adr_seq: 0,
            cases: 0,
        }
    }

    /// 加表项（发布后拒；区间倒序拒；同线同症状同区间重复拒——表无歧义）。
    pub fn add_issue(&mut self, issue: DriverIssue) -> Result<(), XcCode> {
        if self.published {
            self.cases = self.cases.saturating_add(1);
            return Err(XcCode::NvTableFrozenImmutable);
        }
        if !issue.valid() {
            self.cases = self.cases.saturating_add(1);
            return Err(XcCode::NvIssueInvalid);
        }
        let dup = self.issues.iter().any(|x| {
            x.line == issue.line
                && x.symptom == issue.symptom
                && x.ver_lo.key() == issue.ver_lo.key()
                && x.ver_hi.key() == issue.ver_hi.key()
        });
        if dup {
            self.cases = self.cases.saturating_add(1);
            return Err(XcCode::NvIssueInvalid);
        }
        self.issues.push(issue);
        Ok(())
    }

    /// 表项数。
    pub fn issue_count(&self) -> usize {
        self.issues.len()
    }

    /// 发布冻结（公开前置；发布后演进只能走 ADR）。
    pub fn publish(&mut self) {
        self.published = true;
    }

    pub const fn published(&self) -> bool {
        self.published
    }

    /// 差异选路（判据主路径：按驱动线+版本查已知问题 → 规避建议单集）。
    /// 收敛条目降级 Observe（不阻断）；真问题按表内 fix 出单。
    pub fn advise(&self, line: DriverLine, v: DriverVer) -> Result<Vec<RouteAdvice>, XcCode> {
        let mut out = Vec::new();
        for it in &self.issues {
            if it.line != line || !it.hits(v) {
                continue;
            }
            if it.resolved {
                out.push(RouteAdvice {
                    symptom: it.symptom,
                    action: Workaround::Observe,
                    feature: it.feature,
                    blocking: false,
                });
                continue;
            }
            // 表项必须有合法规避动作（Observe 也算合法留痕——但真问题
            // 不允许空规避：空=表配置缺陷显性立案）
            out.push(RouteAdvice {
                symptom: it.symptom,
                action: it.fix,
                feature: it.feature,
                blocking: it.fix != Workaround::Observe,
            });
        }
        Ok(out)
    }

    /// 阻断级命中计数（选路引擎快速判据：>0 即有真问题需处置）。
    pub fn blocking_count(&self, line: DriverLine, v: DriverVer) -> usize {
        self.issues
            .iter()
            .filter(|it| it.line == line && it.hits(v) && !it.resolved && it.fix != Workaround::Observe)
            .count()
    }

    /// 差异表变更申请（发布后唯一演进通道——记 ADR 账并拒直接改）。
    pub fn request_change(
        &mut self,
        add_issue: bool,
        issue: Option<DriverIssue>,
        resolve_idx: Option<usize>,
        reason: &'static str,
    ) -> Result<u32, XcCode> {
        if !self.published {
            // 起草期直接改
            if add_issue {
                if let Some(it) = issue {
                    return self.add_issue(it).map(|_| 0);
                }
                self.cases = self.cases.saturating_add(1);
                return Err(XcCode::NvIssueInvalid);
            }
            if let Some(idx) = resolve_idx {
                if idx < self.issues.len() {
                    self.issues[idx].resolved = true;
                    return Ok(0);
                }
            }
            self.cases = self.cases.saturating_add(1);
            return Err(XcCode::NvIssueInvalid);
        }
        // 校验拟变更内容合法性（ADR 未批先验——批了就能干净落地）
        if add_issue {
            match issue {
                Some(it) if it.valid() => {}
                _ => {
                    self.cases = self.cases.saturating_add(1);
                    return Err(XcCode::NvIssueInvalid);
                }
            }
        } else if resolve_idx.map(|i| i >= self.issues.len()).unwrap_or(true) {
            self.cases = self.cases.saturating_add(1);
            return Err(XcCode::NvIssueInvalid);
        }
        self.adr_seq = self.adr_seq.saturating_add(1);
        self.adr.push(DiffAdr {
            seq: self.adr_seq,
            add_issue,
            issue,
            resolve_idx,
            reason,
            approved: false,
        });
        self.cases = self.cases.saturating_add(1);
        Err(XcCode::NvAdrRequired)
    }

    /// ADR 批准落地（调用方确已走完 ADR 流程）。
    pub fn apply_approved_adr(&mut self, adr_seq: u32) -> Result<(), XcCode> {
        let idx = match self.adr.iter().position(|a| a.seq == adr_seq && !a.approved) {
            Some(i) => i,
            None => {
                self.cases = self.cases.saturating_add(1);
                return Err(XcCode::NvAdrRequired);
            }
        };
        let entry = &mut self.adr[idx];
        entry.approved = true;
        if entry.add_issue {
            if let Some(it) = entry.issue {
                self.issues.push(it);
            }
        } else if let Some(i) = entry.resolve_idx {
            if i < self.issues.len() {
                self.issues[i].resolved = true;
            }
        }
        Ok(())
    }

    /// 差异公开报告（判据"差异公开"：对外口径单点——按线聚合）。
    pub fn public_report(&self, line: DriverLine, v: DriverVer) -> DiffReport {
        let mut open = 0u32;
        let mut stale = 0u32;
        let mut symptoms: Vec<Symptom> = Vec::new();
        for it in &self.issues {
            if it.line != line || !it.hits(v) {
                continue;
            }
            if it.resolved {
                stale += 1;
            } else {
                open += 1;
            }
            if !symptoms.contains(&it.symptom) {
                symptoms.push(it.symptom);
            }
        }
        DiffReport {
            line,
            ver: v,
            open_issues: open,
            resolved_hits: stale,
            symptoms,
            version: NV_DIFF_VERSION,
        }
    }
}

/// 公开差异报告（判据"差异公开"载体——单一出口，外部不直读表）。
#[derive(Clone, Debug)]
pub struct DiffReport {
    /// 驱动线。
    pub line: DriverLine,
    /// 版本。
    pub ver: DriverVer,
    /// 未收敛命中数。
    pub open_issues: u32,
    /// 收敛命中数（历史留档命中）。
    pub resolved_hits: u32,
    /// 命中症状集合（去重）。
    pub symptoms: Vec<Symptom>,
    /// 表版本。
    pub version: &'static str,
}

impl DiffReport {
    /// 是否干净（零未收敛命中）。
    pub const fn clean(&self) -> bool {
        self.open_issues == 0
    }

    /// 单行摘要。
    pub fn summary(&self) -> alloc::string::String {
        alloc::format!(
            "{} {}.{}.{} 未收敛{} 收敛{} 症状{}项 表{}",
            self.line.zh(),
            self.ver.major,
            self.ver.minor,
            self.ver.hotfix,
            self.open_issues,
            self.resolved_hits,
            self.symptoms.len(),
            self.version,
        )
    }
}
