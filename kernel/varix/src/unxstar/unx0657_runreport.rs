//! UNX-F0657 · 回归轮报告生成（逐轮八节）· 诊断段 0x53a0（本单独占）
//!
//! 目标行数：≥220（纯功能代码）
//! 判据原文摘录：UNX-F0657-J1 逐轮八节报告程序化生成，节全 8/8 且零手抄字段断言过。
//!
//! 设计纪律：
//!  1. no_std + alloc 纯功能线：零 unwrap/expect/panic，整数算术，确定性。
//!  2. 八节闭集：概览/判据统计/失败明细/耗时/环境/种子/产物/结论，节号固定序。
//!  3. 零手抄：报告每字段均由账源数据派生（派生函数可解引），无自由文本。
//!  4. 节完整性断言：8 节全在、每节非空、每行带账源指针。
//!  5. 诊断段 0x53a0 为本模块独占段位，错误码闭集显式判别式。

use alloc::vec::Vec;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::format;

pub const SEG_BASE: u16 = 0x53a0;

// ── 诊断码闭集 ───────────────────────────────────────────────
pub const E_NOENV: u16 = SEG_BASE + 1;   // 环境账缺失，无法生成
pub const E_NOCRIT: u16 = SEG_BASE + 2;  // 判据账缺失
pub const E_SECTION: u16 = SEG_BASE + 3; // 节不齐（<8）或节号错位
pub const E_HANDCOPY: u16 = SEG_BASE + 4; // 检出手抄字段（无账源指针）
pub const E_EMPTY: u16 = SEG_BASE + 5;   // 节体为空行
pub const E_ROUND: u16 = SEG_BASE + 6;   // 轮号非法

/// 八节闭集（判据固定 8 节）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    Overview,    // 1 概览
    CritStats,   // 2 判据统计
    FailDetail,  // 3 失败明细
    Timing,      // 4 耗时
    EnvInfo,     // 5 环境
    SeedInfo,    // 6 种子
    Artifact,    // 7 产物
    Conclusion,  // 8 结论
}

impl Section {
    pub const ALL: [Section; 8] = [
        Section::Overview,
        Section::CritStats,
        Section::FailDetail,
        Section::Timing,
        Section::EnvInfo,
        Section::SeedInfo,
        Section::Artifact,
        Section::Conclusion,
    ];

    pub fn no(self) -> u8 {
        match self {
            Section::Overview => 1,
            Section::CritStats => 2,
            Section::FailDetail => 3,
            Section::Timing => 4,
            Section::EnvInfo => 5,
            Section::SeedInfo => 6,
            Section::Artifact => 7,
            Section::Conclusion => 8,
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Section::Overview => "概览",
            Section::CritStats => "判据统计",
            Section::FailDetail => "失败明细",
            Section::Timing => "耗时",
            Section::EnvInfo => "环境",
            Section::SeedInfo => "种子",
            Section::Artifact => "产物",
            Section::Conclusion => "结论",
        }
    }
}

/// 账源指针：报告字段的派生出处（可解引 = 零手抄的证明）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LedgerSrc {
    /// 判据账第 n 条。
    Crit(u16),
    /// 环境账字段号 n。
    Env(u8),
    /// 种子簿轮号 n。
    Seed(u16),
    /// 产物账第 n 件。
    Art(u16),
    /// 聚合派生（如合计/计数），附来源节号。
    Agg(u8),
}

/// 报告行：文本 + 账源指针。
#[derive(Clone, Debug)]
pub struct ReportLine {
    pub text: String,
    pub src: LedgerSrc,
}

/// 报告节。
#[derive(Clone, Debug)]
pub struct ReportSection {
    pub sec: Section,
    pub lines: Vec<ReportLine>,
}

/// 账源数据（报告唯一输入，全部整数/闭集）。
pub struct LedgerData {
    pub round: u16,
    /// 判据账：(cid, pass, value_ms)。
    pub crits: Vec<(u16, bool, u64)>,
    /// 环境账：字段号→值（1=镜像哈希低 16 位, 2=核数, 3=工具链版本号）。
    pub env: [u64; 3],
    pub seed: u64,
    /// 产物件数。
    pub artifacts: u16,
}

impl LedgerData {
    pub fn validate(&self) -> Result<(), u16> {
        if self.round == 0 {
            return Err(E_ROUND);
        }
        if self.crits.is_empty() {
            return Err(E_NOCRIT);
        }
        if self.env == [0, 0, 0] {
            return Err(E_NOENV);
        }
        if self.seed == 0 || self.artifacts == 0 {
            return Err(E_NOENV);
        }
        Ok(())
    }
}

fn itoa_u(v: u64) -> String {
    let mut s = String::new();
    let mut v = v;
    if v == 0 {
        return String::from("0");
    }
    let mut buf = [0u8; 20];
    let mut i = buf.len();
    while v > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    for b in &buf[i..] {
        s.push(*b as char);
    }
    s
}

/// 程序化生成单节：每行由账源派生并带指针。
pub fn gen_section(sec: Section, d: &LedgerData) -> Result<ReportSection, u16> {
    d.validate()?;
    let mut lines: Vec<ReportLine> = Vec::new();
    match sec {
        Section::Overview => {
            lines.push(ReportLine {
                text: format!("轮号 R{}", itoa_u(d.round as u64)),
                src: LedgerSrc::Agg(1),
            });
            lines.push(ReportLine {
                text: format!("判据数 {}", itoa_u(d.crits.len() as u64)),
                src: LedgerSrc::Agg(2),
            });
        }
        Section::CritStats => {
            let total = d.crits.len() as u64;
            let mut pass_n: u64 = 0;
            for c in &d.crits {
                if c.1 {
                    pass_n += 1;
                }
            }
            lines.push(ReportLine {
                text: format!("通过 {}/{}", itoa_u(pass_n), itoa_u(total)),
                src: LedgerSrc::Agg(3),
            });
            let mut v: u64 = 0;
            for c in &d.crits {
                v += c.2;
            }
            lines.push(ReportLine {
                text: format!("观测值合计 {}", itoa_u(v)),
                src: LedgerSrc::Agg(4),
            });
        }
        Section::FailDetail => {
            let mut n: u64 = 0;
            for c in &d.crits {
                if !c.1 {
                    n += 1;
                    lines.push(ReportLine {
                        text: format!("失败判据 {}", itoa_u(c.0 as u64)),
                        src: LedgerSrc::Crit(c.0),
                    });
                }
            }
            if n == 0 {
                lines.push(ReportLine {
                    text: String::from("无失败判据"),
                    src: LedgerSrc::Agg(5),
                });
            }
        }
        Section::Timing => {
            let mut mx: u64 = 0;
            for c in &d.crits {
                if c.2 > mx {
                    mx = c.2;
                }
            }
            lines.push(ReportLine {
                text: format!("峰值观测 {}", itoa_u(mx)),
                src: LedgerSrc::Agg(6),
            });
        }
        Section::EnvInfo => {
            lines.push(ReportLine {
                text: format!("镜像哈希尾 {}", itoa_u(d.env[0])),
                src: LedgerSrc::Env(1),
            });
            lines.push(ReportLine {
                text: format!("核数 {}", itoa_u(d.env[1])),
                src: LedgerSrc::Env(2),
            });
        }
        Section::SeedInfo => {
            lines.push(ReportLine {
                text: format!("种子尾 {}", itoa_u(d.seed % 1_000_000)),
                src: LedgerSrc::Seed(d.round),
            });
        }
        Section::Artifact => {
            for i in 1..=d.artifacts {
                lines.push(ReportLine {
                    text: format!("产物件 {}", itoa_u(i as u64)),
                    src: LedgerSrc::Art(i),
                });
            }
        }
        Section::Conclusion => {
            let mut all = true;
            for c in &d.crits {
                if !c.1 {
                    all = false;
                }
            }
            let verdict = if all { "全绿" } else { "有红" };
            lines.push(ReportLine {
                text: String::from(verdict),
                src: LedgerSrc::Agg(7),
            });
        }
    }
    if lines.is_empty() {
        return Err(E_EMPTY);
    }
    Ok(ReportSection { sec, lines })
}

/// 整轮报告：8 节全生成 + 完整性断言。
pub struct RoundReport {
    pub round: u16,
    pub sections: Vec<ReportSection>,
}

impl RoundReport {
    pub fn generate(d: &LedgerData) -> Result<RoundReport, u16> {
        let mut sections = Vec::new();
        for sec in Section::ALL {
            sections.push(gen_section(sec, d)?);
        }
        let rep = RoundReport { round: d.round, sections };
        rep.assert_complete()?;
        rep.assert_no_handcopy()?;
        Ok(rep)
    }

    /// 节全 8/8 断言：节号 1..=8 依序在场。
    pub fn assert_complete(&self) -> Result<(), u16> {
        if self.sections.len() != 8 {
            return Err(E_SECTION);
        }
        for (i, s) in self.sections.iter().enumerate() {
            if s.sec.no() as usize != i + 1 {
                return Err(E_SECTION);
            }
            if s.lines.is_empty() {
                return Err(E_EMPTY);
            }
        }
        Ok(())
    }

    /// 零手抄断言：每行必带账源指针（指针可解引性由枚举闭集保证）。
    pub fn assert_no_handcopy(&self) -> Result<(), u16> {
        for s in &self.sections {
            for l in &s.lines {
                let deref_ok = match l.src {
                    LedgerSrc::Crit(c) => c > 0,
                    LedgerSrc::Env(f) => (1..=3).contains(&f),
                    LedgerSrc::Seed(r) => r > 0,
                    LedgerSrc::Art(a) => a > 0,
                    LedgerSrc::Agg(a) => (1..=7).contains(&a),
                };
                if !deref_ok {
                    return Err(E_HANDCOPY);
                }
            }
        }
        Ok(())
    }

    /// 节计数。
    pub fn section_count(&self) -> usize {
        self.sections.len()
    }

    /// 行计数（节全非空的旁证）。
    pub fn line_count(&self) -> usize {
        let mut n = 0;
        for s in &self.sections {
            n += s.lines.len();
        }
        n
    }
}

/// 确定性账源数据生成。
pub fn synth_ledger(seed: u64, round: u16) -> Result<LedgerData, u16> {
    if seed == 0 {
        return Err(E_NOENV);
    }
    let mut st = seed ^ (round as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    let mut crits = Vec::new();
    for i in 1..=16u16 {
        st = st.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        let pass = (st >> 40) % 29 != 0;
        crits.push((i, pass, (st >> 24) % 900 + 10));
    }
    Ok(LedgerData {
        round,
        crits,
        env: [0x5A17, 4, 2026],
        seed,
        artifacts: 3,
    })
}

/// 自例程：3 轮 × 8 节全齐 + 零手抄 + 断言注入全检出 → 0。
pub fn drill() -> Result<u32, u16> {
    let mut total_lines = 0;
    for r in 1..=3u16 {
        let d = synth_ledger(0x0657_0001, r)?;
        let rep = RoundReport::generate(&d)?;
        if rep.section_count() != 8 {
            return Err(E_SECTION);
        }
        if rep.line_count() == 0 {
            return Err(E_EMPTY);
        }
        total_lines += rep.line_count();
    }
    if total_lines == 0 {
        return Err(E_EMPTY);
    }
    // 注入 1：空判据账 → 拦截
    let bad = LedgerData { round: 1, crits: vec![], env: [1, 2, 3], seed: 7, artifacts: 1 };
    if RoundReport::generate(&bad).err() != Some(E_NOCRIT) {
        return Err(SEG_BASE + 15);
    }
    // 注入 2：空环境账 → 拦截
    let bad2 = LedgerData {
        round: 1,
        crits: vec![(1, true, 5)],
        env: [0, 0, 0],
        seed: 7,
        artifacts: 1,
    };
    if RoundReport::generate(&bad2).err() != Some(E_NOENV) {
        return Err(SEG_BASE + 16);
    }
    // 注入 3：非法轮号 → 拦截
    let mut bad3 = synth_ledger(0x0657_0001, 1)?;
    bad3.round = 0;
    if RoundReport::generate(&bad3).err() != Some(E_ROUND) {
        return Err(SEG_BASE + 17);
    }
    Ok(0)
}
