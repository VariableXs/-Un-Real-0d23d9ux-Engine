//! UNX-F9204 close 坏 fd 矩阵（含双关闭竞态） · 0x7A080段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9204 逐字）：
//!   ① 纯功能代码 ≥320 行；② UNX-F9204-J1 坏 fd 全 EBADF 判据过，
//!   双 close 竞态注入第二次全 EBADF 格过，fd 槽位复用零泄漏格过；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点（本卡域模型：close(2) 释放面 + 句柄生命周期，与 F9202 的
//! read/write 方向面完全不同——这里没有缓冲区与指针，核心是「谁有权
//! 关、关几次、关完之后号还能不能信」）：
//!   ① close 的错误面以 fd 有效性为唯一前置：负值/未开（Free）/
//!      已关（Closed）/越上界四类全部回 EBADF，且先于一切其他判定
//!      （包括 EINTR 路径）——fd 校验不过就绝不触碰槽位、绝不记账。
//!   ② 双关闭竞态：同一句柄连续两次 close，恰好一次成功（赢家唯一）；
//!      第二次必回 EBADF 并归入 DoubleClose 诊断码。close 不是幂等
//!      操作，这是与 dup/引用计数语义的分水岭。
//!   ③ EINTR 特殊语义（Linux 特有）：close 报 EINTR 时 fd 仍已释放，
//!      不存在「EINTR 后 fd 仍打开」的第三态；因此竞态里首关报 EINTR
//!      不改变第二次必 EBADF 的判据。判定次序上 EBADF 仍先于 EINTR。
//!   ④ 槽位复用代际守卫：close 推进槽位代际，同号新分配携带新代际，
//!      旧代际句柄全部失效。Linux 对用户态只有裸 fd（无代际概念），
//!      代际守卫是内核内部分账的增强层；正因如此，裸 fd 的 close 对
//!      复用槽仍然成功（与 Linux 一致），而带旧代际的内核句柄被拒。
//!   ⑤ 零泄漏对账：动态槽恒等式 allocs = closes + live 在每次
//!      open/close 之后都必须成立；容量耗尽回 -EMFILE（24）且账目
//!      完好，关一槽立即复开一槽验证可回收。
//!   ⑥ 确定性：规范语料（正常关/双关/EINTR 关/三类坏 fd/代际探针/
//!      标准流关闭）双跑指纹逐字节一致，无随机、无浮点、无墙钟。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9204 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7A080;

/// 常驻标准流数量（0=stdin，1=stdout，2=stderr）。
pub const STD_FDS: i32 = 3;

/// 动态 fd 槽数量（fd 号 3..=50）。
pub const MAX_FDS: usize = 48;

/// 单类坏 fd 注入的复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// 双关闭竞态注入轮数（判据要求第二次全 EBADF）。
pub const RACE_ROUNDS: u32 = 10;

/// 坏 close 注入类别数（负值/未开/双关/越界/陈旧代际）。
pub const BAD_CLOSE_CLASSES: usize = 5;

/// 槽位复用代际守卫的循环轮数。
pub const REUSE_CYCLES: usize = 3;

/// 打开模式位：只读。
pub const MODE_RO: u32 = 1;
/// 打开模式位：只写。
pub const MODE_WO: u32 = 2;
/// 打开模式位：读写。
pub const MODE_RW: u32 = MODE_RO | MODE_WO;

/// Linux errno 数值（正数口径；系统调用返回负值）。
pub const EINTR: i32 = 4;
pub const EBADF: i32 = 9;
pub const EMFILE: i32 = 24;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内闭集，12 个码两两唯一）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CloseCode {
    /// 负 fd：fd < 0
    NegativeFd,
    /// 未打开：槽位仍为 Free
    NotOpened,
    /// 已关闭：对 Closed 槽再次 close
    AlreadyClosed,
    /// 双关闭竞态：同一句柄第二次 close
    DoubleClose,
    /// 陈旧代际：同号已复用但句柄代际落后
    StaleGeneration,
    /// 越上界：号大于动态槽上限
    BeyondLimit,
    /// fd 表耗尽：open 回 -EMFILE
    TableFull,
    /// EINTR 已释放：报 EINTR 但 fd 仍被释放
    EintrReleased,
    /// 泄漏嫌疑：allocs = closes + live 对账不成立
    LeakSuspected,
    /// 注入账倒退
    LedgerRegress,
    /// 双跑不一致
    NotReproducible,
    /// 段位越界
    SegmentViolation,
}

impl CloseCode {
    /// 码值（段基址 + 变体序号）。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            CloseCode::NegativeFd => "负 fd 被拒：close 的 fd 参数小于 0 属非法号段；\
                请检查句柄来源，负值一律回 EBADF",
            CloseCode::NotOpened => "未打开 fd 被拒：目标槽位从未分配；close 不得对 \
                Free 槽生效，请先 open/pipe/dup 取得有效句柄",
            CloseCode::AlreadyClosed => "已关闭 fd 被拒：槽位处于 Closed 态；重复 \
                close 不是幂等操作，请以第一次的返回值为准",
            CloseCode::DoubleClose => "双关闭竞态被拒：同一句柄第二次 close 必须 \
                EBADF；并发路径上请保证只有一个所有者执行 close",
            CloseCode::StaleGeneration => "陈旧代际被拒：同号槽位已复用且代际推进，\
                旧代际句柄失效；请用最新代际重建内核句柄",
            CloseCode::BeyondLimit => "fd 越上界被拒：号大于动态槽上限；请按 \
                3..3+MAX_FDS 约束分配，越界回 EBADF",
            CloseCode::TableFull => "fd 表耗尽：动态槽全 Open，open 回 -EMFILE；\
                请先 close 腾槽或扩大 MAX_FDS 预算",
            CloseCode::EintrReleased => "EINTR 已释放：close 报 EINTR 时 fd 仍被\
                释放（Linux 语义）；不得重试 close 期待再次成功",
            CloseCode::LeakSuspected => "泄漏嫌疑：allocs = closes + live 对账不\
                成立；请排查丢失的 close 路径或重复记账",
            CloseCode::LedgerRegress => "注入账倒退被拒：计数只增不减；请检查\
                回滚路径是否误改累计值",
            CloseCode::NotReproducible => "双跑不一致：同输入两次结果不同；请\
                排查隐藏状态或非确定序列",
            CloseCode::SegmentViolation => "段位越界：本模块段基址必须落在 \
                0x7A000..=0x7D1C0 且 0x40 对齐",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [CloseCode; 12] = [
        CloseCode::NegativeFd,
        CloseCode::NotOpened,
        CloseCode::AlreadyClosed,
        CloseCode::DoubleClose,
        CloseCode::StaleGeneration,
        CloseCode::BeyondLimit,
        CloseCode::TableFull,
        CloseCode::EintrReleased,
        CloseCode::LeakSuspected,
        CloseCode::LedgerRegress,
        CloseCode::NotReproducible,
        CloseCode::SegmentViolation,
    ];
}

/// 系统调用结果：成功携带效果，失败携带 Linux errno 正数码。
pub type SysResult<T> = Result<T, i32>;

// ── fd 表状态机 ───────────────────────────────────────────────────────
/// 槽位三态。Free 与 Closed 必须分开：前者表示从未分配（close 语义上
/// 同样 EBADF，但对账与复用优先级不同），后者表示分配过且已关闭。
/// 状态机只有两条合法边：Free→Open（open/dup 分配）、Open→Closed
/// （close 释放）；Closed→Open（复用）是 open 的另一入口。
/// 非法边 Free→Closed（跳过打开直接关）与 Closed→Closed（重复关）
/// 由 close 判定器显式拒绝。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SlotState {
    /// 从未分配
    Free,
    /// 已打开
    Open,
    /// 已关闭（等待复用）
    Closed,
}

impl SlotState {
    /// 闭集全量。
    pub const ALL: [SlotState; 3] = [
        SlotState::Free,
        SlotState::Open,
        SlotState::Closed,
    ];
}

/// close 事件的合法前置态：只有 Open 态允许触发释放边；
/// Free（从未打开）与 Closed（已经关闭）一律拒绝。
pub fn closeable_state(st: SlotState) -> bool {
    match st {
        SlotState::Open => true,
        SlotState::Free | SlotState::Closed => false,
    }
}

/// 内核内部句柄：裸 fd 号 + 分配时刻的槽位代际。
/// 用户态只见裸 fd；代际是内核分账用来识别「陈旧句柄」的增强层。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Handle {
    /// 裸 fd 号
    pub fd: i32,
    /// 分配时刻的槽位代际
    pub gen: u16,
}

/// 非法句柄常量（open 失败时的占位，fd=-1 必被一切判定拒绝）。
pub const HANDLE_INVALID: Handle = Handle { fd: -1, gen: 0 };

/// 单个 fd 槽。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FdSlot {
    /// 槽位状态
    pub state: SlotState,
    /// 打开模式（MODE_RO / MODE_WO / MODE_RW；close 后清零）
    pub mode: u32,
    /// 代际：close 时自增；复用后旧代际句柄全部失效
    pub generation: u16,
    /// 最近一次分配的流水号（表级单调，用于审计）
    pub opened_seq: u32,
}

impl FdSlot {
    const fn empty() -> FdSlot {
        FdSlot {
            state: SlotState::Free,
            mode: 0,
            generation: 1,
            opened_seq: 0,
        }
    }
}

/// 定容 fd 表。标准流在 `new()` 中预置为打开态（Linux 启动镜像语义）。
/// 账目只对动态槽记账：标准流不是 open 分配的，不进 allocs/closes 账。
#[derive(Clone, Copy, Debug)]
pub struct FdTable {
    std: [FdSlot; 3],
    dyns: [FdSlot; MAX_FDS],
    /// 动态槽累计分配次数
    allocs: u32,
    /// 动态槽累计关闭次数
    closes: u32,
    /// 分配流水号（表级单调）
    seq: u32,
}

impl FdTable {
    /// 构造标准 fd 表：0=stdin 只读，1=stdout 只写，2=stderr 只写。
    pub const fn new() -> FdTable {
        let mut std0 = FdSlot::empty();
        std0.state = SlotState::Open;
        std0.mode = MODE_RO;
        let mut std1 = FdSlot::empty();
        std1.state = SlotState::Open;
        std1.mode = MODE_WO;
        let mut std2 = FdSlot::empty();
        std2.state = SlotState::Open;
        std2.mode = MODE_WO;
        FdTable {
            std: [std0, std1, std2],
            dyns: [FdSlot::empty(); MAX_FDS],
            allocs: 0,
            closes: 0,
            seq: 0,
        }
    }

    /// 槽位视图：号段非法返回 None（EBADF 第一层）。
    fn slot(&self, fd: i32) -> Option<&FdSlot> {
        if fd < 0 {
            return None;
        }
        if fd < STD_FDS {
            return self.std.get(fd as usize);
        }
        let idx = match fd.checked_sub(STD_FDS) {
            Some(v) => v as usize,
            None => return None,
        };
        self.dyns.get(idx)
    }

    /// 可变槽位视图。
    fn slot_mut(&mut self, fd: i32) -> Option<&mut FdSlot> {
        if fd < 0 {
            return None;
        }
        if fd < STD_FDS {
            return self.std.get_mut(fd as usize);
        }
        let idx = match fd.checked_sub(STD_FDS) {
            Some(v) => v as usize,
            None => return None,
        };
        self.dyns.get_mut(idx)
    }

    /// 打开一个动态槽：优先复用 Closed/Free 槽中最小号（与 Linux 低号
    /// 优先一致）。表满回 -EMFILE。返回携带当前代际的内核句柄。
    pub fn open(&mut self, mode: u32) -> SysResult<Handle> {
        let mut i = 0usize;
        while i < MAX_FDS {
            // 流水号先在块外算好，借用期内只碰槽位本身（E0503 规避）。
            let next_seq = self.seq.wrapping_add(1);
            let candidate: Option<u16> = match self.dyns.get_mut(i) {
                Some(slot) => {
                    if slot.state != SlotState::Open {
                        slot.state = SlotState::Open;
                        slot.mode = mode;
                        slot.opened_seq = next_seq;
                        Some(slot.generation)
                    } else {
                        None
                    }
                }
                None => None,
            };
            if let Some(gen) = candidate {
                self.seq = next_seq;
                self.allocs = self.allocs.saturating_add(1);
                return Ok(Handle { fd: STD_FDS + i as i32, gen });
            }
            i += 1;
        }
        Err(EMFILE)
    }

    /// 释放槽位（close 的副作用层）：Open → Closed，代际自增，模式清零。
    /// 标准流关闭不计入动态账（它不是 open 分配的，账基线不含它）。
    fn release_slot(&mut self, fd: i32) -> SysResult<u16> {
        let gen = {
            let slot = match self.slot_mut(fd) {
                Some(s) => s,
                None => return Err(EBADF),
            };
            if slot.state != SlotState::Open {
                return Err(EBADF);
            }
            slot.state = SlotState::Closed;
            slot.generation = slot.generation.wrapping_add(1);
            slot.mode = 0;
            slot.generation
        };
        if fd >= STD_FDS {
            self.closes = self.closes.saturating_add(1);
        }
        Ok(gen)
    }

    /// 裸 fd 关闭（与 Linux close(2) 对齐的账面入口）。
    /// 负值/越界/Free/Closed 全部 EBADF；成功返回新代际。
    pub fn close_raw(&mut self, fd: i32) -> SysResult<u16> {
        if fd < 0 || fd >= STD_FDS + MAX_FDS as i32 {
            return Err(EBADF);
        }
        self.release_slot(fd)
    }

    /// 只读窥探：返回 (状态, 代际)。
    pub fn peek_pair(&self, fd: i32) -> Option<(SlotState, u16)> {
        let slot = match self.slot(fd) {
            Some(s) => s,
            None => return None,
        };
        Some((slot.state, slot.generation))
    }

    /// 只读窥探：返回槽位状态。
    pub fn peek_state(&self, fd: i32) -> Option<SlotState> {
        match self.peek_pair(fd) {
            Some((st, _g)) => Some(st),
            None => None,
        }
    }

    /// 当前打开的动态槽数量（对账用 live 项）。
    pub fn live_dyn(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < MAX_FDS {
            if let Some(slot) = self.dyns.get(i) {
                if slot.state == SlotState::Open {
                    n = n.saturating_add(1);
                }
            }
            i += 1;
        }
        n
    }

    /// 零泄漏对账恒等式：allocs = closes + live。
    /// 任何一次 open/close 之后都必须成立；破坏即泄漏。
    pub fn reconcile(&self) -> bool {
        self.closes.saturating_add(self.live_dyn() as u32) == self.allocs
    }

    /// 动态槽累计分配次数。
    pub fn allocs(&self) -> u32 {
        self.allocs
    }

    /// 动态槽累计关闭次数。
    pub fn closes(&self) -> u32 {
        self.closes
    }

    /// 分配流水号（表级单调，审计用）。
    pub fn seq(&self) -> u32 {
        self.seq
    }
}

// ── close 判定器 ──────────────────────────────────────────────────────
/// close 判定 flavored 版本：flavor 建模底层「收尾被信号打断」的可能性。
/// 判定次序（与 Linux 一致）：
///   1) fd 号段/槽位状态校验 → 不过即 EBADF（先于一切，含 EINTR）；
///   2) 释放槽位（副作用不可回滚）；
///   3) flavor=Interrupted 时返回 -EINTR，但 fd 已释放。
pub fn judge_close(table: &mut FdTable, fd: i32, flavor: CloseFlavor) -> Result<(), i32> {
    // 第 1 层：fd 有效性。EBADF 优先于一切其他错误码。
    if fd < 0 || fd >= STD_FDS + MAX_FDS as i32 {
        return Err(EBADF);
    }
    {
        let st = match table.peek_state(fd) {
            Some(st) => st,
            None => return Err(EBADF),
        };
        if !closeable_state(st) {
            return Err(EBADF);
        }
    }
    // 第 2 层：释放。Linux 语义——即使随后报 EINTR，fd 也已释放，
    // 不存在「EINTR 后 fd 仍打开」的第三态，重试 close 只会得 EBADF。
    match table.release_slot(fd) {
        Ok(_new_gen) => {}
        Err(e) => return Err(e),
    }
    // 第 3 层：按 flavor 上报。
    match flavor {
        CloseFlavor::Clean => Ok(()),
        CloseFlavor::Interrupted => Err(EINTR),
    }
}

/// 代际守卫版 close 判定器：句柄必须携带最新代际。
/// 陈旧句柄在触碰槽位之前即被拒（不产生任何副作用）——这是双关闭
/// 竞态之外的第二道防线：即便槽位已被复用，旧句柄也打不到新文件。
pub fn judge_close_handle(table: &mut FdTable, h: Handle, flavor: CloseFlavor) -> Result<(), i32> {
    // 第 1 层：fd 号段校验。
    if h.fd < 0 || h.fd >= STD_FDS + MAX_FDS as i32 {
        return Err(EBADF);
    }
    // 第 2 层：槽位状态 + 代际校验（无副作用）。
    {
        let pair = match table.peek_pair(h.fd) {
            Some(p) => p,
            None => return Err(EBADF),
        };
        let (st, gen) = pair;
        match st {
            SlotState::Open => {
                if gen != h.gen {
                    return Err(EBADF);
                }
            }
            SlotState::Free | SlotState::Closed => return Err(EBADF),
        }
    }
    // 第 3 层：释放 + 上报，与 judge_close 同一副作用层。
    match table.release_slot(h.fd) {
        Ok(_new_gen) => {}
        Err(e) => return Err(e),
    }
    match flavor {
        CloseFlavor::Clean => Ok(()),
        CloseFlavor::Interrupted => Err(EINTR),
    }
}

/// 底层收尾 flavor：Clean 一次成功；Interrupted 建模收尾被信号打断
/// （返回 -EINTR，但 fd 释放已完成——Linux 特有语义）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CloseFlavor {
    /// 正常收尾
    Clean,
    /// 收尾被中断（fd 仍被释放）
    Interrupted,
}

/// 竞态第二次 close 的归类：Err(EBADF) → DoubleClose（判据要求的
/// 「第二次全 EBADF」），其余一律 NotReproducible（显性可查）。
pub fn classify_second_close(r: Result<(), i32>) -> CloseCode {
    match r {
        Err(EBADF) => CloseCode::DoubleClose,
        _ => CloseCode::NotReproducible,
    }
}

// ── 坏 close 注入矩阵 ────────────────────────────────────────────────
/// 五类坏 close 样本（负值/未开/双关/越界/陈旧代际）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadCloseCase {
    /// 负值 fd
    Negative,
    /// 从未打开的空闲槽
    NeverOpened,
    /// 已关闭槽（双关闭的静态形态）
    DoubleClosed,
    /// 越过动态槽上界
    BeyondLimit,
    /// 槽位已复用但句柄代际落后
    StaleGen,
}

/// 坏 close 语料夹具：从一张真实 fd 表构造五类样本的实体。
#[derive(Clone, Copy, Debug)]
pub struct BadFixture {
    /// 打开→关闭→复用后的旧代际句柄（槽位现在 Open 且代际已推进）
    pub stale: Handle,
    /// 打开→关闭后的句柄（槽位 Closed，双关闭样本）
    pub closed: Handle,
    /// 从未打开的动态槽 fd
    pub never_fd: i32,
    /// 越上界 fd
    pub beyond_fd: i32,
}

/// 构造夹具：fd3 开→关→复用（陈旧代际样本），fd4 开→关（双关闭样本）。
pub fn build_bad_fixture() -> BadFixture {
    let (_, f) = build_bad_fixture_pair();
    f
}

/// 构造「表+夹具」配对：open 序列确定性（fd3 开→关、fd4 开→关、fd3 复用），
/// 句柄号与代际跨调用一致，供注入循环每轮重建独立判定环境。
pub fn build_bad_fixture_pair() -> (FdTable, BadFixture) {
    let mut t = FdTable::new();
    let a = match t.open(MODE_RW) {
        Ok(h) => h,
        Err(_) => HANDLE_INVALID,
    };
    let b = match t.open(MODE_RW) {
        Ok(h) => h,
        Err(_) => HANDLE_INVALID,
    };
    let _ = t.close_raw(a.fd);
    let _ = t.close_raw(b.fd);
    // 复用 a 槽：同号新分配带新代际，a 的旧句柄自此陈旧。
    let _reuse = t.open(MODE_RW);
    let f = BadFixture {
        stale: a,
        closed: b,
        never_fd: STD_FDS + MAX_FDS as i32 - 1,
        beyond_fd: STD_FDS + MAX_FDS as i32,
    };
    (t, f)
}

impl BadCloseCase {
    /// 构造样本句柄（gen 仅对 StaleGen/DoubleClosed 有意义）。
    pub fn probe(self, f: &BadFixture) -> Handle {
        match self {
            BadCloseCase::Negative => Handle { fd: -3, gen: 0 },
            BadCloseCase::NeverOpened => Handle { fd: f.never_fd, gen: 0 },
            BadCloseCase::DoubleClosed => f.closed,
            BadCloseCase::BeyondLimit => Handle { fd: f.beyond_fd, gen: 0 },
            BadCloseCase::StaleGen => f.stale,
        }
    }

    /// 期望诊断码（每个样本必须归到正确的码，不许笼统「错了就行」）。
    pub fn expected(self) -> CloseCode {
        match self {
            BadCloseCase::Negative => CloseCode::NegativeFd,
            BadCloseCase::NeverOpened => CloseCode::NotOpened,
            BadCloseCase::DoubleClosed => CloseCode::AlreadyClosed,
            BadCloseCase::BeyondLimit => CloseCode::BeyondLimit,
            BadCloseCase::StaleGen => CloseCode::StaleGeneration,
        }
    }

    /// 陈旧代际样本上，裸 fd 的 close 仍应成功（Linux 无代际概念，
    /// 用户态 close(fd) 对复用槽合法）——这正是代际守卫存在的理由。
    pub fn expects_raw_ok(self) -> bool {
        matches!(self, BadCloseCase::StaleGen)
    }

    /// 闭集全量。
    pub const ALL: [BadCloseCase; 5] = [
        BadCloseCase::Negative,
        BadCloseCase::NeverOpened,
        BadCloseCase::DoubleClosed,
        BadCloseCase::BeyondLimit,
        BadCloseCase::StaleGen,
    ];
}

/// 无副作用归类：给定表与句柄，判出它踩中的诊断码；合法句柄返回 None。
pub fn close_fault_kind(table: &FdTable, h: Handle) -> Option<CloseCode> {
    if h.fd < 0 {
        return Some(CloseCode::NegativeFd);
    }
    if h.fd >= STD_FDS + MAX_FDS as i32 {
        return Some(CloseCode::BeyondLimit);
    }
    let pair = match table.peek_pair(h.fd) {
        Some(p) => p,
        None => return Some(CloseCode::BeyondLimit),
    };
    let (st, gen) = pair;
    match st {
        SlotState::Open => {
            if gen != h.gen {
                Some(CloseCode::StaleGeneration)
            } else {
                None
            }
        }
        SlotState::Free => Some(CloseCode::NotOpened),
        SlotState::Closed => Some(CloseCode::AlreadyClosed),
    }
}

/// 注入账：注入 = 检出 + 逃逸，逃逸必须恒为 0。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RaceLedger {
    /// 注入总轮数
    pub rounds: u32,
    /// 检出轮数
    pub caught: u32,
    /// 逃逸轮数（必须为 0）
    pub missed: u32,
}

impl RaceLedger {
    const fn new() -> RaceLedger {
        RaceLedger { rounds: 0, caught: 0, missed: 0 }
    }

    /// 记一轮注入结果；账不守恒时回显性错误（负的 LedgerRegress 码）。
    pub fn note(&mut self, got: bool) -> SysResult<()> {
        self.rounds = self.rounds.saturating_add(1);
        if got {
            self.caught = self.caught.saturating_add(1);
        } else {
            self.missed = self.missed.saturating_add(1);
        }
        if self.caught.saturating_add(self.missed) != self.rounds {
            return Err(-(CloseCode::LedgerRegress as i32));
        }
        Ok(())
    }

    /// 守恒式复算。
    pub fn conserved(&self) -> bool {
        self.caught.saturating_add(self.missed) == self.rounds
    }
}

/// 坏 close 矩阵结果：总账 + 逐类检出数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BadCloseOutcome {
    /// 注入总账
    pub ledger: RaceLedger,
    /// 每类检出轮数（判据：每类都必须 = INJECT_ROUNDS）
    pub per_class: [u32; BAD_CLOSE_CLASSES],
}

/// 执行五类坏 close 注入：每类 INJECT_ROUNDS 轮，全部走
/// judge_close_handle（句柄级）与 judge_close（裸 fd 级）双通道，
/// 且诊断码必须逐码归位。
pub fn run_bad_close_matrix() -> BadCloseOutcome {
    let fixture = build_bad_fixture();
    let mut ledger = RaceLedger::new();
    let mut per_class = [0u32; BAD_CLOSE_CLASSES];
    let mut ci = 0usize;
    while ci < BadCloseCase::ALL.len() {
        let case = match BadCloseCase::ALL.get(ci).copied() {
            Some(c) => c,
            None => break,
        };
        let h = case.probe(&fixture);
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            // 每轮重建确定性表（open 序列一致 → 句柄号/代际一致）：
            // 通道一走 A 表，通道二归类走 B 表快照，通道三走 C 表。
            let (mut tab_a, _) = build_bad_fixture_pair();
            let (tab_b, _) = build_bad_fixture_pair();
            let (mut tab_c, _) = build_bad_fixture_pair();
            // 通道一：句柄级判定必须 EBADF（陈旧代际在此被拒）。
            let handle_res = judge_close_handle(&mut tab_a, h, CloseFlavor::Clean);
            // 通道二：归类必须精确落码。
            let kind_ok = close_fault_kind(&tab_b, h) == Some(case.expected());
            // 通道三：裸 fd 级判定——除陈旧代际样本外都必须 EBADF
            // （陈旧样本上裸 close 成功，恰是 Linux 语义与代际增强的分界）。
            let raw_res = judge_close(&mut tab_c, h.fd, CloseFlavor::Clean);
            let raw_ok = if case.expects_raw_ok() {
                raw_res.is_ok()
            } else {
                raw_res == Err(EBADF)
            };
            let caught = handle_res == Err(EBADF) && kind_ok && raw_ok;
            let _ = ledger.note(caught);
            if caught {
                if let Some(slot) = per_class.get_mut(ci) {
                    *slot = slot.saturating_add(1);
                }
            }
            round += 1;
        }
        ci += 1;
    }
    BadCloseOutcome { ledger, per_class }
}

// ── 双关闭竞态注入器 ─────────────────────────────────────────────────
/// 一轮竞态：同一句柄连续两次 close，返回两次结果（供测试细查）。
pub fn race_close_twice(table: &mut FdTable, flavor: CloseFlavor)
    -> (Result<(), i32>, Result<(), i32>) {
    let h = match table.open(MODE_RW) {
        Ok(h) => h,
        Err(_) => HANDLE_INVALID,
    };
    let first = judge_close_handle(table, h, flavor);
    let second = judge_close_handle(table, h, CloseFlavor::Clean);
    (first, second)
}

/// 竞态矩阵结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RaceOutcome {
    /// 注入总账（每轮记 1 次竞态注入，检出=第二次被拒）
    pub ledger: RaceLedger,
    /// 第二次 close 回 EBADF 的轮数（判据：= RACE_ROUNDS）
    pub second_ebadf: u32,
    /// 槽位确实被释放且只释放一次的轮数（首关报 Ok 或 EINTR 均算释放）
    pub released: u32,
    /// 第二次结果被正确归入 DoubleClose 码的轮数
    pub kinds_ok: u32,
    /// 每轮结束后 allocs = closes + live 恒等式均成立
    pub balanced: bool,
}

/// 双关闭竞态注入：RACE_ROUNDS 轮，奇偶轮交替 Clean/EINTR flavor，
/// 覆盖「首关成功」与「首关报 EINTR 但已释放」两种竞态形态。
pub fn run_double_close_matrix() -> RaceOutcome {
    let mut t = FdTable::new();
    let mut ledger = RaceLedger::new();
    let mut second_ebadf = 0u32;
    let mut released = 0u32;
    let mut kinds_ok = 0u32;
    let mut balanced = true;
    let mut round = 0u32;
    while round < RACE_ROUNDS {
        let flavor = if round % 2 == 0 {
            CloseFlavor::Clean
        } else {
            CloseFlavor::Interrupted
        };
        let h = match t.open(MODE_RW) {
            Ok(h) => h,
            Err(_) => HANDLE_INVALID,
        };
        let first = judge_close_handle(&mut t, h, flavor);
        let second = judge_close_handle(&mut t, h, CloseFlavor::Clean);
        // 判据一：第二次必须 EBADF。
        if second == Err(EBADF) {
            second_ebadf = second_ebadf.saturating_add(1);
        }
        // 判据二：第二次必须归入 DoubleClose 码。
        if classify_second_close(second) == CloseCode::DoubleClose {
            kinds_ok = kinds_ok.saturating_add(1);
        }
        // 判据三：释放不可回滚——首关无论报 Ok 还是 EINTR，槽位都必须
        // 已处于 Closed 态，且账面恰好多记一笔 close。
        let first_ok_or_eintr = first.is_ok() || first == Err(EINTR);
        if first_ok_or_eintr && t.peek_state(h.fd) == Some(SlotState::Closed) {
            released = released.saturating_add(1);
        }
        // 判据四：竞态不得破坏零泄漏对账。
        balanced = balanced && t.reconcile();
        let _ = ledger.note(second == Err(EBADF));
        round += 1;
    }
    RaceOutcome { ledger, second_ebadf, released, kinds_ok, balanced }
}

// ── 槽位复用代际守卫 ─────────────────────────────────────────────────
/// 复用矩阵结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ReuseOutcome {
    /// 每轮复用拿到的句柄代际（应严格递增）
    pub handle_gens: [u16; REUSE_CYCLES],
    /// 每轮都复用了同一个槽位号（低号优先 + 立即回收）
    pub same_slot_reused: bool,
    /// 代际严格递增
    pub strictly_increasing: bool,
    /// 所有旧代际句柄都被拒（EBADF，且无副作用）
    pub stale_all_rejected: bool,
    /// 最新代际句柄可正常关闭
    pub current_handle_accepted: bool,
    /// 最终句柄代际 = 上一轮代际 + 1
    pub final_gen_advances: bool,
    /// 结束后对账成立
    pub balanced: bool,
}

/// 槽位复用代际守卫：REUSE_CYCLES 轮 open→close 循环，验证
/// ① 同号复用；② 代际严格递增；③ 全部旧代际句柄失效；
/// ④ 最新句柄可用；⑤ 对账恒等式保持。
pub fn run_reuse_matrix() -> ReuseOutcome {
    let mut t = FdTable::new();
    let mut gens = [0u16; REUSE_CYCLES];
    let mut first_fd: i32 = -1;
    let mut same_slot = true;
    let mut c = 0usize;
    while c < REUSE_CYCLES {
        let h = match t.open(MODE_RW) {
            Ok(h) => h,
            Err(_) => HANDLE_INVALID,
        };
        if c == 0 {
            first_fd = h.fd;
        } else if h.fd != first_fd {
            same_slot = false;
        }
        if let Some(g) = gens.get_mut(c) {
            *g = h.gen;
        }
        let _ = t.close_raw(h.fd);
        c += 1;
    }
    // 代际严格递增复算。
    let mut increasing = true;
    let mut i = 1usize;
    while i < gens.len() {
        let prev = match gens.get(i - 1) {
            Some(v) => *v,
            None => 0,
        };
        let cur = match gens.get(i) {
            Some(v) => *v,
            None => 0,
        };
        if cur <= prev {
            increasing = false;
        }
        i += 1;
    }
    // 最后一轮结束后槽位是 Closed；再开一次得到「当前」句柄，
    // 其代际必须等于上一轮句柄代际 + 1（close 推进、复用继承）。
    let h_cur = match t.open(MODE_RW) {
        Ok(h) => h,
        Err(_) => HANDLE_INVALID,
    };
    let last_gen = match gens.get(REUSE_CYCLES - 1) {
        Some(v) => *v,
        None => 0,
    };
    let final_advances = h_cur.gen == last_gen.wrapping_add(1) && h_cur.fd == first_fd;
    // 旧代际句柄全部失效：槽位现在 Open 且代际已推进，旧句柄必须
    // 在触碰槽位之前被拒（无副作用——后续当前句柄仍可正常关闭）。
    let mut stale_ok = true;
    let mut k = 0usize;
    while k < gens.len() {
        if let Some(g) = gens.get(k).copied() {
            let stale_probe = Handle { fd: first_fd, gen: g };
            if judge_close_handle(&mut t, stale_probe, CloseFlavor::Clean) != Err(EBADF) {
                stale_ok = false;
            }
        }
        k += 1;
    }
    // 当前句柄必须可用（证明旧代际被拒不是「槽位坏了」）。
    let current_ok = judge_close_handle(&mut t, h_cur, CloseFlavor::Clean).is_ok();
    ReuseOutcome {
        handle_gens: gens,
        same_slot_reused: same_slot,
        strictly_increasing: increasing,
        stale_all_rejected: stale_ok,
        current_handle_accepted: current_ok,
        final_gen_advances: final_advances,
        balanced: t.reconcile(),
    }
}

// ── 零泄漏对账 ───────────────────────────────────────────────────────
/// 泄漏对账结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LeakOutcome {
    /// 每一步操作之后 allocs = closes + live 都成立
    pub balanced_every_step: bool,
    /// 期末分配总数
    pub final_allocs: u32,
    /// 期末关闭总数
    pub final_closes: u32,
    /// 期末存活的动态槽数（1 个常驻句柄）
    pub live: usize,
}

/// 零泄漏对账：先开 1 个常驻句柄，再 rounds 轮「开二关二」churn，
/// 每一步之后都复算对账恒等式；期末 live 必须恰好是常驻那 1 个。
pub fn run_leak_reconciliation(rounds: u32) -> LeakOutcome {
    let mut t = FdTable::new();
    let mut balanced = true;
    // 常驻句柄：全程不关，期末应计入 live。
    let _resident = match t.open(MODE_RO) {
        Ok(h) => h,
        Err(_) => HANDLE_INVALID,
    };
    balanced = balanced && t.reconcile();
    let mut r = 0u32;
    while r < rounds {
        // 每轮：开 A、开 B、关 A、关 B——净流量为零，账必须闭合。
        let ha = match t.open(MODE_RW) {
            Ok(h) => h,
            Err(_) => HANDLE_INVALID,
        };
        balanced = balanced && t.reconcile();
        let hb = match t.open(MODE_RW) {
            Ok(h) => h,
            Err(_) => HANDLE_INVALID,
        };
        balanced = balanced && t.reconcile();
        let _ = t.close_raw(ha.fd);
        balanced = balanced && t.reconcile();
        let _ = t.close_raw(hb.fd);
        balanced = balanced && t.reconcile();
        r += 1;
    }
    LeakOutcome {
        balanced_every_step: balanced,
        final_allocs: t.allocs(),
        final_closes: t.closes(),
        live: t.live_dyn(),
    }
}

// ── 容量耗尽 ─────────────────────────────────────────────────────────
/// 容量探针结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CapacityOutcome {
    /// 成功打开的槽数（应恰为 MAX_FDS）
    pub opened: usize,
    /// 第 MAX_FDS+1 次 open 恰好回 -EMFILE
    pub full_is_emfile: bool,
    /// 满表时刻对账成立
    pub balanced_at_full: bool,
    /// 关闭一槽后立即复开成功
    pub reuse_after_close: bool,
    /// 期末账目完好：allocs = MAX_FDS+1，closes = 1，live = MAX_FDS
    pub accounts_intact: bool,
}

/// 容量耗尽探针：填满 → 溢出一次必须 -EMFILE → 满表对账 →
/// 关一槽 → 立即复开 → 期末账目逐项复算。
pub fn run_capacity_probe() -> CapacityOutcome {
    let mut t = FdTable::new();
    let mut opened = 0usize;
    let mut full_res: SysResult<Handle> = Ok(HANDLE_INVALID);
    let mut i = 0usize;
    while i <= MAX_FDS {
        // 故意多试一次：第 MAX_FDS+1 次必须被拒。
        let r = t.open(MODE_RW);
        if r.is_ok() {
            opened = opened.saturating_add(1);
        } else {
            full_res = r;
        }
        i += 1;
    }
    let full_is_emfile = full_res == Err(EMFILE) && opened == MAX_FDS;
    let balanced_at_full = t.reconcile();
    // 腾槽复用：关掉最早分配的动态槽（低号 3），同号立即复开。
    let reuse_after_close = match t.close_raw(STD_FDS) {
        Ok(_new_gen) => t.open(MODE_RW).is_ok(),
        Err(_) => false,
    };
    let accounts_intact = t.allocs() == (MAX_FDS as u32).saturating_add(1)
        && t.closes() == 1
        && t.live_dyn() == MAX_FDS
        && t.reconcile();
    CapacityOutcome {
        opened,
        full_is_emfile,
        balanced_at_full,
        reuse_after_close,
        accounts_intact,
    }
}

// ── Linux 逐码对照表 ─────────────────────────────────────────────────
/// (名称, 系统调用返回的负值, Windows 近似码, 说明)。
/// Windows 对照码仅作说明，不参与判定。
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EBADF", -9, 6, "Windows ERROR_INVALID_HANDLE；坏句柄族"),
    ("EINTR", -4, 995, "Windows ERROR_OPERATION_ABORTED；中断族（close 仍释放）"),
    ("EMFILE", -24, 4, "Windows ERROR_TOO_MANY_OPEN_FILES；fd 表满"),
];

/// 查表：按 errno 正数取表内负值；表外码返回 0（显性可查）。
pub fn linux_neg(code: i32) -> i32 {
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(i) {
            if row.1 == -code {
                return row.1;
            }
        }
        i += 1;
    }
    0
}

/// 与 Linux 逐码一致：只允许表内正值且负值精确可复算。
pub fn agrees_with_linux(code: i32) -> bool {
    linux_neg(code) == -code && code > 0
}

// ── Linux close(2) 语义对账 ──────────────────────────────────────────
/// 语义对账行数。
pub const SEMANTIC_COUNT: usize = 5;

/// 与 man close(2) 逐项对账（每行都是一次真实探针，不是常量）：
/// ① 合法句柄 close 成功；
/// ② 同一 fd 第二次 close 回 EBADF（close 不幂等）；
/// ③ close(-1) 回 EBADF；
/// ④ close 报 EINTR 后 fd 仍已释放（Linux 特有）；
/// ⑤ 关闭后同号 fd 可被立即重新分配且代际推进。
pub fn semantic_reconciliation() -> [bool; SEMANTIC_COUNT] {
    let mut t = FdTable::new();
    let mut rows = [false; SEMANTIC_COUNT];
    let h = match t.open(MODE_RW) {
        Ok(h) => h,
        Err(_) => HANDLE_INVALID,
    };
    // ① close 合法句柄成功
    if let Some(r0) = rows.get_mut(0) {
        *r0 = judge_close_handle(&mut t, h, CloseFlavor::Clean) == Ok(());
    }
    // ② 同一 fd 第二次 close → EBADF
    if let Some(r1) = rows.get_mut(1) {
        *r1 = judge_close(&mut t, h.fd, CloseFlavor::Clean) == Err(EBADF);
    }
    // ③ close(-1) → EBADF
    if let Some(r2) = rows.get_mut(2) {
        *r2 = judge_close(&mut t, -1, CloseFlavor::Clean) == Err(EBADF);
    }
    // ④ EINTR 后 fd 已释放
    let h2 = match t.open(MODE_RW) {
        Ok(h) => h,
        Err(_) => HANDLE_INVALID,
    };
    let eintr_res = judge_close_handle(&mut t, h2, CloseFlavor::Interrupted);
    if let Some(r3) = rows.get_mut(3) {
        *r3 = eintr_res == Err(EINTR) && t.peek_state(h2.fd) == Some(SlotState::Closed);
    }
    // ⑤ 同号复用且代际推进
    let h3 = match t.open(MODE_RW) {
        Ok(h) => h,
        Err(_) => HANDLE_INVALID,
    };
    if let Some(r4) = rows.get_mut(4) {
        *r4 = h3.fd == h2.fd && h3.gen == h2.gen.wrapping_add(1);
    }
    rows
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位（无浮点、无随机、无墙钟）。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        i += 1;
    }
    h
}

/// 把一次 close 判定结果编码为单字节：Ok→1；Err(e)→e 低 8 位
/// （EINTR=4 / EBADF=9，与 1 互异，逐字节可复算）。
fn push_outcome(bytes: &mut Vec<u8>, r: Result<(), i32>) {
    match r {
        Ok(()) => bytes.push(1),
        Err(e) => bytes.push((e & 0xFF) as u8),
    }
}

/// open 的失败安全包装（语料内表不会满，防御性归到非法句柄）。
fn open_or_invalid(t: &mut FdTable) -> Handle {
    match t.open(MODE_RW) {
        Ok(h) => h,
        Err(_) => HANDLE_INVALID,
    }
}

/// 规范语料指纹：固定序列覆盖正常关/双关/EINTR 关/负 fd/越界/
/// 未开槽/陈旧代际/标准流八类操作，双跑必须逐字节一致。
pub fn corpus_fingerprint() -> u32 {
    let mut t = FdTable::new();
    let mut bytes: Vec<u8> = Vec::new();
    let h1 = open_or_invalid(&mut t);
    let h2 = open_or_invalid(&mut t);
    // 正常关：成功。
    push_outcome(&mut bytes, judge_close_handle(&mut t, h1, CloseFlavor::Clean));
    // 双关：同一句柄再关，必 EBADF。
    push_outcome(&mut bytes, judge_close_handle(&mut t, h1, CloseFlavor::Clean));
    // EINTR 关：报 EINTR 但槽位已释放。
    push_outcome(&mut bytes, judge_close_handle(&mut t, h2, CloseFlavor::Interrupted));
    // EINTR 后重试：必 EBADF（fd 已释放）。
    push_outcome(&mut bytes, judge_close_handle(&mut t, h2, CloseFlavor::Clean));
    // 负 fd + EINTR 请求：仍 EBADF（判定次序）。
    push_outcome(&mut bytes, judge_close(&mut t, -1, CloseFlavor::Interrupted));
    // 越上界：EBADF。
    push_outcome(&mut bytes, judge_close(&mut t, STD_FDS + MAX_FDS as i32, CloseFlavor::Clean));
    // 从未打开的尾槽：EBADF。
    push_outcome(&mut bytes, judge_close(&mut t, STD_FDS + MAX_FDS as i32 - 1, CloseFlavor::Clean));
    // 陈旧代际：h1 的旧代际句柄（槽位已 Closed），必 EBADF。
    let stale = Handle { fd: h1.fd, gen: h1.gen.wrapping_sub(1) };
    push_outcome(&mut bytes, judge_close_handle(&mut t, stale, CloseFlavor::Clean));
    // 标准流关闭：合法。
    push_outcome(&mut bytes, judge_close(&mut t, 2, CloseFlavor::Clean));
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9204 判据集（22 条静态 + 5 条动态快照）。
pub fn run_unx9204_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9204");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7A000..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x7A000..=0x7D1C0", seg_ok,
          "段基址必须落在本批窗口且独占 0x40 窗口；越界即拒绝注册");

    // 2. 状态机：close 边只允许从 Open 触发
    let mut t_sm = FdTable::new();
    let sm_ok = !closeable_state(SlotState::Free)
        && !closeable_state(SlotState::Closed)
        && closeable_state(SlotState::Open)
        && judge_close(&mut t_sm, STD_FDS, CloseFlavor::Clean) == Err(EBADF);
    s.add("状态机：Free→Closed 非法边被拒，仅 Open 态可触发 close", sm_ok,
          "从未打开的槽位不允许跳过打开直接关闭；非法倒退/越级显性拒绝");

    // 3. 正常流：open → close 成功，代际推进，二次 close 必拒
    let mut t_norm = FdTable::new();
    let normal = match t_norm.open(MODE_RW) {
        Ok(h) => {
            let g0 = h.gen;
            match t_norm.close_raw(h.fd) {
                Ok(g1) => {
                    g1 == g0.wrapping_add(1)
                        && t_norm.peek_state(h.fd) == Some(SlotState::Closed)
                        && judge_close(&mut t_norm, h.fd, CloseFlavor::Clean) == Err(EBADF)
                }
                Err(_) => false,
            }
        }
        Err(_) => false,
    };
    s.add("正常流：open 后 close 成功且代际推进、二次 close 必拒", normal,
          "合法句柄关闭一次成功并推进代际；重复关闭必须回 EBADF");

    // 4. 正常流：标准流可关闭（Linux 允许 close 0/1/2）
    let mut t_std = FdTable::new();
    let std_ok = judge_close(&mut t_std, 2, CloseFlavor::Clean) == Ok(())
        && t_std.peek_state(2) == Some(SlotState::Closed)
        && judge_close(&mut t_std, 2, CloseFlavor::Clean) == Err(EBADF)
        && judge_close(&mut t_std, 0, CloseFlavor::Clean) == Ok(());
    s.add("正常流：close 标准流 fd 2/0 合法且关闭后同样拒绝复用", std_ok,
          "标准流不是特权 fd；关闭后进入 Closed 态，与动态槽同语义");

    // 5. 五类坏 close 注入矩阵：全 EBADF 且逐码归位
    let bad = run_bad_close_matrix();
    let all_badf = {
        let mut ok = bad.ledger.conserved() && bad.ledger.missed == 0;
        let mut i = 0usize;
        while i < bad.per_class.len() {
            if bad.per_class.get(i).copied() != Some(INJECT_ROUNDS) {
                ok = false;
            }
            i += 1;
        }
        ok
    };
    s.add("五类坏 close ×10/10：全 EBADF 且诊断码逐码归位", all_badf,
          "负值/未开/双关/越界/陈旧代际全部拒绝并归入正确诊断码");

    // 6. 注入账守恒
    s.add("注入账守恒：50 注入 = 50 检出 + 0 逃逸",
          bad.ledger.rounds == (BAD_CLOSE_CLASSES as u32) * INJECT_ROUNDS
              && bad.ledger.caught == bad.ledger.rounds
              && bad.ledger.missed == 0,
          "每次注入都必须被抓，逃逸恒为 0，账只增不减");

    // 7. 双 close 竞态：第二次全 EBADF
    let race = run_double_close_matrix();
    s.add("双 close 竞态：第二次全 EBADF（10/10）",
          race.second_ebadf == RACE_ROUNDS,
          "同一句柄连续两次 close，第二次必须回 EBADF，无一例外");

    // 8. 竞态释放唯一且不可回滚
    s.add("竞态释放唯一：每轮恰好释放一次且槽位归 Closed",
          race.released == RACE_ROUNDS && race.kinds_ok == RACE_ROUNDS,
          "首关无论报 Ok 还是 EINTR，槽位必须已释放且只释放一次；第二次必归 DoubleClose 码");

    // 9. 竞态不破坏对账
    s.add("竞态账平衡：每轮 allocs = closes + live",
          race.balanced && race.ledger.missed == 0,
          "竞态注入不得破坏 fd 表零泄漏对账恒等式");

    // 10. 槽位复用代际守卫
    let reuse = run_reuse_matrix();
    s.add("槽位复用代际守卫：close 后同号新分配带新代际，旧句柄全 EBADF",
          reuse.stale_all_rejected && reuse.same_slot_reused,
          "旧代际句柄在触碰槽位之前被拒；同号立即复用（低号优先）");

    // 11. 代际单调
    s.add("代际单调：复用轮次代际严格递增且当前句柄可用",
          reuse.strictly_increasing && reuse.current_handle_accepted
              && reuse.final_gen_advances,
          "代际只增不减；最新句柄可正常关闭，证明被拒的是旧代际而非槽位本身");

    // 12. EINTR 特殊语义
    let mut t_eintr = FdTable::new();
    let eintr_ok = {
        let h = open_or_invalid(&mut t_eintr);
        match judge_close_handle(&mut t_eintr, h, CloseFlavor::Interrupted) {
            Err(EINTR) => {
                t_eintr.peek_state(h.fd) == Some(SlotState::Closed)
                    && judge_close(&mut t_eintr, h.fd, CloseFlavor::Clean) == Err(EBADF)
                    && t_eintr.closes() == 1
            }
            _ => false,
        }
    };
    s.add("EINTR 语义：close 报 EINTR 但 fd 已释放、重试必 EBADF", eintr_ok,
          "Linux 特有：EINTR 不回滚释放；调用方不得重试 close 期待再次成功");

    // 13. 判定次序：EBADF 先于 EINTR
    let mut t_pri = FdTable::new();
    let closed_probe = {
        let h = open_or_invalid(&mut t_pri);
        let _ = t_pri.close_raw(h.fd);
        h.fd
    };
    let pri_ok = judge_close(&mut t_pri, -1, CloseFlavor::Interrupted) == Err(EBADF)
        && judge_close(&mut t_pri, STD_FDS + MAX_FDS as i32, CloseFlavor::Interrupted) == Err(EBADF)
        && judge_close(&mut t_pri, closed_probe, CloseFlavor::Interrupted) == Err(EBADF);
    s.add("判定次序：EBADF 先于 EINTR，坏 fd 不改报中断", pri_ok,
          "fd 校验先于一切副作用；负值/越界/已关槽即使请求 EINTR 路径也必须回 EBADF");

    // 14. 容量耗尽
    let cap = run_capacity_probe();
    s.add("容量耗尽：表满回 -EMFILE 且账目完好、关一复开一",
          cap.full_is_emfile && cap.reuse_after_close && cap.accounts_intact
              && cap.balanced_at_full,
          "fd 表满必须回 -EMFILE；腾出的槽可立即复用；期末 allocs/closes/live 逐项复算");

    // 15. 零泄漏对账
    let leak = run_leak_reconciliation(12);
    s.add("零泄漏对账：allocs = closes + live 每步成立且期末 live=1",
          leak.balanced_every_step
              && leak.final_allocs == leak.final_closes.saturating_add(leak.live as u32)
              && leak.live == 1,
          "每次 open/close 之后对账恒等式都必须成立；泄漏即失败");

    // 16. Linux 逐码一致
    let codes_ok = agrees_with_linux(EBADF)
        && agrees_with_linux(EINTR)
        && agrees_with_linux(EMFILE)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999)
        && linux_neg(EBADF) == -EBADF;
    s.add("Linux 逐码一致：EBADF=9/EINTR=4/EMFILE=24 精确可复算", codes_ok,
          "错误码只允许 errno 表内正值，负值与 man 2 逐项对齐；表外码显性拒绝");

    // 17. 语义对账（每行都是真实探针）
    let sem = semantic_reconciliation();
    let sem_ok = {
        let mut ok = true;
        let mut i = 0usize;
        while i < sem.len() {
            if sem.get(i).copied() != Some(true) {
                ok = false;
            }
            i += 1;
        }
        ok
    };
    s.add("语义对账：close(2) 五条 Linux 语义逐项复算一致", sem_ok,
          "成功/重复关/负 fd/EINTR 释放/同号复用与 man close(2) 对齐");

    // 18. 双跑一致
    let fa = corpus_fingerprint();
    let fb = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", fa == fb,
          "同输入两次必须逐字段相等，隐藏状态即失败");

    // 19. 文案闭集
    s.add("文案闭集：全部诊断码带中文建议",
          CloseCode::ALL.iter().all(|c| !c.detail().is_empty()),
          "每个码必须能取到非空且可操作的说明");

    // 20. 诊断码唯一
    let mut uniq = true;
    let mut i = 0usize;
    while i < CloseCode::ALL.len() {
        let mut j = i + 1;
        while j < CloseCode::ALL.len() {
            if let (Some(a), Some(b)) = (CloseCode::ALL.get(i), CloseCode::ALL.get(j)) {
                if a.val() == b.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    s.add("诊断码：12 个码值两两唯一", uniq,
          "段内闭集码不得重复，否则判据无法定位");

    // 21. 账目单调
    let mut t_mono = FdTable::new();
    let mono_ok = {
        let c0 = t_mono.closes();
        let mut ok = true;
        let mut k = 0u32;
        while k < 4 {
            let h = open_or_invalid(&mut t_mono);
            if t_mono.close_raw(h.fd).is_err() {
                ok = false;
            }
            k += 1;
        }
        ok && t_mono.closes() == c0.saturating_add(4)
    };
    s.add("账目单调：close 计数只增不减", mono_ok,
          "关闭账禁止回退或饱和到旧值");

    // 22. 收口：判据条数达到声明值
    let n_before = s.len();
    s.add("收口：判据条数达到声明值", n_before >= 16,
          "本模块声明的判据条数必须全部落账，不得截断");

    // 动态明细（≤8 条）：只给最关键的运行期快照
    let race_text = format!(
        "竞态 {} 轮 / 二次EBADF {} / 逃逸 {}",
        race.ledger.rounds, race.second_ebadf, race.ledger.missed
    );
    s.add_dyn("竞态账快照",
              race.second_ebadf == RACE_ROUNDS && race.ledger.missed == 0,
              &race_text);

    let bad_text = format!(
        "注入 {} / 检出 {} / 逃逸 {}",
        bad.ledger.rounds, bad.ledger.caught, bad.ledger.missed
    );
    s.add_dyn("坏 close 矩阵快照",
              bad.ledger.conserved() && bad.ledger.missed == 0,
              &bad_text);

    let leak_text = format!(
        "allocs {} = closes {} + live {}",
        leak.final_allocs, leak.final_closes, leak.live
    );
    s.add_dyn("泄漏对账快照", leak.balanced_every_step, &leak_text);

    let cap_text = format!(
        "满表 {} 槽 / EMFILE 拒绝 / 复用 1 槽",
        cap.opened
    );
    s.add_dyn("容量快照", cap.full_is_emfile && cap.reuse_after_close, &cap_text);

    let fp_text = format!("语料指纹 0x{:08X}", fa);
    s.add_dyn("指纹快照", fa == fb, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_close_and_generation() {
        let mut t = FdTable::new();
        let h = t.open(MODE_RW).unwrap_or(HANDLE_INVALID);
        assert!(h.fd >= STD_FDS);
        assert_eq!(judge_close_handle(&mut t, h, CloseFlavor::Clean), Ok(()));
        assert_eq!(t.peek_state(h.fd), Some(SlotState::Closed));
        assert_eq!(judge_close_handle(&mut t, h, CloseFlavor::Clean), Err(EBADF));
        assert!(t.reconcile());
    }

    #[test]
    fn std_fd_close_allowed_then_ebadf() {
        let mut t = FdTable::new();
        assert_eq!(judge_close(&mut t, 2, CloseFlavor::Clean), Ok(()));
        assert_eq!(t.peek_state(2), Some(SlotState::Closed));
        assert_eq!(judge_close(&mut t, 2, CloseFlavor::Clean), Err(EBADF));
    }

    #[test]
    fn four_boundaries_all_ebadf() {
        let mut t = FdTable::new();
        // 负边界：-1
        assert_eq!(judge_close(&mut t, -1, CloseFlavor::Clean), Err(EBADF));
        // 零边界：fd 0 是合法标准流，close 成功
        assert_eq!(judge_close(&mut t, 0, CloseFlavor::Clean), Ok(()));
        // 上边界：最后一个动态槽号（从未打开）
        assert_eq!(
            judge_close(&mut t, STD_FDS + MAX_FDS as i32 - 1, CloseFlavor::Clean),
            Err(EBADF)
        );
        // 上界+1：越界
        assert_eq!(
            judge_close(&mut t, STD_FDS + MAX_FDS as i32, CloseFlavor::Clean),
            Err(EBADF)
        );
    }

    #[test]
    fn bad_close_matrix_all_caught() {
        let out = run_bad_close_matrix();
        assert!(out.ledger.conserved());
        assert_eq!(out.ledger.missed, 0);
        assert_eq!(out.ledger.rounds, (BAD_CLOSE_CLASSES as u32) * INJECT_ROUNDS);
        let mut i = 0usize;
        while i < out.per_class.len() {
            assert_eq!(out.per_class.get(i).copied().unwrap_or(0), INJECT_ROUNDS);
            i += 1;
        }
    }

    #[test]
    fn double_close_race_second_always_ebadf() {
        let out = run_double_close_matrix();
        assert_eq!(out.second_ebadf, RACE_ROUNDS);
        assert_eq!(out.released, RACE_ROUNDS);
        assert_eq!(out.kinds_ok, RACE_ROUNDS);
        assert!(out.balanced);
        assert_eq!(out.ledger.missed, 0);
    }

    #[test]
    fn race_winner_unique_per_round() {
        let mut t = FdTable::new();
        let (a, b) = race_close_twice(&mut t, CloseFlavor::Clean);
        assert_eq!(a, Ok(()));
        assert_eq!(b, Err(EBADF));
        assert_eq!(a.is_ok() as u32 + b.is_ok() as u32, 1);
        // EINTR 形态：首关报 EINTR 但已释放，第二次仍必 EBADF。
        let (c, d) = race_close_twice(&mut t, CloseFlavor::Interrupted);
        assert_eq!(c, Err(EINTR));
        assert_eq!(d, Err(EBADF));
        assert_eq!(c.is_ok() as u32 + d.is_ok() as u32, 0);
    }

    #[test]
    fn stale_generation_rejected_after_reuse() {
        let out = run_reuse_matrix();
        assert!(out.same_slot_reused);
        assert!(out.strictly_increasing);
        assert!(out.stale_all_rejected);
        assert!(out.current_handle_accepted);
        assert!(out.final_gen_advances);
        assert!(out.balanced);
    }

    #[test]
    fn eintr_releases_fd() {
        let mut t = FdTable::new();
        let h = t.open(MODE_RW).unwrap_or(HANDLE_INVALID);
        assert_eq!(judge_close_handle(&mut t, h, CloseFlavor::Interrupted), Err(EINTR));
        assert_eq!(t.peek_state(h.fd), Some(SlotState::Closed));
        assert_eq!(judge_close(&mut t, h.fd, CloseFlavor::Clean), Err(EBADF));
        assert_eq!(t.closes(), 1);
    }

    #[test]
    fn priority_ebadf_over_eintr() {
        let mut t = FdTable::new();
        assert_eq!(judge_close(&mut t, -1, CloseFlavor::Interrupted), Err(EBADF));
        assert_eq!(
            judge_close(&mut t, STD_FDS + MAX_FDS as i32, CloseFlavor::Interrupted),
            Err(EBADF)
        );
        let h = t.open(MODE_RW).unwrap_or(HANDLE_INVALID);
        let _ = t.close_raw(h.fd);
        assert_eq!(judge_close(&mut t, h.fd, CloseFlavor::Interrupted), Err(EBADF));
    }

    #[test]
    fn capacity_exhaustion_emfile() {
        let out = run_capacity_probe();
        assert!(out.full_is_emfile);
        assert_eq!(out.opened, MAX_FDS);
        assert!(out.balanced_at_full);
        assert!(out.reuse_after_close);
        assert!(out.accounts_intact);
    }

    #[test]
    fn zero_leak_reconciliation() {
        let out = run_leak_reconciliation(12);
        assert!(out.balanced_every_step);
        assert_eq!(out.final_allocs, out.final_closes + out.live as u32);
        assert_eq!(out.live, 1);
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(EBADF));
        assert!(agrees_with_linux(EINTR));
        assert!(agrees_with_linux(EMFILE));
        assert!(!agrees_with_linux(0));
        assert!(!agrees_with_linux(9999));
        assert_eq!(linux_neg(EINTR), -EINTR);
        assert_eq!(linux_neg(EMFILE), -EMFILE);
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn semantic_table_all_linux_consistent() {
        let rows = semantic_reconciliation();
        assert_eq!(rows.len(), SEMANTIC_COUNT);
        let mut i = 0usize;
        while i < rows.len() {
            assert!(rows.get(i).copied().unwrap_or(false));
            i += 1;
        }
    }

    #[test]
    fn segment_guard() {
        assert!(SEG_BASE % 0x40 == 0);
        assert!((0x7A000..=0x7D1C0).contains(&SEG_BASE));
        assert_eq!(CloseCode::NegativeFd.val(), SEG_BASE);
        assert_eq!(CloseCode::SegmentViolation.val(), SEG_BASE + 11);
    }

    #[test]
    fn detail_closure_nonempty() {
        let mut i = 0usize;
        while i < CloseCode::ALL.len() {
            let c = CloseCode::ALL.get(i).copied().unwrap_or(CloseCode::NegativeFd);
            assert!(!c.detail().is_empty());
            i += 1;
        }
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9204_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
        assert!(!s.truncated());
    }
}
