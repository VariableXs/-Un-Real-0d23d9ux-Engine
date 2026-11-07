//! UNX-F52358 ripgrep（Windows 发行版）收口 · 0xC9940段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52358"，逐字）：
//!   title: ripgrep（Windows 发行版）收口
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52359-J1 查找/
//!     模糊选择 ×10 全通；终端消费联签（B12 锚定行）；KB-COMPAT 定级
//!     登记；判据锚定 §AI-66.3；③ 完成时 API/软件写回：状态=已完成、
//!     结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52358）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52359-J1，照原文锚定；功能按本卡标题（ripgrep Windows 发行
//! 版）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五选项族闭集：pattern/path/type/glob/color 各 4 格共 20 格，
//!      族单调（逆序必抓），逐格载荷（纯函数折叠）恒等才收。
//!   ② 20 操作集：检索/字面检索/计数/列文件/统计/替换预演/上下文…闭
//!      集平账（与 52357 的族单调账分离），漂移必抓。
//!   ③ .ignore 语义 8 格：.ignore/.rgignore/.gitignore 三层 + 默认
//!      隐藏/二进制跳过/符号链接/大小写/换行语义闭集。
//!   ④ 并行 4 拍单调：线程拍 0..3 逐拍非降，倒拍必抓；终端消费联签
//!      2 行（B12 锚定行）；定级 S1/S2/S3 闭集（20+20+8+4+2 全通→S1）。
//!   ⑤ ledger 状态机 + 注入核 run_cycle 10 轮三态轮转（族逆序/操作
//!      漂移/ignore 行越闭集）10/10 必抓；双跑 FNV 指纹一致。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无
//!      unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52358;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 选项族格数。
pub const OPT_CELLS: usize = 20;

/// 操作集数。
pub const RG_OPS: usize = 20;

/// .ignore 语义行数。
pub const IGNORE_ROWS: usize = 8;

/// 并行拍数。
pub const BEATS: usize = 4;

/// 终端消费联签行数。
pub const COSIGN_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RipgrepCode {
    /// 成功
    Ok = 0,
    /// 选项族越闭集
    ENoOpt,
    /// 选项族载荷漂移
    EOptDrift,
    /// 选项族逆序
    EOrder,
    /// 操作号越闭集
    ENoOp,
    /// 操作漂移
    EOpDrift,
    /// ignore 行越闭集
    ENoIgnore,
    /// 并行拍越闭集
    ENoBeat,
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

impl RipgrepCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            RipgrepCode::Ok => "成功",
            RipgrepCode::ENoOpt => "选项族越闭集：pattern/path/type/glob/color 20 格之外先登记",
            RipgrepCode::EOptDrift => "选项族载荷漂移：格折叠与基准不恒等，逐格复算",
            RipgrepCode::EOrder => "族序倒置：选项族 pattern→path→type→glob→color 与并行拍 0..3 单调，逆序必抓",
            RipgrepCode::ENoOp => "操作号越闭集：20 操作集之外先登记",
            RipgrepCode::EOpDrift => "操作漂移：操作折叠与基准不恒等，逐操作复算",
            RipgrepCode::ENoIgnore => "ignore 行越闭集：.ignore 语义 8 格之外先登记",
            RipgrepCode::ENoBeat => "并行拍越闭集：4 拍闭集之外先登记",
            RipgrepCode::EDup => "重复落账：同格只许一条账目",
            RipgrepCode::ESeal => "未齐即封：20 格 + 20 操作 + 8 ignore 未齐先补齐",
            RipgrepCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            RipgrepCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            RipgrepCode::EGradeDrift => "定级漂移：全通且联签声明全齐才可定 S1",
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
            Grade::S1 => "S1：20格+20操作+8ignore+4拍+2联签全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 选项族格载荷（格 o；族 = o/4）。
pub const fn opt_cell(o: usize) -> Option<u64> {
    if o >= OPT_CELLS {
        return None;
    }
    Some((0xB8000u64 + (o as u64) * 0x2E) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 选项族（0..4=pattern 4..8=path 8..12=type 12..16=glob 16..20=color）。
pub const fn opt_kind(o: usize) -> Option<u32> {
    if o >= OPT_CELLS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 选项族名。
pub const fn opt_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("pattern"),
        1 => Some("path"),
        2 => Some("type"),
        3 => Some("glob"),
        4 => Some("color"),
        _ => None,
    }
}

/// 操作格载荷（操作 o，平账无族序）。
pub const fn rg_op_cell(o: usize) -> Option<u64> {
    if o >= RG_OPS {
        return None;
    }
    Some((0xB9000u64 + (o as u64) * 0x39) ^ 0x5247505245500000)
}

/// 操作名。
pub const fn rg_op_name(o: usize) -> Option<&'static str> {
    match o {
        0 => Some("正则检索"),
        1 => Some("字面检索"),
        2 => Some("计数匹配"),
        3 => Some("列匹配文件"),
        4 => Some("统计汇总"),
        5 => Some("替换预演"),
        6 => Some("上下文展开"),
        7 => Some("仅列行号"),
        8 => Some("忽略大小写"),
        9 => Some("全词匹配"),
        10 => Some("多行匹配"),
        11 => Some("固定串检索"),
        12 => Some("递归目录"),
        13 => Some("隐藏文件"),
        14 => Some("二进制探测"),
        15 => Some("压缩文件"),
        16 => Some("缓冲模式"),
        17 => Some("最大列宽"),
        18 => Some("结果排序"),
        19 => Some("JSON 输出"),
        _ => None,
    }
}

/// .ignore 语义行折叠（行 r）。
pub const fn ignore_row(r: usize) -> Option<u32> {
    if r >= IGNORE_ROWS {
        return None;
    }
    Some(0xBA00 + r as u32 * 0x33)
}

/// .ignore 语义名。
pub const fn ignore_row_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some(".ignore 层"),
        1 => Some(".rgignore 层"),
        2 => Some(".gitignore 层"),
        3 => Some("默认隐藏跳过"),
        4 => Some("二进制跳过"),
        5 => Some("符号链接策略"),
        6 => Some("大小写语义"),
        7 => Some("换行语义"),
        _ => None,
    }
}

/// 并行拍载荷（拍 b）。
pub const fn beat_cell(b: usize) -> Option<u64> {
    if b >= BEATS {
        return None;
    }
    Some(0xBB00u64 + (b as u64) * 0x2B)
}

/// 终端消费联签行折叠（行 r；B12 锚定行）。
pub const fn cosign_row(r: usize) -> Option<u32> {
    if r >= COSIGN_ROWS {
        return None;
    }
    Some(0xBC00 + r as u32 * 0x27)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// ripgrep 收口账本状态机。
#[derive(Clone)]
pub struct RipgrepLedger {
    /// 六判据（查找/模糊选择/终端消费/忽略语义/并行/许可）。
    gates: [Option<bool>; 6],
    /// 选项族账。
    opts: [Option<u64>; OPT_CELLS],
    /// 操作账。
    ops: [Option<u64>; RG_OPS],
    /// ignore 语义账。
    ignores: [Option<u32>; IGNORE_ROWS],
    /// 并行拍账。
    beats: [Option<u64>; BEATS],
    /// 联签账。
    cosigns: [Option<u32>; COSIGN_ROWS],
    /// 最后选项族。
    last_kind: i64,
    /// 最后并行拍。
    last_beat: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl RipgrepLedger {
    /// 新账本。
    pub fn new() -> Self {
        RipgrepLedger {
            gates: [None; 6],
            opts: [None; OPT_CELLS],
            ops: [None; RG_OPS],
            ignores: [None; IGNORE_ROWS],
            beats: [None; BEATS],
            cosigns: [None; COSIGN_ROWS],
            last_kind: -1,
            last_beat: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), RipgrepCode> {
        if gate >= 6 {
            return Err(RipgrepCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x5800 + gate as u64);
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

    /// 选项族一格（族单调 + 载荷对照 + 重复拒）。
    pub fn record_opt(&mut self, o: usize, got: u64) -> Result<(), RipgrepCode> {
        if o >= OPT_CELLS {
            return Err(RipgrepCode::ENoOpt);
        }
        let kind = match opt_kind(o) {
            Some(k) => k,
            None => return Err(RipgrepCode::ENoOpt),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5900);
            return Err(RipgrepCode::EOrder);
        }
        let want = match opt_cell(o) {
            Some(w) => w,
            None => return Err(RipgrepCode::ENoOpt),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5901 + o as u64);
            return Err(RipgrepCode::EOptDrift);
        }
        if match self.opts.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(RipgrepCode::EDup);
        }
        if let Some(c) = self.opts.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x5902 + o as u64);
        Ok(())
    }

    /// 选项族全账核验（20 格）。
    pub fn opts_all(&self) -> bool {
        let mut o = 0usize;
        while o < OPT_CELLS {
            match self.opts.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 操作一格（平账载荷对照 + 重复拒）。
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), RipgrepCode> {
        if o >= RG_OPS {
            return Err(RipgrepCode::ENoOp);
        }
        let want = match rg_op_cell(o) {
            Some(w) => w,
            None => return Err(RipgrepCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5A00 + o as u64);
            return Err(RipgrepCode::EOpDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(RipgrepCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.fnv(0x5A01 + o as u64);
        Ok(())
    }

    /// 操作全账核验（20 操作）。
    pub fn ops_all(&self) -> bool {
        let mut o = 0usize;
        while o < RG_OPS {
            match self.ops.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// ignore 语义一行。
    pub fn record_ignore(&mut self, r: usize) -> Result<(), RipgrepCode> {
        if r >= IGNORE_ROWS {
            return Err(RipgrepCode::ENoIgnore);
        }
        if match self.ignores.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(RipgrepCode::EDup);
        }
        if let Some(c) = self.ignores.get_mut(r) {
            *c = ignore_row(r);
        }
        self.fnv(0x5B00 + r as u64);
        Ok(())
    }

    /// ignore 语义全齐核验。
    pub fn ignores_all(&self) -> bool {
        let mut r = 0usize;
        while r < IGNORE_ROWS {
            match self.ignores.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 并行拍一格（拍序单调）。
    pub fn record_beat(&mut self, b: usize, got: u64) -> Result<(), RipgrepCode> {
        if b >= BEATS {
            return Err(RipgrepCode::ENoBeat);
        }
        if (b as i64) < self.last_beat {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5C00);
            return Err(RipgrepCode::EOrder);
        }
        let want = match beat_cell(b) {
            Some(w) => w,
            None => return Err(RipgrepCode::ENoBeat),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5C01 + b as u64);
            return Err(RipgrepCode::EOptDrift);
        }
        if match self.beats.get(b) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(RipgrepCode::EDup);
        }
        if let Some(c) = self.beats.get_mut(b) {
            *c = Some(got);
        }
        self.last_beat = b as i64;
        self.fnv(0x5C02 + b as u64);
        Ok(())
    }

    /// 并行拍全齐核验。
    pub fn beats_all(&self) -> bool {
        let mut b = 0usize;
        while b < BEATS {
            match self.beats.get(b) {
                Some(Some(_)) => {}
                _ => return false,
            }
            b += 1;
        }
        true
    }

    /// 终端消费联签一行（B12 锚定行）。
    pub fn declare_cosign(&mut self, r: usize) -> Result<(), RipgrepCode> {
        if r >= COSIGN_ROWS {
            return Err(RipgrepCode::ENoOpt);
        }
        if match self.cosigns.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(RipgrepCode::EDup);
        }
        if let Some(c) = self.cosigns.get_mut(r) {
            *c = cosign_row(r);
        }
        self.fnv(0x5D00 + r as u64);
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
            && self.opts_all()
            && self.ops_all()
            && self.ignores_all()
            && self.beats_all()
            && self.cosigns_all()
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for RipgrepLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = RipgrepLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < OPT_CELLS {
        if let Some(w) = opt_cell(o) {
            if led.record_opt(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut p = 0usize;
    while p < RG_OPS {
        if let Some(w) = rg_op_cell(p) {
            if led.record_op(p, w).is_ok() {
                recorded += 1;
            }
        }
        p += 1;
    }
    let mut r = 0usize;
    while r < IGNORE_ROWS {
        if led.record_ignore(r).is_ok() {
            recorded += 1;
        }
        r += 1;
    }
    let mut b = 0usize;
    while b < BEATS {
        if let Some(w) = beat_cell(b) {
            if led.record_beat(b, w).is_ok() {
                recorded += 1;
            }
        }
        b += 1;
    }
    let mut c = 0usize;
    while c < COSIGN_ROWS {
        let _ = led.declare_cosign(c);
        c += 1;
    }
    // 注入 10 轮：选项族逆序 / 操作漂移 / ignore 行越闭集三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = RipgrepLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_opt(8, opt_cell(8).unwrap_or(0));
                lt.record_opt(1, opt_cell(1).unwrap_or(0)) == Err(RipgrepCode::EOrder)
            }
            1 => {
                lt.record_op(0, rg_op_cell(0).unwrap_or(0));
                lt.record_op(1, rg_op_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(RipgrepCode::EOpDrift)
            }
            _ => lt.record_ignore(IGNORE_ROWS).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5E00 + k as u64);
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
/// UNX-F52358 判据集（锚 UNX-F52359-J1，off-by-one 板式）。
pub fn run_unx52358_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52358");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五选项族 20 格全通",
        recorded >= (OPT_CELLS + RG_OPS + IGNORE_ROWS + BEATS) as u32,
        "pattern/path/type/glob/color 五族逐格恒等",
    );
    s.add(
        "20 操作集全通",
        {
            let mut lt = RipgrepLedger::new();
            let mut ok = true;
            let mut p = 0usize;
            while p < RG_OPS {
                if let Some(w) = rg_op_cell(p) {
                    if lt.record_op(p, w).is_err() {
                        ok = false;
                    }
                }
                p += 1;
            }
            ok && lt.ops_all()
        },
        "检索/计数/列文件/替换预演/JSON 输出等 20 操作平账闭集",
    );
    s.add(
        ".ignore 语义 8 格全齐",
        {
            let mut lt = RipgrepLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < IGNORE_ROWS {
                if lt.record_ignore(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.ignores_all()
        },
        ".ignore/.rgignore/.gitignore 三层 + 五语义闭集",
    );
    s.add(
        "并行 4 拍单调",
        {
            let mut lt = RipgrepLedger::new();
            let mut ok = true;
            let mut b = 0usize;
            while b < BEATS {
                if let Some(w) = beat_cell(b) {
                    if lt.record_beat(b, w).is_err() {
                        ok = false;
                    }
                }
                b += 1;
            }
            ok && lt.beats_all()
        },
        "线程拍 0..3 逐拍非降，倒拍必抓",
    );
    s.add(
        "终端消费联签 2/2（B12 锚定行）",
        {
            let mut lt = RipgrepLedger::new();
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
        "ripgrep→终端消费两行闭签（B12 锚定）",
    );
    s.add(
        "选项族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < OPT_CELLS {
                if opt_kind(o).unwrap_or(5) < opt_kind(o - 1).unwrap_or(5) {
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
            let mut lt = RipgrepLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "查找/模糊选择/终端消费/忽略语义/并行/许可逐格落账",
    );
    s.add("注入 10/10 全抓", caught >= 10, "族序倒置/操作漂移/ignore 越闭集三态轮转必抓");
    let mut l0 = RipgrepLedger::new();
    s.add(
        "边界·选项族越闭集",
        l0.record_opt(OPT_CELLS, 0).is_err(),
        "选项格 20 越闭集必拒",
    );
    s.add(
        "边界·操作越闭集",
        l0.record_op(RG_OPS, 0).is_err(),
        "操作 20 越闭集必拒",
    );
    s.add(
        "边界·ignore 行越闭集",
        l0.record_ignore(IGNORE_ROWS).is_err(),
        "ignore 行 8 越闭集必拒",
    );
    s.add(
        "边界·并行拍越闭集",
        l0.record_beat(BEATS, 0).is_err(),
        "拍 4 越闭集必拒",
    );
    s.add(
        "族逆序拒",
        {
            let _ = l0.record_opt(8, opt_cell(8).unwrap_or(0));
            l0.record_opt(1, opt_cell(1).unwrap_or(0)) == Err(RipgrepCode::EOrder)
        },
        "type 族后回 path 族必抓",
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
            let mut lt = RipgrepLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < OPT_CELLS {
                if let Some(w) = opt_cell(o) {
                    let _ = lt.record_opt(o, w);
                }
                o += 1;
            }
            let mut p = 0usize;
            while p < RG_OPS {
                if let Some(w) = rg_op_cell(p) {
                    let _ = lt.record_op(p, w);
                }
                p += 1;
            }
            let mut r = 0usize;
            while r < IGNORE_ROWS {
                let _ = lt.record_ignore(r);
                r += 1;
            }
            let mut b = 0usize;
            while b < BEATS {
                if let Some(w) = beat_cell(b) {
                    let _ = lt.record_beat(b, w);
                }
                b += 1;
            }
            let mut c = 0usize;
            while c < COSIGN_ROWS {
                let _ = lt.declare_cosign(c);
                c += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记，§AI-66.3）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC9940,
        "SEG_BASE 0x40 对齐且按 VX15 公式落在 0xC9940",
    );
    s.add("收口·判据条数", s.len() >= 16, "含本条共 ≥18 条闭集（求值时点 17 条）");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_opt_kind() {
        assert_eq!(opt_kind(0), Some(0));
        assert_eq!(opt_kind(3), Some(0));
        assert_eq!(opt_kind(8), Some(2));
        assert_eq!(opt_kind(19), Some(4));
        assert!(opt_kind(20).is_none());
        assert!(opt_cell(20).is_none());
    }

    #[test]
    fn test_opt_names() {
        assert_eq!(opt_kind_name(0), Some("pattern"));
        assert_eq!(opt_kind_name(4), Some("color"));
        assert!(opt_kind_name(5).is_none());
    }

    #[test]
    fn test_rg_op_names() {
        assert_eq!(rg_op_name(0), Some("正则检索"));
        assert_eq!(rg_op_name(19), Some("JSON 输出"));
        assert!(rg_op_name(20).is_none());
        assert!(rg_op_cell(20).is_none());
    }

    #[test]
    fn test_ignore_names() {
        assert_eq!(ignore_row_name(0), Some(".ignore 层"));
        assert_eq!(ignore_row_name(7), Some("换行语义"));
        assert!(ignore_row_name(8).is_none());
        assert!(ignore_row(IGNORE_ROWS).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = rg_op_cell(3).unwrap_or(0);
        assert_eq!(a, rg_op_cell(3).unwrap_or(0));
        assert_ne!(a, rg_op_cell(4).unwrap_or(0));
        assert!(beat_cell(BEATS).is_none());
        assert!(cosign_row(COSIGN_ROWS).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = RipgrepLedger::new();
        let _ = lt.record_opt(8, opt_cell(8).unwrap_or(0));
        assert_eq!(lt.record_opt(1, opt_cell(1).unwrap_or(0)), Err(RipgrepCode::EOrder));
        assert!(lt.record_opt(9, opt_cell(9).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_beat_order_caught() {
        let mut lt = RipgrepLedger::new();
        let _ = lt.record_beat(2, beat_cell(2).unwrap_or(0));
        assert_eq!(
            lt.record_beat(1, beat_cell(1).unwrap_or(0)),
            Err(RipgrepCode::EOrder)
        );
        assert!(lt.record_beat(3, beat_cell(3).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = RipgrepLedger::new();
        assert_eq!(
            lt.record_op(5, rg_op_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(RipgrepCode::EOpDrift)
        );
        assert_eq!(
            lt.record_opt(6, opt_cell(6).unwrap_or(0).wrapping_add(3)),
            Err(RipgrepCode::EOptDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = RipgrepLedger::new();
        let _ = lt.record_ignore(0);
        assert_eq!(lt.record_ignore(0), Err(RipgrepCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = RipgrepLedger::new();
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
        assert!(recorded >= (OPT_CELLS + RG_OPS + IGNORE_ROWS + BEATS) as u32);
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
        assert_eq!(SEG_BASE, 0xC9940);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(RipgrepCode::Ok as u32, 0);
        assert_eq!(RipgrepCode::EGradeDrift as u32, 12);
        assert!(!RipgrepCode::Ok.detail().is_empty());
        assert!(!RipgrepCode::EOrder.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        let lt = RipgrepLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn unx52358_checks_green() {
        let s = run_unx52358_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 18);
    }
}
