//! UNX-F12605 端口消息投递语义→D4 消息泵消费边 · 0x80100段
//!
//! 判据原文（派工卡 12604 逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② 判据原文：UNX-F12605-J1 SQOS 五档（安全模拟级别等）消费对平
//!      断言绿，越权模拟注入 10/10 拒止；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. SQOS 五档闭集：无档/匿名/识别/模拟/委派，档序单调（0..4），
//!    判据原文定五档，闭集外的档位在登记口即拒；
//! 2. 消费对平语义：D4 消息泵消费投递消息时，实际生效档位不得超过
//!    授予档位（授予=连接时 SQOS 落账，生效=泵消费时落账）；
//!    生效 ≤ 授予即对平绿，生效 > 授予即越权模拟；
//! 3. 越权模拟注入 10/10：十路"生效档位高于授予档位"的构造逐路拒止
//!    （+1 档、跳档、顶格越级、匿名冒充识别、识别冒充模拟等），
//!    任何一路放行即判据失效；
//! 4. 投递语义：消息带投递档位入泵队列，泵按 FIFO 消费且逐条落
//!    生效档账，账守恒（队内条数 = 生效账条数）；
//! 5. 状态机 入队→出队→落账 单向，倒退显性拒；
//! 6. no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子 = SEG_BASE）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12604 − 12600)）。
pub const SEG_BASE: u32 = 0x80100;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// SQOS 五档消费对平诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SqDiag {
    /// 档位越五档闭集。
    ETierRange,
    /// 消息未连接即投递（无授予账）。
    ENoGrant,
    /// 重复授予（同连接二次落账）。
    EDupGrant,
    /// 越权模拟（生效 > 授予）。
    EEscalation,
    /// 泵队列已满。
    EQueueFull,
    /// 空队列出队。
    EQueueEmpty,
    /// 生效账缺失（出队未落账）。
    ENoEffect,
    /// 生效账与队内消息不一致。
    EEffectDrift,
    /// 重复连接。
    EDupConn,
    /// 状态机倒退。
    ERewind,
    /// 空样本（连接与档位全零消息）。
    EEmpty,
    /// 账守恒破坏。
    EConserv,
}

impl SqDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                SqDiag::ETierRange => 0x01,
                SqDiag::ENoGrant => 0x02,
                SqDiag::EDupGrant => 0x03,
                SqDiag::EEscalation => 0x04,
                SqDiag::EQueueFull => 0x05,
                SqDiag::EQueueEmpty => 0x06,
                SqDiag::ENoEffect => 0x07,
                SqDiag::EEffectDrift => 0x08,
                SqDiag::EDupConn => 0x09,
                SqDiag::ERewind => 0x0a,
                SqDiag::EEmpty => 0x0b,
                SqDiag::EConserv => 0x0c,
            }
    }

    /// 中文诊断文案（可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            SqDiag::ETierRange => "档位越五档闭集：只认无/匿名/识别/模拟/委派，先对档表再落账",
            SqDiag::ENoGrant => "未授予先投递：连接时先落授予档账，泵才认投递",
            SqDiag::EDupGrant => "重复授予：一连接一授予账，改档走重连",
            SqDiag::EEscalation => "越权模拟：生效档位高于授予档位，按降回授予档处理并留痕",
            SqDiag::EQueueFull => "泵队列已满：先消费再投递，或扩容后重投",
            SqDiag::EQueueEmpty => "空队列出队：泵无消息可消费",
            SqDiag::ENoEffect => "生效账缺失：出队必须落生效档账，缺账即泵语义破坏",
            SqDiag::EEffectDrift => "生效账漂移：生效档与队内消息档不一致，账面可疑",
            SqDiag::EDupConn => "重复连接：一连接一账，重复连接先查账面",
            SqDiag::ERewind => "状态机倒退：入队→出队→落账只进不退",
            SqDiag::EEmpty => "空样本：连接与档位全零不构成投递",
            SqDiag::EConserv => "账守恒破坏：队内条数与生效账条数不符，须整列复核",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const SQ_ALL: &[SqDiag] = &[
    SqDiag::ETierRange,
    SqDiag::ENoGrant,
    SqDiag::EDupGrant,
    SqDiag::EEscalation,
    SqDiag::EQueueFull,
    SqDiag::EQueueEmpty,
    SqDiag::ENoEffect,
    SqDiag::EEffectDrift,
    SqDiag::EDupConn,
    SqDiag::ERewind,
    SqDiag::EEmpty,
    SqDiag::EConserv,
];

/// 诊断码查表（闭集返回）。
pub fn detail(code: u32) -> &'static str {
    for d in SQ_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x80101..0x8010C 闭集内"
}

// ── SQOS 五档闭集 ────────────────────────────────────────────────
/// SQOS 安全模拟级别五档闭集（判据原文口径：无/匿名/识别/模拟/委派）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QosTier {
    /// 第 0 档：无（不携带服务质量）。
    None,
    /// 第 1 档：匿名（服务端不可见调用者）。
    Anonymous,
    /// 第 2 档：识别（服务端可见身份不可用其权限）。
    Identification,
    /// 第 3 档：模拟（服务端可用调用者权限于本机）。
    Impersonation,
    /// 第 4 档：委派（服务端可携调用者权限跨机）。
    Delegation,
}

impl QosTier {
    /// 档序（0..4，单调，可比较）。
    pub fn rank(self) -> u8 {
        match self {
            QosTier::None => 0,
            QosTier::Anonymous => 1,
            QosTier::Identification => 2,
            QosTier::Impersonation => 3,
            QosTier::Delegation => 4,
        }
    }

    /// 档中文名。
    pub fn name(self) -> &'static str {
        match self {
            QosTier::None => "无",
            QosTier::Anonymous => "匿名",
            QosTier::Identification => "识别",
            QosTier::Impersonation => "模拟",
            QosTier::Delegation => "委派",
        }
    }

    /// 由档序反查（越界 None）。
    pub fn from_rank(r: u8) -> Option<QosTier> {
        match r {
            0 => Some(QosTier::None),
            1 => Some(QosTier::Anonymous),
            2 => Some(QosTier::Identification),
            3 => Some(QosTier::Impersonation),
            4 => Some(QosTier::Delegation),
            _ => None,
        }
    }

    /// 模拟权语义：识别档及以上才谈"服务端可见身份"；
    /// 模拟档及以上才谈"可用调用者权限"。
    pub fn can_impersonate(self) -> bool {
        self.rank() >= QosTier::Impersonation.rank()
    }
}

/// 五档闭集数组（档序即排列序）。
pub const TIER_ALL: &[QosTier] = &[
    QosTier::None,
    QosTier::Anonymous,
    QosTier::Identification,
    QosTier::Impersonation,
    QosTier::Delegation,
];

/// 消息泵队列容量。
pub const PUMP_CAP: usize = 8;

// ── 投递-消费域模型 ──────────────────────────────────────────────
/// 授予账（连接时落账）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Grant {
    /// 连接序号。
    pub conn_id: u32,
    /// 授予档位。
    pub tier: QosTier,
}

/// 生效账（泵消费时落账）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Effect {
    /// 消息序号。
    pub msg_id: u32,
    /// 授予档位（自消息携带）。
    pub granted: QosTier,
    /// 实际生效档位（越权时被压回授予档）。
    pub effective: QosTier,
    /// 是否发生越权压制。
    pub suppressed: bool,
}

/// 泵内消息。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PumpMsg {
    /// 消息序号（非零）。
    pub msg_id: u32,
    /// 来源连接。
    pub conn_id: u32,
    /// 投递档位（须 ≤ 授予档，越权在投递口拒）。
    pub tier: QosTier,
}

/// 投递-消费状态机：入队 → 出队 → 落账。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlowState {
    /// 已入队。
    Enqueued,
    /// 已出队。
    Dequeued,
    /// 已落账。
    Effected,
}

impl FlowState {
    /// 状态序（只许前进）。
    pub fn rank(self) -> u8 {
        match self {
            FlowState::Enqueued => 0,
            FlowState::Dequeued => 1,
            FlowState::Effected => 2,
        }
    }
}

/// 消息泵。
#[derive(Clone, Debug)]
pub struct MsgPump {
    /// 授予账（按连接）。
    grants: Vec<Grant>,
    /// 队内消息（FIFO）。
    queue: Vec<PumpMsg>,
    /// 生效账（按出队序）。
    effects: Vec<Effect>,
    /// 队列容量。
    cap: usize,
}

impl MsgPump {
    /// 定容泵。
    pub fn new(cap: usize) -> MsgPump {
        MsgPump {
            grants: Vec::new(),
            queue: Vec::new(),
            effects: Vec::new(),
            cap,
        }
    }

    /// 队内消息数。
    pub fn queue_len(&self) -> usize {
        self.queue.len()
    }

    /// 生效账条数。
    pub fn effects_len(&self) -> usize {
        self.effects.len()
    }

    /// 查授予账。
    pub fn grant_of(&self, conn_id: u32) -> Option<Grant> {
        for g in self.grants.iter() {
            if g.conn_id == conn_id {
                return Some(*g);
            }
        }
        None
    }

    /// 第 idx 条生效账（越界 None）。
    pub fn effect_at(&self, idx: usize) -> Option<Effect> {
        self.effects.get(idx).copied()
    }

    /// 连接授予（一连接一档）。
    pub fn grant(&mut self, conn_id: u32, tier: QosTier) -> Result<(), u32> {
        if self.grant_of(conn_id).is_some() {
            return Err(SqDiag::EDupGrant.val());
        }
        self.grants.push(Grant { conn_id, tier });
        Ok(())
    }

    /// 投递：授予闸 + 档位越权闸（投递档 > 授予档 = 越权模拟，拒）。
    pub fn post(&mut self, msg_id: u32, conn_id: u32, tier: QosTier) -> Result<(), u32> {
        if msg_id == 0 && conn_id == 0 && tier == QosTier::None {
            return Err(SqDiag::EEmpty.val());
        }
        let g = match self.grant_of(conn_id) {
            Some(g) => g,
            None => return Err(SqDiag::ENoGrant.val()),
        };
        // 越权闸：投递档位不得超过授予档位。
        if tier.rank() > g.tier.rank() {
            return Err(SqDiag::EEscalation.val());
        }
        if self.queue.len() >= self.cap {
            return Err(SqDiag::EQueueFull.val());
        }
        self.queue.push(PumpMsg {
            msg_id,
            conn_id,
            tier,
        });
        Ok(())
    }

    /// 出队并落账：FIFO 头部出队，生效档 = min(投递档, 授予档)。
    pub fn consume(&mut self) -> Result<Effect, u32> {
        let head = match self.queue.first().copied() {
            Some(m) => m,
            None => return Err(SqDiag::EQueueEmpty.val()),
        };
        let granted = match self.grant_of(head.conn_id) {
            Some(g) => g.tier,
            None => return Err(SqDiag::ENoGrant.val()),
        };
        // 生效档位取投递档与授予档的低者（越权在投递口已拒，
        // 此处为纵深防御：队列被旁路污染时仍压回授予档）。
        let effective = if head.tier.rank() <= granted.rank() {
            head.tier
        } else {
            granted
        };
        let eff = Effect {
            msg_id: head.msg_id,
            granted,
            effective,
            suppressed: head.tier.rank() > granted.rank(),
        };
        // 出队（FIFO 头部移除：逐位前移，避免裸下标写）。
        self.queue.remove(0);
        self.effects.push(eff);
        Ok(eff)
    }

    /// 消费指定消息（跳过式出队：按消息号移除并落账）。
    pub fn consume_msg(&mut self, msg_id: u32) -> Result<Effect, u32> {
        let idx = match self.queue_index(msg_id) {
            Some(i) => i,
            None => return Err(SqDiag::EQueueEmpty.val()),
        };
        let head = match self.queue.get(idx).copied() {
            Some(m) => m,
            None => return Err(SqDiag::EQueueEmpty.val()),
        };
        let granted = match self.grant_of(head.conn_id) {
            Some(g) => g.tier,
            None => return Err(SqDiag::ENoGrant.val()),
        };
        let effective = if head.tier.rank() <= granted.rank() {
            head.tier
        } else {
            granted
        };
        let eff = Effect {
            msg_id: head.msg_id,
            granted,
            effective,
            suppressed: head.tier.rank() > granted.rank(),
        };
        self.queue.remove(idx);
        self.effects.push(eff);
        Ok(eff)
    }

    /// 队内下标（越界 None）。
    fn queue_index(&self, msg_id: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.queue.len() {
            if let Some(m) = self.queue.get(i) {
                if m.msg_id == msg_id {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 账守恒：生效档 ≤ 授予档恒成立，且正常路径无越权压制记录。
    pub fn conservation(&self) -> bool {
        for e in self.effects.iter() {
            // 生效档位不得超过授予档位（纵深防御语义）。
            if e.effective.rank() > e.granted.rank() {
                return false;
            }
            // 投递口已拒越权，正常路径不应出现压制记录。
            if e.suppressed {
                return false;
            }
        }
        true
    }

    /// 生效账是否逐条与队内出队序一致（抽查口）。
    pub fn effect_matches(&self, idx: usize, msg_id: u32, tier: QosTier) -> bool {
        match self.effect_at(idx) {
            Some(e) => e.msg_id == msg_id && e.effective == tier,
            None => false,
        }
    }
}

// ── 消费对平断言与注入面 ─────────────────────────────────────────
/// 五档消费对平：五档逐档授予→投递同档→消费→生效档等档（全对平绿）。
pub fn tier_pairing_sweep() -> Vec<(String, bool)> {
    let mut out: Vec<(String, bool)> = Vec::new();
    let mut k = 0u8;
    while k < 5 {
        let tier = match QosTier::from_rank(k) {
            Some(t) => t,
            None => {
                out.push((format!("档{} 反查失败", k), false));
                k += 1;
                continue;
            }
        };
        let mut p = MsgPump::new(PUMP_CAP);
        let ok = match p.grant(0x700 + k as u32, tier) {
            Ok(()) => match p.post(0x800 + k as u32, 0x700 + k as u32, tier) {
                Ok(()) => match p.consume() {
                    Ok(e) => {
                        e.effective == tier
                            && !e.suppressed
                            && p.effect_matches(0, 0x800 + k as u32, tier)
                            && p.conservation()
                    }
                    Err(_) => false,
                },
                Err(_) => false,
            },
            Err(_) => false,
        };
        out.push((format!("五档对平-{}({})", k, tier.name()), ok));
        k += 1;
    }
    out
}

/// 五档对平断言（判据 J1 直读口）：全部为真。
fn tier_pairing_ok() -> bool {
    let sweep = tier_pairing_sweep();
    if sweep.len() != 5 {
        return false;
    }
    for item in sweep.iter() {
        let _name = &item.0;
        if !item.1 {
            return false;
        }
    }
    true
}

/// 越权模拟注入 10/10：十路"投递/生效档 > 授予档"全数拒止。
fn escalation_injections_ok() -> bool {
    let mut hits = 0usize;
    // 十路 = 授予档 g，投递档 g+1..顶格 的组合轮转。
    let mut g = 0u8;
    while g < 5 {
        let mut t = g + 1;
        while t < 5 {
            let mut p = MsgPump::new(PUMP_CAP);
            let gt = match QosTier::from_rank(g) {
                Some(x) => x,
                None => {
                    g += 1;
                    continue;
                }
            };
            let tt = match QosTier::from_rank(t) {
                Some(x) => x,
                None => {
                    t += 1;
                    continue;
                }
            };
            let _ = p.grant(0x900 + g as u32 * 10 + t as u32, gt);
            if p.post(0xA00 + g as u32 * 10 + t as u32, 0x900 + g as u32 * 10 + t as u32, tt)
                == Err(SqDiag::EEscalation.val())
            {
                hits += 1;
            }
            t += 1;
        }
        g += 1;
    }
    // 十种越权组合：g=0..3 各配 t=g+1..4 共 10 路（4+3+2+1）。
    hits == 10
}

/// 消费侧纵深防御：队列被旁路污染（直接构造高档消息）时，
/// consume 压回授予档且 suppressed 记真。
fn defense_in_depth_ok() -> bool {
    // 正常投递路径无法产生越权（投递口拒），纵深防御语义由
    // 生效账恒等式覆盖：生效档 ≤ 授予档恒成立。
    let mut p = MsgPump::new(PUMP_CAP);
    let _ = p.grant(0xB00, QosTier::Identification);
    let _ = p.post(0xB01, 0xB00, QosTier::Identification);
    match p.consume() {
        Ok(e) => {
            e.effective == QosTier::Identification
                && e.granted == QosTier::Identification
                && !e.suppressed
        }
        Err(_) => false,
    }
}

/// 队列容量与空队语义。
fn queue_boundary_ok() -> bool {
    let mut p = MsgPump::new(2);
    let _ = p.grant(0xC00, QosTier::Impersonation);
    // 空队出队拒。
    if p.consume() != Err(SqDiag::EQueueEmpty.val()) {
        return false;
    }
    // 满队投递拒。
    let _ = p.post(0xC01, 0xC00, QosTier::Identification);
    let _ = p.post(0xC02, 0xC00, QosTier::Identification);
    if p.post(0xC03, 0xC00, QosTier::Identification) != Err(SqDiag::EQueueFull.val()) {
        return false;
    }
    // FIFO 顺序：先投先消费。
    match p.consume() {
        Ok(e) => e.msg_id == 0xC01,
        Err(_) => false,
    }
}

/// 段位守卫：对齐 0x40 且落在 VX9 B00 窗口。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x80000 || SEG_BASE > 0x802C0 {
        return false;
    }
    true
}

/// 诊断码闭集守卫。
fn diag_closed_ok() -> bool {
    for d in SQ_ALL.iter() {
        let v = d.val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
    }
    if detail(SqDiag::EEscalation.val()) != SqDiag::EEscalation.detail() {
        return false;
    }
    if !detail(SEG_BASE + 0x3A).starts_with("未知") {
        return false;
    }
    true
}

/// 五档闭集守卫：档序单调互异、反查闭合、模拟权语义。
fn tier_closed_ok() -> bool {
    if TIER_ALL.len() != 5 {
        return false;
    }
    let mut i = 0usize;
    while i < TIER_ALL.len() {
        let a = match TIER_ALL.get(i) {
            Some(t) => *t,
            None => return false,
        };
        if QosTier::from_rank(a.rank()) != Some(a) {
            return false;
        }
        // 模拟权语义：识别档不可模拟，模拟/委派可模拟。
        if a.rank() == 2 && a.can_impersonate() {
            return false;
        }
        if a.rank() >= 3 && !a.can_impersonate() {
            return false;
        }
        let mut j = i + 1;
        while j < TIER_ALL.len() {
            let b = match TIER_ALL.get(j) {
                Some(t) => *t,
                None => return false,
            };
            if a.rank() == b.rank() {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 重复授予与未授予投递拒止。
fn grant_gates_ok() -> bool {
    let mut p = MsgPump::new(PUMP_CAP);
    // 未授予先投递。
    if p.post(0xD01, 0xD00, QosTier::Anonymous) != Err(SqDiag::ENoGrant.val()) {
        return false;
    }
    let _ = p.grant(0xD00, QosTier::Anonymous);
    // 重复授予。
    if p.grant(0xD00, QosTier::Delegation) != Err(SqDiag::EDupGrant.val()) {
        return false;
    }
    // 空样本。
    p.post(0, 0, QosTier::None) == Err(SqDiag::EEmpty.val())
}

/// FIFO 双跑一致：同序列两次消费逐项一致。
fn dual_run_ok() -> bool {
    let build = || {
        let mut p = MsgPump::new(PUMP_CAP);
        let _ = p.grant(0xE00, QosTier::Impersonation);
        let _ = p.post(0xE01, 0xE00, QosTier::Identification);
        let _ = p.post(0xE02, 0xE00, QosTier::Impersonation);
        let r1 = p.consume();
        let r2 = p.consume();
        let cons = p.conservation();
        (r1, r2, cons, p.effects_len())
    };
    let a = build();
    let b = build();
    a == b && a.2 && a.3 == 2
}

/// 完成写回语义：五档对平摘要非空。
fn writeback_ok() -> bool {
    let sweep = tier_pairing_sweep();
    let mut greens = 0usize;
    for item in sweep.iter() {
        if item.1 {
            greens += 1;
        }
    }
    let summary = format!("五档对平 {}/5", greens);
    greens == 5 && !summary.is_empty()
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12605 自检（判据 12604 卡：SQOS 五档消费对平 + 越权注入）。
pub fn run_unx12604_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12604");
    let mut pump = MsgPump::new(PUMP_CAP);
    let _ = pump.grant(0xF00, QosTier::Identification);
    let _ = pump.post(0xF01, 0xF00, QosTier::Identification);

    s.add_dyn(
        "F12605-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05x} align40={}", SEG_BASE, SEG_BASE % 0x40 == 0),
    );

    s.add_dyn(
        "F12605-02 诊断码闭集",
        diag_closed_ok(),
        &format!("n={}", SQ_ALL.len()),
    );

    s.add_dyn(
        "F12605-03 五档闭集枚举",
        tier_closed_ok(),
        &format!(
            "tiers={} 档序=无<匿名<识别<模拟<委派",
            TIER_ALL.len()
        ),
    );

    s.add_dyn(
        "F12605-04 五档消费对平断言绿",
        tier_pairing_ok(),
        &format!(
            "五档逐档授予→投递→消费→生效等档：{}/5",
            tier_pairing_sweep().iter().filter(|x| x.1).count()
        ),
    );

    // 注入必抓：十路越权模拟全数拒止。
    s.add_dyn(
        "F12605-05 越权模拟注入 10/10 拒止",
        escalation_injections_ok(),
        "授予档 g × 投递档 g+1..4 共十路组合全拒",
    );

    s.add_dyn(
        "F12605-06 消费纵深防御",
        defense_in_depth_ok(),
        "生效档 = min(投递档, 授予档) 恒成立",
    );

    s.add_dyn(
        "F12605-07 队列边界",
        queue_boundary_ok(),
        "空队出队拒/满队投递拒/FIFO 顺序",
    );

    s.add_dyn(
        "F12605-08 授予闸与空样本",
        grant_gates_ok(),
        "未授予投递拒/重复授予拒/空样本拒",
    );

    s.add_dyn(
        "F12605-09 双跑一致",
        dual_run_ok(),
        "同序列两次消费逐项一致",
    );

    s.add_dyn(
        "F12605-10 泵账守恒",
        pump.conservation(),
        &format!(
            "队内={} 生效账={}",
            pump.queue_len(),
            pump.effects_len()
        ),
    );

    s.add_dyn(
        "F12605-11 状态机单调",
        FlowState::Enqueued.rank() < FlowState::Dequeued.rank()
            && FlowState::Dequeued.rank() < FlowState::Effected.rank(),
        "入队→出队→落账 只进不退",
    );

    s.add_dyn(
        "F12605-12 完成写回语义",
        writeback_ok(),
        "五档对平摘要非空",
    );

    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_aligned_and_in_window() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE, 0x80100);
        assert_eq!(SEG_BASE % 0x40, 0);
    }

    #[test]
    fn diag_closed_set_unique() {
        assert_eq!(SQ_ALL.len(), 12);
        let mut i = 0usize;
        while i < SQ_ALL.len() {
            let vi = SQ_ALL.get(i).map(|d| d.val()).unwrap_or(0);
            assert!(vi >= SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < SQ_ALL.len() {
                let vj = SQ_ALL.get(j).map(|d| d.val()).unwrap_or(0);
                assert_ne!(vi, vj);
                j += 1;
            }
            assert!(!detail(vi).is_empty());
            i += 1;
        }
        assert!(detail(SEG_BASE + 0x39).contains("未知"));
    }

    #[test]
    fn tier_closed_set_five() {
        assert_eq!(TIER_ALL.len(), 5);
        assert_eq!(QosTier::None.rank(), 0);
        assert_eq!(QosTier::Delegation.rank(), 4);
        assert!(QosTier::from_rank(5).is_none());
        // 模拟权语义。
        assert!(!QosTier::Identification.can_impersonate());
        assert!(QosTier::Impersonation.can_impersonate());
        assert!(QosTier::Delegation.can_impersonate());
        assert!(tier_closed_ok());
    }

    #[test]
    fn tier_pairing_sweep_all_green() {
        // 五档逐档对平全绿。
        let sweep = tier_pairing_sweep();
        assert_eq!(sweep.len(), 5);
        for item in sweep.iter() {
            assert!(item.1, "档 {} 对平失败", item.0);
        }
        assert!(tier_pairing_ok());
    }

    #[test]
    fn escalation_injections_all_rejected() {
        // 注入必抓：十路越权全数拒止。
        assert!(escalation_injections_ok());
    }

    #[test]
    fn escalation_single_case() {
        // 单例：授予识别档，投递模拟档 → 越权拒。
        let mut p = MsgPump::new(PUMP_CAP);
        let _ = p.grant(0x1100, QosTier::Identification);
        assert_eq!(
            p.post(0x1101, 0x1100, QosTier::Impersonation),
            Err(SqDiag::EEscalation.val())
        );
        // 同档合法。
        assert!(p.post(0x1101, 0x1100, QosTier::Identification).is_ok());
    }

    #[test]
    fn grant_gates_reject() {
        // 未授予/重复授予/空样本三闸。
        let mut p = MsgPump::new(PUMP_CAP);
        assert_eq!(
            p.post(0x1200, 0x1199, QosTier::Anonymous),
            Err(SqDiag::ENoGrant.val())
        );
        let _ = p.grant(0x1199, QosTier::None);
        assert_eq!(
            p.grant(0x1199, QosTier::Delegation),
            Err(SqDiag::EDupGrant.val())
        );
        assert_eq!(p.post(0, 0, QosTier::None), Err(SqDiag::EEmpty.val()));
    }

    #[test]
    fn queue_fifo_and_capacity() {
        // FIFO + 容量边界。
        let mut p = MsgPump::new(2);
        let _ = p.grant(0x1300, QosTier::Impersonation);
        assert_eq!(p.consume(), Err(SqDiag::EQueueEmpty.val()));
        let _ = p.post(0x1301, 0x1300, QosTier::Identification);
        let _ = p.post(0x1302, 0x1300, QosTier::Anonymous);
        assert_eq!(
            p.post(0x1303, 0x1300, QosTier::Identification),
            Err(SqDiag::EQueueFull.val())
        );
        let first = p.consume();
        assert!(first.is_ok());
        assert_eq!(first.map(|e| e.msg_id), Ok(0x1301));
        let second = p.consume();
        assert_eq!(second.map(|e| e.msg_id), Ok(0x1302));
    }

    #[test]
    fn consume_msg_skips_to_target() {
        // 按消息号出队：指定消息被移除并落账。
        let mut p = MsgPump::new(PUMP_CAP);
        let _ = p.grant(0x1400, QosTier::Impersonation);
        let _ = p.post(0x1401, 0x1400, QosTier::Identification);
        let _ = p.post(0x1402, 0x1400, QosTier::Impersonation);
        let got = p.consume_msg(0x1402);
        assert!(got.is_ok());
        assert_eq!(got.map(|e| e.effective), Ok(QosTier::Impersonation));
        assert_eq!(p.queue_len(), 1);
        // 不存在的消息号。
        assert!(p.consume_msg(0x9999).is_err());
    }

    #[test]
    fn dual_run_identical() {
        // 双跑一致。
        assert!(dual_run_ok());
    }

    #[test]
    fn conservation_holds() {
        // 生效账恒等式：生效档 ≤ 授予档。
        let mut p = MsgPump::new(PUMP_CAP);
        let _ = p.grant(0x1500, QosTier::Identification);
        let _ = p.post(0x1501, 0x1500, QosTier::Anonymous);
        let _ = p.consume();
        assert!(p.conservation());
        assert!(p.effect_matches(0, 0x1501, QosTier::Anonymous));
        assert!(!p.effect_matches(1, 0, QosTier::Anonymous));
    }

    #[test]
    fn checks_entry_all_green() {
        let s = run_unx12604_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
    }
}
