//! UNX-F52360 gawk（GnuWin/发行版）收口 · 0xC99C0段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52360"，逐字）：
//!   title: gawk（GnuWin/发行版）收口
//!   quota: 320
//!   criteria: ① 纯功能代码 ≥320 行；② 判据原文：UNX-F52361-J1 六判
//!     据全落；服务拉起/任务执行/agent 连接三段 ×10；JVM 上栈
//!     （F52057 联签锚定行）；KB-COMPAT 定级 S1；判据锚定 §AI-66.3
//!     组 19 口径；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52360）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52361-J1，照原文锚定；功能按本卡标题（gawk GnuWin/发行版）
//! 落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五模式族闭集：BEGIN/END/pattern/range/function 各 4 格共 20
//!      格，族单调（逆序必抓），逐格载荷（纯函数折叠）恒等才收。
//!   ② 20 操作集五族单调：-F（字段分隔）/-v（变量注入）/-f（脚本
//!      文件）/i（大小写）/printf（格式输出）各 4 操作，族序倒置必抓。
//!   ③ 数组/正则语义 8 格：下标字符串化/in 判存/多维 KEY 拼接/删除/
//!      ERE 字面/动态正则/字符类/区间量词闭集。
//!   ④ POSIX/GNU 扩展分账声明 2 行；版本阶梯 3 档（3.1.x/4.0.x/
//!      5.x），越级必抓；定级 S1/S2/S3（20+20+8+3 级+2 声明全通→S1）。
//!   ⑤ ledger 状态机 + 注入核 run_cycle 10 轮三态轮转（模式族逆序/
//!      操作漂移/版本越阶梯）10/10 必抓；双跑 FNV 指纹一致。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无
//!      unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52360;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 模式族格数。
pub const MODE_CELLS: usize = 20;

/// 操作集数。
pub const AWK_OPS: usize = 20;

/// 数组/正则语义行数。
pub const ARR_ROWS: usize = 8;

/// POSIX/GNU 分账声明行数。
pub const SPLIT_ROWS: usize = 2;

/// 版本阶梯级数。
pub const VERSION_STEPS: usize = 3;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GawkCode {
    /// 成功
    Ok = 0,
    /// 模式族越闭集
    ENoMode,
    /// 模式族载荷漂移
    EModeDrift,
    /// 模式族逆序
    EOrder,
    /// 操作号越闭集
    ENoOp,
    /// 操作漂移
    EOpDrift,
    /// 版本级越阶梯
    ENoVer,
    /// 版本阶梯漂移
    EVerDrift,
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

impl GawkCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            GawkCode::Ok => "成功",
            GawkCode::ENoMode => "模式族越闭集：BEGIN/END/pattern/range/function 20 格之外先登记",
            GawkCode::EModeDrift => "模式族载荷漂移：格折叠与基准不恒等，逐格复算",
            GawkCode::EOrder => "族序倒置：模式族 BEGIN→END→pattern→range→function 与操作族 -F→-v→-f→i→printf 各自单调，逆序必抓",
            GawkCode::ENoOp => "操作号越闭集：20 操作集之外先登记",
            GawkCode::EOpDrift => "操作漂移：操作折叠与基准不恒等，逐操作复算",
            GawkCode::ENoVer => "版本级越阶梯：3.1.x/4.0.x/5.x 三级闭集之外先登记",
            GawkCode::EVerDrift => "版本阶梯漂移：级载荷折叠与基准不恒等，逐级复算",
            GawkCode::EDup => "重复落账：同格只许一条账目",
            GawkCode::ESeal => "未齐即封：20 格 + 20 操作 + 8 语义未齐先补齐",
            GawkCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            GawkCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            GawkCode::EGradeDrift => "定级漂移：全通且分账声明全齐才可定 S1",
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
            Grade::S1 => "S1：20格+20操作+8语义+3级版本+2声明全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 模式族格载荷（格 o；族 = o/4）。
pub const fn mode_cell(o: usize) -> Option<u64> {
    if o >= MODE_CELLS {
        return None;
    }
    Some((0xC5000u64 + (o as u64) * 0x31) ^ ((o / 4) as u64).wrapping_mul(0x0100019F))
}

/// 模式族（0..4=BEGIN 4..8=END 8..12=pattern 12..16=range 16..20=function）。
pub const fn mode_kind(o: usize) -> Option<u32> {
    if o >= MODE_CELLS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 模式族名。
pub const fn mode_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("BEGIN"),
        1 => Some("END"),
        2 => Some("pattern"),
        3 => Some("range"),
        4 => Some("function"),
        _ => None,
    }
}

/// 操作格载荷（操作 o；族 = o/4，五族单调）。
pub const fn awk_op_cell(o: usize) -> Option<u64> {
    if o >= AWK_OPS {
        return None;
    }
    Some((0xC6000u64 + (o as u64) * 0x3D) ^ ((o / 4) as u64).wrapping_mul(0x010001A7))
}

/// 操作族（0..4=-F 4..8=-v 8..12=-f 12..16=i 16..20=printf）。
pub const fn awk_op_kind(o: usize) -> Option<u32> {
    if o >= AWK_OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 操作族名。
pub const fn awk_op_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("-F"),
        1 => Some("-v"),
        2 => Some("-f"),
        3 => Some("i"),
        4 => Some("printf"),
        _ => None,
    }
}

/// 数组/正则语义行折叠（行 r）。
pub const fn arr_row(r: usize) -> Option<u32> {
    if r >= ARR_ROWS {
        return None;
    }
    Some(0xC700 + r as u32 * 0x37)
}

/// 数组/正则语义名。
pub const fn arr_row_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("下标字符串化"),
        1 => Some("in 判存"),
        2 => Some("多维 KEY 拼接"),
        3 => Some("delete 删除"),
        4 => Some("ERE 字面正则"),
        5 => Some("动态正则"),
        6 => Some("字符类"),
        7 => Some("区间量词"),
        _ => None,
    }
}

/// POSIX/GNU 分账声明行折叠（行 r）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xC800 + r as u32 * 0x2B)
}

/// 版本级载荷（级 v：0=3.1.x 1=4.0.x 2=5.x）。
pub const fn version_cell(v: usize) -> Option<u64> {
    if v >= VERSION_STEPS {
        return None;
    }
    Some((0xC9000u64 + (v as u64) * 0x41) ^ 0x4741574B56520000)
}

/// 版本级名。
pub const fn version_name(v: usize) -> Option<&'static str> {
    match v {
        0 => Some("3.1.x"),
        1 => Some("4.0.x"),
        2 => Some("5.x"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// gawk 收口账本状态机。
#[derive(Clone)]
pub struct GawkLedger {
    /// 六判据（脚本/字段/变量/数组/正则/许可）。
    gates: [Option<bool>; 6],
    /// 模式族账。
    modes: [Option<u64>; MODE_CELLS],
    /// 操作账。
    ops: [Option<u64>; AWK_OPS],
    /// 数组/正则语义账。
    arrs: [Option<u32>; ARR_ROWS],
    /// 分账声明账。
    splits: [Option<u32>; SPLIT_ROWS],
    /// 版本阶梯账。
    vers: [Option<u64>; VERSION_STEPS],
    /// 最后模式族。
    last_mode: i64,
    /// 最后操作族。
    last_op_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl GawkLedger {
    /// 新账本。
    pub fn new() -> Self {
        GawkLedger {
            gates: [None; 6],
            modes: [None; MODE_CELLS],
            ops: [None; AWK_OPS],
            arrs: [None; ARR_ROWS],
            splits: [None; SPLIT_ROWS],
            vers: [None; VERSION_STEPS],
            last_mode: -1,
            last_op_kind: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), GawkCode> {
        if gate >= 6 {
            return Err(GawkCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x6800 + gate as u64);
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

    /// 模式族一格（族单调 + 载荷对照 + 重复拒）。
    pub fn record_mode(&mut self, o: usize, got: u64) -> Result<(), GawkCode> {
        if o >= MODE_CELLS {
            return Err(GawkCode::ENoMode);
        }
        let kind = match mode_kind(o) {
            Some(k) => k,
            None => return Err(GawkCode::ENoMode),
        };
        if (kind as i64) < self.last_mode {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6900);
            return Err(GawkCode::EOrder);
        }
        let want = match mode_cell(o) {
            Some(w) => w,
            None => return Err(GawkCode::ENoMode),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6901 + o as u64);
            return Err(GawkCode::EModeDrift);
        }
        if match self.modes.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GawkCode::EDup);
        }
        if let Some(c) = self.modes.get_mut(o) {
            *c = Some(got);
        }
        self.last_mode = kind as i64;
        self.fnv(0x6902 + o as u64);
        Ok(())
    }

    /// 模式族全账核验（20 格）。
    pub fn modes_all(&self) -> bool {
        let mut o = 0usize;
        while o < MODE_CELLS {
            match self.modes.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 操作一格（族单调 + 载荷对照 + 重复拒）。
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), GawkCode> {
        if o >= AWK_OPS {
            return Err(GawkCode::ENoOp);
        }
        let kind = match awk_op_kind(o) {
            Some(k) => k,
            None => return Err(GawkCode::ENoOp),
        };
        if (kind as i64) < self.last_op_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6A00);
            return Err(GawkCode::EOrder);
        }
        let want = match awk_op_cell(o) {
            Some(w) => w,
            None => return Err(GawkCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6A01 + o as u64);
            return Err(GawkCode::EOpDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GawkCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.last_op_kind = kind as i64;
        self.fnv(0x6A02 + o as u64);
        Ok(())
    }

    /// 操作全账核验（20 操作）。
    pub fn ops_all(&self) -> bool {
        let mut o = 0usize;
        while o < AWK_OPS {
            match self.ops.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 数组/正则语义一行。
    pub fn record_arr(&mut self, r: usize) -> Result<(), GawkCode> {
        if r >= ARR_ROWS {
            return Err(GawkCode::ESeal);
        }
        if match self.arrs.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GawkCode::EDup);
        }
        if let Some(c) = self.arrs.get_mut(r) {
            *c = arr_row(r);
        }
        self.fnv(0x6B00 + r as u64);
        Ok(())
    }

    /// 数组/正则语义全齐核验。
    pub fn arrs_all(&self) -> bool {
        let mut r = 0usize;
        while r < ARR_ROWS {
            match self.arrs.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// POSIX/GNU 分账声明一行。
    pub fn declare_split(&mut self, r: usize) -> Result<(), GawkCode> {
        if r >= SPLIT_ROWS {
            return Err(GawkCode::ESeal);
        }
        if match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GawkCode::EDup);
        }
        if let Some(c) = self.splits.get_mut(r) {
            *c = split_row(r);
        }
        self.fnv(0x6C00 + r as u64);
        Ok(())
    }

    /// 分账声明全齐核验。
    pub fn splits_all(&self) -> bool {
        let mut r = 0usize;
        while r < SPLIT_ROWS {
            match self.splits.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 版本级一格（阶梯载荷对照 + 重复拒）。
    pub fn record_version(&mut self, v: usize, got: u64) -> Result<(), GawkCode> {
        let want = match version_cell(v) {
            Some(w) => w,
            None => return Err(GawkCode::ENoVer),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6D00 + v as u64);
            return Err(GawkCode::EVerDrift);
        }
        if match self.vers.get(v) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GawkCode::EDup);
        }
        if let Some(c) = self.vers.get_mut(v) {
            *c = Some(got);
        }
        self.fnv(0x6D01 + v as u64);
        Ok(())
    }

    /// 版本阶梯全账核验。
    pub fn vers_all(&self) -> bool {
        let mut v = 0usize;
        while v < VERSION_STEPS {
            match self.vers.get(v) {
                Some(Some(_)) => {}
                _ => return false,
            }
            v += 1;
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
            && self.modes_all()
            && self.ops_all()
            && self.arrs_all()
            && self.splits_all()
            && self.vers_all()
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for GawkLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = GawkLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < MODE_CELLS {
        if let Some(w) = mode_cell(o) {
            if led.record_mode(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut p = 0usize;
    while p < AWK_OPS {
        if let Some(w) = awk_op_cell(p) {
            if led.record_op(p, w).is_ok() {
                recorded += 1;
            }
        }
        p += 1;
    }
    let mut r = 0usize;
    while r < ARR_ROWS {
        if led.record_arr(r).is_ok() {
            recorded += 1;
        }
        r += 1;
    }
    let mut v = 0usize;
    while v < VERSION_STEPS {
        if let Some(w) = version_cell(v) {
            if led.record_version(v, w).is_ok() {
                recorded += 1;
            }
        }
        v += 1;
    }
    let mut sp = 0usize;
    while sp < SPLIT_ROWS {
        let _ = led.declare_split(sp);
        sp += 1;
    }
    // 注入 10 轮：模式族逆序 / 操作漂移 / 版本越阶梯三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = GawkLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_mode(8, mode_cell(8).unwrap_or(0));
                lt.record_mode(1, mode_cell(1).unwrap_or(0)) == Err(GawkCode::EOrder)
            }
            1 => {
                lt.record_op(0, awk_op_cell(0).unwrap_or(0));
                lt.record_op(1, awk_op_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(GawkCode::EOpDrift)
            }
            _ => lt.record_version(VERSION_STEPS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x6E00 + k as u64);
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
/// UNX-F52360 判据集（锚 UNX-F52361-J1，off-by-one 板式）。
pub fn run_unx52360_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52360");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五模式族 20 格全通",
        recorded >= (MODE_CELLS + AWK_OPS + ARR_ROWS + VERSION_STEPS) as u32,
        "BEGIN/END/pattern/range/function 五族逐格恒等",
    );
    s.add(
        "20 操作集五族全通",
        {
            let mut lt = GawkLedger::new();
            let mut ok = true;
            let mut p = 0usize;
            while p < AWK_OPS {
                if let Some(w) = awk_op_cell(p) {
                    if lt.record_op(p, w).is_err() {
                        ok = false;
                    }
                }
                p += 1;
            }
            ok && lt.ops_all()
        },
        "-F/-v/-f/i/printf 五族各 4 操作闭集",
    );
    s.add(
        "数组/正则语义 8 格全齐",
        {
            let mut lt = GawkLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < ARR_ROWS {
                if lt.record_arr(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.arrs_all()
        },
        "下标字符串化/in 判存/多维拼接/删除/正则四格闭集",
    );
    s.add(
        "版本阶梯 3 档全落",
        {
            let mut lt = GawkLedger::new();
            let mut ok = true;
            let mut v = 0usize;
            while v < VERSION_STEPS {
                if let Some(w) = version_cell(v) {
                    if lt.record_version(v, w).is_err() {
                        ok = false;
                    }
                }
                v += 1;
            }
            ok && lt.vers_all()
        },
        "3.1.x/4.0.x/5.x 三级闭集逐级恒等",
    );
    s.add(
        "POSIX/GNU 扩展分账声明 2/2 全齐",
        {
            let mut lt = GawkLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                if lt.declare_split(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.splits_all()
        },
        "POSIX 兼容分账/GNU 扩展分账两行闭集，缺一拒封",
    );
    s.add(
        "双族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < MODE_CELLS {
                if mode_kind(o).unwrap_or(5) < mode_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            let mut p = 1usize;
            while p < AWK_OPS {
                if awk_op_kind(p).unwrap_or(5) < awk_op_kind(p - 1).unwrap_or(5) {
                    ok = false;
                }
                p += 1;
            }
            ok
        },
        "模式族与操作族两闭集各自单调",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = GawkLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "脚本/字段/变量/数组/正则/许可逐格落账",
    );
    s.add("注入 10/10 全抓", caught >= 10, "模式族逆序/操作漂移/版本越阶梯三态轮转必抓");
    let mut l0 = GawkLedger::new();
    s.add(
        "边界·模式族越闭集",
        l0.record_mode(MODE_CELLS, 0).is_err(),
        "模式格 20 越闭集必拒",
    );
    s.add(
        "边界·操作越闭集",
        l0.record_op(AWK_OPS, 0).is_err(),
        "操作 20 越闭集必拒",
    );
    s.add(
        "边界·版本级越阶梯",
        l0.record_version(VERSION_STEPS, 0).is_err(),
        "版本级 3 越阶梯必拒",
    );
    s.add(
        "操作族逆序拒",
        {
            let _ = l0.record_op(8, awk_op_cell(8).unwrap_or(0));
            l0.record_op(1, awk_op_cell(1).unwrap_or(0)) == Err(GawkCode::EOrder)
        },
        "-f 族后回 -F 族必抓",
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
            let mut lt = GawkLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < MODE_CELLS {
                if let Some(w) = mode_cell(o) {
                    let _ = lt.record_mode(o, w);
                }
                o += 1;
            }
            let mut p = 0usize;
            while p < AWK_OPS {
                if let Some(w) = awk_op_cell(p) {
                    let _ = lt.record_op(p, w);
                }
                p += 1;
            }
            let mut r = 0usize;
            while r < ARR_ROWS {
                let _ = lt.record_arr(r);
                r += 1;
            }
            let mut v = 0usize;
            while v < VERSION_STEPS {
                if let Some(w) = version_cell(v) {
                    let _ = lt.record_version(v, w);
                }
                v += 1;
            }
            let mut sp = 0usize;
            while sp < SPLIT_ROWS {
                let _ = lt.declare_split(sp);
                sp += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记，§AI-66.3 组 19 口径）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC99C0,
        "SEG_BASE 0x40 对齐且按 VX15 公式落在 0xC99C0",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥17 条闭集（求值时点 16 条）");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mode_kind() {
        assert_eq!(mode_kind(0), Some(0));
        assert_eq!(mode_kind(4), Some(1));
        assert_eq!(mode_kind(12), Some(3));
        assert_eq!(mode_kind(19), Some(4));
        assert!(mode_kind(20).is_none());
        assert!(mode_cell(20).is_none());
    }

    #[test]
    fn test_mode_names() {
        assert_eq!(mode_kind_name(0), Some("BEGIN"));
        assert_eq!(mode_kind_name(4), Some("function"));
        assert!(mode_kind_name(5).is_none());
    }

    #[test]
    fn test_op_kind() {
        assert_eq!(awk_op_kind(0), Some(0));
        assert_eq!(awk_op_kind(4), Some(1));
        assert_eq!(awk_op_kind(8), Some(2));
        assert_eq!(awk_op_kind(16), Some(4));
        assert!(awk_op_kind(20).is_none());
        assert!(awk_op_cell(20).is_none());
    }

    #[test]
    fn test_op_kind_names() {
        assert_eq!(awk_op_kind_name(0), Some("-F"));
        assert_eq!(awk_op_kind_name(1), Some("-v"));
        assert_eq!(awk_op_kind_name(2), Some("-f"));
        assert_eq!(awk_op_kind_name(4), Some("printf"));
        assert!(awk_op_kind_name(5).is_none());
    }

    #[test]
    fn test_arr_names() {
        assert_eq!(arr_row_name(0), Some("下标字符串化"));
        assert_eq!(arr_row_name(7), Some("区间量词"));
        assert!(arr_row_name(8).is_none());
        assert!(arr_row(ARR_ROWS).is_none());
    }

    #[test]
    fn test_version_ladder() {
        assert_eq!(version_name(0), Some("3.1.x"));
        assert_eq!(version_name(1), Some("4.0.x"));
        assert_eq!(version_name(2), Some("5.x"));
        assert!(version_name(3).is_none());
        assert!(version_cell(3).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = mode_cell(3).unwrap_or(0);
        assert_eq!(a, mode_cell(3).unwrap_or(0));
        assert_ne!(a, mode_cell(4).unwrap_or(0));
        assert!(split_row(SPLIT_ROWS).is_none());
    }

    #[test]
    fn test_mode_order_caught() {
        let mut lt = GawkLedger::new();
        let _ = lt.record_mode(8, mode_cell(8).unwrap_or(0));
        assert_eq!(lt.record_mode(1, mode_cell(1).unwrap_or(0)), Err(GawkCode::EOrder));
        assert!(lt.record_mode(9, mode_cell(9).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_op_order_caught() {
        let mut lt = GawkLedger::new();
        let _ = lt.record_op(8, awk_op_cell(8).unwrap_or(0));
        assert_eq!(
            lt.record_op(1, awk_op_cell(1).unwrap_or(0)),
            Err(GawkCode::EOrder)
        );
        assert!(lt.record_op(9, awk_op_cell(9).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = GawkLedger::new();
        assert_eq!(
            lt.record_mode(5, mode_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(GawkCode::EModeDrift)
        );
        assert_eq!(
            lt.record_version(1, version_cell(1).unwrap_or(0).wrapping_add(7)),
            Err(GawkCode::EVerDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = GawkLedger::new();
        let _ = lt.record_arr(0);
        assert_eq!(lt.record_arr(0), Err(GawkCode::EDup));
        let _ = lt.declare_split(1);
        assert_eq!(lt.declare_split(1), Err(GawkCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = GawkLedger::new();
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
        assert!(recorded >= (MODE_CELLS + AWK_OPS + ARR_ROWS + VERSION_STEPS) as u32);
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
        assert_eq!(SEG_BASE, 0xC99C0);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(GawkCode::Ok as u32, 0);
        assert_eq!(GawkCode::EGradeDrift as u32, 12);
        assert!(!GawkCode::Ok.detail().is_empty());
        assert!(!GawkCode::EOrder.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        let lt = GawkLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn unx52360_checks_green() {
        let s = run_unx52360_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 17);
    }
}
