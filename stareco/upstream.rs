//! F131 上游回馈通道 · 完整设计（STAR I 主册 G-D-06）。
//!
//! **判据（主册）**：首批回馈 ≥2 件实际 PR 在册；流程文档完整（模板/
//! CLA/礼节三件）。
//!
//! **设计要点（主册）**：PR 拆分纪律（一个逻辑一个 PR）；commit 规范
//! 对齐上游惯例；回馈优先级：安全修复>正确性>性能——安全件走 F142
//! 披露通道协调（同步披露窗口）；PR 被拒 → 记录原因+本地补丁维护
//! （fork 责任文档化）；上游无响应 90 天 → 维护分支公告；回馈与锁版
//! 本冲突 → 升级窗统一处理。
//!
//! 本模块是回馈通道的**纯逻辑核**：流程三件完整性校验、回馈记录册
//! （SeqLedger 序号链）、PR 生命周期状态机、优先级裁决与 90 天无响应
//! 升级判定。真实向上游提交 PR 是组织动作——记录册为空时判据如实
//! 报红，绝不伪造「已提交」。

use crate::checks::CheckSet;
use crate::stareco::ebase::{fnv1a64, SeqLedger};

// ---------------------------------------------------------------------------
// 流程三件（模板/CLA/礼节）完整性
// ---------------------------------------------------------------------------

/// 流程文档三件的必需节。
pub const PROCESS_DOCS: [(&str, usize); 3] = [
    ("pr-template", 4),  // 背景/改动/验证/上游影响 四节
    ("cla", 3),          // 授权范围/署名/撤回 三节
    ("etiquette", 5),    // 拆分/commit 规范/回复礼仪/拒绝处理/无响应处理 五节
];

/// 流程三件登记表：name → 已含节数。
pub struct ProcessDocs {
    have: [Option<usize>; 3],
}

impl ProcessDocs {
    pub fn new() -> ProcessDocs {
        ProcessDocs { have: [None; 3] }
    }

    /// 登记一份流程文档的节数（0 或超出名义节数按登记无效处理——
    /// 空文档不配「在册」二字）。
    pub fn register(&mut self, name: &str, sections: usize) -> bool {
        for (i, (n, nominal)) in PROCESS_DOCS.iter().enumerate() {
            if *n == name {
                if sections == 0 || sections > *nominal {
                    return false;
                }
                self.have[i] = Some(sections);
                return true;
            }
        }
        false
    }

    /// 三件齐全且各节齐全才判「流程文档完整」（判据后半句）。
    pub fn complete(&self) -> bool {
        self.have.iter().zip(PROCESS_DOCS.iter()).all(|(h, (_, nominal))| {
            matches!(h, Some(n) if *n == *nominal)
        })
    }
}

// ---------------------------------------------------------------------------
// 回馈优先级（安全 > 正确性 > 性能）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum UpstreamPriority {
    Perf,
    Correctness,
    Security,
}

/// 优先级裁决：两条回馈同时排队，先送谁。
/// 安全件永不排在非安全件之后——判据「安全件走 F142 披露通道协调」
/// 的前置条件：披露窗口与回馈窗口同步，排程冲突一律让安全件先行。
pub fn priority_first(a: UpstreamPriority, b: UpstreamPriority) -> UpstreamPriority {
    if a >= b {
        a
    } else {
        b
    }
}

// ---------------------------------------------------------------------------
// PR 生命周期
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PrState {
    /// 本地备好（拆分/commit 规范自检过）。
    Ready,
    /// 已提交上游。
    Open,
    /// 上游合并——回馈闭环。
    Merged,
    /// 上游拒绝——记录原因 + 本地补丁维护（fork 责任）。
    Rejected,
    /// 90 天无响应——维护分支公告。
    StaleForked,
}

/// 一条回馈记录（登记进 SeqLedger，载荷指纹 = 组件+链接+状态序列化）。
#[derive(Clone, Copy, Debug)]
pub struct UpstreamPr {
    pub component: &'static str,
    pub pr_url: &'static str,
    pub priority: UpstreamPriority,
    pub state: PrState,
    /// 提交日（日序，天数单位——调用方注入）。
    pub opened_day: u32,
    /// 结案日（合并/拒绝/转 fork；0 = 未结案）。
    pub closed_day: u32,
    /// 拒绝/转 fork 的原因摘录（Merged 时为空）。
    pub note: &'static str,
}

pub const STALE_DAYS: u32 = 90;

impl UpstreamPr {
    /// 生命周期约束自检：结案态必须带结案日；Open 态不许有结案日；
    /// Rejected/StaleForked 必须写原因（静默吞原因是回馈纪律红线）。
    pub fn well_formed(&self) -> bool {
        match self.state {
            PrState::Ready | PrState::Open => self.closed_day == 0 && self.note.is_empty(),
            PrState::Merged => self.closed_day >= self.opened_day && self.note.is_empty(),
            PrState::Rejected | PrState::StaleForked => {
                self.closed_day >= self.opened_day && !self.note.is_empty()
            }
        }
    }

    /// 90 天无响应判定：Open 态且距提交超过 90 天 → 应转维护分支公告。
    pub fn stale_at(&self, today: u32) -> bool {
        self.state == PrState::Open && today.saturating_sub(self.opened_day) > STALE_DAYS
    }

    /// 状态推进（唯一入口，非法迁移拒绝）。
    pub fn transition(&mut self, to: PrState, day: u32, note: &'static str) -> Result<(), &'static str> {
        let legal = matches!(
            (self.state, to),
            (PrState::Ready, PrState::Open)
                | (PrState::Open, PrState::Merged)
                | (PrState::Open, PrState::Rejected)
                | (PrState::Open, PrState::StaleForked)
                | (PrState::Rejected, PrState::Merged) // 本地补丁被上游后续接纳
        );
        if !legal {
            return Err("illegal pr transition");
        }
        if day < self.opened_day {
            return Err("closed before opened");
        }
        // Open 是过程态：结案日只在终态（Merged/Rejected/StaleForked）落账
        self.state = to;
        if matches!(to, PrState::Merged | PrState::Rejected | PrState::StaleForked) {
            self.closed_day = day;
        }
        self.note = note;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 回馈记录册
// ---------------------------------------------------------------------------

/// 回馈记录册：条目链 + 实际在册计数。判据「≥2 件实际 PR 在册」的
/// 「在册」= `Merged` 或 `Open`（Ready 只是备稿，不算回馈事实）。
pub struct UpstreamBook {
    ledger: SeqLedger,
    items: [Option<UpstreamPr>; 8],
    count: usize,
}

impl UpstreamBook {
    pub fn new() -> UpstreamBook {
        UpstreamBook { ledger: SeqLedger::new(), items: [None; 8], count: 0 }
    }

    /// 登记一条回馈。形态不合格（结案态缺日/缺原因）拒绝——诚实纪律：
    /// 记录册里不允许出现说不清的条目。
    pub fn record(&mut self, pr: UpstreamPr) -> Result<usize, &'static str> {
        if !pr.well_formed() {
            return Err("malformed pr record");
        }
        if self.count >= 8 {
            return Err("book full");
        }
        let fp = fnv1a64(pr.component.as_bytes())
            ^ fnv1a64(pr.pr_url.as_bytes())
            ^ (pr.state as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        self.ledger.append(fp);
        self.items[self.count] = Some(pr);
        self.count += 1;
        Ok(self.count)
    }

    /// 「在册」计数：Merged + Open。
    pub fn in_force(&self) -> usize {
        self.items[..self.count]
            .iter()
            .filter(|o| matches!(o.as_ref(), Some(p) if matches!(p.state, PrState::Merged | PrState::Open)))
            .count()
    }

    pub fn chain_ok(&self) -> bool {
        self.ledger.verify()
    }

    pub fn len(&self) -> usize {
        self.count
    }

    /// 只读条目视图（公开页数据源用——不泄漏内部存储）。
    pub fn items_ref(&self) -> &[Option<UpstreamPr>] {
        &self.items[..self.count]
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F131_TAG: &str = "stareco-F131-upstream";

pub fn run_upstream_checks() -> CheckSet {
    let mut set = CheckSet::new(F131_TAG);

    // 流程三件：齐 → 绿；缺一件 → 红
    let mut docs = ProcessDocs::new();
    set.add("f131 docs incomplete initially", !docs.complete(), "nothing registered");
    assert!(docs.register("pr-template", 4));
    assert!(docs.register("cla", 3));
    set.add("f131 docs two of three", !docs.complete(), "etiquette missing");
    assert!(docs.register("etiquette", 5));
    set.add("f131 docs complete", docs.complete(), "template+cla+etiquette");
    set.add("f131 empty doc rejected", !docs.register("cla", 0), "zero sections");
    set.add("f131 unknown doc rejected", !docs.register("cookie-policy", 3), "not one of three");

    // 优先级：安全永不后置
    let first = priority_first(UpstreamPriority::Security, UpstreamPriority::Perf);
    set.add(
        "f131 security first",
        first == UpstreamPriority::Security
            && priority_first(UpstreamPriority::Perf, UpstreamPriority::Correctness)
                == UpstreamPriority::Correctness,
        "s>c>p ladder",
    );

    // PR 生命周期：Ready→Open→Merged；形态校验
    let mut pr = UpstreamPr {
        component: "smoltcp",
        pr_url: "https://example.org/smoltcp/pull/1",
        priority: UpstreamPriority::Correctness,
        state: PrState::Ready,
        opened_day: 100,
        closed_day: 0,
        note: "",
    };
    set.add("f131 ready well-formed", pr.well_formed(), "open no close day");
    assert!(pr.transition(PrState::Open, 100, "").is_ok());
    set.add("f131 open well-formed", pr.well_formed(), "no premature close");
    set.add("f131 stale not yet", !pr.stale_at(100 + STALE_DAYS), "boundary is not stale");
    set.add("f131 stale after 90d", pr.stale_at(100 + STALE_DAYS + 1), "90d line");
    assert!(pr.transition(PrState::Merged, 114, "").is_ok());
    set.add("f131 merged well-formed", pr.well_formed() && pr.closed_day == 114, "closed with day");

    // 非法迁移：Closed → Open；结案早于开案
    let mut bad = UpstreamPr {
        component: "ext4-rs",
        pr_url: "https://example.org/ext4/pull/2",
        priority: UpstreamPriority::Security,
        state: PrState::Merged,
        opened_day: 50,
        closed_day: 60,
        note: "",
    };
    set.add(
        "f131 illegal transition blocked",
        bad.transition(PrState::Open, 70, "").is_err() && bad.transition(PrState::Merged, 40, "").is_err(),
        "closed stays closed; no time travel",
    );

    // Rejected 必须带原因
    let mut rej = UpstreamPr {
        component: "limine",
        pr_url: "https://example.org/limine/pull/3",
        priority: UpstreamPriority::Perf,
        state: PrState::Ready,
        opened_day: 10,
        closed_day: 0,
        note: "",
    };
    assert!(rej.transition(PrState::Open, 10, "").is_ok());
    assert!(rej.transition(PrState::Rejected, 20, "maintainer prefers alt approach").is_ok());
    set.add("f131 rejected carries reason", rej.well_formed() && !rej.note.is_empty(), "no silent rejection");

    // 记录册：链完整 + 在册计数（Ready 不算在册）
    let mut book = UpstreamBook::new();
    assert!(book.record(pr).is_ok());
    assert!(book.record(rej).is_ok());
    let mut draft = UpstreamPr {
        component: "rust-lspa",
        pr_url: "draft-local",
        priority: UpstreamPriority::Perf,
        state: PrState::Ready,
        opened_day: 20,
        closed_day: 0,
        note: "",
    };
    assert!(book.record(draft).is_ok());
    // 在册口径 = Merged + Open：再记一条 Open 态回馈
    let open_pr = UpstreamPr {
        component: "limine-cfg",
        pr_url: "https://example.org/limine/pull/4",
        priority: UpstreamPriority::Correctness,
        state: PrState::Open,
        opened_day: 30,
        closed_day: 0,
        note: "",
    };
    assert!(book.record(open_pr).is_ok());
    set.add(
        "f131 book chain + in-force count",
        book.chain_ok() && book.len() == 4 && book.in_force() == 2,
        "merged+open=2; rejected & ready draft excluded",
    );
    // 伪造形态：Merged 带说明（合并态不许有备注）——记录册拒收
    draft.state = PrState::Merged;
    draft.closed_day = 21;
    draft.note = "pretend merged";
    set.add("f131 malformed record rejected", book.record(draft).is_err(), "merged must not carry note");

    // 判据主句：首批回馈 ≥2 件在册——记录册机制在位即机制绿；实际
    // 提交是组织动作，登记「随闸门补测」于完成报告（不伪造在册数）。
    set.add("f131 >=2 in-force gate formula", book.in_force() >= 2, "gate arithmetic");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pr_lifecycle_full() {
        let mut pr = UpstreamPr {
            component: "smoltcp",
            pr_url: "u",
            priority: UpstreamPriority::Security,
            state: PrState::Ready,
            opened_day: 1,
            closed_day: 0,
            note: "",
        };
        pr.transition(PrState::Open, 1, "").unwrap();
        assert!(pr.stale_at(1 + STALE_DAYS + 1));
        pr.transition(PrState::Rejected, 91, "wontfix").unwrap();
        pr.transition(PrState::Merged, 120, "").unwrap();
        assert!(pr.well_formed());
    }

    #[test]
    fn book_chain_survives_records() {
        let mut book = UpstreamBook::new();
        for i in 0..5u32 {
            book.record(UpstreamPr {
                component: "c",
                pr_url: "u",
                priority: UpstreamPriority::Perf,
                state: PrState::Merged,
                opened_day: i,
                closed_day: i + 1,
                note: "",
            })
            .unwrap();
        }
        assert!(book.chain_ok());
        assert_eq!(book.in_force(), 5);
    }
}
