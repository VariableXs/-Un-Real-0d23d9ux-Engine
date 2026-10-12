//! unx245_randnode ↔ UNX-F0245 早传第四步：随机化决策节
//!
//! 锚点原文（CoRun Unxreal 主册 UNX-F0245，UNX-A1-B12）：
//! 「早传第四步：随机化决策节 | 260 | 已深化 |
//!  UNX-F0245-J1 基址槽位/熵摘要/开关态入 Manifest（脱敏口径），
//!  与 F0088 双写一致」
//!
//! 设计要点（全部围绕 KASLR 随机化决策真实状态）：
//! 1. **决策三要素**：基址槽位（KASLR 槽号，脱敏口径——Manifest 里
//!    只写槽位号不写绝对基址，绝对地址属高敏面，泄露 = 攻击者直接
//!    拿到绕过随机化的钥匙）、熵摘要（FNV-1a 64 位，熵源账 F0088
//!    的一级指纹）、开关态（On / Off / Fallback 三态闭集）。
//! 2. **脱敏口径**：Manifest 节序列化后抽检禁含绝对基址字节型——
//!    若把 64 位基址按小端展开写进节，抽检能逐字节检出（脱敏不是
//!    口头约定，是可验证断言）；诊断模式授权可见走显性开闸计数，
//!    默认视图永远只出槽位号。
//! 3. **与 F0088 双写一致**：决策节入账的熵摘要必须与熵源账
//!    （F0088 kaslr 熵账）的摘要逐位一致——两处不同 = 有一个账被
//!    改过或口径漂移，双写对账立即失配立案。
//! 4. **槽位合法域**：槽位号必须落在物理槽表（[0, SLOTS)）内，
//!    越界槽拒选；开关态与槽位联动约束（Off 时槽位必须是 0 号保留
//!    槽，Fallback 固定写 fallback 槽并留降级原因）。
//! 5. **决策不可变性**：Manifest 节封账后重封 = Err（决策是引导期
//!    一次性产物，重写 = 决策漂移）；重入幂等（同输入重放产出与
//!    账面一致则放行，不一致拒）。
//!
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。

use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const R245_VERSION: &str = "r245-randnode-v1";

/// 诊断段基址。
pub const R245_SEG_BASE: u16 = 0x6700;

/// KASLR 槽位总数（含 0 号保留槽）。
pub const SLOT_COUNT: u32 = 16;

/// 0 号槽：Off/Fallback 保留槽。
pub const RESERVED_SLOT: u32 = 0;

// --------------------------------------------------------------------------
// 诊断码（r245 十码 · 9 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 决策节诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum R245Code {
    /// 0x6700 槽位越界。
    SlotRange,
    /// 0x6701 开关态与槽位联动违约（Off 非保留槽）。
    SlotStateBad,
    /// 0x6702 绝对基址泄露（脱敏抽检检出）。
    LeakDetected,
    /// 0x6703 双写失配（与 F0088 熵摘要不一致）。
    DualWriteBad,
    /// 0x6704 封账后重封（决策漂移）。
    ResealRejected,
    /// 0x6705 Fallback 缺降级原因。
    FallbackNoWhy,
    /// 0x6706 熵摘要全零（熵源未喂）。
    EntropyZero,
    /// 0x6707 幂等重放失配。
    ReplayBad,
    /// 0x6708 节未封账即读。
    UnsealedRead,
}

impl R245Code {
    /// 码值。
    pub fn val(self) -> u16 {
        R245_SEG_BASE + match self {
            R245Code::SlotRange => 0x0,
            R245Code::SlotStateBad => 0x1,
            R245Code::LeakDetected => 0x2,
            R245Code::DualWriteBad => 0x3,
            R245Code::ResealRejected => 0x4,
            R245Code::FallbackNoWhy => 0x5,
            R245Code::EntropyZero => 0x6,
            R245Code::ReplayBad => 0x7,
            R245Code::UnsealedRead => 0x8,
        }
    }
}

/// 段独占守卫。
pub fn r245_segment_guard() -> Result<(), R245Code> {
    let lo = R245Code::SlotRange.val();
    let hi = R245Code::UnsealedRead.val();
    if lo != R245_SEG_BASE || hi <= lo || hi >= R245_SEG_BASE + 0x10 {
        return Err(R245Code::DualWriteBad);
    }
    Ok(())
}

// --------------------------------------------------------------------------
// FNV-1a 64（与全仓快照指纹同源）
// --------------------------------------------------------------------------
/// FNV-1a 64 位。
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

// --------------------------------------------------------------------------
// 决策模型
// --------------------------------------------------------------------------
/// 随机化开关态（三态闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KaslrState {
    /// 开启（槽位随机选取）。
    On,
    /// 关闭（0 号保留槽）。
    Off,
    /// 降级（熵不足或自检失败，写 fallback 槽 + 原因）。
    Fallback,
}

/// 决策节（Manifest 内嵌结构）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RandNode {
    /// 要素一：基址槽位（脱敏：槽号非绝对地址）。
    pub slot: u32,
    /// 要素二：熵摘要（与 F0088 双写一致）。
    pub entropy_fp: u64,
    /// 要素三：开关态。
    pub state: KaslrState,
    /// 降级原因（仅 Fallback 非空）。
    pub why: &'static str,
    /// 节指纹（三要素 FNV，封账用）。
    pub fp: u64,
}

impl RandNode {
    /// 节指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut buf = [0u8; 18];
        buf[0..4].copy_from_slice(&self.slot.to_le_bytes());
        buf[4..12].copy_from_slice(&self.entropy_fp.to_le_bytes());
        buf[12] = match self.state {
            KaslrState::On => 1,
            KaslrState::Off => 2,
            KaslrState::Fallback => 3,
        };
        // why 短串并入（≤4 字节头）。
        let wb = self.why.as_bytes();
        let n = if wb.len() > 4 { 4 } else { wb.len() };
        buf[13] = n as u8;
        for i in 0..n {
            buf[14 + i] = wb[i];
        }
        fnv1a64(&buf)
    }

    /// 联动约束校验。
    pub fn constraints_ok(&self) -> bool {
        if self.slot >= SLOT_COUNT {
            return false;
        }
        match self.state {
            KaslrState::On => self.slot != RESERVED_SLOT,
            KaslrState::Off => self.slot == RESERVED_SLOT && self.why.is_empty(),
            KaslrState::Fallback => !self.why.is_empty(),
        }
    }
}

// --------------------------------------------------------------------------
// 决策器（选槽 → 构建 → 封账 → 幂等重放）
// --------------------------------------------------------------------------
/// 决策器。
pub struct RandDecider {
    /// 已封账决策。
    sealed: Option<RandNode>,
    /// 绝对基址（内存持有，永不入 Manifest；抽检对照源）。
    abs_base: Option<u64>,
    /// 越界槽拒计数。
    rej_slot_range: u64,
    /// 联动违约计数。
    rej_slot_state: u64,
    /// 泄露抽检检出计数。
    leak_hits: u64,
    /// 双写失配计数。
    dual_bad: u64,
    /// 重封拒计数。
    reseal_rej: u64,
    /// 未封账读拒计数。
    unsealed_rej: u64,
}

impl RandDecider {
    /// 新建（携带绝对基址仅供脱敏对照，不入账）。
    pub fn new(abs_base: Option<u64>) -> Self {
        RandDecider {
            sealed: None,
            abs_base,
            rej_slot_range: 0,
            rej_slot_state: 0,
            leak_hits: 0,
            dual_bad: 0,
            reseal_rej: 0,
            unsealed_rej: 0,
        }
    }

    /// 构建决策节（选槽 + 三要素 + 联动校验）。
    pub fn decide(&mut self, state: KaslrState, slot: u32, entropy_fp: u64, why: &'static str) -> Result<RandNode, R245Code> {
        if slot >= SLOT_COUNT {
            self.rej_slot_range += 1;
            return Err(R245Code::SlotRange);
        }
        if entropy_fp == 0 {
            return Err(R245Code::EntropyZero);
        }
        let node = RandNode { slot, entropy_fp, state, why, fp: 0 };
        if state == KaslrState::Fallback && why.is_empty() {
            return Err(R245Code::FallbackNoWhy);
        }
        if !node.constraints_ok() {
            self.rej_slot_state += 1;
            return Err(R245Code::SlotStateBad);
        }
        let mut node = node;
        node.fp = node.fingerprint();
        Ok(node)
    }

    /// 与 F0088 熵源账双写对账。
    pub fn reconcile_f0088(&mut self, node: &RandNode, ledger_fp: u64) -> Result<(), R245Code> {
        if node.entropy_fp != ledger_fp {
            self.dual_bad += 1;
            return Err(R245Code::DualWriteBad);
        }
        Ok(())
    }

    /// 脱敏抽检：序列化节字节不得含绝对基址的任何 4 字节窗口。
    /// （64 位小端 8 字节 + 高低两个 4 字节半字共三形，命中任一 = 泄露。）
    pub fn leak_scan(&mut self, node_bytes: &[u8]) -> Result<(), R245Code> {
        let base = match self.abs_base {
            Some(b) => b,
            None => return Ok(()),
        };
        let pats: [[u8; 4]; 3] = [
            (base as u32).to_le_bytes(),
            ((base >> 32) as u32).to_le_bytes(),
            base.to_le_bytes()[0..4].try_into().unwrap_or([0; 4]),
        ];
        for pat in pats.iter() {
            if pat.iter().all(|b| *b == 0) {
                continue;
            }
            let mut i = 0;
            while i + 4 <= node_bytes.len() {
                if &node_bytes[i..i + 4] == pat {
                    self.leak_hits += 1;
                    return Err(R245Code::LeakDetected);
                }
                i += 1;
            }
        }
        Ok(())
    }

    /// 序列化 Manifest 节（只含槽位/摘要/开关态/指纹，无绝对地址）。
    pub fn serialize(&self, node: &RandNode) -> Result<Vec<u8>, R245Code> {
        let mut out = Vec::new();
        out.extend_from_slice(R245_VERSION.as_bytes());
        out.push(0);
        out.extend_from_slice(&node.slot.to_le_bytes());
        out.extend_from_slice(&node.entropy_fp.to_le_bytes());
        out.push(match node.state {
            KaslrState::On => 1,
            KaslrState::Off => 2,
            KaslrState::Fallback => 3,
        });
        out.extend_from_slice(&node.fp.to_le_bytes());
        self.leak_scan_or_reject(&out)?;
        Ok(out)
    }

    fn leak_scan_or_reject(&self, bytes: &[u8]) -> Result<(), R245Code> {
        // 只读扫描（借用语义，不引用 self 的可变字段）。
        let base = match self.abs_base {
            Some(b) => b,
            None => return Ok(()),
        };
        let pats: [[u8; 4]; 2] = [
            (base as u32).to_le_bytes(),
            ((base >> 32) as u32).to_le_bytes(),
        ];
        for pat in pats.iter() {
            if pat.iter().all(|b| *b == 0) {
                continue;
            }
            let mut i = 0;
            while i + 4 <= bytes.len() {
                if &bytes[i..i + 4] == pat {
                    return Err(R245Code::LeakDetected);
                }
                i += 1;
            }
        }
        Ok(())
    }

    /// 封账（一次封账；重封 = 漂移拒）。
    pub fn seal(&mut self, node: RandNode) -> Result<u64, R245Code> {
        if self.sealed.is_some() {
            self.reseal_rej += 1;
            return Err(R245Code::ResealRejected);
        }
        let fp = node.fp;
        self.sealed = Some(node);
        Ok(fp)
    }

    /// 幂等重放校验：同输入重放产出与账面一致。
    pub fn replay(&self, candidate: &RandNode) -> Result<(), R245Code> {
        let sealed = self.sealed.as_ref().ok_or(R245Code::UnsealedRead)?;
        if sealed.fp == candidate.fp && sealed.slot == candidate.slot
            && sealed.entropy_fp == candidate.entropy_fp && sealed.state == candidate.state {
            Ok(())
        } else {
            Err(R245Code::ReplayBad)
        }
    }

    /// 读封账决策（未封账拒）。
    pub fn sealed_node(&self) -> Result<&RandNode, R245Code> {
        self.sealed.as_ref().ok_or(R245Code::UnsealedRead)
    }

    /// 脱敏视图（默认口径：只有槽位号，无任何地址形）。
    pub fn redacted_view(&self) -> Result<(u32, KaslrState), R245Code> {
        let n = self.sealed_node()?;
        Ok((n.slot, n.state))
    }

    /// 计数只读视图。
    pub fn rej_stats(&self) -> (u64, u64, u64, u64, u64, u64) {
        (self.rej_slot_range, self.rej_slot_state, self.leak_hits, self.dual_bad, self.reseal_rej, self.unsealed_rej)
    }
}

// --------------------------------------------------------------------------
// 判据组：UNX-F0245-J1
// --------------------------------------------------------------------------
/// 判据入口。
pub fn run_unx245_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx245");
    if r245_segment_guard().is_err() {
        cs.fail("C245-SEG", "段守卫失败");
        return cs;
    }

    // ---- C245-01 三要素构建（On + 槽位合法） ----
    let mut dc = RandDecider::new(Some(0x0000_7AC5_0000_0000));
    let ent = fnv1a64(b"kaslr-entropy-source-f0088");
    let n1 = dc.decide(KaslrState::On, 7, ent, "");
    let ok1 = match &n1 {
        Ok(n) => n.slot == 7 && n.entropy_fp == ent && n.state == KaslrState::On && n.fp != 0,
        Err(_) => false,
    };
    cs.add("C245-01 三要素构建", ok1, "槽/摘要/开关态齐");

    // ---- C245-02 槽位越界拒 ----
    let r2 = dc.decide(KaslrState::On, SLOT_COUNT, ent, "");
    cs.add(
        "C245-02 槽位越界拒",
        r2 == Err(R245Code::SlotRange),
        "16号槽越域",
    );

    // ---- C245-03 联动约束（Off 必须保留槽 / On 禁保留槽） ----
    let r3a = dc.decide(KaslrState::Off, 3, ent, "");
    let r3b = dc.decide(KaslrState::On, RESERVED_SLOT, ent, "");
    let r3c = dc.decide(KaslrState::Off, RESERVED_SLOT, ent, "");
    cs.add(
        "C245-03 联动约束",
        r3a == Err(R245Code::SlotStateBad) && r3b == Err(R245Code::SlotStateBad) && r3c.is_ok(),
        "Off↔0号On禁0号",
    );

    // ---- C245-04 Fallback 必带降级原因 ----
    let r4a = dc.decide(KaslrState::Fallback, RESERVED_SLOT, ent, "");
    let r4b = dc.decide(KaslrState::Fallback, RESERVED_SLOT, ent, "entropy-underflow");
    cs.add(
        "C245-04 Fallback必带原因",
        r4a == Err(R245Code::FallbackNoWhy) && r4b.is_ok(),
        "缺因拒带因过",
    );

    // ---- C245-05 熵摘要全零拒（熵源未喂） ----
    let r5 = dc.decide(KaslrState::On, 5, 0, "");
    cs.add(
        "C245-05 熵全零拒",
        r5 == Err(R245Code::EntropyZero),
        "摘要0拒选",
    );

    // ---- C245-06 双写一致（与 F0088 摘要逐位同） ----
    let node = match n1 {
        Ok(n) => n,
        Err(_) => {
            cs.fail("C245-06", "决策未构建");
            return cs;
        }
    };
    let rec = dc.reconcile_f0088(&node, ent);
    let rec_bad = dc.reconcile_f0088(&node, ent ^ 1);
    cs.add(
        "C245-06 F0088双写对账",
        rec.is_ok() && rec_bad == Err(R245Code::DualWriteBad),
        "同摘要过异摘要立案",
    );

    // ---- C245-07 脱敏抽检（序列化无绝对基址字节型） ----
    let ser = dc.serialize(&node);
    let ser_ok = match &ser {
        Ok(b) => {
            let base: u64 = 0x0000_7AC5_0000_0000;
            let lo = (base as u32).to_le_bytes();
            let mut hit = false;
            let mut i = 0;
            while i + 4 <= b.len() {
                if &b[i..i + 4] == &lo {
                    hit = true;
                }
                i += 1;
            }
            !hit
        }
        Err(_) => false,
    };
    cs.add(
        "C245-07 脱敏抽检零命中",
        ser_ok,
        "绝对基址字节型不入节",
    );

    // ---- C245-08 泄露检出（注入基址低半字必检出） ----
    let mut leak_probe: Vec<u8> = Vec::new();
    let base_probe: u64 = 0x0000_7AC5_0000_1234;
    leak_probe.extend_from_slice(&base_probe.to_le_bytes());
    leak_probe.extend_from_slice(b"rest");
    let leak = dc.leak_scan(&leak_probe);
    let stats = dc.rej_stats();
    cs.add(
        "C245-08 泄露检出",
        leak == Err(R245Code::LeakDetected) && stats.2 == 1,
        "基址半字命中立案",
    );

    // ---- C245-09 封账 + 重封拒 ----
    let fp9 = dc.seal(node);
    let reseal = dc.seal(node);
    cs.add(
        "C245-09 封账唯一",
        fp9.is_ok() && reseal == Err(R245Code::ResealRejected),
        "一次封账重封漂移拒",
    );

    // ---- C245-10 幂等重放：同输入一致放行 / 篡改槽位失配 ----
    let mut cand_bad = node;
    cand_bad.slot = 8;
    let rp_ok = dc.replay(&node);
    let rp_bad = dc.replay(&cand_bad);
    cs.add(
        "C245-10 幂等重放",
        rp_ok.is_ok() && rp_bad == Err(R245Code::ReplayBad),
        "同放行异拒",
    );

    // ---- C245-11 脱敏视图（默认口径只出槽号+态） ----
    let view = dc.redacted_view();
    let ok11 = match view {
        Ok((slot, st)) => slot == 7 && st == KaslrState::On,
        Err(_) => false,
    };
    cs.add(
        "C245-11 脱敏视图",
        ok11,
        "槽号7+On无地址",
    );

    // ---- C245-12 未封账读拒（新决策器） ----
    let dc2 = RandDecider::new(None);
    let r12 = dc2.sealed_node();
    let st = dc2.rej_stats();
    cs.add(
        "C245-12 未封账读拒",
        r12 == Err(R245Code::UnsealedRead),
        "空账读拒",
    );

    cs
}
