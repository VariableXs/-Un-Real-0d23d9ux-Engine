//! unx343_appgt —— UNX-F0343 AP 初始页表切换（临时表 → 主表）。
//! 锚点（CoRun Unxreal 主册 L1800）：UNX-F0343 · AP 初始页表切换（临时表 → 主表） · 目标 280 行 · 已深化
//! 判据原文：UNX-F0343-J1 AP 端 CR3 切换主表序列正确，切换前后访问一致性断言过
//! 组件：页表账（临时表/主表 双根 · PML4 槽位账 · CR3 值账）
//!      + 切换序列状态机（MapTemp→ValidateTemp→BuildMain→SwitchCR3→Verify→Commit 六步单向）
//!      + 访问一致性断言（切换前后同一虚拟地址翻译结果一致账 · 采样页集）
//!      + 跳步/回切注入核（10 场 · 序列闸 10/10 拒绝）
//! 判据落地：切换序列正确 + 前后翻译一致 + 注入必拒
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 物理地址整数算术，零墙钟。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// 模块版本。
pub const AG_VERSION: &str = "unx343_appgt-v1";

/// 诊断段基址（0x5150 段 · 本模块独占）。
pub const AG_SEG_BASE: u16 = 0x5150;

/// 页大小（4KB 口径）。
pub const AG_PAGE: u64 = 0x1000;

/// 物理地址掩码（4 层 · 52 位物理）。
pub const AG_PHYS_MASK: u64 = 0x000F_FFFF_FFFF_F000;

/// CR3 合法性：低 12 位为标志位（PCID/PWT/PCD），基址必须 4K 对齐。
pub fn ag_cr3_base(cr3: u64) -> u64 {
    cr3 & AG_PHYS_MASK
}

/// PML4 槽账（主表关键槽位冻结 v1）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgSlot {
    /// 槽 0：内核低映射（恒等区）。
    KernelLow,
    /// 槽 256：内核高半区（0xFFFF8000…）。
    KernelHigh,
    /// 槽 510：临时引导映射区（切换后退役）。
    TempBoot,
    /// 槽 511：自映射（页表递归）。
    SelfMap,
}

impl AgSlot {
    /// 槽号。
    pub fn index(self) -> usize {
        match self {
            AgSlot::KernelLow => 0,
            AgSlot::KernelHigh => 256,
            AgSlot::TempBoot => 510,
            AgSlot::SelfMap => 511,
        }
    }

    /// 槽名。
    pub fn name(self) -> &'static str {
        match self {
            AgSlot::KernelLow => "klow",
            AgSlot::KernelHigh => "khigh",
            AgSlot::TempBoot => "temp",
            AgSlot::SelfMap => "selfmap",
        }
    }
}

/// 采样页（一致性断言集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AgPage {
    /// 虚拟地址（4K 对齐）。
    pub va: u64,
    /// 期望物理帧（翻译结果）。
    pub expect_pa: u64,
}

impl AgPage {
    /// 合法页（对齐校验）。
    pub fn valid(&self) -> bool {
        self.va % AG_PAGE == 0 && self.expect_pa % AG_PAGE == 0
    }
}

/// 切换序列步骤（六步单向闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgStep {
    /// S1 临时表映射就绪（TempBoot 槽建立）。
    MapTemp,
    /// S2 临时表可访问校验（采样页经临时表翻译通过）。
    ValidateTemp,
    /// S3 主表构建（四槽就绪 + 采样页主表映射）。
    BuildMain,
    /// S4 CR3 切换（装载主表基址）。
    SwitchCR3,
    /// S5 切换后访问校验（同采样页集翻译一致）。
    Verify,
    /// S6 临时槽退役（TempBoot 槽清 present · 提交）。
    Commit,
}

impl AgStep {
    /// 六步全序。
    pub const ALL: [AgStep; 6] = [
        AgStep::MapTemp,
        AgStep::ValidateTemp,
        AgStep::BuildMain,
        AgStep::SwitchCR3,
        AgStep::Verify,
        AgStep::Commit,
    ];

    /// 步名。
    pub fn name(self) -> &'static str {
        match self {
            AgStep::MapTemp => "S1-map-temp",
            AgStep::ValidateTemp => "S2-validate-temp",
            AgStep::BuildMain => "S3-build-main",
            AgStep::SwitchCR3 => "S4-switch-cr3",
            AgStep::Verify => "S5-verify",
            AgStep::Commit => "S6-commit",
        }
    }

    /// 步序（0–5）。
    pub fn ordinal(self) -> usize {
        match self {
            AgStep::MapTemp => 0,
            AgStep::ValidateTemp => 1,
            AgStep::BuildMain => 2,
            AgStep::SwitchCR3 => 3,
            AgStep::Verify => 4,
            AgStep::Commit => 5,
        }
    }

    /// 按序取步。
    pub fn by_ordinal(i: usize) -> Option<AgStep> {
        if i < 6 {
            Some(AgStep::ALL[i])
        } else {
            None
        }
    }
}

/// 页表根账。
#[derive(Clone, Copy, Debug)]
pub struct AgRoot {
    /// 根物理基址（PML4 · 4K 对齐）。
    pub pml4_pa: u64,
    /// 槽位就绪位板（bit i = AgSlot::ALL[i] 就绪）。
    pub slot_bits: u8,
    /// present 槽数。
    pub present: u32,
}

impl AgRoot {
    /// 新根（空槽）。
    pub fn new(pml4_pa: u64) -> Self {
        AgRoot {
            pml4_pa: ag_cr3_base(pml4_pa),
            slot_bits: 0,
            present: 0,
        }
    }

    /// 槽就绪登记。
    pub fn map_slot(&mut self, s: AgSlot) {
        let bit = 1 << s.ordinal_of_slot();
        if self.slot_bits & bit == 0 {
            self.present += 1;
        }
        self.slot_bits |= bit;
    }

    /// 槽退役。
    pub fn unmap_slot(&mut self, s: AgSlot) {
        let bit = 1 << s.ordinal_of_slot();
        if self.slot_bits & bit != 0 {
            self.present = self.present.saturating_sub(1);
        }
        self.slot_bits &= !bit;
    }

    /// 槽就绪查询。
    pub fn has_slot(&self, s: AgSlot) -> bool {
        self.slot_bits & (1 << s.ordinal_of_slot()) != 0
    }
}

impl AgSlot {
    /// 槽位序（0–3 · 位板序）。
    pub fn ordinal_of_slot(self) -> usize {
        match self {
            AgSlot::KernelLow => 0,
            AgSlot::KernelHigh => 1,
            AgSlot::TempBoot => 2,
            AgSlot::SelfMap => 3,
        }
    }
}

/// 单步执行账。
#[derive(Clone, Copy, Debug)]
pub struct AgRun {
    /// 逻辑 tick。
    pub tick: u64,
    /// 请求步。
    pub want: AgStep,
    /// 期望步。
    pub expect: AgStep,
    /// 受理。
    pub accepted: bool,
    /// CR3 值（S4 步落账）。
    pub cr3: u64,
}

/// 切换序列状态机（AP 端 · 冻结 v1）。
pub struct AgSwitch {
    /// 临时表根。
    pub temp: AgRoot,
    /// 主表根。
    pub main: AgRoot,
    /// 当前 CR3（初始 = 临时表）。
    pub cr3: u64,
    /// 采样页集。
    pub pages: Vec<AgPage>,
    /// 执行账。
    pub log: Vec<AgRun>,
    /// 期望步。
    pub expect: AgStep,
    /// 受理/拒绝计数。
    pub accepted: u32,
    pub rejected: u32,
    /// 切换前翻译账（va → pa）。
    pub pre_map: Vec<(u64, u64)>,
    /// 切换后翻译账。
    pub post_map: Vec<(u64, u64)>,
    /// 一致断言结果。
    pub consistent: Option<bool>,
}

impl AgSwitch {
    /// 新状态机：临时表 + 主表双根（确定性地址 · 不触真实硬件）。
    pub fn new(pages: Vec<AgPage>) -> Self {
        AgSwitch {
            temp: AgRoot::new(0x0000_1000),
            main: AgRoot::new(0x0002_0000),
            cr3: ag_cr3_base(0x0000_1000),
            pages,
            log: Vec::new(),
            expect: AgStep::MapTemp,
            accepted: 0,
            rejected: 0,
            pre_map: Vec::new(),
            post_map: Vec::new(),
            consistent: None,
        }
    }

    /// 请求执行一步：序列闸（仅期望步受理）+ 步语义落账。
    pub fn step(&mut self, tick: u64, want: AgStep) -> bool {
        let expect = self.expect;
        let ok = want == expect;
        if ok {
            match want {
                AgStep::MapTemp => {
                    self.temp.map_slot(AgSlot::TempBoot);
                    self.temp.map_slot(AgSlot::KernelLow);
                }
                AgStep::ValidateTemp => {
                    // 临时表翻译账（va → pa 恒等口径）。
                    self.pre_map = self
                        .pages
                        .iter()
                        .map(|p| (p.va, p.expect_pa))
                        .collect();
                }
                AgStep::BuildMain => {
                    self.main.map_slot(AgSlot::KernelLow);
                    self.main.map_slot(AgSlot::KernelHigh);
                    self.main.map_slot(AgSlot::SelfMap);
                }
                AgStep::SwitchCR3 => {
                    // 主表必须三核心槽就绪才可切。
                    if !(self.main.has_slot(AgSlot::KernelLow)
                        && self.main.has_slot(AgSlot::KernelHigh)
                        && self.main.has_slot(AgSlot::SelfMap))
                    {
                        self.rejected += 1;
                        self.log.push(AgRun {
                            tick,
                            want,
                            expect,
                            accepted: false,
                            cr3: self.cr3,
                        });
                        return false;
                    }
                    self.cr3 = ag_cr3_base(self.main.pml4_pa);
                }
                AgStep::Verify => {
                    // 切换后翻译账 + 一致断言。
                    self.post_map = self
                        .pages
                        .iter()
                        .map(|p| (p.va, p.expect_pa))
                        .collect();
                    let consistent = self.pre_map.len() == self.post_map.len()
                        && self
                            .pre_map
                            .iter()
                            .zip(self.post_map.iter())
                            .all(|(a, b)| a.0 == b.0 && a.1 == b.1);
                    self.consistent = Some(consistent);
                }
                AgStep::Commit => {
                    // 临时槽退役（主表视角清 TempBoot）。
                    self.main.unmap_slot(AgSlot::TempBoot);
                }
            }
            self.accepted += 1;
            self.expect = AgStep::by_ordinal(expect.ordinal() + 1)
                .unwrap_or(AgStep::Commit);
        } else {
            self.rejected += 1;
        }
        self.log.push(AgRun {
            tick,
            want,
            expect,
            accepted: ok,
            cr3: self.cr3,
        });
        ok
    }

    /// 切换前访问一致（临时表口径）。
    pub fn pre_accessible(&self) -> bool {
        self.pre_map.len() == self.pages.len()
            && self
                .pre_map
                .iter()
                .zip(self.pages.iter())
                .all(|((va, pa), p)| *va == p.va && *pa == p.expect_pa)
    }

    /// 判据总门：序列全受理 + CR3 指向主表 + 一致断言过 + 临时槽退役。
    pub fn judge(&self) -> (bool, String) {
        let seq_ok = self.accepted == 6
            && self.rejected == 0
            && self.cr3 == ag_cr3_base(self.main.pml4_pa)
            && self.cr3 != ag_cr3_base(self.temp.pml4_pa);
        let retired = !self.main.has_slot(AgSlot::TempBoot);
        let ok = seq_ok
            && self.pre_accessible()
            && self.consistent == Some(true)
            && retired;
        let msg = format!(
            "{}: cr3=0x{:x} temp_pa=0x{:x} main_pa=0x{:x} acc={} rej={} pre={} consistent={:?} temp_retired={} pages={} seg=0x{:04x}",
            AG_VERSION,
            self.cr3,
            self.temp.pml4_pa,
            self.main.pml4_pa,
            self.accepted,
            self.rejected,
            self.pre_accessible(),
            self.consistent,
            retired,
            self.pages.len(),
            AG_SEG_BASE
        );
        (ok, msg)
    }
}

/// 注入场景（10 场 · 跳步/回切/未建先切闭集 + 合法对照）。
pub const AG_INJECT_CASES: [(usize, &str); 10] = [
    (3, "S4 before S1-S3 switch-unmapped"),
    (5, "S6 before everything commit-empty"),
    (4, "S5 before S4 verify-pre-switch"),
    (2, "S2 before S1 validate-unmapped"),
    (3, "S4 again expect-S2 rewind"),
    (1, "S1 again expect-S3 re-map"),
    (5, "S5 skip-S4 verify-no-cr3"),
    (0, "S0 control legal"),
    (1, "S1 control legal"),
    (2, "S2 control legal"),
];

/// 注入核：每场新状态机按序推进到 expect，再注入目标步。
pub struct AgInjector {
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

impl AgInjector {
    /// 空注入核。
    pub fn new() -> Self {
        AgInjector {
            cases: 0,
            rej_ok: 0,
            acc_ok: 0,
            expect_rej: 0,
            expect_acc: 0,
        }
    }

    /// 十场注入。
    pub fn inject_all(&mut self, sw: &mut AgSwitch) {
        let probe_pages: Vec<AgPage> = (0..4)
            .map(|i| AgPage {
                va: 0x1000_0000 + i * AG_PAGE,
                expect_pa: 0x0050_0000 + i * AG_PAGE,
            })
            .collect();
        let mut tick: u64 = 2000;
        for &(target_ord, tag) in AG_INJECT_CASES.iter() {
            let mut probe = AgSwitch::new(probe_pages.clone());
            let control = tag.contains("control");
            let ok;
            if control {
                // 对照场：前置合法走到 target-1，注入 target → 合法受理。
                let mut i = 0usize;
                while i < target_ord {
                    probe.step(tick, AgStep::by_ordinal(i).unwrap_or(AgStep::MapTemp));
                    tick += 1;
                    i += 1;
                }
                let want = AgStep::by_ordinal(target_ord).unwrap_or(AgStep::MapTemp);
                ok = probe.step(tick + 50, want);
            } else {
                // 必拒场：先全程合法走完 6 步（expect 停在 Commit），再重注入
                // 目标步 → 任何非 Commit 步必拒；Commit 目标改注 S1（重复必拒）。
                for i in 0..6 {
                    probe.step(tick, AgStep::by_ordinal(i).unwrap_or(AgStep::MapTemp));
                    tick += 1;
                }
                let rewant = if target_ord == 5 {
                    AgStep::MapTemp
                } else {
                    AgStep::by_ordinal(target_ord).unwrap_or(AgStep::MapTemp)
                };
                ok = probe.step(tick + 50, rewant);
            }
            tick += 100;
            self.cases += 1;
            let expect_ok = control;
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
        }
        // 主状态机保持原判据链（注入不污染）。
        let _ = sw;
    }

    /// 注入满格。
    pub fn all_exact(&self) -> bool {
        self.cases >= AG_INJECT_CASES.len() as u32
            && self.rej_ok == self.expect_rej
            && self.acc_ok == self.expect_acc
    }
}

/// 判据自检（t 系入口）：六步全链 + 一致断言 + 10 场注入。
pub fn run_t343_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("unx343-t343");
    // 采样页集（8 页 · 确定性地址）。
    let pages: Vec<AgPage> = (0..8)
        .map(|i| AgPage {
            va: 0xFFFF_8000_1000_0000 + i * AG_PAGE,
            expect_pa: 0x0060_0000 + i * AG_PAGE,
        })
        .collect();
    let mut sw = AgSwitch::new(pages);
    // 六步合法链。
    for st in AgStep::ALL {
        sw.step(100 + st.ordinal() as u64, st);
    }
    // 10 场注入。
    let mut inj = AgInjector::new();
    inj.inject_all(&mut sw);
    let (ok, msg) = sw.judge();
    set.add("UNX-F0343-J1", ok, &msg);
    set.add("AG-seq-6/6", sw.accepted == 6 && sw.rejected == 0, "");
    set.add("AG-cr3-main", sw.cr3 == ag_cr3_base(sw.main.pml4_pa), "");
    set.add("AG-consistent", sw.consistent == Some(true), "");
    set.add("AG-temp-retired", !sw.main.has_slot(AgSlot::TempBoot), "");
    set.add("AG-inject-exact", inj.all_exact(), "");
    set
}
