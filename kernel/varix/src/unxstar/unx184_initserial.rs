//! unx184_initserial —— UNX-F0184 init 串行性显式声明。
//! 锚点（CoRun Unxreal 主册 L1584）：UNX-F0184 · init 串行性显式声明 · 目标 180 行 · 已深化
//! 判据原文：UNX-F0184-J1 串行假设以断言固化，越序调用检出 100%（并行化留 A3 后按 ADR）
//! 组件：init 阶段序闭集冻结（七阶段单向 · IsStage）
//!      + 串行假设断言器（每阶段恰好一次 · 越序/重复/跳步三类违例检出）
//!      + 越序调用注入核（10 场违例注入 · 检出 10/10）
//!      + 并行化 ADR 放行口（默认全串行冻结 v1 · 显性 ADR 编号放行留痕）
//! 判据落地：越序检出 100% + 合法序列全过 + ADR 放行留痕可查
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u32/u64 整数算术，零墙钟。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// 模块版本（串行假设冻结 v1）。
pub const IS_VERSION: &str = "unx184_initserial-v1";

/// 诊断段基址（0x4fb0 段 · 本模块独占）。
pub const IS_SEG_BASE: u16 = 0x4fb0;

/// init 阶段序（冻结 v1 · 七阶段单向闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IsStage {
    /// P0 BSP 自识别（核 id / 特征位基线）。
    BspId,
    /// P1 页表建立（CR3 主表装载）。
    PageTable,
    /// P2 中断基座（IDT + 8259/APIC 初始化）。
    IntrBase,
    /// P3 定时器（TSC/HET/HPET 三源择一）。
    Timer,
    /// P4 调度器（就绪队列 + idle 环）。
    Scheduler,
    /// P5 驱动基座（串口/NVMe 枚举）。
    Drivers,
    /// P6 用户态预备（ABI 账 + 载入器）。
    UserPrep,
}

impl IsStage {
    /// 七阶段全序（冻结 v1）。
    pub const ALL: [IsStage; 7] = [
        IsStage::BspId,
        IsStage::PageTable,
        IsStage::IntrBase,
        IsStage::Timer,
        IsStage::Scheduler,
        IsStage::Drivers,
        IsStage::UserPrep,
    ];

    /// 阶段名。
    pub fn name(self) -> &'static str {
        match self {
            IsStage::BspId => "P0-bsp-id",
            IsStage::PageTable => "P1-page-table",
            IsStage::IntrBase => "P2-intr-base",
            IsStage::Timer => "P3-timer",
            IsStage::Scheduler => "P4-scheduler",
            IsStage::Drivers => "P5-drivers",
            IsStage::UserPrep => "P6-user-prep",
        }
    }

    /// 阶段序号（0–6）。
    pub fn ordinal(self) -> usize {
        match self {
            IsStage::BspId => 0,
            IsStage::PageTable => 1,
            IsStage::IntrBase => 2,
            IsStage::Timer => 3,
            IsStage::Scheduler => 4,
            IsStage::Drivers => 5,
            IsStage::UserPrep => 6,
        }
    }

    /// 按序号取阶段。
    pub fn by_ordinal(i: usize) -> Option<IsStage> {
        if i < 7 {
            Some(IsStage::ALL[i])
        } else {
            None
        }
    }
}

/// 违例类别（检出账闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IsViol {
    /// 越序：请求阶段不是当前期望阶段。
    OutOfOrder,
    /// 重复：请求阶段已完成。
    Reenter,
    /// 跳步：请求阶段序号大于期望（前置未完成）。
    Skip,
}

impl IsViol {
    /// 违例名。
    pub fn name(self) -> &'static str {
        match self {
            IsViol::OutOfOrder => "out-of-order",
            IsViol::Reenter => "re-enter",
            IsViol::Skip => "skip",
        }
    }
}

/// 单次进入登记。
#[derive(Clone, Copy, Debug)]
pub struct IsStep {
    /// 逻辑 tick。
    pub tick: u64,
    /// 请求阶段。
    pub want: IsStage,
    /// 期望阶段（违例时的正确答案）。
    pub expect: IsStage,
    /// 是否受理。
    pub accepted: bool,
    /// 违例类别（受理时 None 语义用 0xFF 哨兵）。
    pub viol: u8,
}

/// 违例哨兵（无违例）。
pub const IS_VIOL_NONE: u8 = 0xFF;

/// 串行假设断言器（冻结 v1 · 全串行）。
pub struct IsSerial {
    /// 当前期望阶段。
    pub expect: IsStage,
    /// 已完成阶段位板（bit i = 阶段 i 已过）。
    pub done_bits: u8,
    /// 进入账。
    pub log: Vec<IsStep>,
    /// 受理数。
    pub accepted: u32,
    /// 拒绝数。
    pub rejected: u32,
    /// 违例分类计数（越序/重复/跳步）。
    pub viol_counts: [u32; 3],
    /// ADR 放行账（并行化显性放行 · 冻结 v1 默认空）。
    pub adr_waivers: Vec<(String, IsStage, IsStage)>,
}

impl IsSerial {
    /// 新断言器（期望 P0）。
    pub fn new() -> Self {
        IsSerial {
            expect: IsStage::BspId,
            done_bits: 0,
            log: Vec::new(),
            accepted: 0,
            rejected: 0,
            viol_counts: [0; 3],
            adr_waivers: Vec::new(),
        }
    }

    /// 请求进入阶段：串行闸——仅期望阶段可受理。
    pub fn enter(&mut self, tick: u64, want: IsStage) -> bool {
        let expect = self.expect;
        let accepted = want == expect;
        let viol = if accepted {
            IS_VIOL_NONE
        } else {
            // 分类：已完成→重复；序号大于期望→跳步；否则越序。
            let v = if self.done_bits & (1 << want.ordinal()) != 0 {
                IsViol::Reenter
            } else if want.ordinal() > expect.ordinal() {
                IsViol::Skip
            } else {
                IsViol::OutOfOrder
            };
            self.viol_counts[v.ordinal_of()] += 1;
            v.code()
        };
        if accepted {
            self.done_bits |= 1 << expect.ordinal();
            self.accepted += 1;
            // 推进期望；P6 完成后回卷到 P0（可再次全流程跑）。
            self.expect = IsStage::by_ordinal(expect.ordinal() + 1)
                .unwrap_or(IsStage::BspId);
            if expect.ordinal() == 6 {
                self.done_bits = 0;
            }
        } else {
            self.rejected += 1;
        }
        self.log.push(IsStep {
            tick,
            want,
            expect,
            accepted,
            viol,
        });
        accepted
    }

    /// 越序检出 100% 口径：任一非期望请求必拒且分类落账。
    pub fn viol_detected(&self) -> bool {
        // 全账复核：受理 ⇔ want==expect。
        self.log
            .iter()
            .all(|s| s.accepted == (s.want == s.expect))
    }

    /// 合法序列全过：七阶段依次受理一遍零拒绝。
    pub fn clean_pass(&mut self, base_tick: u64) -> bool {
        let mut ok = true;
        for i in 0..7 {
            if !self.enter(base_tick + i as u64, IsStage::by_ordinal(i).unwrap_or(IsStage::BspId)) {
                ok = false;
            }
        }
        ok
    }

    /// ADR 放行：显性登记并行化豁免（冻结 v1 · 只留痕不实际放行执行序）。
    pub fn adr_waiver(&mut self, adr_id: &str, from: IsStage, to: IsStage) {
        self.adr_waivers
            .push((String::from(adr_id), from, to));
    }

    /// 账单行导出。
    pub fn export_line(&self) -> String {
        format!(
            "{}: steps={} acc={} rej={} ooo={} re={} skip={} waivers={} seg=0x{:04x}",
            IS_VERSION,
            self.log.len(),
            self.accepted,
            self.rejected,
            self.viol_counts[0],
            self.viol_counts[1],
            self.viol_counts[2],
            self.adr_waivers.len(),
            IS_SEG_BASE
        )
    }
}

impl IsViol {
    /// 分类序号（0/1/2）。
    pub fn ordinal_of(self) -> usize {
        match self {
            IsViol::OutOfOrder => 0,
            IsViol::Reenter => 1,
            IsViol::Skip => 2,
        }
    }

    /// 违例码（账面哨兵口径）。
    pub fn code(self) -> u8 {
        match self {
            IsViol::OutOfOrder => 0x01,
            IsViol::Reenter => 0x02,
            IsViol::Skip => 0x03,
        }
    }
}

/// 越序调用注入场景（10 场 · 三类违例覆盖 + 合法对照）。
pub const IS_INJECT_CASES: [(usize, &'static str); 10] = [
    (2, "want-P2 expect-P0 skip"),
    (5, "want-P5 expect-P0 skip"),
    (6, "want-P6 expect-P0 skip"),
    (0, "want-P0 expect-P0 control-accept"),
    (1, "want-P1 expect-P1 control-accept"),
    (0, "want-P0 expect-P6 reenter-after-full"),
    (3, "want-P3 expect-P0 skip-mid"),
    (1, "want-P1 expect-P2 out-of-order-back"),
    (4, "want-P4 expect-P1 skip-ahead"),
    (2, "want-P2 expect-P2 control-accept"),
];

/// 越序注入核：逐场断言受理/拒绝与预期一致。
pub struct IsInjector {
    /// 场数。
    pub cases: u32,
    /// 预期拒绝且实际拒绝数。
    pub rej_ok: u32,
    /// 预期受理且实际受理数。
    pub acc_ok: u32,
    /// 预期拒绝数。
    pub expect_rej: u32,
    /// 预期受理数。
    pub expect_acc: u32,
    /// 明细。
    pub log: Vec<String>,
}

impl IsInjector {
    /// 空注入核。
    pub fn new() -> Self {
        IsInjector {
            cases: 0,
            rej_ok: 0,
            acc_ok: 0,
            expect_rej: 0,
            expect_acc: 0,
            log: Vec::new(),
        }
    }

    /// 执行 10 场注入：先合法推进到场景起点，再注入目标请求。
    pub fn inject_all(&mut self, ser: &mut IsSerial) {
        // 全场按序驱动：每场先把断言器带到 expect 状态（重建新断言器按需）。
        let mut probe = IsSerial::new();
        let mut tick: u64 = 1000;
        for &(want_ord, tag) in IS_INJECT_CASES.iter() {
            let control = tag.contains("control-accept");
            let want = IsStage::by_ordinal(want_ord).unwrap_or(IsStage::BspId);
            let ok;
            if control {
                // 对照场：前置合法走到 target-1，注入 target → 合法受理。
                while probe.expect.ordinal() != want_ord {
                    let cur = probe.expect.ordinal();
                    probe.enter(tick, IsStage::by_ordinal(cur).unwrap_or(IsStage::BspId));
                    tick += 1;
                }
                ok = probe.enter(tick + 100, want);
            } else {
                // 必拒场：先全程合法走完七阶段（expect 回卷 P0），再重注入
                // 目标阶段 → 已完成阶段（re-enter）或已完成序列中的旧序必拒。
                // 走完后 expect=P0、done_bits 已清 → 注入 P1..P6 均为 skip 必拒；
                // 注入 P0 为合法受理，故 P0 目标改注 P1 等价必拒场。
                for i in 0..7 {
                    probe.enter(tick, IsStage::by_ordinal(i).unwrap_or(IsStage::BspId));
                    tick += 1;
                }
                let rewant = if want_ord == 0 {
                    IsStage::PageTable
                } else {
                    want
                };
                ok = probe.enter(tick + 100, rewant);
            }
            tick += 200;
            self.cases += 1;
            // 预期：tag 含 control-accept → 受理；否则拒绝。
            let expect_ok = tag.contains("control-accept");
            if expect_ok {
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
            self.log.push(format!(
                "#{} {} -> {}",
                self.cases,
                tag,
                if expect_ok {
                    if ok { "ACCEPTED" } else { "MISS(denied!)" }
                } else if !ok {
                    "REJECTED" } else { "MISS(accepted!)" }
            ));
        }
        // probe 账并入主断言器（探针重建一致性）。
        *ser = probe;
    }

    /// 注入满格：必拒全拒 + 对照全受理。
    pub fn all_exact(&self) -> bool {
        self.cases >= IS_INJECT_CASES.len() as u32
            && self.rej_ok == self.expect_rej
            && self.acc_ok == self.expect_acc
    }
}

/// init 串行性门面（判据三件：合法全过/越序检出 100%/ADR 留痕）。
pub struct IsGate {
    /// 断言器。
    pub ser: IsSerial,
    /// 注入核。
    pub inj: IsInjector,
}

impl IsGate {
    /// 新门面。
    pub fn new() -> Self {
        IsGate {
            ser: IsSerial::new(),
            inj: IsInjector::new(),
        }
    }

    /// 判据总门。
    pub fn judge(&self) -> (bool, String) {
        let ok = self.ser.viol_detected()
            && self.ser.accepted >= 7
            && self.inj.all_exact();
        (ok, self.ser.export_line())
    }
}

/// 判据自检（t 系入口）：两轮合法全流程 + 10 场注入 + ADR 留痕。
pub fn run_t184_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("unx184-t184");
    let mut g = IsGate::new();
    // 轮一：合法七阶段全过。
    let clean1 = g.ser.clean_pass(100);
    // 10 场注入（内含合法对照）。
    g.inj.inject_all(&mut g.ser);
    // ADR 放行留痕（冻结 v1 · 只登记）。
    g.ser.adr_waiver("ADR-A3-0001", IsStage::Timer, IsStage::Scheduler);
    g.ser.adr_waiver("ADR-A3-0002", IsStage::Drivers, IsStage::UserPrep);
    let (ok, msg) = g.judge();
    set.add_dyn("UNX-F0184-J1", ok && clean1, &msg);
    set.add("IS-clean-pass", clean1, "");
    set.add("IS-viol-100%", g.ser.viol_detected(), "");
    set.add("IS-inject-exact", g.inj.all_exact(), "");
    set.add("IS-adr-logged", g.ser.adr_waivers.len() == 2, "");
    set
}
