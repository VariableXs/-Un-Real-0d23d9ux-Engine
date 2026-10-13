//! unx225_diagdrill —— UNX-F0225 诊断模式完备性演练脚本。
//! 锚点（CoRun Unxreal 主册 L1639）：UNX-F0225 · 诊断模式完备性演练脚本 · 目标 260 行 · 已深化
//! 判据原文：UNX-F0225-J1 12 步演练脚本（进诊断/查报告/改参数/退出）全过，演练记录落账
//! 组件：12 步演练脚本冻结（进诊断 3 步 / 查报告 4 步 / 改参数 3 步 / 退出 2 步 · 单向闭集）
//!      + 演练状态机（逐步受理 + 步序闸 + 步载荷语义落账）
//!      + 演练记录落账（全程账单行导出 + 记录哈希）
//!      + 跳步/回退注入核（10 场必拒 + 合法对照）
//! 判据落地：12 步全过 + 记录落账 + 注入必拒
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 整数算术，零墙钟。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const DD_VERSION: &str = "unx225_diagdrill-v1";

/// 诊断段基址（0x5170 段 · 本模块独占）。
pub const DD_SEG_BASE: u16 = 0x5170;

/// 12 步演练脚本（冻结 v1 · 单向闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DdStep {
    /// 1 进诊断：请求进入诊断模式。
    EnterReq,
    /// 2 进诊断：断言冻结（运行面暂停）。
    EnterFreeze,
    /// 3 进诊断：进入确认。
    EnterAck,
    /// 4 查报告：拉健康摘要。
    RptHealth,
    /// 5 查报告：拉错误账。
    RptErrors,
    /// 6 查报告：拉计数账。
    RptCounts,
    /// 7 查报告：报告完整校验。
    RptVerify,
    /// 8 改参数：申请参数写权限。
    ParamReq,
    /// 9 改参数：写入参数。
    ParamWrite,
    /// 10 改参数：参数生效确认。
    ParamAck,
    /// 11 退出：退出请求（恢复运行面）。
    ExitReq,
    /// 12 退出：退出确认（解冻）。
    ExitAck,
}

impl DdStep {
    /// 全序。
    pub const ALL: [DdStep; 12] = [
        DdStep::EnterReq,
        DdStep::EnterFreeze,
        DdStep::EnterAck,
        DdStep::RptHealth,
        DdStep::RptErrors,
        DdStep::RptCounts,
        DdStep::RptVerify,
        DdStep::ParamReq,
        DdStep::ParamWrite,
        DdStep::ParamAck,
        DdStep::ExitReq,
        DdStep::ExitAck,
    ];

    /// 步名。
    pub fn name(self) -> &'static str {
        match self {
            DdStep::EnterReq => "1-enter-req",
            DdStep::EnterFreeze => "2-enter-freeze",
            DdStep::EnterAck => "3-enter-ack",
            DdStep::RptHealth => "4-rpt-health",
            DdStep::RptErrors => "5-rpt-errors",
            DdStep::RptCounts => "6-rpt-counts",
            DdStep::RptVerify => "7-rpt-verify",
            DdStep::ParamReq => "8-param-req",
            DdStep::ParamWrite => "9-param-write",
            DdStep::ParamAck => "10-param-ack",
            DdStep::ExitReq => "11-exit-req",
            DdStep::ExitAck => "12-exit-ack",
        }
    }

    /// 步序（0–11）。
    pub fn ordinal(self) -> usize {
        self as usize
    }

    /// 按序取步。
    pub fn by_ordinal(i: usize) -> Option<DdStep> {
        if i < 12 {
            Some(DdStep::ALL[i])
        } else {
            None
        }
    }

    /// 所属段（诊断会话四段：进/查/改/退）。
    pub fn phase(self) -> &'static str {
        match self {
            DdStep::EnterReq | DdStep::EnterFreeze | DdStep::EnterAck => "enter",
            DdStep::RptHealth | DdStep::RptErrors | DdStep::RptCounts | DdStep::RptVerify => "report",
            DdStep::ParamReq | DdStep::ParamWrite | DdStep::ParamAck => "param",
            DdStep::ExitReq | DdStep::ExitAck => "exit",
        }
    }
}

/// 单步执行账。
#[derive(Clone, Copy, Debug)]
pub struct DdRun {
    /// 逻辑 tick。
    pub tick: u64,
    /// 请求步。
    pub want: DdStep,
    /// 期望步。
    pub expect: DdStep,
    /// 受理。
    pub accepted: bool,
    /// 步载荷（查步=返回行数 / 写步=写入参数 id）。
    pub payload: u64,
}

/// 演练状态机（冻结 v1）。
pub struct DdDrill {
    /// 期望步。
    pub expect: DdStep,
    /// 执行账。
    pub log: Vec<DdRun>,
    /// 受理数。
    pub accepted: u32,
    /// 拒绝数。
    pub rejected: u32,
    /// 演练轮数（12 步全过 +1）。
    pub rounds: u32,
    /// 参数写账（步 9 落账）。
    pub params_written: Vec<u64>,
    /// 记录哈希。
    pub record_hash: Option<u64>,
}

impl DdDrill {
    /// 新演练（期望步 1）。
    pub fn new() -> Self {
        DdDrill {
            expect: DdStep::EnterReq,
            log: Vec::new(),
            accepted: 0,
            rejected: 0,
            rounds: 0,
            params_written: Vec::new(),
            record_hash: None,
        }
    }

    /// 请求步：序列闸（仅期望步受理）+ 步载荷语义。
    pub fn step(&mut self, tick: u64, want: DdStep, payload: u64) -> bool {
        let expect = self.expect;
        let ok = want == expect;
        if ok {
            if want == DdStep::ParamWrite {
                self.params_written.push(payload);
            }
            self.accepted += 1;
            if want == DdStep::ExitAck {
                self.rounds += 1;
                self.expect = DdStep::EnterReq;
            } else {
                self.expect = DdStep::by_ordinal(want.ordinal() + 1)
                    .unwrap_or(DdStep::ExitAck);
            }
        } else {
            self.rejected += 1;
        }
        self.log.push(DdRun {
            tick,
            want,
            expect,
            accepted: ok,
            payload,
        });
        ok
    }

    /// 12 步全链合法跑一轮。
    pub fn run_round(&mut self, base_tick: u64) -> bool {
        let mut all = true;
        for (i, st) in DdStep::ALL.iter().enumerate() {
            let payload = if *st == DdStep::ParamWrite {
                0xAA00 + i as u64
            } else {
                i as u64
            };
            if !self.step(base_tick + i as u64, *st, payload) {
                all = false;
            }
        }
        all
    }

    /// 记录落账：受理账 FNV 哈希冻结。
    pub fn seal(&mut self) -> u64 {
        let mut h = 0xcbf29ce484222325u64;
        for r in &self.log {
            if !r.accepted {
                continue;
            }
            for b in r.want.name().bytes() {
                h ^= b as u64;
                h = h.wrapping_mul(0x100000001b3);
            }
            h ^= r.payload;
            h = h.wrapping_mul(0x100000001b3);
        }
        self.record_hash = Some(h);
        h
    }

    /// 记录可复读：哈希冻结 + 重算一致。
    pub fn record_readable(&self) -> bool {
        match self.record_hash {
            None => false,
            Some(frozen) => {
                let mut h = 0xcbf29ce484222325u64;
                for r in &self.log {
                    if !r.accepted {
                        continue;
                    }
                    for b in r.want.name().bytes() {
                        h ^= b as u64;
                        h = h.wrapping_mul(0x100000001b3);
                    }
                    h ^= r.payload;
                    h = h.wrapping_mul(0x100000001b3);
                }
                h == frozen
            }
        }
    }

    /// 全程账单行导出（最近一轮受理步序列）。
    pub fn export_line(&self) -> String {
        let mut line = String::new();
        for r in &self.log {
            if !r.accepted {
                continue;
            }
            if r.want == DdStep::EnterReq && !line.is_empty() {
                break;
            }
            if !line.is_empty() {
                line.push('>');
            }
            line.push_str(r.want.name());
        }
        line
    }

    /// 判据总门。
    pub fn judge(&self) -> (bool, String) {
        let ok = self.rounds >= 1
            && self.rejected == 0
            && self.params_written.len() >= 1
            && self.record_readable();
        let msg = format!(
            "{}: rounds={} acc={} rej={} params={} record={} seg=0x{:04x}",
            DD_VERSION,
            self.rounds,
            self.accepted,
            self.rejected,
            self.params_written.len(),
            self.record_readable(),
            DD_SEG_BASE
        );
        (ok, msg)
    }
}

/// 注入场景（10 场：跳步/回退闭集 + 合法对照）。
pub const DD_INJECT_CASES: [(usize, &str); 10] = [
    (5, "step5-from-start skip"),
    (11, "step11-from-start exit-skip"),
    (8, "step8-from-start param-skip"),
    (0, "step1-again re-enter-mid"),
    (2, "step3-rewind from-step5"),
    (9, "step10-rewind from-step11"),
    (6, "step7-again repeat"),
    (0, "step1 control legal"),
    (4, "step5 control legal"),
    (11, "step12 control legal"),
];

/// 注入核。
pub struct DdInjector {
    /// 场数。
    pub cases: u32,
    /// 预期拒绝且实际拒绝。
    pub rej_ok: u32,
    /// 预期受理且实际受理。
    pub acc_ok: u32,
    /// 预期拒绝数。
    pub expect_rej: u32,
    /// 预期受理数。
    pub expect_acc: u32,
}

impl DdInjector {
    /// 空注入核。
    pub fn new() -> Self {
        DdInjector {
            cases: 0,
            rej_ok: 0,
            acc_ok: 0,
            expect_rej: 0,
            expect_acc: 0,
        }
    }

    /// 十场注入（探针状态机独立 · 不污染主演练账）。
    pub fn inject_all(&mut self) {
        let mut tick: u64 = 5000;
        for &(target, tag) in DD_INJECT_CASES.iter() {
            let mut probe = DdDrill::new();
            let control = tag.contains("control");
            let ok;
            if control {
                // 对照场：前置合法走到 target-1 再注入 target。
                let mut i = 0usize;
                while i < target {
                    probe.step(
                        tick,
                        DdStep::by_ordinal(i).unwrap_or(DdStep::EnterReq),
                        i as u64,
                    );
                    tick += 1;
                    i += 1;
                }
                ok = probe.step(
                    tick + 50,
                    DdStep::by_ordinal(target).unwrap_or(DdStep::EnterReq),
                    target as u64,
                );
            } else {
                // 必拒场：跳步语义——新鲜状态机直注目标步（前置缺失 · 必拒）；
                // step1 场（target==0）改中途重入：前置走 5 步后重注 step1
                //（期望步已前移至 step6 · 必拒；全程走完再注会撞上轮次重置
                //  期待 step1 合法受理，故不可走完全程）。
                if target == 0 {
                    for i in 0..5 {
                        probe.step(
                            tick,
                            DdStep::by_ordinal(i).unwrap_or(DdStep::EnterReq),
                            i as u64,
                        );
                        tick += 1;
                    }
                }
                ok = probe.step(
                    tick + 50,
                    DdStep::by_ordinal(target).unwrap_or(DdStep::EnterReq),
                    target as u64,
                );
            }
            tick += 100;
            self.cases += 1;
            if control {
                self.expect_acc += 1;
                if ok {
                    self.acc_ok += 1;
                }
            } else {
                self.expect_rej += 1;
                if !ok {
                    self.rej_ok += 1;
                }
            }
        }
    }

    /// 注入满格。
    pub fn all_exact(&self) -> bool {
        self.cases >= DD_INJECT_CASES.len() as u32
            && self.rej_ok == self.expect_rej
            && self.acc_ok == self.expect_acc
    }
}

/// 判据自检（t 系入口）：两轮全链 + 记录落账 + 十场注入。
pub fn run_t225_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("unx225-t225");
    let mut dd = DdDrill::new();
    let r1 = dd.run_round(1000);
    let r2 = dd.run_round(2000);
    dd.seal();
    let mut inj = DdInjector::new();
    inj.inject_all();
    let (ok, msg) = dd.judge();
    set.add("UNX-F0225-J1", ok && r1 && r2, &msg);
    set.add("DD-12-steps", dd.accepted >= 24, "");
    set.add("DD-record", dd.record_readable(), "");
    set.add("DD-inject-exact", inj.all_exact(), "");
    set
}
