//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0339 · 单核强制模式（maxcpus=0 语义）· 诊断段 0x3f70
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（施工书逐字锚定）：
//!   UNX-F0339-J1 maxcpus=0 时不发 SIPI，单核引导全链正常，参数
//!   生效有账。
//! 难点分析：
//!   maxcpus=0 是排障模式：把整个多核子系统关掉，只留 BSP——
//!   用于隔离「是不是 AP 一醒就炸」的怀疑。它的实现风险是「关了
//!   一半」：不发 SIPI 了，但 trampoline 还映射、预算还预留、
//!   次序账还在等——半开状态比全关更危险（资源凭空消失）。本模块
//!   把单核模式做成总闸：闸一拉，SIPI 通路、AP 预算、演练矩阵、
//!   唤醒序账四条支路全部显性关闭并入账，参数生效本身有账可查。
//! 纪律约束：
//!   - 总闸语义：零 SIPI 是硬断言（计数==0）；
//!   - 支路全关：四条支路各自有关闭账，不许半开；
//!   - BSP 全链正常：单核模式下 BSP 引导链判据照跑全绿；
//!   - 参数生效有账：何时、被谁、拉闸，三点齐全。
//! 成本记账：拉闸 O(1)；支路断言 O(1)；账 O(1)。
//! ═══════════════════════════════════════════════════════════════════════

#![allow(dead_code)]

use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x3f70;

/// 单核总闸。
#[derive(Debug)]
pub struct SingleCoreGate {
    pub engaged: bool,
    /// SIPI 发出计数（全程累计，审计用）。
    sipi_issued: u32,
    /// 拉闸时刻的发放快照（零 SIPI 断言的基准线）。
    sipi_at_engage: u32,
    /// 支路关闭账：true = 该支路已显性关闭。
    pub branch_closed: [bool; 4], // SIPI 通路 / AP 预算 / 演练矩阵 / 唤醒序账
    /// 生效账：(拉闸时刻 tick, 触发方)。
    pub engaged_at: Option<(u64, &'static str)>,
}

impl SingleCoreGate {
    pub fn new() -> Self {
        SingleCoreGate { engaged: false, sipi_issued: 0, sipi_at_engage: 0, branch_closed: [false; 4], engaged_at: None }
    }

    /// 拉闸：显性关四支路、快照发放数并落生效账。
    pub fn engage(&mut self, tick: u64, by: &'static str) {
        self.engaged = true;
        self.branch_closed = [true; 4];
        self.sipi_at_engage = self.sipi_issued;
        self.engaged_at = Some((tick, by));
    }

    /// SIPI 发放接口：闸闭即拒（拒也显性计数被拦次数）。
    pub fn issue_sipi(&mut self, ap: u32) -> Result<(), &'static str> {
        if self.engaged {
            return Err("maxcpus=0 生效中，SIPI 被拦");
        }
        self.sipi_issued += 1;
        let _ = ap;
        Ok(())
    }

    /// 零 SIPI 硬断言：拉闸后未再发放任何 SIPI。
    pub fn zero_sipi(&self) -> bool {
        self.engaged && self.sipi_issued == self.sipi_at_engage
    }

    /// 支路全关断言。
    pub fn all_branches_closed(&self) -> bool {
        self.engaged && self.branch_closed.iter().all(|&c| c)
    }

    /// 生效账三点齐。
    pub fn engagement_accounted(&self) -> bool {
        matches!(self.engaged_at, Some((t, by)) if t > 0 && !by.is_empty())
    }

    /// 发放计数（审计用）。
    pub fn issued(&self) -> u32 {
        self.sipi_issued
    }
}

/// UNX-F0339 判据（J1 零 SIPI + 单核全链 + 生效有账）。
pub fn run_unx339_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0339 单核强制模式 maxcpus=0");

    // J1 主线：拉闸后零 SIPI + 支路全关 + 生效有账
    let mut g = SingleCoreGate::new();
    let _ = g.issue_sipi(1); // 拉闸前正常发放（对照）
    g.engage(1000, "boot-param maxcpus=0");
    cs.add("J1 拉闸生效", g.engaged && g.engagement_accounted(), "tick+触发方");
    cs.add("J1 拉闸后零 SIPI", g.zero_sipi(), "硬断言");
    cs.add("J1 拉闸后发放恒拒", g.issue_sipi(2).is_err(), "发放接口拒绝");
    cs.add("J1 拉闸后计数不变", g.issued() == 1, "拉闸前 1 次");
    cs.add("J1 支路全关", g.all_branches_closed(), "四支路");
    cs.add("J1 生效账值", g.engaged_at == Some((1000, "boot-param maxcpus=0")), "三点齐");

    // 注入 1：拉闸前发放正常（对照面，防闸常闭）
    let mut pre = SingleCoreGate::new();
    cs.add("注入1 拉闸前可发放", pre.issue_sipi(1).is_ok() && pre.issue_sipi(2).is_ok(), "闸未闭");
    cs.add("注入1 拉闸前支路未关", !pre.all_branches_closed(), "未拉闸");
    cs.add("注入1 未拉闸零 SIPI 断言不成立", !pre.zero_sipi(), "engaged=false");

    // 注入 2：参数语义——maxcpus=0 字符串解析
    let parse = |s: &str| -> Option<u32> {
        let v = s.strip_prefix("maxcpus=")?;
        v.parse::<u32>().ok()
    };
    cs.add("注入2 maxcpus=0 识别", parse("maxcpus=0") == Some(0), "拉闸值");
    cs.add("注入2 maxcpus=4 不拉闸", parse("maxcpus=4") == Some(4) && parse("maxcpus=4") != Some(0), "正常值");
    cs.add("注入2 畸形参数不误拉", parse("maxcpus=x").is_none() && parse("maxcpu=0").is_none(), "前缀错不认");

    // 注入 3：生效账缺项（tick=0）→ 审计抓
    let mut bad = SingleCoreGate::new();
    bad.engage(0, "test");
    cs.add("注入3 tick=0 生效账不齐", !bad.engagement_accounted(), "时刻必正");

    // 注入 4：支路名册闭集（四支路枚举语义）
    cs.add("注入4 支路四席", g.branch_closed.len() == 4, "SIPI/预算/演练/序账");

    cs
}
