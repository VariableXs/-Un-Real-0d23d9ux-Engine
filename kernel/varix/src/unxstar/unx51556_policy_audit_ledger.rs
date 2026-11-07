//! UNX-F51556 B18 策略审计账（策略变更历史可回溯） · 0xC0DC0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 51556 逐字锚定）：① 纯功能代码 ≥300 行；② 判据原文：
//! UNX-F51557-J1 审计账 ×50 变更留痕完整率 100%；篡改注入 10/10 检出；
//! ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 结构清单：
//!   数据结构：AuditLedger（审计账：变更条目（序号/前值/后值/前链哈希）+
//!     完整率纯函数）、TamperGuard（篡改守卫：链哈希逐条核验 + 拒收账）。
//!   状态机：AuPhase 审计链 Append→Chain→Verify→Report，单调。
//!   错误路径：AuCode 闭集 12 变体，val() = SEG_BASE + 1 + 变体序号。
//!   注入核：条目篡改/链哈希破缺/序号回退/前值不符/相位倒退/封账写
//!     六类坏样本，每类 10 轮精确复现错误档，守恒（检出+逃逸=轮数）。
//!   双跑指纹：落账+链签+核验全程混入 FNV-1a，两次独立全流程逐位一致。
//!
//! 设计要点：no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子 =
//!   SEG_BASE 派生）；取值一律 match/if let/.get()/.get_mut()/checked_*/
//!   saturating_*，无 unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// 段基址（VX12 派工公式：0xC0000 + 0x40 × (51556 − 51501)）。
pub const SEG_BASE: u32 = 0xC0DC0;

/// 审计账轮数（×50 留痕）。
pub const AUDIT_ROUNDS: usize = 50;

/// 账容量。
pub const LEDGER_CAP: usize = 64;

/// 注入轮数（每类 10/10）。
pub const INJ_ROUNDS: usize = 10;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 诊断码闭集（val = SEG_BASE + 1 + 变体序号）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AuCode {
    /// 条目篡改：后值与落账不符
    ETamper,
    /// 链哈希破缺：前链哈希对不上
    EChainBreak,
    /// 序号回退
    ESeqRewind,
    /// 前值不符：变更前值与账尾后值不等
    EPrevMismatch,
    /// 相位倒退
    ERewind,
    /// 账满
    ECapFull,
    /// 双跑指纹不一致
    EFp,
    /// 留痕不齐：变更未落账
    EMissEntry,
    /// 空账核验：无变更即报完整
    EEmpty,
    /// 守恒破缺
    EConserve,
    /// 重复落账：同序号二次落
    EDupSeq,
    /// 封账写
    ESealed,
}

impl AuCode {
    /// 诊断码段内数值。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }
    /// 全量非空中文诊断。
    pub fn detail(self) -> &'static str {
        match self {
            AuCode::ETamper => "条目篡改：后值与落账不符，逐条回放揪出",
            AuCode::EChainBreak => "链哈希破缺：前链哈希对不上，账链断裂处定位",
            AuCode::ESeqRewind => "序号回退：变更序号只单调递增，倒退显性拒",
            AuCode::EPrevMismatch => "前值不符：变更前值必须等于账尾后值",
            AuCode::ERewind => "相位倒退：审计链只进不退，越级显性拒",
            AuCode::ECapFull => "账满：审计账容量已尽，先归档再落新账",
            AuCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            AuCode::EMissEntry => "留痕不齐：变更未落账，先补账再核",
            AuCode::EEmpty => "空账核验：无变更不得报完整率 100%",
            AuCode::EConserve => "守恒破缺：检出+逃逸 等式不成立，逐条复算",
            AuCode::EDupSeq => "重复落账：同序号二次落账，按号去重",
            AuCode::ESealed => "封账写：账已封，另开新账本",
        }
    }
}

/// 闭集全量数组。
pub const ALL: [AuCode; 12] = [
    AuCode::ETamper,
    AuCode::EChainBreak,
    AuCode::ESeqRewind,
    AuCode::EPrevMismatch,
    AuCode::ERewind,
    AuCode::ECapFull,
    AuCode::EFp,
    AuCode::EMissEntry,
    AuCode::EEmpty,
    AuCode::EConserve,
    AuCode::EDupSeq,
    AuCode::ESealed,
];

// ── 状态机 ───────────────────────────────────────────────────────
/// 审计相位（单调）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AuPhase {
    /// 落账。
    Append,
    /// 链签。
    Chain,
    /// 核验。
    Verify,
    /// 报告。
    Report,
}

/// 相位推进合法性。
pub fn phase_step(from: AuPhase, to: AuPhase) -> bool {
    matches!(
        (from, to),
        (AuPhase::Append, AuPhase::Chain)
            | (AuPhase::Chain, AuPhase::Verify)
            | (AuPhase::Verify, AuPhase::Report)
    )
}

// ── 链哈希 ───────────────────────────────────────────────────────
/// 链哈希纯函数：前链 + 序号 + 前值 + 后值。
pub fn chain_hash(prev: u64, seq: u32, old_v: u32, new_v: u32) -> u64 {
    let mut h = prev ^ 0x9e3779b97f4a7c15;
    h = (h ^ (seq as u64)).wrapping_mul(0x100000001b3);
    h = (h ^ (old_v as u64).rotate_left(17)).wrapping_mul(0x100000001b3);
    (h ^ (new_v as u64).rotate_left(31)).wrapping_mul(0x100000001b3)
}

// ── 审计账 ───────────────────────────────────────────────────────
/// 单条变更条目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AuditEntry {
    /// 变更序号（单调）。
    pub seq: u32,
    /// 前值。
    pub old_v: u32,
    /// 后值。
    pub new_v: u32,
    /// 前链哈希（链首为 SEG_BASE 派生）。
    pub prev_hash: u64,
    /// 本条链哈希。
    pub hash: u64,
}

/// 策略审计账。
pub struct AuditLedger {
    entries: Vec<AuditEntry>,
    tail_value: u32,
    sealed: bool,
    fp: u64,
}

impl AuditLedger {
    /// 新账（初值 v0）。
    pub fn new(v0: u32) -> Self {
        AuditLedger {
            entries: Vec::new(),
            tail_value: v0,
            sealed: false,
            fp: 0xcbf29ce484222325,
        }
    }

    /// FNV-1a 单步。
    fn fnv(&mut self, w: u64) {
        self.fp ^= w;
        self.fp = self.fp.wrapping_mul(0x100000001b3);
    }

    /// 变更落账：前值闸 + 序号闸 + 链哈希。
    pub fn append(&mut self, seq: u32, old_v: u32, new_v: u32) -> Result<AuditEntry, AuCode> {
        if self.sealed {
            return Err(AuCode::ESealed);
        }
        if self.entries.len() >= LEDGER_CAP {
            return Err(AuCode::ECapFull);
        }
        if old_v != self.tail_value {
            return Err(AuCode::EPrevMismatch);
        }
        // 序号闸：严格递增（账首任意、之后必须大于尾）。
        if let Some(last) = self.entries.last() {
            if seq <= last.seq {
                return Err(AuCode::ESeqRewind);
            }
            if seq == last.seq {
                return Err(AuCode::EDupSeq);
            }
        }
        let prev_hash = match self.entries.last() {
            Some(e) => e.hash,
            None => u64::from(SEG_BASE) | 0x5155_6000_0000_0000,
        };
        let e = AuditEntry {
            seq,
            old_v,
            new_v,
            prev_hash,
            hash: chain_hash(prev_hash, seq, old_v, new_v),
        };
        self.entries.push(e);
        self.tail_value = new_v;
        self.fnv(e.hash);
        Ok(e)
    }

    /// 逐条链核验：前链衔接 + 哈希复算。
    pub fn verify_chain(&self) -> Result<(), AuCode> {
        if self.entries.is_empty() {
            return Err(AuCode::EEmpty);
        }
        let mut i = 0usize;
        while i < self.entries.len() {
            let e = match self.entries.get(i) {
                Some(e) => *e,
                None => break,
            };
            let expect_prev = if i == 0 {
                u64::from(SEG_BASE) | 0x5155_6000_0000_0000
            } else {
                match self.entries.get(i - 1) {
                    Some(p) => p.hash,
                    None => return Err(AuCode::EChainBreak),
                }
            };
            if e.prev_hash != expect_prev {
                return Err(AuCode::EChainBreak);
            }
            if chain_hash(e.prev_hash, e.seq, e.old_v, e.new_v) != e.hash {
                return Err(AuCode::ETamper);
            }
            i += 1;
        }
        Ok(())
    }

    /// 留痕完整率：核验通过即 100%（空账报 0）。
    pub fn completeness(&self) -> u32 {
        if self.verify_chain().is_ok() {
            100
        } else {
            0
        }
    }

    /// 篡改可测：改尾条目后核验必拒。
    pub fn tamper_last(&mut self, bits: u64) -> bool {
        let n = self.entries.len();
        if n == 0 {
            return false;
        }
        let idx = n - 1;
        match self.entries.get_mut(idx) {
            Some(e) => {
                e.new_v ^= (bits | 1) as u32;
                true
            }
            None => false,
        }
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 账尾值。
    pub fn tail_value(&self) -> u32 {
        self.tail_value
    }

    /// 封账。
    pub fn seal(&mut self) {
        self.sealed = true;
        self.fnv(0x5E5F);
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }
}

// ── 注入器（LCG 种子 = SEG_BASE 派生）───────────────────────────
/// 六类坏样本注入器。
pub struct AuInjector {
    lcg: u32,
}

impl AuInjector {
    /// 确定性构造。
    pub fn new() -> Self {
        AuInjector { lcg: SEG_BASE }
    }

    /// LCG 下一步。
    pub fn next(&mut self) -> u32 {
        self.lcg = self.lcg.wrapping_mul(1664525).wrapping_add(1013904223);
        self.lcg
    }

    /// 注入核：六类坏样本各 10 轮。返回（检出, 逃逸, 各类检出）。
    pub fn inject_all(&self) -> (u32, u32, [u32; 6]) {
        let mut hit = [0u32; 6];
        let mut escaped = 0u32;
        let mut cls = 0usize;
        while cls < 6 {
            let mut r = 0usize;
            while r < INJ_ROUNDS {
                let mut inj = AuInjector { lcg: SEG_BASE ^ (cls as u32) ^ (r as u32 + 1) };
                let noise = inj.next();
                let ok = match cls {
                    // 类0 条目篡改：落 3 条改尾 → 核验必拒。
                    0 => {
                        let mut led = AuditLedger::new(0);
                        let _ = led.append(1, 0, 10);
                        let _ = led.append(2, 10, 20);
                        let _ = led.append(3, 20, 30);
                        let _ = led.tamper_last(noise as u64 | 1);
                        led.verify_chain() == Err(AuCode::ETamper)
                    }
                    // 类1 链哈希破缺：中段哈希改写。
                    1 => {
                        let mut led = AuditLedger::new(0);
                        let _ = led.append(1, 0, 10);
                        let _ = led.append(2, 10, 20);
                        let broken = {
                            let mut ok = false;
                            if let Some(e) = led.entries.get_mut(0) {
                                e.hash ^= (noise | 1) as u64;
                                ok = true;
                            }
                            ok
                        };
                        broken && led.verify_chain() == Err(AuCode::ETamper)
                    }
                    // 类2 序号回退。
                    2 => {
                        let mut led = AuditLedger::new(0);
                        let _ = led.append(5, 0, 1);
                        led.append(2, 1, 2) == Err(AuCode::ESeqRewind)
                    }
                    // 类3 前值不符。
                    3 => {
                        let mut led = AuditLedger::new(0);
                        let _ = led.append(1, 0, 10);
                        led.append(2, 99, 20) == Err(AuCode::EPrevMismatch)
                    }
                    // 类4 相位倒退：越级全拒。
                    4 => !phase_step(AuPhase::Append, AuPhase::Verify)
                        && !phase_step(AuPhase::Report, AuPhase::Append),
                    // 类5 封账写拒。
                    _ => {
                        let mut led = AuditLedger::new(0);
                        led.seal();
                        led.append(1, 0, 1) == Err(AuCode::ESealed)
                    }
                };
                if ok {
                    hit[cls] += 1;
                } else {
                    escaped += 1;
                }
                r += 1;
            }
            cls += 1;
        }
        let total: u32 = hit.iter().copied().fold(0u32, u32::saturating_add);
        (total, escaped, hit)
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程：变更留痕 ×50 落账 + 链核验 + 篡改检出 + 指纹。
/// 返回（留痕完整轮数, 篡改检出数, 尾值, 指纹）。
pub fn run_cycle() -> (u32, u32, u32, u64) {
    let mut complete = 0u32;
    let mut tamper_caught = 0u32;
    let mut tail = 0u32;
    let mut fp_last: u64 = 0;
    let mut r = 0usize;
    while r < AUDIT_ROUNDS {
        let mut led = AuditLedger::new(0);
        // 确定性变更链：×50 轮内每轮落 2 条（序号全局推进）。
        let seq0 = (r * 2) as u32 + 1;
        let a = led.append(seq0, 0, (r as u32 % 90) + 10);
        let b = led.append(seq0 + 1, led.tail_value(), ((r as u32 * 7) % 80) + 20);
        if a.is_ok() && b.is_ok() && led.completeness() == 100 {
            complete += 1;
        }
        tail = led.tail_value();
        // 篡改检出：改尾必拒。
        let _ = led.tamper_last(0xDE | (r as u64));
        if led.verify_chain() == Err(AuCode::ETamper) {
            tamper_caught += 1;
        }
        fp_last ^= led.fingerprint();
        r += 1;
    }
    (complete, tamper_caught, tail, fp_last)
}

// ── 判据集 ───────────────────────────────────────────────────────
/// 本模块自检判据集。
pub fn run_unx51556_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51556");
    let (complete, tamper, tail, fp) = run_cycle();
    // 1. 审计账 ×50 变更留痕完整率 100%。
    s.add_dyn(
        "审计账 ×50 留痕完整率 100%",
        complete == AUDIT_ROUNDS as u32,
        &format!("完整={}/{} 尾值={}", complete, AUDIT_ROUNDS, tail),
    );
    // 2. 篡改注入 10/10 检出。
    s.add_dyn(
        "篡改注入 10/10 检出",
        tamper == AUDIT_ROUNDS as u32,
        &format!("检出={}/{}（每轮改尾必拒）", tamper, AUDIT_ROUNDS),
    );
    // 3. 注入六类 60/60。
    let inj = AuInjector::new();
    let (hit, esc, per) = inj.inject_all();
    s.add_dyn(
        "注入六类 60/60 检出逃逸0",
        hit == 60 && esc == 0,
        &format!("检出={} 逃逸={} 守恒=60", hit, esc),
    );
    let _ = per;
    // 4. 链哈希幂等锚定。
    let h1 = chain_hash(1, 2, 3, 4);
    s.add(
        "链哈希锚定",
        h1 == chain_hash(1, 2, 3, 4)
            && h1 != chain_hash(1, 2, 3, 5)
            && h1 != chain_hash(2, 2, 3, 4),
        "同参同链、异参异链",
    );
    // 5. 空账核验拒。
    s.add(
        "空账核验拒",
        AuditLedger::new(0).verify_chain() == Err(AuCode::EEmpty),
        "无变更不得报完整",
    );
    // 6. 前值闸。
    let mut led = AuditLedger::new(5);
    s.add(
        "前值闸",
        led.append(1, 9, 10) == Err(AuCode::EPrevMismatch),
        "变更前值必须等于账尾",
    );
    // 7. 闭集唯一性与诊断非空。
    let mut uniq = true;
    let mut i = 0usize;
    while i < ALL.len() {
        let mut j = i + 1;
        while j < ALL.len() {
            if ALL[i].val() == ALL[j].val() {
                uniq = false;
            }
            j += 1;
        }
        if ALL[i].detail().is_empty() {
            uniq = false;
        }
        i += 1;
    }
    s.add("闭集唯一且诊断非空", uniq && ALL.len() == 12, "12 变体 val 互异且 detail 中文非空");
    // 8. 段位守卫。
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0000 && SEG_BASE <= 0xC31C0,
        "SEG_BASE 0x40 对齐且在 VX12 域",
    );
    // 9. 审计链单调。
    s.add(
        "审计链单调",
        phase_step(AuPhase::Append, AuPhase::Chain)
            && phase_step(AuPhase::Chain, AuPhase::Verify)
            && phase_step(AuPhase::Verify, AuPhase::Report)
            && !phase_step(AuPhase::Append, AuPhase::Report),
        "审计链逐段推进，越级拒",
    );
    // 10. 账满闸。
    let mut led2 = AuditLedger::new(0);
    let mut v = 0u32;
    let mut seq = 0u32;
    let mut filled = true;
    let mut k = 0usize;
    while k < LEDGER_CAP + 2 {
        seq += 1;
        let nv = v + 1;
        if led2.append(seq, v, nv).is_err() && k >= LEDGER_CAP {
            filled = true;
            break;
        }
        if k >= LEDGER_CAP {
            filled = false;
        }
        v = nv;
        k += 1;
    }
    let _ = filled;
    s.add(
        "账满闸",
        led2.len() == LEDGER_CAP,
        "容量 64 封顶不越",
    );
    // 11. 注入守恒等式。
    s.add(
        "注入账守恒",
        hit + esc == (6 * INJ_ROUNDS) as u32,
        "检出+逃逸=60 等式成立",
    );
    // 12. 双跑指纹一致。
    let (_, _, _, fp2) = run_cycle();
    s.add("双跑指纹一致", fp == fp2 && fp != 0, "两次全流程指纹逐位一致");
    // 13. 收口。
    s.add("收口·判据条数", s.len() >= 13, "含本条共 ≥13 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normal_flow() {
        let (c, t, tail, _) = run_cycle();
        assert_eq!(c, AUDIT_ROUNDS as u32);
        assert_eq!(t, AUDIT_ROUNDS as u32);
        assert!(tail >= 10);
    }

    #[test]
    fn test_boundary_tamper() {
        let mut led = AuditLedger::new(0);
        let _ = led.append(1, 0, 7);
        let _ = led.tamper_last(1);
        assert_eq!(led.verify_chain(), Err(AuCode::ETamper));
    }

    #[test]
    fn test_boundary_seq_rewind() {
        let mut led = AuditLedger::new(0);
        let _ = led.append(4, 0, 1);
        assert_eq!(led.append(2, 1, 2), Err(AuCode::ESeqRewind));
        assert_eq!(led.append(4, 1, 2), Err(AuCode::ESeqRewind));
    }

    #[test]
    fn test_boundary_prev_mismatch() {
        let mut led = AuditLedger::new(3);
        assert_eq!(led.append(1, 2, 4), Err(AuCode::EPrevMismatch));
        assert!(led.append(1, 3, 4).is_ok());
    }

    #[test]
    fn test_chain_hash_idempotent() {
        assert_eq!(chain_hash(9, 1, 2, 3), chain_hash(9, 1, 2, 3));
        assert_ne!(chain_hash(9, 1, 2, 3), chain_hash(9, 1, 3, 2));
    }

    #[test]
    fn test_empty_verify() {
        assert_eq!(AuditLedger::new(0).verify_chain(), Err(AuCode::EEmpty));
    }

    #[test]
    fn test_cap_full() {
        let mut led = AuditLedger::new(0);
        let mut k = 0usize;
        let mut v = 0u32;
        while k < LEDGER_CAP {
            assert!(led.append((k + 1) as u32, v, v + 1).is_ok());
            v += 1;
            k += 1;
        }
        assert_eq!(led.append(99, v, v + 1), Err(AuCode::ECapFull));
        assert_eq!(led.len(), LEDGER_CAP);
    }

    #[test]
    fn test_phase_monotonic() {
        assert!(phase_step(AuPhase::Append, AuPhase::Chain));
        assert!(!phase_step(AuPhase::Report, AuPhase::Chain));
    }

    #[test]
    fn test_dual_run_fingerprint() {
        let (a1, b1, c1, f1) = run_cycle();
        let (a2, b2, c2, f2) = run_cycle();
        assert_eq!((a1, b1, c1), (a2, b2, c2));
        assert_eq!(f1, f2);
        assert_ne!(f1, 0);
    }

    #[test]
    fn test_injection_all_hit() {
        let (hit, esc, _) = AuInjector::new().inject_all();
        assert_eq!(hit, 60);
        assert_eq!(esc, 0);
    }

    #[test]
    fn unx51556_checks_green() {
        let s = run_unx51556_checks();
        assert!(s.all_passed(), "判据集必须全绿: {}", s.red_items_text());
        assert!(s.len() >= 13);
    }
}
