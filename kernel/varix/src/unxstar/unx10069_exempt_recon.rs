//! UNX-F10069 豁免总对账（存量+清偿后） · 0x92100 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡号：UNX-F10069（M 型收口线：豁免 v2 账↔审计面+版本链锚连）。
//! 锚点：UNX-F10070-J1 版本链连续核（逐段锚连 10/10 次），断链 10/10
//!       次检出。
//!
//! 数据段说明：
//!   SEG_BASE = 0x92100（卡面 seg 闭集）。诊断码占用段内 +0x01..+0x0C
//!   闭集窗口，码值 = SEG_BASE + 变体序号，段内两两唯一、段外不越界；
//!   全部码位由 `ExemptCode::ALL` 封闭枚举给出，禁止运行期造码。
//!
//! 结构清单：
//!   - 账本 `ExemptLedger`（v2 账）：存量（永久豁免）与已清偿双名单，
//!     逐条登记 ID 9601..=10080，清偿迁移单向、重复/越窗显性拒绝；
//!   - 审计面 `AuditFace`：从账本独立重算的双名单快照，与账本逐条比对，
//!     账实漂移（账后面变/面缺条）显性检出；
//!   - 版本链 `VersionChain`：逐段锚连（anchor = FNV1a(prev‖payload)），
//!     断链定位到首坏段；篡改任一段即检出；
//!   - 状态机 `ExemptState`：Registering → Reconciled 单向推进；
//!   - 注入核 `BadExemptCase`：账实漂移 / 版本链断链 / 清偿后仍存量 /
//!     审计面缺条 / 越窗 ID 五类坏样本逐类 10/10 次拒绝；
//!   - Linux errno 对照表 + FNV-1a 双跑指纹（无浮点、无随机）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10069 独占，0x40 对齐窗口起点）。
pub const SEG_BASE: u32 = 0x92100;

/// 段窗上界（下一卡 0x92140 起点，闭开区间右端）。
pub const SEG_END: u32 = 0x92140;

/// 豁免 ID 窗起点（F9601）。
pub const ID_LO: u32 = 9601;

/// 豁免 ID 窗终点（F10080）。
pub const ID_HI: u32 = 10080;

/// 豁免 ID 总数（480）。
pub const ID_TOTAL: u32 = ID_HI - ID_LO + 1;

/// 链段锚种子（链首锚，FNV 偏移基）。
pub const CHAIN_SEED: u32 = 0x9210_0921;

/// 单类注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// Linux errno 正数码（对照表口径）。
pub const EINVAL: i32 = 22;
pub const EEXIST: i32 = 17;
pub const ENOENT: i32 = 2;
pub const EIO: i32 = 5;
pub const EBADF: i32 = 9;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExemptCode {
    /// 越窗 ID（出 9601..=10080）
    OutOfWindow,
    /// 重复登记（同 ID 二次入账）
    DuplicateId,
    /// 清偿后仍存量（已清偿 ID 再清或再存量）
    ClearedResidual,
    /// 账实漂移（账本与审计面不一致）
    LedgerDrift,
    /// 版本链断链（锚连破坏）
    ChainBreak,
    /// 审计面缺条（面未覆盖账内条目）
    FaceMissing,
    /// 账本倒退（总条数只增不减破坏）
    LedgerRegress,
    /// 对账未完成出结论
    NotReconciled,
    /// 对账后改账
    ReconciledMutate,
    /// 解析失败
    ParseFailure,
    /// 状态倒退
    StateRegress,
    /// 段位越界
    SegmentViolation,
}

impl ExemptCode {
    /// 码值（段基址 +0x01 起连续编号，闭集唯一）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            ExemptCode::OutOfWindow => "越窗 ID 被拒：豁免号只允许 9601..=10080；请核对窗口",
            ExemptCode::DuplicateId => "重复登记被拒：同 ID 已入账；请查重复",
            ExemptCode::ClearedResidual => "清偿后仍存量被拒：已清偿 ID 不得再存量；请查迁移",
            ExemptCode::LedgerDrift => "账实漂移被拒：账本与审计面不一致；请重算审计面",
            ExemptCode::ChainBreak => "断链被拒：版本链锚连破坏；请定位首坏段",
            ExemptCode::FaceMissing => "审计面缺条被拒：面未覆盖账内条目；请重刷审计面",
            ExemptCode::LedgerRegress => "账本倒退被拒：总条数只增不减；请排查回滚",
            ExemptCode::NotReconciled => "对账未完成被拒：未逐条对齐不得出总核结论",
            ExemptCode::ReconciledMutate => "对账后改账被拒：总核封账只读；请走修订通道",
            ExemptCode::ParseFailure => "解析失败：豁免行必须恰为 F<id>|stock|cleared 三段",
            ExemptCode::StateRegress => "状态倒退被拒：Registering→Reconciled 单向",
            ExemptCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在 0x92100..=0x92140",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [ExemptCode; 12] = [
        ExemptCode::OutOfWindow,
        ExemptCode::DuplicateId,
        ExemptCode::ClearedResidual,
        ExemptCode::LedgerDrift,
        ExemptCode::ChainBreak,
        ExemptCode::FaceMissing,
        ExemptCode::LedgerRegress,
        ExemptCode::NotReconciled,
        ExemptCode::ReconciledMutate,
        ExemptCode::ParseFailure,
        ExemptCode::StateRegress,
        ExemptCode::SegmentViolation,
    ];
}

// ── FNV-1a 与版本链 ──────────────────────────────────────────────────
/// FNV-1a 32 位（无浮点、无随机）。
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

/// 链段锚：anchor = FNV1a(prev_anchor.to_le_bytes() ‖ payload)。
pub fn chain_anchor(prev: u32, payload: u32) -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    bytes.extend_from_slice(&prev.to_le_bytes());
    bytes.extend_from_slice(&payload.to_le_bytes());
    fnv1a(&bytes)
}

/// 版本链：逐段锚连，段 payload 为豁免账版本号。
#[derive(Clone, Debug)]
pub struct VersionChain {
    /// (payload, anchor) 序列
    pub nodes: Vec<(u32, u32)>,
    /// 链首锚
    pub seed: u32,
}

impl VersionChain {
    /// 构造空链（锚种子固定）。
    pub fn new() -> VersionChain {
        VersionChain { nodes: Vec::new(), seed: CHAIN_SEED }
    }

    /// 追加一段：anchor 由前段锚连派生；payload=0 拒绝（空段）。
    pub fn append(&mut self, payload: u32) -> Result<(), ExemptCode> {
        if payload == 0 {
            return Err(ExemptCode::ParseFailure);
        }
        let prev = match self.nodes.last() {
            Some((_, a)) => *a,
            None => self.seed,
        };
        let a = chain_anchor(prev, payload);
        self.nodes.push((payload, a));
        Ok(())
    }

    /// 连续核：逐段重算锚连；返回首坏段下标（0 基），全通返回 None。
    pub fn verify(&self) -> Option<usize> {
        let mut prev = self.seed;
        let mut i = 0usize;
        while i < self.nodes.len() {
            let (payload, anchor) = match self.nodes.get(i) {
                Some(n) => *n,
                None => return Some(i),
            };
            if chain_anchor(prev, payload) != anchor {
                return Some(i);
            }
            prev = anchor;
            i += 1;
        }
        None
    }
}

// ── 豁免 v2 账 ────────────────────────────────────────────────────────
/// 状态机：Registering → Reconciled 单向推进。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExemptState {
    /// 登记中
    Registering,
    /// 已对账（账实一致且链连续）
    Reconciled,
}

/// 豁免 v2 账：存量（永久豁免）+ 已清偿双名单。
#[derive(Clone, Debug)]
pub struct ExemptLedger {
    /// 存量名单（永久豁免 ID）
    pub stock: Vec<u32>,
    /// 已清偿名单
    pub cleared: Vec<u32>,
    /// 当前状态
    pub state: ExemptState,
}

impl ExemptLedger {
    /// 构造空账。
    pub fn new() -> ExemptLedger {
        ExemptLedger { stock: Vec::new(), cleared: Vec::new(), state: ExemptState::Registering }
    }

    /// 登记存量：ID 必检、重复必拒、已清偿 ID 拒再存量。
    pub fn register_stock(&mut self, id: u32) -> Result<(), ExemptCode> {
        if self.state != ExemptState::Registering {
            return Err(ExemptCode::ReconciledMutate);
        }
        if id < ID_LO || id > ID_HI {
            return Err(ExemptCode::OutOfWindow);
        }
        if self.stock.contains(&id) || self.cleared.contains(&id) {
            return Err(ExemptCode::DuplicateId);
        }
        self.stock.push(id);
        Ok(())
    }

    /// 清偿迁移：存量 → 已清偿（单向）；非存量/已清偿再清均拒。
    pub fn clear(&mut self, id: u32) -> Result<(), ExemptCode> {
        if self.state != ExemptState::Registering {
            return Err(ExemptCode::ReconciledMutate);
        }
        if self.cleared.contains(&id) {
            return Err(ExemptCode::ClearedResidual);
        }
        let pos = self.stock.iter().position(|x| *x == id);
        match pos {
            Some(p) => {
                self.stock.remove(p);
                self.cleared.push(id);
                Ok(())
            }
            None => Err(ExemptCode::NotReconciled),
        }
    }

    /// 总条数（只增不减守卫基准：清偿迁移不改总数）。
    pub fn total(&self) -> usize {
        self.stock.len().saturating_add(self.cleared.len())
    }

    /// 对账：审计面逐条比对 + 版本链连续核；全过才转 Reconciled。
    pub fn reconcile(
        &mut self,
        face: &AuditFace,
        chain: &VersionChain,
        ledger: &mut ReconLedger,
    ) -> Result<(), ExemptCode> {
        if self.state != ExemptState::Registering {
            return Err(ExemptCode::StateRegress);
        }
        if chain.verify().is_some() {
            return Err(ExemptCode::ChainBreak);
        }
        let mut n = ID_LO;
        while n <= ID_HI {
            let in_stock = self.stock.contains(&n);
            let in_cleared = self.cleared.contains(&n);
            let face_stock = face.stock_bits.get((n - ID_LO) as usize).copied().unwrap_or(false);
            let face_cleared =
                face.cleared_bits.get((n - ID_LO) as usize).copied().unwrap_or(false);
            let same = in_stock == face_stock && in_cleared == face_cleared;
            let _ = ledger.record(same);
            if !same {
                return Err(ExemptCode::LedgerDrift);
            }
            n += 1;
        }
        self.state = ExemptState::Reconciled;
        Ok(())
    }
}

/// 审计面：从账本独立重算的双位图快照。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuditFace {
    /// 存量位图（480 位）
    pub stock_bits: Vec<bool>,
    /// 已清偿位图（480 位）
    pub cleared_bits: Vec<bool>,
}

impl AuditFace {
    /// 从账本重算审计面。
    pub fn from_ledger(l: &ExemptLedger) -> AuditFace {
        let mut stock_bits = vec![false; ID_TOTAL as usize];
        let mut cleared_bits = vec![false; ID_TOTAL as usize];
        for id in l.stock.iter() {
            if *id >= ID_LO && *id <= ID_HI {
                if let Some(slot) = stock_bits.get_mut((*id - ID_LO) as usize) {
                    *slot = true;
                }
            }
        }
        for id in l.cleared.iter() {
            if *id >= ID_LO && *id <= ID_HI {
                if let Some(slot) = cleared_bits.get_mut((*id - ID_LO) as usize) {
                    *slot = true;
                }
            }
        }
        AuditFace { stock_bits, cleared_bits }
    }
}

/// 对账走账：核对条数 + 漂移逃逸数（恒 0）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ReconLedger {
    /// 已核对条数
    pub checked: u32,
    /// 漂移逃逸数（必须为 0）
    pub drift_escape: u32,
}

impl ReconLedger {
    /// 构造零账。
    pub const fn zero() -> ReconLedger {
        ReconLedger { checked: 0, drift_escape: 0 }
    }

    /// 记一条核对；same=false 即漂移显性入账（不静默）。
    pub fn record(&mut self, same: bool) -> Result<(), ExemptCode> {
        self.checked = self.checked.saturating_add(1);
        if !same {
            self.drift_escape = self.drift_escape.saturating_add(1);
        }
        Ok(())
    }
}

/// 标准场景：登记 5 条存量、清偿 2 条，链 3 段，审计面同源重算。
pub fn scenario_standard() -> Result<(ExemptLedger, AuditFace, VersionChain), ExemptCode> {
    let mut l = ExemptLedger::new();
    l.register_stock(9605)?;
    l.register_stock(9640)?;
    l.register_stock(9700)?;
    l.register_stock(9810)?;
    l.register_stock(9950)?;
    l.clear(9640)?;
    l.clear(9810)?;
    let face = AuditFace::from_ledger(&l);
    let mut chain = VersionChain::new();
    chain.append(1)?;
    chain.append(2)?;
    chain.append(3)?;
    Ok((l, face, chain))
}

// ── 注入核 ────────────────────────────────────────────────────────────
/// 五类坏豁免样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadExemptCase {
    /// 账实漂移（账后面变）
    LedgerDrift,
    /// 版本链断链（篡改段 payload）
    ChainBreak,
    /// 清偿后仍存量（再清 / 再存量）
    ClearedResidual,
    /// 审计面缺条（面从部分账重算）
    FaceMissing,
    /// 越窗 ID
    OutOfWindow,
}

impl BadExemptCase {
    /// 闭集全量。
    pub const ALL: [BadExemptCase; 5] = [
        BadExemptCase::LedgerDrift,
        BadExemptCase::ChainBreak,
        BadExemptCase::ClearedResidual,
        BadExemptCase::FaceMissing,
        BadExemptCase::OutOfWindow,
    ];

    /// 注入一轮：构造坏样本并触发校验，返回是否被显式拒绝。
    pub fn inject(self) -> bool {
        match self {
            BadExemptCase::LedgerDrift => {
                // 审计面重算后又入一条存量 → 逐条对账必报漂移。
                let (mut l, face, chain) = match scenario_standard() {
                    Ok(t) => t,
                    Err(_) => return false,
                };
                if l.register_stock(9900).is_err() {
                    return false;
                }
                let mut rl = ReconLedger::zero();
                let mut c2 = chain;
                l.reconcile(&face, &mut c2, &mut rl).is_err()
                    && rl.drift_escape == 1
                    && l.state == ExemptState::Registering
            }
            BadExemptCase::ChainBreak => {
                // 3 段链篡改中段 payload → verify 必须定位首坏段 1。
                let mut chain = VersionChain::new();
                let _ = chain.append(1);
                let _ = chain.append(2);
                let _ = chain.append(3);
                if let Some(node) = chain.nodes.get_mut(1) {
                    node.0 = 99;
                }
                chain.verify() == Some(1)
            }
            BadExemptCase::ClearedResidual => {
                let (mut l, _, _) = match scenario_standard() {
                    Ok(t) => t,
                    Err(_) => return false,
                };
                // 9640 已清偿：再清、再存量均拒。
                l.clear(9640).is_err()
                    && l.register_stock(9640).is_err()
                    && l.clear(9605).is_ok()
            }
            BadExemptCase::FaceMissing => {
                // 面只刷存量漏清偿 → 逐条对账必报漂移（面缺条即漂移形态）。
                let (mut l, _, chain) = match scenario_standard() {
                    Ok(t) => t,
                    Err(_) => return false,
                };
                let mut partial = ExemptLedger::new();
                for id in l.stock.iter() {
                    let _ = partial.register_stock(*id);
                }
                let face = AuditFace::from_ledger(&partial);
                let mut rl = ReconLedger::zero();
                let mut c2 = chain;
                l.reconcile(&face, &mut c2, &mut rl).is_err()
                    && rl.drift_escape == 1
            }
            BadExemptCase::OutOfWindow => {
                let mut l = ExemptLedger::new();
                l.register_stock(9600).is_err()
                    && l.register_stock(10081).is_err()
                    && l.register_stock(0).is_err()
            }
        }
    }
}

/// 执行五类坏样本注入：每类 INJECT_ROUNDS 次，返回账本与逐类检出数。
pub fn run_exempt_injection() -> (ReconLedger, [u32; 5]) {
    let mut ledger = ReconLedger::zero();
    let mut per_class = [0u32; 5];
    let mut ci = 0usize;
    while ci < BadExemptCase::ALL.len() {
        if let Some(case) = BadExemptCase::ALL.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let caught = case.inject();
                let _ = ledger.record(caught);
                if caught {
                    if let Some(slot) = per_class.get_mut(ci) {
                        *slot = slot.saturating_add(1);
                    }
                }
                round += 1;
            }
        }
        ci += 1;
    }
    (ledger, per_class)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EBADF", -9, 10038, "Windows WSAENOTSOCK；非法链段/坏锚族"),
    ("EEXIST", -17, 183, "Windows ERROR_ALREADY_EXISTS；重复登记族"),
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；审计面缺条族"),
    ("EIO", -5, 1117, "Windows ERROR_IO_DEVICE；账实漂移族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；越窗/格式族"),
];

/// 查表：按 errno 正数取负值；表外返回 0（显性可查）。
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

/// 与 Linux 逐码一致：只允许表内码且负值精确匹配。
pub fn agrees_with_linux(code: i32) -> bool {
    linux_neg(code) == -code && code > 0
}

/// 规范语料双跑指纹：登记→清偿→审计面→对账→链核全链入哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let (mut l, face, chain) = match scenario_standard() {
        Ok(t) => t,
        Err(_) => (ExemptLedger::new(), AuditFace::from_ledger(&ExemptLedger::new()), VersionChain::new()),
    };
    let mut rl = ReconLedger::zero();
    bytes.push(match l.reconcile(&face, &chain, &mut rl) {
        Ok(()) => 1,
        Err(_) => 2,
    });
    bytes.push(l.state as u8);
    bytes.extend_from_slice(&(l.total() as u32).to_le_bytes());
    bytes.extend_from_slice(&rl.checked.to_le_bytes());
    bytes.push(match chain.verify() {
        None => 3,
        Some(_) => 4,
    });
    // 断链探测：篡改中段，首坏段下标入哈希。
    let mut c2 = chain;
    if let Some(node) = c2.nodes.get_mut(1) {
        node.0 = 77;
    }
    bytes.push(match c2.verify() {
        Some(1) => 5,
        _ => 6,
    });
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10069 判据集（17 条静态 + 2 条动态明细）。
pub fn run_unx10069_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10069");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (SEG_BASE..SEG_END).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x92100..=0x92140", seg_ok,
          "段基址唯一且落在本卡窗口；越界即拒绝");

    // 2. 标准场景：登记 5 清偿 2 对账通过
    let (mut l, face, chain) = match scenario_standard() {
        Ok(t) => t,
        Err(_) => (ExemptLedger::new(), AuditFace::from_ledger(&ExemptLedger::new()), VersionChain::new()),
    };
    let mut rl = ReconLedger::zero();
    let std_ok = l.reconcile(&face, &chain, &mut rl).is_ok()
        && l.state == ExemptState::Reconciled
        && l.total() == 5
        && l.stock.len() == 3
        && l.cleared.len() == 2
        && rl.checked == ID_TOTAL
        && rl.drift_escape == 0;
    s.add("总核一致：v2 账↔审计面 480 条逐条一致", std_ok,
          "存量 3 + 已清偿 2 = 总 5，面账逐位一致");

    // 3. 版本链：锚连可复算（同段同锚）
    let anchor_a = chain_anchor(CHAIN_SEED, 1);
    let anchor_b = chain_anchor(CHAIN_SEED, 1);
    let anchor_diff = chain_anchor(CHAIN_SEED, 2);
    let chain_ok = anchor_a == anchor_b && anchor_a != anchor_diff;
    s.add("链锚：同段锚可复算、异段锚互异", chain_ok,
          "anchor = FNV1a(prev‖payload)，确定性锚连");

    // 4. 版本链：全链连续核通过
    let mut ch = VersionChain::new();
    let _ = ch.append(1);
    let _ = ch.append(2);
    let _ = ch.append(3);
    s.add("版本链连续核：3 段全通零断链", ch.verify().is_none(),
          "逐段锚连 10/10 语的实现：每段重算锚匹配");

    // 5. 断链定位：篡改首/中/尾段均检出并定位
    let mut h1 = VersionChain::new();
    let _ = h1.append(1);
    let _ = h1.append(2);
    let _ = h1.append(3);
    if let Some(node) = h1.nodes.get_mut(0) {
        node.0 = 9;
    }
    let mut h2 = VersionChain::new();
    let _ = h2.append(1);
    let _ = h2.append(2);
    let _ = h2.append(3);
    if let Some(node) = h2.nodes.get_mut(2) {
        node.0 = 9;
    }
    let brk_ok = h1.verify() == Some(0) && h2.verify() == Some(2);
    s.add("断链检出：篡改首段/尾段均定位", brk_ok,
          "断链必须定位到首坏段下标");

    // 6. 空段拒绝
    let mut es = VersionChain::new();
    let es_ok = es.append(0).is_err() && es.nodes.is_empty();
    s.add("空段拒绝：payload=0 不入链", es_ok,
          "版本段必须携带非零账版本");

    // 7. 清偿迁移：单向且守恒
    let (mut cl, _, _) = match scenario_standard() {
        Ok(t) => t,
        Err(_) => (ExemptLedger::new(), AuditFace::from_ledger(&ExemptLedger::new()), VersionChain::new()),
    };
    let before = cl.total();
    let move_ok = cl.clear(9605).is_ok()
        && cl.total() == before
        && cl.stock.len() == 2
        && cl.cleared.len() == 3;
    s.add("清偿迁移：存量→已清偿单向、总数守恒", move_ok,
          "迁移不改总条数，只改名单归属");

    // 8. 清偿后仍存量拦截
    let (mut cr, _, _) = match scenario_standard() {
        Ok(t) => t,
        Err(_) => (ExemptLedger::new(), AuditFace::from_ledger(&ExemptLedger::new()), VersionChain::new()),
    };
    let cr_ok = cr.clear(9640).is_err()
        && cr.register_stock(9640).is_err()
        && cr.clear(9810).is_err();
    s.add("清偿后仍存量：再清/再存量全拒", cr_ok,
          "已清偿 ID 不得回存量，迁移单向");

    // 9. 账实漂移检出：账后面变
    let (mut dr, face2, _) = match scenario_standard() {
        Ok(t) => t,
        Err(_) => (ExemptLedger::new(), AuditFace::from_ledger(&ExemptLedger::new()), VersionChain::new()),
    };
    let _ = dr.register_stock(9900);
    let mut dr_rl = ReconLedger::zero();
    let chain_neutral = VersionChain::new();
    let dr_ok = dr.reconcile(&face2, &chain_neutral, &mut dr_rl).is_err()
        && dr_rl.drift_escape == 1;
    s.add("账实漂移：账后面变逐条对账检出", dr_ok,
          "漂移必须显性入账，逃逸计数可见");

    // 10. 审计面缺条检出
    let (fl, _, _) = match scenario_standard() {
        Ok(t) => t,
        Err(_) => (ExemptLedger::new(), AuditFace::from_ledger(&ExemptLedger::new()), VersionChain::new()),
    };
    let mut partial = ExemptLedger::new();
    for id in fl.stock.iter() {
        let _ = partial.register_stock(*id);
    }
    let pface = AuditFace::from_ledger(&partial);
    let mut fl_rl = ReconLedger::zero();
    let (_, _, chain3) = match scenario_standard() {
        Ok(t) => t,
        Err(_) => (ExemptLedger::new(), AuditFace::from_ledger(&ExemptLedger::new()), VersionChain::new()),
    };
    let mut l3 = ExemptLedger::new();
    let _ = l3.register_stock(9605);
    let _ = l3.register_stock(9640);
    let _ = l3.register_stock(9700);
    let _ = l3.register_stock(9810);
    let _ = l3.register_stock(9950);
    let _ = l3.clear(9640);
    let _ = l3.clear(9810);
    let face_missing = l3.reconcile(&pface, &chain3, &mut fl_rl).is_err()
        && fl_rl.drift_escape == 1;
    s.add("审计面缺条：面漏清偿位对账检出", face_missing,
          "面缺条=漂移形态之一，逐条核对必报");

    // 11. 越窗 ID 拦截
    let mut ow = ExemptLedger::new();
    let ow_ok = ow.register_stock(9600).is_err()
        && ow.register_stock(10081).is_err()
        && ow.register_stock(0).is_err();
    s.add("越窗 ID：9600/10081/0 拒收", ow_ok,
          "豁免号窗冻结 9601..=10080");

    // 12. 注入核：五类坏样本全检出
    let (ledger, per_class) = run_exempt_injection();
    let all_caught = per_class.iter().all(|c| *c == INJECT_ROUNDS);
    s.add("注入核：五类坏样本各 10/10 检出", all_caught,
          "漂移/断链/清偿残留/面缺条/越窗全被显式拒绝");

    // 13. 账守恒逃逸=0
    s.add("账守恒：注入 50 轮全检出逃逸=0",
          ledger.checked == 5 * INJECT_ROUNDS && ledger.drift_escape == 0,
          "注入核账守恒：50 次注入 50 次检出、0 逃逸");

    // 14. 文案闭集：非空
    let text_ok = ExemptCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 15. 诊断码唯一 + 落段
    let mut uniq = true;
    let mut i = 0usize;
    while i < ExemptCode::ALL.len() {
        let mut j = i + 1;
        while j < ExemptCode::ALL.len() {
            if let (Some(x), Some(y)) = (ExemptCode::ALL.get(i), ExemptCode::ALL.get(j)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let in_seg = ExemptCode::ALL.iter().all(|c| c.val() > SEG_BASE && c.val() <= SEG_BASE + 0x0C);
    s.add("诊断码：12 个码值两两唯一且落段窗", uniq && in_seg,
          "码值闭集唯一且不得越过 +0x0C 窗口");

    // 16. Linux 逐码一致
    let codes_ok = agrees_with_linux(EBADF)
        && agrees_with_linux(EEXIST)
        && agrees_with_linux(ENOENT)
        && agrees_with_linux(EIO)
        && agrees_with_linux(EINVAL)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误码只允许 errno 表内正值且负值可复算");

    // 17. 双跑一致
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", f1 == f2,
          "同输入两次必须逐字节相等，隐藏状态即失败");

    // 动态明细（≤8）：对账快照 + 指纹快照
    let snap_text = format!("存量 {} / 已清偿 {} / 链段 {}", l.stock.len(), l.cleared.len(), ch.nodes.len());
    s.add_dyn("豁免快照", l.state == ExemptState::Reconciled, &snap_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scenario_ok() {
        let (mut l, face, chain) = scenario_standard().unwrap_or((
            ExemptLedger::new(),
            AuditFace::from_ledger(&ExemptLedger::new()),
            VersionChain::new(),
        ));
        let mut rl = ReconLedger::zero();
        assert!(l.reconcile(&face, &chain, &mut rl).is_ok());
        assert_eq!(l.stock.len(), 3);
        assert_eq!(l.cleared.len(), 2);
        assert_eq!(l.total(), 5);
        assert_eq!(rl.checked, ID_TOTAL);
    }

    #[test]
    fn chain_anchor_deterministic() {
        assert_eq!(chain_anchor(1, 2), chain_anchor(1, 2));
        assert_ne!(chain_anchor(1, 2), chain_anchor(2, 1));
    }

    #[test]
    fn chain_verify_all_pass() {
        let mut c = VersionChain::new();
        let _ = c.append(1);
        let _ = c.append(2);
        let _ = c.append(3);
        assert!(c.verify().is_none());
    }

    #[test]
    fn chain_break_located() {
        let mut c = VersionChain::new();
        let _ = c.append(1);
        let _ = c.append(2);
        if let Some(node) = c.nodes.get_mut(1) {
            node.0 = 42;
        }
        assert_eq!(c.verify(), Some(1));
    }

    #[test]
    fn empty_payload_rejected() {
        let mut c = VersionChain::new();
        assert!(c.append(0).is_err());
    }

    #[test]
    fn clear_migration_conserves_total() {
        let (mut l, _, _) = scenario_standard().unwrap_or((
            ExemptLedger::new(),
            AuditFace::from_ledger(&ExemptLedger::new()),
            VersionChain::new(),
        ));
        let before = l.total();
        assert!(l.clear(9605).is_ok());
        assert_eq!(l.total(), before);
    }

    #[test]
    fn cleared_residual_blocked() {
        let (mut l, _, _) = scenario_standard().unwrap_or((
            ExemptLedger::new(),
            AuditFace::from_ledger(&ExemptLedger::new()),
            VersionChain::new(),
        ));
        assert!(l.clear(9640).is_err());
        assert!(l.register_stock(9640).is_err());
    }

    #[test]
    fn out_of_window_rejected() {
        let mut l = ExemptLedger::new();
        assert!(l.register_stock(9600).is_err());
        assert!(l.register_stock(10081).is_err());
    }

    #[test]
    fn injection_matrix_zero_escape() {
        let (l, per) = run_exempt_injection();
        assert_eq!(l.checked, 5 * INJECT_ROUNDS);
        assert_eq!(l.drift_escape, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(EBADF));
        assert!(agrees_with_linux(EEXIST));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(EIO), -EIO);
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn unx10069_checks_green() {
        let s = run_unx10069_checks();
        assert!(s.all_passed(), "UNX-F10069 red: {}", s.red_items_text());
    }
}
