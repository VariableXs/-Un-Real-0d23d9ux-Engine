//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12612 ALPC 上下文端口与直接/间接消息 · 0x80300段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② 判据原文：UNX-F12613-J1 句柄随消息传递协议（复制语义/权限收缩）
//!      对平断言绿，坏句柄注入 10/10 拒止；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 上下文端口两形态闭集：直接消息（数据内联，≤ PORT_DIRECT_MAX 字节）
//!     与间接消息（数据落在共享区，消息体只带区段长度与句柄）——形态与
//!     载荷越界即显性 Err，不静默降级；
//!   - 句柄随消息传递协议：消息跨端时携带句柄，接收侧得到的是**复制**
//!     （发送侧原句柄仍有效，接收侧新句柄值独立记账），权限只许收缩
//!     不许扩张——请求掩码超出源权限即拒（RightsExpand）；
//!   - 复制槽状态机单调：Free→DupPending→Duped→Revoked，只许前进，
//!     倒退显性拒（StateRegress）；Revoked 后再使用必抓（RevokedUse）；
//!   - 坏句柄注入 10/10：越界句柄 / 空 / 已吊销 三族各次全被拒止，
//!     拒止码逐次入账，账目 tries==caught 才算绿；
//!   - 黄金判定表：LCG 抽 10 组（形态×请求权限）与闭式期望对平，
//!     双跑指纹一致才放行；
//!   - LCG 种子写死 = SEG_BASE（0x80300），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect；诊断码闭集 0x80300..0x80340。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x80300，AI-VX9-1105 B01 批 12 槽第 1 槽）。
pub const SEG_BASE: u32 = 0x80300;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CtxPortCode {
    /// 坏句柄：槽位不存在或越界
    HandleNotFound,
    /// 权限扩张：请求掩码超出源权限（只许收缩）
    RightsExpand,
    /// 复制槽表已满：显性拒收不静默挤占
    SlotFull,
    /// 直接消息载荷超内联上限
    DirectOverflow,
    /// 间接消息区段长度越界
    IndirectOutOfRange,
    /// 状态机倒退：复制槽状态只能前进
    StateRegress,
    /// 形态错配：直接/间接与记账声明不符
    FormMismatch,
    /// 复制账缺失：消息带句柄但跨端未复制
    DupMissing,
    /// 已吊销句柄仍被使用
    RevokedUse,
    /// 双跑指纹漂移：同输入两次结算不一致
    FingerprintDrift,
    /// 守恒破：复制账 不等式（复制数≠成功+吊销）
    ConservationBreak,
    /// 未知诊断码兜底提示来源
    Unknown,
}

impl CtxPortCode {
    /// 诊断码取值：SEG_BASE + 序号，段内唯一。
    pub const fn val(self) -> u32 {
        SEG_BASE
            + match self {
                CtxPortCode::HandleNotFound => 0x01,
                CtxPortCode::RightsExpand => 0x02,
                CtxPortCode::SlotFull => 0x03,
                CtxPortCode::DirectOverflow => 0x04,
                CtxPortCode::IndirectOutOfRange => 0x05,
                CtxPortCode::StateRegress => 0x06,
                CtxPortCode::FormMismatch => 0x07,
                CtxPortCode::DupMissing => 0x08,
                CtxPortCode::RevokedUse => 0x09,
                CtxPortCode::FingerprintDrift => 0x0A,
                CtxPortCode::ConservationBreak => 0x0B,
                CtxPortCode::Unknown => 0x3F,
            }
    }

    /// 中文诊断：含可操作建议（写入 detail 文案）。
    pub fn detail(self) -> &'static str {
        match self {
            CtxPortCode::HandleNotFound => "句柄不在表内：核对传入句柄来源，勿手搓句柄值",
            CtxPortCode::RightsExpand => "权限扩张被拒：申请掩码必须是源权限子集",
            CtxPortCode::SlotFull => "复制槽满：先吊销失效槽位或扩容账面",
            CtxPortCode::DirectOverflow => "直接消息超内联上限：改走间接消息（共享区+句柄）",
            CtxPortCode::IndirectOutOfRange => "间接区段长度越界：核对区段登记长度",
            CtxPortCode::StateRegress => "复制槽状态倒退：状态机只进不退，重建槽位",
            CtxPortCode::FormMismatch => "消息形态与记账不符：重发并声明一致形态",
            CtxPortCode::DupMissing => "带句柄消息未复制：先走复制路径再投递",
            CtxPortCode::RevokedUse => "句柄已吊销：重新申请复制，勿复用旧值",
            CtxPortCode::FingerprintDrift => "双跑指纹漂移：检查非确定输入，冻结种子",
            CtxPortCode::ConservationBreak => "复制账守恒破：对账 复制数=成功+吊销",
            CtxPortCode::Unknown => "未知诊断码：不在 0x80300 段闭集内",
        }
    }

    /// 闭集 ALL 数组（闭集返回，遍历守卫用）。
    pub const ALL: [CtxPortCode; 12] = [
        CtxPortCode::HandleNotFound,
        CtxPortCode::RightsExpand,
        CtxPortCode::SlotFull,
        CtxPortCode::DirectOverflow,
        CtxPortCode::IndirectOutOfRange,
        CtxPortCode::StateRegress,
        CtxPortCode::FormMismatch,
        CtxPortCode::DupMissing,
        CtxPortCode::RevokedUse,
        CtxPortCode::FingerprintDrift,
        CtxPortCode::ConservationBreak,
        CtxPortCode::Unknown,
    ];
}

/// 闭集查表入口：u32 码 → 中文诊断（未命中走 Unknown 兜底）。
pub fn detail(code: u32) -> &'static str {
    for c in CtxPortCode::ALL.iter() {
        if c.val() == code {
            return c.detail();
        }
    }
    CtxPortCode::Unknown.detail()
}

// ── 冻结常量表（来源：ALPC 端口语义·消息属性冻结册） ────────────
/// 直接消息内联上限（字节）。超过即改走间接形态。
pub const PORT_DIRECT_MAX: u32 = 64;
/// 间接消息区段上限（字节）。
pub const PORT_INDIRECT_MAX: u32 = 4096;
/// 复制槽表容量（冻结，扩容须走账面变更）。
pub const HANDLE_SLOTS: usize = 16;

/// 权限位：读。
pub const RIGHT_READ: u32 = 0x1;
/// 权限位：写。
pub const RIGHT_WRITE: u32 = 0x2;
/// 权限位：可再复制（DUPLICATE）。
pub const RIGHT_DUP: u32 = 0x4;
/// 全权限冻结掩码。
pub const RIGHT_ALL: u32 = RIGHT_READ | RIGHT_WRITE | RIGHT_DUP;

// ── 消息形态与消息体 ─────────────────────────────────────────────
/// 消息形态闭集：直接 / 间接。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MsgForm {
    /// 直接消息：载荷内联，≤ PORT_DIRECT_MAX。
    Direct,
    /// 间接消息：载荷在共享区，消息体带区段长度。
    Indirect,
}

/// 一条跨端消息：形态 + 载荷长度 + 区段长度 + 随行句柄（可选）。
#[derive(Clone, Copy, Debug)]
pub struct AlpcMessage {
    /// 消息形态（直接/间接）。
    pub form: MsgForm,
    /// 内联载荷长度（直接形态有效）。
    pub inline_len: u32,
    /// 共享区段长度（间接形态有效）。
    pub section_len: u32,
    /// 随行源句柄（None=不带句柄）。
    pub carry_handle: Option<u32>,
}

impl AlpcMessage {
    /// 构造直接消息。
    pub const fn direct(inline_len: u32) -> AlpcMessage {
        AlpcMessage { form: MsgForm::Direct, inline_len, section_len: 0, carry_handle: None }
    }

    /// 构造间接消息。
    pub const fn indirect(section_len: u32) -> AlpcMessage {
        AlpcMessage { form: MsgForm::Indirect, inline_len: 0, section_len, carry_handle: None }
    }

    /// 携带句柄（建造者式，返回新值保持 Copy 语义可读）。
    pub const fn with_handle(mut self, h: u32) -> AlpcMessage {
        self.carry_handle = Some(h);
        self
    }

    /// 形态-长度自洽校验：直接看内联，间接看区段。
    pub fn bounds_ok(&self) -> bool {
        match self.form {
            MsgForm::Direct => self.inline_len <= PORT_DIRECT_MAX && self.section_len == 0,
            MsgForm::Indirect => {
                self.section_len <= PORT_INDIRECT_MAX && self.section_len > 0
                    && self.inline_len == 0
            }
        }
    }
}

// ── 复制槽状态机（单调，只进不退） ──────────────────────────────
/// 复制槽状态：Free(0)→DupPending(1)→Duped(2)→Revoked(3)。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DupState {
    /// 空槽。
    Free,
    /// 复制在途（跨端联签未回签）。
    DupPending,
    /// 复制完成，接收侧句柄可用。
    Duped,
    /// 已吊销（终态，不可复活）。
    Revoked,
}

impl DupState {
    /// 状态序数：用于单调性判定。
    pub const fn rank(self) -> u32 {
        match self {
            DupState::Free => 0,
            DupState::DupPending => 1,
            DupState::Duped => 2,
            DupState::Revoked => 3,
        }
    }
}

/// 一个复制槽：源句柄 / 接收侧句柄 / 收缩后权限 / 状态。
#[derive(Clone, Copy, Debug)]
pub struct DupSlot {
    /// 发送侧源句柄。
    pub src: u32,
    /// 接收侧新句柄（复制语义：与 src 独立）。
    pub dst: u32,
    /// 收缩后权限掩码（恒为源权限子集）。
    pub rights: u32,
    /// 槽状态。
    pub state: DupState,
}

// ── 上下文端口 ───────────────────────────────────────────────────
/// ALPC 上下文端口：句柄表 + 收发账。
pub struct CtxPort {
    /// 复制槽表（定容 HANDLE_SLOTS，None=空槽）。
    slots: Vec<Option<DupSlot>>,
    /// 源句柄→槽下标的登记账（线性查找，容量小）。
    src_index: Vec<(u32, usize)>,
    /// 累计收到的消息数。
    pub recv_total: u32,
    /// 累计投递的消息数（含拒收前的有效判定）。
    pub sent_total: u32,
    /// 累计复制成功数。
    pub dup_ok_total: u32,
    /// 累计吊销数。
    pub revoke_total: u32,
}

impl CtxPort {
    /// 新端口：空表空账。
    pub fn new() -> CtxPort {
        let mut slots = Vec::new();
        let mut i = 0;
        while i < HANDLE_SLOTS {
            slots.push(None);
            i += 1;
        }
        CtxPort {
            slots,
            src_index: Vec::new(),
            recv_total: 0,
            sent_total: 0,
            dup_ok_total: 0,
            revoke_total: 0,
        }
    }

    /// 按 src 句柄找槽下标（get 语义，不裸取）。
    fn find_slot(&self, src: u32) -> Option<usize> {
        for (s, idx) in self.src_index.iter() {
            if *s == src {
                return Some(*idx);
            }
        }
        None
    }

    /// 收一条消息：先形态校验，再走句柄随行协议。
    /// 返回接收侧拿到的新句柄（不带句柄时返回 0）。
    pub fn receive(&mut self, msg: &AlpcMessage, req_rights: u32) -> Result<u32, u32> {
        // 形态与长度闭式校验（直接/间接各自边界）。
        if !msg.bounds_ok() {
            return Err(match msg.form {
                MsgForm::Direct => CtxPortCode::DirectOverflow.val(),
                MsgForm::Indirect => CtxPortCode::IndirectOutOfRange.val(),
            });
        }
        self.recv_total += 1;
        // 无随行句柄：直接放行，不涉及复制账。
        let src = match msg.carry_handle {
            None => {
                self.sent_total += 1;
                return Ok(0);
            }
            Some(h) => h,
        };
        // 句柄随消息传递协议：接收侧必须落一笔复制账。
        // 源句柄在本端口表内 → 走收缩复制；不在 → DupMissing（坏注入面）。
        let idx = match self.find_slot(src) {
            None => return Err(CtxPortCode::DupMissing.val()),
            Some(i) => i,
        };
        let slot = match self.slots.get(idx) {
            Some(Some(s)) => *s,
            _ => return Err(CtxPortCode::HandleNotFound.val()),
        };
        // 源槽必须处于 Duped（可再复制需 RIGHT_DUP）。
        if slot.state != DupState::Duped {
            return Err(CtxPortCode::StateRegress.val());
        }
        // 权限收缩闭式：请求必须是子集；且源须有 RIGHT_DUP 才能再复制。
        if req_rights & !slot.rights != 0 {
            return Err(CtxPortCode::RightsExpand.val());
        }
        if slot.rights & RIGHT_DUP == 0 {
            return Err(CtxPortCode::RightsExpand.val());
        }
        // 找空槽落新复制。
        let mut placed: Option<usize> = None;
        let mut i = 0;
        while i < HANDLE_SLOTS {
            if let Some(None) = self.slots.get(i) {
                placed = Some(i);
                break;
            }
            i += 1;
        }
        let pi = match placed {
            None => return Err(CtxPortCode::SlotFull.val()),
            Some(p) => p,
        };
        // 新句柄值：按槽序确定性派生（段内唯一，槽不复用故不撞）。
        let dst = SEG_BASE.wrapping_add(pi as u32);
        let fresh = DupSlot { src, dst, rights: req_rights, state: DupState::DupPending };
        if let Some(cell) = self.slots.get_mut(pi) {
            *cell = Some(fresh);
        }
        self.src_index.push((dst, pi));
        // 回签：DupPending → Duped（跨端联签到位即完成）。
        if let Some(Some(s)) = self.slots.get_mut(pi) {
            s.state = DupState::Duped;
        }
        self.dup_ok_total += 1;
        self.sent_total += 1;
        Ok(dst)
    }

    /// 使用接收侧句柄：状态须 Duped 且权限覆盖所需位。
    pub fn use_handle(&self, h: u32, need: u32) -> Result<u32, u32> {
        let idx = match self.find_slot(h) {
            None => return Err(CtxPortCode::HandleNotFound.val()),
            Some(i) => i,
        };
        let slot = match self.slots.get(idx) {
            Some(Some(s)) => *s,
            _ => return Err(CtxPortCode::HandleNotFound.val()),
        };
        if slot.state == DupState::Revoked {
            return Err(CtxPortCode::RevokedUse.val());
        }
        if slot.state != DupState::Duped {
            return Err(CtxPortCode::StateRegress.val());
        }
        if need & !slot.rights != 0 {
            return Err(CtxPortCode::RightsExpand.val());
        }
        Ok(slot.rights)
    }

    /// 吊销句柄：Duped→Revoked（前进），Free/Pending 吊销即倒退拒。
    pub fn revoke(&mut self, h: u32) -> Result<(), u32> {
        let idx = match self.find_slot(h) {
            None => return Err(CtxPortCode::HandleNotFound.val()),
            Some(i) => i,
        };
        let cur = match self.slots.get(idx) {
            Some(Some(s)) => s.state,
            _ => return Err(CtxPortCode::HandleNotFound.val()),
        };
        if cur != DupState::Duped {
            return Err(CtxPortCode::StateRegress.val());
        }
        if let Some(Some(s)) = self.slots.get_mut(idx) {
            s.state = DupState::Revoked;
        }
        self.revoke_total += 1;
        Ok(())
    }

    /// 预置源槽（首端登记：只造 Duped 源，不做跨端）。
    pub fn seed_source(&mut self, src: u32, rights: u32) -> Result<(), u32> {
        if rights & !RIGHT_ALL != 0 {
            return Err(CtxPortCode::RightsExpand.val());
        }
        if self.find_slot(src).is_some() {
            return Err(CtxPortCode::SlotFull.val());
        }
        let mut placed: Option<usize> = None;
        let mut i = 0;
        while i < HANDLE_SLOTS {
            if let Some(None) = self.slots.get(i) {
                placed = Some(i);
                break;
            }
            i += 1;
        }
        let pi = match placed {
            None => return Err(CtxPortCode::SlotFull.val()),
            Some(p) => p,
        };
        if let Some(cell) = self.slots.get_mut(pi) {
            *cell = Some(DupSlot { src, dst: 0, rights, state: DupState::Duped });
        }
        self.src_index.push((src, pi));
        Ok(())
    }

    /// 守恒复核：复制成功数 + 拒收在途 = 账面可解释。
    /// 本闭式：dup_ok_total ≤ recv_total 且 ≤ HANDLE_SLOTS * 2。
    pub fn conservation(&self) -> Result<(), u32> {
        if self.dup_ok_total > self.recv_total {
            return Err(CtxPortCode::ConservationBreak.val());
        }
        if self.revoke_total > self.dup_ok_total {
            return Err(CtxPortCode::ConservationBreak.val());
        }
        Ok(())
    }

    /// 结算指纹：把账面数字折成一个 u32（双跑一致基准）。
    pub fn fingerprint(&self) -> u32 {
        self.recv_total
            .wrapping_mul(31)
            .wrapping_add(self.sent_total.wrapping_mul(7))
            .wrapping_add(self.dup_ok_total.wrapping_mul(3))
            .wrapping_add(self.revoke_total)
    }
}

// ── 黄金判定表（抽样对平基准） ───────────────────────────────────
/// 黄金闭式：给定（形态合法、请求权限相对源）期望结果。
/// 形态不合法 → Err(形态对应码)；权限扩张 → Err(RightsExpand)；
/// 其余 → Ok（不带句柄=0，带句柄=新句柄非零）。
pub fn golden_expect(form_ok: bool, expand: bool) -> bool {
    form_ok && !expand
}

/// 确定性 LCG（种子写死 = SEG_BASE）。
pub struct Lcg(u64);
impl Lcg {
    pub const fn new(seed: u64) -> Lcg {
        Lcg(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407))
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// 抽样 10 例：LCG 抽（形态、载荷、请求权限）组合逐一与黄金闭式对平。
/// 全对平返回 Ok(10)，否则 Err。
pub fn sample_ten() -> Result<u32, u32> {
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut agree = 0u32;
    let mut i = 0;
    while i < 10 {
        let form_ok = rng.below(2) == 0;
        let expand = rng.below(2) == 0;
        let mut p = CtxPort::new();
        // 先种一个全权源句柄。
        let _ = p.seed_source(0x1110, RIGHT_ALL);
        let msg = if form_ok {
            AlpcMessage::direct(PORT_DIRECT_MAX).with_handle(0x1110)
        } else {
            // 故意坏形态：直接消息超上限。
            AlpcMessage::direct(PORT_DIRECT_MAX + 1).with_handle(0x1110)
        };
        let req = if expand { RIGHT_ALL | 0x8 } else { RIGHT_READ };
        let res = p.receive(&msg, req);
        let ok = res.is_ok();
        if ok == golden_expect(form_ok, expand) {
            agree += 1;
        }
        i += 1;
    }
    if agree == 10 {
        Ok(10)
    } else {
        Err(CtxPortCode::FingerprintDrift.val())
    }
}

// ── 注入面：坏句柄 10/10 必抓 ────────────────────────────────────
/// 注入账：tries/caught 双计数，caught==tries 才算全抓。
pub struct InjectLedger {
    pub tries: u32,
    pub caught: u32,
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: 0, caught: 0 }
    }
    pub fn all_caught(&self) -> bool {
        self.tries > 0 && self.caught == self.tries
    }
}

/// 坏句柄注入族：越界值 / 未登记值 / 已吊销值 各轮换，共 10 次。
/// 每次都必须被拒（Err），且拒止码落在闭集内。
pub fn inject_bad_handle(led: &mut InjectLedger) {
    let mut p = CtxPort::new();
    let _ = p.seed_source(0x2220, RIGHT_ALL);
    // 先造一个可吊销的接收侧句柄。
    let m0 = AlpcMessage::direct(8).with_handle(0x2220);
    let dup = p.receive(&m0, RIGHT_READ).unwrap_or(0);
    let _ = p.revoke(dup);
    let mut i = 0;
    while i < 10 {
        led.tries += 1;
        // 三族轮换：i%3==0 越界（0xFFFFFFF0+）、i%3==1 未登记、i%3==2 已吊销。
        let bad = match i % 3 {
            0 => 0xFFFFFFF0u32.wrapping_add(i as u32),
            1 => 0x0BAD0BADu32.wrapping_add(i as u32),
            _ => dup,
        };
        let need = if i % 2 == 0 { RIGHT_READ } else { RIGHT_WRITE };
        let res = p.use_handle(bad, need);
        let in_closed = match res {
            Err(code) => {
                // 拒止码必须落在段闭集内（不静默、不越段）。
                code >= SEG_BASE && code < SEG_BASE + 0x40
            }
            Ok(_) => false,
        };
        if in_closed {
            led.caught += 1;
        }
        i += 1;
    }
}

// ── 端到端正演 + 双跑一致 ────────────────────────────────────────
/// 正演：种源→直接带柄消息→收缩复制→使用→吊销→守恒 全绿。
pub fn golden_run() -> Result<u32, u32> {
    let mut p = CtxPort::new();
    p.seed_source(0x3330, RIGHT_ALL)?;
    let m = AlpcMessage::direct(16).with_handle(0x3330);
    let dst = p.receive(&m, RIGHT_READ | RIGHT_WRITE)?;
    if dst == 0 {
        return Err(CtxPortCode::DupMissing.val());
    }
    // 使用：需求权限 ≤ 收缩后权限（读写 ≤ 读写）。
    p.use_handle(dst, RIGHT_READ)?;
    // 间接消息不带柄也放行。
    let m2 = AlpcMessage::indirect(512);
    p.receive(&m2, RIGHT_READ)?;
    p.conservation()?;
    Ok(p.fingerprint())
}

/// 双跑一致：同一脚本跑两遍，指纹必须相等。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run();
    let b = golden_run();
    match (a, b) {
        (Ok(fa), Ok(fb)) => {
            if fa == fb {
                Ok(())
            } else {
                Err(CtxPortCode::FingerprintDrift.val())
            }
        }
        _ => Err(CtxPortCode::ConservationBreak.val()),
    }
}

// ── 判据谓词（私有 xxx_ok） ──────────────────────────────────────
/// J1-A：复制语义对平——接收侧新句柄与源独立且可使用。
fn dup_semantics_ok() -> bool {
    let mut p = CtxPort::new();
    if p.seed_source(0x4440, RIGHT_ALL).is_err() {
        return false;
    }
    let m = AlpcMessage::direct(4).with_handle(0x4440);
    match p.receive(&m, RIGHT_READ) {
        Ok(dst) => {
            if dst == 0x4440 {
                return false; // 复制语义：新句柄不得等于源值
            }
            p.use_handle(dst, RIGHT_READ).is_ok()
        }
        Err(_) => false,
    }
}

/// J1-B：权限收缩对平——收缩后权限恒为源子集，越权必拒。
/// 源权限 = READ|DUP（可复制基线），请求只许取子集。
fn rights_shrink_ok() -> bool {
    let mut p = CtxPort::new();
    if p.seed_source(0x4450, RIGHT_READ | RIGHT_DUP).is_err() {
        return false;
    }
    let m = AlpcMessage::direct(4).with_handle(0x4450);
    // 请求 WRITE：源无 WRITE 位 → 扩张必拒。
    if p.receive(&m, RIGHT_WRITE).is_ok() {
        return false;
    }
    // 请求 READ：子集 → 收缩成功，且后续用 WRITE 必拒。
    match p.receive(&m, RIGHT_READ) {
        Ok(dst) => p.use_handle(dst, RIGHT_WRITE).is_err(),
        Err(_) => false,
    }
}

/// J2：坏句柄注入 10/10 全抓。
fn inject_ok() -> bool {
    let mut led = InjectLedger::new();
    inject_bad_handle(&mut led);
    led.all_caught()
}

/// J3：抽样 10 例对平。
fn sample_ok() -> bool {
    sample_ten() == Ok(10)
}

/// J4：双跑一致。
fn repro_ok() -> bool {
    repro_check().is_ok()
}

/// 段位守卫：VX9 窗口内、0x40 对齐、诊断码闭集落段。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if !(0x80000..=0x831C0).contains(&SEG_BASE) {
        return false;
    }
    for c in CtxPortCode::ALL.iter() {
        let v = c.val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
    }
    true
}

// ── 检查入口（≥12 条，全部 add_dyn） ─────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 正常流：黄金正演。
    let g = golden_run();
    v.push(("J1-黄金正演", g.is_ok(), format!("{:?}", g)));
    // 正常流：直接/间接两形态边界。
    let mut p = CtxPort::new();
    let _ = p.seed_source(0x4460, RIGHT_ALL);
    let dmax = p.receive(&AlpcMessage::direct(PORT_DIRECT_MAX), RIGHT_READ).is_ok();
    let d_over = p.receive(&AlpcMessage::direct(PORT_DIRECT_MAX + 1), RIGHT_READ).is_err();
    let imax = p.receive(&AlpcMessage::indirect(PORT_INDIRECT_MAX), RIGHT_READ).is_ok();
    let i_over = p.receive(&AlpcMessage::indirect(PORT_INDIRECT_MAX + 1), RIGHT_READ).is_err();
    v.push((
        "J1-两形态边界",
        dmax && d_over && imax && i_over,
        format!("直{}+/直超{}拒/间{}+/间超{}拒", dmax, d_over, imax, i_over),
    ));
    // 谓词：复制语义。
    v.push(("J1-复制语义对平", dup_semantics_ok(), String::from("dst≠src 且可用")));
    // 谓词：权限收缩。
    v.push(("J1-权限收缩对平", rights_shrink_ok(), String::from("扩张拒/子集过")));
    // 注入必抓。
    v.push(("J2-坏句柄注入10/10", inject_ok(), String::from("越界/未登记/吊销")));
    // 抽样。
    v.push(("J3-抽样10例对平", sample_ok(), format!("{:?}", sample_ten())));
    // 双跑一致。
    v.push(("J4-双跑一致", repro_ok(), String::from("指纹相等")));
    // 边界：槽满显性拒（种 1 源占 1 槽，再连收 16 次必在第 16 次满账拒收）。
    let mut q = CtxPort::new();
    let _ = q.seed_source(0x5000, RIGHT_ALL);
    let mut full_rejected = false;
    let mut k = 0;
    while k <= HANDLE_SLOTS {
        let m = AlpcMessage::direct(4).with_handle(0x5000);
        let r = q.receive(&m, RIGHT_READ);
        if k == HANDLE_SLOTS - 1 {
            full_rejected = r == Err(CtxPortCode::SlotFull.val());
        }
        k += 1;
    }
    v.push(("J5-槽满显性拒", full_rejected, String::from("16 槽冻结")));
    // 边界：吊销后再用必抓。
    let mut r2 = CtxPort::new();
    let _ = r2.seed_source(0x5040, RIGHT_ALL);
    let h = r2.receive(&AlpcMessage::direct(4).with_handle(0x5040), RIGHT_READ).unwrap_or(0);
    let _ = r2.revoke(h);
    let revoked_rejected = r2.use_handle(h, RIGHT_READ)
        == Err(CtxPortCode::RevokedUse.val());
    v.push(("J5-吊销后再用拒", revoked_rejected, String::from("Revoked 终态")));
    // 守恒。
    let mut r3 = CtxPort::new();
    let _ = r3.seed_source(0x5080, RIGHT_ALL);
    let _ = r3.receive(&AlpcMessage::direct(4).with_handle(0x5080), RIGHT_READ);
    v.push((
        "J6-账面守恒",
        r3.conservation().is_ok(),
        String::from("dup≤recv 且 revoke≤dup"),
    ));
    // 段位守卫。
    v.push(("J7-段位守卫", segment_guard(), String::from("0x80300 窗口")));
    // 闭集查表兜底。
    v.push((
        "J7-闭集查表",
        detail(CtxPortCode::RightsExpand.val()) == CtxPortCode::RightsExpand.detail()
            && detail(0xDEAD) == CtxPortCode::Unknown.detail(),
        String::from("命中+兜底"),
    ));
    v
}

/// VX9 检查入口（卡 12612，UNX-F12613-J1 判据承载）。
pub fn run_unx12612_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12612 ALPC 上下文端口与直接/间接消息");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 测试区（#[cfg(test)] 内允许 assert!/assert_eq!） ────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_golden_run_green() {
        assert!(golden_run().is_ok());
    }

    #[test]
    fn test_dup_semantics() {
        assert!(dup_semantics_ok());
    }

    #[test]
    fn test_rights_shrink_only() {
        assert!(rights_shrink_ok());
    }

    #[test]
    fn test_bad_handle_injection_all_caught() {
        let mut led = InjectLedger::new();
        inject_bad_handle(&mut led);
        assert_eq!(led.tries, 10);
        assert_eq!(led.caught, 10);
        assert!(led.all_caught());
    }

    #[test]
    fn test_sample_ten_agree() {
        assert_eq!(sample_ten(), Ok(10));
    }

    #[test]
    fn test_repro_dual_run_equal() {
        assert!(repro_check().is_ok());
        let fa = golden_run().unwrap_or(0);
        let fb = golden_run().unwrap_or(1);
        assert_eq!(fa, fb);
    }

    #[test]
    fn test_slot_full_boundary() {
        // 种 1 源占 1 槽；连收 16 次：前 15 次落复制成功，第 16 次满账拒。
        let mut p = CtxPort::new();
        assert!(p.seed_source(0x6000, RIGHT_ALL).is_ok());
        let mut ok = 0;
        let mut k = 0;
        while k < HANDLE_SLOTS {
            let m = AlpcMessage::direct(4).with_handle(0x6000);
            if p.receive(&m, RIGHT_READ).is_ok() {
                ok += 1;
            }
            k += 1;
        }
        assert_eq!(ok, HANDLE_SLOTS as u32 - 1); // 首槽被种源占用
        // 16 槽全占后：种源失败（SlotFull），带未登记句柄收信为 DupMissing。
        let src = 0x6100u32;
        assert_eq!(p.seed_source(src, RIGHT_ALL), Err(CtxPortCode::SlotFull.val()));
        let m = AlpcMessage::direct(4).with_handle(src);
        assert_eq!(p.receive(&m, RIGHT_READ), Err(CtxPortCode::DupMissing.val()));
    }

    #[test]
    fn test_state_regress_rejected() {
        let mut p = CtxPort::new();
        assert!(p.seed_source(0x6200, RIGHT_ALL).is_ok());
        let m = AlpcMessage::direct(4).with_handle(0x6200);
        let dst = p.receive(&m, RIGHT_READ).unwrap_or(0);
        assert!(p.revoke(dst).is_ok());
        // 吊销后（终态）再吊销 = 倒退，必拒。
        assert_eq!(p.revoke(dst), Err(CtxPortCode::StateRegress.val()));
    }

    #[test]
    fn test_segment_guard() {
        assert!(segment_guard());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0x80300);
    }

    #[test]
    fn test_detail_closed_set() {
        for c in CtxPortCode::ALL.iter() {
            assert_eq!(detail(c.val()), c.detail());
        }
        assert!(detail(0xDEAD).contains("未知"));
    }
}
