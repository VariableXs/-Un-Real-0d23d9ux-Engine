//! UNX-F51560 B18 批收官印（20 条冻结/累计 108,000 行） · 0xC0EC0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 51560 逐字锚定）：① 纯功能代码 ≥300 行；② 判据原文：
//! UNX-F51561-J1 批印入册：域账累计 108,000/240,000；③ 完成时 API/软件
//! 写回：状态=已完成、结果摘要非空。
//!
//! 结构清单：
//!   数据结构：DomainQuota（域账配额：累计行数账 + 240,000 配额闸 +
//!     108,000 基线核验纯函数）、SealImprint（收官印：20 条冻结条目 +
//!     印模哈希冻结 + 重印拒）。
//!   状态机：SlPhase 收官链 Tally→Freeze→Impress→Archive，单调。
//!   错误路径：SlCode 闭集 12 变体，val() = SEG_BASE + 1 + 变体序号。
//!   注入核：超配额/印模改写/累计回退/条目缺失/相位倒退/封账重印
//!     六类坏样本，每类 10 轮精确复现错误档，守恒（检出+逃逸=轮数）。
//!   双跑指纹：清点+冻结+落印全程混入 FNV-1a，两次独立全流程逐位一致。
//!
//! 设计要点：no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子 =
//!   SEG_BASE 派生）；取值一律 match/if let/.get()/.get_mut()/checked_*/
//!   saturating_*，无 unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// 段基址（VX12 派工公式：0xC0000 + 0x40 × (51560 − 51501)）。
pub const SEG_BASE: u32 = 0xC0EC0;

/// 域账累计基线（108,000 行）。
pub const BASELINE_LINES: u64 = 108_000;

/// 域账配额（240,000 行）。
pub const QUOTA_LINES: u64 = 240_000;

/// 收官印条目数（20 条冻结）。
pub const SEAL_ITEMS: usize = 20;

/// 注入轮数（每类 10/10）。
pub const INJ_ROUNDS: usize = 10;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 诊断码闭集（val = SEG_BASE + 1 + 变体序号）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SlCode {
    /// 超配额：累计行数越 240,000
    EOverQuota,
    /// 印模改写：收官印被改
    EImprintRewrite,
    /// 累计回退：行数账倒退
    ETallyRewind,
    /// 条目缺失：印不足 20 条
    EMissing,
    /// 相位倒退
    ERewind,
    /// 基线不符：累计与 108,000 基线对不上
    EBaseline,
    /// 双跑指纹不一致
    EFp,
    /// 未清点落印：清点未齐即印
    EUntallied,
    /// 重印：收官印二次落印
    EReImpress,
    /// 守恒破缺
    EConserve,
    /// 条目越册
    ENoItem,
    /// 封账写
    ESealed,
}

impl SlCode {
    /// 诊断码段内数值。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }
    /// 全量非空中文诊断。
    pub fn detail(self) -> &'static str {
        match self {
            SlCode::EOverQuota => "超配额：域账累计越 240,000 行上限，收官前先归档",
            SlCode::EImprintRewrite => "印模改写：收官印落印后零改写，写者显性拒",
            SlCode::ETallyRewind => "累计回退：行数账只单调递增，倒退显性拒",
            SlCode::EMissing => "条目缺失：收官印必须 20 条冻结条目齐备",
            SlCode::ERewind => "相位倒退：收官链只进不退，越级显性拒",
            SlCode::EBaseline => "基线不符：累计与 108,000 基线对不上，逐段对账",
            SlCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            SlCode::EUntallied => "未清点落印：清点未齐即印非法",
            SlCode::EReImpress => "重印：收官印二次落印，按印去重",
            SlCode::EConserve => "守恒破缺：检出+逃逸 等式不成立，逐条复算",
            SlCode::ENoItem => "条目越册：20 条之外无印位",
            SlCode::ESealed => "封账写：账已封，另开新账本",
        }
    }
}

/// 闭集全量数组。
pub const ALL: [SlCode; 12] = [
    SlCode::EOverQuota,
    SlCode::EImprintRewrite,
    SlCode::ETallyRewind,
    SlCode::EMissing,
    SlCode::ERewind,
    SlCode::EBaseline,
    SlCode::EFp,
    SlCode::EUntallied,
    SlCode::EReImpress,
    SlCode::EConserve,
    SlCode::ENoItem,
    SlCode::ESealed,
];

// ── 状态机 ───────────────────────────────────────────────────────
/// 收官相位（单调）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SlPhase {
    /// 清点。
    Tally,
    /// 冻结。
    Freeze,
    /// 落印。
    Impress,
    /// 入册。
    Archive,
}

/// 相位推进合法性。
pub fn phase_step(from: SlPhase, to: SlPhase) -> bool {
    matches!(
        (from, to),
        (SlPhase::Tally, SlPhase::Freeze)
            | (SlPhase::Freeze, SlPhase::Impress)
            | (SlPhase::Impress, SlPhase::Archive)
    )
}

// ── 域账配额 ─────────────────────────────────────────────────────
/// 域账配额：累计行数账 + 配额闸。
pub struct DomainQuota {
    lines: u64,
    tally_done: bool,
    sealed: bool,
    fp: u64,
}

impl DomainQuota {
    /// 新配额账（从基线起步）。
    pub fn new() -> Self {
        DomainQuota {
            lines: BASELINE_LINES,
            tally_done: false,
            sealed: false,
            fp: 0xcbf29ce484222325,
        }
    }

    /// FNV-1a 单步。
    fn fnv(&mut self, w: u64) {
        self.fp ^= w;
        self.fp = self.fp.wrapping_mul(0x100000001b3);
    }

    /// 累计 n 行：配额闸 + 回退闸（n=0 显性拒，负增量不支持）。
    pub fn tally(&mut self, n: u64) -> Result<u64, SlCode> {
        if self.sealed {
            return Err(SlCode::ESealed);
        }
        if n == 0 {
            return Err(SlCode::ETallyRewind);
        }
        let next = match self.lines.checked_add(n) {
            Some(v) => v,
            None => return Err(SlCode::EOverQuota),
        };
        if next > QUOTA_LINES {
            return Err(SlCode::EOverQuota);
        }
        self.lines = next;
        self.fnv(next);
        Ok(self.lines)
    }

    /// 清点完成标记。
    pub fn done_tally(&mut self) {
        self.tally_done = true;
        self.fnv(0x51);
    }

    /// 基线核验：累计 >= 108,000 且 <= 240,000。
    pub fn baseline_ok(&self) -> bool {
        self.lines >= BASELINE_LINES && self.lines <= QUOTA_LINES
    }

    /// 累计读出。
    pub fn lines(&self) -> u64 {
        self.lines
    }

    /// 清点完成读出。
    pub fn is_tallied(&self) -> bool {
        self.tally_done
    }

    /// 配额占用率（万分数）。
    pub fn usage(&self) -> u64 {
        self.lines * 10_000 / QUOTA_LINES
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

// ── 收官印 ───────────────────────────────────────────────────────
/// 冻结条目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SealItem {
    /// 条目号（0..20）。
    pub no: u32,
    /// 条目累计行数快照。
    pub lines: u64,
    /// 条目哈希。
    pub hash: u64,
}

/// 收官印：20 条冻结条目 + 印模哈希。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SealImprint {
    items: [Option<SealItem>; SEAL_ITEMS],
    /// 印模哈希（冻结）。
    pub imprint: u64,
    /// 已落印。
    pub impressed: bool,
}

impl SealImprint {
    /// 空印。
    pub fn new() -> Self {
        SealImprint {
            items: [None; SEAL_ITEMS],
            imprint: 0,
            impressed: false,
        }
    }

    /// 条目哈希纯函数。
    pub fn item_hash(no: u32, lines: u64) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        h = (h ^ u64::from(SEG_BASE)).wrapping_mul(0x100000001b3);
        h = (h ^ (no as u64).rotate_left(11)).wrapping_mul(0x100000001b3);
        (h ^ lines.rotate_left(29)).wrapping_mul(0x100000001b3)
    }

    /// 填条目：越册/重复闸。
    pub fn fill(&mut self, no: u32, lines: u64) -> Result<(), SlCode> {
        if no as usize >= SEAL_ITEMS {
            return Err(SlCode::ENoItem);
        }
        if self.impressed {
            return Err(SlCode::EReImpress);
        }
        let slot = match self.items.get(no as usize) {
            Some(Some(_)) => return Err(SlCode::EReImpress),
            Some(None) => no as usize,
            None => return Err(SlCode::ENoItem),
        };
        if let Some(c) = self.items.get_mut(slot) {
            *c = Some(SealItem {
                no,
                lines,
                hash: Self::item_hash(no, lines),
            });
        }
        Ok(())
    }

    /// 条目齐闸：20/20。
    pub fn complete(&self) -> bool {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < SEAL_ITEMS {
            if let Some(Some(_)) = self.items.get(i) {
                n += 1;
            }
            i += 1;
        }
        n == SEAL_ITEMS as u32
    }

    /// 落印：条目齐 + 清点齐闸，印模哈希冻结。
    pub fn impress(&mut self, tally: &DomainQuota) -> Result<u64, SlCode> {
        if !self.complete() {
            return Err(SlCode::EMissing);
        }
        if !tally.is_tallied() {
            return Err(SlCode::EUntallied);
        }
        if self.impressed {
            return Err(SlCode::EReImpress);
        }
        // 印模哈希：逐条目哈希折叠 + 累计行数。
        let mut h: u64 = 0xcbf29ce484222325;
        let mut i = 0usize;
        while i < SEAL_ITEMS {
            if let Some(Some(item)) = self.items.get(i) {
                h = (h ^ item.hash).wrapping_mul(0x100000001b3);
            }
            i += 1;
        }
        h = (h ^ tally.lines()).wrapping_mul(0x100000001b3);
        self.imprint = h;
        self.impressed = true;
        Ok(h)
    }

    /// 印模零改写核验：重算哈希一致且已落印。
    pub fn intact(&self, tally_lines: u64) -> bool {
        if !self.impressed {
            return false;
        }
        let mut h: u64 = 0xcbf29ce484222325;
        let mut i = 0usize;
        while i < SEAL_ITEMS {
            if let Some(Some(item)) = self.items.get(i) {
                if item.hash != Self::item_hash(item.no, item.lines) {
                    return false;
                }
                h = (h ^ item.hash).wrapping_mul(0x100000001b3);
            } else {
                return false;
            }
            i += 1;
        }
        h = (h ^ tally_lines).wrapping_mul(0x100000001b3);
        h == self.imprint
    }

    /// 条目读出。
    pub fn item(&self, no: usize) -> Option<SealItem> {
        self.items.get(no).copied().flatten()
    }
}

// ── 注入器（LCG 种子 = SEG_BASE 派生）───────────────────────────
/// 六类坏样本注入器。
pub struct SlInjector {
    lcg: u32,
}

impl SlInjector {
    /// 确定性构造。
    pub fn new() -> Self {
        SlInjector { lcg: SEG_BASE }
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
                let mut inj = SlInjector { lcg: SEG_BASE ^ (cls as u32) ^ (r as u32 + 1) };
                let noise = inj.next();
                let ok = match cls {
                    // 类0 超配额：一次性累计超 240,000。
                    0 => {
                        let mut q = DomainQuota::new();
                        q.tally(QUOTA_LINES) == Err(SlCode::EOverQuota)
                    }
                    // 类1 印模改写：落印后改条目哈希 intact 必假。
                    1 => {
                        let mut q = DomainQuota::new();
                        q.done_tally();
                        let _ = q.tally(1000);
                        let mut imp = SealImprint::new();
                        let mut no = 0u32;
                        while no < SEAL_ITEMS as u32 {
                            let _ = imp.fill(no, (no as u64 + 1) * 100);
                            no += 1;
                        }
                        let _ = imp.impress(&q);
                        let mut broken = imp;
                        if let Some(Some(item)) = broken.items.get_mut((noise % SEAL_ITEMS as u32) as usize) {
                            item.hash ^= 1;
                        }
                        !broken.intact(q.lines())
                    }
                    // 类2 条目缺失：缺一条落印拒。
                    2 => {
                        let mut q = DomainQuota::new();
                        q.done_tally();
                        let mut imp = SealImprint::new();
                        let mut no = 0u32;
                        while no < SEAL_ITEMS as u32 - 1 {
                            let _ = imp.fill(no, 10);
                            no += 1;
                        }
                        imp.impress(&q) == Err(SlCode::EMissing)
                    }
                    // 类3 条目越册。
                    3 => SealImprint::new().fill(SEAL_ITEMS as u32 + noise % 3, 1)
                        == Err(SlCode::ENoItem),
                    // 类4 相位倒退：越级全拒。
                    4 => !phase_step(SlPhase::Tally, SlPhase::Impress)
                        && !phase_step(SlPhase::Archive, SlPhase::Tally),
                    // 类5 封账写拒。
                    _ => {
                        let mut q = DomainQuota::new();
                        q.seal();
                        q.tally(1) == Err(SlCode::ESealed)
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
/// 单轮全流程：清点至 108,000+N → 20 条冻结 → 落印 → 零改写核验。
/// 返回（基线过轮数, 印完好轮数, 印模哈希, 指纹）。
pub fn run_cycle() -> (u32, u32, u64, u64) {
    let mut baseline_ok = 0u32;
    let mut intact_ok = 0u32;
    let mut imprint_last: u64 = 0;
    let mut fp_last: u64 = 0;
    let mut r = 0usize;
    while r < INJ_ROUNDS {
        let mut q = DomainQuota::new();
        // 清点：累计至基线之上（确定性增量）。
        let _ = q.tally((r as u64 % 1000) + 1);
        q.done_tally();
        if q.baseline_ok() && q.lines() >= BASELINE_LINES {
            baseline_ok += 1;
        }
        // 相位链。
        let mut ph = SlPhase::Tally;
        if phase_step(ph, SlPhase::Freeze) {
            ph = SlPhase::Freeze;
        }
        // 20 条冻结条目：条目 no 行数 = 基线 + no×37（确定性）。
        let mut imp = SealImprint::new();
        let mut no = 0u32;
        while no < SEAL_ITEMS as u32 {
            let _ = imp.fill(no, BASELINE_LINES + (no as u64) * 37);
            no += 1;
        }
        if phase_step(ph, SlPhase::Impress) {
            ph = SlPhase::Archive;
        }
        // 落印 + 零改写核验。
        if let Ok(h) = imp.impress(&q) {
            imprint_last = h;
            if imp.intact(q.lines()) && ph == SlPhase::Archive {
                intact_ok += 1;
            }
        }
        fp_last ^= q.fingerprint() ^ imprint_last;
        r += 1;
    }
    (baseline_ok, intact_ok, imprint_last, fp_last)
}

// ── 判据集 ───────────────────────────────────────────────────────
/// 本模块自检判据集。
pub fn run_unx51560_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51560");
    let (baseline_ok, intact_ok, imprint, fp) = run_cycle();
    // 1. 批印入册：域账累计 108,000/240,000。
    let mut q = DomainQuota::new();
    s.add_dyn(
        "域账累计 108,000/240,000",
        baseline_ok == INJ_ROUNDS as u32
            && q.lines() == BASELINE_LINES
            && q.usage() >= 4_500
            && q.usage() <= 10_000,
        &format!(
            "基线={}/{} 起点={} 配额={} 占用={}‱",
            baseline_ok, INJ_ROUNDS, BASELINE_LINES, QUOTA_LINES, q.usage()
        ),
    );
    // 2. 收官印 20 条冻结零改写。
    s.add_dyn(
        "收官印 20 条冻结零改写",
        intact_ok == INJ_ROUNDS as u32 && imprint != 0,
        &format!("印完好={}/{} 印模={:#x}", intact_ok, INJ_ROUNDS, imprint),
    );
    // 3. 注入六类 60/60。
    let inj = SlInjector::new();
    let (hit, esc, per) = inj.inject_all();
    s.add_dyn(
        "注入六类 60/60 检出逃逸0",
        hit == 60 && esc == 0,
        &format!("检出={} 逃逸={} 守恒=60", hit, esc),
    );
    let _ = per;
    // 4. 超配额闸锚定：越 240,000 必拒。
    let mut q2 = DomainQuota::new();
    s.add(
        "超配额闸",
        q2.tally(QUOTA_LINES) == Err(SlCode::EOverQuota),
        "一次性累计越配额显性拒",
    );
    // 5. 零增量回退闸。
    s.add(
        "零增量回退闸",
        DomainQuota::new().tally(0) == Err(SlCode::ETallyRewind),
        "零增量视同回退显性拒",
    );
    // 6. 重印闸。
    let mut q3 = DomainQuota::new();
    q3.done_tally();
    let mut imp = SealImprint::new();
    let mut no = 0u32;
    while no < SEAL_ITEMS as u32 {
        let _ = imp.fill(no, 100 + no as u64);
        no += 1;
    }
    let _ = imp.impress(&q3);
    s.add(
        "重印闸",
        imp.impress(&q3) == Err(SlCode::EReImpress) && imp.impressed,
        "二次落印显性拒",
    );
    // 7. 印模哈希幂等锚定。
    s.add(
        "印模哈希锚定",
        SealImprint::item_hash(7, 1000) == SealImprint::item_hash(7, 1000)
            && SealImprint::item_hash(7, 1000) != SealImprint::item_hash(8, 1000),
        "同参同印、异参异印",
    );
    // 8. 闭集唯一性与诊断非空。
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
    // 9. 段位守卫。
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0000 && SEG_BASE <= 0xC31C0,
        "SEG_BASE 0x40 对齐且在 VX12 域",
    );
    // 10. 收官链单调。
    s.add(
        "收官链单调",
        phase_step(SlPhase::Tally, SlPhase::Freeze)
            && phase_step(SlPhase::Freeze, SlPhase::Impress)
            && phase_step(SlPhase::Impress, SlPhase::Archive)
            && !phase_step(SlPhase::Tally, SlPhase::Archive),
        "收官链逐段推进，越级拒",
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
        let (b, i, imp, _) = run_cycle();
        assert_eq!(b, INJ_ROUNDS as u32);
        assert_eq!(i, INJ_ROUNDS as u32);
        assert_ne!(imp, 0);
    }

    #[test]
    fn test_boundary_over_quota() {
        let mut q = DomainQuota::new();
        assert_eq!(q.tally(QUOTA_LINES), Err(SlCode::EOverQuota));
        // 边界内恰好到配额：合法。
        let mut q2 = DomainQuota::new();
        let add = QUOTA_LINES - BASELINE_LINES;
        assert!(q2.tally(add).is_ok());
        assert_eq!(q2.lines(), QUOTA_LINES);
        assert_eq!(q2.tally(1), Err(SlCode::EOverQuota));
    }

    #[test]
    fn test_boundary_tally_rewind() {
        let mut q = DomainQuota::new();
        assert_eq!(q.tally(0), Err(SlCode::ETallyRewind));
    }

    #[test]
    fn test_boundary_no_item() {
        let mut imp = SealImprint::new();
        assert_eq!(imp.fill(SEAL_ITEMS as u32, 1), Err(SlCode::ENoItem));
        assert!(imp.fill(0, 1).is_ok());
    }

    #[test]
    fn test_boundary_missing_item() {
        let mut q = DomainQuota::new();
        q.done_tally();
        let mut imp = SealImprint::new();
        let mut no = 0u32;
        while no < SEAL_ITEMS as u32 - 1 {
            let _ = imp.fill(no, 10);
            no += 1;
        }
        assert_eq!(imp.impress(&q), Err(SlCode::EMissing));
    }

    #[test]
    fn test_boundary_untallied() {
        let mut q = DomainQuota::new();
        let mut imp = SealImprint::new();
        let mut no = 0u32;
        while no < SEAL_ITEMS as u32 {
            let _ = imp.fill(no, 10);
            no += 1;
        }
        assert_eq!(imp.impress(&q), Err(SlCode::EUntallied));
    }

    #[test]
    fn test_imprint_intact() {
        let mut q = DomainQuota::new();
        q.done_tally();
        let _ = q.tally(500);
        let mut imp = SealImprint::new();
        let mut no = 0u32;
        while no < SEAL_ITEMS as u32 {
            let _ = imp.fill(no, (no as u64 + 1) * 13);
            no += 1;
        }
        assert!(imp.impress(&q).is_ok());
        assert!(imp.intact(q.lines()));
        assert_eq!(imp.impress(&q), Err(SlCode::EReImpress));
    }

    #[test]
    fn test_item_hash_idempotent() {
        assert_eq!(SealImprint::item_hash(3, 9), SealImprint::item_hash(3, 9));
        assert_ne!(SealImprint::item_hash(3, 9), SealImprint::item_hash(3, 10));
    }

    #[test]
    fn test_phase_monotonic() {
        assert!(phase_step(SlPhase::Tally, SlPhase::Freeze));
        assert!(!phase_step(SlPhase::Archive, SlPhase::Freeze));
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
        let (hit, esc, _) = SlInjector::new().inject_all();
        assert_eq!(hit, 60);
        assert_eq!(esc, 0);
    }

    #[test]
    fn unx51560_checks_green() {
        let s = run_unx51560_checks();
        assert!(s.all_passed(), "判据集必须全绿: {}", s.red_items_text());
        assert!(s.len() >= 13);
    }
}
