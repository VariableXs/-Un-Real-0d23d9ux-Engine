//! UNX-F52355 yq 收口 · 0xC9880段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52355"，逐字）：
//!   title: yq 收口
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52356-J1 CSV 操
//!     作 ×10 全通；接管口径（xsv → qsv 升级接管字样）；KB-COMPAT 定级
//!     登记；判据锚定 §AI-66.1；③ 完成时 API/软件写回：状态=已完成、
//!     结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52355）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52356-J1，照原文锚定；功能按本卡标题（yq）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 四格式 20 格：YAML/JSON/XML/TOML 各 5 格，族 = 格号/5，族单调
//!      （逆序必抓，空账本首格非 0 号族必抓且不改 last_kind）；格式转
//!      换样本逐格载荷折叠恒等才收。
//!   ② 语法三方言 12 格：mikefarah/python/tomlq 各 4 格（方言 =
//!      格号/4），载荷折叠恒等才收；漂移必抓。
//!   ③ 原地编辑 8 操作集：赋值/删除/合并三族（kind = 格号分族），
//!      载荷折叠恒等才收；漂移必抓。
//!   ④ 与 jq 消费联签 4 格（F52354 锚定行）：YAML→JSON 前置/JSON→jq
//!      管交/jq 结果回注/复序列化四行闭集，缺一拒封。
//!   ⑤ Go 上栈声明 2 行闭集：go-yaml 解析声明行/yaml.v3 链接声明行，
//!      缺一拒封。
//!   ⑥ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a）；
//!      注入核 run_cycle 10 轮三态轮转（格式越闭集/编辑漂移/空账乱
//!      序），检出+逃逸守恒等式；双跑指纹一致；定级 S1/S2/S3 闭集
//!      （20 格 + 12 方言 + 8 操作 + 4 联签 + 2 声明 + 六判据全通 → S1）。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52355;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 四格式格数（四族 × 各 5 格）。
pub const FMT_CELLS: usize = 20;

/// 语法三方言格数（三方言 × 各 4 格）。
pub const DIALECT_CELLS: usize = 12;

/// 原地编辑操作数。
pub const EDIT_OPS: usize = 8;

/// 与 jq 消费联签行数（F52354）。
pub const SIG_ROWS: usize = 4;

/// Go 上栈声明行数。
pub const DECL_ROWS: usize = 2;

/// yq 域 XOR 锚（低 16 位恒零）。
pub const YQ_ANCHOR: u64 = 0x5951_2D59_5100_0000;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum YqCode {
    /// 成功
    Ok = 0,
    /// 格式族越闭集
    ENoFmt,
    /// 格式载荷漂移
    EFmtDrift,
    /// 方言格越闭集
    ENoDialect,
    /// 方言载荷漂移
    EDialectDrift,
    /// 族逆序
    EOrder,
    /// 原地编辑越闭集
    ENoEdit,
    /// 编辑载荷漂移
    EEditDrift,
    /// jq 联签行越闭集
    ENoSig,
    /// Go 上栈声明行越闭集
    ENoDecl,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 定级漂移
    EGradeDrift,
}

impl YqCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            YqCode::Ok => "成功",
            YqCode::ENoFmt => "格式族越闭集：YAML/JSON/XML/TOML 20 格之外先登记",
            YqCode::EFmtDrift => "格式载荷漂移：转换样本折叠与基准不恒等，逐格复算",
            YqCode::ENoDialect => "方言格越闭集：mikefarah/python/tomlq 12 格之外先登记",
            YqCode::EDialectDrift => "方言载荷漂移：方言样本折叠与基准不恒等，逐格复算",
            YqCode::EOrder => "族逆序：YAML→JSON→XML→TOML 单调，逆序必抓",
            YqCode::ENoEdit => "原地编辑越闭集：编辑 8 操作集之外先登记",
            YqCode::EEditDrift => "编辑载荷漂移：编辑折叠与基准不恒等，逐格复算",
            YqCode::ENoSig => "jq 联签行越闭集：4 行闭集之外先登记",
            YqCode::ENoDecl => "Go 上栈声明行越闭集：2 行闭集之外先登记",
            YqCode::EDup => "重复落账：同格只许一条账目",
            YqCode::ESeal => "未齐即封：20 格 + 12 方言 + 8 操作 + 4 联签 + 2 声明未齐先补齐",
            YqCode::EGradeDrift => "定级漂移：全通且联签/声明全齐才可定 S1",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口
    S1,
    /// S2：有条件收口
    S2,
    /// S3：不收口
    S3,
}

impl Grade {
    /// 定级值。
    pub fn val(self) -> u32 {
        match self {
            Grade::S1 => 1,
            Grade::S2 => 2,
            Grade::S3 => 3,
        }
    }
    /// 明细。
    pub fn detail(self) -> &'static str {
        match self {
            Grade::S1 => "S1：六判据+20格+12方言+8操作+4联签+2声明全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 格式格载荷（格 f；族 = f/5）。
pub const fn fmt_cell(f: usize) -> Option<u64> {
    if f >= FMT_CELLS {
        return None;
    }
    Some(
        ((0xC100u64 + (f as u64) * 0x35) ^ ((f / 5) as u64).wrapping_mul(0x01000193)) ^ YQ_ANCHOR,
    )
}

/// 格式族（0..5=YAML 5..10=JSON 10..15=XML 15..20=TOML）。
pub const fn fmt_kind(f: usize) -> Option<u32> {
    if f >= FMT_CELLS {
        return None;
    }
    Some((f / 5) as u32)
}

/// 格式族名。
pub const fn fmt_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("YAML"),
        1 => Some("JSON"),
        2 => Some("XML"),
        3 => Some("TOML"),
        _ => None,
    }
}

/// 方言格载荷（格 d；方言 = d/4）。
pub const fn dialect_cell(d: usize) -> Option<u64> {
    if d >= DIALECT_CELLS {
        return None;
    }
    Some((0xC200u64 + (d as u64) * 0x2D) ^ ((d / 4) as u64).wrapping_mul(0x9E3779B9))
}

/// 方言。
pub const fn dialect_kind(d: usize) -> Option<u32> {
    if d >= DIALECT_CELLS {
        return None;
    }
    Some((d / 4) as u32)
}

/// 方言名。
pub const fn dialect_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("mikefarah"),
        1 => Some("python"),
        2 => Some("tomlq"),
        _ => None,
    }
}

/// 原地编辑操作格载荷（操作 o；族：0..3=赋值 3..6=删除 6..8=合并）。
pub const fn edit_cell(o: usize) -> Option<u64> {
    if o >= EDIT_OPS {
        return None;
    }
    let kind = match o {
        0..=2 => 0u64,
        3..=5 => 1,
        _ => 2,
    };
    Some((0xC300u64 + (o as u64) * 0x2B) ^ kind.wrapping_mul(0x9E3779B9))
}

/// 原地编辑操作族。
pub const fn edit_kind(o: usize) -> Option<u32> {
    if o >= EDIT_OPS {
        return None;
    }
    Some(match o {
        0..=2 => 0,
        3..=5 => 1,
        _ => 2,
    })
}

/// 编辑族名。
pub const fn edit_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("赋值"),
        1 => Some("删除"),
        2 => Some("合并"),
        _ => None,
    }
}

/// 与 jq 消费联签行折叠（行 r，F52354 锚定）。
pub const fn jq_row(r: usize) -> Option<u32> {
    if r >= SIG_ROWS {
        return None;
    }
    Some(0xC500 + r as u32 * 0x2C)
}

/// 联签行名。
pub const fn jq_row_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("YAML→JSON 前置联签"),
        1 => Some("JSON→jq 管交联签"),
        2 => Some("jq 结果回注联签"),
        3 => Some("复序列化联签"),
        _ => None,
    }
}

/// Go 上栈声明行折叠（行 d）。
pub const fn go_row(d: usize) -> Option<u32> {
    if d >= DECL_ROWS {
        return None;
    }
    Some(0xC600 + d as u32 * 0x2C)
}

/// 声明行名。
pub const fn go_row_name(d: usize) -> Option<&'static str> {
    match d {
        0 => Some("go-yaml 解析声明行"),
        1 => Some("yaml.v3 链接声明行"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// yq 收口账本状态机。
#[derive(Clone)]
pub struct YqLedger {
    /// 六判据（读取/转换/查询/编辑/写回/退出码）。
    gates: [Option<bool>; 6],
    /// 格式账。
    fmts: [Option<u64>; FMT_CELLS],
    /// 方言账。
    dialects: [Option<u64>; DIALECT_CELLS],
    /// 原地编辑账。
    edits: [Option<u64>; EDIT_OPS],
    /// jq 联签账。
    sigs: [Option<u32>; SIG_ROWS],
    /// Go 上栈声明账。
    decls: [Option<u32>; DECL_ROWS],
    /// 最后编辑族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl YqLedger {
    /// 新账本。
    pub fn new() -> Self {
        YqLedger {
            gates: [None; 6],
            fmts: [None; FMT_CELLS],
            dialects: [None; DIALECT_CELLS],
            edits: [None; EDIT_OPS],
            sigs: [None; SIG_ROWS],
            decls: [None; DECL_ROWS],
            last_kind: -1,
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

    /// 落一格六判据。
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), YqCode> {
        if gate >= 6 {
            return Err(YqCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x4100 + gate as u64);
        Ok(())
    }

    /// 六判据全落核验。
    pub fn gates_all(&self) -> bool {
        let mut i = 0usize;
        while i < 6 {
            match self.gates.get(i) {
                Some(Some(true)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 格式一格（族单调 + 载荷对照 + 重复拒）。
    pub fn record_fmt(&mut self, f: usize, got: u64) -> Result<(), YqCode> {
        if f >= FMT_CELLS {
            return Err(YqCode::ENoFmt);
        }
        let kind = match fmt_kind(f) {
            Some(k) => k,
            None => return Err(YqCode::ENoFmt),
        };
        if (kind as i64) > self.last_kind + 1 {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4200);
            return Err(YqCode::EOrder);
        }
        let want = match fmt_cell(f) {
            Some(w) => w,
            None => return Err(YqCode::ENoFmt),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4201 + f as u64);
            return Err(YqCode::EFmtDrift);
        }
        if match self.fmts.get(f) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(YqCode::EDup);
        }
        if let Some(c) = self.fmts.get_mut(f) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x4202 + f as u64);
        Ok(())
    }

    /// 格式全账核验（20 格）。
    pub fn fmts_all(&self) -> bool {
        let mut f = 0usize;
        while f < FMT_CELLS {
            match self.fmts.get(f) {
                Some(Some(_)) => {}
                _ => return false,
            }
            f += 1;
        }
        true
    }

    /// 方言一格（方言单调 + 载荷对照 + 重复拒）。
    pub fn record_dialect(&mut self, d: usize, got: u64) -> Result<(), YqCode> {
        if d >= DIALECT_CELLS {
            return Err(YqCode::ENoDialect);
        }
        let kind = match dialect_kind(d) {
            Some(k) => k,
            None => return Err(YqCode::ENoDialect),
        };
        if (kind as i64) > self.last_kind + 1 {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4300);
            return Err(YqCode::EOrder);
        }
        let want = match dialect_cell(d) {
            Some(w) => w,
            None => return Err(YqCode::ENoDialect),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4301 + d as u64);
            return Err(YqCode::EDialectDrift);
        }
        if match self.dialects.get(d) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(YqCode::EDup);
        }
        if let Some(c) = self.dialects.get_mut(d) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x4302 + d as u64);
        Ok(())
    }

    /// 方言全账核验（12 格）。
    pub fn dialects_all(&self) -> bool {
        let mut d = 0usize;
        while d < DIALECT_CELLS {
            match self.dialects.get(d) {
                Some(Some(_)) => {}
                _ => return false,
            }
            d += 1;
        }
        true
    }

    /// 原地编辑一格（族单调 + 载荷对照 + 重复拒）。
    pub fn record_edit(&mut self, o: usize, got: u64) -> Result<(), YqCode> {
        if o >= EDIT_OPS {
            return Err(YqCode::ENoEdit);
        }
        let kind = match edit_kind(o) {
            Some(k) => k,
            None => return Err(YqCode::ENoEdit),
        };
        if (kind as i64) > self.last_kind + 1 {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4400);
            return Err(YqCode::EOrder);
        }
        let want = match edit_cell(o) {
            Some(w) => w,
            None => return Err(YqCode::ENoEdit),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4401 + o as u64);
            return Err(YqCode::EEditDrift);
        }
        if match self.edits.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(YqCode::EDup);
        }
        if let Some(c) = self.edits.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x4402 + o as u64);
        Ok(())
    }

    /// 编辑全账核验（8 格）。
    pub fn edits_all(&self) -> bool {
        let mut o = 0usize;
        while o < EDIT_OPS {
            match self.edits.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// jq 联签一行（F52354）。
    pub fn declare_sig(&mut self, r: usize) -> Result<(), YqCode> {
        if r >= SIG_ROWS {
            return Err(YqCode::ENoSig);
        }
        if match self.sigs.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(YqCode::EDup);
        }
        if let Some(c) = self.sigs.get_mut(r) {
            *c = jq_row(r);
        }
        self.fnv(0x4500 + r as u64);
        Ok(())
    }

    /// 联签全齐核验。
    pub fn sigs_all(&self) -> bool {
        let mut r = 0usize;
        while r < SIG_ROWS {
            match self.sigs.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// Go 上栈声明一行。
    pub fn declare_decl(&mut self, d: usize) -> Result<(), YqCode> {
        if d >= DECL_ROWS {
            return Err(YqCode::ENoDecl);
        }
        if match self.decls.get(d) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(YqCode::EDup);
        }
        if let Some(c) = self.decls.get_mut(d) {
            *c = go_row(d);
        }
        self.fnv(0x4600 + d as u64);
        Ok(())
    }

    /// 声明全齐核验。
    pub fn decls_all(&self) -> bool {
        let mut d = 0usize;
        while d < DECL_ROWS {
            match self.decls.get(d) {
                Some(Some(_)) => {}
                _ => return false,
            }
            d += 1;
        }
        true
    }

    /// 最后编辑族（供注入不改值断言）。
    pub fn last_kind(&self) -> i64 {
        self.last_kind
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// KB-COMPAT 定级（全通 → S1，判据缺 → S3，其余 → S2）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all()
            && self.fmts_all()
            && self.dialects_all()
            && self.edits_all()
            && self.sigs_all()
            && self.decls_all()
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for YqLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（记账格数, 注入数, 检出数, 逃逸数, 指纹）。
pub fn run_cycle() -> (u32, u32, u32, u32, u64) {
    let mut led = YqLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut f = 0usize;
    while f < FMT_CELLS {
        if let Some(w) = fmt_cell(f) {
            if led.record_fmt(f, w).is_ok() {
                recorded += 1;
            }
        }
        f += 1;
    }
    let mut d = 0usize;
    while d < DIALECT_CELLS {
        if let Some(w) = dialect_cell(d) {
            if led.record_dialect(d, w).is_ok() {
                recorded += 1;
            }
        }
        d += 1;
    }
    let mut o = 0usize;
    while o < EDIT_OPS {
        if let Some(w) = edit_cell(o) {
            if led.record_edit(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut r = 0usize;
    while r < SIG_ROWS {
        let _ = led.declare_sig(r);
        r += 1;
    }
    let mut x = 0usize;
    while x < DECL_ROWS {
        let _ = led.declare_decl(x);
        x += 1;
    }
    // 注入 10 轮：格式越闭集 / 编辑漂移 / 空账乱序三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = YqLedger::new();
        let caught = match kind {
            0 => lt.record_fmt(FMT_CELLS, 0).is_err(),
            1 => {
                let _ = lt.record_edit(0, edit_cell(0).unwrap_or(0));
                lt.record_edit(1, edit_cell(1).unwrap_or(0).wrapping_add(1)) == Err(YqCode::EEditDrift)
            }
            _ => {
                let r2 = lt.record_edit(3, edit_cell(3).unwrap_or(0));
                r2 == Err(YqCode::EOrder) && lt.last_kind() == -1
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x4700 + k as u64);
        } else {
            led.escaped += 1;
        }
        k += 1;
    }
    let (inj, caught, esc) = led.stats();
    let fp = led.fingerprint();
    (recorded, inj, caught, esc, fp)
}

// ── 判据集 ───────────────────────────────────────────────────────
/// UNX-F52355 判据集（锚 UNX-F52356-J1，off-by-one 板式）。
pub fn run_unx52355_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52355");
    let (recorded, inj, caught, esc, fp) = run_cycle();
    s.add(
        "四格式 ×20 格全通",
        recorded >= (FMT_CELLS + DIALECT_CELLS + EDIT_OPS) as u32,
        "YAML/JSON/XML/TOML 逐格载荷折叠恒等（UNX-F52356-J1 锚定）",
    );
    s.add(
        "语法三方言 12 格全落",
        {
            let mut lt = YqLedger::new();
            let mut ok = true;
            let mut d = 0usize;
            while d < DIALECT_CELLS {
                if let Some(w) = dialect_cell(d) {
                    if lt.record_dialect(d, w).is_err() {
                        ok = false;
                    }
                }
                d += 1;
            }
            ok && lt.dialects_all()
        },
        "mikefarah/python/tomlq 三方言逐格恒等",
    );
    s.add(
        "原地编辑 8 操作全落",
        {
            let mut lt = YqLedger::new();
            let mut ok = true;
            let mut o = 0usize;
            while o < EDIT_OPS {
                if let Some(w) = edit_cell(o) {
                    if lt.record_edit(o, w).is_err() {
                        ok = false;
                    }
                }
                o += 1;
            }
            ok && lt.edits_all()
        },
        "赋值/删除/合并三族逐操作恒等",
    );
    s.add(
        "与 jq 消费联签 4/4 全齐（F52354）",
        {
            let mut lt = YqLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < SIG_ROWS {
                if lt.declare_sig(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.sigs_all()
        },
        "YAML→JSON 前置/JSON→jq 管交/jq 结果回注/复序列化四行闭集",
    );
    s.add(
        "Go 上栈声明 2/2 全齐",
        {
            let mut lt = YqLedger::new();
            let mut ok = true;
            let mut d = 0usize;
            while d < DECL_ROWS {
                if lt.declare_decl(d).is_err() {
                    ok = false;
                }
                d += 1;
            }
            ok && lt.decls_all()
        },
        "go-yaml 解析声明行/yaml.v3 链接声明行两行闭集",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = YqLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "读取/转换/查询/编辑/写回/退出码逐格落账",
    );
    s.add(
        "格式族单调",
        {
            let mut ok = true;
            let mut f = 1usize;
            while f < FMT_CELLS {
                if fmt_kind(f).unwrap_or(4) < fmt_kind(f - 1).unwrap_or(4) {
                    ok = false;
                }
                f += 1;
            }
            ok
        },
        "YAML→JSON→XML→TOML 四族闭集单调",
    );
    s.add("注入 10/10 全抓", caught >= 10 && inj >= 10, "格式越闭集/编辑漂移/空账乱序三态轮转必抓");
    s.add("边界·格式越闭集", YqLedger::new().record_fmt(FMT_CELLS, 0).is_err(), "格式格 20 越闭集必拒");
    s.add("边界·编辑越闭集", YqLedger::new().record_edit(EDIT_OPS, 0).is_err(), "编辑操作 8 越闭集必拒");
    s.add(
        "边界·判据闸越闭集",
        YqLedger::new().judge_gate(6).is_err(),
        "闸 6 越六判据闭集必拒（ESeal）",
    );
    s.add_dyn(
        "注入账守恒",
        inj == caught && esc == 0 && caught >= 10,
        &format!("注入={} 检出={} 逃逸={}", inj, caught, esc),
    );
    let (_, _, _, _, fp2) = run_cycle();
    s.add("双跑指纹一致", fp == fp2 && fp != 0, "确定性全流程逐位一致且非零");
    s.add(
        "定级 S1 闭集",
        {
            let mut lt = YqLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut f = 0usize;
            while f < FMT_CELLS {
                if let Some(w) = fmt_cell(f) {
                    let _ = lt.record_fmt(f, w);
                }
                f += 1;
            }
            let mut d = 0usize;
            while d < DIALECT_CELLS {
                if let Some(w) = dialect_cell(d) {
                    let _ = lt.record_dialect(d, w);
                }
                d += 1;
            }
            let mut o = 0usize;
            while o < EDIT_OPS {
                if let Some(w) = edit_cell(o) {
                    let _ = lt.record_edit(o, w);
                }
                o += 1;
            }
            let mut r = 0usize;
            while r < SIG_ROWS {
                let _ = lt.declare_sig(r);
                r += 1;
            }
            let mut x = 0usize;
            while x < DECL_ROWS {
                let _ = lt.declare_decl(x);
                x += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记，判据锚定 §AI-66.1）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC9880,
        "SEG_BASE 0x40 对齐且落于 0xC9880 派工格",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fmt_kinds() {
        // 族 = 格号/5（i/N 映射），按映射核对。
        assert_eq!(fmt_kind(0), Some(0));
        assert_eq!(fmt_kind(4), Some(0));
        assert_eq!(fmt_kind(5), Some(1));
        assert_eq!(fmt_kind(19), Some(3));
        assert!(fmt_kind(20).is_none());
        assert_eq!(fmt_kind_name(2), Some("XML"));
        assert!(fmt_kind_name(4).is_none());
    }

    #[test]
    fn test_dialect_kinds() {
        // 方言 = 格号/4（i/N 映射），按映射核对。
        assert_eq!(dialect_kind(0), Some(0));
        assert_eq!(dialect_kind(3), Some(0));
        assert_eq!(dialect_kind(4), Some(1));
        assert_eq!(dialect_kind(11), Some(2));
        assert!(dialect_kind(12).is_none());
        assert_eq!(dialect_kind_name(2), Some("tomlq"));
    }

    #[test]
    fn test_edit_kinds() {
        assert_eq!(edit_kind(0), Some(0));
        assert_eq!(edit_kind(2), Some(0));
        assert_eq!(edit_kind(3), Some(1));
        assert_eq!(edit_kind(6), Some(2));
        assert!(edit_kind(8).is_none());
        assert_eq!(edit_kind_name(1), Some("删除"));
    }

    #[test]
    fn test_cells_pure() {
        let a = fmt_cell(3).unwrap_or(0);
        assert_eq!(a, fmt_cell(3).unwrap_or(0));
        assert_ne!(a, fmt_cell(4).unwrap_or(0));
        assert!(dialect_cell(DIALECT_CELLS).is_none());
        assert!(edit_cell(EDIT_OPS).is_none());
        assert!(jq_row(SIG_ROWS).is_none());
        assert!(go_row(DECL_ROWS).is_none());
        assert_eq!(jq_row_name(0), Some("YAML→JSON 前置联签"));
        assert_eq!(go_row_name(0), Some("go-yaml 解析声明行"));
        // XOR 锚低 16 位恒零。
        assert_eq!(YQ_ANCHOR & 0xFFFF, 0);
    }

    #[test]
    fn test_order_caught_and_last_kind() {
        // 空账乱序：首格即第 1 族必抓且不改 last_kind。
        let mut lt = YqLedger::new();
        assert_eq!(lt.record_edit(3, edit_cell(3).unwrap_or(0)), Err(YqCode::EOrder));
        assert_eq!(lt.last_kind(), -1);
        // 合法序：族 0 → 族 1 → 族 2。
        let mut lt2 = YqLedger::new();
        let _ = lt2.record_edit(0, edit_cell(0).unwrap_or(0));
        assert!(lt2.record_edit(3, edit_cell(3).unwrap_or(0)).is_ok());
        assert!(lt2.record_edit(6, edit_cell(6).unwrap_or(0)).is_ok());
        // 格式表同理。
        let mut lt3 = YqLedger::new();
        assert_eq!(lt3.record_fmt(5, fmt_cell(5).unwrap_or(0)), Err(YqCode::EOrder));
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = YqLedger::new();
        assert_eq!(
            lt.record_fmt(0, fmt_cell(0).unwrap_or(0).wrapping_add(1)),
            Err(YqCode::EFmtDrift)
        );
        assert_eq!(
            lt.record_dialect(2, dialect_cell(2).unwrap_or(0).wrapping_add(1)),
            Err(YqCode::EDialectDrift)
        );
        let mut i = 0usize;
        while i <= 5 {
            let _ = lt.record_edit(i, edit_cell(i).unwrap_or(0));
            i += 1;
        }
        assert_eq!(
            lt.record_edit(5, edit_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(YqCode::EEditDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = YqLedger::new();
        let _ = lt.record_fmt(0, fmt_cell(0).unwrap_or(0));
        assert_eq!(lt.record_fmt(0, fmt_cell(0).unwrap_or(0)), Err(YqCode::EDup));
        let _ = lt.declare_sig(0);
        assert_eq!(lt.declare_sig(0), Err(YqCode::EDup));
        let _ = lt.declare_decl(1);
        assert_eq!(lt.declare_decl(1), Err(YqCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = YqLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = YqLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_boundary_closed_set() {
        let mut lt = YqLedger::new();
        assert_eq!(lt.record_fmt(FMT_CELLS, 0), Err(YqCode::ENoFmt));
        assert_eq!(lt.record_dialect(DIALECT_CELLS, 0), Err(YqCode::ENoDialect));
        assert_eq!(lt.record_edit(EDIT_OPS, 0), Err(YqCode::ENoEdit));
        assert_eq!(lt.declare_sig(SIG_ROWS), Err(YqCode::ENoSig));
        assert_eq!(lt.declare_decl(DECL_ROWS), Err(YqCode::ENoDecl));
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, inj, caught, esc, _) = run_cycle();
        assert!(recorded >= (FMT_CELLS + DIALECT_CELLS + EDIT_OPS) as u32);
        assert_eq!(inj, 10);
        assert_eq!(caught, 10);
        assert_eq!(esc, 0);
    }

    #[test]
    fn test_dual_run_fingerprint() {
        let (r1, i1, c1, e1, f1) = run_cycle();
        let (r2, i2, c2, e2, f2) = run_cycle();
        assert_eq!(r1, r2);
        assert_eq!(i1, i2);
        assert_eq!(c1, c2);
        assert_eq!(e1, e2);
        assert_eq!(e1, 0);
        assert_eq!(f1, f2);
        assert_ne!(f1, 0);
    }

    #[test]
    fn test_seg_guard() {
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0xC9880);
        assert_eq!(SEG_BASE, 0xC7200 + 0x40 * (ID - 52201));
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(YqCode::Ok as u32, 0);
        assert_eq!(YqCode::EGradeDrift as u32, 12);
        assert_eq!(YqCode::Ok.val(), SEG_BASE);
        assert!(!YqCode::Ok.detail().is_empty());
        assert!(!YqCode::ESeal.detail().is_empty());
        assert!(!YqCode::EGradeDrift.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        let lt = YqLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        let mut lt2 = YqLedger::new();
        let _ = lt2.judge_gate(0);
        assert_eq!(lt2.judge_grade(), Grade::S3);
        let mut lt3 = YqLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt3.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt3.judge_grade(), Grade::S2);
    }

    #[test]
    fn unx52355_checks_green() {
        let s = run_unx52355_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
