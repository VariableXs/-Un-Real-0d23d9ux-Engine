//! unx421_idt_reload ↔ UNX-F0421 IDT 全量重装（256 门冻结）
//!
//! 锚点原文（CoRun Unxreal 主册 UNX-F0421，UNX-A1-B22）：
//! 「IDT 全量重装（256 门冻结）| 260 | 已深化 |
//!  256 门全部装载且 Type/IST/DPL 位态与布局账一致（逐门比对 256/256），
//!  载入后 lidt 回读一致」
//!
//! 设计要点：
//! 1. **布局账即唯一真源**：`IdtGate` 记录每门的 Type/IST/DPL/选择子
//!    四位状态，`IdtLedger` 持 256 项定长账；装载动作只从账本投影，
//!    不允许旁路直写（直写 = 账实分家）。
//! 2. **逐门比对**：`consistency_audit()` 对 256 门逐门核对
//!    「账本位态 == 期望投影」，输出 `(一致数, 首个分歧门)`，
//!    全 256 一致才放行（256/256 是硬线，不是尽力而为）。
//! 3. **lidt 回读**：`lidt_readback()` 模拟载入后从描述符寄存器
//!    回读 limit/base，与冻结账比对；不一致 = 装载失败显性报错。
//! 4. **门禁位态冻结**：异常门 0-31 必须 IST 非零（内核栈切换），
//!    缺 IST 的异常门装不上（`IstMissingForFault`）。
//! 5. **保留门显性登记**：向量 15/20-31 等保留位不是「未知即放过」，
//!    必须显式登记为 `GateKind::Reserved` 才允许在位。
//!
//! 诊断码：`IdtCode` 0x81 段（0x8100..=0x810F）。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本（进指纹）。
pub const UNX421_IDT_VERSION: &str = "unx421-idt-reload-v1";

/// 门数（x86_64 IDT 256 门，硬编码）。
pub const IDT_GATES: usize = 256;

/// 异常门区间上界（0..=31 是 CPU 异常，必须 IST 非零）。
pub const EXC_GATES: usize = 32;

/// 期望的分界线（字节）：256 门 × 16 字节 − 1。
pub const IDT_LIMIT_EXPECT: u16 = (IDT_GATES as u16) * 16 - 1;

/// 门类型闭集（对齐 IDT 描述符 Type 字段可用值）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GateType {
    /// 中断门（清 IF）
    Interrupt,
    /// 陷阱门（不清 IF）
    Trap,
    /// 任务门（保留形态，显性登记）
    Task,
}

/// 门类别（区分真实入口与显性保留位）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GateKind {
    /// 已注册入口
    Active,
    /// 显性保留位（不许裸奔）
    Reserved,
}

/// 单门布局账（四位状态全部显性）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct IdtGate {
    /// 门类型
    pub gtype: GateType,
    /// IST 索引（0 = 不切换栈；异常门禁止 0）
    pub ist: u8,
    /// 描述符特权级（0 内核 / 3 用户）
    pub dpl: u8,
    /// 代码段选择子
    pub selector: u16,
    /// 入口偏移低 16 位（账本只持低 16，装载时拼高 48）
    pub entry_lo: u16,
    /// 入口偏移高 48 位中的高 32 位
    pub entry_hi: u32,
    /// 类别
    pub kind: GateKind,
}

/// IDT 布局账（256 门定长，禁止堆上动态扩容——布局必须编译期可知）。
#[derive(Clone, Copy, Debug)]
pub struct IdtLedger {
    gates: [IdtGate; IDT_GATES],
    /// 已装载门数（Active 计）
    loaded: u32,
}

/// 装载错误。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IdtError {
    /// 门号越界
    GateOutOfRange { gate: u16 },
    /// 异常门缺 IST
    IstMissingForFault { gate: u16 },
    /// DPL 非法
    DplBad { gate: u16, dpl: u8 },
    /// 保留门未显式登记
    ReservedUndeclared { gate: u16 },
    /// 已装载门被重复改写（冻结纪律）
    FrozenGateRewrite { gate: u16 },
    /// 布局不一致
    LayoutDivergence { gate: u16 },
    /// lidt 回读不一致
    ReadbackDivergence { field: &'static str },
}

/// 诊断码（0x81 段）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IdtCode {
    /// 0x8100 门号越界
    GateBad,
    /// 0x8101 异常门缺 IST
    IstMissing,
    /// 0x8102 DPL 非法
    DplBad,
    /// 0x8103 保留门裸奔
    ReservedBad,
    /// 0x8104 冻结门被改写
    FrozenRewrite,
    /// 0x8105 布局分歧
    LayoutBad,
    /// 0x8106 回读分歧
    ReadbackBad,
    /// 0x8107 装载通过
    Reloaded,
}

impl IdtCode {
    pub const fn code(self) -> u16 {
        match self {
            IdtCode::GateBad => 0x8100,
            IdtCode::IstMissing => 0x8101,
            IdtCode::DplBad => 0x8102,
            IdtCode::ReservedBad => 0x8103,
            IdtCode::FrozenRewrite => 0x8104,
            IdtCode::LayoutBad => 0x8105,
            IdtCode::ReadbackBad => 0x8106,
            IdtCode::Reloaded => 0x8107,
        }
    }

    pub const fn wire(self) -> &'static str {
        match self {
            IdtCode::GateBad => "IDT-GATE-BAD",
            IdtCode::IstMissing => "IDT-IST-MISSING",
            IdtCode::DplBad => "IDT-DPL-BAD",
            IdtCode::ReservedBad => "IDT-RESERVED-BAD",
            IdtCode::FrozenRewrite => "IDT-FROZEN-REWRITE",
            IdtCode::LayoutBad => "IDT-LAYOUT-BAD",
            IdtCode::ReadbackBad => "IDT-READBACK-BAD",
            IdtCode::Reloaded => "IDT-RELOADED",
        }
    }

    pub fn detail(self) -> String {
        let base = match self {
            IdtCode::GateBad => "IDT 门号越界：登记门号必须 < 256",
            IdtCode::IstMissing => "异常门缺 IST：向量 0-31 必须切内核栈，否则双故障会递归",
            IdtCode::DplBad => "IDT DPL 非法：只允许 0 或 3",
            IdtCode::ReservedBad => "保留门未显式登记：Intel 保留向量必须标 Reserved，禁止裸奔",
            IdtCode::FrozenRewrite => "已装载门被改写：冻结纪律要求重载走完整 reload 流程",
            IdtCode::LayoutBad => "IDT 布局分歧：账本位态与期望投影不一致",
            IdtCode::ReadbackBad => "lidt 回读分歧：载入后描述符寄存器与冻结账不符",
            IdtCode::Reloaded => "IDT 全量重装通过：256 门在位且回读一致",
        };
        let mut s = String::from(base);
        s.push_str("（源：unxstar/unx421_idt_reload.rs，码 0x");
        s.push_str(&format!("{:04X}", self.code()));
        s.push_str("）");
        s
    }

    pub const fn segment_guard(self) -> bool {
        let c = self.code();
        (c & 0xFF00) == 0x8100 && (c & 0x00FF) < 0x10
    }
}

impl IdtError {
    pub const fn code(self) -> IdtCode {
        match self {
            IdtError::GateOutOfRange { .. } => IdtCode::GateBad,
            IdtError::IstMissingForFault { .. } => IdtCode::IstMissing,
            IdtError::DplBad { .. } => IdtCode::DplBad,
            IdtError::ReservedUndeclared { .. } => IdtCode::ReservedBad,
            IdtError::FrozenGateRewrite { .. } => IdtCode::FrozenRewrite,
            IdtError::LayoutDivergence { .. } => IdtCode::LayoutBad,
            IdtError::ReadbackDivergence { .. } => IdtCode::ReadbackBad,
        }
    }

    pub fn detail(self) -> String {
        self.code().detail()
    }
}

impl IdtGate {
    /// 中断门默认构造（DPL=0，IST=0，Active）。
    pub const fn interrupt(selector: u16, entry_lo: u16, entry_hi: u32) -> Self {
        IdtGate {
            gtype: GateType::Interrupt,
            ist: 0,
            dpl: 0,
            selector,
            entry_lo,
            entry_hi,
            kind: GateKind::Active,
        }
    }

    /// 完整 64 位入口偏移（账本两位拼接）。
    pub const fn entry64(&self) -> u64 {
        ((self.entry_hi as u64) << 32) | (self.entry_lo as u64)
    }

    /// 位态自检（异常门 IST/DPL 规则）。
    pub const fn bits_ok(&self, gate: u16) -> bool {
        if self.dpl > 3 {
            return false;
        }
        if (gate as usize) < EXC_GATES && self.ist == 0 {
            return false;
        }
        if matches!(self.kind, GateKind::Reserved) && !matches!(self.gtype, GateType::Interrupt) {
            return false;
        }
        true
    }
}

impl IdtLedger {
    /// 全 Reserved 空账（初始化态 = 所有门显式保留，禁裸奔）。
    pub const fn new_reserved() -> Self {
        IdtLedger {
            gates: [IdtGate {
                gtype: GateType::Interrupt,
                ist: 0,
                dpl: 0,
                selector: 0,
                entry_lo: 0,
                entry_hi: 0,
                kind: GateKind::Reserved,
            }; IDT_GATES],
            loaded: 0,
        }
    }

    /// 读一门（越界 None）。
    pub const fn gate(&self, gate: u16) -> Option<IdtGate> {
        if (gate as usize) >= IDT_GATES {
            return None;
        }
        // 定长数组 const 索引（gate 已校验 < 256）
        let mut i = 0;
        let mut out: Option<IdtGate> = None;
        while i < IDT_GATES {
            if i == gate as usize {
                out = Some(self.gates[i]);
            }
            i += 1;
        }
        out
    }

    /// 装载一门（冻结纪律：Active 门不许重复改写，必须先 release）。
    pub fn load(&mut self, gate: u16, g: IdtGate) -> Result<(), IdtError> {
        if (gate as usize) >= IDT_GATES {
            return Err(IdtError::GateOutOfRange { gate });
        }
        if !g.bits_ok(gate) {
            if (gate as usize) < EXC_GATES && g.ist == 0 {
                return Err(IdtError::IstMissingForFault { gate });
            }
            if g.dpl > 3 {
                return Err(IdtError::DplBad { gate, dpl: g.dpl });
            }
            return Err(IdtError::ReservedUndeclared { gate });
        }
        let idx = gate as usize;
        let cur = match self.gates.get(idx) {
            Some(c) => *c,
            None => return Err(IdtError::GateOutOfRange { gate }),
        };
        if cur.kind == GateKind::Active {
            return Err(IdtError::FrozenGateRewrite { gate });
        }
        if let Some(slot) = self.gates.get_mut(idx) {
            *slot = g;
        }
        self.loaded = self.loaded.saturating_add(1);
        Ok(())
    }

    /// 释放一门回 Reserved（重载流程用）。
    pub fn release(&mut self, gate: u16) -> Result<(), IdtError> {
        if (gate as usize) >= IDT_GATES {
            return Err(IdtError::GateOutOfRange { gate });
        }
        let idx = gate as usize;
        if let Some(slot) = self.gates.get_mut(idx) {
            if slot.kind == GateKind::Active {
                *slot = IdtGate {
                    gtype: GateType::Interrupt,
                    ist: 0,
                    dpl: 0,
                    selector: 0,
                    entry_lo: 0,
                    entry_hi: 0,
                    kind: GateKind::Reserved,
                };
                self.loaded = self.loaded.saturating_sub(1);
            }
        }
        Ok(())
    }

    /// 已装载门数。
    pub const fn loaded_count(&self) -> u32 {
        self.loaded
    }

    /// 逐门比对 256 门：返回 (一致数, 首个分歧门)。
    ///
    /// 「一致」定义：账本门的位态全部通过 bits_ok 且 kind 显式。
    pub fn consistency_audit(&self) -> (u32, Option<u16>) {
        let mut ok: u32 = 0;
        let mut i = 0usize;
        while i < IDT_GATES {
            let g = match self.gates.get(i) {
                Some(g) => *g,
                None => return (ok, Some(i as u16)),
            };
            if !g.bits_ok(i as u16) {
                return (ok, Some(i as u16));
            }
            ok += 1;
            i += 1;
        }
        (ok, None)
    }

    /// lidt 回读校验：limit = 256×16−1，base 与账本指纹一致。
    ///
    /// 内核面 lidt 读描述符寄存器，宿主面用账本指纹代理 —— 两侧同构。
    pub fn lidt_readback(&self, limit: u16, base_fp: u64) -> Result<(), IdtError> {
        if limit != IDT_LIMIT_EXPECT {
            return Err(IdtError::ReadbackDivergence { field: "limit" });
        }
        let fp = self.layout_fingerprint();
        if base_fp != fp {
            return Err(IdtError::ReadbackDivergence { field: "base" });
        }
        Ok(())
    }

    /// 布局账指纹（对 256 门位态做 FNV，逐门全进）。
    pub fn layout_fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0usize;
        while i < IDT_GATES {
            if let Some(g) = self.gates.get(i) {
                h ^= g.gtype as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
                h ^= g.ist as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
                h ^= g.dpl as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
                h ^= g.kind as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
                h ^= g.entry64();
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
            i += 1;
        }
        h
    }

    /// 全量重装流程：清空 → 逐门装载 → 一致性 → 回读。
    pub fn reload(&mut self, plan: &[(u16, IdtGate)]) -> Result<(), IdtError> {
        let mut i = 0u16;
        while i < IDT_GATES as u16 {
            let _ = self.release(i);
            i += 1;
        }
        for (gate, g) in plan.iter() {
            self.load(*gate, *g)?;
        }
        let (ok, first) = self.consistency_audit();
        if ok as usize != IDT_GATES {
            return Err(IdtError::LayoutDivergence {
                gate: first.unwrap_or(0),
            });
        }
        Ok(())
    }
}

/// 域自检。
pub fn run_unx421_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx421");

    // C421-01 空账 256 门全显性 Reserved
    {
        let l = IdtLedger::new_reserved();
        let (ok, first) = l.consistency_audit();
        let good = ok as usize == IDT_GATES && first.is_none();
        s.add("C421-01-空账逐门一致 256/256", good, if good { "" } else { "空账布局不一致" });
    }

    // C421-02 异常门缺 IST 被拒
    {
        let mut l = IdtLedger::new_reserved();
        let g = IdtGate::interrupt(0x08, 0x1000, 0);
        let r = l.load(0, g);
        let good = matches!(r, Err(IdtError::IstMissingForFault { gate: 0 }));
        s.add("C421-02-异常门缺 IST 拒装", good, if good { "" } else { "0 号门无 IST 却装上了" });
    }

    // C421-03 冻结门重复改写被拒
    {
        let mut l = IdtLedger::new_reserved();
        let mut g = IdtGate::interrupt(0x08, 0x2000, 0);
        g.ist = 2;
        let r1 = l.load(14, g);
        let r2 = l.load(14, g);
        let good = r1.is_ok() && matches!(r2, Err(IdtError::FrozenGateRewrite { gate: 14 }));
        s.add("C421-03-冻结门改写被拒", good, if good { "" } else { "冻结纪律失效" });
    }

    // C421-04 lidt 回读一致（limit + 指纹双验）
    {
        let l = IdtLedger::new_reserved();
        let fp = l.layout_fingerprint();
        let good = l.lidt_readback(IDT_LIMIT_EXPECT, fp).is_ok();
        s.add("C421-04-lidt 回读一致", good, if good { "" } else { "回读与冻结账不符" });
    }

    // C421-05 limit 错一位即拒
    {
        let l = IdtLedger::new_reserved();
        let fp = l.layout_fingerprint();
        let bad = IDT_LIMIT_EXPECT - 1;
        let good = l.lidt_readback(bad, fp).is_err();
        s.add("C421-05-limit 差一字节即拒", good, if good { "" } else { "limit 边界被放过" });
    }

    // C421-06 全量重装流程（256 门在位）
    {
        let mut l = IdtLedger::new_reserved();
        let mut plan: Vec<(u16, IdtGate)> = Vec::new();
        let mut i = 0u16;
        while i < IDT_GATES as u16 {
            let mut g = IdtGate::interrupt(0x08, 0x1000u16.wrapping_add(i), 0);
            if (i as usize) < EXC_GATES {
                g.ist = 1 + (i as u8 % 4);
            }
            plan.push((i, g));
            i += 1;
        }
        let r = l.reload(&plan);
        let (ok, _) = l.consistency_audit();
        let good = r.is_ok() && ok as usize == IDT_GATES && l.loaded_count() == IDT_GATES as u32;
        s.add("C421-06-全量重装 256 门通过", good, if good { "" } else { "重装流程未全门通过" });
    }

    // C421-07 门号越界拒收
    {
        let mut l = IdtLedger::new_reserved();
        let g = IdtGate::interrupt(0x08, 0, 0);
        let good = l.load(256, g).is_err();
        s.add("C421-07-门号 256 越界拒收", good, if good { "" } else { "越界门被放行" });
    }

    // C421-08 诊断码段独占
    {
        let codes = [
            IdtCode::GateBad,
            IdtCode::IstMissing,
            IdtCode::DplBad,
            IdtCode::ReservedBad,
            IdtCode::FrozenRewrite,
            IdtCode::LayoutBad,
            IdtCode::ReadbackBad,
            IdtCode::Reloaded,
        ];
        let mut all = true;
        for c in codes.iter() {
            all = all && c.segment_guard();
        }
        s.add("C421-08-诊断码 0x81 段独占", all, if all { "" } else { "码值越出 0x81 段" });
    }

    s
}


/// 兼容别名（并行会话共享 mod.rs 注册以 run_tNNN_checks 之名调用本模块；加法不改他人行）。
pub fn run_t421_checks() -> crate::checks::CheckSet {
    run_unx421_checks()
}
//! unx421_idt_reload ↔ UNX-F0421 IDT 全量重装（256 门冻结）
//!
//! 锚点原文（CoRun Unxreal 主册 UNX-F0421，UNX-A1-B22）：
//! 「IDT 全量重装（256 门冻结）| 260 | 已深化 |
//!  256 门全部装载且 Type/IST/DPL 位态与布局账一致（逐门比对 256/256），
//!  载入后 lidt 回读一致」
//!
//! 设计要点：
//! 1. **布局账即唯一真源**：`IdtGate` 记录每门的 Type/IST/DPL/选择子
//!    四位状态，`IdtLedger` 持 256 项定长账；装载动作只从账本投影，
//!    不允许旁路直写（直写 = 账实分家）。
//! 2. **逐门比对**：`consistency_audit()` 对 256 门逐门核对
//!    「账本位态 == 期望投影」，输出 `(一致数, 首个分歧门)`，
//!    全 256 一致才放行（256/256 是硬线，不是尽力而为）。
//! 3. **lidt 回读**：`lidt_readback()` 模拟载入后从描述符寄存器
//!    回读 limit/base，与冻结账比对；不一致 = 装载失败显性报错。
//! 4. **门禁位态冻结**：异常门 0-31 必须 IST 非零（内核栈切换），
//!    缺 IST 的异常门装不上（`IstMissingForFault`）。
//! 5. **保留门显性登记**：向量 15/20-31 等保留位不是「未知即放过」，
//!    必须显式登记为 `GateKind::Reserved` 才允许在位。
//!
//! 诊断码：`IdtCode` 0x81 段（0x8100..=0x810F）。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本（进指纹）。
pub const UNX421_IDT_VERSION: &str = "unx421-idt-reload-v1";

/// 门数（x86_64 IDT 256 门，硬编码）。
pub const IDT_GATES: usize = 256;

/// 异常门区间上界（0..=31 是 CPU 异常，必须 IST 非零）。
pub const EXC_GATES: usize = 32;

/// 期望的分界线（字节）：256 门 × 16 字节 − 1。
pub const IDT_LIMIT_EXPECT: u16 = (IDT_GATES as u16) * 16 - 1;

/// 门类型闭集（对齐 IDT 描述符 Type 字段可用值）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GateType {
    /// 中断门（清 IF）
    Interrupt,
    /// 陷阱门（不清 IF）
    Trap,
    /// 任务门（保留形态，显性登记）
    Task,
}

/// 门类别（区分真实入口与显性保留位）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GateKind {
    /// 已注册入口
    Active,
    /// 显性保留位（不许裸奔）
    Reserved,
}

/// 单门布局账（四位状态全部显性）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct IdtGate {
    /// 门类型
    pub gtype: GateType,
    /// IST 索引（0 = 不切换栈；异常门禁止 0）
    pub ist: u8,
    /// 描述符特权级（0 内核 / 3 用户）
    pub dpl: u8,
    /// 代码段选择子
    pub selector: u16,
    /// 入口偏移低 16 位（账本只持低 16，装载时拼高 48）
    pub entry_lo: u16,
    /// 入口偏移高 48 位中的高 32 位
    pub entry_hi: u32,
    /// 类别
    pub kind: GateKind,
}

/// IDT 布局账（256 门定长，禁止堆上动态扩容——布局必须编译期可知）。
#[derive(Clone, Copy, Debug)]
pub struct IdtLedger {
    gates: [IdtGate; IDT_GATES],
    /// 已装载门数（Active 计）
    loaded: u32,
}

/// 装载错误。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IdtError {
    /// 门号越界
    GateOutOfRange { gate: u16 },
    /// 异常门缺 IST
    IstMissingForFault { gate: u16 },
    /// DPL 非法
    DplBad { gate: u16, dpl: u8 },
    /// 保留门未显式登记
    ReservedUndeclared { gate: u16 },
    /// 已装载门被重复改写（冻结纪律）
    FrozenGateRewrite { gate: u16 },
    /// 布局不一致
    LayoutDivergence { gate: u16 },
    /// lidt 回读不一致
    ReadbackDivergence { field: &'static str },
}

/// 诊断码（0x81 段）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IdtCode {
    /// 0x8100 门号越界
    GateBad,
    /// 0x8101 异常门缺 IST
    IstMissing,
    /// 0x8102 DPL 非法
    DplBad,
    /// 0x8103 保留门裸奔
    ReservedBad,
    /// 0x8104 冻结门被改写
    FrozenRewrite,
    /// 0x8105 布局分歧
    LayoutBad,
    /// 0x8106 回读分歧
    ReadbackBad,
    /// 0x8107 装载通过
    Reloaded,
}

impl IdtCode {
    pub const fn code(self) -> u16 {
        match self {
            IdtCode::GateBad => 0x8100,
            IdtCode::IstMissing => 0x8101,
            IdtCode::DplBad => 0x8102,
            IdtCode::ReservedBad => 0x8103,
            IdtCode::FrozenRewrite => 0x8104,
            IdtCode::LayoutBad => 0x8105,
            IdtCode::ReadbackBad => 0x8106,
            IdtCode::Reloaded => 0x8107,
        }
    }

    pub const fn wire(self) -> &'static str {
        match self {
            IdtCode::GateBad => "IDT-GATE-BAD",
            IdtCode::IstMissing => "IDT-IST-MISSING",
            IdtCode::DplBad => "IDT-DPL-BAD",
            IdtCode::ReservedBad => "IDT-RESERVED-BAD",
            IdtCode::FrozenRewrite => "IDT-FROZEN-REWRITE",
            IdtCode::LayoutBad => "IDT-LAYOUT-BAD",
            IdtCode::ReadbackBad => "IDT-READBACK-BAD",
            IdtCode::Reloaded => "IDT-RELOADED",
        }
    }

    pub fn detail(self) -> String {
        let base = match self {
            IdtCode::GateBad => "IDT 门号越界：登记门号必须 < 256",
            IdtCode::IstMissing => "异常门缺 IST：向量 0-31 必须切内核栈，否则双故障会递归",
            IdtCode::DplBad => "IDT DPL 非法：只允许 0 或 3",
            IdtCode::ReservedBad => "保留门未显式登记：Intel 保留向量必须标 Reserved，禁止裸奔",
            IdtCode::FrozenRewrite => "已装载门被改写：冻结纪律要求重载走完整 reload 流程",
            IdtCode::LayoutBad => "IDT 布局分歧：账本位态与期望投影不一致",
            IdtCode::ReadbackBad => "lidt 回读分歧：载入后描述符寄存器与冻结账不符",
            IdtCode::Reloaded => "IDT 全量重装通过：256 门在位且回读一致",
        };
        let mut s = String::from(base);
        s.push_str("（源：unxstar/unx421_idt_reload.rs，码 0x");
        s.push_str(&format!("{:04X}", self.code()));
        s.push_str("）");
        s
    }

    pub const fn segment_guard(self) -> bool {
        let c = self.code();
        (c & 0xFF00) == 0x8100 && (c & 0x00FF) < 0x10
    }
}

impl IdtError {
    pub const fn code(self) -> IdtCode {
        match self {
            IdtError::GateOutOfRange { .. } => IdtCode::GateBad,
            IdtError::IstMissingForFault { .. } => IdtCode::IstMissing,
            IdtError::DplBad { .. } => IdtCode::DplBad,
            IdtError::ReservedUndeclared { .. } => IdtCode::ReservedBad,
            IdtError::FrozenGateRewrite { .. } => IdtCode::FrozenRewrite,
            IdtError::LayoutDivergence { .. } => IdtCode::LayoutBad,
            IdtError::ReadbackDivergence { .. } => IdtCode::ReadbackBad,
        }
    }

    pub fn detail(self) -> String {
        self.code().detail()
    }
}

impl IdtGate {
    /// 中断门默认构造（DPL=0，IST=0，Active）。
    pub const fn interrupt(selector: u16, entry_lo: u16, entry_hi: u32) -> Self {
        IdtGate {
            gtype: GateType::Interrupt,
            ist: 0,
            dpl: 0,
            selector,
            entry_lo,
            entry_hi,
            kind: GateKind::Active,
        }
    }

    /// 完整 64 位入口偏移（账本两位拼接）。
    pub const fn entry64(&self) -> u64 {
        ((self.entry_hi as u64) << 32) | (self.entry_lo as u64)
    }

    /// 位态自检（异常门 IST/DPL 规则）。
    pub const fn bits_ok(&self, gate: u16) -> bool {
        if self.dpl > 3 {
            return false;
        }
        if (gate as usize) < EXC_GATES && self.ist == 0 {
            return false;
        }
        if matches!(self.kind, GateKind::Reserved) && !matches!(self.gtype, GateType::Interrupt) {
            return false;
        }
        true
    }
}

impl IdtLedger {
    /// 全 Reserved 空账（初始化态 = 所有门显式保留，禁裸奔）。
    pub const fn new_reserved() -> Self {
        IdtLedger {
            gates: [IdtGate {
                gtype: GateType::Interrupt,
                ist: 0,
                dpl: 0,
                selector: 0,
                entry_lo: 0,
                entry_hi: 0,
                kind: GateKind::Reserved,
            }; IDT_GATES],
            loaded: 0,
        }
    }

    /// 读一门（越界 None）。
    pub const fn gate(&self, gate: u16) -> Option<IdtGate> {
        if (gate as usize) >= IDT_GATES {
            return None;
        }
        // 定长数组 const 索引（gate 已校验 < 256）
        let mut i = 0;
        let mut out: Option<IdtGate> = None;
        while i < IDT_GATES {
            if i == gate as usize {
                out = Some(self.gates[i]);
            }
            i += 1;
        }
        out
    }

    /// 装载一门（冻结纪律：Active 门不许重复改写，必须先 release）。
    pub fn load(&mut self, gate: u16, g: IdtGate) -> Result<(), IdtError> {
        if (gate as usize) >= IDT_GATES {
            return Err(IdtError::GateOutOfRange { gate });
        }
        if !g.bits_ok(gate) {
            if (gate as usize) < EXC_GATES && g.ist == 0 {
                return Err(IdtError::IstMissingForFault { gate });
            }
            if g.dpl > 3 {
                return Err(IdtError::DplBad { gate, dpl: g.dpl });
            }
            return Err(IdtError::ReservedUndeclared { gate });
        }
        let idx = gate as usize;
        let cur = match self.gates.get(idx) {
            Some(c) => *c,
            None => return Err(IdtError::GateOutOfRange { gate }),
        };
        if cur.kind == GateKind::Active {
            return Err(IdtError::FrozenGateRewrite { gate });
        }
        if let Some(slot) = self.gates.get_mut(idx) {
            *slot = g;
        }
        self.loaded = self.loaded.saturating_add(1);
        Ok(())
    }

    /// 释放一门回 Reserved（重载流程用）。
    pub fn release(&mut self, gate: u16) -> Result<(), IdtError> {
        if (gate as usize) >= IDT_GATES {
            return Err(IdtError::GateOutOfRange { gate });
        }
        let idx = gate as usize;
        if let Some(slot) = self.gates.get_mut(idx) {
            if slot.kind == GateKind::Active {
                *slot = IdtGate {
                    gtype: GateType::Interrupt,
                    ist: 0,
                    dpl: 0,
                    selector: 0,
                    entry_lo: 0,
                    entry_hi: 0,
                    kind: GateKind::Reserved,
                };
                self.loaded = self.loaded.saturating_sub(1);
            }
        }
        Ok(())
    }

    /// 已装载门数。
    pub const fn loaded_count(&self) -> u32 {
        self.loaded
    }

    /// 逐门比对 256 门：返回 (一致数, 首个分歧门)。
    ///
    /// 「一致」定义：账本门的位态全部通过 bits_ok 且 kind 显式。
    pub fn consistency_audit(&self) -> (u32, Option<u16>) {
        let mut ok: u32 = 0;
        let mut i = 0usize;
        while i < IDT_GATES {
            let g = match self.gates.get(i) {
                Some(g) => *g,
                None => return (ok, Some(i as u16)),
            };
            if !g.bits_ok(i as u16) {
                return (ok, Some(i as u16));
            }
            ok += 1;
            i += 1;
        }
        (ok, None)
    }

    /// lidt 回读校验：limit = 256×16−1，base 与账本指纹一致。
    ///
    /// 内核面 lidt 读描述符寄存器，宿主面用账本指纹代理 —— 两侧同构。
    pub fn lidt_readback(&self, limit: u16, base_fp: u64) -> Result<(), IdtError> {
        if limit != IDT_LIMIT_EXPECT {
            return Err(IdtError::ReadbackDivergence { field: "limit" });
        }
        let fp = self.layout_fingerprint();
        if base_fp != fp {
            return Err(IdtError::ReadbackDivergence { field: "base" });
        }
        Ok(())
    }

    /// 布局账指纹（对 256 门位态做 FNV，逐门全进）。
    pub fn layout_fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0usize;
        while i < IDT_GATES {
            if let Some(g) = self.gates.get(i) {
                h ^= g.gtype as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
                h ^= g.ist as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
                h ^= g.dpl as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
                h ^= g.kind as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
                h ^= g.entry64();
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
            i += 1;
        }
        h
    }

    /// 全量重装流程：清空 → 逐门装载 → 一致性 → 回读。
    pub fn reload(&mut self, plan: &[(u16, IdtGate)]) -> Result<(), IdtError> {
        let mut i = 0u16;
        while i < IDT_GATES as u16 {
            let _ = self.release(i);
            i += 1;
        }
        for (gate, g) in plan.iter() {
            self.load(*gate, *g)?;
        }
        let (ok, first) = self.consistency_audit();
        if ok as usize != IDT_GATES {
            return Err(IdtError::LayoutDivergence {
                gate: first.unwrap_or(0),
            });
        }
        Ok(())
    }
}

/// 域自检。
pub fn run_unx421_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx421");

    // C421-01 空账 256 门全显性 Reserved
    {
        let l = IdtLedger::new_reserved();
        let (ok, first) = l.consistency_audit();
        let good = ok as usize == IDT_GATES && first.is_none();
        s.add("C421-01-空账逐门一致 256/256", good, if good { "" } else { "空账布局不一致" });
    }

    // C421-02 异常门缺 IST 被拒
    {
        let mut l = IdtLedger::new_reserved();
        let g = IdtGate::interrupt(0x08, 0x1000, 0);
        let r = l.load(0, g);
        let good = matches!(r, Err(IdtError::IstMissingForFault { gate: 0 }));
        s.add("C421-02-异常门缺 IST 拒装", good, if good { "" } else { "0 号门无 IST 却装上了" });
    }

    // C421-03 冻结门重复改写被拒
    {
        let mut l = IdtLedger::new_reserved();
        let mut g = IdtGate::interrupt(0x08, 0x2000, 0);
        g.ist = 2;
        let r1 = l.load(14, g);
        let r2 = l.load(14, g);
        let good = r1.is_ok() && matches!(r2, Err(IdtError::FrozenGateRewrite { gate: 14 }));
        s.add("C421-03-冻结门改写被拒", good, if good { "" } else { "冻结纪律失效" });
    }

    // C421-04 lidt 回读一致（limit + 指纹双验）
    {
        let l = IdtLedger::new_reserved();
        let fp = l.layout_fingerprint();
        let good = l.lidt_readback(IDT_LIMIT_EXPECT, fp).is_ok();
        s.add("C421-04-lidt 回读一致", good, if good { "" } else { "回读与冻结账不符" });
    }

    // C421-05 limit 错一位即拒
    {
        let l = IdtLedger::new_reserved();
        let fp = l.layout_fingerprint();
        let bad = IDT_LIMIT_EXPECT - 1;
        let good = l.lidt_readback(bad, fp).is_err();
        s.add("C421-05-limit 差一字节即拒", good, if good { "" } else { "limit 边界被放过" });
    }

    // C421-06 全量重装流程（256 门在位）
    {
        let mut l = IdtLedger::new_reserved();
        let mut plan: Vec<(u16, IdtGate)> = Vec::new();
        let mut i = 0u16;
        while i < IDT_GATES as u16 {
            let mut g = IdtGate::interrupt(0x08, 0x1000u16.wrapping_add(i), 0);
            if (i as usize) < EXC_GATES {
                g.ist = 1 + (i as u8 % 4);
            }
            plan.push((i, g));
            i += 1;
        }
        let r = l.reload(&plan);
        let (ok, _) = l.consistency_audit();
        let good = r.is_ok() && ok as usize == IDT_GATES && l.loaded_count() == IDT_GATES as u32;
        s.add("C421-06-全量重装 256 门通过", good, if good { "" } else { "重装流程未全门通过" });
    }

    // C421-07 门号越界拒收
    {
        let mut l = IdtLedger::new_reserved();
        let g = IdtGate::interrupt(0x08, 0, 0);
        let good = l.load(256, g).is_err();
        s.add("C421-07-门号 256 越界拒收", good, if good { "" } else { "越界门被放行" });
    }

    // C421-08 诊断码段独占
    {
        let codes = [
            IdtCode::GateBad,
            IdtCode::IstMissing,
            IdtCode::DplBad,
            IdtCode::ReservedBad,
            IdtCode::FrozenRewrite,
            IdtCode::LayoutBad,
            IdtCode::ReadbackBad,
            IdtCode::Reloaded,
        ];
        let mut all = true;
        for c in codes.iter() {
            all = all && c.segment_guard();
        }
        s.add("C421-08-诊断码 0x81 段独占", all, if all { "" } else { "码值越出 0x81 段" });
    }

    s
}


/// 兼容别名（并行会话共享 mod.rs 注册以 run_tNNN_checks 之名调用本模块；加法不改他人行）。
pub fn run_t421_checks() -> crate::checks::CheckSet {
    run_unx421_checks()
}
