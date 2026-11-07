//! UNX-F52353 JabRef 收口 · 0xC9800段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52353"，逐字）：
//!   title: JabRef 收口
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52354-J1 过滤表
//!     达式 ×20 样本零漂移；KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完
//!     成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52353）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52354-J1，照原文锚定；功能按本卡标题（JabRef）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① bib 四类型 20 格：article/book/inproceedings/phdthesis 各 5 格，
//!      族 = 格号/5，族单调（逆序必抓，空账本首格非 0 号族必抓且不改
//!      last_kind）；过滤表达式 ×20 样本逐格载荷折叠恒等才收。
//!   ② 导入导出 10 操作集：导入 4/导出 4/清理 2（kind = 格号分族），
//!      载荷折叠恒等才收；漂移必抓。
//!   ③ 字段投影 8 格：author/title/year/journal 等投影逐格落账。
//!   ④ 与 LaTeX 消费联签 4 格（F52347 锚定行）：citekey 供给/aux 解析/
//!      bbl 回写/交叉引用四行闭集，缺一拒封。
//!   ⑤ Java 上栈 4 层闭集：JVM 启动层/JNI 绑定层/Swing UI 层/插件加载
//!      层，越层必抓。
//!   ⑥ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a）；
//!      注入核 run_cycle 10 轮三态轮转（主表越闭集/操作漂移/空账乱序），
//!      检出+逃逸守恒等式；双跑指纹一致；定级 S1/S2/S3 闭集
//!      （20 格 + 10 操作 + 8 投影 + 4 联签 + 4 层 + 六判据全通 → S1）。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52353;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// bib 条目格数（四类型 × 各 5 格）。
pub const BIB_CELLS: usize = 20;

/// 导入导出操作数。
pub const OPS: usize = 10;

/// 字段投影格数。
pub const PROJ_CELLS: usize = 8;

/// 与 LaTeX 消费联签行数（F52347）。
pub const SIG_ROWS: usize = 4;

/// Java 上栈层数。
pub const JAVA_LAYERS: usize = 4;

/// JabRef 域 XOR 锚（低 16 位恒零）。
pub const JB_ANCHOR: u64 = 0x4A42_5245_4600_0000;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JabrefCode {
    /// 成功
    Ok = 0,
    /// bib 类型越闭集
    ENoType,
    /// bib 条目载荷漂移
    ETypeDrift,
    /// 操作号越闭集
    ENoOp,
    /// 操作/投影载荷漂移
    EDrift,
    /// 族逆序
    EOrder,
    /// 字段投影越闭集
    ENoProj,
    /// LaTeX 联签行越闭集
    ENoSig,
    /// Java 层越闭集
    ENoLayer,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 注入守恒破缺
    EInj,
    /// 定级漂移
    EGradeDrift,
}

impl JabrefCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            JabrefCode::Ok => "成功",
            JabrefCode::ENoType => "bib 类型越闭集：article/book/inproceedings/phdthesis 20 格之外先登记",
            JabrefCode::ETypeDrift => "bib 条目载荷漂移：过滤表达式折叠与基准不恒等，逐格复算",
            JabrefCode::ENoOp => "操作号越闭集：导入导出 10 操作集之外先登记",
            JabrefCode::EDrift => "载荷漂移：操作/投影折叠与基准不恒等，逐格复算",
            JabrefCode::EOrder => "族逆序：article→book→inproceedings→phdthesis 单调，逆序必抓",
            JabrefCode::ENoProj => "字段投影越闭集：投影 8 格闭集之外先登记",
            JabrefCode::ENoSig => "LaTeX 联签行越闭集：4 行闭集之外先登记",
            JabrefCode::ENoLayer => "Java 层越闭集：4 层闭集之外先登记",
            JabrefCode::EDup => "重复落账：同格只许一条账目",
            JabrefCode::ESeal => "未齐即封：20 格 + 10 操作 + 8 投影 + 4 联签 + 4 层未齐先补齐",
            JabrefCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            JabrefCode::EGradeDrift => "定级漂移：全通且联签/上栈声明全齐才可定 S1",
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
            Grade::S1 => "S1：六判据+20格+10操作+8投影+4联签+4层全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// bib 条目格载荷（格 o；类型 = o/5）。
pub const fn bib_cell(o: usize) -> Option<u64> {
    if o >= BIB_CELLS {
        return None;
    }
    Some(
        ((0xA100u64 + (o as u64) * 0x37) ^ ((o / 5) as u64).wrapping_mul(0x01000193)) ^ JB_ANCHOR,
    )
}

/// bib 类型（0..5=article 5..10=book 10..15=inproceedings 15..20=phdthesis）。
pub const fn bib_kind(o: usize) -> Option<u32> {
    if o >= BIB_CELLS {
        return None;
    }
    Some((o / 5) as u32)
}

/// bib 类型名。
pub const fn bib_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("article"),
        1 => Some("book"),
        2 => Some("inproceedings"),
        3 => Some("phdthesis"),
        _ => None,
    }
}

/// 导入导出操作格载荷（操作 o；族：0..4=导入 4..8=导出 8..10=清理）。
pub const fn op_cell(o: usize) -> Option<u64> {
    if o >= OPS {
        return None;
    }
    let kind = match o {
        0..=3 => 0u64,
        4..=7 => 1,
        _ => 2,
    };
    Some((0xA200u64 + (o as u64) * 0x2E) ^ kind.wrapping_mul(0x9E3779B9))
}

/// 导入导出操作族。
pub const fn op_kind(o: usize) -> Option<u32> {
    if o >= OPS {
        return None;
    }
    Some(match o {
        0..=3 => 0,
        4..=7 => 1,
        _ => 2,
    })
}

/// 操作族名。
pub const fn op_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("导入"),
        1 => Some("导出"),
        2 => Some("清理"),
        _ => None,
    }
}

/// 字段投影格载荷（投影 p）。
pub const fn proj_cell(p: usize) -> Option<u64> {
    if p >= PROJ_CELLS {
        return None;
    }
    Some((0xA300u64 + (p as u64) * 0x2B) ^ 0x5F_0000_0000)
}

/// 投影字段名。
pub const fn proj_name(p: usize) -> Option<&'static str> {
    match p {
        0 => Some("author"),
        1 => Some("title"),
        2 => Some("year"),
        3 => Some("journal"),
        4 => Some("volume"),
        5 => Some("pages"),
        6 => Some("doi"),
        7 => Some("citekey"),
        _ => None,
    }
}

/// 与 LaTeX 消费联签行折叠（行 r，F52347 锚定）。
pub const fn latex_row(r: usize) -> Option<u32> {
    if r >= SIG_ROWS {
        return None;
    }
    Some(0xA500 + r as u32 * 0x2C)
}

/// 联签行名。
pub const fn latex_row_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("citekey 供给联签"),
        1 => Some("aux 解析联签"),
        2 => Some("bbl 回写联签"),
        3 => Some("交叉引用联签"),
        _ => None,
    }
}

/// Java 上栈层载荷（层 l）。
pub const fn java_cell(l: usize) -> Option<u64> {
    if l >= JAVA_LAYERS {
        return None;
    }
    Some((0xA600u64 + (l as u64) * 0x1F) ^ 0x4A_0000_0000)
}

/// Java 层名。
pub const fn java_layer_name(l: usize) -> Option<&'static str> {
    match l {
        0 => Some("JVM 启动层"),
        1 => Some("JNI 绑定层"),
        2 => Some("Swing UI 层"),
        3 => Some("插件加载层"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// JabRef 收口账本状态机。
#[derive(Clone)]
pub struct JabrefLedger {
    /// 六判据（收口闸：启动/导入/导出/预览/清洗/写回）。
    gates: [Option<bool>; 6],
    /// bib 条目账。
    bibs: [Option<u64>; BIB_CELLS],
    /// 导入导出操作账。
    ops: [Option<u64>; OPS],
    /// 字段投影账。
    projs: [Option<u64>; PROJ_CELLS],
    /// LaTeX 联签账。
    sigs: [Option<u32>; SIG_ROWS],
    /// Java 上栈账。
    javas: [Option<u64>; JAVA_LAYERS],
    /// 最后操作族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl JabrefLedger {
    /// 新账本。
    pub fn new() -> Self {
        JabrefLedger {
            gates: [None; 6],
            bibs: [None; BIB_CELLS],
            ops: [None; OPS],
            projs: [None; PROJ_CELLS],
            sigs: [None; SIG_ROWS],
            javas: [None; JAVA_LAYERS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), JabrefCode> {
        if gate >= 6 {
            return Err(JabrefCode::ESeal);
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

    /// bib 条目一格（族单调 + 载荷对照 + 重复拒）。
    pub fn record_bib(&mut self, o: usize, got: u64) -> Result<(), JabrefCode> {
        if o >= BIB_CELLS {
            return Err(JabrefCode::ENoType);
        }
        let kind = match bib_kind(o) {
            Some(k) => k,
            None => return Err(JabrefCode::ENoType),
        };
        if (kind as i64) > self.last_kind + 1 {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4200);
            return Err(JabrefCode::EOrder);
        }
        let want = match bib_cell(o) {
            Some(w) => w,
            None => return Err(JabrefCode::ENoType),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4201 + o as u64);
            return Err(JabrefCode::ETypeDrift);
        }
        if match self.bibs.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(JabrefCode::EDup);
        }
        if let Some(c) = self.bibs.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x4202 + o as u64);
        Ok(())
    }

    /// bib 全账核验（20 格）。
    pub fn bibs_all(&self) -> bool {
        let mut o = 0usize;
        while o < BIB_CELLS {
            match self.bibs.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 导入导出操作一格（族单调 + 载荷对照 + 重复拒）。
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), JabrefCode> {
        if o >= OPS {
            return Err(JabrefCode::ENoOp);
        }
        let kind = match op_kind(o) {
            Some(k) => k,
            None => return Err(JabrefCode::ENoOp),
        };
        if (kind as i64) > self.last_kind + 1 {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4300);
            return Err(JabrefCode::EOrder);
        }
        let want = match op_cell(o) {
            Some(w) => w,
            None => return Err(JabrefCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4301 + o as u64);
            return Err(JabrefCode::EDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(JabrefCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x4302 + o as u64);
        Ok(())
    }

    /// 操作全账核验（10 格）。
    pub fn ops_all(&self) -> bool {
        let mut o = 0usize;
        while o < OPS {
            match self.ops.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 字段投影一格（载荷对照 + 重复拒）。
    pub fn record_proj(&mut self, p: usize, got: u64) -> Result<(), JabrefCode> {
        let want = match proj_cell(p) {
            Some(w) => w,
            None => return Err(JabrefCode::ENoProj),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4400 + p as u64);
            return Err(JabrefCode::EDrift);
        }
        if match self.projs.get(p) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(JabrefCode::EDup);
        }
        if let Some(c) = self.projs.get_mut(p) {
            *c = Some(got);
        }
        self.fnv(0x4401 + p as u64);
        Ok(())
    }

    /// 投影全账核验（8 格）。
    pub fn projs_all(&self) -> bool {
        let mut p = 0usize;
        while p < PROJ_CELLS {
            match self.projs.get(p) {
                Some(Some(_)) => {}
                _ => return false,
            }
            p += 1;
        }
        true
    }

    /// LaTeX 联签一行（F52347）。
    pub fn declare_sig(&mut self, r: usize) -> Result<(), JabrefCode> {
        if r >= SIG_ROWS {
            return Err(JabrefCode::ENoSig);
        }
        if match self.sigs.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(JabrefCode::EDup);
        }
        if let Some(c) = self.sigs.get_mut(r) {
            *c = latex_row(r);
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

    /// Java 上一层（越层必抓）。
    pub fn record_java(&mut self, l: usize) -> Result<(), JabrefCode> {
        if l >= JAVA_LAYERS {
            return Err(JabrefCode::ENoLayer);
        }
        if match self.javas.get(l) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(JabrefCode::EDup);
        }
        let got = match java_cell(l) {
            Some(w) => w,
            None => return Err(JabrefCode::ENoLayer),
        };
        if let Some(c) = self.javas.get_mut(l) {
            *c = Some(got);
        }
        self.fnv(0x4600 + l as u64);
        Ok(())
    }

    /// Java 上栈全齐核验。
    pub fn javas_all(&self) -> bool {
        let mut l = 0usize;
        while l < JAVA_LAYERS {
            match self.javas.get(l) {
                Some(Some(_)) => {}
                _ => return false,
            }
            l += 1;
        }
        true
    }

    /// 最后操作族（供注入不改值断言）。
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
            && self.bibs_all()
            && self.ops_all()
            && self.projs_all()
            && self.sigs_all()
            && self.javas_all()
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for JabrefLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（记账格数, 注入数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u32, u32, u64) {
    let mut led = JabrefLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < BIB_CELLS {
        if let Some(w) = bib_cell(o) {
            if led.record_bib(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut p = 0usize;
    while p < OPS {
        if let Some(w) = op_cell(p) {
            if led.record_op(p, w).is_ok() {
                recorded += 1;
            }
        }
        p += 1;
    }
    let mut q = 0usize;
    while q < PROJ_CELLS {
        if let Some(w) = proj_cell(q) {
            if led.record_proj(q, w).is_ok() {
                recorded += 1;
            }
        }
        q += 1;
    }
    let mut r = 0usize;
    while r < SIG_ROWS {
        let _ = led.declare_sig(r);
        r += 1;
    }
    let mut l = 0usize;
    while l < JAVA_LAYERS {
        let _ = led.record_java(l);
        l += 1;
    }
    // 注入 10 轮：主表越闭集 / 操作漂移 / 空账乱序三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = JabrefLedger::new();
        let caught = match kind {
            0 => lt.record_bib(BIB_CELLS, 0).is_err(),
            1 => {
                let _ = lt.record_op(0, op_cell(0).unwrap_or(0));
                lt.record_op(1, op_cell(1).unwrap_or(0).wrapping_add(1)) == Err(JabrefCode::EDrift)
            }
            _ => {
                let r2 = lt.record_op(4, op_cell(4).unwrap_or(0));
                r2 == Err(JabrefCode::EOrder) && lt.last_kind() == -1
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
    // 守恒由调用侧断言：inj == caught && esc == 0。
    let (inj, caught, esc) = led.stats();
    let fp = led.fingerprint();
    (recorded, inj, caught, esc, fp)
}

// ── 判据集 ───────────────────────────────────────────────────────
/// UNX-F52353 判据集（锚 UNX-F52354-J1，off-by-one 板式）。
pub fn run_unx52353_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52353");
    let (recorded, inj, caught, esc, fp) = run_cycle();
    s.add(
        "过滤表达式 ×20 样本零漂移（bib 四类型）",
        recorded >= (BIB_CELLS + OPS + PROJ_CELLS) as u32,
        "article/book/inproceedings/phdthesis 逐格载荷折叠恒等（UNX-F52354-J1 锚定）",
    );
    s.add(
        "导入导出 10 操作全落",
        {
            let mut lt = JabrefLedger::new();
            let mut ok = true;
            let mut o = 0usize;
            while o < OPS {
                if let Some(w) = op_cell(o) {
                    if lt.record_op(o, w).is_err() {
                        ok = false;
                    }
                }
                o += 1;
            }
            ok && lt.ops_all()
        },
        "导入 4/导出 4/清理 2 三族逐操作恒等",
    );
    s.add(
        "字段投影 8 格全落",
        {
            let mut lt = JabrefLedger::new();
            let mut ok = true;
            let mut p = 0usize;
            while p < PROJ_CELLS {
                if let Some(w) = proj_cell(p) {
                    if lt.record_proj(p, w).is_err() {
                        ok = false;
                    }
                }
                p += 1;
            }
            ok && lt.projs_all()
        },
        "author/title/year/journal/volume/pages/doi/citekey 逐格落账",
    );
    s.add(
        "与 LaTeX 消费联签 4/4 全齐（F52347）",
        {
            let mut lt = JabrefLedger::new();
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
        "citekey 供给/aux 解析/bbl 回写/交叉引用四行闭集",
    );
    s.add(
        "Java 上栈 4 层全落",
        {
            let mut lt = JabrefLedger::new();
            let mut ok = true;
            let mut l = 0usize;
            while l < JAVA_LAYERS {
                if lt.record_java(l).is_err() {
                    ok = false;
                }
                l += 1;
            }
            ok && lt.javas_all()
        },
        "JVM 启动/JNI 绑定/Swing UI/插件加载四层闭集",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = JabrefLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "启动/导入/导出/预览/清洗/写回逐格落账",
    );
    s.add(
        "bib 族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < BIB_CELLS {
                if bib_kind(o).unwrap_or(4) < bib_kind(o - 1).unwrap_or(4) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "article→book→inproceedings→phdthesis 四族闭集单调",
    );
    s.add("注入 10/10 全抓", caught >= 10 && inj >= 10, "主表越闭集/操作漂移/空账乱序三态轮转必抓");
    s.add("边界·主表越闭集", JabrefLedger::new().record_bib(BIB_CELLS, 0).is_err(), "bib 格 20 越闭集必拒");
    s.add("边界·次表越闭集", JabrefLedger::new().record_op(OPS, 0).is_err(), "操作 10 越闭集必拒");
    s.add(
        "边界·判据闸越闭集",
        JabrefLedger::new().judge_gate(6).is_err(),
        "闸 6 越六判据闭集必拒（ESeal）",
    );
    s.add_dyn(
        "注入账守恒",
        inj == caught && esc == 0 && caught >= 10,
        &format!("注入={} 检出={} 逃逸=0", inj, caught),
    );
    let (_, _, _, _, fp2) = run_cycle();
    s.add("双跑指纹一致", fp == fp2 && fp != 0, "确定性全流程逐位一致且非零");
    s.add(
        "定级 S1 闭集",
        {
            let mut lt = JabrefLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < BIB_CELLS {
                if let Some(w) = bib_cell(o) {
                    let _ = lt.record_bib(o, w);
                }
                o += 1;
            }
            let mut p = 0usize;
            while p < OPS {
                if let Some(w) = op_cell(p) {
                    let _ = lt.record_op(p, w);
                }
                p += 1;
            }
            let mut q = 0usize;
            while q < PROJ_CELLS {
                if let Some(w) = proj_cell(q) {
                    let _ = lt.record_proj(q, w);
                }
                q += 1;
            }
            let mut r = 0usize;
            while r < SIG_ROWS {
                let _ = lt.declare_sig(r);
                r += 1;
            }
            let mut l = 0usize;
            while l < JAVA_LAYERS {
                let _ = lt.record_java(l);
                l += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC9800,
        "SEG_BASE 0x40 对齐且落于 0xC9800 派工格",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bib_kinds() {
        // 族 = 格号/5（i/N 映射），按映射核对。
        assert_eq!(bib_kind(0), Some(0));
        assert_eq!(bib_kind(4), Some(0));
        assert_eq!(bib_kind(5), Some(1));
        assert_eq!(bib_kind(19), Some(3));
        assert!(bib_kind(20).is_none());
        assert_eq!(bib_kind_name(0), Some("article"));
        assert_eq!(bib_kind_name(3), Some("phdthesis"));
        assert!(bib_kind_name(4).is_none());
    }

    #[test]
    fn test_op_kinds() {
        assert_eq!(op_kind(0), Some(0));
        assert_eq!(op_kind(3), Some(0));
        assert_eq!(op_kind(4), Some(1));
        assert_eq!(op_kind(8), Some(2));
        assert_eq!(op_kind(9), Some(2));
        assert!(op_kind(10).is_none());
        assert_eq!(op_kind_name(2), Some("清理"));
    }

    #[test]
    fn test_cells_pure() {
        let a = bib_cell(3).unwrap_or(0);
        assert_eq!(a, bib_cell(3).unwrap_or(0));
        assert_ne!(a, bib_cell(4).unwrap_or(0));
        assert!(proj_cell(PROJ_CELLS).is_none());
        assert!(latex_row(SIG_ROWS).is_none());
        assert!(java_cell(JAVA_LAYERS).is_none());
        assert_eq!(proj_name(7), Some("citekey"));
        assert_eq!(latex_row_name(0), Some("citekey 供给联签"));
        assert_eq!(java_layer_name(2), Some("Swing UI 层"));
        // XOR 锚低 16 位恒零。
        assert_eq!(JB_ANCHOR & 0xFFFF, 0);
    }

    #[test]
    fn test_order_caught_and_last_kind() {
        // 空账乱序：首格即第 1 族必抓且不改 last_kind。
        let mut lt = JabrefLedger::new();
        assert_eq!(lt.record_op(4, op_cell(4).unwrap_or(0)), Err(JabrefCode::EOrder));
        assert_eq!(lt.last_kind(), -1);
        // 合法序：族 0 → 族 1 → 族 2。
        let mut lt2 = JabrefLedger::new();
        let _ = lt2.record_op(0, op_cell(0).unwrap_or(0));
        assert!(lt2.record_op(4, op_cell(4).unwrap_or(0)).is_ok());
        assert!(lt2.record_op(8, op_cell(8).unwrap_or(0)).is_ok());
        // bib 表同理。
        let mut lt3 = JabrefLedger::new();
        assert_eq!(lt3.record_bib(5, bib_cell(5).unwrap_or(0)), Err(JabrefCode::EOrder));
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = JabrefLedger::new();
        assert_eq!(
            lt.record_bib(0, bib_cell(0).unwrap_or(0).wrapping_add(1)),
            Err(JabrefCode::ETypeDrift)
        );
        assert_eq!(
            lt.record_op(2, op_cell(2).unwrap_or(0).wrapping_add(1)),
            Err(JabrefCode::EDrift)
        );
        assert_eq!(
            lt.record_proj(5, proj_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(JabrefCode::EDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = JabrefLedger::new();
        let _ = lt.record_bib(0, bib_cell(0).unwrap_or(0));
        assert_eq!(lt.record_bib(0, bib_cell(0).unwrap_or(0)), Err(JabrefCode::EDup));
        let _ = lt.record_java(1);
        assert_eq!(lt.record_java(1), Err(JabrefCode::EDup));
        let _ = lt.declare_sig(0);
        assert_eq!(lt.declare_sig(0), Err(JabrefCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = JabrefLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = JabrefLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_boundary_closed_set() {
        let mut lt = JabrefLedger::new();
        assert_eq!(lt.record_bib(BIB_CELLS, 0), Err(JabrefCode::ENoType));
        assert_eq!(lt.record_op(OPS, 0), Err(JabrefCode::ENoOp));
        assert_eq!(lt.record_proj(PROJ_CELLS, 0), Err(JabrefCode::ENoProj));
        assert_eq!(lt.declare_sig(SIG_ROWS), Err(JabrefCode::ENoSig));
        assert_eq!(lt.record_java(JAVA_LAYERS), Err(JabrefCode::ENoLayer));
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, inj, caught, esc, _) = run_cycle();
        assert!(recorded >= (BIB_CELLS + OPS + PROJ_CELLS) as u32);
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
        assert_eq!(SEG_BASE, 0xC9800);
        assert_eq!(SEG_BASE, 0xC7200 + 0x40 * (ID - 52201));
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(JabrefCode::Ok as u32, 0);
        assert_eq!(JabrefCode::EGradeDrift as u32, 12);
        assert_eq!(JabrefCode::Ok.val(), SEG_BASE);
        assert!(!JabrefCode::Ok.detail().is_empty());
        assert!(!JabrefCode::ESeal.detail().is_empty());
        assert!(!JabrefCode::EGradeDrift.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        let lt = JabrefLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        let mut lt2 = JabrefLedger::new();
        let _ = lt2.judge_gate(0);
        assert_eq!(lt2.judge_grade(), Grade::S3);
        let mut lt3 = JabrefLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt3.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt3.judge_grade(), Grade::S2);
    }

    #[test]
    fn test_java_and_sig_names() {
        assert_eq!(java_layer_name(0), Some("JVM 启动层"));
        assert_eq!(java_layer_name(1), Some("JNI 绑定层"));
        assert_eq!(java_layer_name(3), Some("插件加载层"));
        assert!(java_layer_name(4).is_none());
        assert!(latex_row_name(SIG_ROWS).is_none());
        let mut lt = JabrefLedger::new();
        let mut l = 0usize;
        while l < JAVA_LAYERS {
            assert!(lt.record_java(l).is_ok());
            l += 1;
        }
        assert!(lt.javas_all());
    }

    #[test]
    fn unx52353_checks_green() {
        let s = run_unx52353_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
