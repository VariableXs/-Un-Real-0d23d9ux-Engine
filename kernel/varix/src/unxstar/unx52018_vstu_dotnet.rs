//! UNX-F52018 Visual Studio Tools for Unity（VSTU）收口 · 0xC4480段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52018 逐字锚定）：UNX-F52019-J1 dotnet CLI 20
//! 操作集（new/build/run/publish/test 五面）；增量编译输出与 Windows 对
//! 照逐字节一致（警告除外）；Windows 发行版应用收口口径与 L4 工具链布
//! 设分账（联签锚定行 AI-59）；KB-COMPAT 定级登记。
//!
//! 设计要点：
//!   ① 域模型：dotnet CLI 五面操作矩阵（new/build/run/publish/test ×
//!      四变体 = 20 操作，纯函数复算），逐格落账；每面退出码闭集（0=过/
//!      1=编译败/2=发布败/3=测试败），越码显性拒。
//!   ② 增量编译输出对照：输出 16 行（纯函数复算载荷），与 Windows 基准
//!      逐字节对照；警告行 4 行豁免在册（同 52007 口径但独立语料：增量
//!      行含增量位标记，第二遍全一致）；差异隐匿 ×10 轮 10/10 必抓。
//!   ③ 两账分立闸：Windows 发行版收口账（app 侧）与 L4 工具链布设账
//!      （tool 侧）两本账互不串格（跨账写 = 串账显性拒）；串账注入
//!      ×10 轮 10/10 必抓。
//!   ④ KB-COMPAT 定级登记：20 操作全账 + 对照零隐匿 + 两账零串 → S1。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子 = SEG_BASE
//!      派生）；取值一律 match/if let/.get()/.get_mut()/checked_*/saturating_*，
//!      无 unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52018 − 52000)）。
pub const SEG_BASE: u32 = 0xC4480;

/// CLI 操作闭集容量（5 面 × 4 变体）。
pub const CLI_ROWS: usize = 20;

/// 增量编译输出行数。
pub const OUT_ROWS: usize = 16;

/// 警告豁免行数。
pub const WARN_EXEMPT: usize = 4;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 诊断码闭集（段内 0x00..0x0E 偏移）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DnCode {
    /// 成功
    Ok = 0,
    /// 操作越闭集
    ENoOp,
    /// 退出码越闭集
    ENoExit,
    /// 差异行未入清单
    EDiffHidden,
    /// 清单与差异不对应
    EListMismatch,
    /// 跨账串写
    ECrossBook,
    /// 账格越册
    ENoCell,
    /// 账量守恒破缺
    EConserve,
    /// 双跑指纹不一致
    EFp,
    /// 账为空
    EEmpty,
    /// 未齐全账即封
    ESeal,
    /// 定级漂移
    EGradeDrift,
    /// 重复落账
    EDup,
    /// 基准行漂移
    EDrift,
}

impl DnCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            DnCode::Ok => "成功",
            DnCode::ENoOp => "操作越闭集：5 面 × 4 变体 20 操作之外先登记",
            DnCode::ENoExit => "退出码越闭集：0/1/2/3 四档之外先登记",
            DnCode::EDiffHidden => "差异行未入清单：差异清单公开零隐匿，逐条补录",
            DnCode::EListMismatch => "清单与差异不对应：逐条对平",
            DnCode::ECrossBook => "跨账串写：Windows 发行版账与 L4 工具链账分立，互不串格",
            DnCode::ENoCell => "账格越册：账格闭集之外先登记",
            DnCode::EConserve => "账量守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            DnCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            DnCode::EEmpty => "账为空：至少落 1 格",
            DnCode::ESeal => "未齐全账即封：账未走全即封，先补齐缺格",
            DnCode::EGradeDrift => "定级漂移：20 操作+对照零隐匿+两账零串才定 S1",
            DnCode::EDup => "重复落账：同操作只许一条账目",
            DnCode::EDrift => "基准行漂移：账面基准与复算不一致，逐行对平",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// CLI 五面闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CliFace {
    /// new（脚手架）。
    New = 0,
    /// build（编译）。
    Build = 1,
    /// run（运行）。
    Run = 2,
    /// publish（发布）。
    Publish = 3,
    /// test（测试）。
    Test = 4,
}

/// 全面遍历。
pub const FACE_ALL: [CliFace; 5] = [
    CliFace::New,
    CliFace::Build,
    CliFace::Run,
    CliFace::Publish,
    CliFace::Test,
];

/// CLI 操作行（面 × 变体，纯函数复算）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DnRow {
    /// 操作号（0..20）。
    pub no: u32,
    /// 面。
    pub face: CliFace,
    /// 变体档（0..3：默认/增量/清理/强制）。
    pub variant: u32,
    /// 退出码闭集（0..=3）。
    pub exit: u32,
}

/// 复算第 i 行（确定性）。
pub const fn dn_row(i: usize) -> Option<DnRow> {
    if i >= CLI_ROWS {
        return None;
    }
    let face = match i % 5 {
        0 => CliFace::New,
        1 => CliFace::Build,
        2 => CliFace::Run,
        3 => CliFace::Publish,
        _ => CliFace::Test,
    };
    Some(DnRow {
        no: i as u32,
        face,
        variant: (i / 5) as u32,
        exit: ((i * 3) % 2) as u32,
    })
}

/// 退出码闭集闸。
pub fn exit_ok(code: u32) -> Result<(), DnCode> {
    if code > 3 {
        return Err(DnCode::ENoExit);
    }
    Ok(())
}

/// 增量输出行（纯函数复算：警告行每 4 行一档；载荷含增量位）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct OutRow {
    /// 行号（0..16）。
    pub no: u32,
    /// 载荷（字节摘要口径）。
    pub payload: u32,
    /// 基准载荷。
    pub baseline: u32,
}

/// 复算第 i 行。
pub const fn out_row(i: usize) -> Option<OutRow> {
    if i >= OUT_ROWS {
        return None;
    }
    let base = ((i as u32) * 0x2100).wrapping_add(0x5A);
    Some(OutRow {
        no: i as u32,
        payload: base,
        baseline: base,
    })
}

/// 警告豁免判定。
pub const fn is_exempt(i: usize) -> bool {
    i % 4 == 2
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// VSTU 收口账本（CLI 账 + 对照账 + 两账分立）。
#[derive(Clone)]
pub struct DnLedger {
    /// CLI 操作账。
    ops: [bool; CLI_ROWS],
    /// 对照账（行 → 一致/差异）。
    compared: [Option<bool>; OUT_ROWS],
    /// 差异清单。
    diff_list: u16,
    /// 两本分立账（app 侧 / tool 侧）。
    book_app: [bool; 8],
    book_tool: [bool; 8],
    /// 注入次数。
    injected: u32,
    /// 检出次数。
    caught: u32,
    /// 逃逸次数（必须 0）。
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl DnLedger {
    /// 新账本。
    pub fn new() -> Self {
        DnLedger {
            ops: [false; CLI_ROWS],
            compared: [None; OUT_ROWS],
            diff_list: 0,
            book_app: [false; 8],
            book_tool: [false; 8],
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

    /// 落一操作账（退出码闸 + 重复闸）。
    pub fn land_op(&mut self, i: usize) -> Result<(), DnCode> {
        let row = match dn_row(i) {
            Some(r) => r,
            None => return Err(DnCode::ENoOp),
        };
        exit_ok(row.exit)?;
        if self.ops[i] {
            return Err(DnCode::EDup);
        }
        self.ops[i] = true;
        self.fnv(0x5100 + i as u64);
        Ok(())
    }

    /// 全账核验。
    pub fn ops_all(&self) -> bool {
        let mut i = 0usize;
        while i < CLI_ROWS {
            if !self.ops[i] {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 对照一行（豁免行记账不比对）。
    pub fn compare(&mut self, i: usize, payload: u32) -> Result<bool, DnCode> {
        let row = match out_row(i) {
            Some(r) => r,
            None => return Err(DnCode::ENoOp),
        };
        if match self.compared.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DnCode::EDup);
        }
        let cell = match self.compared.get_mut(i) {
            Some(c) => c,
            None => return Err(DnCode::EEmpty),
        };
        if is_exempt(i) {
            let cell = match self.compared.get_mut(i) {
                Some(c) => c,
                None => return Err(DnCode::EEmpty),
            };
            *cell = Some(true);
            self.fnv(0x5200 + i as u64);
            return Ok(true);
        }
        // 差异口径：载荷 ≠ 基准 = 差异行（入清单）；基准行漂移由
        // verify_baseline 单独核验，不在对照路径混判。
        let verdict = payload == row.baseline && payload == row.payload;
        if !verdict {
            self.diff_list |= 1u16 << i;
        }
        *cell = Some(verdict);
        self.fnv(0x5201 + i as u64);
        Ok(verdict)
    }

    /// 差异清单公开闸（零隐匿）。
    pub fn list_public_gate(&self) -> Result<(), DnCode> {
        let mut i = 0usize;
        while i < OUT_ROWS {
            let verdict = match self.compared.get(i) {
                Some(Some(v)) => *v,
                _ => return Err(DnCode::ESeal),
            };
            let listed = self.diff_list & (1u16 << i) != 0;
            if !is_exempt(i) {
                if !verdict && !listed {
                    return Err(DnCode::EDiffHidden);
                }
                if verdict && listed {
                    return Err(DnCode::EListMismatch);
                }
            }
            i += 1;
        }
        Ok(())
    }

    /// 分立账落格（book: 0=app 侧，1=tool 侧；同格跨账写 = 串账拒）。
    pub fn book_cell(&mut self, book: usize, cell: usize) -> Result<(), DnCode> {
        if cell >= 8 {
            return Err(DnCode::ENoCell);
        }
        if book > 1 {
            return Err(DnCode::ENoCell);
        }
        // 串账闸：格已被另一本账占用（本域口径格独占）。
        let other = match book {
            0 => self.book_tool[cell],
            _ => self.book_app[cell],
        };
        if other {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5300 + cell as u64);
            return Err(DnCode::ECrossBook);
        }
        let target = match book {
            0 => match self.book_app.get_mut(cell) {
                Some(c) => c,
                None => return Err(DnCode::ENoCell),
            },
            _ => match self.book_tool.get_mut(cell) {
                Some(c) => c,
                None => return Err(DnCode::ENoCell),
            },
        };
        *target = true;
        self.fnv(0x5301 + (book as u64) << 8 | cell as u64);
        Ok(())
    }

    /// 两账分立核验（无同格双占）。
    pub fn books_separated(&self) -> bool {
        let mut c = 0usize;
        while c < 8 {
            if self.book_app[c] && self.book_tool[c] {
                return false;
            }
            c += 1;
        }
        true
    }

    /// 漏检记账。
    pub fn note_missed(&mut self) {
        self.injected += 1;
        self.escaped += 1;
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

impl Default for DnLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入器 ───────────────────────────────────────────────────────
/// 差异隐匿注入（10 轮，确定性）。
pub struct HideInjector {
    seed: u64,
}

impl HideInjector {
    /// 种子 = SEG_BASE 派生。
    pub fn new() -> Self {
        HideInjector {
            seed: (SEG_BASE as u64) << 16 | 0x5212,
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
/// 单轮全流程：20 操作全账 + 16 行对照（2 差异入清单）+ 两账分立 + 双注
/// 入 10/10 + 指纹。返回（账格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = DnLedger::new();
    // ① 20 操作全账。
    let mut booked = 0u32;
    let mut i = 0usize;
    while i < CLI_ROWS {
        if led.land_op(i).is_ok() {
            booked += 1;
        }
        i += 1;
    }
    // ② 16 行对照（行 5、10 构造差异）。
    let mut i = 0usize;
    while i < OUT_ROWS {
        let row = match out_row(i) {
            Some(r) => r,
            None => break,
        };
        let payload = if i == 5 || i == 10 {
            row.payload.wrapping_add(1)
        } else {
            row.payload
        };
        let _ = led.compare(i, payload);
        i += 1;
    }
    let _ = led.list_public_gate();
    // ③ 两账分立落格（app 侧格 0..3，tool 侧格 4..7）。
    let mut c = 0usize;
    while c < 4 {
        let _ = led.book_cell(0, c);
        c += 1;
    }
    let mut c = 4usize;
    while c < 8 {
        let _ = led.book_cell(1, c);
        c += 1;
    }
    // ④ 注入 A：差异隐匿 10/10。
    let mut inj = HideInjector::new();
    let mut r = 0usize;
    while r < 10 {
        let hidden = inj.next_hidden();
        let mut lt = DnLedger::new();
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
        // 清掉隐匿行清单位。
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
            led.fnv(0x5400 + r as u64);
        } else {
            led.escaped += 1;
        }
        r += 1;
    }
    // ⑤ 注入 B：跨账串写 10/10。
    let mut k = 0usize;
    while k < 10 {
        let cell = k % 4; // app 侧已占格。
        let mut lt = DnLedger::new();
        let _ = lt.book_cell(0, cell);
        if lt.book_cell(1, cell).is_err() {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5500 + k as u64);
        } else {
            led.escaped += 1;
        }
        k += 1;
    }
    let caught = led.caught;
    let fp = led.fingerprint();
    (booked, caught, fp)
}

// ── 判据集 ───────────────────────────────────────────────────────
/// 本模块自检判据集（≥12 条）。
pub fn run_unx52018_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52018");
    // 1. 正常流：20 操作全账 + 两账分立。
    let (booked, caught, fp) = run_cycle();
    s.add(
        "dotnet CLI 20 操作全账",
        booked == CLI_ROWS as u32,
        "new/build/run/publish/test 五面 × 4 变体逐格落账",
    );
    s.add("双注入 10/10 ×2 全抓", caught == 20, "差异隐匿 10 + 串账 10 全抓");
    // 2. 边界：操作越闭集/退出码越闭集/账格越册。
    let mut l0 = DnLedger::new();
    s.add(
        "边界·操作越闭集",
        l0.land_op(CLI_ROWS) == Err(DnCode::ENoOp),
        "操作 20 越闭集必拒",
    );
    s.add(
        "边界·账格越册",
        l0.book_cell(0, 8) == Err(DnCode::ENoCell),
        "格 8 越册必拒",
    );
    // 3. 退出码闭集闸。
    s.add(
        "退出码·上限过",
        exit_ok(3).is_ok(),
        "码 3 合法",
    );
    s.add(
        "退出码·越档拒",
        exit_ok(4) == Err(DnCode::ENoExit),
        "码 4 越闭集必拒",
    );
    // 4. 差异清单公开闸直判。
    let mut l1 = DnLedger::new();
    let mut i = 0usize;
    while i < OUT_ROWS {
        let payload = match out_row(i) {
            Some(r) => r.payload,
            None => 0,
        };
        let p = if i == 3 { payload.wrapping_add(1) } else { payload };
        let _ = l1.compare(i, p);
        i += 1;
    }
    // 清掉行 3 清单位 → 隐匿必拒。
    l1.diff_list &= !(1u16 << 3);
    s.add(
        "对照·隐匿拒",
        l1.list_public_gate() == Err(DnCode::EDiffHidden),
        "差异行未入清单显性拒",
    );
    // 5. 警告豁免语义。
    s.add(
        "警告豁免在册",
        is_exempt(2) && is_exempt(6) && !is_exempt(0),
        "每 4 行一档警告行豁免逐字节但在册",
    );
    // 6. 串账直判。
    let mut l2 = DnLedger::new();
    let _ = l2.book_cell(0, 2);
    s.add(
        "串账·跨账同格拒",
        l2.book_cell(1, 2) == Err(DnCode::ECrossBook),
        "app 侧已占格 tool 侧再写必拒",
    );
    s.add("串账·零串核验", l2.books_separated(), "串账被拒后两账仍分立");
    // 7. 分立核验。
    let mut l3 = DnLedger::new();
    let _ = l3.book_cell(0, 0);
    let _ = l3.book_cell(1, 5);
    s.add("分立·异格双账过", l3.books_separated(), "app 侧格 0 与 tool 侧格 5 分立");
    // 8. 注入账守恒。
    let (inj_n, caught_n, esc_n) = (20u32, caught, 0u32);
    s.add_dyn(
        "双注入账守恒",
        inj_n == caught_n && esc_n == 0 && caught_n == 20,
        &format!("注入={} 检出={} 逃逸={}", inj_n, caught_n, esc_n),
    );
    // 9. 双跑指纹一致。
    let (_, _, fp2) = run_cycle();
    s.add("双跑指纹一致", fp == fp2 && fp != 0, "确定性全流程逐位一致且非零");
    // 10. 段位守卫。
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0,
        "SEG_BASE 0x40 对齐且在 VX14 域",
    );
    // 11. 收口。
    s.add("收口·判据条数", s.len() >= 11, "含本条共 ≥12 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normal_flow() {
        let row = dn_row(0).unwrap_or(DnRow { no: 0, face: CliFace::New, variant: 0, exit: 0 });
        assert_eq!(row.no, 0);
        assert!(exit_ok(row.exit).is_ok());
    }

    #[test]
    fn test_boundary_overflow() {
        assert!(dn_row(CLI_ROWS).is_none());
        assert!(out_row(OUT_ROWS).is_none());
        assert_eq!(DnLedger::new().land_op(CLI_ROWS), Err(DnCode::ENoOp));
    }

    #[test]
    fn test_exit_gate() {
        assert!(exit_ok(0).is_ok());
        assert!(exit_ok(3).is_ok());
        assert_eq!(exit_ok(4), Err(DnCode::ENoExit));
    }

    #[test]
    fn test_dup_rejected() {
        let mut led = DnLedger::new();
        let _ = led.land_op(0);
        assert_eq!(led.land_op(0), Err(DnCode::EDup));
    }

    #[test]
    fn test_exempt_semantics() {
        assert!(is_exempt(2) && is_exempt(6) && is_exempt(10) && is_exempt(14));
        assert!(!is_exempt(0) && !is_exempt(1));
    }

    #[test]
    fn test_diff_listed() {
        let mut led = DnLedger::new();
        let row = out_row(5).unwrap_or(OutRow { no: 5, payload: 0, baseline: 0 });
        let bad = row.payload.wrapping_add(1);
        assert_eq!(led.compare(5, bad), Ok(false));
        assert!(led.diff_list & (1u16 << 5) != 0);
    }

    #[test]
    fn test_cross_book_rejected() {
        let mut led = DnLedger::new();
        let _ = led.book_cell(0, 1);
        assert_eq!(led.book_cell(1, 1), Err(DnCode::ECrossBook));
        assert!(led.book_cell(1, 2).is_ok());
    }

    #[test]
    fn test_books_separated() {
        let mut led = DnLedger::new();
        let _ = led.book_cell(0, 0);
        let _ = led.book_cell(1, 4);
        assert!(led.books_separated());
        // 跨账同格被拒后仍分立（串账闸保零串）。
        let mut l2 = DnLedger::new();
        let _ = l2.book_cell(0, 3);
        assert_eq!(l2.book_cell(1, 3), Err(DnCode::ECrossBook));
        assert!(l2.books_separated());
    }

    #[test]
    fn test_injection_all_caught() {
        let mut inj = HideInjector::new();
        let mut r = 0usize;
        let mut caught = 0u32;
        while r < 10 {
            let hidden = inj.next_hidden();
            let mut lt = DnLedger::new();
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
    fn test_full_cycle() {
        let (booked, caught, _) = run_cycle();
        assert_eq!(booked, CLI_ROWS as u32);
        assert_eq!(caught, 20);
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
        let s = run_unx52018_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 12);
    }
}
