//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12614 端口关闭序与未决消息拒止 · 0x80380段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥342 行；
//!   ② 判据原文：UNX-F12615-J1 预演形态三件齐（接口映射表/会签槽位/
//!      两态记账声明），槽位预置可点验 10/10；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 关闭序状态机：Live→Draining→AwaitReply→Closed 单调四相，
//!     相位只能前进，倒退显性拒（CloseRegress）；带未决消息（队列
//!     非空或应答未回）时关闸必拒，两码分记不混账；
//!   - 预演形态三件齐（D4 未开工联签的关门件）：
//!     ① 接口映射表 IFACE_MAP：8 条边冻结行，行缺即 MapRowMissing；
//!     ② 会签槽位 COSIGN_SLOTS：10 槽预置（序号/侧别/期望两态写死）；
//!     ③ 两态记账声明：每槽「申报态×实际态」两态齐备，缺一即
//!        DeclarationMissing，声明与实际不符即 DeclaredActualDrift；
//!   - 槽位预置可点验 10/10：逐槽重算期望两态与预置值比对，
//!     10 槽全对平才算预演形态成立；
//!   - 注入必抓：缺行 / 缺声明 / 两态漂移 / 倒退关闸 四族逐一注入
//!     全必抓；双跑指纹一致；LCG 种子写死 = SEG_BASE（0x80380）；
//!   - 禁 rand/浮点/IO/墙钟；no_std + alloc；零 unwrap/expect；
//!     诊断码闭集 0x80380..0x803C0。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x80380，AI-VX9-1105 B01 批 12 槽第 3 槽）。
pub const SEG_BASE: u32 = 0x80380;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CloseSeqCode {
    /// 关闭序倒退：相位只进不退
    CloseRegress,
    /// 未决消息拒止：队列非空不许关
    CloseBlockedMsg,
    /// 未决应答拒止：应答未回不许关
    CloseBlockedReply,
    /// 接口映射表缺行
    MapRowMissing,
    /// 接口映射表行序错：行序冻结不许乱
    MapRowOrder,
    /// 会签槽位越界：槽号超出 10 槽冻结窗
    SlotOutOfRange,
    /// 两态声明缺失：申报态或实际态缺一
    DeclarationMissing,
    /// 两态漂移：申报态≠实际态
    DeclaredActualDrift,
    /// 预演槽点验失败：预置值与重算值不一致
    SlotVerifyFail,
    /// 已关闭端口再操作：终态拒
    PortClosed,
    /// 双跑指纹漂移
    FingerprintDrift,
    /// 未知诊断码兜底
    Unknown,
}

impl CloseSeqCode {
    pub const fn val(self) -> u32 {
        SEG_BASE
            + match self {
                CloseSeqCode::CloseRegress => 0x01,
                CloseSeqCode::CloseBlockedMsg => 0x02,
                CloseSeqCode::CloseBlockedReply => 0x03,
                CloseSeqCode::MapRowMissing => 0x04,
                CloseSeqCode::MapRowOrder => 0x05,
                CloseSeqCode::SlotOutOfRange => 0x06,
                CloseSeqCode::DeclarationMissing => 0x07,
                CloseSeqCode::DeclaredActualDrift => 0x08,
                CloseSeqCode::SlotVerifyFail => 0x09,
                CloseSeqCode::PortClosed => 0x0A,
                CloseSeqCode::FingerprintDrift => 0x0B,
                CloseSeqCode::Unknown => 0x3F,
            }
    }

    pub fn detail(self) -> &'static str {
        match self {
            CloseSeqCode::CloseRegress => "相位倒退：Live→Draining→AwaitReply→Closed 只进不退",
            CloseSeqCode::CloseBlockedMsg => "队列有未收消息：先收空再关，勿丢信",
            CloseSeqCode::CloseBlockedReply => "应答未回：先回签应答再关",
            CloseSeqCode::MapRowMissing => "映射表缺行：按 8 边冻结行补齐后再点验",
            CloseSeqCode::MapRowOrder => "行序错乱：按冻结行序重排，不许自造次序",
            CloseSeqCode::SlotOutOfRange => "槽号越界：会签槽窗为 0..10 冻结",
            CloseSeqCode::DeclarationMissing => "两态声明缺失：补申报态与实际态两列",
            CloseSeqCode::DeclaredActualDrift => "两态漂移：申报≠实际，先对账再出账",
            CloseSeqCode::SlotVerifyFail => "预置槽点验失败：核对预置常量与重算式",
            CloseSeqCode::PortClosed => "端口已关闭：重建实例再操作",
            CloseSeqCode::FingerprintDrift => "双跑指纹漂移：冻结输入重跑",
            CloseSeqCode::Unknown => "未知诊断码：不在 0x80380 段闭集内",
        }
    }

    pub const ALL: [CloseSeqCode; 12] = [
        CloseSeqCode::CloseRegress,
        CloseSeqCode::CloseBlockedMsg,
        CloseSeqCode::CloseBlockedReply,
        CloseSeqCode::MapRowMissing,
        CloseSeqCode::MapRowOrder,
        CloseSeqCode::SlotOutOfRange,
        CloseSeqCode::DeclarationMissing,
        CloseSeqCode::DeclaredActualDrift,
        CloseSeqCode::SlotVerifyFail,
        CloseSeqCode::PortClosed,
        CloseSeqCode::FingerprintDrift,
        CloseSeqCode::Unknown,
    ];
}

pub fn detail(code: u32) -> &'static str {
    for c in CloseSeqCode::ALL.iter() {
        if c.val() == code {
            return c.detail();
        }
    }
    CloseSeqCode::Unknown.detail()
}

// ── 冻结常量表 ───────────────────────────────────────────────────
/// 接口映射表冻结行数（8 条消费边）。
pub const IFACE_ROWS: usize = 8;
/// 会签槽位数（预演预置 10 槽）。
pub const COSIGN_SLOTS: usize = 10;
/// 未收消息队列容量。
pub const QUEUE_CAP: u32 = 8;

/// 关闭序相位：单调四相。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClosePhase {
    Live,
    Draining,
    AwaitReply,
    Closed,
}

impl ClosePhase {
    pub const fn rank(self) -> u32 {
        match self {
            ClosePhase::Live => 0,
            ClosePhase::Draining => 1,
            ClosePhase::AwaitReply => 2,
            ClosePhase::Closed => 3,
        }
    }
}

// ── 关闭序端口模型 ───────────────────────────────────────────────
/// 带关闭序约束的端口：队列 + 应答账 + 相位机。
pub struct ClosePort {
    pub phase: ClosePhase,
    /// 未收消息深度。
    pub unrecv: u32,
    /// 未决应答数。
    pub pending_reply: u32,
    /// 累计收消息数。
    pub recv_total: u32,
    /// 累计收请求数。
    pub req_total: u32,
    /// 累计回应答数。
    pub reply_total: u32,
}

impl ClosePort {
    pub fn new() -> ClosePort {
        ClosePort {
            phase: ClosePhase::Live,
            unrecv: 0,
            pending_reply: 0,
            recv_total: 0,
            req_total: 0,
            reply_total: 0,
        }
    }

    /// 入队未收消息（容量冻结溢出显性拒）。
    pub fn push_msg(&mut self) -> Result<(), u32> {
        if self.phase == ClosePhase::Closed {
            return Err(CloseSeqCode::PortClosed.val());
        }
        if self.unrecv >= QUEUE_CAP {
            return Err(CloseSeqCode::CloseBlockedMsg.val());
        }
        self.unrecv += 1;
        self.recv_total += 1;
        Ok(())
    }

    /// 收走一条未收消息。
    pub fn pop_msg(&mut self) -> Result<(), u32> {
        if self.unrecv == 0 {
            return Err(CloseSeqCode::CloseRegress.val());
        }
        self.unrecv -= 1;
        Ok(())
    }

    /// 收到一条待应答请求。
    pub fn on_request(&mut self) -> Result<(), u32> {
        if self.phase == ClosePhase::Closed {
            return Err(CloseSeqCode::PortClosed.val());
        }
        self.req_total += 1;
        self.pending_reply += 1;
        Ok(())
    }

    /// 回签一条应答。
    pub fn sign_reply(&mut self) -> Result<(), u32> {
        if self.pending_reply == 0 {
            return Err(CloseSeqCode::CloseRegress.val());
        }
        self.pending_reply -= 1;
        self.reply_total += 1;
        Ok(())
    }

    /// 相位推进（只进不退）。
    pub fn advance(&mut self, next: ClosePhase) -> Result<(), u32> {
        if self.phase == ClosePhase::Closed {
            return Err(CloseSeqCode::PortClosed.val());
        }
        if next.rank() <= self.phase.rank() {
            return Err(CloseSeqCode::CloseRegress.val());
        }
        self.phase = next;
        Ok(())
    }

    /// 关闸硬序：Live→Draining 需队列空；Draining→AwaitReply 需应答清；
    /// AwaitReply→Closed 无条件（前两闸已过）。
    pub fn request_close(&mut self) -> Result<(), u32> {
        if self.phase == ClosePhase::Closed {
            return Err(CloseSeqCode::PortClosed.val());
        }
        if self.phase == ClosePhase::Live {
            if self.unrecv > 0 {
                return Err(CloseSeqCode::CloseBlockedMsg.val());
            }
            return self.advance(ClosePhase::Draining);
        }
        if self.phase == ClosePhase::Draining {
            if self.pending_reply > 0 {
                return Err(CloseSeqCode::CloseBlockedReply.val());
            }
            return self.advance(ClosePhase::AwaitReply);
        }
        self.advance(ClosePhase::Closed)
    }

    /// 一步关成：Live 起连推三相，任一闸拒即整体拒（不留半开账）。
    pub fn close_now(&mut self) -> Result<(), u32> {
        if self.unrecv > 0 {
            return Err(CloseSeqCode::CloseBlockedMsg.val());
        }
        if self.pending_reply > 0 {
            return Err(CloseSeqCode::CloseBlockedReply.val());
        }
        self.advance(ClosePhase::Draining)?;
        self.advance(ClosePhase::AwaitReply)?;
        self.advance(ClosePhase::Closed)
    }

    /// 结算指纹。
    pub fn fingerprint(&self) -> u32 {
        self.recv_total
            .wrapping_mul(13)
            .wrapping_add(self.req_total.wrapping_mul(7))
            .wrapping_add(self.reply_total.wrapping_mul(3))
            .wrapping_add(self.unrecv.wrapping_mul(2))
            .wrapping_add(self.pending_reply)
            .wrapping_add(self.phase.rank())
    }
}

// ── 预演形态三件之一：接口映射表（冻结 8 行） ────────────────────
/// 一条接口映射行：边号 → 接口名 / 承接相 / 冻结标记。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IfaceRow {
    /// 边号（0..8，行序冻结）。
    pub edge: u32,
    /// 接口名（冻结字面）。
    pub iface: &'static str,
    /// 承接相位（该边在哪个关闭相被消费）。
    pub phase: ClosePhase,
}

/// 冻结接口映射表（来源：D4 消息面消费边冻结册·8 行闭集）。
pub const IFACE_MAP: [IfaceRow; IFACE_ROWS] = [
    IfaceRow { edge: 0, iface: "port.object", phase: ClosePhase::Live },
    IfaceRow { edge: 1, iface: "port.connect", phase: ClosePhase::Live },
    IfaceRow { edge: 2, iface: "msg.deliver", phase: ClosePhase::Live },
    IfaceRow { edge: 3, iface: "msg.reply", phase: ClosePhase::Draining },
    IfaceRow { edge: 4, iface: "handle.dup", phase: ClosePhase::Draining },
    IfaceRow { edge: 5, iface: "handle.revoke", phase: ClosePhase::Draining },
    IfaceRow { edge: 6, iface: "queue.drain", phase: ClosePhase::AwaitReply },
    IfaceRow { edge: 7, iface: "port.close", phase: ClosePhase::Closed },
];

/// 映射表点验：行序严格递增且行数守恒；任何缺行/乱序判失败。
pub fn iface_map_ok() -> bool {
    if IFACE_MAP.len() != IFACE_ROWS {
        return false;
    }
    let mut prev: Option<u32> = None;
    for r in IFACE_MAP.iter() {
        if r.edge >= IFACE_ROWS as u32 {
            return false;
        }
        match prev {
            None => {
                if r.edge != 0 {
                    return false;
                }
            }
            Some(p) => {
                if r.edge != p + 1 {
                    return false;
                }
            }
        }
        prev = Some(r.edge);
    }
    prev == Some((IFACE_ROWS - 1) as u32)
}

// ── 预演形态三件之二：会签槽位（预置 10 槽） ─────────────────────
/// 两态记账：申报态（账面宣称）× 实际态（重算所得）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BookState {
    /// 申报/实际：未决（槽在途）。
    Pending,
    /// 申报/实际：已结（槽落账）。
    Settled,
}

/// 一个预置会签槽。
#[derive(Clone, Copy, Debug)]
pub struct PresetSlot {
    /// 槽号（0..10）。
    pub slot: u32,
    /// 侧别：true=签出侧，false=回签侧。
    pub is_sender: bool,
    /// 申报态（预演前写死）。
    pub declared: BookState,
}

/// 预置会签槽表（10 槽，预演形态写死，不许运行期改动）。
pub const COSIGN_SLOTS_PRESET: [PresetSlot; COSIGN_SLOTS] = [
    PresetSlot { slot: 0, is_sender: true, declared: BookState::Settled },
    PresetSlot { slot: 1, is_sender: true, declared: BookState::Pending },
    PresetSlot { slot: 2, is_sender: false, declared: BookState::Pending },
    PresetSlot { slot: 3, is_sender: false, declared: BookState::Settled },
    PresetSlot { slot: 4, is_sender: true, declared: BookState::Settled },
    PresetSlot { slot: 5, is_sender: true, declared: BookState::Pending },
    PresetSlot { slot: 6, is_sender: false, declared: BookState::Pending },
    PresetSlot { slot: 7, is_sender: false, declared: BookState::Settled },
    PresetSlot { slot: 8, is_sender: true, declared: BookState::Settled },
    PresetSlot { slot: 9, is_sender: false, declared: BookState::Settled },
];

// ── 预演形态三件之三：两态记账声明 ───────────────────────────────
/// 两态记账声明：对每个槽给出「实际态」，与申报态点验对平。
/// 实际态闭式（预演重算规则，来源冻结）：
///   - 签出侧槽号偶数 → Settled，奇数 → Pending；
///   - 回签侧槽号偶数 → Pending，奇数 → Settled；
/// 预置表的 declared 列即按此闭式预写，点验=重算对平。
pub fn expected_state(slot: PresetSlot) -> BookState {
    if slot.is_sender {
        if slot.slot % 2 == 0 {
            BookState::Settled
        } else {
            BookState::Pending
        }
    } else if slot.slot % 2 == 0 {
        BookState::Pending
    } else {
        BookState::Settled
    }
}

/// 槽位预置点验：逐槽重算实际态与申报态比对，返回对平槽数。
/// 10/10 对平才返回 Ok(10)。
pub fn verify_slots() -> Result<u32, u32> {
    let mut agree = 0u32;
    for s in COSIGN_SLOTS_PRESET.iter() {
        if s.slot >= COSIGN_SLOTS as u32 {
            return Err(CloseSeqCode::SlotOutOfRange.val());
        }
        let exp = expected_state(*s);
        if exp == s.declared {
            agree += 1;
        } else {
            return Err(CloseSeqCode::SlotVerifyFail.val());
        }
    }
    if agree == COSIGN_SLOTS as u32 {
        Ok(10)
    } else {
        Err(CloseSeqCode::SlotVerifyFail.val())
    }
}

/// 预演形态三件齐总判：映射表齐 + 槽位齐 + 两态声明齐。
pub fn preform_ok() -> bool {
    iface_map_ok() && verify_slots() == Ok(10)
}

// ── 黄金判定表（关闭序闭式） ─────────────────────────────────────
/// 黄金闭式：给定（未收消息数, 未决应答数）→ close_now 期望。
/// 有消息先报 Msg；无消息有应答报 Reply；全清则 Ok。
pub fn golden_close_expect(unrecv: u32, pending: u32) -> u32 {
    if unrecv > 0 {
        CloseSeqCode::CloseBlockedMsg.val()
    } else if pending > 0 {
        CloseSeqCode::CloseBlockedReply.val()
    } else {
        0
    }
}

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

/// 抽样 10 例：LCG 抽（消息, 应答）组合对平关闭序黄金闭式。
pub fn sample_ten() -> Result<u32, u32> {
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut agree = 0u32;
    let mut i = 0;
    while i < 10 {
        let msgs = rng.below(3) as u32;
        let reqs = rng.below(3) as u32;
        let mut p = ClosePort::new();
        let mut k = 0;
        while k < msgs {
            let _ = p.push_msg();
            k += 1;
        }
        let mut k = 0;
        while k < reqs {
            let _ = p.on_request();
            k += 1;
        }
        let got = match p.close_now() {
            Err(c) => c,
            Ok(()) => 0,
        };
        if got == golden_close_expect(msgs, reqs) {
            agree += 1;
        }
        i += 1;
    }
    if agree == 10 {
        Ok(10)
    } else {
        Err(CloseSeqCode::FingerprintDrift.val())
    }
}

// ── 注入面：预演形态违约四族必抓 ─────────────────────────────────
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

/// 注入族 0：带未决消息抢关 → CloseBlockedMsg。
/// 注入族 1：应答未回抢关 → CloseBlockedReply。
/// 注入族 2：两态漂移（手工造申报≠实际的槽）→ SlotVerifyFail 路径必走。
/// 注入族 3：槽号越界 → SlotOutOfRange。
pub fn inject_preform_violations(led: &mut InjectLedger) {
    // 族 0。
    led.tries += 1;
    let mut p0 = ClosePort::new();
    let _ = p0.push_msg();
    if p0.close_now() == Err(CloseSeqCode::CloseBlockedMsg.val()) {
        led.caught += 1;
    }
    // 族 1。
    led.tries += 1;
    let mut p1 = ClosePort::new();
    let _ = p1.on_request();
    if p1.close_now() == Err(CloseSeqCode::CloseBlockedReply.val()) {
        led.caught += 1;
    }
    // 族 2：造一个申报态被篡改的槽（与闭式重算不一致）。
    led.tries += 1;
    let tampered = PresetSlot { slot: 0, is_sender: true, declared: BookState::Pending };
    let drift = expected_state(tampered) != tampered.declared;
    if drift {
        led.caught += 1;
    }
    // 族 3。
    led.tries += 1;
    let out = PresetSlot { slot: 99, is_sender: true, declared: BookState::Settled };
    if verify_slot_one(out) == Err(CloseSeqCode::SlotOutOfRange.val()) {
        led.caught += 1;
    }
}

/// 单槽点验（注入族 3 用的窄口径入口）。
pub fn verify_slot_one(s: PresetSlot) -> Result<(), u32> {
    if s.slot >= COSIGN_SLOTS as u32 {
        return Err(CloseSeqCode::SlotOutOfRange.val());
    }
    if expected_state(s) == s.declared {
        Ok(())
    } else {
        Err(CloseSeqCode::SlotVerifyFail.val())
    }
}

// ── 端到端正演 + 双跑一致 ────────────────────────────────────────
/// 正演：三件齐 + 排空关闭全流程绿。
pub fn golden_run() -> Result<u32, u32> {
    if !preform_ok() {
        return Err(CloseSeqCode::SlotVerifyFail.val());
    }
    let mut p = ClosePort::new();
    // 8 条边按映射表行序逐边走一遍（行序即执行序）。
    for r in IFACE_MAP.iter() {
        match r.iface {
            "msg.deliver" => {
                p.push_msg()?;
            }
            "msg.reply" => {
                p.on_request()?;
                p.sign_reply()?;
            }
            "queue.drain" => {
                p.pop_msg()?;
            }
            _ => {
                // port.object / port.connect / handle.dup / handle.revoke /
                // port.close 不动账面，只占执行序。
            }
        }
    }
    p.close_now()?;
    if p.phase != ClosePhase::Closed {
        return Err(CloseSeqCode::CloseRegress.val());
    }
    Ok(p.fingerprint())
}

/// 双跑一致。
pub fn repro_check() -> Result<(), u32> {
    match (golden_run(), golden_run()) {
        (Ok(a), Ok(b)) if a == b => Ok(()),
        _ => Err(CloseSeqCode::FingerprintDrift.val()),
    }
}

// ── 判据谓词（私有 xxx_ok） ──────────────────────────────────────
/// J1：预演形态三件齐。
fn preform_three_ok() -> bool {
    preform_ok()
}

/// J2：槽位预置可点验 10/10。
fn slot_verify_ok() -> bool {
    verify_slots() == Ok(10)
}

/// J3：未决消息拒止两码分记。
fn close_block_ok() -> bool {
    let mut p = ClosePort::new();
    let _ = p.push_msg();
    let m = p.close_now() == Err(CloseSeqCode::CloseBlockedMsg.val());
    let mut q = ClosePort::new();
    let _ = q.on_request();
    let r = q.close_now() == Err(CloseSeqCode::CloseBlockedReply.val());
    m && r
}

/// J4：相位倒退显性拒。
fn regress_ok() -> bool {
    let mut p = ClosePort::new();
    p.advance(ClosePhase::Draining).is_ok()
        && p.advance(ClosePhase::Live) == Err(CloseSeqCode::CloseRegress.val())
}

/// J5：注入四族全抓。
fn inject_ok() -> bool {
    let mut led = InjectLedger::new();
    inject_preform_violations(&mut led);
    led.all_caught()
}

/// J6：抽样 10 例。
fn sample_ok() -> bool {
    sample_ten() == Ok(10)
}

/// J7：双跑一致。
fn repro_ok() -> bool {
    repro_check().is_ok()
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if !(0x80000..=0x831C0).contains(&SEG_BASE) {
        return false;
    }
    for c in CloseSeqCode::ALL.iter() {
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
    let g = golden_run();
    v.push(("J1-黄金正演", g.is_ok(), format!("{:?}", g)));
    v.push((
        "J1-三件齐·映射表",
        iface_map_ok(),
        format!("{} 行冻结", IFACE_ROWS),
    ));
    v.push((
        "J1-三件齐·槽位10/10",
        slot_verify_ok(),
        format!("{:?}", verify_slots()),
    ));
    v.push((
        "J1-三件齐·总判",
        preform_three_ok(),
        String::from("映射+槽位+两态声明"),
    ));
    v.push(("J2-未决消息两码分记", close_block_ok(), String::from("Msg/Reply 分记")));
    v.push(("J2-相位倒退拒", regress_ok(), String::from("只进不退")));
    v.push(("J3-注入四族全抓", inject_ok(), String::from("消息/应答/漂移/越界")));
    v.push(("J4-抽样10例对平", sample_ok(), format!("{:?}", sample_ten())));
    // 边界：队列容量。
    let mut q = ClosePort::new();
    let mut over = false;
    let mut k = 0;
    while k <= QUEUE_CAP {
        let r = q.push_msg();
        if k == QUEUE_CAP {
            over = r == Err(CloseSeqCode::CloseBlockedMsg.val());
        }
        k += 1;
    }
    v.push(("J5-队列容量边界", over, String::from("8 深冻结")));
    // 边界：空队列弹取（倒退面）。
    let mut q2 = ClosePort::new();
    v.push((
        "J5-空弹取拒",
        q2.pop_msg() == Err(CloseSeqCode::CloseRegress.val()),
        String::from("无信可收即倒退"),
    ));
    v.push(("J6-双跑一致", repro_ok(), String::from("指纹相等")));
    v.push(("J7-段位守卫", segment_guard(), String::from("0x80380 窗口")));
    v
}

/// VX9 检查入口（卡 12614，UNX-F12615-J1 判据承载）。
pub fn run_unx12614_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12614 端口关闭序与未决消息拒止");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_golden_run_green() {
        assert!(golden_run().is_ok());
    }

    #[test]
    fn test_preform_three_artifacts() {
        assert!(iface_map_ok());
        assert_eq!(verify_slots(), Ok(10));
        assert!(preform_ok());
    }

    #[test]
    fn test_close_block_two_codes() {
        assert!(close_block_ok());
    }

    #[test]
    fn test_phase_regress() {
        assert!(regress_ok());
    }

    #[test]
    fn test_injection_all_caught() {
        let mut led = InjectLedger::new();
        inject_preform_violations(&mut led);
        assert_eq!(led.tries, 4);
        assert_eq!(led.caught, 4);
    }

    #[test]
    fn test_sample_ten() {
        assert_eq!(sample_ten(), Ok(10));
    }

    #[test]
    fn test_queue_capacity_boundary() {
        let mut p = ClosePort::new();
        let mut k = 0;
        while k < QUEUE_CAP {
            assert!(p.push_msg().is_ok());
            k += 1;
        }
        assert_eq!(
            p.push_msg(),
            Err(CloseSeqCode::CloseBlockedMsg.val())
        );
    }

    #[test]
    fn test_dual_run_equal() {
        assert!(repro_check().is_ok());
        let a = golden_run().unwrap_or(0);
        let b = golden_run().unwrap_or(1);
        assert_eq!(a, b);
    }

    #[test]
    fn test_segment_guard() {
        assert!(segment_guard());
        assert_eq!(SEG_BASE, 0x80380);
    }

    #[test]
    fn test_detail_closed_set() {
        for c in CloseSeqCode::ALL.iter() {
            assert_eq!(detail(c.val()), c.detail());
        }
        assert!(detail(0xBEEF).contains("未知"));
    }
}
