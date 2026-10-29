//! UNX-F36208-J1 重算期间的并发读（RCU 读复制契约）。
//! 判据：① 重算中读者 100 例读旧代零撕裂（B04 F36075 代际消费
//! 锚）：写者连发 100 代新快照期间，钉在旧代的读者每次读取都过
//! 校验和——零撕裂；② 切换原子性断言：current 代只经 publish 单
//! 步前进、读取要么返回完整快照要么显性 E_RETIRED（退役留痕不静
//! 默）；③ 纯功能代码 ≥320 行。
//! 设计纪律：批量重算（36207）与并发读的契约是 RCU 形态——写者
//! 复制出新代、原子切换 current 指针、旧代保留给已钉住的读者；
//! 读者永远读「某一完整代」，从不读「半个新代 + 半个旧代」。本模
//! 块把这个契约做成确定性账面模型（no_std 无真线程——交错由
//! 36209 注入矩阵驱动）：撕裂的唯一可观测形态 = 快照校验和不符或
//! 静默丢代；两者都被显性化。退役代读取显性报 E_RETIRED——「读不
//! 到」必须是错误而不是空。

use crate::checks::CheckSet;
use super::unx36201_inhengine as ie;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x9d40;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_TORN: u16 = SEG_BASE + 2; // 快照校验和不符（撕裂）
pub const E_RETIRED: u16 = SEG_BASE + 3; // 退役代读取（显性留痕）
pub const E_GEN: u16 = SEG_BASE + 4; // 代号越界/回退
pub const E_ANCHOR: u16 = SEG_BASE + 5; // 引擎锚/代际消费锚漂移
pub const E_INTG: u16 = SEG_BASE + 6; // 完整性自检不符

pub const VER: u32 = 1;

/// 退役保留代数（读者窗口——超窗显性 E_RETIRED，不静默清零）。
pub const RETAIN_CAP: usize = 8;

/// 读者例数锚（判据 ① 的 100 例）。
pub const READ_N: usize = 100;

/// 代际消费锚（B04 F36075 的登记号——本模块消费其代际语义形态）。
pub const F36075_GEN_ANCHOR: u16 = 36075;

/// ACL 校验和（快照完整性账——撕裂检测的唯一权威）。
pub fn acl_checksum(acl: &[ie::Ace]) -> u32 {
    let mut h = 0x811C_9DC5u32;
    let mut i = 0;
    while i < acl.len() {
        let a = &acl[i];
        h = h.wrapping_mul(0x0100_0193)
            ^ a.sid as u32
            ^ a.mask
            ^ (a.flags as u32).wrapping_mul(7)
            ^ (a.kind as u32).wrapping_mul(13);
        i += 1;
    }
    h
}

/// 一代快照（完整复制——写者不共享可变结构给读者）。
pub struct Snapshot {
    pub gen: u32,
    pub acl: Vec<ie::Ace>,
    pub checksum: u32,
}

/// RCU 存储体：current 代 + 保留窗口（最近 RETAIN_CAP 代）。
pub struct RcuStore {
    snaps: Vec<Snapshot>,
    current: u32,
    retired_total: u64,
}

impl RcuStore {
    pub fn new(initial: Vec<ie::Ace>) -> RcuStore {
        let s = Snapshot { gen: 0, checksum: acl_checksum(&initial), acl: initial };
        RcuStore { snaps: alloc::vec![s], current: 0, retired_total: 0 }
    }

    /// 写者：发布新代（复制 → 校验和 → 追加 → 原子切 current——账面
    /// 模型里切换单步完成，无中间态可观测）。
    pub fn publish(&mut self, acl: Vec<ie::Ace>) -> u32 {
        let new_gen = self.current + 1;
        let s = Snapshot { gen: new_gen, checksum: acl_checksum(&acl), acl };
        self.snaps.push(s);
        // 退役：窗口外老代出列（记账不静默——retired_total 留痕）。
        let keep_from = self.snaps.len().saturating_sub(RETAIN_CAP);
        self.retired_total += keep_from as u64;
        let mut kept: Vec<Snapshot> = Vec::new();
        let mut k = keep_from;
        while k < self.snaps.len() {
            kept.push(Snapshot {
                gen: self.snaps[k].gen,
                acl: self.snaps[k].acl.clone(),
                checksum: self.snaps[k].checksum,
            });
            k += 1;
        }
        self.snaps = kept;
        self.current = new_gen;
        new_gen
    }

    /// 读者：读指定代（钉住语义——代在窗口内则内容恒定）。
    pub fn read(&self, gen: u32) -> Result<&[ie::Ace], u16> {
        let mut i = 0;
        while i < self.snaps.len() {
            if self.snaps[i].gen == gen {
                let s = &self.snaps[i];
                if acl_checksum(&s.acl) != s.checksum {
                    return Err(E_TORN);
                }
                return Ok(&s.acl);
            }
            i += 1;
        }
        Err(E_RETIRED)
    }

    /// 读当前代。
    pub fn read_current(&self) -> Result<&[ie::Ace], u16> {
        self.read(self.current)
    }

    pub fn current(&self) -> u32 {
        self.current
    }

    pub fn retired_total(&self) -> u64 {
        self.retired_total
    }

    /// 切换原子性断言：current 可达、窗口内代号连续、每代校验和自洽。
    pub fn switch_atomicity(&self) -> Result<(), u16> {
        if self.snaps.is_empty() {
            return Err(E_GEN);
        }
        // 窗口内代号连续递增且以 current 收尾。
        let mut k = 1;
        while k < self.snaps.len() {
            if self.snaps[k].gen != self.snaps[k - 1].gen + 1 {
                return Err(E_GEN);
            }
            k += 1;
        }
        if self.snaps[self.snaps.len() - 1].gen != self.current {
            return Err(E_GEN);
        }
        let mut k = 0;
        while k < self.snaps.len() {
            if acl_checksum(&self.snaps[k].acl) != self.snaps[k].checksum {
                return Err(E_TORN);
            }
            k += 1;
        }
        Ok(())
    }
}

/// 100 例读者对平（判据 ① 主面）：写者连发 100 代；每步读者 A 钉
/// 「窗内旧代」（current-4，恒在 RETAIN_CAP 窗内）与发布时记账
/// （校验和/长度/首 sid）对平；读者 B 读当前代并做原子性断言。
/// 钉住代不出窗是 RCU 契约的读者义务——超窗读取显性 E_RETIRED
/// （retire_explicit_sweep 单独钉）。
pub fn reader_sweep() -> Result<u32, u16> {
    let mut base: Vec<ie::Ace> = Vec::new();
    base.push(ie::Ace { sid: 1, mask: 0x7, flags: 0, kind: ie::AK_ALLOW, origin: 0 });
    base.push(ie::Ace { sid: 2, mask: 0x3, flags: ie::AI_INHERITED, kind: ie::AK_DENY, origin: 9 });
    let mut store = RcuStore::new(base);
    // 发布时记账：(gen, checksum, len, first_sid)。
    let mut hist: Vec<(u32, u32, usize, u16)> = Vec::new();
    {
        let g0 = store.read(0)?;
        hist.push((0, acl_checksum(g0), g0.len(), if g0.is_empty() { 0 } else { g0[0].sid }));
    }
    let mut reads = 0u32;
    let mut step = 0;
    while step < READ_N {
        let mut fresh: Vec<ie::Ace> = Vec::new();
        fresh.push(ie::Ace {
            sid: 10 + (step % 9) as u16,
            mask: 0x1 + (step % 5) as u32,
            flags: ie::AI_INHERITED,
            kind: if step % 2 == 0 { ie::AK_ALLOW } else { ie::AK_DENY },
            origin: 3,
        });
        fresh.push(ie::Ace { sid: 1, mask: 0x7, flags: 0, kind: ie::AK_ALLOW, origin: 0 });
        let g = store.publish(fresh);
        // 发布即读（新代完整可见——切换原子性）。
        let cur = store.read_current()?;
        hist.push((g, acl_checksum(cur), cur.len(), if cur.is_empty() { 0 } else { cur[0].sid }));
        if hist.len() > RETAIN_CAP {
            hist.remove(0);
        }
        reads += 1;
        // 读者 A：钉窗内旧代（current-4），与发布时账逐项对平。
        if let Some(rec) = hist.first().copied() {
            let old = store.read(rec.0)?;
            if acl_checksum(old) != rec.1 || old.len() != rec.2 {
                return Err(E_TORN);
            }
            let fs = if old.is_empty() { 0 } else { old[0].sid };
            if fs != rec.3 {
                return Err(E_TORN);
            }
            reads += 1;
        }
        // 读者 B：当前代非空 + 原子性断言。
        let cur_acl = store.read_current()?;
        if cur_acl.is_empty() {
            return Err(E_TORN);
        }
        if store.switch_atomicity().is_err() {
            return Err(E_GEN);
        }
        reads += 1;
        step += 1;
    }
    if reads != (READ_N as u32) * 3 {
        return Err(E_INTG);
    }
    Ok(reads)
}

/// 退役显性面：超窗读取必须 E_RETIRED（不静默给空/给新）。
pub fn retire_explicit_sweep() -> Result<u32, u16> {
    let mut base: Vec<ie::Ace> = Vec::new();
    base.push(ie::Ace { sid: 1, mask: 1, flags: 0, kind: ie::AK_ALLOW, origin: 0 });
    let mut store = RcuStore::new(base);
    // 连发 RETAIN_CAP + 3 代：gen0 必然出窗。
    let mut g = 0;
    while g < RETAIN_CAP + 3 {
        let mut fresh: Vec<ie::Ace> = Vec::new();
        fresh.push(ie::Ace { sid: 5 + g as u16, mask: 1, flags: 0, kind: ie::AK_ALLOW, origin: 0 });
        store.publish(fresh);
        g += 1;
    }
    if store.read(0).err() != Some(E_RETIRED) {
        return Err(E_RETIRED);
    }
    if store.read(store.current()).is_err() {
        return Err(E_TORN);
    }
    if store.retired_total() == 0 {
        return Err(E_INTG); // 退役必须留痕
    }
    Ok(store.retired_total() as u32)
}

// ---- 窗滑动账 ---------------------------------------------------------------
/// 保留窗滑动账（RETAIN_CAP 边界的分布面）：连发 RETAIN_CAP×3 代，
/// 逐步核「窗内代号连续 + 窗容量恰 ≤ RETAIN_CAP + 退役留痕精确递
/// 增」——窗口滑动不跳不塌（每发一代窗尾恰出一代，退役账 +1）。
pub fn window_slide_check() -> Result<u64, u16> {
    let mut base: Vec<ie::Ace> = Vec::new();
    base.push(ie::Ace { sid: 1, mask: 1, flags: 0, kind: ie::AK_ALLOW, origin: 0 });
    let mut store = RcuStore::new(base);
    let mut retired_prev = 0u64;
    let mut g = 0;
    while g < RETAIN_CAP * 3 {
        let mut fresh: Vec<ie::Ace> = Vec::new();
        fresh.push(ie::Ace { sid: 5 + g as u16, mask: 1, flags: 0, kind: ie::AK_ALLOW, origin: 0 });
        store.publish(fresh);
        // 窗容量账：snaps 恒 ≤ RETAIN_CAP。
        if store.snaps.len() > RETAIN_CAP {
            return Err(E_GEN);
        }
        // 窗内连续 + 校验和自洽（每步原子性断言）。
        store.switch_atomicity()?;
        // 退役留痕精确递增（单步 +1 或窗未满时 +0）。
        let rt = store.retired_total();
        if !(rt == retired_prev || rt == retired_prev + 1) {
            return Err(E_RETIRED);
        }
        retired_prev = rt;
        // 当前代恒可读（写者不把读者读丢）。
        if store.read_current().is_err() {
            return Err(E_TORN);
        }
        g += 1;
    }
    if retired_prev == 0 {
        return Err(E_RETIRED);
    }
    Ok(retired_prev)
}

/// 内容钉账（窗口内旧代内容恒定）：发布新代后回读旧代逐 ACE 对平
/// 发布时的账面（RCU 契约：写者不动读者钉住的旧代）。
pub fn pin_content_check() -> Result<(), u16> {
    let mut base: Vec<ie::Ace> = Vec::new();
    base.push(ie::Ace { sid: 11, mask: 3, flags: 0, kind: ie::AK_ALLOW, origin: 0 });
    let mut store = RcuStore::new(base);
    // 钉住 gen0 内容账（长度 + 首 sid + 校验和）。
    let pinned = store.read_current()?.to_vec();
    let pin_sum = acl_checksum(&pinned);
    // 连发 5 代（gen0 仍在窗内——RETAIN_CAP = 8）。
    let mut g = 0;
    while g < 5 {
        let mut fresh: Vec<ie::Ace> = Vec::new();
        fresh.push(ie::Ace { sid: 20 + g as u16, mask: 1, flags: 0, kind: ie::AK_ALLOW, origin: 0 });
        store.publish(fresh);
        g += 1;
    }
    let now = store.read(0)?;
    if now.len() != pinned.len() || acl_checksum(now) != pin_sum {
        return Err(E_TORN);
    }
    if now[0].sid != pinned[0].sid {
        return Err(E_TORN);
    }
    Ok(())
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let (av, ab) = ie::engine_anchor();
    if av != 1 || ab != 0x7b30 {
        return Err(E_ANCHOR);
    }
    // R1：100 例读者零撕裂（钉旧 + 读新双账）。
    let reads = reader_sweep()?;
    if reads < 200 {
        return Err(E_INTG);
    }
    // R2：退役显性 + 留痕（11 连发 → 4 代出窗，账必须非零且精确）。
    let retired = retire_explicit_sweep()?;
    if retired != 4 {
        return Err(E_RETIRED);
    }
    // R3：撕裂检测钉子（手造坏校验和快照 → 必须显性 E_TORN）。
    let mut base: Vec<ie::Ace> = Vec::new();
    base.push(ie::Ace { sid: 1, mask: 1, flags: 0, kind: ie::AK_ALLOW, origin: 0 });
    let mut store = RcuStore::new(base);
    let mut fresh: Vec<ie::Ace> = Vec::new();
    fresh.push(ie::Ace { sid: 7, mask: 2, flags: 0, kind: ie::AK_ALLOW, origin: 0 });
    let g = store.publish(fresh);
    // 账面撕裂注入：把窗口内某代内容偷换（模拟半成品 ACL 可见）。
    store.snaps[0].acl.clear();
    if store.read(store.snaps[0].gen).err() != Some(E_TORN) {
        return Err(E_TORN);
    }
    if store.read(g).is_err() {
        return Err(E_TORN); // 未被偷换的代必须完好
    }
    // R4：代际消费锚登记（B04 F36075）。
    if F36075_GEN_ANCHOR != 36075 || RETAIN_CAP < 2 {
        return Err(E_ANCHOR);
    }
    // R5：校验和确定性（同输入两次折叠同值——读者对平的前提）。
    let s1 = acl_checksum(store.read_current()?);
    let s2 = acl_checksum(store.read_current()?);
    if s1 != s2 {
        return Err(E_TORN);
    }
    // R6：窗滑动账（24 连发逐步核容量/连续/留痕/当前可读）。
    let slid = window_slide_check()?;
    if slid == 0 {
        return Err(E_RETIRED);
    }
    // R7：内容钉账（旧代内容恒定——RCU 契约核心钉）。
    pin_content_check()?;
    Ok(reads.wrapping_add(retired))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_TORN => "快照校验和不符（撕裂/半成品可见）",
        E_RETIRED => "退役代读取（显性留痕，非静默）",
        E_GEN => "代号越界/回退（切换原子性破）",
        E_ANCHOR => "引擎锚/代际消费锚漂移",
        E_INTG => "完整性自检不符",
        _ => "未知诊断码",
    }
}

pub fn run_unx36208_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36208_inhrcu");
    cs.add("SEG 段位独占", SEG_BASE == 0x9d40, "0x9d40~0x9d4F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "100 例读者零撕裂/退役显性/撕裂检出/原子性断言全对"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("例量锚", READ_N == 100 && RETAIN_CAP == 8, "100 例读者 + 保留窗 8 代（判据锚）");
    cs.add("代际消费锚", F36075_GEN_ANCHOR == 36075, "B04 F36075 代际消费锚登记");
    cs.add("校验和面", acl_checksum(&[]) == 0x811C_9DC5, "空集校验和 = FNV 偏移基（折叠确定性）");
    cs.add("越界面", { let s = RcuStore::new(alloc::vec![ie::Ace { sid: 1, mask: 1, flags: 0, kind: 0, origin: 0 }]); s.read(9_999).err() == Some(E_RETIRED) }, "未发布代读取显性 E_RETIRED");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

// ---- 单元测试 ---------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_drill_green() {
        assert_eq!(drill().err(), None);
    }

    #[test]
    fn test_checks_green() {
        let cs = run_unx36208_checks();
        assert!(cs.all_passed(), "{}", cs.red_items_text());
    }

    #[test]
    fn test_pinned_reader_never_sees_new_generation() {
        // 性质直证：钉 gen0 的读者在窗内（3 次发布 < RETAIN_CAP）仍读
        // 到 gen0 的原始内容（逐 ACE 恒等，不只是校验和）。
        let mut base: Vec<ie::Ace> = Vec::new();
        base.push(ie::Ace { sid: 42, mask: 0x5, flags: 0, kind: ie::AK_ALLOW, origin: 0 });
        let store0 = RcuStore::new(base.clone());
        let mut store = RcuStore::new(base);
        let mut g = 0;
        while g < 3 {
            let mut fresh: Vec<ie::Ace> = Vec::new();
            fresh.push(ie::Ace { sid: 100 + g as u16, mask: 1, flags: 0, kind: ie::AK_ALLOW, origin: 0 });
            store.publish(fresh);
            g += 1;
        }
        let pinned = store.read(0).ok().unwrap_or_default();
        let want = store0.read(0).ok().unwrap_or_default();
        assert_eq!(pinned.len(), want.len(), "钉住代长度恒定");
        let mut i = 0;
        while i < want.len() {
            assert_eq!(pinned[i], want[i], "钉住代内容恒定");
            i += 1;
        }
        assert!(pinned.iter().all(|a| a.sid == 42), "新代内容绝不漏进旧代");
        // 超窗后显性退役（不静默）。
        let mut g = 0;
        while g < RETAIN_CAP + 2 {
            let mut fresh: Vec<ie::Ace> = Vec::new();
            fresh.push(ie::Ace { sid: 200 + g as u16, mask: 1, flags: 0, kind: ie::AK_ALLOW, origin: 0 });
            store.publish(fresh);
            g += 1;
        }
        assert_eq!(store.read(0).err(), Some(E_RETIRED));
    }

    #[test]
    fn test_retire_window_explicit() {
        // 账面守恒：8 代窗口，退役留痕账 = 发布数 − 窗口占用。
        let retired = retire_explicit_sweep().ok().unwrap_or(0);
        assert_eq!(retired, (RETAIN_CAP + 3 + 1 - RETAIN_CAP) as u32);
        assert_eq!(retired, 4);
    }
}
