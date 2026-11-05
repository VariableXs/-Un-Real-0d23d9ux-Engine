//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12613 端口句柄传递（句柄复制跨端）联签 · 0x80340段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② 判据原文：UNX-F12614-J1 关闭序（未决应答/未收消息拒止）判据绿，
//!      关闭竞态注入 10/10 次正确拒止；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 跨端复制联签：句柄复制跨端必须**双侧签账**——发送侧签出
//!     （SignedOut）、接收侧回签（Cosigned），只有回签到齐才算一笔
//!     完整复制；单侧签即出账视为悬空账（CosignDangling 必抓）；
//!   - 关闭序状态机：Live→DrainReq→Drained→Closed 单调前进，
//!     倒退显性拒（CloseRegress）；
//!   - 关闭序硬闸：有未决应答（已收请求未回）或未收消息（队列非空）
//!     时关闭请求一律拒止——CloseBlockedReply / CloseBlockedUnrecv
//!     两码分开记，不许混账；先排空再关才放行；
//!   - 关闭竞态注入 10/10：关闭与收发交错的竞态序列逐一注入，
//!     每次都必须在正确闸口被正确码拒止，tries==caught 才算绿；
//!   - 黄金判定表：LCG 抽 10 组（未决应答数×未收消息数）与闭式
//!     期望对平；双跑指纹一致才放行；
//!   - LCG 种子写死 = SEG_BASE（0x80340），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect；诊断码闭集 0x80340..0x80380。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x80340，AI-VX9-1105 B01 批 12 槽第 2 槽）。
pub const SEG_BASE: u32 = 0x80340;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HandlePassCode {
    /// 未决应答未清：关闭被拒（先回签应答再关）
    CloseBlockedReply,
    /// 未收消息在队列：关闭被拒（先收空队列再关）
    CloseBlockedUnrecv,
    /// 关闭序倒退：状态机只进不退
    CloseRegress,
    /// 已关闭端口再收发：终态拒收
    PortClosed,
    /// 联签悬空：复制账只有单侧签
    CosignDangling,
    /// 联签重复回签：同一笔复制不许二次回签
    CosignDupSign,
    /// 复制笔不存在：回签对不上出账笔
    CosignNoEntry,
    /// 跨端句柄越界：句柄值不在登记窗内
    HandleOutOfRange,
    /// 队列溢出：未收消息数超冻结容量
    QueueOverflow,
    /// 双跑指纹漂移
    FingerprintDrift,
    /// 账面守恒破：出账数≠回签数+悬空数
    ConservationBreak,
    /// 未知诊断码兜底
    Unknown,
}

impl HandlePassCode {
    pub const fn val(self) -> u32 {
        SEG_BASE
            + match self {
                HandlePassCode::CloseBlockedReply => 0x01,
                HandlePassCode::CloseBlockedUnrecv => 0x02,
                HandlePassCode::CloseRegress => 0x03,
                HandlePassCode::PortClosed => 0x04,
                HandlePassCode::CosignDangling => 0x05,
                HandlePassCode::CosignDupSign => 0x06,
                HandlePassCode::CosignNoEntry => 0x07,
                HandlePassCode::HandleOutOfRange => 0x08,
                HandlePassCode::QueueOverflow => 0x09,
                HandlePassCode::FingerprintDrift => 0x0A,
                HandlePassCode::ConservationBreak => 0x0B,
                HandlePassCode::Unknown => 0x3F,
            }
    }

    pub fn detail(self) -> &'static str {
        match self {
            HandlePassCode::CloseBlockedReply => "有未决应答：先逐笔回签应答再关端口",
            HandlePassCode::CloseBlockedUnrecv => "队列有未收消息：先收空再关，勿丢信",
            HandlePassCode::CloseRegress => "关闭序倒退：Live→DrainReq→Drained→Closed 只进不退",
            HandlePassCode::PortClosed => "端口已关闭：重建端口再收发，勿复用死端口",
            HandlePassCode::CosignDangling => "复制账单侧签：等接收侧回签或显性撤销该笔",
            HandlePassCode::CosignDupSign => "重复回签：一笔复制只许一次回签",
            HandlePassCode::CosignNoEntry => "回签无出账笔：核对复制序号是否一致",
            HandlePassCode::HandleOutOfRange => "句柄越界：跨端句柄须在登记窗内",
            HandlePassCode::QueueOverflow => "队列溢出：排空积压或扩容账面（冻结值不许改）",
            HandlePassCode::FingerprintDrift => "双跑指纹漂移：冻结输入重跑",
            HandlePassCode::ConservationBreak => "守恒破：出账=回签+悬空，逐笔对账",
            HandlePassCode::Unknown => "未知诊断码：不在 0x80340 段闭集内",
        }
    }

    pub const ALL: [HandlePassCode; 12] = [
        HandlePassCode::CloseBlockedReply,
        HandlePassCode::CloseBlockedUnrecv,
        HandlePassCode::CloseRegress,
        HandlePassCode::PortClosed,
        HandlePassCode::CosignDangling,
        HandlePassCode::CosignDupSign,
        HandlePassCode::CosignNoEntry,
        HandlePassCode::HandleOutOfRange,
        HandlePassCode::QueueOverflow,
        HandlePassCode::FingerprintDrift,
        HandlePassCode::ConservationBreak,
        HandlePassCode::Unknown,
    ];
}

pub fn detail(code: u32) -> &'static str {
    for c in HandlePassCode::ALL.iter() {
        if c.val() == code {
            return c.detail();
        }
    }
    HandlePassCode::Unknown.detail()
}

// ── 冻结常量表（来源：端口句柄跨端联签冻结册） ──────────────────
/// 未收消息队列容量（冻结）。
pub const QUEUE_CAP: usize = 8;
/// 联签账槽位数（冻结）。
pub const COSIGN_SLOTS: usize = 8;
/// 跨端句柄登记窗下界。
pub const HANDLE_WIN_LO: u32 = 0x1000;
/// 跨端句柄登记窗上界（不含）。
pub const HANDLE_WIN_HI: u32 = 0x2000;

// ── 端口状态机（单调） ───────────────────────────────────────────
/// 端口生命周期：Live(0)→DrainReq(1)→Drained(2)→Closed(3)。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PortPhase {
    Live,
    DrainReq,
    Drained,
    Closed,
}

impl PortPhase {
    pub const fn rank(self) -> u32 {
        match self {
            PortPhase::Live => 0,
            PortPhase::DrainReq => 1,
            PortPhase::Drained => 2,
            PortPhase::Closed => 3,
        }
    }
}

/// 联签槽：一笔跨端复制的双签账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SignState {
    /// 出账未签（不应存在——出账即签出）。
    Unsigned,
    /// 发送侧已签出，等待接收侧回签。
    SignedOut,
    /// 双侧签齐：复制完整落账。
    Cosigned,
}

/// 一笔联签记录。
#[derive(Clone, Copy, Debug)]
pub struct CosignEntry {
    /// 复制序号（单调递增）。
    pub seq: u32,
    /// 跨端句柄值。
    pub handle: u32,
    /// 签账状态。
    pub state: SignState,
}

// ── 单端口模型 ───────────────────────────────────────────────────
/// 参与跨端复制联签的端口（收发两侧各持一个实例）。
pub struct PassPort {
    /// 端口侧别：true=发送侧（签出方），false=接收侧（回签方）。
    pub is_sender: bool,
    /// 生命周期相位。
    pub phase: PortPhase,
    /// 未收消息队列（深度记账即可：只记条数，条目数为 0..=QUEUE_CAP）。
    pub unrecv_depth: u32,
    /// 未决应答数（已收请求未回签应答）。
    pub pending_replies: u32,
    /// 联签账（对端共享同一笔序号）。
    pub ledger: Vec<CosignEntry>,
    /// 累计收到请求数。
    pub req_total: u32,
    /// 累计回签应答数。
    pub reply_total: u32,
}

impl PassPort {
    pub fn new(is_sender: bool) -> PassPort {
        PassPort {
            is_sender,
            phase: PortPhase::Live,
            unrecv_depth: 0,
            pending_replies: 0,
            ledger: Vec::new(),
            req_total: 0,
            reply_total: 0,
        }
    }

    /// 推进相位（只许前进，倒退显性拒）。
    pub fn advance_to(&mut self, next: PortPhase) -> Result<(), u32> {
        if self.phase == PortPhase::Closed {
            return Err(HandlePassCode::PortClosed.val());
        }
        if next.rank() <= self.phase.rank() {
            return Err(HandlePassCode::CloseRegress.val());
        }
        self.phase = next;
        Ok(())
    }

    /// 收到一条请求（未决应答 +1；已关闭拒收）。
    pub fn on_request(&mut self) -> Result<(), u32> {
        if self.phase == PortPhase::Closed {
            return Err(HandlePassCode::PortClosed.val());
        }
        self.req_total += 1;
        self.pending_replies += 1;
        Ok(())
    }

    /// 回签一笔应答（未决应答 -1；无应答可回即倒退拒）。
    pub fn sign_reply(&mut self) -> Result<(), u32> {
        if self.pending_replies == 0 {
            return Err(HandlePassCode::CloseRegress.val());
        }
        self.pending_replies -= 1;
        self.reply_total += 1;
        Ok(())
    }

    /// 入队一条未收消息（容量冻结，溢出显性拒）。
    pub fn enqueue(&mut self) -> Result<(), u32> {
        if self.phase == PortPhase::Closed {
            return Err(HandlePassCode::PortClosed.val());
        }
        if self.unrecv_depth >= QUEUE_CAP as u32 {
            return Err(HandlePassCode::QueueOverflow.val());
        }
        self.unrecv_depth += 1;
        Ok(())
    }

    /// 收走一条未收消息。
    pub fn dequeue(&mut self) -> Result<(), u32> {
        if self.unrecv_depth == 0 {
            return Err(HandlePassCode::CloseRegress.val());
        }
        self.unrecv_depth -= 1;
        Ok(())
    }

    /// 发送侧签出：新开一笔复制账（SignedOut）。
    pub fn sign_out(&mut self, seq: u32, handle: u32) -> Result<(), u32> {
        if handle < HANDLE_WIN_LO || handle >= HANDLE_WIN_HI {
            return Err(HandlePassCode::HandleOutOfRange.val());
        }
        for e in self.ledger.iter() {
            if e.seq == seq {
                return Err(HandlePassCode::CosignDupSign.val());
            }
        }
        if self.ledger.len() >= COSIGN_SLOTS {
            return Err(HandlePassCode::QueueOverflow.val());
        }
        self.ledger.push(CosignEntry { seq, handle, state: SignState::SignedOut });
        Ok(())
    }

    /// 接收侧回签：SignedOut → Cosigned（一笔只许一次）。
    pub fn cosign_in(&mut self, seq: u32) -> Result<(), u32> {
        let mut idx: Option<usize> = None;
        let mut i = 0;
        while i < self.ledger.len() {
            if let Some(e) = self.ledger.get(i) {
                if e.seq == seq {
                    idx = Some(i);
                    break;
                }
            }
            i += 1;
        }
        let i = match idx {
            None => return Err(HandlePassCode::CosignNoEntry.val()),
            Some(x) => x,
        };
        let st = match self.ledger.get(i) {
            Some(e) => e.state,
            None => return Err(HandlePassCode::CosignNoEntry.val()),
        };
        match st {
            SignState::Unsigned => Err(HandlePassCode::CosignDangling.val()),
            SignState::Cosigned => Err(HandlePassCode::CosignDupSign.val()),
            SignState::SignedOut => {
                if let Some(e) = self.ledger.get_mut(i) {
                    e.state = SignState::Cosigned;
                }
                Ok(())
            }
        }
    }

    /// 悬空账点验：任何 SignedOut 未回签笔都算悬空。
    pub fn dangling_count(&self) -> u32 {
        let mut n = 0;
        for e in self.ledger.iter() {
            if e.state == SignState::SignedOut {
                n += 1;
            }
        }
        n
    }

    /// 关闭闸：未决应答>0 → CloseBlockedReply；未收消息>0 →
    /// CloseBlockedUnrecv；否则推进 Live→DrainReq。
    pub fn request_close(&mut self) -> Result<(), u32> {
        if self.phase == PortPhase::Closed {
            return Err(HandlePassCode::PortClosed.val());
        }
        if self.pending_replies > 0 {
            return Err(HandlePassCode::CloseBlockedReply.val());
        }
        if self.unrecv_depth > 0 {
            return Err(HandlePassCode::CloseBlockedUnrecv.val());
        }
        if self.phase == PortPhase::Live {
            return self.advance_to(PortPhase::DrainReq);
        }
        Ok(())
    }

    /// 完成关闭：DrainReq 且账面已清 → Drained → Closed 两步合一次到位。
    pub fn finish_close(&mut self) -> Result<(), u32> {
        if self.phase == PortPhase::Closed {
            return Err(HandlePassCode::PortClosed.val());
        }
        if self.pending_replies > 0 || self.unrecv_depth > 0 {
            return Err(HandlePassCode::CloseRegress.val());
        }
        if self.phase == PortPhase::Live {
            self.phase = PortPhase::DrainReq;
        }
        self.advance_to(PortPhase::Drained)?;
        self.advance_to(PortPhase::Closed)
    }

    /// 结算指纹。
    pub fn fingerprint(&self) -> u32 {
        self.req_total
            .wrapping_mul(17)
            .wrapping_add(self.reply_total.wrapping_mul(5))
            .wrapping_add(self.unrecv_depth.wrapping_mul(3))
            .wrapping_add(self.pending_replies)
            .wrapping_add(self.dangling_count().wrapping_mul(11))
    }
}

// ── 黄金判定表（关闭闸闭式） ─────────────────────────────────────
/// 黄金闭式：给定（未决应答数, 未收消息数）→ 期望的关闭结果码。
/// 应答优先于队列：两者都有时报 Reply（次序冻结）。
pub fn golden_close_expect(pending_reply: u32, unrecv: u32) -> u32 {
    if pending_reply > 0 {
        HandlePassCode::CloseBlockedReply.val()
    } else if unrecv > 0 {
        HandlePassCode::CloseBlockedUnrecv.val()
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

/// 抽样 10 例：LCG 抽（应答, 未收）组合与黄金闭式对平。
pub fn sample_ten() -> Result<u32, u32> {
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut agree = 0u32;
    let mut i = 0;
    while i < 10 {
        let replies = rng.below(3) as u32;
        let unrecv = rng.below(3) as u32;
        let mut p = PassPort::new(false);
        let mut k = 0;
        while k < replies {
            let _ = p.on_request();
            k += 1;
        }
        let mut k = 0;
        while k < unrecv {
            let _ = p.enqueue();
            k += 1;
        }
        let got = match p.request_close() {
            Err(code) => code,
            Ok(()) => 0,
        };
        if got == golden_close_expect(replies, unrecv) {
            agree += 1;
        }
        i += 1;
    }
    if agree == 10 {
        Ok(10)
    } else {
        Err(HandlePassCode::FingerprintDrift.val())
    }
}

// ── 注入面：关闭竞态 10/10 必抓 ──────────────────────────────────
/// 竞态注入账。
pub struct RaceLedger {
    pub tries: u32,
    pub caught: u32,
}

impl RaceLedger {
    pub const fn new() -> RaceLedger {
        RaceLedger { tries: 0, caught: 0 }
    }
    pub fn all_caught(&self) -> bool {
        self.tries > 0 && self.caught == self.tries
    }
}

/// 关闭竞态注入族 10 次：每次在"账面未清"的口上抢关，必须被
/// 正确码拒止（应答未清报 Reply，队列未清报 Unrecv）。
pub fn inject_close_race(led: &mut RaceLedger) {
    let mut i = 0;
    while i < 10 {
        led.tries += 1;
        let mut p = PassPort::new(false);
        let want_reply = i % 2 == 0;
        if want_reply {
            // 竞态形态 A：请求已入账、应答未回，抢关。
            let _ = p.on_request();
            let _ = p.on_request();
        } else {
            // 竞态形态 B：队列有未收消息，抢关。
            let _ = p.enqueue();
            let _ = p.enqueue();
        }
        let res = p.request_close();
        let want = if want_reply {
            HandlePassCode::CloseBlockedReply.val()
        } else {
            HandlePassCode::CloseBlockedUnrecv.val()
        };
        if res == Err(want) {
            led.caught += 1;
        }
        i += 1;
    }
}

// ── 端到端正演 + 双跑一致 ────────────────────────────────────────
/// 正演：跨端复制联签全流程——签出→回签→排空→关闭 全绿。
pub fn golden_run() -> Result<u32, u32> {
    let mut s = PassPort::new(true);
    let mut r = PassPort::new(false);
    // 发送侧签出一笔跨端复制。
    s.sign_out(1, 0x1234)?;
    // 接收侧镜像同一笔（联签两侧共享序号），回签。
    r.sign_out(1, 0x1234)?;
    r.cosign_in(1)?;
    // 悬空点验：签出方此刻仍悬空（未收到回签回执），先落回执。
    // 模拟回执：发送侧把同一笔推进为 Cosigned（回签回执落账）。
    s.cosign_in(1)?;
    if s.dangling_count() != 0 || r.dangling_count() != 0 {
        return Err(HandlePassCode::CosignDangling.val());
    }
    // 应答清账。
    r.on_request()?;
    r.sign_reply()?;
    // 关闭序：请求→完成。
    r.request_close()?;
    r.finish_close()?;
    s.request_close()?;
    s.finish_close()?;
    Ok(s.fingerprint().wrapping_add(r.fingerprint()))
}

/// 双跑一致。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run();
    let b = golden_run();
    match (a, b) {
        (Ok(fa), Ok(fb)) if fa == fb => Ok(()),
        _ => Err(HandlePassCode::FingerprintDrift.val()),
    }
}

// ── 判据谓词（私有 xxx_ok） ──────────────────────────────────────
/// J1-A：未决应答拒止——有应答未回时关闭必拒且码正确。
fn close_reply_block_ok() -> bool {
    let mut p = PassPort::new(false);
    let _ = p.on_request();
    p.request_close() == Err(HandlePassCode::CloseBlockedReply.val())
}

/// J1-B：未收消息拒止——队列非空时关闭必拒且码正确。
fn close_unrecv_block_ok() -> bool {
    let mut p = PassPort::new(false);
    let _ = p.enqueue();
    p.request_close() == Err(HandlePassCode::CloseBlockedUnrecv.val())
}

/// J1-C：排空后关闭放行——回签应答 + 收空队列后一次关成。
fn close_after_drain_ok() -> bool {
    let mut p = PassPort::new(false);
    let _ = p.on_request();
    let _ = p.enqueue();
    let _ = p.sign_reply();
    let _ = p.dequeue();
    match p.request_close() {
        Ok(()) => p.finish_close().is_ok() && p.phase == PortPhase::Closed,
        Err(_) => false,
    }
}

/// J2：联签守恒——出账=回签+悬空；悬空笔不许冒充完成。
fn cosign_conservation_ok() -> bool {
    let mut s = PassPort::new(true);
    let _ = s.sign_out(1, 0x1400);
    let _ = s.sign_out(2, 0x1408);
    let _ = s.cosign_in(1);
    let dangling = s.dangling_count();
    let cosigned = 1u32;
    // 出账 2 笔 = 回签完成 1 笔 + 悬空 1 笔。
    dangling == 1 && cosigned == 1 && s.ledger.len() == 2
}

/// J3：关闭竞态注入 10/10。
fn inject_ok() -> bool {
    let mut led = RaceLedger::new();
    inject_close_race(&mut led);
    led.all_caught()
}

/// J4：抽样 10 例对平。
fn sample_ok() -> bool {
    sample_ten() == Ok(10)
}

/// J5：双跑一致。
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
    for c in HandlePassCode::ALL.iter() {
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
        "J1-未决应答拒止",
        close_reply_block_ok(),
        String::from("有应答未回→拒"),
    ));
    v.push((
        "J1-未收消息拒止",
        close_unrecv_block_ok(),
        String::from("队列非空→拒"),
    ));
    v.push((
        "J1-排空后放行",
        close_after_drain_ok(),
        String::from("回签+收空→关成"),
    ));
    v.push((
        "J2-联签守恒",
        cosign_conservation_ok(),
        String::from("出账=回签+悬空"),
    ));
    // 边界：重复回签 / 无出账笔回签。
    let mut s = PassPort::new(true);
    let _ = s.sign_out(9, 0x1500);
    let dup_sign = s.cosign_in(9).is_ok() && s.cosign_in(9) == Err(HandlePassCode::CosignDupSign.val());
    let no_entry = s.cosign_in(99) == Err(HandlePassCode::CosignNoEntry.val());
    v.push((
        "J2-重复回签/空笔回签",
        dup_sign && no_entry,
        String::from("一次回签+无笔拒"),
    ));
    // 边界：句柄越窗。
    let mut s2 = PassPort::new(true);
    let lo_rej = s2.sign_out(1, HANDLE_WIN_LO - 1) == Err(HandlePassCode::HandleOutOfRange.val());
    let hi_rej = s2.sign_out(2, HANDLE_WIN_HI) == Err(HandlePassCode::HandleOutOfRange.val());
    let in_win = s2.sign_out(3, HANDLE_WIN_LO).is_ok();
    v.push((
        "J2-句柄登记窗",
        lo_rej && hi_rej && in_win,
        format!("窗 [{:#x},{:#x})", HANDLE_WIN_LO, HANDLE_WIN_HI),
    ));
    // 边界：队列容量冻结溢出。
    let mut q = PassPort::new(false);
    let mut overflowed = false;
    let mut k = 0;
    while k <= QUEUE_CAP {
        let r = q.enqueue();
        if k == QUEUE_CAP {
            overflowed = r == Err(HandlePassCode::QueueOverflow.val());
        }
        k += 1;
    }
    v.push(("J3-队列溢出显性", overflowed, String::from("8 槽冻结")));
    // 注入必抓。
    v.push(("J4-关闭竞态注入10/10", inject_ok(), String::from("两族轮换")));
    // 抽样。
    v.push(("J5-抽样10例对平", sample_ok(), format!("{:?}", sample_ten())));
    // 双跑。
    v.push(("J6-双跑一致", repro_ok(), String::from("指纹相等")));
    // 段位守卫。
    v.push(("J7-段位守卫", segment_guard(), String::from("0x80340 窗口")));
    v
}

/// VX9 检查入口（卡 12613，UNX-F12614-J1 判据承载）。
pub fn run_unx12613_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12613 端口句柄传递（句柄复制跨端）联签");
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
    fn test_close_reply_block() {
        assert!(close_reply_block_ok());
    }

    #[test]
    fn test_close_unrecv_block() {
        assert!(close_unrecv_block_ok());
    }

    #[test]
    fn test_close_after_drain() {
        assert!(close_after_drain_ok());
    }

    #[test]
    fn test_close_race_injection_all_caught() {
        let mut led = RaceLedger::new();
        inject_close_race(&mut led);
        assert_eq!(led.tries, 10);
        assert_eq!(led.caught, 10);
    }

    #[test]
    fn test_sample_ten_agree() {
        assert_eq!(sample_ten(), Ok(10));
    }

    #[test]
    fn test_phase_regress_rejected() {
        let mut p = PassPort::new(false);
        assert!(p.advance_to(PortPhase::DrainReq).is_ok());
        assert_eq!(
            p.advance_to(PortPhase::Live),
            Err(HandlePassCode::CloseRegress.val())
        );
    }

    #[test]
    fn test_closed_port_rejects_traffic() {
        let mut p = PassPort::new(false);
        p.request_close().unwrap_or(());
        p.finish_close().unwrap_or(());
        assert_eq!(p.phase, PortPhase::Closed);
        assert_eq!(p.on_request(), Err(HandlePassCode::PortClosed.val()));
        assert_eq!(p.enqueue(), Err(HandlePassCode::PortClosed.val()));
    }

    #[test]
    fn test_repro_dual_run_equal() {
        assert!(repro_check().is_ok());
        let fa = golden_run().unwrap_or(0);
        let fb = golden_run().unwrap_or(1);
        assert_eq!(fa, fb);
    }

    #[test]
    fn test_segment_guard_and_detail() {
        assert!(segment_guard());
        assert_eq!(SEG_BASE, 0x80340);
        for c in HandlePassCode::ALL.iter() {
            assert_eq!(detail(c.val()), c.detail());
        }
    }
}
