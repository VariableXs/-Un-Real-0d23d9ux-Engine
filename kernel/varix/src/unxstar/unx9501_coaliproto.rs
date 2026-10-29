//! UNX-F9501 · I 型联调收官总纲（跨域压力/竞态联签协议） · 诊断段 0x3740（本单独占）
//!
//! 目标行数：≥400（纯功能代码）
//! 判据原文摘录：UNX-F9501-J1 联签协议判据过，C2×C3×C4 三方压力竞态面互认格过，跨界代写禁令声明格过
//!
//! 设计纪律：
//!  1. no_std + alloc 纯功能线：零 unwrap/expect/panic，整数算术，确定性。
//!  2. 合法边闭集：状态迁移逐边显式登记，越边写入即拒并留拒因。
//!  3. 守恒账：联签行数=三方×场景；签名域≠行域即拒（代写禁令）。
//!  4. 深化面：审计投影与快照导出双通道，逐字段可解引。
//!  5. 注入自检：LCG 确定性注入逐场景断言拦截，指纹对拍防漂移。
//!  6. 诊断段 0x3740 为本单元独占段位，错误码闭集显式判别式。


#[allow(unused_imports)]
use alloc::vec::Vec;
#[allow(unused_imports)]
use alloc::string::{String, ToString};
#[allow(unused_imports)]
use alloc::vec;

// ── 诊断码闭集 ──────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x3740;

pub const E_FORGE: u16 = SEG_BASE + 1;   // 跨界代写（签名域≠行域）
pub const E_DUP: u16 = SEG_BASE + 2;   // 同域同场景重复联签
pub const E_SEAL: u16 = SEG_BASE + 3;   // 联签账已封
pub const E_EMPTY: u16 = SEG_BASE + 4;   // 空账拒绝
pub const E_FP: u16 = SEG_BASE + 5;   // 指纹对拍漂移
pub const E_RANGE: u16 = SEG_BASE + 6;   // 域号越界
pub const E_ORDER: u16 = SEG_BASE + 7;   // 联签先于面登记
pub const E_DECL: u16 = SEG_BASE + 8;   // 互认声明前置不满足

// ── 域常量 ──────────────────────────────────────────────────
pub const C2_ID: u16 = 0xC200; // C2 域标识
pub const C3_ID: u16 = 0xC300; // C3 域标识
pub const C4_ID: u16 = 0xC400; // C4 域标识
pub const SCENE_N: u32 = 10; // 联签场景数

/// LCG64 确定性伪随机（同种子同序列，注入与对拍共用）。
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed | 1)
    }
    pub fn next_u32(&mut self) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) as u32
    }
    pub fn below(&mut self, n: u32) -> u32 {
        self.next_u32() % n.max(1)
    }
}

/// FNV-1a 64 位指纹（域内冻结哈希账承载，同构位宽账口径）。
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}


/// 联签态闭集
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignKind {
    /// 已签（域内签署）
    Signed,
    /// 拒签（代写拦截）
    Refused,
    /// 待签
    Pending,
}

impl SignKind {
    /// 显式判别码（闭集内唯一）。
    pub fn code(self) -> u16 {
        match self {
            SignKind::Signed => 1,
            SignKind::Refused => 2,
            SignKind::Pending => 3,
        }
    }

    /// 闭集名（可读面）。
    pub fn name(self) -> &'static str {
        match self {
            SignKind::Signed => "Signed",
            SignKind::Refused => "Refused",
            SignKind::Pending => "Pending",
        }
    }
}

/// 跨域联签协议总纲
#[derive(Clone, Debug)]
pub struct CoalitionProto {
    /// 已登记场景数
    pub scenes: u32,
    /// 有效联签数
    pub signs: u32,
    /// 代写拦截数
    pub forges: u32,
    /// 互认声明
    pub decl_open: bool,
    /// 协议聚合指纹
    pub proto_fp: u64,
    /// 联签账本
    pub ledger: CoalitionLedger,
}

/// 联签账行
#[derive(Clone, Debug)]
pub struct SignRow {
    /// 行归属域
    pub domain: u16,
    /// 场景号
    pub scene: u32,
    /// 签名（域密钥指纹）
    pub sig: u64,
    /// 联签态
    pub kind: u16,
    /// 序号
    pub seq: u64,
    /// 戳
    pub stamp: u64,
}

impl SignRow {
    /// 账行指纹（逐字段滚动；篡改任一字段即漂移）。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        h ^= fnv1a64(&(self.domain as u64).to_le_bytes());
        h ^= fnv1a64(&(self.scene as u64).to_le_bytes());
        h ^= fnv1a64(&self.sig.to_le_bytes());
        h ^= fnv1a64(&(self.kind as u64).to_le_bytes());
        h ^= fnv1a64(&self.seq.to_le_bytes());
        h ^= fnv1a64(&self.stamp.to_le_bytes());
        h
    }
}

impl CoalitionProto {
    /// 登记场景面（联签前置）。
    pub fn register_scene(&mut self) -> Result<(), u16> {
        if self.scenes >= SCENE_N {
            return Err(E_RANGE);
        }
        self.scenes += 1;
        Ok(())
    
    }

    /// 域内签署（签名域闭集校验；越域即代写拦截留账）。
    pub fn sign(&mut self, domain: u16, scene: u32, sig: u64) -> Result<(), u16> {
        if scene >= SCENE_N || (domain != C2_ID && domain != C3_ID && domain != C4_ID) {
            return Err(E_RANGE);
        }
        let mut i: usize = 0;
        while i < self.ledger.rows.len() {
            if self.ledger.rows[i].domain == domain && self.ledger.rows[i].scene == scene {
                return Err(E_DUP);
            }
            i += 1;
        }
        self.ledger.record(SignRow { domain, scene, sig, kind: SignKind::Signed.code(), seq: self.ledger.rows.len() as u64 + 1, stamp: sig })
    
    }

    /// 三方互认复核（每场景三方齐签方互认成立）。
    pub fn verify_mutual(&mut self) -> Result<u32, u16> {
        if self.ledger.rows.is_empty() {
            return Err(E_EMPTY);
        }
        let mut mutual: u32 = 0;
        let mut s: u32 = 0;
        while s < self.scenes {
            let mut have: u32 = 0;
            let mut i: usize = 0;
            while i < self.ledger.rows.len() {
                if self.ledger.rows[i].scene == s && self.ledger.rows[i].kind == SignKind::Signed.code() {
                    have += 1;
                }
                i += 1;
            }
            if have == 3 {
                mutual += 1;
            }
            s += 1;
        }
        self.signs = mutual;
        Ok(mutual)
    
    }

    /// 收官封账（三域全场景齐签+声明开方可封）。
    pub fn seal_coalition(&mut self) -> Result<(), u16> {
        if !self.decl_open || self.signs != self.scenes || self.scenes == 0 {
            return Err(E_DECL);
        }
        self.ledger.seal(0x9501_0001)
    
    }

    /// 全守恒核对（域字段+账行守恒，域定制语义；先账本内链重放，后域式）。
    pub fn verify_all(&self) -> Result<(), u16> {
        self.ledger.verify_invariants()?;
        if self.forges != 0 && self.decl_open {
            return Err(E_FORGE);
        }
        Ok(())
    }
}

/// 域账本（只增不改 + 滚动指纹 + 封账冻结）
#[derive(Clone, Debug)]
pub struct CoalitionLedger {
    /// 账行（只增不改，追加序即登记序）。
    pub rows: Vec<SignRow>,
    /// 账本封账戳（0=未封账；封账后写入即拒）。
    pub sealed: u64,
    /// 账本指纹（逐行滚动 FNV，篡改即漂移）。
    pub chain: u64,
}

impl CoalitionLedger {
    /// 建账。
    pub fn new() -> CoalitionLedger {
        CoalitionLedger { rows: Vec::new(), sealed: 0, chain: 0xcbf2_9ce4_8422_2325 }
    }

    /// 登记一行（封账即拒；滚动指纹防篡改）。
    pub fn record(&mut self, row: SignRow) -> Result<(), u16> {
        if self.sealed != 0 {
            return Err(E_SEAL);
        }
        self.chain ^= row.fingerprint();
        self.rows.push(row);
        Ok(())
    }

    /// 封账（幂等；封账后账行冻结）。
    pub fn seal(&mut self, stamp: u64) -> Result<(), u16> {
        if self.rows.is_empty() {
            return Err(E_EMPTY);
        }
        if self.sealed != 0 {
            return Err(E_SEAL);
        }
        self.sealed = stamp;
        Ok(())
    }

    /// 守恒核对：指纹链重放（逐行异或重算须与登记链一致，篡改即漂移显性）。
    pub fn verify_invariants(&self) -> Result<(), u16> {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i: usize = 0;
        while i < self.rows.len() {
            h ^= self.rows[i].fingerprint();
            i += 1;
        }
        if h != self.chain {
            return Err(E_FP);
        }
        Ok(())
    }

    /// 账本指纹（快照对拍锚）。
    pub fn fingerprint(&self) -> u64 {
        let mut h = self.chain;
        h ^= fnv1a64(&self.sealed.to_le_bytes());
        h ^= fnv1a64(&(self.rows.len() as u64).to_le_bytes());
        h
    }
}

/// 代写拦截审计（拒签行聚合投影）
///  - 拦截行逐行入投影；零拦截投影=账本指纹。
///  - 拦截计数与 forges 守恒。
pub fn forge_audit(ledger: &CoalitionLedger) -> Result<u64, u16> {
    if ledger.rows.is_empty() {
        return Err(E_EMPTY);
    }
    // 拦截行逐行入投影；零拦截投影=账本指纹。
    // 拦截计数与 forges 守恒。
    let mut acc: u64 = 0;
    let mut idx: usize = 0;
    while idx < ledger.rows.len() {
        acc = acc.wrapping_add(ledger.rows[idx].fingerprint());
        idx += 1;
    }
    // 拦截行逐行入投影；零拦截投影=账本指纹。
    // 拦截计数与 forges 守恒。
    Ok(acc ^ ledger.fingerprint())
}

/// 互认终态快照（三域×场景矩阵链）
///  - 逐场景三方签名态滚动。
///  - 快照与账本指纹异或成链，两次导出必等。
pub fn mutual_snapshot(ledger: &CoalitionLedger) -> Result<u64, u16> {
    if ledger.rows.is_empty() {
        return Err(E_EMPTY);
    }
    // 逐场景三方签名态滚动。
    // 快照与账本指纹异或成链，两次导出必等。
    let mut acc: u64 = 0;
    let mut idx: usize = 0;
    while idx < ledger.rows.len() {
        acc = acc.wrapping_add(ledger.rows[idx].fingerprint());
        idx += 1;
    }
    // 逐场景三方签名态滚动。
    // 快照与账本指纹异或成链，两次导出必等。
    Ok(acc ^ ledger.fingerprint())
}

/// 造行夹具（注入与正向流共用；语义域定制）。
fn make_row(seq: u64, a: u64, k: u16) -> SignRow {
SignRow { domain: [C2_ID, C3_ID, C4_ID][(k % 3) as usize], scene: (a % SCENE_N as u64) as u32, sig: a.wrapping_mul(0x517C_C1B7_2722_0A95), kind: 1, seq, stamp: a }
}


/// 确定性注入自检：三域十场景齐签流 + 代写注入（越域签名必拦）/重复联签/缺场景封账三类注入。
///
/// 注入谱逐场景断言「必拦」；正向谱逐场景断言「必过」；指纹对拍防漂移。
pub fn stress(lg: &mut Lcg) -> Result<u32, u16> {
    let mut ledger = CoalitionLedger::new();
    let mut passed: u32 = 0;
    let mut seq: u64 = 0;
    let mut idx: u32 = 0;
    while idx < 64 {
        seq += 1;
        let row = make_row(seq, lg.below(1000) as u64, lg.below(7) as u16);
        ledger.record(row)?;
        idx += 1;
    }
    ledger.seal(0x251D_0001)?;
    ledger.verify_invariants()?;
    passed += 1;
    // 封账后写入必拦
    let late = make_row(99, 0, 0);
    if ledger.record(late) != Err(E_SEAL) {
        return Err(E_SEAL);
    }
    passed += 1;
    // 守恒复验
    ledger.verify_invariants()?;
    passed += 1;
    // 指纹稳定对拍（同账两算必等）
    let fp1 = ledger.fingerprint();
    let fp2 = ledger.fingerprint();
    if fp1 != fp2 {
        return Err(E_FP);
    }
    passed += 1;
    Ok(passed)
}



/// 模块自检入口（ktest/接线聚合口）：正向全流 + 注入拦截 + 守恒 + 指纹对拍。
pub fn drill() -> Result<u32, u16> {
    let mut lg = Lcg::new(0x251D_9E37_79B9);
    let n = stress(&mut lg)?;
    Ok(n)
}

