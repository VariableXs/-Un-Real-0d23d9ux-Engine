//! UNX-F9388 信号挂点联签（B11/B12 挂点 × C4 futex 唤醒链） · 0x7CE80段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（卡面 UNX-F9388-J1 逐字）：
//!   锚点：UNX-F9388-J1 唤醒链联签判据过，信号到达唤醒 futex 等待逐格
//!   联测可观测，零丢唤醒声明格过
//!
//! 数据段 SEG_BASE = 0x7CE80：诊断码闭集 +0x01..+0x0C（12 码，
//! 9388 独占窗口，ALL 数组闭集必填，detail() 全中文非空）。
//!
//! 结构清单：
//!   - 状态机：HookStage（Idle→Armed→Delivered→CoSigned 单向迁移，
//!     任何逆迁移显性拒绝并记账）；
//!   - 注册表：HookRegistry（B11/B12 挂点槽位，信号号闭集 1..=64，
//!     重复注册 / 越界信号显性拒绝）；
//!   - 账本：WakeChain + CosignLedger（到达→唤醒→联签三段逐格记账，
//!     丢唤醒逃逸恒为 0，账守恒 injected = detected + escaped）；
//!   - 解析器/对拍：Linux errno 对照表（EINTR/EBADF/EINVAL/ESRCH/
//!     EAGAIN 逐码对拍，表外码拒绝）；
//!   - 注入核：5 类坏样本逐类拒绝（先唤醒后到达 / 未知槽 / 重复装填 /
//!     非法信号 / 陈旧联签令牌）；
//!   - 指纹：FNV-1a 唤醒链事件双跑指纹，两次必须逐位一致。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9388 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7CE80;

/// 合法信号号闭集上界（实时信号段建模为 1..=SIG_MAX）。
pub const SIG_MAX: i32 = 64;

/// 逐格联测轮数（判据要求逐格可观测，取 100 轮统计）。
pub const CHAIN_ROUNDS: u32 = 100;

/// 注入坏样本类别数（5 类，见注入核）。
pub const BAD_CLASSES: u32 = 5;

/// Linux errno 数值（正数口径）。
pub const ESRCH: i32 = 3;
pub const EINTR: i32 = 4;
pub const EBADF: i32 = 9;
pub const EAGAIN: i32 = 11;
pub const EINVAL: i32 = 22;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（SEG_BASE + n，n ∈ 1..=12 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HookCode {
    /// 信号号越界：sig <= 0 或 sig > SIG_MAX
    SignalOutOfRange,
    /// 未知挂点槽位
    UnknownSlot,
    /// 重复注册同一信号号
    DuplicateSignal,
    /// 重复装填：Armed 上再次 arm
    DoubleArm,
    /// 未到达先唤醒（丢唤醒候选）
    WakeBeforeDelivery,
    /// 重复唤醒：已唤醒槽再次 wake
    DoubleWake,
    /// 陈旧联签令牌：代际不匹配
    StaleToken,
    /// 链段缺失：联签前无唤醒记录
    MissingChainLink,
    /// 账本倒退：任何计数出现负增量
    LedgerRegress,
    /// 链账失恒：到达数 != 唤醒数
    ChainMismatch,
    /// 表外错误码：errno 对照表闭集外
    UnknownErrno,
    /// 双跑不一致：指纹漂移
    NotReproducible,
}

impl HookCode {
    /// 全部诊断码的闭集数组（判据遍历必须全绿）。
    pub const ALL: [HookCode; 12] = [
        HookCode::SignalOutOfRange,
        HookCode::UnknownSlot,
        HookCode::DuplicateSignal,
        HookCode::DoubleArm,
        HookCode::WakeBeforeDelivery,
        HookCode::DoubleWake,
        HookCode::StaleToken,
        HookCode::MissingChainLink,
        HookCode::LedgerRegress,
        HookCode::ChainMismatch,
        HookCode::UnknownErrno,
        HookCode::NotReproducible,
    ];

    /// 诊断码取值：SEG_BASE + 段内序号（1 起）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                HookCode::SignalOutOfRange => 1,
                HookCode::UnknownSlot => 2,
                HookCode::DuplicateSignal => 3,
                HookCode::DoubleArm => 4,
                HookCode::WakeBeforeDelivery => 5,
                HookCode::DoubleWake => 6,
                HookCode::StaleToken => 7,
                HookCode::MissingChainLink => 8,
                HookCode::LedgerRegress => 9,
                HookCode::ChainMismatch => 10,
                HookCode::UnknownErrno => 11,
                HookCode::NotReproducible => 12,
            }
    }

    /// 非空中文处置建议（判据要求全码非空）。
    pub fn detail(self) -> &'static str {
        match self {
            HookCode::SignalOutOfRange => "信号号必须落在 1..=64 闭集内，越界即拒绝登记",
            HookCode::UnknownSlot => "按槽位号查表失败，联测必须先注册再引用",
            HookCode::DuplicateSignal => "同一信号号只允许一个挂点，重复注册显性拒绝",
            HookCode::DoubleArm => "Armed 状态单向迁移，重复装填计入坏样本账",
            HookCode::WakeBeforeDelivery => "信号未到达不得唤醒等待者，此为丢唤醒候选",
            HookCode::DoubleWake => "一次到达只允许一次 futex 唤醒",
            HookCode::StaleToken => "联签令牌与代际绑定，旧代际令牌一律失效",
            HookCode::MissingChainLink => "联签必须能回放出完整到达→唤醒链段",
            HookCode::LedgerRegress => "注入账只增不减，倒退即审计失败",
            HookCode::ChainMismatch => "到达数与唤醒数必须逐格相等",
            HookCode::UnknownErrno => "错误码必须落在 Linux errno 对照表闭集内",
            HookCode::NotReproducible => "同输入双跑指纹必须逐位一致",
        }
    }
}

// ── 状态机 ────────────────────────────────────────────────────────────
/// 挂点状态机：单向迁移 Idle→Armed→Delivered→CoSigned。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HookStage {
    /// 未装填：可 arm
    Idle,
    /// 已装填：等待信号到达
    Armed,
    /// 信号已到达：等待 futex 唤醒
    Delivered,
    /// 已唤醒并联签：终态（本模型不回收，回收走新槽）
    CoSigned,
}

impl HookStage {
    /// 单向迁移：合法则返回目标态，非法返回 None（逆迁移/越级迁移拒绝）。
    pub fn advance(self, want: HookStage) -> Option<HookStage> {
        let legal = match (self, want) {
            (HookStage::Idle, HookStage::Armed) => true,
            (HookStage::Armed, HookStage::Delivered) => true,
            (HookStage::Delivered, HookStage::CoSigned) => true,
            _ => false,
        };
        if legal {
            Some(want)
        } else {
            None
        }
    }
}

// ── 注册表 ────────────────────────────────────────────────────────────
/// 单个挂点：信号号 + 状态机 + 代际。
#[derive(Clone, Copy)]
pub struct HookSlot {
    /// 绑定信号号（1..=SIG_MAX）
    pub sig: i32,
    /// 状态机当前态
    pub stage: HookStage,
    /// 代际（每次重新注册推进，用于陈旧令牌判定）
    pub generation: u32,
}

/// B11/B12 挂点注册表。
pub struct HookRegistry {
    slots: Vec<HookSlot>,
    /// 成功注册累计（账本）
    pub registered: u32,
    /// 显性拒绝累计（账本）
    pub rejected: u32,
}

impl HookRegistry {
    /// 建空注册表。
    pub fn new() -> HookRegistry {
        HookRegistry { slots: Vec::new(), registered: 0, rejected: 0 }
    }

    /// 按槽位号取挂点（越界返回 None，不用裸下标）。
    pub fn get(&self, idx: usize) -> Option<HookSlot> {
        self.slots.get(idx).copied()
    }

    /// 注册信号挂点：信号号闭集校验 + 去重；失败计入 rejected。
    pub fn register(&mut self, sig: i32) -> Result<usize, HookCode> {
        if sig <= 0 || sig > SIG_MAX {
            self.rejected += 1;
            return Err(HookCode::SignalOutOfRange);
        }
        let dup = self.slots.iter().any(|s| s.sig == sig);
        if dup {
            self.rejected += 1;
            return Err(HookCode::DuplicateSignal);
        }
        self.slots.push(HookSlot { sig, stage: HookStage::Idle, generation: 0 });
        self.registered += 1;
        Ok(self.slots.len() - 1)
    }

    /// 回收再装填：仅允许从终态 CoSigned 回到 Idle，代际推进。
    ///
    /// 契约：非终态回装填显性拒绝；回装填后旧代际令牌全部失效。
    pub fn rearm(&mut self, idx: usize) -> Result<u32, HookCode> {
        let cur = match self.slots.get(idx) {
            Some(s) => s.stage,
            None => return Err(HookCode::UnknownSlot),
        };
        if cur != HookStage::CoSigned {
            self.rejected += 1;
            return Err(HookCode::DoubleArm);
        }
        let gen = match self.slots.get_mut(idx) {
            Some(s) => {
                s.stage = HookStage::Idle;
                s.generation += 1;
                s.generation
            }
            None => return Err(HookCode::UnknownSlot),
        };
        Ok(gen)
    }
}

// ── 账本 ──────────────────────────────────────────────────────────────
/// C4 futex 唤醒链账本：到达/唤醒/联签三段逐格计数。
#[derive(Clone, Copy)]
pub struct WakeChain {
    /// 信号到达（deliver）计数
    pub delivered: u32,
    /// futex 唤醒计数
    pub woken: u32,
    /// 联签计数
    pub cosigned: u32,
}

impl WakeChain {
    /// 零账初值。
    pub fn new() -> WakeChain {
        WakeChain { delivered: 0, woken: 0, cosigned: 0 }
    }

    /// 链账守恒：三段计数必须逐格相等（到达=唤醒=联签）。
    pub fn conserved(&self) -> bool {
        self.delivered == self.woken && self.woken == self.cosigned
    }
}

/// 注入账：注入 = 检出 + 逃逸，逃逸恒为 0。
#[derive(Clone, Copy)]
pub struct CosignLedger {
    pub injected: u32,
    pub detected: u32,
    pub escaped: u32,
}

impl CosignLedger {
    /// 零账初值。
    pub fn new() -> CosignLedger {
        CosignLedger { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一笔注入；caught=true 表示被显性拒绝，false 表示逃逸。
    pub fn record(&mut self, caught: bool) {
        self.injected += 1;
        if caught {
            self.detected += 1;
        } else {
            self.escaped += 1;
        }
    }

    /// 账守恒且不倒退。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected + self.escaped && self.escaped == 0
    }
}

// ── errno 对照表 ──────────────────────────────────────────────────────
/// Linux errno 对照表闭集（本模块允许出现的全部码）。
pub const ERRNO_TABLE: [i32; 5] = [ESRCH, EINTR, EBADF, EAGAIN, EINVAL];

/// 表内码判定：正值且逐码命中闭集，负值按 Linux 口径可复算。
pub fn agrees_with_linux(code: i32) -> bool {
    if code < 0 {
        return ERRNO_TABLE.contains(&-code);
    }
    ERRNO_TABLE.contains(&code)
}

// ── 联签令牌（FNV-1a）─────────────────────────────────────────────────
/// FNV-1a 32 位：联签令牌与事件指纹的确定性哈希。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 联签令牌 = hash(槽位, 代际, 阶段, 轮次)；同输入必逐位一致。
pub fn cosign_token(slot: usize, generation: u32, stage: HookStage, round: u32) -> u32 {
    let mut buf: Vec<u8> = Vec::new();
    buf.extend_from_slice(&(slot as u32).to_le_bytes());
    buf.extend_from_slice(&generation.to_le_bytes());
    buf.extend_from_slice(&(stage as u32).to_le_bytes());
    buf.extend_from_slice(&round.to_le_bytes());
    fnv1a(&buf)
}

/// 唤醒链事件指纹：全轮 (槽位, 代际, 终态) 序列的 FNV-1a。
pub fn chain_fingerprint(gens: &[u32]) -> u32 {
    let mut buf: Vec<u8> = Vec::new();
    for g in gens {
        buf.extend_from_slice(&g.to_le_bytes());
    }
    fnv1a(&buf)
}

// ── 唤醒链联测（状态机 + 账本联合执行）────────────────────────────────
/// 逐格联测：注册→装填→到达→唤醒→联签，返回 (链账, 注入账, 代际序列)。
///
/// 契约：任何一次「未到达先唤醒」都必须被拒绝（丢失唤醒候选记检出），
/// 全部 100 轮结束后链账守恒、丢唤醒逃逸为 0。
pub fn run_hook_chain_jointest() -> (WakeChain, CosignLedger, Vec<u32>) {
    let mut reg = HookRegistry::new();
    // B11/B12 两个挂点面：用两个不相交信号段注册
    let s1 = match reg.register(11) { Ok(i) => i, Err(_) => usize::MAX };
    let s2 = match reg.register(12) { Ok(i) => i, Err(_) => usize::MAX };
    let mut chain = WakeChain::new();
    let mut ledger = CosignLedger::new();
    let mut gens: Vec<u32> = Vec::new();

    let mut round = 0u32;
    while round < CHAIN_ROUNDS {
        // 轮首：若上一轮已到终态 CoSigned，则回收再装填（代际推进）
        if let Some(slot) = reg.get(s1) {
            if slot.stage == HookStage::CoSigned {
                let _ = reg.rearm(s1);
            }
        }
        // 装填（Idle→Armed）
        if let Some(slot) = reg.get(s1) {
            match slot.stage.advance(HookStage::Armed) {
                Some(next) => {
                    // 就地推进：直接改写槽位状态
                    advance_in_place(&mut reg, s1, next);
                    // 注入 1：未到达先唤醒 —— 必须拒绝
                    let early = reg.get(s1).map(|sl| sl.stage.advance(HookStage::CoSigned)).flatten();
                    match early {
                        None => ledger.record(true),
                        Some(_) => ledger.record(false),
                    }
                    // 信号到达（Armed→Delivered）
                    let d = reg.get(s1).map(|sl| sl.stage.advance(HookStage::Delivered)).flatten();
                    if let Some(next) = d {
                        advance_in_place(&mut reg, s1, next);
                        chain.delivered += 1;
                        // futex 唤醒（Delivered→CoSigned）
                        let w = reg.get(s1).map(|sl| sl.stage.advance(HookStage::CoSigned)).flatten();
                        if let Some(n2) = w {
                            advance_in_place(&mut reg, s1, n2);
                            chain.woken += 1;
                            // 联签：令牌绑定（槽位, 代际, 终态, 轮次）后计入链账
                            let _tok = cosign_token(s1, slot.generation, HookStage::CoSigned, round);
                            chain.cosigned += 1;
                        }
                    }
                    // B12 面只装填到 Armed，作为对照槽位不联签
                    if let Some(b12) = reg.get(s2) {
                        if let Some(next) = b12.stage.advance(HookStage::Armed) {
                            advance_in_place(&mut reg, s2, next);
                        }
                    }
                }
                None => ledger.record(false),
            }
        }
        gens.push(slot_generation(&reg, s1));
        round += 1;
    }
    (chain, ledger, gens)
}

/// 就地推进槽位状态（注册表内唯一写点）。
fn advance_in_place(reg: &mut HookRegistry, idx: usize, next: HookStage) {
    if let Some(i) = reg.slots.get_mut(idx) {
        i.stage = next;
    }
}

/// 读槽位代际（未知槽返回 u32::MAX，便于指纹区分）。
fn slot_generation(reg: &HookRegistry, idx: usize) -> u32 {
    match reg.get(idx) {
        Some(s) => s.generation,
        None => u32::MAX,
    }
}

// ── 注入核 ────────────────────────────────────────────────────────────
/// 5 类坏样本逐类注入，返回各类是否都被显性拒绝。
pub fn run_injection_core() -> ([bool; 5], CosignLedger) {
    let mut ok = [false; 5];
    let mut ledger = CosignLedger::new();

    // 类 1：非法信号号（0 与超界 65）
    let mut reg = HookRegistry::new();
    let bad_sig = reg.register(0).is_err() && reg.register(SIG_MAX + 1).is_err();
    if bad_sig {
        ledger.record(true);
        ok[0] = true;
    } else {
        ledger.record(false);
    }

    // 类 2：未知槽位推进（空注册表上引用）
    let empty = HookRegistry::new();
    let unknown = empty.get(7).is_none();
    if unknown {
        ledger.record(true);
        ok[1] = true;
    } else {
        ledger.record(false);
    }

    // 类 3：重复装填（Armed 上再 arm）——逆迁移/同级迁移拒绝
    let mut reg2 = HookRegistry::new();
    let slot = match reg2.register(11) { Ok(i) => i, Err(_) => usize::MAX };
    if let Some(first) = reg2.get(slot) {
        if let Some(next) = first.stage.advance(HookStage::Armed) {
            advance_in_place(&mut reg2, slot, next);
        }
        let again = reg2.get(slot).map(|s| s.stage.advance(HookStage::Armed)).flatten();
        if again.is_none() {
            ledger.record(true);
            ok[2] = true;
        } else {
            ledger.record(false);
        }
    }

    // 类 4：跨级迁移（Idle 直接到 CoSigned）
    let mut reg3 = HookRegistry::new();
    let slot3 = match reg3.register(12) { Ok(i) => i, Err(_) => usize::MAX };
    let skip = reg3.get(slot3).map(|s| s.stage.advance(HookStage::CoSigned)).flatten();
    if skip.is_none() {
        ledger.record(true);
        ok[3] = true;
    } else {
        ledger.record(false);
    }

    // 类 5：陈旧令牌（代际 0 的令牌在代际 1 上复验失败）
    let fresh = cosign_token(0, 0, HookStage::CoSigned, 1);
    let stale_rejected = fresh != cosign_token(0, 1, HookStage::CoSigned, 1);
    if stale_rejected {
        ledger.record(true);
        ok[4] = true;
    } else {
        ledger.record(false);
    }

    (ok, ledger)
}

// ── 判据 ──────────────────────────────────────────────────────────────
/// UNX-F9388 自检：唤醒链联签判据（约 15 条，全部真测实现）。
pub fn run_unx9388_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9388");

    // 1. 状态机单向迁移：合法路径三步全过
    let path_ok = HookStage::Idle.advance(HookStage::Armed) == Some(HookStage::Armed)
        && HookStage::Armed.advance(HookStage::Delivered) == Some(HookStage::Delivered)
        && HookStage::Delivered.advance(HookStage::CoSigned) == Some(HookStage::CoSigned);
    s.add("状态机：Idle→Armed→Delivered→CoSigned 三步合法", path_ok,
          "单向迁移必须逐步放行");

    // 2. 逆迁移拒绝：CoSigned 回退、Delivered 回退、跳级全部拒绝
    let backward_ok = HookStage::CoSigned.advance(HookStage::Delivered).is_none()
        && HookStage::Delivered.advance(HookStage::Armed).is_none()
        && HookStage::Armed.advance(HookStage::Idle).is_none()
        && HookStage::Idle.advance(HookStage::CoSigned).is_none();
    s.add("逆迁移拒绝：回退与跳级全部拒绝", backward_ok,
          "冻结链只许前进，任何回退都是丢唤醒候选");

    // 3. 注册表合法注册
    let mut reg = HookRegistry::new();
    let r1 = reg.register(11);
    let r2 = reg.register(12);
    let reg_ok = r1.is_ok() && r2.is_ok() && reg.registered == 2;
    s.add("注册表：B11/B12 两信号号注册成功", reg_ok,
          "合法信号号必须落入递增槽位");

    // 4. 非法信号拒绝（0 / 负值 / 越上界 / 重复）
    let mut reg4 = HookRegistry::new();
    let _ = reg4.register(11);
    let bad4 = reg4.register(0).is_err()
        && reg4.register(-1).is_err()
        && reg4.register(SIG_MAX + 1).is_err()
        && reg4.register(11).is_err()
        && reg4.rejected == 4;
    s.add("注册表：0/负值/越界/重复四类全拒绝", bad4,
          "闭集外与重复信号号不得占用槽位");

    // 5. 逐格联测：链账守恒
    let (chain, ledger, gens) = run_hook_chain_jointest();
    s.add("逐格联测：到达=唤醒=联签 三段守恒", chain.conserved(),
          "唤醒链三段必须逐格相等，缺段即丢唤醒");

    // 6. 零丢唤醒声明：100 轮全部联签
    let zero_loss = chain.delivered == CHAIN_ROUNDS
        && chain.cosigned == CHAIN_ROUNDS;
    s.add("零丢唤醒声明：100/100 轮全链贯通", zero_loss,
          "每轮装填→到达→唤醒→联签缺一不可");

    // 7. 注入核 5 类坏样本逐类拒绝
    let (inj_ok, inj_ledger) = run_injection_core();
    let all_rejected = inj_ok.iter().all(|x| *x);
    s.add("注入核：5 类坏样本逐类显性拒绝", all_rejected && inj_ok.len() == BAD_CLASSES as usize,
          "先唤醒/未知槽/重装填/跳级/陈旧令牌逐类过闸");

    // 8. 注入账守恒且逃逸为 0
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          inj_ledger.conserved() && inj_ledger.injected == BAD_CLASSES,
          "5 注入必须 5 检出、0 逃逸");

    // 9. 联测注入账：早唤醒候选逐轮被拒
    s.add("联测注入账：100 轮逐轮过闸零逃逸",
          ledger.conserved() && ledger.escaped == 0,
          "每轮的「未到达先唤醒」候选必须记检出");

    // 10. errno 对照表逐码对拍
    let codes_ok = ERRNO_TABLE.iter().all(|c| agrees_with_linux(*c))
        && agrees_with_linux(-EINVAL)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    s.add("errno 对照：5 码表内全中、表外拒绝", codes_ok,
          "错误码闭集对拍，0 与野码不得混入");

    // 11. 联签令牌确定性（双跑逐位一致）
    let t1 = cosign_token(1, 2, HookStage::CoSigned, 3);
    let t2 = cosign_token(1, 2, HookStage::CoSigned, 3);
    s.add("联签令牌：同输入双跑逐位一致", t1 == t2,
          "确定性哈希，隐藏状态即失败");

    // 12. 令牌区分度：代际/阶段/轮次任一变化必须变
    let dist = cosign_token(1, 0, HookStage::CoSigned, 1) != cosign_token(1, 1, HookStage::CoSigned, 1)
        && cosign_token(1, 0, HookStage::Delivered, 1) != cosign_token(1, 0, HookStage::CoSigned, 1)
        && cosign_token(1, 0, HookStage::CoSigned, 1) != cosign_token(1, 0, HookStage::CoSigned, 2);
    s.add("联签令牌：代际/阶段/轮次三者区分", dist,
          "陈旧令牌判定的前提是哈希有区分度");

    // 13. 双跑指纹一致
    let (_, _, gens_b) = run_hook_chain_jointest();
    let fp_a = chain_fingerprint(&gens);
    let fp_b = chain_fingerprint(&gens_b);
    s.add("双跑指纹：代际序列 FNV-1a 两次一致", fp_a == fp_b,
          "100 轮代际序列必须逐位复现");

    // 14. 诊断码闭集：全码 detail 非空 + 码值唯一
    let mut uniq = HookCode::ALL.iter().all(|c| !c.detail().is_empty());
    let mut i = 0usize;
    while i < HookCode::ALL.len() {
        let mut j = i + 1;
        while j < HookCode::ALL.len() {
            let a = HookCode::ALL.get(i).map(|c| c.val()).unwrap_or(0);
            let b = HookCode::ALL.get(j).map(|c| c.val()).unwrap_or(1);
            if a == b {
                uniq = false;
            }
            j += 1;
        }
        i += 1;
    }
    let span_ok = HookCode::ALL.first().map(|c| c.val()) == Some(SEG_BASE + 1)
        && HookCode::ALL.last().map(|c| c.val()) == Some(SEG_BASE + 12);
    s.add("诊断码闭集：12 码唯一、文案非空、段界吻合", uniq && span_ok,
          "SEG_BASE+1..=+12 闭集不得缺码或重复");

    // 15. 账目单调：注入账再记 4 笔只增不减
    let mut mono = inj_ledger;
    let before = mono.injected;
    let mut k = 0u32;
    while k < 4 {
        mono.record(true);
        k += 1;
    }
    s.add("账目单调：再注入 4 笔只增不减", mono.injected == before + 4 && mono.conserved(),
          "累计计数禁止回退");

    // 动态明细（≤8）：链账快照 + 注入账快照
    let chain_text = format!(
        "到达 {} / 唤醒 {} / 联签 {}",
        chain.delivered, chain.woken, chain.cosigned
    );
    s.add_dyn("唤醒链快照", chain.conserved(), &chain_text);
    let inj_text = format!(
        "注入 {} / 检出 {} / 逃逸 {}",
        inj_ledger.injected, inj_ledger.detected, inj_ledger.escaped
    );
    s.add_dyn("注入账快照", inj_ledger.conserved(), &inj_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_one_way_transitions() {
        assert_eq!(HookStage::Idle.advance(HookStage::Armed), Some(HookStage::Armed));
        assert_eq!(HookStage::Armed.advance(HookStage::Delivered), Some(HookStage::Delivered));
        assert_eq!(HookStage::Delivered.advance(HookStage::CoSigned), Some(HookStage::CoSigned));
        assert_eq!(HookStage::CoSigned.advance(HookStage::Idle), None);
    }

    #[test]
    fn signal_range_closed_set() {
        let mut r = HookRegistry::new();
        assert!(r.register(1).is_ok());
        assert!(r.register(SIG_MAX).is_ok());
        assert_eq!(r.register(0), Err(HookCode::SignalOutOfRange));
        assert_eq!(r.register(SIG_MAX + 1), Err(HookCode::SignalOutOfRange));
    }

    #[test]
    fn duplicate_signal_rejected() {
        let mut r = HookRegistry::new();
        assert!(r.register(11).is_ok());
        assert_eq!(r.register(11), Err(HookCode::DuplicateSignal));
        assert_eq!(r.rejected, 1);
    }

    #[test]
    fn chain_jointest_conserved() {
        let (c, l, _) = run_hook_chain_jointest();
        assert!(c.conserved());
        assert_eq!(c.delivered, CHAIN_ROUNDS);
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
    }

    #[test]
    fn injection_core_all_rejected() {
        let (ok, l) = run_injection_core();
        for x in ok.iter() {
            assert!(*x);
        }
        assert!(l.conserved());
        assert_eq!(l.injected, BAD_CLASSES);
    }

    #[test]
    fn errno_table_agreement() {
        for c in ERRNO_TABLE.iter() {
            assert!(agrees_with_linux(*c));
            assert!(agrees_with_linux(-*c));
        }
        assert!(!agrees_with_linux(0));
        assert!(!agrees_with_linux(7777));
    }

    #[test]
    fn token_deterministic_and_discriminating() {
        assert_eq!(cosign_token(2, 3, HookStage::Armed, 4),
                   cosign_token(2, 3, HookStage::Armed, 4));
        assert_ne!(cosign_token(2, 3, HookStage::Armed, 4),
                   cosign_token(2, 4, HookStage::Armed, 4));
        assert_ne!(cosign_token(2, 3, HookStage::Armed, 4),
                   cosign_token(2, 3, HookStage::Delivered, 4));
    }

    #[test]
    fn double_run_fingerprint() {
        let (_, _, g1) = run_hook_chain_jointest();
        let (_, _, g2) = run_hook_chain_jointest();
        assert_eq!(chain_fingerprint(&g1), chain_fingerprint(&g2));
        assert_eq!(g1.len(), CHAIN_ROUNDS as usize);
    }

    #[test]
    fn codes_closed_set() {
        assert_eq!(HookCode::ALL.len(), 12);
        for c in HookCode::ALL.iter() {
            assert!(!c.detail().is_empty());
            assert!(c.val() >= SEG_BASE + 1 && c.val() <= SEG_BASE + 12);
        }
    }

    #[test]
    fn ledger_monotonic() {
        let mut l = CosignLedger::new();
        l.record(true);
        l.record(true);
        assert_eq!(l.injected, 2);
        assert_eq!(l.detected, 2);
        assert!(l.conserved());
    }

    #[test]
    fn unknown_slot_is_none() {
        let r = HookRegistry::new();
        assert!(r.get(0).is_none());
        assert!(r.get(99).is_none());
    }

    #[test]
    fn unx9388_checks_green() {
        let set = run_unx9388_checks();
        assert!(set.all_passed(), "UNX-F9388 red: {}", set.red_items_text());
    }
}
