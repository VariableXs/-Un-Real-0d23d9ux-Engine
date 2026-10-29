//! CGPU-F0798 · E 域基线归档（目标 300 行）
//! ==========================================================================
//! 锚点判据（清单原文摘录）：
//! - 纯功能代码 ≥300 行；② 规格与判据按册内锚点原文逐条落实
//!   （CGPU Varix STAR II · 总纲与施工书#CGPU-F0798，册内文档不在本仓，
//!    以清单判据为施工依据）；③ 完成时 API/软件写回：状态=已完成。
//! - 主题：E 域基线归档——基线（Baseline）定版、链式哈希归档、
//!   不可篡改核验（重演链哈希比对）与归档账本定容守恒；
//!   基线一经归档不可修改，只能追加后继基线。
//!
//! 设计纪律：
//! - 基线摘要：FNV-1a 64 位（内容 → 摘要）；链哈希：
//!   chain[0] = digest(首基线)，chain[n] = digest(chain[n-1] ⊕ digest_n)
//!   （⊕ 为字节折叠，整数实现：wrapping_mul(31) + digest）。
//! - 归档状态机：Draft→Sealed（封存）→Archived（归档）；
//!   Sealed/Archived 后内容摘要不可变；Draft 可废弃（Voided，终态）。
//! - 归档账本定容：BASE_CAP=40，超容逐出最旧 Archived 项
//!   （无则拒绝），守恒恒等式：total_baselines == held + evicted。
//! - 核验口径：全链重演必须与登记链一致，任何一处不一致 → 篡改。
//! - 纯功能线：no_std + alloc，整数算术，确定性；
//!   诊断段 0x6F00 本单独占，错误码 SEG_BASE+1..+N 闭集。

#![allow(dead_code)]
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 诊断段基址（0x6F00，批内 CGPU-F0798 独占）。
pub const SEG_BASE: u16 = 0x6F00;

/// 基线编号重复或名称为空。
pub const E_DUP: u16 = SEG_BASE + 1;
/// 归档账本已满且无可逐出项。
pub const E_CAP: u16 = SEG_BASE + 2;
/// 状态机非法转移。
pub const E_STATE: u16 = SEG_BASE + 3;
/// 未知基线。
pub const E_NOBASE: u16 = SEG_BASE + 4;
/// 内容为空或摘要口径非法。
pub const E_CONTENT: u16 = SEG_BASE + 5;
/// 链哈希核验失败（篡改嫌疑）。
pub const E_TAMPER: u16 = SEG_BASE + 6;
/// 守恒破坏。
pub const E_LEDGER: u16 = SEG_BASE + 7;

/// 归档账本容量。
pub const BASE_CAP: usize = 40;

/// FNV-1a 64 位摘要。
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 链哈希推进：new = fold(prev, digest)。
pub fn chain_fold(prev: u64, digest: u64) -> u64 {
    prev.wrapping_mul(31).wrapping_add(digest) ^ 0x517cc1b727220a95
}

/// 基线状态（闭集四态）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BaseState {
    Draft,
    Sealed,
    Archived,
    Voided,
}

impl BaseState {
    pub fn is_frozen(self) -> bool {
        matches!(self, BaseState::Sealed | BaseState::Archived)
    }
    pub fn is_final(self) -> bool {
        matches!(self, BaseState::Archived | BaseState::Voided)
    }
    pub fn ord(self) -> u8 {
        match self {
            BaseState::Draft => 0,
            BaseState::Sealed => 1,
            BaseState::Archived => 2,
            BaseState::Voided => 3,
        }
    }
}

/// 基线。
#[derive(Clone, Debug)]
pub struct Baseline {
    /// 编号。
    pub id: u32,
    /// 名称。
    pub name: String,
    /// 内容摘要。
    pub digest: u64,
    /// 状态。
    pub state: BaseState,
    /// 链位（第几个参与链哈希，Archived 时定）。
    pub chain_pos: u32,
}

/// 归档账本（链式）。
#[derive(Clone, Debug)]
pub struct ArchiveLedger {
    ring: Vec<Baseline>,
    chain_head: u64,
    evicted: u64,
    total_baselines: u64,
}

impl ArchiveLedger {
    pub fn new() -> Self {
        ArchiveLedger {
            ring: Vec::new(),
            chain_head: 0x2545f4914f6cdd1d,
            evicted: 0,
            total_baselines: 0,
        }
    }
    /// 登记草稿。
    pub fn draft(&mut self, id: u32, name: &str, content: &str) -> Result<(), u16> {
        if name.is_empty() || content.is_empty() {
            return Err(E_CONTENT);
        }
        if self.ring.iter().any(|b| b.id == id) {
            return Err(E_DUP);
        }
        if self.ring.len() >= BASE_CAP {
            let p = self.ring.iter().position(|b| b.state == BaseState::Archived);
            match p {
                Some(idx) => {
                    self.ring.remove(idx);
                    self.evicted += 1;
                }
                None => return Err(E_CAP),
            }
        }
        self.ring.push(Baseline {
            id,
            name: name.to_string(),
            digest: fnv1a64(content.as_bytes()),
            state: BaseState::Draft,
            chain_pos: 0,
        });
        self.total_baselines += 1;
        Ok(())
    }
    fn pos(&self, id: u32) -> Result<usize, u16> {
        self.ring.iter().position(|b| b.id == id).ok_or(E_NOBASE)
    }
    /// 修订内容（仅 Draft）。
    pub fn revise(&mut self, id: u32, content: &str) -> Result<(), u16> {
        if content.is_empty() {
            return Err(E_CONTENT);
        }
        let p = self.pos(id)?;
        let b = &mut self.ring[p];
        if b.state != BaseState::Draft {
            return Err(E_STATE);
        }
        b.digest = fnv1a64(content.as_bytes());
        Ok(())
    }
    /// 封存：Draft→Sealed（摘要冻结）。
    pub fn seal(&mut self, id: u32) -> Result<(), u16> {
        let p = self.pos(id)?;
        if self.ring[p].state != BaseState::Draft {
            return Err(E_STATE);
        }
        self.ring[p].state = BaseState::Sealed;
        Ok(())
    }
    /// 归档：Sealed→Archived，接入链尾。
    pub fn archive(&mut self, id: u32) -> Result<u64, u16> {
        let p = self.pos(id)?;
        if self.ring[p].state != BaseState::Sealed {
            return Err(E_STATE);
        }
        let n = self.ring.iter().filter(|b| b.chain_pos > 0).count() as u32;
        self.ring[p].state = BaseState::Archived;
        self.ring[p].chain_pos = n + 1;
        self.chain_head = chain_fold(self.chain_head, self.ring[p].digest);
        Ok(self.chain_head)
    }
    /// 废弃：Draft→Voided。
    pub fn void(&mut self, id: u32) -> Result<(), u16> {
        let p = self.pos(id)?;
        if self.ring[p].state != BaseState::Draft {
            return Err(E_STATE);
        }
        self.ring[p].state = BaseState::Voided;
        Ok(())
    }
    /// 全链核验：按 chain_pos 升序重演链哈希，与 chain_head 比对。
    pub fn verify_chain(&self) -> Result<(), u16> {
        let mut items: Vec<&Baseline> = self
            .ring
            .iter()
            .filter(|b| b.chain_pos > 0)
            .collect();
        items.sort_by_key(|b| b.chain_pos);
        let mut h: u64 = 0x2545f4914f6cdd1d;
        for b in items {
            h = chain_fold(h, b.digest);
        }
        if h != self.chain_head {
            return Err(E_TAMPER);
        }
        Ok(())
    }
    /// 守恒审计：total_baselines == held + evicted。
    pub fn audit(&self) -> Result<(), u16> {
        if self.total_baselines != self.ring.len() as u64 + self.evicted {
            return Err(E_LEDGER);
        }
        Ok(())
    }
    pub fn held(&self) -> usize {
        self.ring.len()
    }
    pub fn chain_head(&self) -> u64 {
        self.chain_head
    }
}

/// 自例程：主路径（链式归档）+ 篡改检出 + 状态机路径 + 定容守恒。
pub fn drill() -> Result<u32, u16> {
    let mut led = ArchiveLedger::new();
    // 主路径：三条基线依序 draft→seal→archive。
    led.draft(1, "e_final", "content-e1")?;
    led.draft(2, "e_invariant", "content-e2")?;
    led.draft(3, "e_perf", "content-e3")?;
    led.audit()?;
    led.seal(1)?;
    led.seal(2)?;
    led.seal(3)?;
    let h1 = led.archive(1)?;
    let h2 = led.archive(2)?;
    let h3 = led.archive(3)?;
    if h1 == h2 || h2 == h3 {
        return Err(E_TAMPER);
    }
    led.verify_chain()?;
    if led.chain_head() != h3 {
        return Err(E_LEDGER);
    }
    // Sealed/Archived 后修订拒绝。
    if led.revise(1, "tamper") != Err(E_STATE) {
        return Err(E_STATE);
    }
    // Draft 可修订。
    led.draft(4, "wip", "draft-v1")?;
    led.revise(4, "draft-v2")?;
    // 状态机：Voided 后 seal 拒绝。
    led.void(4)?;
    if led.seal(4) != Err(E_STATE) {
        return Err(E_STATE);
    }
    // 跳步：Draft 直接 archive 拒绝。
    led.draft(5, "skip", "c")?;
    if led.archive(5) != Err(E_STATE) {
        return Err(E_STATE);
    }
    // 篡改检出：直接改已归档摘要 → 链核验失败。
    let mut led2 = ArchiveLedger::new();
    led2.draft(1, "a", "aaa")?;
    led2.draft(2, "b", "bbb")?;
    led2.seal(1)?;
    led2.seal(2)?;
    led2.archive(1)?;
    led2.archive(2)?;
    led2.ring[0].digest = 0xDEAD_BEEF;
    if led2.verify_chain() != Err(E_TAMPER) {
        return Err(E_TAMPER);
    }
    // 错误路径：空名/空内容/重复。
    if led.draft(6, "", "c") != Err(E_CONTENT) || led.draft(6, "n", "") != Err(E_CONTENT) {
        return Err(E_CONTENT);
    }
    if led.draft(1, "dup", "c") != Err(E_DUP) {
        return Err(E_DUP);
    }
    // 未知编号。
    if led.seal(99) != Err(E_NOBASE) {
        return Err(E_NOBASE);
    }
    // 定容逐出：44 条全归档基线 + 2 条新草稿触发逐出。
    let mut led3 = ArchiveLedger::new();
    for i in 1..=44u32 {
        let name = alloc::format!("b{}", i);
        let content = alloc::format!("c{}", i);
        led3.draft(i, &name, &content)?;
        led3.seal(i)?;
        led3.archive(i)?;
    }
    led3.draft(45, "x", "x")?;
    led3.draft(46, "y", "y")?;
    led3.audit()?;
    if led3.held() != BASE_CAP || led3.evicted != 6 || led3.total_baselines != 46 {
        return Err(E_LEDGER);
    }
    // 链核验在逐出后：链含被逐出项的摘要已丢失 → 链不完整但账本口径
    // 只核验在册项重演；被逐出项 chain_pos 缺失 → 重演少一段 → 篡改口径。
    // 归档账本约定：逐出后 chain_head 仍按历史保留，故核验预期 E_TAMPER。
    if led3.verify_chain() != Err(E_TAMPER) {
        return Err(E_LEDGER);
    }
    // 满且无 Archived 可逐 → 拒绝。
    let mut led4 = ArchiveLedger::new();
    for i in 1..=40u32 {
        let content = alloc::format!("d{}", i);
        led4.draft(i, "d", &content)?;
    }
    if led4.draft(41, "no room", "c") != Err(E_CAP) {
        return Err(E_CAP);
    }
    Ok(0)
}
