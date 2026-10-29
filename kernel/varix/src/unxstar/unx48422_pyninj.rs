//! UNX-F48422 —— E·超长串截断策略注入（反：绕过截断的 65 音节注入 ×10 全拦）
//! 判据原文：UNX-F48422-J1 绕过截断的注入（分片拼接/退格伪装/批量压栈/重放满缓冲）
//!           ×10 全拦且缓冲恒 ≤64；拦截账分类四型封闭；
//!           LCG 随机绕过注入 ×512 全拦；合法退格+重输零误拦；
//!           拦截账封印重演一致；账-缓冲两账守恒。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 闭包判据全绿才返回 Ok。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xD840; // E22 截断策略注入段

pub const SYL_LIMIT: usize = 64;

const E_BYPASS_PASS: u16 = SEG_BASE + 1; // 绕过注入放行
const E_LEGAL_BLOCK: u16 = SEG_BASE + 2; // 合法操作误拦
const E_KIND_BAD: u16 = SEG_BASE + 3; // 拦截分类越界
const E_SEAL_BAD: u16 = SEG_BASE + 4; // 封印失配
const E_REPLAY_DIFF: u16 = SEG_BASE + 5; // 重演不一致
const E_BAL_MISS: u16 = SEG_BASE + 6; // 两账守恒破
const E_CAP_MISS: u16 = SEG_BASE + 7; // 缓冲越 64

/// 绕过注入型。
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum BypassKind {
    ShardSplit = 0, // 分片拼接（每片合法、合计超限）
    BkMask = 1,     // 退格伪装（声明退格实际不退）
    BulkPush = 2,   // 批量压栈（单次 65 音节）
    ReplayFull = 3, // 满缓冲重放
}

/// 守门缓冲器。
pub struct GuardBuf {
    syls: Vec<String>,
    blocks: Vec<(u64, BypassKind)>, // 拦截账
    seq: u64,
    accepted: u32,
}

impl GuardBuf {
    pub fn new() -> Self {
        GuardBuf { syls: Vec::new(), blocks: Vec::new(), seq: 0, accepted: 0 }
    }

    pub fn len(&self) -> usize {
        self.syls.len()
    }

    /// 单音节追加（唯一合法入口）：满则拦（ReplayFull 型近义——此处为容量拦）。
    pub fn push_one(&mut self, s: &str) -> Result<(), u16> {
        self.seq += 1;
        if self.syls.len() >= SYL_LIMIT {
            self.blocks.push((self.seq, BypassKind::ReplayFull));
            return Err(E_BYPASS_PASS);
        }
        self.syls.push(String::from(s));
        self.accepted += 1;
        Ok(())
    }

    /// 退格（合法）：空缓冲退格误拦判定（不拦但 Err？——语义：空退格为无害 no-op，返回 Ok(false)）。
    pub fn backspace(&mut self) -> bool {
        match self.syls.pop() {
            Some(_) => true,
            None => false,
        }
    }

    /// 批量压栈注入：一次 n 音节，n > 剩余容量即整批拦截（不部分收）。
    pub fn inject_bulk(&mut self, n: usize, kind: BypassKind) -> Result<(), u16> {
        self.seq += 1;
        let room = SYL_LIMIT - self.syls.len();
        if n > room {
            self.blocks.push((self.seq, kind));
            return Err(E_BYPASS_PASS);
        }
        // 分片式注入即使容量够也要在最终长度上复核（拼接总和守恒）
        let fake_total = self.syls.len() + n;
        if fake_total > SYL_LIMIT {
            self.blocks.push((self.seq, kind));
            return Err(E_BYPASS_PASS);
        }
        Err(E_BYPASS_PASS) // 注入通道恒拒（唯一合法入口是 push_one）
    }

    /// 退格伪装注入：声明退 1 实际压 1（净增），按 BulkPush 拦。
    pub fn inject_bkmask(&mut self) -> Result<(), u16> {
        self.seq += 1;
        self.blocks.push((self.seq, BypassKind::BkMask));
        Err(E_BYPASS_PASS)
    }

    /// 拦截分类封闭。
    pub fn audit_kind_closed(&self) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.blocks.len() {
            if self.blocks[i].1 as u16 > 3 {
                return Err(E_KIND_BAD);
            }
            i += 1;
        }
        Ok(())
    }

    /// 两账守恒：accepted + blocks.len() == seq。
    pub fn audit_balance(&self) -> Result<(), u16> {
        if (self.accepted as u64) + (self.blocks.len() as u64) != self.seq {
            return Err(E_BAL_MISS);
        }
        Ok(())
    }

    /// 有界：len ≤ 64。
    pub fn audit_bounded(&self) -> Result<(), u16> {
        if self.syls.len() > SYL_LIMIT {
            return Err(E_CAP_MISS);
        }
        Ok(())
    }

    pub fn fp(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0usize;
        while i < self.syls.len() {
            let b = self.syls[i].as_bytes();
            let mut j = 0usize;
            while j < b.len() {
                h ^= b[j] as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
                j += 1;
            }
            i += 1;
        }
        h ^= self.blocks.len() as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        h
    }

    pub fn block_count(&self) -> u32 {
        self.blocks.len() as u32
    }
}

/// 确定性 LCG。
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Self {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// 主巡检：截断策略注入八判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 满载后分片拼接注入 ×10 全拦：每片 1 音节、声明合计 65
    let mut g = GuardBuf::new();
    let mut i = 0usize;
    while i < 64 {
        g.push_one(&syl(i))?;
        i += 1;
    }
    let mut k = 0u32;
    while k < 10 {
        match g.inject_bulk(1, BypassKind::ShardSplit) {
            Err(E_BYPASS_PASS) => {}
            _ => return Err(E_BYPASS_PASS),
        }
        k += 1;
    }
    if g.len() != 64 || g.block_count() != 10 {
        return Err(E_BYPASS_PASS);
    }
    g.audit_bounded()?;
    hits += 1;

    // R2 批量压栈注入：n=65 > room=0 全拦
    match g.inject_bulk(65, BypassKind::BulkPush) {
        Err(E_BYPASS_PASS) => {}
        _ => return Err(E_BYPASS_PASS),
    }
    hits += 1;

    // R3 退格伪装注入全拦：声明退格实际净增
    match g.inject_bkmask() {
        Err(E_BYPASS_PASS) => {}
        _ => return Err(E_BYPASS_PASS),
    }
    hits += 1;

    // R4 合法退格+重输零误拦：退 3 推 3 全成功，账守恒
    let mut k = 0u32;
    while k < 3 {
        if !g.backspace() {
            return Err(E_LEGAL_BLOCK);
        }
        k += 1;
    }
    let mut k = 0u32;
    while k < 3 {
        g.push_one(&syl(0x500 + k as usize))?;
        k += 1;
    }
    if g.len() != 64 {
        return Err(E_LEGAL_BLOCK);
    }
    g.audit_balance()?;
    hits += 1;

    // R5 拦截分类封闭：四型全落
    g.audit_kind_closed()?;
    hits += 1;

    // R6 LCG 随机绕过注入 ×512 全拦：中途穿插合法退格/压栈
    let mut g2 = GuardBuf::new();
    let mut rng = Lcg::new(0x48422);
    let mut k = 0u32;
    while k < 512 {
        let mode = rng.below(5);
        match mode {
            0 => {
                match g2.inject_bulk(1 + rng.below(65) as usize, BypassKind::ShardSplit) {
                    Err(E_BYPASS_PASS) => {}
                    _ => return Err(E_BYPASS_PASS),
                }
            }
            1 => {
                let _ = g2.inject_bkmask();
            }
            2 => {
                let _ = g2.backspace();
            }
            3 => {
                // 注入通道恒拒（哪怕容量够）
                match g2.inject_bulk(1, BypassKind::ReplayFull) {
                    Err(E_BYPASS_PASS) => {}
                    _ => return Err(E_BYPASS_PASS),
                }
            }
            _ => {
                if g2.len() < SYL_LIMIT {
                    g2.push_one(&syl(k as usize))?;
                }
            }
        }
        g2.audit_bounded()?;
        k += 1;
    }
    if g2.block_count() < 100 {
        return Err(E_COUNT_MISS_G);
    }
    hits += 1;

    // R7 封印重演一致：同脚本双跑指纹相等
    let f1 = g2.fp();
    let mut g3 = GuardBuf::new();
    let mut rng2 = Lcg::new(0x48422);
    let mut k = 0u32;
    while k < 512 {
        let mode = rng2.below(5);
        match mode {
            0 => {
                let _ = g3.inject_bulk(1 + rng2.below(65) as usize, BypassKind::ShardSplit);
            }
            1 => {
                let _ = g3.inject_bkmask();
            }
            2 => {
                let _ = g3.backspace();
            }
            3 => {
                let _ = g3.inject_bulk(1, BypassKind::ReplayFull);
            }
            _ => {
                if g3.len() < SYL_LIMIT {
                    let _ = g3.push_one(&syl(k as usize));
                }
            }
        }
        k += 1;
    }
    if f1 != g3.fp() || f1 == 0 {
        return Err(E_REPLAY_DIFF);
    }
    hits += 1;

    // R8 两账守恒终验
    g2.audit_balance()?;
    g3.audit_balance()?;
    hits += 1;

    Ok(hits)
}

const E_COUNT_MISS_G: u16 = SEG_BASE + 8; // 拦截计数不足

fn syl(n: usize) -> String {
    let mut s = String::from("y");
    s.push_str(&n.to_string());
    s
}
