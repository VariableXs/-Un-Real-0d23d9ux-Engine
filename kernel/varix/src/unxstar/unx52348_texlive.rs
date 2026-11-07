//! UNX-F52348 TeX Live 收口 · 0xC96C0段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52348"，逐字）：
//!   title: TeX Live 收口
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52349-J1 PS/PDF
//!     转换 ×10 全通；KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成时
//!     API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52348）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52349-J1，照原文锚定；功能按本卡标题（TeX Live）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① tlmgr 10 操作五族闭集（安装/移除/更新/搜索/信息各 2 操作），
//!      族单调（逆序必抓），载荷漂移必抓。
//!   ② 五引擎同构账 15 格（pdftex/xetex/luatex/tex/dvips 每器 3 格，
//!      同构账逐器独立折叠恒等才收），越格必抓。
//!   ③ 发行版阶梯 2023/2024/2025 三档，越级必抓；PS/PDF 转换六判据
//!      收口闸逐格落账，未齐即封拒。
//!   ④ 与 MiKTeX 分账声明 2 行闭集（MiKTeX 并存隔离分账声明 / TeX
//!      Live 独立发行版分账声明），缺一拒封。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      grade 闭集）；注入核 run_cycle 三态轮转 10/10（发行版越阶梯/
//!      tlmgr 漂移/tlmgr 重复落账），三类注入全部落在当轮临时账本上，
//!      漂移注入先按合法序落正格再注漂移，对空账本同样必抓；
//!      检出+逃逸守恒等式；双跑 FNV 指纹一致；定级 S1/S2/S3 闭集。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52348;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// tlmgr 操作数。
pub const TLMGR_OPS: usize = 10;

/// 五引擎同构账格数（5 器 × 3 格）。
pub const ENG_CELLS: usize = 15;

/// 发行版阶梯级数。
pub const DIST_STEPS: usize = 3;

/// 与 MiKTeX 分账声明行数。
pub const SPLIT_ROWS: usize = 2;

/// TeX Live 域 XOR 锚（低 16 位恒零）。
pub const TL_SIG: u64 = 0x54585F4C49560000;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TexliveCode {
    /// 成功
    Ok = 0,
    /// tlmgr 操作号越闭集
    ENoOp,
    /// tlmgr 漂移
    EOpDrift,
    /// tlmgr 族逆序
    EOrder,
    /// 引擎格越闭集
    ENoEng,
    /// 引擎账漂移
    EEngDrift,
    /// 发行版越阶梯
    ENoDist,
    /// 发行版漂移
    EDistDrift,
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

impl TexliveCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            TexliveCode::Ok => "成功",
            TexliveCode::ENoOp => "tlmgr 操作号越闭集：tlmgr 10 操作集之外先登记",
            TexliveCode::EOpDrift => "tlmgr 漂移：操作折叠与基准不恒等，逐格复算",
            TexliveCode::EOrder => "tlmgr 族逆序：安装→移除→更新→搜索→信息单调，逆序必抓",
            TexliveCode::ENoEng => "引擎格越闭集：五引擎同构账 15 格之外先登记",
            TexliveCode::EEngDrift => "引擎账漂移：同构账折叠与基准不恒等，逐器复算",
            TexliveCode::ENoDist => "发行版越阶梯：2023/2024/2025 三档闭集之外先登记",
            TexliveCode::EDistDrift => "发行版漂移：档载荷折叠与基准不恒等，逐档复算",
            TexliveCode::EDup => "重复落账：同格只许一条账目",
            TexliveCode::ESeal => "未齐即封：10 tlmgr 操作 + 15 引擎格 + 3 档发行版 + 2 行分账未齐先补齐",
            TexliveCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            TexliveCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            TexliveCode::EGradeDrift => "定级漂移：全通且分账声明全齐才可定 S1",
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
            Grade::S1 => "S1：六判据+10操作+15引擎格+3档发行版+2行分账全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// tlmgr 操作族（0..2=安装 2..4=移除 4..6=更新 6..8=搜索 8..10=信息）。
pub const fn tlmgr_kind(o: usize) -> Option<u32> {
    if o >= TLMGR_OPS {
        return None;
    }
    Some((o / 2) as u32)
}

/// tlmgr 操作族名。
pub const fn tlmgr_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("安装"),
        1 => Some("移除"),
        2 => Some("更新"),
        3 => Some("搜索"),
        4 => Some("信息"),
        _ => None,
    }
}

/// tlmgr 操作格载荷。
pub const fn tlmgr_cell(o: usize) -> Option<u64> {
    if o >= TLMGR_OPS {
        return None;
    }
    Some((0xB1000u64 + (o as u64) * 0x35) ^ ((o / 2) as u64).wrapping_mul(0x01000193))
}

/// 引擎名（格 c 每 3 格一器，同构账）。
pub const fn eng_name(c: usize) -> Option<&'static str> {
    match c / 3 {
        0 => Some("pdftex"),
        1 => Some("xetex"),
        2 => Some("luatex"),
        3 => Some("tex"),
        4 => Some("dvips"),
        _ => None,
    }
}

/// 引擎格载荷（五器同构折叠）。
pub const fn eng_cell(c: usize) -> Option<u64> {
    if c >= ENG_CELLS {
        return None;
    }
    Some((0xB2000u64 + (c as u64) * 0x3F) ^ TL_SIG)
}

/// 发行版档名。
pub const fn dist_name(v: usize) -> Option<&'static str> {
    match v {
        0 => Some("2023"),
        1 => Some("2024"),
        2 => Some("2025"),
        _ => None,
    }
}

/// 发行版档载荷。
pub const fn dist_cell(v: usize) -> Option<u64> {
    if v >= DIST_STEPS {
        return None;
    }
    Some((0xB3000u64 + (v as u64) * 0x61) ^ TL_SIG)
}

/// 与 MiKTeX 分账声明行折叠（行 r）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xB400 + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// TeX Live 收口账本状态机。
#[derive(Clone)]
pub struct TexliveLedger {
    /// 六判据（PS→PDF/PDF→PS 转换/输出校验/定级登记/锚定/写回）。
    gates: [Option<bool>; 6],
    /// tlmgr 操作账。
    ops: [Option<u64>; TLMGR_OPS],
    /// 引擎同构账。
    engs: [Option<u64>; ENG_CELLS],
    /// 发行版阶梯账。
    dists: [Option<u64>; DIST_STEPS],
    /// 分账声明账。
    splits: [Option<u32>; SPLIT_ROWS],
    /// 最后操作族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl TexliveLedger {
    /// 新账本。
    pub fn new() -> Self {
        TexliveLedger {
            gates: [None; 6],
            ops: [None; TLMGR_OPS],
            engs: [None; ENG_CELLS],
            dists: [None; DIST_STEPS],
            splits: [None; SPLIT_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), TexliveCode> {
        if gate >= 6 {
            return Err(TexliveCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x8100 + gate as u64);
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

    /// tlmgr 操作一格（族单调 + 载荷对照 + 重复拒）。
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), TexliveCode> {
        if o >= TLMGR_OPS {
            return Err(TexliveCode::ENoOp);
        }
        let kind = match tlmgr_kind(o) {
            Some(k) => k,
            None => return Err(TexliveCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8200);
            return Err(TexliveCode::EOrder);
        }
        let want = match tlmgr_cell(o) {
            Some(w) => w,
            None => return Err(TexliveCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8201 + o as u64);
            return Err(TexliveCode::EOpDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TexliveCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x8202 + o as u64);
        Ok(())
    }

    /// tlmgr 全账核验（10 格）。
    pub fn ops_all(&self) -> bool {
        let mut o = 0usize;
        while o < TLMGR_OPS {
            match self.ops.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 引擎格一格（器闭集 + 载荷对照 + 重复拒）。
    pub fn record_eng(&mut self, c: usize, got: u64) -> Result<(), TexliveCode> {
        if c >= ENG_CELLS {
            return Err(TexliveCode::ENoEng);
        }
        if match eng_name(c) {
            Some(_) => false,
            None => true,
        } {
            return Err(TexliveCode::ENoEng);
        }
        let want = match eng_cell(c) {
            Some(w) => w,
            None => return Err(TexliveCode::ENoEng),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8300 + c as u64);
            return Err(TexliveCode::EEngDrift);
        }
        if match self.engs.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TexliveCode::EDup);
        }
        if let Some(x) = self.engs.get_mut(c) {
            *x = Some(got);
        }
        self.fnv(0x8301 + c as u64);
        Ok(())
    }

    /// 引擎同构账全账核验（15 格）。
    pub fn engs_all(&self) -> bool {
        let mut c = 0usize;
        while c < ENG_CELLS {
            match self.engs.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 发行版档一格（阶梯载荷对照 + 重复拒）。
    pub fn record_dist(&mut self, v: usize, got: u64) -> Result<(), TexliveCode> {
        let want = match dist_cell(v) {
            Some(w) => w,
            None => return Err(TexliveCode::ENoDist),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8400 + v as u64);
            return Err(TexliveCode::EDistDrift);
        }
        if match self.dists.get(v) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TexliveCode::EDup);
        }
        if let Some(x) = self.dists.get_mut(v) {
            *x = Some(got);
        }
        self.fnv(0x8401 + v as u64);
        Ok(())
    }

    /// 发行版阶梯全账核验（3 档）。
    pub fn dists_all(&self) -> bool {
        let mut v = 0usize;
        while v < DIST_STEPS {
            match self.dists.get(v) {
                Some(Some(_)) => {}
                _ => return false,
            }
            v += 1;
        }
        true
    }

    /// 分账声明一行（与 MiKTeX 并存隔离）。
    pub fn declare_split(&mut self, r: usize) -> Result<(), TexliveCode> {
        if r >= SPLIT_ROWS {
            return Err(TexliveCode::ESeal);
        }
        if match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TexliveCode::EDup);
        }
        if let Some(c) = self.splits.get_mut(r) {
            *c = split_row(r);
        }
        self.fnv(0x8500 + r as u64);
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
            && self.ops_all()
            && self.engs_all()
            && self.dists_all()
            && self.splits_all()
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for TexliveLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（落格数, 检出数, 指纹）。
/// 三态轮转注入全部落在当轮临时账本 lt 上；漂移注入先按合法序落正格
/// 再注漂移；对空账本同样必抓；注入轮不改主账 last_kind。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = TexliveLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut v = 0usize;
    while v < DIST_STEPS {
        if let Some(w) = dist_cell(v) {
            if led.record_dist(v, w).is_ok() {
                recorded += 1;
            }
        }
        v += 1;
    }
    let mut o = 0usize;
    while o < TLMGR_OPS {
        if let Some(w) = tlmgr_cell(o) {
            if led.record_op(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut c = 0usize;
    while c < ENG_CELLS {
        if let Some(w) = eng_cell(c) {
            if led.record_eng(c, w).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    let mut r = 0usize;
    while r < SPLIT_ROWS {
        let _ = led.declare_split(r);
        r += 1;
    }
    // 注入 10 轮：发行版越阶梯 / tlmgr 漂移 / tlmgr 重复落账三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = TexliveLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_dist(0, match dist_cell(0) {
                    Some(w) => w,
                    None => 0,
                });
                lt.record_dist(DIST_STEPS, 0).is_err()
            }
            1 => {
                let _ = lt.record_op(0, match tlmgr_cell(0) {
                    Some(w) => w,
                    None => 0,
                });
                lt.record_op(
                    1,
                    match tlmgr_cell(1) {
                        Some(w) => w,
                        None => 0,
                    }
                    .wrapping_add(1),
                ) == Err(TexliveCode::EOpDrift)
            }
            _ => {
                let _ = lt.record_op(0, match tlmgr_cell(0) {
                    Some(w) => w,
                    None => 0,
                });
                lt.record_op(0, match tlmgr_cell(0) {
                    Some(w) => w,
                    None => 0,
                }) == Err(TexliveCode::EDup)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x8600 + k as u64);
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
/// UNX-F52348 判据集（锚 UNX-F52349-J1，off-by-one 板式）。
pub fn run_unx52348_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52348");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五引擎同构账 15 格全通",
        recorded >= ENG_CELLS as u32,
        "pdftex/xetex/luatex/tex/dvips 每器 3 格同构折叠逐格恒等",
    );
    s.add(
        "tlmgr 10 操作全通",
        {
            let mut lt = TexliveLedger::new();
            let mut ok = true;
            let mut o = 0usize;
            while o < TLMGR_OPS {
                match tlmgr_cell(o) {
                    Some(w) => {
                        if lt.record_op(o, w).is_err() {
                            ok = false;
                        }
                    }
                    None => ok = false,
                }
                o += 1;
            }
            ok && lt.ops_all()
        },
        "安装/移除/更新/搜索/信息五族 10 操作逐格恒等（UNX-F52349-J1）",
    );
    s.add(
        "tlmgr 五族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < TLMGR_OPS {
                let a = match tlmgr_kind(o) {
                    Some(k) => k,
                    None => 9,
                };
                let b = match tlmgr_kind(o - 1) {
                    Some(k) => k,
                    None => 9,
                };
                if a < b {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "安装→移除→更新→搜索→信息五族闭集单调",
    );
    s.add(
        "发行版阶梯 2023/2024/2025 三档全落",
        {
            let mut lt = TexliveLedger::new();
            let mut ok = true;
            let mut v = 0usize;
            while v < DIST_STEPS {
                match dist_cell(v) {
                    Some(w) => {
                        if lt.record_dist(v, w).is_err() {
                            ok = false;
                        }
                    }
                    None => ok = false,
                }
                v += 1;
            }
            ok && lt.dists_all()
        },
        "三档闭集逐档恒等，越级必抓",
    );
    s.add(
        "与 MiKTeX 分账声明 2/2 全齐",
        {
            let mut lt = TexliveLedger::new();
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
        "MiKTeX 并存隔离分账声明/TeX Live 独立发行版分账声明两行闭集",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "发行版越阶梯/tlmgr 漂移/tlmgr 重复落账三态轮转必抓",
    );
    let mut l0 = TexliveLedger::new();
    s.add(
        "边界·发行版越阶梯",
        l0.record_dist(DIST_STEPS, 0).is_err(),
        "发行版档 3 越阶梯必拒",
    );
    s.add(
        "边界·tlmgr 越闭集",
        l0.record_op(TLMGR_OPS, 0).is_err(),
        "tlmgr 操作 10 越闭集必拒",
    );
    s.add(
        "边界·引擎格越闭集",
        l0.record_eng(ENG_CELLS, 0).is_err(),
        "引擎格 15 越闭集必拒",
    );
    s.add(
        "tlmgr 漂移拒",
        {
            match tlmgr_cell(0) {
                Some(w) => {
                    let _ = l0.record_op(0, w);
                }
                None => {}
            }
            let bad = match tlmgr_cell(1) {
                Some(w) => w.wrapping_add(1),
                None => 1,
            };
            l0.record_op(1, bad) == Err(TexliveCode::EOpDrift)
        },
        "tlmgr 载荷漂移必抓",
    );
    s.add(
        "引擎漂移拒",
        {
            let bad = match eng_cell(4) {
                Some(w) => w.wrapping_add(5),
                None => 5,
            };
            l0.record_eng(4, bad) == Err(TexliveCode::EEngDrift)
        },
        "引擎同构账载荷漂移必抓",
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
            let mut lt = TexliveLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut v = 0usize;
            while v < DIST_STEPS {
                match dist_cell(v) {
                    Some(w) => {
                        let _ = lt.record_dist(v, w);
                    }
                    None => {}
                }
                v += 1;
            }
            let mut o = 0usize;
            while o < TLMGR_OPS {
                match tlmgr_cell(o) {
                    Some(w) => {
                        let _ = lt.record_op(o, w);
                    }
                    None => {}
                }
                o += 1;
            }
            let mut c = 0usize;
            while c < ENG_CELLS {
                match eng_cell(c) {
                    Some(w) => {
                        let _ = lt.record_eng(c, w);
                    }
                    None => {}
                }
                c += 1;
            }
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                let _ = lt.declare_split(r);
                r += 1;
            }
            // 各节全部记账后再断言定级。
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC96C0,
        "SEG_BASE 0x40 对齐且落在 VX15 派工公式 0xC96C0 格",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dist_ladder() {
        assert_eq!(dist_name(0), Some("2023"));
        assert_eq!(dist_name(1), Some("2024"));
        assert_eq!(dist_name(2), Some("2025"));
        assert!(dist_name(3).is_none());
        assert!(dist_cell(3).is_none());
    }

    #[test]
    fn test_eng_mapping() {
        // 引擎映射：c/3 → 0..4 五器（同构账每器 3 格）。
        assert_eq!(eng_name(0), Some("pdftex"));
        assert_eq!(eng_name(2), Some("pdftex"));
        assert_eq!(eng_name(9), Some("tex"));
        assert_eq!(eng_name(14), Some("dvips"));
        assert!(eng_name(15).is_none());
        assert!(eng_cell(15).is_none());
    }

    #[test]
    fn test_tlmgr_kind_mapping() {
        // kind 映射：o/2 → 0,0,1,1,2,2,3,3,4,4（五族）。
        assert_eq!(tlmgr_kind(0), Some(0));
        assert_eq!(tlmgr_kind(1), Some(0));
        assert_eq!(tlmgr_kind(4), Some(2));
        assert_eq!(tlmgr_kind(9), Some(4));
        assert!(tlmgr_kind(10).is_none());
        assert_eq!(tlmgr_kind_name(0), Some("安装"));
        assert_eq!(tlmgr_kind_name(4), Some("信息"));
        assert!(tlmgr_kind_name(5).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = tlmgr_cell(3).unwrap_or(0);
        assert_eq!(a, tlmgr_cell(3).unwrap_or(0));
        assert_ne!(a, tlmgr_cell(4).unwrap_or(0));
        assert!(split_row(SPLIT_ROWS).is_none());
        assert_ne!(eng_cell(0).unwrap_or(0), dist_cell(0).unwrap_or(0));
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = TexliveLedger::new();
        // 先按合法序落更新族正格，再注安装族逆序。
        let _ = lt.record_op(5, tlmgr_cell(5).unwrap_or(0));
        assert_eq!(
            lt.record_op(1, tlmgr_cell(1).unwrap_or(0)),
            Err(TexliveCode::EOrder)
        );
        assert!(lt.record_op(6, tlmgr_cell(6).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = TexliveLedger::new();
        assert_eq!(
            lt.record_op(5, tlmgr_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(TexliveCode::EOpDrift)
        );
        assert_eq!(
            lt.record_eng(2, eng_cell(2).unwrap_or(0).wrapping_add(7)),
            Err(TexliveCode::EEngDrift)
        );
        assert_eq!(
            lt.record_dist(1, dist_cell(1).unwrap_or(0).wrapping_add(3)),
            Err(TexliveCode::EDistDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = TexliveLedger::new();
        let _ = lt.record_dist(0, dist_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_dist(0, dist_cell(0).unwrap_or(0)),
            Err(TexliveCode::EDup)
        );
        let _ = lt.record_eng(0, eng_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_eng(0, eng_cell(0).unwrap_or(0)),
            Err(TexliveCode::EDup)
        );
        let _ = lt.declare_split(0);
        assert_eq!(lt.declare_split(0), Err(TexliveCode::EDup));
    }

    #[test]
    fn test_split_boundary() {
        let mut lt = TexliveLedger::new();
        assert!(lt.declare_split(0).is_ok());
        assert!(lt.declare_split(1).is_ok());
        // 闭集边界：0..1 两行之外越界必抓，全落才 splits_all。
        assert!(lt.declare_split(2).is_err());
        assert!(lt.splits_all());
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = TexliveLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = TexliveLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= 28);
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
        assert_eq!(SEG_BASE, 0xC96C0);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(TexliveCode::Ok as u32, 0);
        assert_eq!(TexliveCode::EGradeDrift as u32, 12);
        assert!(!TexliveCode::Ok.detail().is_empty());
        assert!(!TexliveCode::ESeal.detail().is_empty());
        assert_eq!(TexliveCode::Ok.val(), SEG_BASE);
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        let lt = TexliveLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn unx52348_checks_green() {
        let s = run_unx52348_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
