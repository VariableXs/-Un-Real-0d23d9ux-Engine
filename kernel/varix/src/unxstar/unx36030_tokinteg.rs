//! unx36030_tokinteg — UNX-F36030 令牌校验和与损坏即隔离。
//!
//! 判据原文（UNX-F36030-J1）：
//!   内存翻转注入 ×10 全部校验失败 → 令牌作废 + 持有进程终止 + 审计事件，
//!   零「尽力修复」路径（X3/X10 母版纪律）。
//! 设计纪律：
//!   1. 校验和单源：FNV-1a 64 位覆盖令牌全部语义字段（不含校验和自身）；
//!      seal() 封印、verify() 重算比对——不匹配即损坏。
//!   2. 损坏即隔离三联动作（原子序，缺一即判据红）：
//!      ① invalidate（令牌作废，VALID→INVALID 单程态机）；
//!      ② 持有进程终止记账（ProcBook.kill 计数）；
//!      ③ 审计事件入账（AuditRing 事件槽 + 序号单调）。
//!   3. 零修复路径：verify 失配不回滚不重算不「尽力修复」——作废是唯一
//!      出口；作废后 verify 恒拒（隔离态单程不可逆）。
//!   4. 纯功能线：no_std，零 unwrap/expect/panic，确定性。

use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xf8c0;

// ── 诊断码闭集（0xf8c0~0xf8df，独占）─────────────────────────
const E_CSUM: u16 = SEG_BASE + 1; // 校验和失配（内存翻转检出）
const E_INVALID: u16 = SEG_BASE + 2; // 令牌已作废（隔离态恒拒）
const E_BAL: u16 = SEG_BASE + 3; // 三联账守恒破（作废/终止/审计不对平）
const E_STATE: u16 = SEG_BASE + 4; // 态机非法（未封印先验等）
const E_SEQ: u16 = SEG_BASE + 5; // 审计序号漂移

/// 令牌态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IntegState {
    Live = 1,    // 已封印未损坏
    Invalid = 2, // 已作废（单程）
}

/// 持有进程账（fake 面：终止记账）。
pub struct ProcBook {
    pub killed: u64,
    pub live_procs: u64,
}

impl ProcBook {
    pub const fn new() -> ProcBook {
        ProcBook { killed: 0, live_procs: 0 }
    }

    pub fn spawn(&mut self) {
        self.live_procs += 1;
    }

    pub fn kill(&mut self) -> Result<(), u16> {
        if self.live_procs == 0 {
            return Err(E_BAL);
        }
        self.live_procs -= 1;
        self.killed += 1;
        Ok(())
    }
}

/// 审计事件环（定容 + 单调序号）。
pub const AUD_CAP: usize = 64;

pub struct AuditRing {
    pub kinds: [u8; AUD_CAP],
    pub token_ids: [u32; AUD_CAP],
    pub seqs: [u64; AUD_CAP],
    pub len: usize,
    pub next_seq: u64,
    pub dropped: usize,
}

impl AuditRing {
    pub const fn new() -> AuditRing {
        AuditRing {
            kinds: [0; AUD_CAP],
            token_ids: [0; AUD_CAP],
            seqs: [0; AUD_CAP],
            len: 0,
            next_seq: 1,
            dropped: 0,
        }
    }

    pub fn push(&mut self, kind: u8, token_id: u32) -> u64 {
        if self.len >= AUD_CAP {
            self.dropped += 1;
            // 环满丢新保旧——但序号仍前进（单调账不破）。
            self.next_seq += 1;
            return self.next_seq - 1;
        }
        let s = self.next_seq;
        self.kinds[self.len] = kind;
        self.token_ids[self.len] = token_id;
        self.seqs[self.len] = s;
        self.next_seq += 1;
        self.len += 1;
        s
    }

    /// 序号单调连续校验。
    pub fn seq_monotonic(&self) -> bool {
        let mut i = 1;
        while i < self.len {
            if self.seqs[i] != self.seqs[i - 1] + 1 {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 审计事件类型（损坏隔离事件）。
pub const AUD_INVALIDATE: u8 = 1;

/// 受保护令牌（校验和面）。
#[derive(Clone, Copy)]
pub struct SealedToken {
    pub id: u32,
    pub owner_fp: u64,
    pub session: u32,
    pub il: u32,
    pub type_bits: u32,
    pub csum: u64,
    pub state: IntegState,
}

fn fnv_fields(t: &SealedToken) -> u64 {
    // 完整逐字节覆盖（指纹逃逸防线：任一字节翻转必失配）。
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let push = |bytes: &[u8], h: &mut u64| {
        let mut i = 0usize;
        while i < bytes.len() {
            *h ^= bytes[i] as u64;
            *h = h.wrapping_mul(0x0000_0100_0000_01b3);
            i += 1;
        }
    };
    push(&t.id.to_le_bytes(), &mut h);
    push(&t.owner_fp.to_le_bytes(), &mut h);
    push(&t.session.to_le_bytes(), &mut h);
    push(&t.il.to_le_bytes(), &mut h);
    push(&t.type_bits.to_le_bytes(), &mut h);
    h
}

impl SealedToken {
    /// 封印：重算校验和置 Live。
    pub fn seal(mut self) -> SealedToken {
        self.csum = fnv_fields(&self);
        self.state = IntegState::Live;
        self
    }
}

/// 损坏即隔离三联动作（唯一损坏出口；零修复路径）。
pub fn invalidate(
    t: &mut SealedToken,
    procs: &mut ProcBook,
    aud: &mut AuditRing,
) -> Result<(), u16> {
    if t.state == IntegState::Invalid {
        return Err(E_INVALID);
    }
    // ① 令牌作废（单程）。
    t.state = IntegState::Invalid;
    // ② 持有进程终止记账。
    procs.kill()?;
    // ③ 审计事件入账。
    aud.push(AUD_INVALIDATE, t.id);
    Ok(())
}

/// 校验：重算校验和比对；失配即三联隔离；已作废恒拒。
pub fn verify(
    t: &mut SealedToken,
    procs: &mut ProcBook,
    aud: &mut AuditRing,
) -> Result<(), u16> {
    if t.state == IntegState::Invalid {
        return Err(E_INVALID);
    }
    if t.csum != fnv_fields(t) {
        invalidate(t, procs, aud)?;
        return Err(E_CSUM);
    }
    Ok(())
}

/// 翻转注入器（确定性）：第 k 例翻转第 k 个语义字段的低位。
pub fn flip_bit(t: &mut SealedToken, k: usize) {
    match k % 5 {
        0 => t.id ^= 0x1,
        1 => t.owner_fp ^= 0x100,
        2 => t.session ^= 0x2,
        3 => t.il ^= 0x4,
        _ => t.type_bits ^= 0x1,
    }
}

/// 诊断释义。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_CSUM => "校验和失配：内存翻转检出即三联隔离",
        E_INVALID => "令牌已作废：隔离态恒拒（单程不可逆，零修复路径）",
        E_BAL => "三联账守恒破：作废/终止/审计不对平",
        E_STATE => "态机非法",
        E_SEQ => "审计序号漂移：非单调连续",
        _ => "未知诊断码",
    }
}

/// 确定性演练（J1 判据五段）。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    let mut procs = ProcBook::new();
    let mut aud = AuditRing::new();
    // R1：封印后校验通过（未翻转的 5 例健康面）。
    let mut i = 0;
    while i < 5 {
        procs.spawn();
        let base = SealedToken {
            id: 100 + i,
            owner_fp: 0xa11ce ^ (i as u64),
            session: 7,
            il: 8192,
            type_bits: 1,
            csum: 0,
            state: IntegState::Live,
        }
        .seal();
        let mut t = base;
        verify(&mut t, &mut procs, &mut aud)?;
        if t.state != IntegState::Live {
            return Err(E_STATE);
        }
        hits += 1;
        i += 1;
    }
    // R2：内存翻转注入 ×10 全部校验失败 → 三联隔离。
    let mut j = 0;
    while j < 10 {
        procs.spawn();
        let base = SealedToken {
            id: 200 + j,
            owner_fp: 0xb0b ^ (j as u64),
            session: 7 + j,
            il: 8192,
            type_bits: 1,
            csum: 0,
            state: IntegState::Live,
        }
        .seal();
        let mut t = base;
        flip_bit(&mut t, j as usize);
        if verify(&mut t, &mut procs, &mut aud) != Err(E_CSUM) {
            return Err(E_CSUM);
        }
        // ① 作废。
        if t.state != IntegState::Invalid {
            return Err(E_BAL);
        }
        // ③ 审计事件（下条断言在 R4 对平）。
        // ② 终止账（下条 R3 对平）。
        hits += 1;
        j += 1;
    }
    // R3：持有进程终止账对平（10 例翻转各 kill 1 进程）。
    if procs.killed != 10 {
        return Err(E_BAL);
    }
    hits += 1;
    // R4：审计事件账对平（10 事件 + 序号单调连续）。
    if aud.len != 10 || !aud.seq_monotonic() {
        return Err(E_SEQ);
    }
    let mut k = 0;
    while k < aud.len {
        if aud.kinds[k] != AUD_INVALIDATE || aud.seqs[k] != (k + 1) as u64 {
            return Err(E_SEQ);
        }
        k += 1;
    }
    hits += 1;
    // R5：作废后 verify 恒拒（隔离态单程）+ 未封印非法。
    let mut dead = SealedToken {
        id: 300,
        owner_fp: 0xc0de,
        session: 7,
        il: 8192,
        type_bits: 1,
        csum: 0,
        state: IntegState::Invalid,
    };
    if verify(&mut dead, &mut procs, &mut aud) != Err(E_INVALID) {
        return Err(E_INVALID);
    }
    let unsealed = SealedToken {
        id: 301,
        owner_fp: 0xc0de,
        session: 7,
        il: 8192,
        type_bits: 1,
        csum: 0,
        state: IntegState::Live,
    };
    // csum=0 未封印——verify 必失配即隔离（无「未封印先过」路径）。
    let mut t2 = unsealed;
    procs.spawn();
    if verify(&mut t2, &mut procs, &mut aud) != Err(E_CSUM) {
        return Err(E_CSUM);
    }
    hits += 1;
    Ok(hits)
}

/// 隔离面视图辅助。

/// 审计环快照（只读视图：len + 逐条 (kind, token_id) 投影）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AuditSnapshot {
    pub len: usize,
    pub kinds: [u8; AUD_CAP],
    pub token_ids: [u32; AUD_CAP],
}

impl AuditRing {
    /// 快照导出。
    pub fn snapshot(&self) -> AuditSnapshot {
        let mut ks = [0u8; AUD_CAP];
        let mut ids = [0u32; AUD_CAP];
        let mut i = 0;
        while i < self.len {
            ks[i] = self.kinds[i];
            ids[i] = self.token_ids[i];
            i += 1;
        }
        AuditSnapshot { len: self.len, kinds: ks, token_ids: ids }
    }

    /// 快照守恒核。
    pub fn snapshot_conserve(&self, s: &AuditSnapshot) -> bool {
        if s.len != self.len {
            return false;
        }
        let mut i = 0;
        while i < self.len {
            if s.kinds[i] != self.kinds[i] || s.token_ids[i] != self.token_ids[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

pub fn run_unx36030_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36030_tokinteg");
    cs.add("SEG 段位独占", SEG_BASE == 0xf8c0, "0xf8c0~0xf8df 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 校验和与损坏隔离", true, "翻转 ×10 三联隔离+零修复路径"),
        Err(code) => cs.add("J1 校验和与损坏隔离", false, detail(code)),
    }
    cs
}

#[cfg(test)]
mod tests {
    use super::drill;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(18));
    }
}
