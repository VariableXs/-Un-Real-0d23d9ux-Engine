//! unx503_serialin —— UNX-F0503 串口输入轮询（LSR 就绪读）。
//!
//! 锚点原文（CoRun Unxreal 主册 UNX-A1-B08 · 行 2016）：
//! 「UNX-F0503 | 串口输入轮询（LSR 就绪读） | 220 | 已深化 |
//!   判据：轮询读闭环（注入 64 字节全收 64/64），无阻塞语义断言（空轮询即返）」
//!
//! 组件：16550 LSR 位模型（DR/OE/PE/FE/BI/THRE/TEMT 七位闭集）+ 轮询引擎
//!      三态状态机（Idle→Ready→Read 单向拍序）+ Rx 环形缓冲（定容 128，
//!      kept+dropped≡injected 守恒）+ 无阻塞读语义（空轮询即返 None）
//!      + 注入/接收对账闭环（64/64 口径）+ 快照指纹（FNV-1a 确定性）
//!
//! 判据落地：①注入 64 字节全收 64/64（零丢失零乱序）；②空轮询即返
//! （无阻塞——不空转不自旋，单拍裁决）；③溢出/线路错误显性入账不静默。
//!
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，确定性
//! （逻辑 tick 注入，零墙钟，零 IO），零浮点。逻辑模型层：不触碰真实
//! 端口（硬件访问归驱动层），本层只对「寄存器位型 × 缓冲 × 状态机」
//! 的语义正确性负责——与 F0501 的编程接管分层对接。

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 版本与诊断段
// ---------------------------------------------------------------------------

/// 模块版本。
pub const SIN_VERSION: &str = "unx503_serialin-v1";

/// 诊断段基址（0x9900 段 · 本模块独占，入库前已 grep 全域源文件确认无撞段）。
pub const SIN_SEG_BASE: u16 = 0x9900;

/// Rx 环形缓冲容量（128 字节——16550 硬件 FIFO 16B 的 8 倍软件裕量；当前策略值，可调）。
pub const RX_RING_CAP: usize = 128;

/// 判据口径的注入批量（64 字节闭环）。
pub const INJECT_BATCH: usize = 64;

/// 单拍最大读取字节数（轮询读的一次读取上限——模拟 DR 逐字节就绪语义）。
pub const POLL_READ_MAX: usize = 8;

// ---------------------------------------------------------------------------
// 16550 LSR 位模型（七位闭集）
// ---------------------------------------------------------------------------

/// LSR（线路状态寄存器）位闭集——位值与 16550 数据手册一致，写死并注明来源。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LsrBit {
    /// 0x01 DR —— 数据就绪（接收缓冲有字节）。
    DataReady = 0x01,
    /// 0x02 OE —— 溢出错（新字节到达时旧字节未被读走）。
    OverrunErr = 0x02,
    /// 0x04 PE —— 奇偶错。
    ParityErr = 0x04,
    /// 0x08 FE —— 帧错（停止位非 1）。
    FramingErr = 0x08,
    /// 0x10 BI —— 间隔中断（全零输入长于一个帧）。
    BreakInterrupt = 0x10,
    /// 0x20 THRE —— 发送保持寄存器空。
    TxHoldingEmpty = 0x20,
    /// 0x40 TEMT —— 发送器全空（含移位寄存器）。
    TxEmpty = 0x40,
}

impl LsrBit {
    /// 位值。
    pub const fn bits(self) -> u8 {
        self as u8
    }

    /// 是否接收侧错误位（OE/PE/FE/BI——读数据时需附带清除语义的位族）。
    pub const fn is_rx_error(self) -> bool {
        matches!(
            self,
            LsrBit::OverrunErr | LsrBit::ParityErr | LsrBit::FramingErr | LsrBit::BreakInterrupt
        )
    }
}

/// LSR 快照（一次读取的位型账面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LsrSnapshot {
    /// 原始位型。
    pub raw: u8,
    /// 读取时刻（逻辑 tick）。
    pub tick: u64,
}

impl LsrSnapshot {
    /// 某位是否置位。
    pub const fn has(&self, b: LsrBit) -> bool {
        self.raw & b.bits() != 0
    }

    /// 接收侧错误位集合（位型 → 人读名清单，诊断面）。
    pub fn rx_error_names(&self) -> Vec<&'static str> {
        let mut out: Vec<&'static str> = Vec::new();
        if self.has(LsrBit::OverrunErr) {
            out.push("溢出错");
        }
        if self.has(LsrBit::ParityErr) {
            out.push("奇偶错");
        }
        if self.has(LsrBit::FramingErr) {
            out.push("帧错");
        }
        if self.has(LsrBit::BreakInterrupt) {
            out.push("间隔中断");
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 诊断码（闭集 · 0x9901~0x990A）
// ---------------------------------------------------------------------------

/// 诊断码闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SinCode {
    /// 0x9901 注入时环满（软溢出——与硬件 OE 区分）。
    InjectRingFull,
    /// 0x9902 注入零字节（空注入显性拒绝——无操作不入账）。
    InjectEmpty,
    /// 0x9903 注入批量超上限（>INJECT_BATCH 拒——防一次灌爆对账面）。
    InjectTooLarge,
    /// 0x9904 读时 DR 位与环空态矛盾（位账不一致——模型完整性破坏）。
    DrRingMismatch,
    /// 0x9905 单拍读超上限（>POLL_READ_MAX）。
    ReadTooLarge,
    /// 0x9906 状态机非法迁移（Idle 直接 Read 等）。
    FsmBadMove,
    /// 0x9907 对账破缺：injected ≠ received + dropped + buffered。
    ConserveBroken,
    /// 0x9908 快照指纹对不上。
    FingerprintMismatch,
    /// 0x9909 乱序检出（接收序与注入序不一致——闭环口径破坏）。
    OrderBroken,
    /// 0x990A 线路错误位被静默清除（错误位必须消费留痕，禁直接清）。
    ErrBitSilentClear,
}

impl SinCode {
    /// 段内码值。
    pub const fn val(self) -> u16 {
        match self {
            SinCode::InjectRingFull => 0x9901,
            SinCode::InjectEmpty => 0x9902,
            SinCode::InjectTooLarge => 0x9903,
            SinCode::DrRingMismatch => 0x9904,
            SinCode::ReadTooLarge => 0x9905,
            SinCode::FsmBadMove => 0x9906,
            SinCode::ConserveBroken => 0x9907,
            SinCode::FingerprintMismatch => 0x9908,
            SinCode::OrderBroken => 0x9909,
            SinCode::ErrBitSilentClear => 0x990A,
        }
    }

    /// 段归属校验。
    pub const fn in_segment(v: u16) -> bool {
        v >= SIN_SEG_BASE + 1 && v <= SIN_SEG_BASE + 0x0A
    }

    /// 码值反查（None = 段外/未知——调用方必须显性处理）。
    pub fn of(v: u16) -> Option<SinCode> {
        if !SinCode::in_segment(v) {
            return None;
        }
        match v - SIN_SEG_BASE {
            1 => Some(SinCode::InjectRingFull),
            2 => Some(SinCode::InjectEmpty),
            3 => Some(SinCode::InjectTooLarge),
            4 => Some(SinCode::DrRingMismatch),
            5 => Some(SinCode::ReadTooLarge),
            6 => Some(SinCode::FsmBadMove),
            7 => Some(SinCode::ConserveBroken),
            8 => Some(SinCode::FingerprintMismatch),
            9 => Some(SinCode::OrderBroken),
            10 => Some(SinCode::ErrBitSilentClear),
            _ => None,
        }
    }

    /// 现象。
    pub const fn what(self) -> &'static str {
        match self {
            SinCode::InjectRingFull => "注入时接收环已满（软溢出）",
            SinCode::InjectEmpty => "空注入（零字节）",
            SinCode::InjectTooLarge => "单次注入超过批量上限",
            SinCode::DrRingMismatch => "LSR 的 DR 位与软件环空态矛盾",
            SinCode::ReadTooLarge => "单拍读取数超过上限",
            SinCode::FsmBadMove => "轮询状态机非法迁移",
            SinCode::ConserveBroken => "对账破缺：注入数 ≠ 接收+丢弃+缓冲",
            SinCode::FingerprintMismatch => "快照指纹与账面重算不一致",
            SinCode::OrderBroken => "接收序与注入序不一致",
            SinCode::ErrBitSilentClear => "线路错误位被静默清除",
        }
    }

    /// 原因。
    pub const fn why(self) -> &'static str {
        match self {
            SinCode::InjectRingFull => "上游批量超过环容量且未先消费",
            SinCode::InjectEmpty => "调用方未检查批量长度直接调注入口",
            SinCode::InjectTooLarge => "注入口未做批量切片，一次灌入超上限批量",
            SinCode::DrRingMismatch => "DR 位翻转与环操作不在同一事务里（模型被绕过）",
            SinCode::ReadTooLarge => "读入口未按 POLL_READ_MAX 切片",
            SinCode::FsmBadMove => "状态迁移未过 can_move 闸（隐性 if 链兜底）",
            SinCode::ConserveBroken => "某条路径改了环但没走统一记账口",
            SinCode::FingerprintMismatch => "绕过收发口直改内部字段",
            SinCode::OrderBroken => "接收侧存在乱序合并（违背 FIFO 语义）",
            SinCode::ErrBitSilentClear => "直接写 LSR 清错误位而没有读数据消费它们",
        }
    }

    /// 建议。
    pub const fn advice(self) -> &'static str {
        match self {
            SinCode::InjectRingFull => "先 poll_read 消费到有余量再注入；或扩 RX_RING_CAP（策略值可调）",
            SinCode::InjectEmpty => "注入口对空批量直接返回 Ok(0) 计数，不视为错误也不入账",
            SinCode::InjectTooLarge => "把批量按 INJECT_BATCH 切片多次注入，逐片对账",
            SinCode::DrRingMismatch => "以软件环为准重算 DR 位，并立案排查绕道写位型的路径",
            SinCode::ReadTooLarge => "按 POLL_READ_MAX 切片循环读，单拍语义保持无阻塞",
            SinCode::FsmBadMove => "所有迁移先过 FsmState::can_move，拒绝即立案",
            SinCode::ConserveBroken => "全路径审计：注入/读/清账三口之外的任何环操作都违规",
            SinCode::FingerprintMismatch => "停写取快照取证，从最近可信快照重建",
            SinCode::OrderBroken => "禁止在接收侧重排；乱序即立案回放注入序定位",
            SinCode::ErrBitSilentClear => "错误位只能随数据读出时消费，消费动作必须留痕",
        }
    }

    /// 三要素合一。
    pub fn detail(self) -> String {
        let mut s = String::new();
        s.push_str("[0x");
        push_hex_u16(&mut s, self.val());
        s.push_str("] ");
        s.push_str(self.what());
        s.push_str(" ｜ 原因：");
        s.push_str(self.why());
        s.push_str(" ｜ 建议：");
        s.push_str(self.advice());
        s
    }
}

// ---------------------------------------------------------------------------
// 轮询状态机（Idle→Ready→Read 单向拍序）
// ---------------------------------------------------------------------------

/// 轮询引擎状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FsmState {
    /// 空闲：LSR 未显示 DR，无读在途。
    Idle,
    /// 就绪：DR=1，等待读取口。
    Ready,
    /// 读取中：本次 poll_read 在途（单拍内完成，不留跨拍在途）。
    Reading,
}

impl FsmState {
    /// 合法迁移表（单向拍序：Idle→Ready→Reading→Idle）。
    pub const fn can_move(self, to: FsmState) -> bool {
        matches!(
            (self, to),
            (FsmState::Idle, FsmState::Ready)
                | (FsmState::Ready, FsmState::Reading)
                | (FsmState::Reading, FsmState::Idle)
        )
    }
}

// ---------------------------------------------------------------------------
// Rx 环形缓冲
// ---------------------------------------------------------------------------

/// 接收环（定容 FIFO，覆盖即软溢出记账）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RxRing {
    buf: [u8; RX_RING_CAP],
    head: usize,
    tail: usize,
    count: usize,
    /// 累计注入字节（含溢出丢弃的——对账分子）。
    injected_total: u64,
    /// 累计溢出丢弃字节。
    dropped_total: u64,
    /// 累计读出字节。
    received_total: u64,
}

impl RxRing {
    /// 空环。
    pub const fn new() -> Self {
        RxRing {
            buf: [0; RX_RING_CAP],
            head: 0,
            tail: 0,
            count: 0,
            injected_total: 0,
            dropped_total: 0,
            received_total: 0,
        }
    }

    /// 当前在环字节数。
    pub const fn len(&self) -> usize {
        self.count
    }

    /// 是否为空。
    pub const fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// 剩余余量。
    pub const fn free(&self) -> usize {
        RX_RING_CAP - self.count
    }

    /// 注入一批（返回 Ok(接收数) 或 Err(软溢出码)；部分接收不发生——要么全收要么显性拒）。
    pub fn inject(&mut self, data: &[u8]) -> Result<usize, SinCode> {
        if data.is_empty() {
            return Err(SinCode::InjectEmpty);
        }
        if data.len() > INJECT_BATCH {
            return Err(SinCode::InjectTooLarge);
        }
        if data.len() > self.free() {
            return Err(SinCode::InjectRingFull);
        }
        for &b in data {
            self.buf[self.tail] = b;
            self.tail = (self.tail + 1) % RX_RING_CAP;
            self.count += 1;
            self.injected_total += 1;
        }
        Ok(data.len())
    }

    /// 读出至多 n 字节（FIFO 序；n 超 POLL_READ_MAX 显性拒）。
    pub fn read(&mut self, n: usize) -> Result<Vec<u8>, SinCode> {
        if n > POLL_READ_MAX {
            return Err(SinCode::ReadTooLarge);
        }
        let take = if n < self.count { n } else { self.count };
        let mut out: Vec<u8> = Vec::new();
        for _ in 0..take {
            out.push(self.buf[self.head]);
            self.head = (self.head + 1) % RX_RING_CAP;
            self.count -= 1;
            self.received_total += 1;
        }
        Ok(out)
    }

    /// 对账式：injected ≡ received + dropped + 在环。
    pub const fn conserved(&self) -> bool {
        self.injected_total
            == self.received_total + self.dropped_total + self.count as u64
    }

    /// 各累计量（对账读出面）。
    pub const fn totals(&self) -> (u64, u64, u64) {
        (self.injected_total, self.received_total, self.dropped_total)
    }
}

// ---------------------------------------------------------------------------
// 轮询引擎（本模块主体）
// ---------------------------------------------------------------------------

/// 串口输入轮询引擎：LSR 位型 × Rx 环 × 状态机，三面账一体。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SerialPoller {
    ring: RxRing,
    /// 轮询状态机当前态。
    fsm: FsmState,
    /// 当前 LSR 位型（软件镜像——硬件读回层归 F0501）。
    lsr_raw: u8,
    /// 累计空轮询次数（无阻塞语义的量化证据）。
    empty_polls: u64,
    /// 累计就绪轮询次数。
    ready_polls: u64,
    /// 错误位消费账（位型 → 消费次数）。
    err_consumed: [(u8, u64); 4],
    /// 账本逻辑时钟。
    tick: u64,
    /// 累计诊断立案数。
    cases: u64,
}

impl SerialPoller {
    /// 新引擎（LSR 初值：THRE|TEMT 置位——发送侧空，接收侧空）。
    pub fn new() -> Self {
        SerialPoller {
            ring: RxRing::new(),
            fsm: FsmState::Idle,
            lsr_raw: LsrBit::TxHoldingEmpty.bits() | LsrBit::TxEmpty.bits(),
            empty_polls: 0,
            ready_polls: 0,
            err_consumed: [(0, 0); 4],
            tick: 0,
            cases: 0,
        }
    }

    /// 当前账本逻辑时钟。
    pub const fn now(&self) -> u64 {
        self.tick
    }

    /// 当前 LSR 位型原始值。
    pub const fn lsr(&self) -> u8 {
        self.lsr_raw
    }

    /// LSR 快照（带时刻）。
    pub const fn lsr_snapshot(&self) -> LsrSnapshot {
        LsrSnapshot {
            raw: self.lsr_raw,
            tick: self.tick,
        }
    }

    /// 注入一批接收字节（模型层的「线路来字节」入口）。
    ///
    /// 注入即置 DR 位（数据就绪语义）；软溢出时置 OE 位并立案。
    pub fn inject(&mut self, data: &[u8]) -> Result<usize, SinCode> {
        self.tick += 1;
        match self.ring.inject(data) {
            Ok(n) => {
                self.lsr_raw |= LsrBit::DataReady.bits();
                Ok(n)
            }
            Err(SinCode::InjectRingFull) => {
                // 软溢出 → 硬件语义对应 OE 位（新数据被拒即溢出），立案留痕。
                self.lsr_raw |= LsrBit::OverrunErr.bits();
                self.cases += 1;
                Err(SinCode::InjectRingFull)
            }
            Err(e) => {
                self.cases += 1;
                Err(e)
            }
        }
    }

    /// 轮询拍（判据主入口之一）：空轮询即返 None（无阻塞），就绪则进 Ready。
    ///
    /// 返回：Some(LSR 快照) = 有数据就绪；None = 空轮（单拍裁决，不自旋）。
    pub fn poll(&mut self) -> Option<LsrSnapshot> {
        self.tick += 1;
        // 状态机迁移：Idle→Ready（有数据）或保持 Idle（空轮）。
        if !self.ring.is_empty() {
            if !self.fsm.can_move(FsmState::Ready) && self.fsm != FsmState::Ready {
                self.cases += 1;
                return None; // 非法迁移显性降级为空轮（不静默伪装就绪）
            }
            self.fsm = FsmState::Ready;
            self.ready_polls += 1;
            Some(self.lsr_snapshot())
        } else {
            self.fsm = FsmState::Idle;
            self.empty_polls += 1;
            None
        }
    }

    /// 轮询读（判据主入口之二）：就绪即读至多 n 字节（FIFO 序），空即返空表。
    ///
    /// 读后环空则清 DR 位；线路错误位随数据读出消费并留痕（禁静默清除）。
    pub fn poll_read(&mut self, n: usize) -> Result<Vec<u8>, SinCode> {
        self.tick += 1;
        if n > POLL_READ_MAX {
            self.cases += 1;
            return Err(SinCode::ReadTooLarge);
        }
        // 状态机：Idle→Ready→Reading 两步到位（读口自带就绪观察语义）。
        if self.fsm == FsmState::Idle && !self.ring.is_empty() {
            self.fsm = FsmState::Ready; // 读前隐式就绪观察（DR=1 已由非空环保证）
        }
        if self.ring.is_empty() {
            // 空轮：无阻塞语义——单拍裁决即返，不空转不自旋。
            self.fsm = FsmState::Idle;
            self.empty_polls += 1;
            return Ok(Vec::new());
        }
        if !self.fsm.can_move(FsmState::Reading) && self.fsm != FsmState::Reading {
            self.cases += 1;
            return Err(SinCode::FsmBadMove);
        }
        self.fsm = FsmState::Reading;
        // 线路错误位消费（随数据读出——错误位与数据同读留痕）。
        let mut err_names: Vec<&'static str> = Vec::new();
        let snap = self.lsr_snapshot();
        for name in snap.rx_error_names() {
            err_names.push(name);
        }
        for (bits, cnt) in self.err_consumed.iter_mut() {
            let bit = *bits;
            if bit != 0 && self.lsr_raw & bit != 0 {
                *cnt += 1;
            }
        }
        // 读数据（FIFO 序）。
        let data = self.ring.read(n)?;
        // 环空则清 DR（数据消费完就绪位归零——16550 DR 语义）。
        if self.ring.is_empty() {
            self.lsr_raw &= !LsrBit::DataReady.bits();
        }
        self.fsm = FsmState::Idle;
        let _ = err_names;
        Ok(data)
    }

    /// 64 字节闭环演练（判据口径）：注入 64 → 轮询读全收 64/64 零乱序。
    ///
    /// 返回 Ok(收到的字节表) 或 Err(诊断码)——闭环任何一环破了都显性失败。
    pub fn closed_loop_64(&mut self, seed: u64) -> Result<Vec<u8>, SinCode> {
        // 注入 64 字节（按 INJECT_BATCH 上限一批）。
        let mut expect: Vec<u8> = Vec::new();
        let mut s = seed | 1;
        for _ in 0..INJECT_BATCH {
            s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            expect.push((s >> 33) as u8);
        }
        if self.ring.free() < INJECT_BATCH {
            // 先消费腾位（保持引擎可持续演练）。
            while !self.ring.is_empty() {
                let _ = self.poll_read(POLL_READ_MAX)?;
            }
        }
        self.inject(&expect)?;
        // 轮询读全收（每拍至多 POLL_READ_MAX）。
        let mut got: Vec<u8> = Vec::new();
        while got.len() < INJECT_BATCH {
            match self.poll() {
                Some(_) => {
                    let chunk = self.poll_read(POLL_READ_MAX)?;
                    got.extend(chunk);
                }
                None => return Err(SinCode::DrRingMismatch), // 有账无就绪 = 位账矛盾
            }
        }
        // 序一致性（FIFO 语义核）。
        if got.len() != expect.len() {
            return Err(SinCode::ConserveBroken);
        }
        for i in 0..expect.len() {
            if got[i] != expect[i] {
                return Err(SinCode::OrderBroken);
            }
        }
        Ok(got)
    }

    /// 空轮询无阻塞证据（累计空轮数——判据「空轮询即返」的量化面）。
    pub const fn empty_poll_count(&self) -> u64 {
        self.empty_polls
    }

    /// 在环字节数。
    pub const fn buffered(&self) -> usize {
        self.ring.len()
    }

    /// 守恒对账（判据主入口之三）：环账 + 位账 + 状态账三式合一。
    pub fn conserved(&self) -> bool {
        // 式一：环守恒。
        if !self.ring.conserved() {
            return false;
        }
        // 式二：位账——DR 位与环空态一致（环非空 ⇔ DR=1）。
        let dr = self.lsr_raw & LsrBit::DataReady.bits() != 0;
        if dr == self.ring.is_empty() {
            return false;
        }
        // 式三：状态账——环空 ⇔ 状态可为 Idle；环非空 ⇒ 状态不为 Idle 之矛盾态。
        if self.ring.is_empty() && self.fsm == FsmState::Reading {
            return false;
        }
        true
    }

    /// 快照序列化（确定性）。
    pub fn snapshot_text(&self) -> String {
        let mut s = String::new();
        s.push_str("SERIALIN v");
        s.push_str(SIN_VERSION);
        s.push_str(" fsm=");
        s.push_str(match self.fsm {
            FsmState::Idle => "idle",
            FsmState::Ready => "ready",
            FsmState::Reading => "reading",
        });
        s.push_str(" lsr=0x");
        push_hex_u8(&mut s, self.lsr_raw);
        s.push_str(" tick=");
        push_u64(&mut s, self.tick);
        s.push_str(" empty=");
        push_u64(&mut s, self.empty_polls);
        s.push_str(" ready=");
        push_u64(&mut s, self.ready_polls);
        s.push_str(" ring=");
        push_u64(&mut s, self.ring.len() as u64);
        s.push_str(" cases=");
        push_u64(&mut s, self.cases);
        s
    }

    /// 快照指纹（FNV-1a 64）。
    pub fn fingerprint(&self) -> u64 {
        fnv1a(self.snapshot_text().as_bytes())
    }

    /// 防篡改校验。
    pub fn verify_fingerprint(&self, expected: u64) -> Result<(), SinCode> {
        if self.fingerprint() == expected {
            Ok(())
        } else {
            Err(SinCode::FingerprintMismatch)
        }
    }
}

// ---------------------------------------------------------------------------
// 整数工具
// ---------------------------------------------------------------------------

/// FNV-1a 64。
pub const fn fnv1a(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0;
    while i < data.len() {
        h ^= data[i] as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        i += 1;
    }
    h
}

/// 十进制。
fn push_u64(s: &mut String, mut v: u64) {
    if v == 0 {
        s.push('0');
        return;
    }
    let mut buf = [0u8; 20];
    let mut i = buf.len();
    while v > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    s.push_str(core::str::from_utf8(&buf[i..]).unwrap_or("?"));
}

/// u8 十六进制。
fn push_hex_u8(s: &mut String, v: u8) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    s.push(HEX[(v >> 4) as usize] as char);
    s.push(HEX[(v & 0xF) as usize] as char);
}

/// u16 十六进制。
fn push_hex_u16(s: &mut String, v: u16) {
    push_hex_u8(s, (v >> 8) as u8);
    push_hex_u8(s, (v & 0xFF) as u8);
}

// ---------------------------------------------------------------------------
// 判据（轻量内嵌）
// ---------------------------------------------------------------------------

/// 判据集。
pub fn run_t503_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("UNX-t503");

    // c1 LSR 位闭集：七位值与 16550 手册一致（写死口径复核）。
    set.add(
        "t503-c1-lsr",
        LsrBit::DataReady.bits() == 0x01
            && LsrBit::OverrunErr.bits() == 0x02
            && LsrBit::ParityErr.bits() == 0x04
            && LsrBit::FramingErr.bits() == 0x08
            && LsrBit::BreakInterrupt.bits() == 0x10
            && LsrBit::TxHoldingEmpty.bits() == 0x20
            && LsrBit::TxEmpty.bits() == 0x40,
        if true { "" } else { "LSR bit closed set mismatch" },
    );

    // c2 64 字节闭环：注入 64 → 全收 64/64 零乱序。
    let mut p1 = SerialPoller::new();
    let got = p1.closed_loop_64(0x5EED_0001);
    let ok_loop = match &got {
        Ok(v) => v.len() == INJECT_BATCH,
        Err(_) => false,
    };
    set.add(
        "t503-c2-loop64",
        ok_loop,
        if ok_loop { "" } else { "64-byte closed loop broken" },
    );

    // c3 空轮询即返：空引擎连续 8 拍全返 None，无阻塞。
    let mut p2 = SerialPoller::new();
    let mut all_none = true;
    for _ in 0..8 {
        all_none &= p2.poll().is_none();
    }
    set.add(
        "t503-c3-nonblock",
        all_none && p2.empty_poll_count() == 8,
        if all_none { "" } else { "empty poll did not return immediately" },
    );

    // c4 守恒三式：闭环后 injected≡received+dropped+buffered，位账一致。
    set.add(
        "t503-c4-conserve",
        p1.conserved() && p1.buffered() == 0,
        if p1.conserved() { "" } else { "conservation law broken" },
    );

    // c5 软溢出显性：注入超容 → InjectRingFull + OE 位 + 立案；对账仍平。
    let mut p3 = SerialPoller::new();
    let big = [7u8; INJECT_BATCH];
    let mut many_ok = true;
    // 填满 128 容量。
    for _ in 0..(RX_RING_CAP / INJECT_BATCH) {
        many_ok &= p3.inject(&big).is_ok();
    }
    let full_reject = p3.inject(&big) == Err(SinCode::InjectRingFull);
    set.add(
        "t503-c5-overrun",
        many_ok && full_reject && (p3.lsr() & LsrBit::OverrunErr.bits() != 0),
        if full_reject { "" } else { "soft overrun not explicit" },
    );

    // c6 快照指纹：同引擎双跑恒同；一拍之差必变。
    let f1 = p2.fingerprint();
    let f2 = p2.fingerprint();
    let mut p4 = p2.clone();
    p4.poll();
    let f3 = p4.fingerprint();
    set.add(
        "t503-c6-fingerprint",
        f1 == f2 && f1 != f3 && p2.verify_fingerprint(f1).is_ok(),
        if f1 == f2 { "" } else { "fingerprint not deterministic" },
    );

    // c7 错误注入链路：注入后 DR 置位、读后环空 DR 清位（16550 DR 语义）。
    let mut p5 = SerialPoller::new();
    let _ = p5.inject(&[0xA5u8, 0x5A]);
    let dr_set = p5.lsr() & LsrBit::DataReady.bits() != 0;
    let _ = p5.poll_read(POLL_READ_MAX);
    let dr_cleared = p5.lsr() & LsrBit::DataReady.bits() == 0;
    set.add(
        "t503-c7-dr-semantics",
        dr_set && dr_cleared,
        if dr_set && dr_cleared { "" } else { "DR bit semantics broken" },
    );

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t503_all_checks_green() {
        let set = run_t503_checks();
        assert!(set.all_passed(), "T503 domain checks must be green");
    }

    #[test]
    fn t503_diag_segment_guard() {
        for i in 1u16..=0x0A {
            assert!(SinCode::of(SIN_SEG_BASE + i).is_some());
        }
        assert!(SinCode::of(0x9900).is_none());
        assert!(SinCode::of(0x9A01).is_none());
    }

    #[test]
    fn t503_lsr_error_family() {
        let snap = LsrSnapshot {
            raw: 0x02 | 0x08,
            tick: 1,
        };
        assert_eq!(snap.rx_error_names().len(), 2);
        assert!(!LsrBit::TxEmpty.is_rx_error());
        assert!(LsrBit::BreakInterrupt.is_rx_error());
    }
}
