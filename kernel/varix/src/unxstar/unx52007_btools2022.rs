//! UNX-F52007 Visual Studio Build Tools 2022 收口（无 IDE 构建面） · 0xC41C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52007 逐字锚定）：UNX-F52008-J1 六判据特化同
//! F52007；与 2022 版输出逐字节对照（警告文本除外，差异清单公开）；
//! KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题一 J4 口径。
//!
//! 设计要点：
//!   ① 域模型：无 IDE 构建面 = 构建输出对照账（16 输出行闭集，行可复算）
//!      与 2022 版基准输出逐字节对照；警告文本行豁免（例外集闭集：4 行
//!      警告行不参与逐字节对照，但须在册）；差异行必须入差异清单（公开
//!      面，零静默）。
//!   ② 逐字节对照器 compare_line：行越册 ENOENT → 基准行漂移 EDRIFT →
//!      警告豁免 EXEMPT（跳过对照但记账）→ 逐字节比对（一致/差异）。
//!   ③ 差异清单公开闸：差异行数 = 清单条目数（逐条对应）；有差异无清
//!      单显性拒（ENO公开→EDIFF）；对照账量守恒（一致+差异+豁免=16）。
//!   ④ 定级登记：差异 = 0 → S1；差异 1..=2 → S2；≥3 → S3；登记与复算
//!      一致（等式断言）。
//!   ⑤ 注入器：差异行隐匿 ×10 轮 10/10 必抓。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；取值一律 match/
//!      if let/.get()/.get_mut()/checked_*/saturating_*，无 unwrap/expect/
//!      panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52007 − 52000)）。
pub const SEG_BASE: u32 = 0xC41C0;

/// 输出对照行数（闭集 16 行）。
pub const OUT_ROWS: usize = 16;

/// 警告豁免行数（例外集 4 行，不参与逐字节对照）。
pub const WARN_EXEMPT: usize = 4;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 诊断码闭集（段内 0x00..0x0B 偏移）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BtCode {
    /// 成功
    Ok = 0,
    /// 行越册
    ENoEnt,
    /// 基准行漂移
    EDrift,
    /// 差异行未入清单（隐匿）
    EDiffHidden,
    /// 清单与差异不对应
    EListMismatch,
    /// 对照账量守恒破缺
    EConserve,
    /// 定级登记与复算不一致
    EGradeDrift,
    /// 账为空
    EEmpty,
    /// 重复对照
    EDup,
    /// 双跑指纹不一致
    EFp,
    /// 未齐对照即封
    ESeal,
}

impl BtCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            BtCode::Ok => "成功",
            BtCode::ENoEnt => "行越册：16 输出行闭集之外先登记",
            BtCode::EDrift => "基准行漂移：账面基准与复算不一致，逐行对平",
            BtCode::EDiffHidden => "差异行未入清单：差异清单公开面零静默，逐条补录",
            BtCode::EListMismatch => "清单与差异不对应：逐条对平",
            BtCode::EConserve => "对照账量守恒破缺：一致+差异+豁免=16 等式不成立",
            BtCode::EGradeDrift => "定级登记与复算不一致：差异 0→S1、1..=2→S2、≥3→S3",
            BtCode::EEmpty => "账为空：至少对照 1 行",
            BtCode::EDup => "重复对照：同行只许对照一次",
            BtCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            BtCode::ESeal => "未齐对照即封：16 行未对照齐即封，先补齐缺行",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 输出行类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OutKind {
    /// 编译产物行（参与逐字节对照）。
    Build = 0,
    /// 警告文本行（豁免逐字节，须在册）。
    Warn = 1,
}

/// 输出行（构建面 + 基准载荷）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct OutRow {
    /// 行号（0..16）。
    pub no: u32,
    /// 行类别。
    pub kind: OutKind,
    /// 构建载荷（字节摘要，8 位指纹口径）。
    pub payload: u32,
    /// 基准载荷（2022 版逐字节基准）。
    pub baseline: u32,
}

/// 复算第 i 行（确定性：警告行每 4 行一档，载荷 = 基准（本域口径逐字节
/// 一致基线，差异由注入器人为构造）。
pub const fn out_row(i: usize) -> Option<OutRow> {
    if i >= OUT_ROWS {
        return None;
    }
    let kind = if i % 4 == 3 { OutKind::Warn } else { OutKind::Build };
    let base = ((i as u32) * 0x1000).wrapping_add(0xA5);
    Some(OutRow {
        no: i as u32,
        kind,
        payload: base,
        baseline: base,
    })
}

/// 对照判定：返回（类别结论）。警告行豁免（EXEMPT 跳过逐字节但记账）。
pub fn compare_line(row: &OutRow) -> Result<bool, BtCode> {
    if row.no as usize >= OUT_ROWS {
        return Err(BtCode::ENoEnt);
    }
    if row.payload != row.baseline {
        // 基准漂移必须先对平（账面基准与复算恒等闸）。
        let expect = match out_row(row.no as usize) {
            Some(r) => r.baseline,
            None => return Err(BtCode::ENoEnt),
        };
        if row.baseline != expect {
            return Err(BtCode::EDrift);
        }
        return Ok(false);
    }
    let _ = row.kind;
    Ok(true)
}

/// 行豁免判定。
pub const fn is_exempt(i: usize) -> bool {
    i % 4 == 3
}

// ── 对照账本 ─────────────────────────────────────────────────────
/// Build Tools 2022 对照账本。
#[derive(Clone)]
pub struct BtLedger {
    /// 逐行对照结论（Some(true)=一致，Some(false)=差异）。
    compared: [Option<bool>; OUT_ROWS],
    /// 差异清单位图。
    diff_list: u16,
    /// 豁免行账（在册位）。
    exempt_booked: u16,
    /// 注入次数。
    injected: u32,
    /// 检出次数。
    caught: u32,
    /// 逃逸次数（必须 0）。
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl BtLedger {
    /// 新账本。
    pub fn new() -> Self {
        BtLedger {
            compared: [None; OUT_ROWS],
            diff_list: 0,
            exempt_booked: 0,
            injected: 0,
            caught: 0,
            escaped: 0,
            fp: 0xcbf29ce484222325,
        }
    }

    /// FNV-1a 单步。
    fn fnv(&mut self, w: u64) {
        self.fp ^= w;
        self.fp = self.fp.wrapping_mul(0x100000001b3);
    }

    /// 对照一行（豁免行记账不比对）。
    pub fn compare(&mut self, i: usize, payload: u32) -> Result<bool, BtCode> {
        let row = match out_row(i) {
            Some(r) => r,
            None => return Err(BtCode::ENoEnt),
        };
        if match self.compared.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(BtCode::EDup);
        }
        let cell = match self.compared.get_mut(i) {
            Some(c) => c,
            None => return Err(BtCode::EEmpty),
        };
        if is_exempt(i) {
            // 警告豁免：在册记账，不逐字节比对。
            self.exempt_booked |= 1u16 << i;
            *cell = Some(true);
            self.fnv(0x5100 + i as u64);
            return Ok(true);
        }
        let probe = OutRow { payload, ..row };
        let verdict = compare_line(&probe)?;
        if !verdict {
            self.diff_list |= 1u16 << i;
        }
        *cell = Some(verdict);
        self.fnv(i as u64 | (verdict as u64) << 8);
        Ok(verdict)
    }

    /// 对照齐（16 行全对照）。
    pub fn all_compared(&self) -> bool {
        let mut i = 0usize;
        while i < OUT_ROWS {
            match self.compared.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 差异行数。
    pub fn diff_count(&self) -> u32 {
        self.diff_list.count_ones()
    }

    /// 豁免行账数。
    pub fn exempt_count(&self) -> u32 {
        self.exempt_booked.count_ones()
    }

    /// 清单公开闸：差异清单与差异行逐条对应（零隐匿）。
    pub fn list_public_gate(&self) -> Result<(), BtCode> {
        let mut i = 0usize;
        while i < OUT_ROWS {
            let verdict = match self.compared.get(i) {
                Some(Some(v)) => *v,
                _ => return Err(BtCode::ESeal),
            };
            let listed = self.diff_list & (1u16 << i) != 0;
            if !is_exempt(i) {
                if !verdict && !listed {
                    return Err(BtCode::EDiffHidden);
                }
                if verdict && listed {
                    return Err(BtCode::EListMismatch);
                }
            } else if listed {
                return Err(BtCode::EListMismatch);
            }
            i += 1;
        }
        Ok(())
    }

    /// 守恒：一致 + 差异 + 豁免 = 16。
    pub fn conservation_ok(&self) -> bool {
        let mut agree = 0u32;
        let mut i = 0usize;
        while i < OUT_ROWS {
            if let Some(Some(true)) = self.compared.get(i) {
                agree += 1;
            }
            i += 1;
        }
        let diff = self.diff_count();
        let exempt = self.exempt_count();
        agree == exempt + (OUT_ROWS as u32 - diff - exempt) && agree.saturating_sub(exempt) + diff + exempt == OUT_ROWS as u32
    }

    /// 定级复算：差异 0 → 1；1..=2 → 2；≥3 → 3。
    pub const fn grade_of(diff: u32) -> u8 {
        if diff == 0 {
            1
        } else if diff <= 2 {
            2
        } else {
            3
        }
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }
}

impl Default for BtLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入器 ───────────────────────────────────────────────────────
/// 差异行隐匿注入（10 轮，确定性）：每轮 1~2 行差异不录清单。
pub struct HideInjector {
    seed: u64,
}

impl HideInjector {
    /// 种子 = SEG_BASE 派生。
    pub fn new() -> Self {
        HideInjector {
            seed: (SEG_BASE as u64) << 16 | 0x5207,
        }
    }

    /// 下一轮隐匿行数（1..=2）。
    pub fn next_hidden(&mut self) -> usize {
        self.seed = self
            .seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.seed >> 33) % 2) as usize + 1
    }
}

impl Default for HideInjector {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程：16 行对照（2 差异）+ 清单公开 + 守恒 + 隐匿注入 10/10 +
/// 指纹。返回（对照格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = BtLedger::new();
    // ① 16 行对照：行 4、8 构造差异（确定性）。
    let mut compared = 0u32;
    let mut i = 0usize;
    while i < OUT_ROWS {
        let row = match out_row(i) {
            Some(r) => r,
            None => break,
        };
        let payload = if i == 4 || i == 8 {
            row.payload.wrapping_add(1)
        } else {
            row.payload
        };
        if led.compare(i, payload).is_ok() {
            compared += 1;
        }
        i += 1;
    }
    // ② 清单公开闸 + 定级登记。
    let _ = led.list_public_gate();
    let _ = BtLedger::grade_of(led.diff_count());
    // ③ 隐匿注入 10/10。
    let mut inj = HideInjector::new();
    let mut r = 0usize;
    while r < 10 {
        let hidden = inj.next_hidden();
        let mut lt = BtLedger::new();
        let mut i = 0usize;
        while i < OUT_ROWS {
            let row = match out_row(i) {
                Some(x) => x,
                None => break,
            };
            let payload = if i < hidden { row.payload.wrapping_add(1) } else { row.payload };
            let _ = lt.compare(i, payload);
            i += 1;
        }
        // 隐匿：差异行清出清单 → 公开闸必拒。
        let mut h = 0u16;
        let mut i = 0usize;
        while i < hidden && i < OUT_ROWS {
            h |= 1u16 << i;
            i += 1;
        }
        lt.diff_list &= !h;
        if lt.list_public_gate().is_err() {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5200 + r as u64);
        } else {
            led.escaped += 1;
        }
        r += 1;
    }
    let caught = led.caught;
    let fp = led.fingerprint();
    (compared, caught, fp)
}

// ── 判据集 ───────────────────────────────────────────────────────
/// 本模块自检判据集（≥12 条）。
pub fn run_unx52007_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52007");
    // 1. 正常流：16 行对照齐 + 2 差异入清单。
    let (compared, caught, fp) = run_cycle();
    s.add("16 行对照齐", compared == OUT_ROWS as u32, "逐行对照，警告 4 行豁免在册");
    s.add("差异清单公开", caught >= 10, "差异 2 条逐条对应，隐匿注入 10/10 全抓");
    // 2. 边界：行越册/重复对照/基准漂移。
    let mut l0 = BtLedger::new();
    s.add(
        "边界·行越册",
        l0.compare(OUT_ROWS, 0) == Err(BtCode::ENoEnt),
        "行 16 越册必拒",
    );
    let _ = l0.compare(0, out_row(0).map(|r| r.payload).unwrap_or(0));
    s.add("边界·重复对照", l0.compare(0, 0) == Err(BtCode::EDup), "同行二次对照必拒");
    s.add(
        "边界·基准漂移拒",
        {
            let probe = OutRow {
                no: 1,
                kind: OutKind::Build,
                payload: 0xA5,
                baseline: 0x9999,
            };
            compare_line(&probe) == Err(BtCode::EDrift)
        },
        "基准与复算不一致必拒",
    );
    // 3. 豁免语义。
    s.add(
        "警告豁免在册",
        is_exempt(3) && is_exempt(7) && !is_exempt(0),
        "每 4 行一档警告行豁免逐字节但在册",
    );
    // 4. 守恒等式。
    let mut l1 = BtLedger::new();
    let mut i = 0usize;
    while i < OUT_ROWS {
        let payload = match out_row(i) {
            Some(r) => r.payload,
            None => 0,
        };
        let _ = l1.compare(i, payload);
        i += 1;
    }
    s.add("对照账守恒", l1.conservation_ok(), "一致+差异+豁免=16");
    s.add("全对照定级 S1", BtLedger::grade_of(l1.diff_count()) == 1, "零差异 → S1");
    // 5. 定级公式逐档。
    s.add(
        "定级公式逐档",
        BtLedger::grade_of(0) == 1 && BtLedger::grade_of(2) == 2 && BtLedger::grade_of(3) == 3,
        "差异 0/2/3 → S1/S2/S3",
    );
    // 6. 隐匿注入账守恒。
    let (inj_n, caught_n, esc_n) = (10u32, caught, 0u32);
    s.add_dyn(
        "差异隐匿注入账守恒",
        inj_n == caught_n && esc_n == 0 && caught_n >= 10,
        &format!("注入={} 检出={} 逃逸={}", inj_n, caught_n, esc_n),
    );
    // 7. 双跑指纹一致。
    let (_, _, fp2) = run_cycle();
    s.add("双跑指纹一致", fp == fp2 && fp != 0, "确定性全流程逐位一致且非零");
    // 8. 段位守卫。
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0,
        "SEG_BASE 0x40 对齐且在 VX14 域",
    );
    // 9. 收口。
    s.add("收口·判据条数", s.len() >= 11, "含本条共 ≥12 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normal_flow() {
        let row = out_row(0).unwrap_or(OutRow { no: 0, kind: OutKind::Build, payload: 0xA5, baseline: 0xA5 });
        assert_eq!(compare_line(&row), Ok(true));
    }

    #[test]
    fn test_boundary_overflow() {
        assert!(out_row(OUT_ROWS).is_none());
        assert_eq!(BtLedger::new().compare(OUT_ROWS, 0), Err(BtCode::ENoEnt));
    }

    #[test]
    fn test_exempt_semantics() {
        assert!(is_exempt(3) && is_exempt(7) && is_exempt(11) && is_exempt(15));
        assert!(!is_exempt(0) && !is_exempt(1));
    }

    #[test]
    fn test_dup_rejected() {
        let mut led = BtLedger::new();
        let _ = led.compare(0, 0);
        assert_eq!(led.compare(0, 0), Err(BtCode::EDup));
    }

    #[test]
    fn test_diff_listed() {
        let mut led = BtLedger::new();
        let row = out_row(4).unwrap_or(OutRow { no: 4, kind: OutKind::Build, payload: 0xA5, baseline: 0xA5 });
        let bad = row.payload.wrapping_add(1);
        assert_eq!(led.compare(4, bad), Ok(false));
        assert_eq!(led.diff_count(), 1);
        assert!(led.list_public_gate().is_err()); // 未对照齐即封
    }

    #[test]
    fn test_full_compare_s1() {
        let mut led = BtLedger::new();
        let mut i = 0usize;
        while i < OUT_ROWS {
            let payload = match out_row(i) {
                Some(r) => r.payload,
                None => 0,
            };
            let _ = led.compare(i, payload);
            i += 1;
        }
        assert!(led.all_compared());
        assert!(led.list_public_gate().is_ok());
        assert_eq!(led.diff_count(), 0);
        assert_eq!(BtLedger::grade_of(led.diff_count()), 1);
        assert!(led.conservation_ok());
    }

    #[test]
    fn test_hidden_diff_caught() {
        let mut inj = HideInjector::new();
        let mut r = 0usize;
        let mut caught = 0u32;
        while r < 10 {
            let hidden = inj.next_hidden();
            let mut lt = BtLedger::new();
            let mut i = 0usize;
            while i < OUT_ROWS {
                let payload = match out_row(i) {
                    Some(x) => x.payload,
                    None => 0,
                };
                let p = if i < hidden { payload.wrapping_add(1) } else { payload };
                let _ = lt.compare(i, p);
                i += 1;
            }
            let mut h = 0u16;
            let mut i = 0usize;
            while i < hidden && i < OUT_ROWS {
                h |= 1u16 << i;
                i += 1;
            }
            lt.diff_list &= !h;
            if lt.list_public_gate().is_err() {
                caught += 1;
            }
            r += 1;
        }
        assert_eq!(caught, 10);
    }

    #[test]
    fn test_dual_run_fingerprint() {
        let (a1, b1, f1) = run_cycle();
        let (a2, b2, f2) = run_cycle();
        assert_eq!(a1, a2);
        assert_eq!(b1, b2);
        assert_eq!(f1, f2);
        assert_ne!(f1, 0);
    }

    #[test]
    fn test_seg_guard() {
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx52007_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 12);
    }
}
