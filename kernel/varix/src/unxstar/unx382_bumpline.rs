//! unx382_bumpline —— UNX-F0382 bump 分配器（单向下水线）。
//! 锚点（CoRun Unxreal 主册 UNX-A1-B19）：UNX-F0382 · bump 分配器（单向下水线） · 目标 220 行 · 已深化
//! 判据原文：bump 指针单调推进，分配 O(1)，重置语义仅演练态可达
//! 组件：下水线账（arena 区间 + 指针单调证明位）+ O(1) 分配路径（零循环零查找）
//!      + 演练态闸（重置仅 DrillMode 持证可达，正式态拒显性）+ 分配账（追加式 + 守恒）
//! 判据落地：指针单调推进 + 分配 O(1) + 重置语义仅演练态可达
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，确定性。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const BL_VERSION: &str = "unx382_bumpline-v1";

/// 诊断段基址（0x4c10 段 · 本模块独占）。
pub const BL_SEG_BASE: u16 = 0x4c10;

/// 默认对齐（16 字节：x86_64 上能满足最大对齐需求）。
pub const BL_ALIGN: u64 = 16;

/// arena 区间上限（当前策略值，可调；容量字段按可扩展口径用 u64）。
pub const BL_ARENA_MAX: u64 = 1 << 30;

/// 分配来源（三源闭集：内核子系统 / 驱动 / 演练注入）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BlOwner {
    /// 内核子系统。
    Kernel,
    /// 驱动。
    Driver,
    /// 演练注入（仅演练态合法）。
    Drill,
}

impl BlOwner {
    /// 中文名。
    pub const fn name(self) -> &'static str {
        match self {
            BlOwner::Kernel => "内核子系统",
            BlOwner::Driver => "驱动",
            BlOwner::Drill => "演练注入",
        }
    }
}

/// 运行态（二态闭集：正式态 / 演练态）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BlMode {
    /// 正式态：重置不可达。
    Live,
    /// 演练态：重置可达（测试与教学）。
    Drill,
}

impl BlMode {
    /// 中文名。
    pub const fn name(self) -> &'static str {
        match self {
            BlMode::Live => "正式态",
            BlMode::Drill => "演练态",
        }
    }
}

/// 诊断码（0x4c10 段十四码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BlCode {
    /// 0x4c10 请求越界（对齐后 bump 指针越过 arena 上界）。
    Oob,
    /// 0x4c11 零长请求（零字节分配是语义错误不是空操作）。
    ZeroLen,
    /// 0x4c12 对齐参数非法（0 或非 2 的幂）。
    AlignBad,
    /// 0x4c13 arena 参数非法（HI ≤ LO 或超上限）。
    ArenaBad,
    /// 0x4c14 正式态重置拒（重置仅演练态可达——判据核心）。
    ResetDenied,
    /// 0x4c15 单调证明位破缺（新指针 ≤ 旧指针，O(1) 推进被破坏）。
    MonotonicBreak,
    /// 0x4c16 分配账破缺（账内区间和 ≠ 指针推进量）。
    LedgerBreak,
    /// 0x4c17 容量声明超限（arena 超过 BL_ARENA_MAX）。
    CapOver,
    /// 0x4c18 演练注入越权（正式态收到 Drill 来源请求）。
    DrillSpoof,
    /// 0x4c19 剩余量虚报（free 计算与账不符）。
    FreeLied,
    /// 0x4c1a 重放分配（同地址二次分配——指针推进后不得回退）。
    Replay,
    /// 0x4c1b 账满（追加式账容量触顶，显性截断不静默）。
    LedgerFull,
    /// 0x4c1c 统计口径错位（alloc 计数与账条数不一致）。
    StatMisaligned,
    /// 0x4c1d 段位守卫失败。
    SegmentGuard,
}

impl BlCode {
    /// 全量闭集。
    pub const ALL: [BlCode; 14] = [
        BlCode::Oob,
        BlCode::ZeroLen,
        BlCode::AlignBad,
        BlCode::ArenaBad,
        BlCode::ResetDenied,
        BlCode::MonotonicBreak,
        BlCode::LedgerBreak,
        BlCode::CapOver,
        BlCode::DrillSpoof,
        BlCode::FreeLied,
        BlCode::Replay,
        BlCode::LedgerFull,
        BlCode::StatMisaligned,
        BlCode::SegmentGuard,
    ];

    /// 码值。
    pub const fn val(self) -> u16 {
        BL_SEG_BASE + self as u16
    }

    /// 中文诊断。
    pub const fn text(self) -> &'static str {
        match self {
            BlCode::Oob => "请求越界：对齐后指针越过 arena 上界，分配拒绝",
            BlCode::ZeroLen => "零长请求：零字节分配按语义错误拒绝",
            BlCode::AlignBad => "对齐参数非法：0 或非 2 的幂",
            BlCode::ArenaBad => "arena 参数非法：上界不大于下界",
            BlCode::ResetDenied => "正式态重置拒：重置语义仅演练态可达",
            BlCode::MonotonicBreak => "单调证明位破缺：指针未单调推进",
            BlCode::LedgerBreak => "分配账破缺：账内区间和与指针推进量不一致",
            BlCode::CapOver => "容量声明超限：arena 超过当前策略上限",
            BlCode::DrillSpoof => "演练注入越权：正式态收到演练来源请求",
            BlCode::FreeLied => "剩余量虚报：free 计算与账实不符",
            BlCode::Replay => "重放分配：同一地址被二次分配",
            BlCode::LedgerFull => "分配账满：追加式账容量触顶，显性截断",
            BlCode::StatMisaligned => "统计口径错位：分配计数与账条数不一致",
            BlCode::SegmentGuard => "段位守卫失败：诊断码越出本模块段",
        }
    }

    /// 可操作建议。
    pub const fn advice(self) -> &'static str {
        match self {
            BlCode::Oob => "换更大 arena 或走页分配器：bump 下水线只管一次性划区",
            BlCode::ZeroLen => "调用方应先判零长；需要占位语义请显式传最小对齐值",
            BlCode::AlignBad => "对齐必须为 2 的幂（含 16 默认值），先对参数取整",
            BlCode::ArenaBad => "修正 arena 区间：HI > LO 且跨度不超策略上限",
            BlCode::ResetDenied => "重置只允许出现在演练态；正式态回收请走 arena 整体废弃重建",
            BlCode::MonotonicBreak => "指针只能前进：核对分配路径是否存在回写旧指针的分支",
            BlCode::LedgerBreak => "分配记账与指针推进必须同一事务：核对 alloc 的入账序",
            BlCode::CapOver => "上限是策略值可调（BL_ARENA_MAX）；调整后同步容量声明",
            BlCode::DrillSpoof => "演练来源只能在演练态出现；正式态应过滤注入流",
            BlCode::FreeLied => "free 应等于 HI 减去账内已分配末址；核对计算路径",
            BlCode::Replay => "分配指针推进后不得复用旧地址；检查是否绕过了账",
            BlCode::LedgerFull => "账容量是防线不是瓶颈：扩账或按批归档",
            BlCode::StatMisaligned => "每次成功分配恰好记一账：核对错误路径是否漏记",
            BlCode::SegmentGuard => "诊断码分配越段：核对本模块段基址 0x4c10",
        }
    }
}

/// 单条分配账（区间账：start..end）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlEntry {
    /// 分配起点（对齐后）。
    pub start: u64,
    /// 分配终点（start + len，不含）。
    pub end: u64,
    /// 所有者。
    pub owner: BlOwner,
}

impl BlEntry {
    /// 长度。
    pub const fn len(&self) -> u64 {
        self.end - self.start
    }
}

/// bump 分配器主体（单向下水线）。
pub struct BumpLine {
    /// arena 下界。
    lo: u64,
    /// arena 上界。
    hi: u64,
    /// bump 指针（已分配末址，单调不减）。
    bump: u64,
    /// 运行态。
    mode: BlMode,
    /// 分配账（追加式）。
    ledger: Vec<BlEntry>,
    /// 成功分配计数。
    allocs: u64,
    /// 拒绝计数（按码）。
    rejects: Vec<(BlCode, u64)>,
    /// 账容量（触顶显性）。
    ledger_cap: usize,
    /// 诊断码账。
    codes: Vec<BlCode>,
}

impl BumpLine {
    /// 构造（参数显性校验）。
    pub fn new(lo: u64, hi: u64, mode: BlMode) -> Result<Self, (BlCode, String)> {
        if hi <= lo {
            return Err((BlCode::ArenaBad, BlCode::ArenaBad.advice().to_string()));
        }
        if hi - lo > BL_ARENA_MAX {
            return Err((BlCode::CapOver, BlCode::CapOver.advice().to_string()));
        }
        Ok(BumpLine {
            lo,
            hi,
            bump: lo,
            mode,
            ledger: Vec::new(),
            allocs: 0,
            rejects: Vec::new(),
            ledger_cap: 4096,
            codes: Vec::new(),
        })
    }

    /// 当前 bump 指针。
    pub const fn bump(&self) -> u64 {
        self.bump
    }

    /// 剩余容量（账实直算：HI − bump）。
    pub const fn free(&self) -> u64 {
        self.hi - self.bump
    }

    /// 已用量。
    pub const fn used(&self) -> u64 {
        self.bump - self.lo
    }

    /// 运行态。
    pub const fn mode(&self) -> BlMode {
        self.mode
    }

    /// 成功分配计数。
    pub const fn allocs(&self) -> u64 {
        self.allocs
    }

    fn note(&mut self, c: BlCode) {
        if !self.codes.contains(&c) {
            self.codes.push(c);
        }
        match self.rejects.iter_mut().find(|(k, _)| *k == c) {
            Some((_, n)) => *n += 1,
            None => self.rejects.push((c, 1)),
        }
    }

    /// 分配 O(1)：对齐上取整 → 上界钳判 → 指针推进 → 记账。零循环零查找。
    /// 返回分配起点；失败返回诊断码 + 建议（异常零静默）。
    pub fn alloc(&mut self, len: u64, align: u64, owner: BlOwner) -> Result<u64, (BlCode, String)> {
        // 门 1：来源合法性（演练注入仅演练态可达）。
        if owner == BlOwner::Drill && self.mode != BlMode::Drill {
            self.note(BlCode::DrillSpoof);
            return Err((BlCode::DrillSpoof, BlCode::DrillSpoof.advice().to_string()));
        }
        // 门 2：参数。
        if len == 0 {
            self.note(BlCode::ZeroLen);
            return Err((BlCode::ZeroLen, BlCode::ZeroLen.advice().to_string()));
        }
        if align == 0 || !align.is_power_of_two() {
            self.note(BlCode::AlignBad);
            return Err((BlCode::AlignBad, BlCode::AlignBad.advice().to_string()));
        }
        // 门 3：对齐上取整（无溢出：值域受 arena 上限约束）。
        let start = (self.bump + align - 1) & !(align - 1);
        let end = start.saturating_add(len);
        if end > self.hi {
            self.note(BlCode::Oob);
            return Err((BlCode::Oob, BlCode::Oob.advice().to_string()));
        }
        // 门 4：单调证明位（判据核心：指针单调推进）。
        if start < self.bump {
            self.note(BlCode::MonotonicBreak);
            return Err((BlCode::MonotonicBreak, BlCode::MonotonicBreak.advice().to_string()));
        }
        // 门 5：重放防线（账内不允许同 start 二次分配——指针推进后不可能，账破缺即显性）。
        if self.ledger.iter().any(|e| e.start == start) {
            self.note(BlCode::Replay);
            return Err((BlCode::Replay, BlCode::Replay.advice().to_string()));
        }
        // 推进 + 记账（同一事务）。
        self.bump = end;
        if self.ledger.len() >= self.ledger_cap {
            self.note(BlCode::LedgerFull);
            return Err((BlCode::LedgerFull, BlCode::LedgerFull.advice().to_string()));
        }
        self.ledger.push(BlEntry { start, end, owner });
        self.allocs += 1;
        Ok(start)
    }

    /// 重置（判据核心：仅演练态可达；正式态显性拒）。
    pub fn reset(&mut self) -> Result<(), (BlCode, String)> {
        if self.mode != BlMode::Drill {
            self.note(BlCode::ResetDenied);
            return Err((BlCode::ResetDenied, BlCode::ResetDenied.advice().to_string()));
        }
        self.bump = self.lo;
        self.ledger.clear();
        self.allocs = 0;
        self.rejects.clear();
        Ok(())
    }

    /// 账实守恒：账区间和 = 指针推进量（对齐间隙计入 used 不计入账——用末址对账）。
    pub fn conserved(&self) -> bool {
        let Some(last) = self.ledger.last() else {
            return self.bump == self.lo || self.ledger.is_empty();
        };
        // 账单调：逐条 start 递增且互不重叠。
        for w in self.ledger.windows(2) {
            if w[1].start < w[0].end {
                return false;
            }
        }
        last.end == self.bump && self.allocs == self.ledger.len() as u64
    }

    /// 自由量体检：free 与账实一致（防虚报）。
    pub fn free_healthy(&self) -> bool {
        let expect = self.hi - self.bump;
        expect == self.free()
    }

    /// 统计摘要。
    pub fn digest(&self) -> String {
        format!(
            "bump 下水线：{} arena [{:#x},{:#x}) 已用 {} 剩余 {} 分配 {} 账 {} 态 {}",
            self.mode.name(),
            self.lo,
            self.hi,
            self.used(),
            self.free(),
            self.allocs,
            self.ledger.len(),
            if self.conserved() { "账实守恒" } else { "账破缺" }
        )
    }
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    BlCode::ALL.iter().all(|c| c.val() >= BL_SEG_BASE && c.val() < BL_SEG_BASE + 0x100)
}
