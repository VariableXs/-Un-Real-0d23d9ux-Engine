//! UNX-F50013 色彩冻结接口（渲染管线消费单点面）（0x42dc0 段）
//!
//! 锚点：UNX-F50013-J1 接口冻结登记；判据锚定 §AI-63 §4。
//!
//! 数据段 `SEG_BASE = 0x42dc0`；诊断码闭集 = `SEG_BASE + n`：
//!   +1 Empty       冻结内容为空 / 配置非法（非有限浮点）
//!   +2 NotFrozen    当前是 Open 态，没有可消费的快照
//!   +3 AlreadyFrozen 重复冻结（冻结是单向密封，不能叠加）
//!   +4 Stale        票据代次落后于当前快照（解冻后又冻结）
//!   +5 TicketUnknown 票据号不在登记表内
//!   +6 CapFull      票据登记容量满
//!   +7 Conflict     活票据未归还就解冻 / 重复归还 / 票据号耗尽
//!   +8 Terminal     门已退役，任何接口永久关闭
//!   +9 Digest       摘要非有限或退化
//!
//! 为什么冻结要「登记代次 + 票据」：渲染管线是异步消费方，冻结若只是
//! 返回一个引用，管线可能在下一次冻结后继续读旧矩阵——这类撕裂在生产
//! 上表现为偶发偏色，极难复现。代次让旧票据**显式**作废（Stale），
//! 活票据让解冻**显式**被拒（Conflict），两者都进诊断码闭集，不静默。
//!
//! 结构清单：
//! - 状态机：`FreezeState` = Open → Frozen → Open …，`Retired` 为终态
//! - 数据类型：`FrozenColor`（不可变快照）、`ColorView`（Copy 单点视图）、
//!   `FreezeTicket`（消费票据）、`FreezeGate`（门 + 票据登记表）
//! - 账本：票据表（定容、按票号查找、归还记账）、`consumed` 消费审计
//! - 错误面：`FreezeError::code()` 落 0x42dc0 段闭集
//! - 注入核：`injection_core` 八类坏序列（重复冻结/未冻结消费/过期票据/
//!   已释放票据/带活票据解冻/未知票据/容量满/退役后冻结）

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use libm::fabsf;

use super::unx50001_gamutmap::{IccProfile, Intent};

/// 诊断段基址（0x42dc0 独占 0x20）。
pub const SEG_BASE: u32 = 0x42dc0;

pub const E_EMPTY: u16 = 1;
pub const E_NOTFROZEN: u16 = 2;
pub const E_ALREADYFROZEN: u16 = 3;
pub const E_STALE: u16 = 4;
pub const E_TICKETUNKNOWN: u16 = 5;
pub const E_CAPFULL: u16 = 6;
pub const E_CONFLICT: u16 = 7;
pub const E_TERMINAL: u16 = 8;
pub const E_DIGEST: u16 = 9;

/// 色彩冻结错误。`code()` 返回 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FreezeError {
    Empty,
    NotFrozen,
    AlreadyFrozen,
    Stale,
    TicketUnknown,
    CapFull,
    Conflict,
    Terminal,
    Digest,
}

impl FreezeError {
    /// 段内诊断码：`SEG_BASE + n`。
    pub fn code(self) -> u32 {
        let n = match self {
            FreezeError::Empty => E_EMPTY,
            FreezeError::NotFrozen => E_NOTFROZEN,
            FreezeError::AlreadyFrozen => E_ALREADYFROZEN,
            FreezeError::Stale => E_STALE,
            FreezeError::TicketUnknown => E_TICKETUNKNOWN,
            FreezeError::CapFull => E_CAPFULL,
            FreezeError::Conflict => E_CONFLICT,
            FreezeError::Terminal => E_TERMINAL,
            FreezeError::Digest => E_DIGEST,
        };
        SEG_BASE + n as u32
    }

    /// 一行诊断文本。
    pub fn message(self) -> &'static str {
        match self {
            FreezeError::Empty => "nothing to freeze",
            FreezeError::NotFrozen => "gate is open, no snapshot",
            FreezeError::AlreadyFrozen => "gate already frozen",
            FreezeError::Stale => "ticket generation is stale",
            FreezeError::TicketUnknown => "ticket not registered",
            FreezeError::CapFull => "ticket table full",
            FreezeError::Conflict => "live tickets or duplicate release",
            FreezeError::Terminal => "gate retired",
            FreezeError::Digest => "degenerate snapshot digest",
        }
    }
}

fn finite(v: f32) -> bool {
    v == v && v > -1.0e30 && v < 1.0e30
}

/// FNV-1a 64 位摘要（快照指纹）。冻结接口只需要「内容相同 ⇒ 摘要相同」，
/// 不需要密码学强度；FNV 的两条指令成本换来跨平台逐位一致。
#[derive(Clone, Copy)]
pub struct Fnv64 {
    h: u64,
}

impl Fnv64 {
    pub const fn new() -> Fnv64 {
        Fnv64 {
            h: 0xcbf29ce484222325,
        }
    }

    pub fn byte(&mut self, b: u8) {
        self.h ^= b as u64;
        self.h = self.h.wrapping_mul(0x100000001b3);
    }

    pub fn u16le(&mut self, v: u16) {
        self.byte(v as u8);
        self.byte((v >> 8) as u8);
    }

    pub fn u32le(&mut self, v: u32) {
        self.u16le(v as u16);
        self.u16le((v >> 16) as u16);
    }

    pub fn f32q(&mut self, v: f32) {
        let scaled = v * 10000.0;
        let c = if scaled > 2.0e9 {
            2.0e9
        } else if scaled < -2.0e9 {
            -2.0e9
        } else {
            scaled
        };
        self.u32le(c as i32 as u32);
    }

    pub fn digest(self) -> u64 {
        self.h
    }
}

/// 门状态。`Retired` 是终态：退役后渲染管线必须走兜底档，不能再取快照。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FreezeState {
    Open,
    Frozen,
    Retired,
}

impl FreezeState {
    pub fn label(self) -> &'static str {
        match self {
            FreezeState::Open => "open",
            FreezeState::Frozen => "frozen",
            FreezeState::Retired => "retired",
        }
    }
}

/// 冻结快照：把 profile 里「渲染真正要用」的四个字段拷成不可变值。
///
/// 不驻留 `IccProfile` 全文（含 String 描述）是有意的：冻结面必须能放进
/// 定长结构供渲染线程零分配读取；描述只参与摘要，不参与绘制。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrozenColor {
    pub generation: u32,
    pub intent: Intent,
    pub white: [f32; 3],
    pub matrix: [[f32; 3]; 3],
    pub digest: u64,
}

impl FrozenColor {
    /// 从 profile 生成快照。白点/矩阵出现非有限值即显式失败——冻结一个
    /// 含 NaN 的矩阵等于把偏色钉死在管线里。
    pub fn from_profile(p: &IccProfile, intent: Intent, generation: u32) -> Result<FrozenColor, FreezeError> {
        if !finite(p.white.x) || !finite(p.white.y) || !finite(p.white.z) {
            return Err(FreezeError::Empty);
        }
        let mut i = 0usize;
        while i < 3 {
            let row = match p.rgb_to_xyz.m.get(i) {
                Some(r) => r,
                None => return Err(FreezeError::Empty),
            };
            let mut j = 0usize;
            while j < 3 {
                match row.get(j) {
                    Some(v) => {
                        if !finite(*v) {
                            return Err(FreezeError::Empty);
                        }
                    }
                    None => return Err(FreezeError::Empty),
                }
                j += 1;
            }
            i += 1;
        }
        let digest = snapshot_digest(p, intent);
        if digest == 0 {
            return Err(FreezeError::Digest);
        }
        Ok(FrozenColor {
            generation,
            intent,
            white: [p.white.x, p.white.y, p.white.z],
            matrix: [
                [p.rgb_to_xyz.m[0][0], p.rgb_to_xyz.m[0][1], p.rgb_to_xyz.m[0][2]],
                [p.rgb_to_xyz.m[1][0], p.rgb_to_xyz.m[1][1], p.rgb_to_xyz.m[1][2]],
                [p.rgb_to_xyz.m[2][0], p.rgb_to_xyz.m[2][1], p.rgb_to_xyz.m[2][2]],
            ],
            digest,
        })
    }
}

/// 快照指纹：描述字节 + 意图码 + 标签数/尺寸 + 白点 + 色料矩阵。
pub fn snapshot_digest(p: &IccProfile, intent: Intent) -> u64 {
    let mut h = Fnv64::new();
    let mut i = 0usize;
    let bytes = p.desc.as_bytes();
    h.u16le(bytes.len() as u16);
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h.byte(*b);
        }
        i += 1;
    }
    h.byte(intent.code());
    h.u16le(p.tag_count);
    h.u32le(p.size_bytes);
    h.f32q(p.white.x);
    h.f32q(p.white.y);
    h.f32q(p.white.z);
    let mut r = 0usize;
    while r < 3 {
        if let Some(row) = p.rgb_to_xyz.m.get(r) {
            let mut c = 0usize;
            while c < 3 {
                if let Some(v) = row.get(c) {
                    h.f32q(*v);
                }
                c += 1;
            }
        }
        r += 1;
    }
    h.digest()
}

/// 单点消费视图（Copy，无浮点，可跨线程按值传递）。
///
/// 这是「渲染管线消费单点面」：管线只拿 `(代次, 摘要, 意图码)`，矩阵要在
/// 门内用 `snapshot()` 取——视图本身不携带可变数据，因此不存在管线侧改写
/// 冻结内容的通道。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ColorView {
    pub generation: u32,
    pub digest: u64,
    pub intent_code: u8,
}

/// 消费票据：`id` 唯一，`generation` 绑定签发时的快照。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FreezeTicket {
    pub id: u16,
    pub generation: u32,
    pub released: bool,
}

/// 冻结门：状态机 + 票据登记表 + 消费审计。
#[derive(Clone, Debug)]
pub struct FreezeGate {
    state: FreezeState,
    generation: u32,
    frozen: Option<FrozenColor>,
    tickets: Vec<FreezeTicket>,
    cap: usize,
    next_ticket: u16,
    consumed: u32,
}

impl FreezeGate {
    /// 新建门。`cap` 是票据表容量（同时刻最多多少消费者持有票据）。
    pub fn new(cap: usize) -> FreezeGate {
        FreezeGate {
            state: FreezeState::Open,
            generation: 0,
            frozen: None,
            tickets: Vec::new(),
            cap,
            next_ticket: 1,
            consumed: 0,
        }
    }

    pub fn state(&self) -> FreezeState {
        self.state
    }

    pub fn generation(&self) -> u32 {
        self.generation
    }

    /// 单点面被消费的次数（审计：管线到底读了几次冻结内容）。
    pub fn consumed(&self) -> u32 {
        self.consumed
    }

    /// 未归还票据数。
    pub fn live_tickets(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.tickets.len() {
            if let Some(t) = self.tickets.get(i) {
                if !t.released {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 冻结：Open → Frozen，代次自增后返回新代次。
    ///
    /// 冻结是**登记动作**：先密封内容再改状态，最后才返回代次——任何中途
    /// 失败都保持 Open，不会留下「状态已 Frozen 但无快照」的半截态。
    pub fn freeze(&mut self, p: &IccProfile, intent: Intent) -> Result<u32, FreezeError> {
        match self.state {
            FreezeState::Retired => return Err(FreezeError::Terminal),
            FreezeState::Frozen => return Err(FreezeError::AlreadyFrozen),
            FreezeState::Open => {}
        }
        let next = match self.generation.checked_add(1) {
            Some(g) => g,
            None => return Err(FreezeError::Conflict),
        };
        let snap = FrozenColor::from_profile(p, intent, next)?;
        self.frozen = Some(snap);
        self.generation = next;
        self.tickets.clear();
        self.state = FreezeState::Frozen;
        Ok(next)
    }

    /// 换发新代次快照（profile 切换）：Frozen → Frozen，代次 +1。
    ///
    /// 与 `freeze` 的分工：`freeze` 是「首次密封」，`reseal` 是「换档」。
    /// 换档**故意**保留未归还的旧票据——它们下一次消费必须报 Stale，这样
    /// 管线拿着旧矩阵继续画的行为会当场暴露，而不是悄悄画出偏色。
    pub fn reseal(&mut self, p: &IccProfile, intent: Intent) -> Result<u32, FreezeError> {
        match self.state {
            FreezeState::Retired => return Err(FreezeError::Terminal),
            FreezeState::Open => return Err(FreezeError::NotFrozen),
            FreezeState::Frozen => {}
        }
        let next = match self.generation.checked_add(1) {
            Some(g) => g,
            None => return Err(FreezeError::Conflict),
        };
        let snap = FrozenColor::from_profile(p, intent, next)?;
        self.frozen = Some(snap);
        self.generation = next;
        // 已归还票据不再参与任何判定，换档时回收其槽位；活票据保留。
        self.prune_released();
        Ok(next)
    }

    /// 回收已归还票据（就地重建，容量随活票据收敛）。
    fn prune_released(&mut self) {
        let mut kept = Vec::with_capacity(self.tickets.len());
        let mut i = 0usize;
        while i < self.tickets.len() {
            if let Some(t) = self.tickets.get(i) {
                if !t.released {
                    kept.push(*t);
                }
            }
            i += 1;
        }
        self.tickets = kept;
    }

    /// 签发消费票据：仅 Frozen 态可签；活票据数达上限或票号耗尽显式失败。
    pub fn acquire(&mut self) -> Result<FreezeTicket, FreezeError> {
        match self.state {
            FreezeState::Retired => return Err(FreezeError::Terminal),
            FreezeState::Open => return Err(FreezeError::NotFrozen),
            FreezeState::Frozen => {}
        }
        if self.live_tickets() >= self.cap {
            return Err(FreezeError::CapFull);
        }
        let id = self.next_ticket;
        self.next_ticket = match id.checked_add(1) {
            Some(n) => n,
            None => return Err(FreezeError::Conflict),
        };
        let t = FreezeTicket {
            id,
            generation: self.generation,
            released: false,
        };
        self.tickets.push(t);
        Ok(t)
    }

    fn ticket_index(&self, id: u16) -> Option<usize> {
        let mut i = 0usize;
        while i < self.tickets.len() {
            if let Some(t) = self.tickets.get(i) {
                if t.id == id {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 用票据消费单点面：返回 Copy 视图并记一次审计。
    ///
    /// 顺序：状态 → 票据存在 → 未归还 → 代次匹配。代次检查放在归还检查
    /// 之后，是为了让「旧代次 + 已归还」这种双错票据报 Conflict（归还状态
    /// 是事实错误）而不是 Stale（代次是相对错误）。
    pub fn consume(&mut self, id: u16) -> Result<ColorView, FreezeError> {
        match self.state {
            FreezeState::Retired => return Err(FreezeError::Terminal),
            FreezeState::Open => return Err(FreezeError::NotFrozen),
            FreezeState::Frozen => {}
        }
        let idx = match self.ticket_index(id) {
            Some(i) => i,
            None => return Err(FreezeError::TicketUnknown),
        };
        let t = match self.tickets.get(idx) {
            Some(t) => *t,
            None => return Err(FreezeError::TicketUnknown),
        };
        if t.released {
            return Err(FreezeError::Conflict);
        }
        if t.generation != self.generation {
            return Err(FreezeError::Stale);
        }
        let snap = match &self.frozen {
            Some(s) => s,
            None => return Err(FreezeError::NotFrozen),
        };
        self.consumed = self.consumed.saturating_add(1);
        Ok(ColorView {
            generation: snap.generation,
            digest: snap.digest,
            intent_code: snap.intent.code(),
        })
    }

    /// 归还票据。重复归还与未知票据都是显式错误（票据是资源，归还必须
    /// 恰好一次——静默重复归还掩盖的是双重释放 bug）。
    pub fn release(&mut self, id: u16) -> Result<(), FreezeError> {
        if self.state == FreezeState::Retired {
            return Err(FreezeError::Terminal);
        }
        let idx = match self.ticket_index(id) {
            Some(i) => i,
            None => return Err(FreezeError::TicketUnknown),
        };
        let t = match self.tickets.get_mut(idx) {
            Some(t) => t,
            None => return Err(FreezeError::TicketUnknown),
        };
        if t.released {
            return Err(FreezeError::Conflict);
        }
        t.released = true;
        Ok(())
    }

    /// 解冻：Frozen → Open。存在未归还票据时拒绝，防止管线读到一半被抽走。
    pub fn thaw(&mut self) -> Result<(), FreezeError> {
        match self.state {
            FreezeState::Retired => return Err(FreezeError::Terminal),
            FreezeState::Open => return Err(FreezeError::NotFrozen),
            FreezeState::Frozen => {}
        }
        if self.live_tickets() > 0 {
            return Err(FreezeError::Conflict);
        }
        self.frozen = None;
        self.tickets.clear();
        self.state = FreezeState::Open;
        Ok(())
    }

    /// 退役：进入终态。允许从 Open/Frozen 直接退役；重复退役显式失败。
    pub fn retire(&mut self) -> Result<(), FreezeError> {
        if self.state == FreezeState::Retired {
            return Err(FreezeError::Terminal);
        }
        self.frozen = None;
        self.tickets.clear();
        self.state = FreezeState::Retired;
        Ok(())
    }

    /// 单点面（免票据）：只读视图，供管线的「快速探测」路径使用。
    ///
    /// 为什么还保留票据：探测路径拿不到矩阵，只能确认「当前代次/摘要」，
    /// 真正绘制必须持票——这样长时间渲染不会在解冻时悬空。
    pub fn pipeline_view(&self) -> Result<ColorView, FreezeError> {
        match self.state {
            FreezeState::Retired => return Err(FreezeError::Terminal),
            FreezeState::Open => return Err(FreezeError::NotFrozen),
            FreezeState::Frozen => {}
        }
        match &self.frozen {
            Some(s) => Ok(ColorView {
                generation: s.generation,
                digest: s.digest,
                intent_code: s.intent.code(),
            }),
            None => Err(FreezeError::NotFrozen),
        }
    }

    /// 取快照本体（矩阵/白点）。必须持有效票据，返回的是不可变引用。
    pub fn snapshot(&self, id: u16) -> Result<&FrozenColor, FreezeError> {
        match self.state {
            FreezeState::Retired => return Err(FreezeError::Terminal),
            FreezeState::Open => return Err(FreezeError::NotFrozen),
            FreezeState::Frozen => {}
        }
        let idx = match self.ticket_index(id) {
            Some(i) => i,
            None => return Err(FreezeError::TicketUnknown),
        };
        let t = match self.tickets.get(idx) {
            Some(t) => t,
            None => return Err(FreezeError::TicketUnknown),
        };
        if t.released {
            return Err(FreezeError::Conflict);
        }
        if t.generation != self.generation {
            return Err(FreezeError::Stale);
        }
        match &self.frozen {
            Some(s) => Ok(s),
            None => Err(FreezeError::NotFrozen),
        }
    }
}

// ---------------------------------------------------------------------------
// 注入核：八类坏序列必须全部被抓
// ---------------------------------------------------------------------------

/// 八类坏序列注入：返回（被抓数, 总数）。每条都断言具体诊断码。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 8usize;
    let p = IccProfile::builtin_srgb();

    // 1 重复冻结
    let mut g = FreezeGate::new(4);
    let _ = g.freeze(&p, Intent::Perceptual);
    if g.freeze(&p, Intent::Perceptual).err() == Some(FreezeError::AlreadyFrozen) {
        caught += 1;
    }
    // 2 Open 态消费 / 探测
    let mut g2 = FreezeGate::new(4);
    if g2.consume(1).err() == Some(FreezeError::NotFrozen)
        && g2.pipeline_view().err() == Some(FreezeError::NotFrozen)
    {
        caught += 1;
    }
    // 3 过期票据（换档 reseal 后代次 +1，旧票据仍活）
    let mut g3 = FreezeGate::new(4);
    let _ = g3.freeze(&p, Intent::Perceptual);
    let t = match g3.acquire() {
        Ok(t) => t,
        Err(_) => FreezeTicket {
            id: 0,
            generation: 0,
            released: true,
        },
    };
    let _ = g3.reseal(&p, Intent::Relative);
    if g3.consume(t.id).err() == Some(FreezeError::Stale) {
        caught += 1;
    }
    // 4 已释放票据
    let mut g4 = FreezeGate::new(4);
    let _ = g4.freeze(&p, Intent::Perceptual);
    let t4 = match g4.acquire() {
        Ok(t) => t,
        Err(_) => FreezeTicket {
            id: 0,
            generation: 0,
            released: true,
        },
    };
    let _ = g4.release(t4.id);
    if g4.consume(t4.id).err() == Some(FreezeError::Conflict) {
        caught += 1;
    }
    // 5 带活票据解冻
    let mut g5 = FreezeGate::new(4);
    let _ = g5.freeze(&p, Intent::Perceptual);
    let _ = g5.acquire();
    if g5.thaw().err() == Some(FreezeError::Conflict) {
        caught += 1;
    }
    // 6 未知票据
    let mut g6 = FreezeGate::new(4);
    let _ = g6.freeze(&p, Intent::Perceptual);
    if g6.consume(999).err() == Some(FreezeError::TicketUnknown)
        && g6.release(999).err() == Some(FreezeError::TicketUnknown)
    {
        caught += 1;
    }
    // 7 票据容量满
    let mut g7 = FreezeGate::new(1);
    let _ = g7.freeze(&p, Intent::Perceptual);
    let _ = g7.acquire();
    if g7.acquire().err() == Some(FreezeError::CapFull) {
        caught += 1;
    }
    // 8 退役后冻结（终态）
    let mut g8 = FreezeGate::new(2);
    let _ = g8.retire();
    if g8.freeze(&p, Intent::Perceptual).err() == Some(FreezeError::Terminal)
        && g8.thaw().err() == Some(FreezeError::Terminal)
    {
        caught += 1;
    }
    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_unx50013_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50013");
    let p = IccProfile::builtin_srgb();
    let printer = IccProfile::simulated_printer(0.7);

    // J1-1 冻结登记：Open → Frozen，代次从 0 起自增到 1
    let mut g = FreezeGate::new(8);
    let ok_open = g.state() == FreezeState::Open && g.generation() == 0;
    let gen1 = g.freeze(&p, Intent::Perceptual);
    let ok = ok_open && gen1 == Ok(1) && g.state() == FreezeState::Frozen && g.generation() == 1;
    let det = format!("gen={:?} state={}", gen1, g.state().label());
    s.add_dyn("50013-J1-冻结登记", ok, &det);

    // J1-2 快照保真：矩阵/白点逐元素等于源 profile（须持票取矩阵）
    let t1 = g.acquire();
    let ok = match (&t1, g.snapshot(1)) {
        (Ok(_), Ok(sn)) => {
            let mut same = true;
            let mut i = 0usize;
            while i < 3 {
                let mut j = 0usize;
                while j < 3 {
                    if fabsf(sn.matrix[i][j] - p.rgb_to_xyz.m[i][j]) > 1e-9 {
                        same = false;
                    }
                    j += 1;
                }
                i += 1;
            }
            same
                && fabsf(sn.white[0] - p.white.x) < 1e-9
                && fabsf(sn.white[2] - p.white.z) < 1e-9
                && sn.intent == Intent::Perceptual
        }
        _ => false,
    };
    s.add("50013-J1-快照保真", ok, if ok { "" } else { "快照与源 profile 不一致" });

    // J1-3 摘要确定性 + 意图敏感
    let d1 = snapshot_digest(&p, Intent::Perceptual);
    let d2 = snapshot_digest(&p, Intent::Perceptual);
    let d3 = snapshot_digest(&p, Intent::Relative);
    let d4 = snapshot_digest(&printer, Intent::Perceptual);
    let ok = d1 == d2 && d1 != d3 && d1 != d4 && d1 != 0;
    let det = format!("d1={:x} d3={:x}", d1, d3);
    s.add_dyn("50013-J1-摘要敏感", ok, &det);

    // J1-4 无票据不得取矩阵、Open 态不得消费
    let mut probe = FreezeGate::new(2);
    let open_consume = probe.consume(1).err() == Some(FreezeError::NotFrozen);
    let open_view = probe.pipeline_view().err() == Some(FreezeError::NotFrozen);
    let _ = probe.freeze(&p, Intent::Perceptual);
    let no_ticket = probe.snapshot(77).err() == Some(FreezeError::TicketUnknown);
    let ok = open_consume && open_view && no_ticket;
    s.add("50013-J1-未冻结必拒", ok, if ok { "" } else { "未冻结路径未拒绝" });

    // J1-5 二次冻结必拒
    let ok = g.freeze(&p, Intent::Relative).err() == Some(FreezeError::AlreadyFrozen);
    s.add("50013-J1-重复冻结必拒", ok, if ok { "" } else { "重复冻结被接受" });

    // J1-6 单点面一致：探测视图与持票消费返回同一摘要/代次
    let view = g.pipeline_view();
    let c1 = g.consume(1);
    let t2 = g.acquire();
    let c2 = match &t2 {
        Ok(t) => g.consume(t.id),
        Err(_) => Err(FreezeError::TicketUnknown),
    };
    let ok = match (view, c1, c2) {
        (Ok(v), Ok(a), Ok(b)) => {
            v.digest == a.digest && a.digest == b.digest && a.generation == b.generation
        }
        _ => false,
    };
    s.add("50013-J1-单点一致", ok, if ok { "" } else { "单点面视图不一致" });

    // J1-7 已释放票据必拒（且重复归还也冲突）
    let rel = match &t2 {
        Ok(t) => g.release(t.id),
        Err(_) => Err(FreezeError::TicketUnknown),
    };
    let again = match &t2 {
        Ok(t) => g.release(t.id),
        Err(_) => Err(FreezeError::TicketUnknown),
    };
    let consume_after = match &t2 {
        Ok(t) => g.consume(t.id),
        Err(_) => Err(FreezeError::TicketUnknown),
    };
    let ok = rel == Ok(())
        && again.err() == Some(FreezeError::Conflict)
        && consume_after.err() == Some(FreezeError::Conflict);
    s.add("50013-J1-释放必拒", ok, if ok { "" } else { "释放语义错" });

    // J1-8 活票据阻止解冻；全部归还后解冻成功
    let blocked = g.thaw().err() == Some(FreezeError::Conflict);
    let r1 = match &t1 {
        Ok(t) => g.release(t.id),
        Err(_) => Err(FreezeError::TicketUnknown),
    };
    let thawed = g.thaw() == Ok(()) && g.state() == FreezeState::Open && g.live_tickets() == 0;
    let ok = blocked && r1 == Ok(()) && thawed;
    let det = format!("blocked={} thawed={}", blocked, thawed);
    s.add_dyn("50013-J1-解冻闸", ok, &det);

    // J1-9 代次单调 + 换档后旧票据必报 Stale
    let gen2 = g.freeze(&printer, Intent::Relative);
    let t9 = g.acquire();
    let gen3 = g.reseal(&p, Intent::Perceptual);
    let old_ticket = match &t9 {
        Ok(t) => g.consume(t.id),
        Err(_) => Err(FreezeError::TicketUnknown),
    };
    let ok = gen2 == Ok(2)
        && gen3 == Ok(3)
        && g.generation() == 3
        && old_ticket.err() == Some(FreezeError::Stale)
        && g.pipeline_view().map(|v| v.generation) == Ok(3);
    let det = format!("g2={:?} g3={:?} old={:?}", gen2, gen3, old_ticket);
    s.add_dyn("50013-J1-代次单调", ok, &det);

    // J1-10 双门同输入 → 同摘要（跨实例可复算）
    let mut ga = FreezeGate::new(2);
    let mut gb = FreezeGate::new(2);
    let _ = ga.freeze(&printer, Intent::Absolute);
    let _ = gb.freeze(&printer, Intent::Absolute);
    let ok = match (ga.pipeline_view(), gb.pipeline_view()) {
        (Ok(a), Ok(b)) => a.digest == b.digest && a.intent_code == Intent::Absolute.code(),
        _ => false,
    };
    s.add("50013-J1-跨门一致", ok, if ok { "" } else { "同输入摘要不同" });

    // J1-11 退役是终态：之后一切接口都报 Terminal
    let mut gt = FreezeGate::new(2);
    let r0 = gt.retire();
    let r1 = gt.retire();
    let ok = r0 == Ok(())
        && r1.err() == Some(FreezeError::Terminal)
        && gt.freeze(&p, Intent::Perceptual).err() == Some(FreezeError::Terminal)
        && gt.acquire().err() == Some(FreezeError::Terminal)
        && gt.consume(1).err() == Some(FreezeError::Terminal)
        && gt.pipeline_view().err() == Some(FreezeError::Terminal)
        && gt.thaw().err() == Some(FreezeError::Terminal);
    s.add("50013-J1-退役终态", ok, if ok { "" } else { "退役后仍可操作" });

    // J1-12 容量满必拒，且消费审计计数真实
    let mut gc = FreezeGate::new(1);
    let _ = gc.freeze(&p, Intent::Perceptual);
    let a = gc.acquire();
    let b = gc.acquire();
    let c = match &a {
        Ok(t) => gc.consume(t.id),
        Err(_) => Err(FreezeError::TicketUnknown),
    };
    let ok = a.is_ok() && b.err() == Some(FreezeError::CapFull) && c.is_ok() && gc.consumed() == 1;
    let det = format!("capfull={:?} consumed={}", b, gc.consumed());
    s.add_dyn("50013-J1-容量与审计", ok, &det);

    // J1-13 错误码落在段内闭集（每个变体逐一核对）
    let all = [
        FreezeError::Empty,
        FreezeError::NotFrozen,
        FreezeError::AlreadyFrozen,
        FreezeError::Stale,
        FreezeError::TicketUnknown,
        FreezeError::CapFull,
        FreezeError::Conflict,
        FreezeError::Terminal,
        FreezeError::Digest,
    ];
    let mut ok = true;
    let mut i = 0usize;
    while i < all.len() {
        if let Some(e) = all.get(i) {
            let c = e.code();
            if c <= SEG_BASE || c > SEG_BASE + 0x1f || e.message().is_empty() {
                ok = false;
            }
        }
        i += 1;
    }
    s.add("50013-J1-码域闭集", ok, if ok { "" } else { "诊断码越段" });

    // J1-14 八类坏序列全抓
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50013-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50013_checks_green() {
        let set = run_unx50013_checks();
        assert!(set.all_passed(), "UNX-F50013 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50013_generation_monotonic() {
        let p = IccProfile::builtin_srgb();
        let mut g = FreezeGate::new(4);
        assert_eq!(g.freeze(&p, Intent::Perceptual), Ok(1));
        let t = g.acquire();
        if let Ok(t) = t {
            let _ = g.release(t.id);
        }
        assert_eq!(g.thaw(), Ok(()));
        assert_eq!(g.freeze(&p, Intent::Saturation), Ok(2));
    }

    #[test]
    fn unx50013_digest_is_deterministic() {
        let p = IccProfile::builtin_srgb();
        assert_eq!(
            snapshot_digest(&p, Intent::Perceptual),
            snapshot_digest(&p, Intent::Perceptual)
        );
    }
}
