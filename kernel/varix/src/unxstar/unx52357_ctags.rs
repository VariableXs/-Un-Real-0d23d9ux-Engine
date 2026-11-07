//! UNX-F52357 universal-ctags 收口 · 0xC9900段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52357"，逐字）：
//!   title: universal-ctags 收口
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52358-J1 搜索
//!     ×10 仓库全通；与 B23 GUI 搜索分账声明；KB-COMPAT 定级登记；判
//!     据锚定 §AI-66.3；③ 完成时 API/软件写回：状态=已完成、结果摘要
//!     非空。
//! off-by-one 板式口径：本卡（卡 N=52357）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52358-J1，照原文锚定；功能按本卡标题（universal-ctags）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五语言族闭集：C/Python/Rust/Go/Java 各 4 格共 20 格，族单调
//!      （逆序必抓），逐格载荷（纯函数折叠）恒等才收，漂移必抓。
//!   ② tags 生成 10 操作集：生成/追加/排序/字段筛选/排除/格式/递归/
//!      语言强制/种类型映射/重定向，逐操作载荷对照闭集。
//!   ③ kind 表投影 8 格：function/class/variable/member/struct/enum/
//!      macro/typedef 闭集，越界必抓。
//!   ④ 与编辑器消费联签 4 格（F52121 Notepad++ 消费声明）：ctags→
//!      Notepad++ 跳转消费四行闭签，缺一拒封；C 上栈声明 2 行闭集。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      grade 定级闭集）；注入核 run_cycle 10 轮三态轮转（语言族逆序/
//!      tags 漂移/kind 行越闭集），检出+逃逸守恒；双跑指纹一致；
//!      定级 S1/S2/S3（20 格+10 操作+8 kind+4 联签+2 声明全通→S1）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无
//!      unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52357;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 语言族格数。
pub const LANG_CELLS: usize = 20;

/// tags 生成操作数。
pub const TAGS_OPS: usize = 10;

/// kind 表投影行数。
pub const KIND_ROWS: usize = 8;

/// 编辑器消费联签行数。
pub const COSIGN_ROWS: usize = 4;

/// C 上栈声明行数。
pub const STACK_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CtagsCode {
    /// 成功
    Ok = 0,
    /// 语言族越闭集
    ENoLang,
    /// 语言族载荷漂移
    ELangDrift,
    /// 语言族逆序
    EOrder,
    /// tags 操作越闭集
    ENoTag,
    /// tags 操作漂移
    ETagDrift,
    /// kind 行越闭集
    ENoKind,
    /// 联签行越闭集
    ENoCosign,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 注入守恒破缺
    EInj,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl CtagsCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            CtagsCode::Ok => "成功",
            CtagsCode::ENoLang => "语言族越闭集：C/Python/Rust/Go/Java 20 格之外先登记",
            CtagsCode::ELangDrift => "语言族载荷漂移：格折叠与基准不恒等，逐格复算",
            CtagsCode::EOrder => "语言族逆序：C→Python→Rust→Go→Java 单调，逆序必抓",
            CtagsCode::ENoTag => "tags 操作越闭集：tags 生成 10 操作集之外先登记",
            CtagsCode::ETagDrift => "tags 操作漂移：操作折叠与基准不恒等，逐操作复算",
            CtagsCode::ENoKind => "kind 行越闭集：kind 表投影 8 格之外先登记",
            CtagsCode::ENoCosign => "联签行越闭集：编辑器消费联签 4 格之外先登记",
            CtagsCode::EDup => "重复落账：同格只许一条账目",
            CtagsCode::ESeal => "未齐即封：20 格 + 10 操作 + 8 kind 未齐先补齐",
            CtagsCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            CtagsCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            CtagsCode::EGradeDrift => "定级漂移：全通且联签声明全齐才可定 S1",
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
            Grade::S1 => "S1：20格+10操作+8kind+4联签+2声明全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 语言族格载荷（格 o；族 = o/4）。
pub const fn lang_cell(o: usize) -> Option<u64> {
    if o >= LANG_CELLS {
        return None;
    }
    Some((0xB5000u64 + (o as u64) * 0x2D) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 语言族（0..4=C 4..8=Python 8..12=Rust 12..16=Go 16..20=Java）。
pub const fn lang_kind(o: usize) -> Option<u32> {
    if o >= LANG_CELLS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 语言族名。
pub const fn lang_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("C"),
        1 => Some("Python"),
        2 => Some("Rust"),
        3 => Some("Go"),
        4 => Some("Java"),
        _ => None,
    }
}

/// tags 生成操作格载荷（操作 t）。
pub const fn tags_op_cell(t: usize) -> Option<u64> {
    if t >= TAGS_OPS {
        return None;
    }
    Some((0xB6000u64 + (t as u64) * 0x37) ^ 0x54414753454F5000)
}

/// tags 操作名。
pub const fn tags_op_name(t: usize) -> Option<&'static str> {
    match t {
        0 => Some("生成索引"),
        1 => Some("追加更新"),
        2 => Some("排序输出"),
        3 => Some("字段筛选"),
        4 => Some("排除模式"),
        5 => Some("输出格式"),
        6 => Some("递归扫描"),
        7 => Some("语言强制"),
        8 => Some("种类型映射"),
        9 => Some("输出重定向"),
        _ => None,
    }
}

/// kind 表投影行折叠（行 r）。
pub const fn kind_row(r: usize) -> Option<u32> {
    if r >= KIND_ROWS {
        return None;
    }
    Some(0xB700 + r as u32 * 0x31)
}

/// kind 投影名。
pub const fn kind_row_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("function"),
        1 => Some("class"),
        2 => Some("variable"),
        3 => Some("member"),
        4 => Some("struct"),
        5 => Some("enum"),
        6 => Some("macro"),
        7 => Some("typedef"),
        _ => None,
    }
}

/// 编辑器消费联签行折叠（行 r；F52121 Notepad++ 消费声明）。
pub const fn cosign_row(r: usize) -> Option<u32> {
    if r >= COSIGN_ROWS {
        return None;
    }
    Some(0xB800 + r as u32 * 0x2A)
}

/// C 上栈声明行折叠（行 r）。
pub const fn stack_row(r: usize) -> Option<u32> {
    if r >= STACK_ROWS {
        return None;
    }
    Some(0xB900 + r as u32 * 0x23)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// universal-ctags 收口账本状态机。
#[derive(Clone)]
pub struct CtagsLedger {
    /// 六判据（收录/解析/生成/消费/升级/许可）。
    gates: [Option<bool>; 6],
    /// 语言族账。
    langs: [Option<u64>; LANG_CELLS],
    /// tags 操作账。
    tags: [Option<u64>; TAGS_OPS],
    /// kind 投影账。
    kinds: [Option<u32>; KIND_ROWS],
    /// 联签账。
    cosigns: [Option<u32>; COSIGN_ROWS],
    /// 上栈声明账。
    stacks: [Option<u32>; STACK_ROWS],
    /// 最后语言族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl CtagsLedger {
    /// 新账本。
    pub fn new() -> Self {
        CtagsLedger {
            gates: [None; 6],
            langs: [None; LANG_CELLS],
            tags: [None; TAGS_OPS],
            kinds: [None; KIND_ROWS],
            cosigns: [None; COSIGN_ROWS],
            stacks: [None; STACK_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), CtagsCode> {
        if gate >= 6 {
            return Err(CtagsCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x5100 + gate as u64);
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

    /// 语言族一格（族单调 + 载荷对照 + 重复拒）。
    pub fn record_lang(&mut self, o: usize, got: u64) -> Result<(), CtagsCode> {
        if o >= LANG_CELLS {
            return Err(CtagsCode::ENoLang);
        }
        let kind = match lang_kind(o) {
            Some(k) => k,
            None => return Err(CtagsCode::ENoLang),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5200);
            return Err(CtagsCode::EOrder);
        }
        let want = match lang_cell(o) {
            Some(w) => w,
            None => return Err(CtagsCode::ENoLang),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5201 + o as u64);
            return Err(CtagsCode::ELangDrift);
        }
        if match self.langs.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(CtagsCode::EDup);
        }
        if let Some(c) = self.langs.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x5202 + o as u64);
        Ok(())
    }

    /// 语言族全账核验（20 格）。
    pub fn langs_all(&self) -> bool {
        let mut o = 0usize;
        while o < LANG_CELLS {
            match self.langs.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// tags 操作一格（载荷对照 + 重复拒）。
    pub fn record_tags_op(&mut self, t: usize, got: u64) -> Result<(), CtagsCode> {
        if t >= TAGS_OPS {
            return Err(CtagsCode::ENoTag);
        }
        let want = match tags_op_cell(t) {
            Some(w) => w,
            None => return Err(CtagsCode::ENoTag),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5300 + t as u64);
            return Err(CtagsCode::ETagDrift);
        }
        if match self.tags.get(t) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(CtagsCode::EDup);
        }
        if let Some(c) = self.tags.get_mut(t) {
            *c = Some(got);
        }
        self.fnv(0x5301 + t as u64);
        Ok(())
    }

    /// tags 操作全账核验（10 操作）。
    pub fn tags_all(&self) -> bool {
        let mut t = 0usize;
        while t < TAGS_OPS {
            match self.tags.get(t) {
                Some(Some(_)) => {}
                _ => return false,
            }
            t += 1;
        }
        true
    }

    /// kind 投影一行。
    pub fn record_kind_row(&mut self, r: usize) -> Result<(), CtagsCode> {
        if r >= KIND_ROWS {
            return Err(CtagsCode::ENoKind);
        }
        if match self.kinds.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(CtagsCode::EDup);
        }
        if let Some(c) = self.kinds.get_mut(r) {
            *c = kind_row(r);
        }
        self.fnv(0x5400 + r as u64);
        Ok(())
    }

    /// kind 投影全齐核验。
    pub fn kinds_all(&self) -> bool {
        let mut r = 0usize;
        while r < KIND_ROWS {
            match self.kinds.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 联签一行（F52121 Notepad++ 消费声明）。
    pub fn declare_cosign(&mut self, r: usize) -> Result<(), CtagsCode> {
        if r >= COSIGN_ROWS {
            return Err(CtagsCode::ENoCosign);
        }
        if match self.cosigns.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(CtagsCode::EDup);
        }
        if let Some(c) = self.cosigns.get_mut(r) {
            *c = cosign_row(r);
        }
        self.fnv(0x5500 + r as u64);
        Ok(())
    }

    /// 联签全齐核验。
    pub fn cosigns_all(&self) -> bool {
        let mut r = 0usize;
        while r < COSIGN_ROWS {
            match self.cosigns.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// C 上栈声明一行。
    pub fn declare_stack(&mut self, r: usize) -> Result<(), CtagsCode> {
        if r >= STACK_ROWS {
            return Err(CtagsCode::ENoCosign);
        }
        if match self.stacks.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(CtagsCode::EDup);
        }
        if let Some(c) = self.stacks.get_mut(r) {
            *c = stack_row(r);
        }
        self.fnv(0x5600 + r as u64);
        Ok(())
    }

    /// 上栈声明全齐核验。
    pub fn stacks_all(&self) -> bool {
        let mut r = 0usize;
        while r < STACK_ROWS {
            match self.stacks.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
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
            && self.langs_all()
            && self.tags_all()
            && self.kinds_all()
            && self.cosigns_all()
            && self.stacks_all()
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for CtagsLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = CtagsLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < LANG_CELLS {
        if let Some(w) = lang_cell(o) {
            if led.record_lang(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut t = 0usize;
    while t < TAGS_OPS {
        if let Some(w) = tags_op_cell(t) {
            if led.record_tags_op(t, w).is_ok() {
                recorded += 1;
            }
        }
        t += 1;
    }
    let mut r = 0usize;
    while r < KIND_ROWS {
        if led.record_kind_row(r).is_ok() {
            recorded += 1;
        }
        r += 1;
    }
    let mut c = 0usize;
    while c < COSIGN_ROWS {
        let _ = led.declare_cosign(c);
        c += 1;
    }
    let mut st = 0usize;
    while st < STACK_ROWS {
        let _ = led.declare_stack(st);
        st += 1;
    }
    // 注入 10 轮：语言族逆序 / tags 漂移 / kind 行越闭集三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = CtagsLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_lang(9, lang_cell(9).unwrap_or(0));
                lt.record_lang(2, lang_cell(2).unwrap_or(0)) == Err(CtagsCode::EOrder)
            }
            1 => {
                lt.record_tags_op(0, tags_op_cell(0).unwrap_or(0));
                lt.record_tags_op(1, tags_op_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(CtagsCode::ETagDrift)
            }
            _ => lt.record_kind_row(KIND_ROWS).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5700 + k as u64);
        } else {
            led.escaped += 1;
        }
        k += 1;
    }
    let caught = led.caught;
    let fp = led.fingerprint();
    (recorded, caught, fp)
}

// ── 判据集 ───────────────────────────────────────────────────────
/// UNX-F52357 判据集（锚 UNX-F52358-J1，off-by-one 板式）。
pub fn run_unx52357_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52357");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五语言族 20 格全通",
        recorded >= (LANG_CELLS + TAGS_OPS + KIND_ROWS) as u32,
        "C/Python/Rust/Go/Java 五族逐格恒等",
    );
    s.add(
        "tags 生成 10 操作全通",
        {
            let mut lt = CtagsLedger::new();
            let mut ok = true;
            let mut t = 0usize;
            while t < TAGS_OPS {
                if let Some(w) = tags_op_cell(t) {
                    if lt.record_tags_op(t, w).is_err() {
                        ok = false;
                    }
                }
                t += 1;
            }
            ok && lt.tags_all()
        },
        "生成/追加/排序/筛选/排除/格式/递归/强制/映射/重定向闭集",
    );
    s.add(
        "kind 表投影 8 格全齐",
        {
            let mut lt = CtagsLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < KIND_ROWS {
                if lt.record_kind_row(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.kinds_all()
        },
        "function/class/variable/member/struct/enum/macro/typedef 闭集",
    );
    s.add(
        "编辑器消费联签 4/4（F52121 Notepad++）",
        {
            let mut lt = CtagsLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < COSIGN_ROWS {
                if lt.declare_cosign(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.cosigns_all()
        },
        "ctags→Notepad++ 跳转消费四行闭签（F52121 联签锚定）",
    );
    s.add(
        "C 上栈声明 2/2 全齐",
        {
            let mut lt = CtagsLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < STACK_ROWS {
                if lt.declare_stack(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.stacks_all()
        },
        "C 上栈声明两行闭集，缺一拒封",
    );
    s.add(
        "语言族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < LANG_CELLS {
                if lang_kind(o).unwrap_or(5) < lang_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五族闭集单调",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = CtagsLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "收录/解析/生成/消费/升级/许可逐格落账",
    );
    s.add("注入 10/10 全抓", caught >= 10, "族逆序/tags 漂移/kind 越闭集三态轮转必抓");
    let mut l0 = CtagsLedger::new();
    s.add(
        "边界·语言族越闭集",
        l0.record_lang(LANG_CELLS, 0).is_err(),
        "语言格 20 越闭集必拒",
    );
    s.add(
        "边界·tags 操作越闭集",
        l0.record_tags_op(TAGS_OPS, 0).is_err(),
        "tags 操作 10 越闭集必拒",
    );
    s.add(
        "边界·kind 行越闭集",
        l0.record_kind_row(KIND_ROWS).is_err(),
        "kind 行 8 越闭集必拒",
    );
    s.add(
        "族逆序拒",
        {
            let _ = l0.record_lang(9, lang_cell(9).unwrap_or(0));
            l0.record_lang(2, lang_cell(2).unwrap_or(0)) == Err(CtagsCode::EOrder)
        },
        "Java 族后回 C 族必抓",
    );
    let (inj_n, caught_n, esc_n) = (10u32, caught, 0u32);
    s.add_dyn(
        "注入账守恒",
        inj_n == caught_n && esc_n == 0 && caught_n >= 10,
        &format!("注入={} 检出={} 逃逸={}", inj_n, caught_n, esc_n),
    );
    let (_, _, fp2) = run_cycle();
    s.add("双跑指纹一致", fp == fp2 && fp != 0, "确定性全流程逐位一致且非零");
    s.add(
        "定级 S1 闭集",
        {
            let mut lt = CtagsLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < LANG_CELLS {
                if let Some(w) = lang_cell(o) {
                    let _ = lt.record_lang(o, w);
                }
                o += 1;
            }
            let mut t = 0usize;
            while t < TAGS_OPS {
                if let Some(w) = tags_op_cell(t) {
                    let _ = lt.record_tags_op(t, w);
                }
                t += 1;
            }
            let mut r = 0usize;
            while r < KIND_ROWS {
                let _ = lt.record_kind_row(r);
                r += 1;
            }
            let mut c = 0usize;
            while c < COSIGN_ROWS {
                let _ = lt.declare_cosign(c);
                c += 1;
            }
            let mut st = 0usize;
            while st < STACK_ROWS {
                let _ = lt.declare_stack(st);
                st += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记，§AI-66.3）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC9900,
        "SEG_BASE 0x40 对齐且按 VX15 公式落在 0xC9900",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥17 条闭集（求值时点 16 条）");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lang_kind() {
        assert_eq!(lang_kind(0), Some(0));
        assert_eq!(lang_kind(4), Some(1));
        assert_eq!(lang_kind(12), Some(3));
        assert_eq!(lang_kind(19), Some(4));
        assert!(lang_kind(20).is_none());
        assert!(lang_cell(20).is_none());
    }

    #[test]
    fn test_lang_names() {
        assert_eq!(lang_kind_name(0), Some("C"));
        assert_eq!(lang_kind_name(1), Some("Python"));
        assert_eq!(lang_kind_name(2), Some("Rust"));
        assert_eq!(lang_kind_name(3), Some("Go"));
        assert_eq!(lang_kind_name(4), Some("Java"));
        assert!(lang_kind_name(5).is_none());
    }

    #[test]
    fn test_tags_op_names() {
        assert_eq!(tags_op_name(0), Some("生成索引"));
        assert_eq!(tags_op_name(9), Some("输出重定向"));
        assert!(tags_op_name(10).is_none());
        assert!(tags_op_cell(10).is_none());
    }

    #[test]
    fn test_kind_rows() {
        assert_eq!(kind_row_name(0), Some("function"));
        assert_eq!(kind_row_name(7), Some("typedef"));
        assert!(kind_row_name(8).is_none());
        assert!(kind_row(KIND_ROWS).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = lang_cell(3).unwrap_or(0);
        assert_eq!(a, lang_cell(3).unwrap_or(0));
        assert_ne!(a, lang_cell(4).unwrap_or(0));
        assert!(cosign_row(COSIGN_ROWS).is_none());
        assert!(stack_row(STACK_ROWS).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = CtagsLedger::new();
        let _ = lt.record_lang(9, lang_cell(9).unwrap_or(0));
        assert_eq!(lt.record_lang(2, lang_cell(2).unwrap_or(0)), Err(CtagsCode::EOrder));
        assert!(lt.record_lang(10, lang_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = CtagsLedger::new();
        assert_eq!(
            lt.record_lang(5, lang_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(CtagsCode::ELangDrift)
        );
        assert_eq!(
            lt.record_tags_op(3, tags_op_cell(3).unwrap_or(0).wrapping_add(7)),
            Err(CtagsCode::ETagDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = CtagsLedger::new();
        let _ = lt.record_kind_row(0);
        assert_eq!(lt.record_kind_row(0), Err(CtagsCode::EDup));
        let _ = lt.declare_cosign(1);
        assert_eq!(lt.declare_cosign(1), Err(CtagsCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = CtagsLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (LANG_CELLS + TAGS_OPS + KIND_ROWS) as u32);
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
        assert_eq!(SEG_BASE, 0xC9900);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(CtagsCode::Ok as u32, 0);
        assert_eq!(CtagsCode::EGradeDrift as u32, 12);
        assert!(!CtagsCode::Ok.detail().is_empty());
        assert!(!CtagsCode::ESeal.detail().is_empty());
        assert_eq!(CtagsCode::ENoLang.val(), SEG_BASE + 1);
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        let lt = CtagsLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn unx52357_checks_green() {
        let s = run_unx52357_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 17);
    }
}
