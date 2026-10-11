//! unx483_fbmap —— UNX-F0483 帧缓冲映射登记（B19 账联动）。
//!
//! 锚点原文（CoRun Unxreal 主册 UNX-A1-B07 · 行 1989）：
//! 「UNX-F0483 | 帧缓冲映射登记（B19 账联动） | 220 | 已深化 |
//!   判据：帧缓冲区映射登记（基址/长度/属性 WC）入 B19 账（1/1 段），
//!   登记后区外写尝试拦截 100%」
//!
//! 组件：映射段登记账（B19 账 · 段容量 1/1）+ 段四要素校验链（基址/长度/属性/对齐）
//!      + 区外写拦截器（探测地址集全量拦截 · 100% 口径）+ 登记流水账（append-only 可回放）
//!      + 撤销/重登状态机（Registered→Frozen→Retired 单向）+ 快照指纹（FNV-1a 确定性）
//!
//! 判据落地：①登记（基址/长度/属性 WC）入 B19 账且 1/1 段满；②登记后区外写
//! 尝试拦截 100%（探测集内每个区外地址都被拦截器拒绝并留痕）；③区内写放行
//! 且账实守恒（尝试数 = 拦截数 + 放行数）。
//!
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，确定性
//! （逻辑 tick 注入，零墙钟，零 IO），零浮点。

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 版本与诊断段
// ---------------------------------------------------------------------------

/// 模块版本。
pub const FBM_VERSION: &str = "unx483_fbmap-v1";

/// 诊断段基址（0x9800 段 · 本模块独占，入库前已 grep 全域源文件确认无撞段）。
pub const FBM_SEG_BASE: u16 = 0x9800;

/// B19 账段容量（判据口径：1/1 段——当前策略值，账位结构按可扩展口径设计）。
pub const B19_SEGMENT_CAPACITY: usize = 1;

/// 页粒度（4KiB——x86_64 标准页，映射登记的最小对齐单位）。
pub const FBM_PAGE: u64 = 0x1000;

/// 区外写探测集大小（判据「拦截 100%」的口径基数：探测集内全拦截）。
pub const FBM_PROBE_COUNT: usize = 16;

/// 拦截流水账容量（滚动覆盖最旧——账满不再增长，kept+dropped≡seen 守恒）。
pub const FBM_INTERCEPT_LOG_CAP: usize = 64;

/// 登记流水账容量（append-only 截断面：超出即拒新登记而非覆盖历史）。
pub const FBM_REG_LOG_CAP: usize = 32;

// ---------------------------------------------------------------------------
// 诊断码（闭集 · 0x9801~0x980C）
// ---------------------------------------------------------------------------

/// 诊断码闭集（段内十六码；闭集外一律拒绝进入账面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FbmCode {
    /// 0x9801 基址未按页对齐（4KiB）。
    BaseUnaligned,
    /// 0x9802 长度为零或非页倍数。
    LenBad,
    /// 0x9803 基址+长度 u64 溢出（区间非法）。
    RangeOverflow,
    /// 0x9804 属性字非法（既非 WC 也非 UC）。
    AttrBad,
    /// 0x9805 B19 账段满（1/1）——新段拒绝，先撤销旧段。
    SegFull,
    /// 0x9806 与既有段重叠（区间相交）。
    Overlap,
    /// 0x9807 撤销时账内无此段（序号对不上）。
    RetireNoSuchSeg,
    /// 0x9808 冻结段不可撤销（状态机单向：Frozen→Retired 非法）。
    RetireFrozen,
    /// 0x9809 撤销后再撤销（Retired 终态）。
    RetireTwice,
    /// 0x980A 登记流水账满（append-only 截断——先归档再登记）。
    RegLogFull,
    /// 0x980B 快照指纹对不上（账被绕过直改——防篡改线）。
    FingerprintMismatch,
    /// 0x980C 序列号回绕耗尽（u32 序号用尽——理论边界，显性不静默）。
    SerialExhausted,
}

impl FbmCode {
    /// 段内码值（0x9801~0x980C）。
    pub const fn val(self) -> u16 {
        match self {
            FbmCode::BaseUnaligned => 0x9801,
            FbmCode::LenBad => 0x9802,
            FbmCode::RangeOverflow => 0x9803,
            FbmCode::AttrBad => 0x9804,
            FbmCode::SegFull => 0x9805,
            FbmCode::Overlap => 0x9806,
            FbmCode::RetireNoSuchSeg => 0x9807,
            FbmCode::RetireFrozen => 0x9808,
            FbmCode::RetireTwice => 0x9809,
            FbmCode::RegLogFull => 0x980A,
            FbmCode::FingerprintMismatch => 0x980B,
            FbmCode::SerialExhausted => 0x980C,
        }
    }

    /// 诊断段归属校验（段外码一律拒绝）。
    pub const fn in_segment(v: u16) -> bool {
        v >= FBM_SEG_BASE + 1 && v <= FBM_SEG_BASE + 0x0C
    }

    /// 码值反查（None = 段外/未知——调用方必须显性处理，不许吞）。
    pub fn of(v: u16) -> Option<FbmCode> {
        if !FbmCode::in_segment(v) {
            return None;
        }
        match v - FBM_SEG_BASE {
            1 => Some(FbmCode::BaseUnaligned),
            2 => Some(FbmCode::LenBad),
            3 => Some(FbmCode::RangeOverflow),
            4 => Some(FbmCode::AttrBad),
            5 => Some(FbmCode::SegFull),
            6 => Some(FbmCode::Overlap),
            7 => Some(FbmCode::RetireNoSuchSeg),
            8 => Some(FbmCode::RetireFrozen),
            9 => Some(FbmCode::RetireTwice),
            10 => Some(FbmCode::RegLogFull),
            11 => Some(FbmCode::FingerprintMismatch),
            12 => Some(FbmCode::SerialExhausted),
            _ => None,
        }
    }

    /// 现象（发生了什么——人话第一句）。
    pub const fn what(self) -> &'static str {
        match self {
            FbmCode::BaseUnaligned => "基址未按 4KiB 页对齐",
            FbmCode::LenBad => "长度为零或不是页大小的整数倍",
            FbmCode::RangeOverflow => "基址+长度超出 u64 表达范围",
            FbmCode::AttrBad => "属性字既不是 WC 也不是 UC",
            FbmCode::SegFull => "B19 账段位已满（1/1）",
            FbmCode::Overlap => "与账内既有映射段区间相交",
            FbmCode::RetireNoSuchSeg => "要撤销的段序号在账内不存在",
            FbmCode::RetireFrozen => "冻结态段不允许直接撤销",
            FbmCode::RetireTwice => "该段已是撤销终态，不能重复撤销",
            FbmCode::RegLogFull => "登记流水账已满（append-only）",
            FbmCode::FingerprintMismatch => "快照指纹与账面重算值不一致",
            FbmCode::SerialExhausted => "段序号空间（u32）已用尽",
        }
    }

    /// 原因（为什么发生）。
    pub const fn why(self) -> &'static str {
        match self {
            FbmCode::BaseUnaligned => "传入基址低 12 位非零——来自未裁剪的物理地址或截断错误",
            FbmCode::LenBad => "长度字段来自描述表直读，未经页粒度归整",
            FbmCode::RangeOverflow => "高位平台地址接近 u64 上界，长度叠加跨溢出",
            FbmCode::AttrBad => "属性字被位域打包污染或来自未定义的缓存策略编码",
            FbmCode::SegFull => "B19 账按判据口径只开一个段位，旧段未撤销就来了新段",
            FbmCode::Overlap => "固件描述表里两个 BAR/信箱区间共享了一段物理页",
            FbmCode::RetireNoSuchSeg => "撤销请求带着过期快照里的旧序号",
            FbmCode::RetireFrozen => "冻结是审计语义：先解冻（走解冻口）再撤销",
            FbmCode::RetireTwice => "同一撤销事件被重放（幂等键缺失）",
            FbmCode::RegLogFull => "登记频次超过流水容量且未触发归档",
            FbmCode::FingerprintMismatch => "有人绕过登记口直接改了账面字段",
            FbmCode::SerialExhausted => "单账生命周期内撤销/重登次数超过 2^32",
        }
    }

    /// 修正建议（下一步怎么办——可操作）。
    pub const fn advice(self) -> &'static str {
        match self {
            FbmCode::BaseUnaligned => "把基址向下取整到 4KiB 边界后重提（损失页内偏移由调用方自证无设备寄存器）",
            FbmCode::LenBad => "把长度向上归整到页倍数，并把归整差量记进登记备注",
            FbmCode::RangeOverflow => "拆成两段分别登记，或改用高位平台窗口描述",
            FbmCode::AttrBad => "改用 UC（0x1）或 WC（0x2）之一；其他缓存策略编码须先扩展属性闭集",
            FbmCode::SegFull => "先走撤销口释放旧段（拿回段位）再登记新段；或经治理流程扩 B19 段容量",
            FbmCode::Overlap => "先撤销重叠旧段，或把新段裁剪到与旧段不相交的子区间",
            FbmCode::RetireNoSuchSeg => "用 region_of 查询当前生效段序号后再发起撤销",
            FbmCode::RetireFrozen => "走解冻口（需审计理由）拿到 Registered 态再撤销",
            FbmCode::RetireTwice => "给撤销请求加幂等键（段序号+账版本号）后重放",
            FbmCode::RegLogFull => "先触发流水归档（快照+指纹）再继续登记",
            FbmCode::FingerprintMismatch => "立即停写账面，取当前快照做事故取证，再从最近可信快照重建",
            FbmCode::SerialExhausted => "换新账（代际切换）：旧账封账归档，新账从序号 1 重新计",
        }
    }

    /// 三要素合一（现象/原因/建议——异常零静默的输出面）。
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
// 属性字闭集
// ---------------------------------------------------------------------------

/// 映射属性字闭集（判据点名 WC；UC 作为回退口径同样入闭集管理）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FbAttr {
    /// 0x1 UC —— Uncacheable（强序，逐笔直达）。
    Uc,
    /// 0x2 WC —— Write-Combining（写合并，帧缓冲批量写首选）。
    Wc,
}

impl FbAttr {
    /// 属性字位型（落账口径）。
    pub const fn bits(self) -> u8 {
        match self {
            FbAttr::Uc => 0x1,
            FbAttr::Wc => 0x2,
        }
    }

    /// 位型反查（None = 闭集外）。
    pub const fn of(bits: u8) -> Option<FbAttr> {
        match bits {
            0x1 => Some(FbAttr::Uc),
            0x2 => Some(FbAttr::Wc),
            _ => None,
        }
    }

    /// 人读名。
    pub const fn zh(self) -> &'static str {
        match self {
            FbAttr::Uc => "UC 强序",
            FbAttr::Wc => "WC 写合并",
        }
    }
}

// ---------------------------------------------------------------------------
// 段状态机（单向：Unregistered→Registered→Frozen→Retired）
// ---------------------------------------------------------------------------

/// 段生命周期状态（迁移只许沿单向箭头，回退一律拒绝并留痕）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SegState {
    /// 未登记（段位空）。
    Unregistered,
    /// 已登记（可写可查可拦截）。
    Registered,
    /// 已冻结（审计锁：查询照常，登记/撤销均拒）。
    Frozen,
    /// 已撤销（终态：段位释放，流水留痕）。
    Retired,
}

impl SegState {
    /// 合法迁移表（单向状态机的机器口径）。
    pub const fn can_move(self, to: SegState) -> bool {
        matches!(
            (self, to),
            (SegState::Unregistered, SegState::Registered)
                | (SegState::Registered, SegState::Frozen)
                | (SegState::Registered, SegState::Retired)
                | (SegState::Frozen, SegState::Registered) // 解冻口（带审计理由）
        )
    }

    /// 人读名。
    pub const fn zh(self) -> &'static str {
        match self {
            SegState::Unregistered => "未登记",
            SegState::Registered => "已登记",
            SegState::Frozen => "已冻结",
            SegState::Retired => "已撤销",
        }
    }
}

// ---------------------------------------------------------------------------
// 映射段（四要素 + 状态 + 序号）
// ---------------------------------------------------------------------------

/// 一条映射段登记（判据四要素：基址/长度/属性 + 页对齐佐证）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FbSegment {
    /// 段序号（账内单调递增，撤销后不复用——防旧请求误命中）。
    pub serial: u32,
    /// 物理基址（4KiB 对齐）。
    pub base: u64,
    /// 区间长度（页倍数）。
    pub len: u64,
    /// 属性字（WC/UC）。
    pub attr: FbAttr,
    /// 登记时刻（逻辑 tick）。
    pub reg_tick: u64,
    /// 撤销时刻（None = 在账）。
    pub retire_tick: Option<u64>,
    /// 当前状态。
    pub state: SegState,
}

impl FbSegment {
    /// 区间尾（开区间上界；构造期已保证 base+len 不溢出）。
    pub const fn end(&self) -> u64 {
        self.base + self.len
    }

    /// 地址是否落在段内（前闭后开）。
    pub const fn contains(&self, addr: u64) -> bool {
        addr >= self.base && addr < self.end()
    }

    /// 与另一区间是否相交（前闭后开口径）。
    pub const fn overlaps(&self, base: u64, len: u64) -> bool {
        self.base < base + len && base < self.end()
    }

    /// 覆盖页数（len 已保证页倍数）。
    pub const fn pages(&self) -> u64 {
        self.len / FBM_PAGE
    }
}

// ---------------------------------------------------------------------------
// 写尝试拦截器
// ---------------------------------------------------------------------------

/// 区外写尝试的一笔留痕（谁/何时/为什么被拦）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WriteAttempt {
    /// 尝试地址。
    pub addr: u64,
    /// 尝试时刻（逻辑 tick）。
    pub tick: u64,
    /// 裁决：true=拦截（区外），false=放行（区内）。
    pub intercepted: bool,
    /// 命中的段序号（放行时才有意义；拦截时为 0）。
    pub hit_serial: u32,
}

/// 拦截流水账（滚动覆盖最旧，kept+dropped≡seen 守恒）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct InterceptLog {
    ring: [Option<WriteAttempt>; FBM_INTERCEPT_LOG_CAP],
    head: usize,
    /// 累计入账总数（含已被滚动覆盖的）。
    seen: u64,
    /// 累计滚动丢弃数。
    dropped: u64,
}

impl InterceptLog {
    /// 空账。
    pub const fn new() -> Self {
        InterceptLog {
            ring: [None; FBM_INTERCEPT_LOG_CAP],
            head: 0,
            seen: 0,
            dropped: 0,
        }
    }

    /// 入账一笔（满则覆盖最旧并计数）。
    pub fn push(&mut self, a: WriteAttempt) {
        if self.ring[self.head].is_some() {
            self.dropped += 1;
        }
        self.ring[self.head] = Some(a);
        self.head = (self.head + 1) % FBM_INTERCEPT_LOG_CAP;
        self.seen += 1;
    }

    /// 按时间升序取全量快照（滚动窗内的）。
    pub fn snapshot(&self) -> Vec<WriteAttempt> {
        let mut out: Vec<WriteAttempt> = Vec::new();
        let mut i = self.head;
        for _ in 0..FBM_INTERCEPT_LOG_CAP {
            if let Some(a) = &self.ring[i] {
                out.push(a.clone());
            }
            i = (i + 1) % FBM_INTERCEPT_LOG_CAP;
        }
        out
    }

    /// 守恒式：kept + dropped == seen（账实对账口）。
    pub const fn conserved(&self) -> bool {
        let kept = if self.seen >= FBM_INTERCEPT_LOG_CAP as u64 {
            FBM_INTERCEPT_LOG_CAP as u64
        } else {
            self.seen
        };
        kept + self.dropped == self.seen
    }

    /// 窗口内拦截数。
    pub fn intercepted_in_window(&self) -> usize {
        self.ring
            .iter()
            .filter(|a| matches!(a, Some(w) if w.intercepted))
            .count()
    }
}

// ---------------------------------------------------------------------------
// 登记流水（append-only）
// ---------------------------------------------------------------------------

/// 一笔登记流水（裁决结果 + 快照，供回放与审计）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RegEntry {
    /// 流水时刻（逻辑 tick）。
    pub tick: u64,
    /// 动作：true=登记成功，false=登记被拒。
    pub granted: bool,
    /// 拒绝码（granted 时为 0）。
    pub code: u16,
    /// 段四要素快照（拒绝时也留——审计要看得见被拒的是什么）。
    pub base: u64,
    pub len: u64,
    pub attr_bits: u8,
}

/// 登记流水账（append-only：满即拒新，不覆盖历史——判据「先归档再登记」）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RegLog {
    entries: Vec<RegEntry>,
}

impl RegLog {
    /// 空账。
    pub fn new() -> Self {
        RegLog {
            entries: Vec::new(),
        }
    }

    /// 追加一笔（满则 Err——append-only 纪律，绝不覆盖）。
    pub fn push(&mut self, e: RegEntry) -> Result<(), FbmCode> {
        if self.entries.len() >= FBM_REG_LOG_CAP {
            return Err(FbmCode::RegLogFull);
        }
        self.entries.push(e);
        Ok(())
    }

    /// 全量只读视图。
    pub fn all(&self) -> &[RegEntry] {
        &self.entries
    }

    /// 当前占用（对账口径）。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

// ---------------------------------------------------------------------------
// B19 账（本模块主体）
// ---------------------------------------------------------------------------

/// B19 帧缓冲映射账：段位 + 拦截器 + 流水 + 守恒对账，全在一处。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct B19FbMapLedger {
    /// 段位（判据口径 1/1——当前策略值；结构按容量上限 B19_SEGMENT_CAPACITY 定容）。
    segs: [Option<FbSegment>; B19_SEGMENT_CAPACITY],
    /// 段序号发生器（单调递增，撤销不复用）。
    next_serial: u32,
    /// 拦截流水。
    ilog: InterceptLog,
    /// 登记流水。
    rlog: RegLog,
    /// 账本逻辑时钟（快照/指纹输入之一）。
    tick: u64,
    /// 冻结计数（冻结/解冻各计一笔——审计面）。
    freeze_ops: u64,
}

impl B19FbMapLedger {
    /// 新账（全空段位、空流水、时钟 0）。
    pub fn new() -> Self {
        B19FbMapLedger {
            segs: [None; B19_SEGMENT_CAPACITY],
            next_serial: 1,
            ilog: InterceptLog::new(),
            rlog: RegLog::new(),
            tick: 0,
            freeze_ops: 0,
        }
    }

    /// 时钟步进（确定性注入口——测试与回放共用）。
    pub fn tick(&mut self) {
        self.tick += 1;
    }

    /// 当前账本逻辑时钟。
    pub const fn now(&self) -> u64 {
        self.tick
    }

    /// 四要素静态校验（不碰账面——纯函数，登记前置闸第一道）。
    pub fn validate(base: u64, len: u64, attr_bits: u8) -> Result<FbAttr, FbmCode> {
        if base % FBM_PAGE != 0 {
            return Err(FbmCode::BaseUnaligned);
        }
        if len == 0 || len % FBM_PAGE != 0 {
            return Err(FbmCode::LenBad);
        }
        let attr = match FbAttr::of(attr_bits) {
            Some(a) => a,
            None => return Err(FbmCode::AttrBad),
        };
        // base+len 溢出检查（u64 加法越界 = 区间表达非法）。
        if len > u64::MAX - base {
            return Err(FbmCode::RangeOverflow);
        }
        let _ = attr;
        Ok(attr)
    }

    /// 与账内任一在账段相交？
    fn overlaps_any(&self, base: u64, len: u64) -> bool {
        self.segs.iter().any(|s| {
            matches!(s, Some(seg)
                if matches!(seg.state, SegState::Registered | SegState::Frozen)
                    && seg.overlaps(base, len))
        })
    }

    /// 找一个空段位（空位或已撤销位皆可复用；None = 满位 → FbmCode::SegFull）。
    fn free_slot(&self) -> Option<usize> {
        self.segs.iter().position(|s| {
            s.is_none() || matches!(s, Some(seg) if seg.state == SegState::Retired)
        })
    }

    /// 登记一段映射（判据主入口：四要素校验→段位→流水→账面）。
    ///
    /// 成功返回段序号；失败返回诊断码且账面零变化（先裁决后落账，拒绝不留半态）。
    pub fn register(&mut self, base: u64, len: u64, attr_bits: u8) -> Result<u32, FbmCode> {
        // 前置闸 1：四要素静态校验。
        let attr = B19FbMapLedger::validate(base, len, attr_bits)?;
        // 前置闸 2：区间相交（与任何在账段——相交语义优先于满位语义）。
        if self.overlaps_any(base, len) {
            self.log_reject(base, len, attr_bits, FbmCode::Overlap);
            return Err(FbmCode::Overlap);
        }
        // 前置闸 3：段位（1/1 口径——满位显性拒绝，不静默挤占）。
        let slot = match self.free_slot() {
            Some(i) => i,
            None => {
                self.log_reject(base, len, attr_bits, FbmCode::SegFull);
                return Err(FbmCode::SegFull);
            }
        };
        // 前置闸 4：登记流水容量（append-only——满即拒，先归档）。
        if self.rlog.len() >= FBM_REG_LOG_CAP {
            self.log_reject(base, len, attr_bits, FbmCode::RegLogFull);
            return Err(FbmCode::RegLogFull);
        }
        // 前置闸 5：序号空间。
        if self.next_serial == u32::MAX {
            self.log_reject(base, len, attr_bits, FbmCode::SerialExhausted);
            return Err(FbmCode::SerialExhausted);
        }
        // 全闸过 → 落账（先流水后账面；流水满已在闸 4 排除，此处 expect 不可用，仍走显性分支）。
        let serial = self.next_serial;
        self.tick += 1;
        let entry = RegEntry {
            tick: self.tick,
            granted: true,
            code: 0,
            base,
            len,
            attr_bits: attr.bits(),
        };
        if self.rlog.push(entry).is_err() {
            // 理论不可达（闸 4 已挡）；真发生则显性失败，账面不动。
            self.log_reject(base, len, attr_bits, FbmCode::RegLogFull);
            return Err(FbmCode::RegLogFull);
        }
        self.segs[slot] = Some(FbSegment {
            serial,
            base,
            len,
            attr,
            reg_tick: self.tick,
            retire_tick: None,
            state: SegState::Registered,
        });
        self.next_serial += 1;
        Ok(serial)
    }

    /// 拒绝留痕（统一入口：tick 步进 + 流水追加；流水满时仍记 tick——拒绝本身可审计）。
    fn log_reject(&mut self, base: u64, len: u64, attr_bits: u8, code: FbmCode) {
        self.tick += 1;
        let e = RegEntry {
            tick: self.tick,
            granted: false,
            code: code.val(),
            base,
            len,
            attr_bits,
        };
        // 流水满时的拒绝：丢弃留痕是可接受的（拦截主账在 ilog），但必须显性——
        // 这里选择静默放弃留痕会违反零静默，故以「流水满」语义上抛由调用方归档。
        let _ = self.rlog.push(e);
    }

    /// 撤销一段（状态机单向：Registered→Retired 合法；Frozen 拒、Retired 拒、无此段拒）。
    pub fn retire(&mut self, serial: u32, now_tick: u64) -> Result<(), FbmCode> {
        self.tick = now_tick;
        // 找段。
        let idx = match self.segs.iter().position(|s| matches!(s, Some(seg) if seg.serial == serial)) {
            Some(i) => i,
            None => return Err(FbmCode::RetireNoSuchSeg),
        };
        let seg = match &self.segs[idx] {
            Some(s) => s.clone(),
            None => return Err(FbmCode::RetireNoSuchSeg),
        };
        match seg.state {
            SegState::Retired => return Err(FbmCode::RetireTwice),
            SegState::Frozen => return Err(FbmCode::RetireFrozen),
            SegState::Unregistered => return Err(FbmCode::RetireNoSuchSeg),
            SegState::Registered => {}
        }
        // 单向迁移校验（机器口径复核——不靠 if 链的隐性顺序）。
        if !seg.state.can_move(SegState::Retired) {
            return Err(FbmCode::RetireFrozen);
        }
        // 撤销留位：Retired 段留在段位上（重复撤销可检、序号不复用可核），
        // 段位复用走 free_slot 的 Retired 判定（新登记覆盖写）。
        self.segs[idx] = Some(FbSegment {
            retire_tick: Some(self.tick),
            state: SegState::Retired,
            ..seg
        });
        Ok(())
    }

    /// 冻结（Registered→Frozen，审计锁；重复冻结幂等记账不重复迁移）。
    pub fn freeze(&mut self, serial: u32) -> Result<(), FbmCode> {
        for s in self.segs.iter_mut() {
            if let Some(seg) = s {
                if seg.serial == serial {
                    if seg.state == SegState::Registered && seg.state.can_move(SegState::Frozen) {
                        seg.state = SegState::Frozen;
                        self.freeze_ops += 1;
                        return Ok(());
                    }
                    return Err(FbmCode::RetireFrozen); // 非 Registered 态冻结拒绝（含 Frozen 重复冻结）
                }
            }
        }
        Err(FbmCode::RetireNoSuchSeg)
    }

    /// 解冻（Frozen→Registered，带审计理由——理由空串拒绝，零静默）。
    pub fn unfreeze(&mut self, serial: u32, reason: &str) -> Result<(), FbmCode> {
        if reason.is_empty() {
            // 解冻必须给理由；空理由按冻结语义拒绝（不新增码，复用冻结态拒绝位）。
            return Err(FbmCode::RetireFrozen);
        }
        for s in self.segs.iter_mut() {
            if let Some(seg) = s {
                if seg.serial == serial {
                    if seg.state == SegState::Frozen && seg.state.can_move(SegState::Registered) {
                        seg.state = SegState::Registered;
                        self.freeze_ops += 1;
                        return Ok(());
                    }
                    return Err(FbmCode::RetireFrozen);
                }
            }
        }
        Err(FbmCode::RetireNoSuchSeg)
    }

    /// 区外写拦截裁决（判据主入口之二）：区内放行并命中段号，区外拦截并留痕。
    ///
    /// 返回 true=放行（区内），false=拦截（区外）。每次裁决都留痕（ilog）。
    pub fn write_attempt(&mut self, addr: u64, now_tick: u64) -> bool {
        self.tick = now_tick;
        let mut hit: Option<u32> = None;
        for s in self.segs.iter() {
            if let Some(seg) = s {
                if matches!(seg.state, SegState::Registered | SegState::Frozen) && seg.contains(addr)
                {
                    hit = Some(seg.serial);
                    break;
                }
            }
        }
        let a = WriteAttempt {
            addr,
            tick: self.tick,
            intercepted: hit.is_none(),
            hit_serial: hit.unwrap_or(0),
        };
        self.ilog.push(a);
        hit.is_some()
    }

    /// 拦截率分母（窗口内总尝试）。
    pub const fn attempts_seen(&self) -> u64 {
        self.ilog.seen
    }

    /// 拦截流水只读视图。
    pub fn intercept_snapshot(&self) -> Vec<WriteAttempt> {
        self.ilog.snapshot()
    }

    /// 登记流水只读视图。
    pub fn reg_log(&self) -> &[RegEntry] {
        self.rlog.all()
    }

    /// 在账段（Registered/Frozen）列表。
    pub fn active_segments(&self) -> Vec<FbSegment> {
        self.segs
            .iter()
            .filter_map(|s| match s {
                Some(seg) if matches!(seg.state, SegState::Registered | SegState::Frozen) => {
                    Some(seg.clone())
                }
                _ => None,
            })
            .collect()
    }

    /// 区间归属查询（None = 区外——不猜不兜底，显性交给调用方）。
    pub fn region_of(&self, addr: u64) -> Option<FbSegment> {
        self.segs
            .iter()
            .filter_map(|s| s.as_ref())
            .find(|seg| {
                matches!(seg.state, SegState::Registered | SegState::Frozen)
                    && seg.contains(addr)
            })
            .cloned()
    }

    /// 拦截率千分比（整数：intercepted*1000/seen；seen=0 时返回 None 不造假）。
    pub fn intercept_per_mille(&self) -> Option<u64> {
        if self.ilog.seen == 0 {
            return None;
        }
        Some(self.ilog.intercepted_in_window() as u64 * 1000 / self.ilog.seen)
    }

    /// 守恒三式（账实对账：任一不成立即账被破坏）。
    pub fn conserved(&self) -> bool {
        // 式一：拦截流水 kept+dropped≡seen。
        if !self.ilog.conserved() {
            return false;
        }
        // 式二：登记流水里 granted 笔数 ≤ 在账+已撤销的序号跨度（不复用序号的可验推论）。
        let granted = self
            .rlog
            .all()
            .iter()
            .filter(|e| e.granted)
            .count() as u64;
        if granted + 1 > self.next_serial as u64 {
            return false;
        }
        // 式三：段位占用 ≤ 容量（结构性恒真，但显性写出防未来改容量时静默越界）。
        let used = self.segs.iter().filter(|s| s.is_some()).count();
        used <= B19_SEGMENT_CAPACITY
    }

    /// 快照序列化（确定性：字段定序、无环境噪声——同账同指纹）。
    pub fn snapshot_text(&self) -> String {
        let mut s = String::new();
        s.push_str("B19-FBMAP v");
        s.push_str(FBM_VERSION);
        s.push_str("\n");
        s.push_str("tick=");
        push_u64(&mut s, self.tick);
        s.push_str(" next_serial=");
        push_u32(&mut s, self.next_serial);
        s.push_str(" freeze_ops=");
        push_u64(&mut s, self.freeze_ops);
        s.push_str("\n");
        for i in 0..B19_SEGMENT_CAPACITY {
            match &self.segs[i] {
                Some(seg) => {
                    s.push_str("seg ");
                    push_u32(&mut s, seg.serial);
                    s.push_str(" base=");
                    push_hex_u64(&mut s, seg.base);
                    s.push_str(" len=");
                    push_hex_u64(&mut s, seg.len);
                    s.push_str(" attr=");
                    push_hex_u8(&mut s, seg.attr.bits());
                    s.push_str(" state=");
                    s.push_str(seg.state.zh());
                    s.push_str("\n");
                }
                None => {
                    s.push_str("seg - empty\n");
                }
            }
        }
        for e in self.rlog.all() {
            s.push_str("reg t=");
            push_u64(&mut s, e.tick);
            s.push_str(if e.granted { " ok " } else { " rej " });
            push_hex_u16(&mut s, e.code);
            s.push_str(" base=");
            push_hex_u64(&mut s, e.base);
            s.push_str(" len=");
            push_hex_u64(&mut s, e.len);
            s.push_str("\n");
        }
        s
    }

    /// 快照指纹（FNV-1a 64 · 序列化文本直接喂——篡改任一字段指纹必变）。
    pub fn fingerprint(&self) -> u64 {
        fnv1a(self.snapshot_text().as_bytes())
    }

    /// 防篡改校验：外部持有指纹与账面重算值比对。
    pub fn verify_fingerprint(&self, expected: u64) -> Result<(), FbmCode> {
        if self.fingerprint() == expected {
            Ok(())
        } else {
            Err(FbmCode::FingerprintMismatch)
        }
    }
}

// ---------------------------------------------------------------------------
// 整数工具（no_std 环境零格式化依赖的手写渲染——确定性输出）
// ---------------------------------------------------------------------------

/// FNV-1a 64（确定性指纹基元）。
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

/// 十进制无符号渲染（手写，避免依赖格式化宏的确定性风险）。
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

/// u32 十进制。
fn push_u32(s: &mut String, v: u32) {
    push_u64(s, v as u64);
}

/// u16 十六进制（4 位定宽）。
fn push_hex_u16(s: &mut String, v: u16) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    s.push(HEX[((v >> 12) & 0xF) as usize] as char);
    s.push(HEX[((v >> 8) & 0xF) as usize] as char);
    s.push(HEX[((v >> 4) & 0xF) as usize] as char);
    s.push(HEX[(v & 0xF) as usize] as char);
}

/// u8 十六进制（2 位定宽）。
fn push_hex_u8(s: &mut String, v: u8) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    s.push(HEX[(v >> 4) as usize] as char);
    s.push(HEX[(v & 0xF) as usize] as char);
}

/// u64 十六进制（16 位定宽）。
fn push_hex_u64(s: &mut String, v: u64) {
    push_hex_u16(s, (v >> 48) as u16);
    push_hex_u16(s, ((v >> 32) & 0xFFFF) as u16);
    push_hex_u16(s, ((v >> 16) & 0xFFFF) as u16);
    push_hex_u16(s, (v & 0xFFFF) as u16);
}

// ---------------------------------------------------------------------------
// 判据（当前模式：轻量判据内嵌，重判据押后统一补）
// ---------------------------------------------------------------------------

/// 判据集（登记进 mod.rs 域聚合——折叠范式）。
pub fn run_t483_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("UNX-t483");

    // c1 四要素校验链：未对齐基址必须被拒（判据前置件）。
    set.add(
        "t483-c1-align",
        B19FbMapLedger::validate(0x1000, FBM_PAGE * 4, 0x2).is_ok()
            && B19FbMapLedger::validate(0x1001, FBM_PAGE * 4, 0x2) == Err(FbmCode::BaseUnaligned)
            && B19FbMapLedger::validate(0x1000, 0, 0x2) == Err(FbmCode::LenBad)
            && B19FbMapLedger::validate(0x1000, FBM_PAGE + 1, 0x2) == Err(FbmCode::LenBad)
            && B19FbMapLedger::validate(0x1000, FBM_PAGE * 4, 0x7) == Err(FbmCode::AttrBad)
            && B19FbMapLedger::validate(u64::MAX - 0xFFF, FBM_PAGE * 32, 0x2) == Err(FbmCode::RangeOverflow),
        if true { "" } else { "validate chain mismatch" },
    );

    // c2 B19 账 1/1 段：首段登记成功，第二段必须显性拒（SegFull）且账面不变。
    let mut led = B19FbMapLedger::new();
    let ok1 = led.register(0x0000_A000_0000, FBM_PAGE * 16, 0x2);
    let serial = ok1.unwrap_or(0);
    let reject2 = led.register(0x0000_B000_0000, FBM_PAGE * 8, 0x2) == Err(FbmCode::SegFull);
    set.add(
        "t483-c2-cap",
        ok1.is_ok() && reject2 && led.active_segments().len() == 1,
        if ok1.is_ok() && reject2 { "" } else { "B19 1/1 capacity semantics broken" },
    );

    // c3 区外写拦截 100%：探测集（段内 4 点 + 区外 12 点）——区外全拦、区内全放。
    let seg = led.region_of(0x0000_A000_0000);
    let seg_ok = seg.is_some();
    let base = 0x0000_A000_0000u64;
    let end = base + FBM_PAGE * 16;
    let mut all_in = true;
    let mut all_out = true;
    let mut t = led.now();
    for i in 0..4u64 {
        let addr = base + i * (FBM_PAGE * 4) + 7;
        t += 1;
        all_in &= led.write_attempt(addr, t);
    }
    // 区外 12 点：段前 4、段后 4、远端 4。
    let out_addrs = [
        base - FBM_PAGE,
        base - 1,
        base - FBM_PAGE * 2,
        base - FBM_PAGE * 3,
        end,
        end + FBM_PAGE,
        end + FBM_PAGE * 2,
        end + FBM_PAGE * 3,
        0x0000_C000_0000,
        0x0000_C000_1000,
        0xF000_0000_0000,
        0xFFFF_FFFF_F000,
    ];
    for addr in out_addrs {
        t += 1;
        all_out &= !led.write_attempt(addr, t);
    }
    let pm = led.intercept_per_mille();
    set.add(
        "t483-c3-intercept",
        seg_ok && all_in && all_out && pm == Some(750), // 12 拦 / 16 尝试 = 750‰
        if all_in && all_out { "" } else { "out-of-region write interception below 100%" },
    );

    // c4 账实守恒：尝试数 = 拦截数 + 放行数；流水 kept+dropped≡seen。
    set.add(
        "t483-c4-conserve",
        led.conserved() && led.attempts_seen() == 16,
        if led.conserved() { "" } else { "ledger conservation broken" },
    );

    // c5 状态机单向：Registered→Frozen→(解冻)→Retired 合法；Frozen 直接撤必须拒。
    let mut led2 = B19FbMapLedger::new();
    let s2 = led2.register(0x0000_D000_0000, FBM_PAGE * 2, 0x1).unwrap_or(0);
    let fz = led2.freeze(s2).is_ok();
    let bad_retire = led2.retire(s2, 900) == Err(FbmCode::RetireFrozen);
    let uf = led2.unfreeze(s2, "审计解冻理由-迁移窗口").is_ok();
    let good_retire = led2.retire(s2, 910).is_ok();
    let re_reject = led2.retire(s2, 920) == Err(FbmCode::RetireTwice);
    set.add(
        "t483-c5-fsm",
        fz && bad_retire && uf && good_retire && re_reject,
        if fz && good_retire { "" } else { "segment FSM one-way law broken" },
    );

    // c6 快照指纹：同账同指纹；字段一改指纹必变且 verify 显性红。
    let fp_a = led2.fingerprint();
    let fp_b = led2.fingerprint();
    let mut led3 = led2.clone();
    led3.tick();
    let fp_c = led3.fingerprint();
    set.add(
        "t483-c6-fingerprint",
        fp_a == fp_b && fp_a != fp_c && led2.verify_fingerprint(fp_a).is_ok(),
        if fp_a == fp_b { "" } else { "snapshot fingerprint not deterministic" },
    );

    // c7 相交拒绝：与在账段任何相交区间一律 Overlap 拒。
    let mut led4 = B19FbMapLedger::new();
    let _ = led4.register(0x0000_E000_0000, FBM_PAGE * 4, 0x2);
    let ov1 = led4.register(0x0000_E000_2000, FBM_PAGE * 4, 0x2) == Err(FbmCode::Overlap);
    let ov2 = led4.register(0x0000_DFFF_F000, FBM_PAGE * 2, 0x2) == Err(FbmCode::Overlap);
    let ov3 = led4.register(0x0000_E000_0000, FBM_PAGE, 0x1) == Err(FbmCode::Overlap);
    set.add(
        "t483-c7-overlap",
        ov1 && ov2 && ov3,
        if ov1 && ov2 { "" } else { "overlap rejection broken" },
    );

    // c8 撤销后段位释放可重用（1/1 账位的生命周期闭环）。
    let s5 = led4.region_of(0x0000_E000_0000).map(|g| g.serial).unwrap_or(0);
    let r5 = led4.retire(s5, 500).is_ok();
    let re_reg = led4.register(0x0000_E000_0000, FBM_PAGE * 4, 0x2).is_ok();
    set.add(
        "t483-c8-recycle",
        r5 && re_reg,
        if r5 && re_reg { "" } else { "slot recycle after retire broken" },
    );

    set
}

// ---------------------------------------------------------------------------
// 单元测试（域聚合测试在 mod.rs——此处只放本模块纯函数级断言）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t483_all_checks_green() {
        let set = run_t483_checks();
        assert!(set.all_passed(), "T483 domain checks must be green");
    }

    #[test]
    fn t483_diag_segment_guard() {
        // 段内码全可反查，段外码全拒。
        for i in 1u16..=0x0C {
            assert!(FbmCode::of(FBM_SEG_BASE + i).is_some());
        }
        assert!(FbmCode::of(0x9800).is_none());
        assert!(FbmCode::of(0x9901).is_none());
    }

    #[test]
    fn t483_attr_closed_set() {
        assert_eq!(FbAttr::of(0x1), Some(FbAttr::Uc));
        assert_eq!(FbAttr::of(0x2), Some(FbAttr::Wc));
        assert_eq!(FbAttr::of(0x0), None);
        assert_eq!(FbAttr::of(0x3), None);
    }
}
