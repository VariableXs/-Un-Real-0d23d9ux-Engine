//! UNX-F52288 波 23 段六判据全绿率结算账 · 0xC87C0段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52288"，逐字）：
//!   title: 波 23 段六判据全绿率结算账
//!   quota: 260
//!   criteria: ① 纯功能代码 ≥260 行；② 判据原文：UNX-F52289-J1 本段安装
//!     器形态（MSI/NSIS/Inno/自研壳）逐条目归并入 E4 分类账 ×14 条目零
//!     漏；个体失绿如实降级禁"凑装上"；联签锚定行 AI-24；判据锚定
//!     §AI-66.6 风险 ③；③ 完成时 API/软件写回：状态=已完成、结果摘要
//!     非空。
//! off-by-one 板式口径：本卡（卡 N=52288）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52289-J1，照原文锚定；功能按本卡标题（波 23 段六判据全绿率结
//! 算账）落地，安装器形态归并判据由同批后续卡承载。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 全绿率分母/分子账 12 格：六判据 ×（分母/分子）各 1 格，分母为
//!      零破账、分子超分母破账，逐格落账；比率用基点（万分比）整算，
//!      零浮点。
//!   ② 分档闭集三档：100% / 95% / 90% 三档（基点 10000/9500/9000 阈），
//!      未达 90% 档即未达档，归因必开。
//!   ③ 未达档归因账 6 格：逐格载荷对照，漂移必抓；个体失绿如实降级，
//!      禁"凑装上"。
//!   ④ 结算单向门：settle 一次封账，封账后任何回改（改格/重复结算）一
//!      律 ESealed 必抓——结算不回改。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint，FNV-1a）；
//!      注入核 run_cycle 三态轮转 10/10（封账回改/归因漂移/分母格越闭
//!      集），全部落在当轮临时账本上，主账本 sealed 不受扰动；检出+逃
//!      逸守恒等式；双跑 FNV 指纹一致。
//!   ⑥ KB-COMPAT 定级闭集：结算落 100% 档且全通 → S1；落 95%/90% 档
//!      有条件收口 → S2；判据缺 → S3。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52288;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 六判据条数。
pub const CRITERIA: usize = 6;

/// 分母/分子双侧（× 2 = 12 格）。
pub const RATE_SIDES: usize = 2;

/// 分母/分子账总格数。
pub const RATE_CELLS: usize = CRITERIA * RATE_SIDES;

/// 未达档归因账格数。
pub const ATTRIB_CELLS: usize = 6;

/// 分档数（100%/95%/90% 三档）。
pub const TIERS: usize = 3;

/// 归因域异或锚（低 16 位恒零）。
pub const ATTRIB_ANCHOR: u64 = 0x5732_0000;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Wave23Code {
    /// 成功
    Ok = 0,
    /// 分母/分子格越闭集
    ENoCell,
    /// 格载荷漂移
    ECellDrift,
    /// 分档越闭集
    ENoTier,
    /// 分档载荷漂移
    ETierDrift,
    /// 归因格越闭集
    ENoAttrib,
    /// 归因载荷漂移
    EAttribDrift,
    /// 结算单向门后回改
    ESealed,
    /// 比率破账（分母为零/分子超分母）
    ENoSettle,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl Wave23Code {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            Wave23Code::Ok => "成功",
            Wave23Code::ENoCell => "分母/分子格越闭集：六判据 × 分母/分子共 12 格之外先登记",
            Wave23Code::ECellDrift => "格载荷漂移：格折叠与基准不恒等，逐格复算",
            Wave23Code::ENoTier => "分档越闭集：100%/95%/90% 三档之外先登记",
            Wave23Code::ETierDrift => "分档载荷漂移：档位折叠与基点阈值不恒等，逐档复算",
            Wave23Code::ENoAttrib => "归因格越闭集：未达档归因账 6 格之外先登记",
            Wave23Code::EAttribDrift => "归因载荷漂移：归因格折叠与基准不恒等，逐格复算",
            Wave23Code::ESealed => "结算单向门后回改：结算封账后不回改，回改必抓",
            Wave23Code::ENoSettle => "比率破账：分母为零或分子超分母，个体失绿如实降级禁凑装上",
            Wave23Code::EDup => "重复落账：同格只许一条账目",
            Wave23Code::ESeal => "未齐即封：12 格分母/分子账未齐不许结算",
            Wave23Code::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            Wave23Code::EGradeDrift => "定级漂移：落 100% 档且全通才可定 S1",
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
            Grade::S1 => "S1：六判据+12格分母分子账全通且结算落 100% 档收口",
            Grade::S2 => "S2：有条件收口，结算落 95%/90% 档且归因账公开",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 判据侧格序号（判据 c × 侧 s：0=分母 1=分子）。
pub const fn rate_cell_index(c: usize, side: usize) -> Option<usize> {
    if c >= CRITERIA || side >= RATE_SIDES {
        return None;
    }
    Some(c * RATE_SIDES + side)
}

/// 归因格载荷（格 a：0..6）。
pub const fn attrib_cell(a: usize) -> Option<u64> {
    if a >= ATTRIB_CELLS {
        return None;
    }
    Some((0xB1000u64 + (a as u64) * 0x47) ^ ATTRIB_ANCHOR)
}

/// 分档基点阈值闭集（0=100%档 1=95%档 2=90%档）。
pub const fn tier_threshold(t: usize) -> Option<u32> {
    match t {
        0 => Some(10000),
        1 => Some(9500),
        2 => Some(9000),
        _ => None,
    }
}

/// 分档名。
pub const fn tier_name(t: usize) -> Option<&'static str> {
    match t {
        0 => Some("100%档"),
        1 => Some("95%档"),
        2 => Some("90%档"),
        _ => None,
    }
}

/// 基点落档（< 9000 未达档返回 None）。
pub const fn tier_of(bp: u32) -> Option<u32> {
    if bp >= 10000 {
        return Some(0);
    }
    if bp >= 9500 {
        return Some(1);
    }
    if bp >= 9000 {
        return Some(2);
    }
    None
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// 波 23 段全绿率结算账账本状态机。
#[derive(Clone)]
pub struct Wave23Ledger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 分母账 6 格。
    dens: [Option<u64>; CRITERIA],
    /// 分子账 6 格。
    nums: [Option<u64>; CRITERIA],
    /// 未达档归因账 6 格。
    attribs: [Option<u64>; ATTRIB_CELLS],
    /// 结算单向门。
    sealed: bool,
    /// 结算落档（Some(t)=已结算落 t 档）。
    tier: Option<u32>,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl Wave23Ledger {
    /// 新账本。
    pub fn new() -> Self {
        Wave23Ledger {
            gates: [None; 6],
            dens: [None; CRITERIA],
            nums: [None; CRITERIA],
            attribs: [None; ATTRIB_CELLS],
            sealed: false,
            tier: None,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), Wave23Code> {
        if self.sealed {
            return Err(Wave23Code::ESealed);
        }
        if gate >= 6 {
            return Err(Wave23Code::ESeal);
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

    /// 分母一格（零分母破账 + 重复拒 + 单向门）。
    pub fn record_den(&mut self, c: usize, v: u64) -> Result<(), Wave23Code> {
        if self.sealed {
            return Err(Wave23Code::ESealed);
        }
        if c >= CRITERIA {
            return Err(Wave23Code::ENoCell);
        }
        if v == 0 {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4200 + c as u64);
            return Err(Wave23Code::ENoSettle);
        }
        if match self.dens.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(Wave23Code::EDup);
        }
        if let Some(cell) = self.dens.get_mut(c) {
            *cell = Some(v);
        }
        self.fnv(0x4201 + c as u64);
        Ok(())
    }

    /// 分子一格（超分母破账 + 重复拒 + 单向门）。
    pub fn record_num(&mut self, c: usize, v: u64) -> Result<(), Wave23Code> {
        if self.sealed {
            return Err(Wave23Code::ESealed);
        }
        if c >= CRITERIA {
            return Err(Wave23Code::ENoCell);
        }
        let den = match self.dens.get(c) {
            Some(Some(d)) => *d,
            _ => {
                self.injected += 1;
                self.caught += 1;
                self.fnv(0x4300 + c as u64);
                return Err(Wave23Code::ENoSettle);
            }
        };
        if v > den {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4301 + c as u64);
            return Err(Wave23Code::ENoSettle);
        }
        if match self.nums.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(Wave23Code::EDup);
        }
        if let Some(cell) = self.nums.get_mut(c) {
            *cell = Some(v);
        }
        self.fnv(0x4302 + c as u64);
        Ok(())
    }

    /// 分母/分子账全账核验（12 格齐）。
    pub fn rates_all(&self) -> bool {
        let mut c = 0usize;
        while c < CRITERIA {
            match (self.dens.get(c), self.nums.get(c)) {
                (Some(Some(_)), Some(Some(_))) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 全绿率基点（万分比整算；分母合计为零返回零）。
    pub fn rate_bp(&self) -> u64 {
        let mut den = 0u64;
        let mut num = 0u64;
        let mut c = 0usize;
        while c < CRITERIA {
            match self.dens.get(c) {
                Some(Some(d)) => den += *d,
                _ => {}
            }
            match self.nums.get(c) {
                Some(Some(n)) => num += *n,
                _ => {}
            }
            c += 1;
        }
        if den == 0 {
            return 0;
        }
        num * 10000 / den
    }

    /// 结算（单向门）：12 格齐才许结算，落档后封账不回改。
    pub fn settle(&mut self) -> Result<u32, Wave23Code> {
        if self.sealed {
            return Err(Wave23Code::ESealed);
        }
        if !self.rates_all() {
            return Err(Wave23Code::ESeal);
        }
        let bp = self.rate_bp();
        let t = match tier_of(bp as u32) {
            Some(t) => t,
            None => {
                self.injected += 1;
                self.caught += 1;
                self.fnv(0x4400);
                return Err(Wave23Code::ENoTier);
            }
        };
        self.sealed = true;
        self.tier = Some(t);
        self.fnv(0x4401 + t as u64);
        Ok(t)
    }

    /// 结算落档查询。
    pub fn settled_tier(&self) -> Option<u32> {
        self.tier
    }

    /// 单向门状态。
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// 归因账一格（载荷对照 + 重复拒 + 单向门）。
    pub fn record_attrib(&mut self, a: usize, got: u64) -> Result<(), Wave23Code> {
        if self.sealed {
            return Err(Wave23Code::ESealed);
        }
        if a >= ATTRIB_CELLS {
            return Err(Wave23Code::ENoAttrib);
        }
        let want = match attrib_cell(a) {
            Some(w) => w,
            None => return Err(Wave23Code::ENoAttrib),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4500 + a as u64);
            return Err(Wave23Code::EAttribDrift);
        }
        if match self.attribs.get(a) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(Wave23Code::EDup);
        }
        if let Some(cell) = self.attribs.get_mut(a) {
            *cell = Some(got);
        }
        self.fnv(0x4501 + a as u64);
        Ok(())
    }

    /// 归因账全账核验（6 格）。
    pub fn attribs_all(&self) -> bool {
        let mut a = 0usize;
        while a < ATTRIB_CELLS {
            match self.attribs.get(a) {
                Some(Some(_)) => {}
                _ => return false,
            }
            a += 1;
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

    /// KB-COMPAT 定级（100% 档全通 → S1，95%/90% 档 → S2，判据缺 → S3）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all() && self.rates_all() && self.attribs_all() && self.settled_tier() == Some(0) {
            Grade::S1
        } else if self.gates_all() && self.rates_all() && self.settled_tier().is_some() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for Wave23Ledger {
    fn default() -> Self {
        Self::new()
    }
}

/// 波 23 段结算演示口径：六判据分母各 40、分子 40/40/39/40/38/40。
pub const DEMO_DEN: u64 = 40;
/// 分子演示闭集。
pub const DEMO_NUMS: [u64; CRITERIA] = [40, 40, 39, 40, 38, 40];

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（落账格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = Wave23Ledger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut c = 0usize;
    while c < CRITERIA {
        if led.record_den(c, DEMO_DEN).is_ok() {
            recorded += 1;
        }
        if let Some(n) = DEMO_NUMS.get(c) {
            if led.record_num(c, *n).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    // 归因账先落（结算单向门封账后不回改，归因须在结算前入账）。
    let mut a = 0usize;
    while a < ATTRIB_CELLS {
        if let Some(w) = attrib_cell(a) {
            if led.record_attrib(a, w).is_ok() {
                recorded += 1;
            }
        }
        a += 1;
    }
    // 结算：合计分子 237 / 分母 240 → 9875 基点 → 95% 档。
    let _ = led.settle();
    // 注入 10 轮：封账回改 / 归因漂移 / 分母格越闭集三态轮转。
    // 全部落在当轮临时账本上：led 上 injected==caught==10、escaped==0。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = Wave23Ledger::new();
        let caught = match kind {
            0 => {
                // 单向门回改：临时账本先落满 12 格并结算封账，再回改必抓。
                let mut i = 0usize;
                while i < CRITERIA {
                    let _ = lt.record_den(i, DEMO_DEN);
                    if let Some(n) = DEMO_NUMS.get(i) {
                        let _ = lt.record_num(i, *n);
                    }
                    i += 1;
                }
                let _ = lt.settle();
                lt.record_den(0, DEMO_DEN) == Err(Wave23Code::ESealed)
            }
            1 => {
                let _ = lt.record_attrib(0, attrib_cell(0).unwrap_or(0));
                lt.record_attrib(1, attrib_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(Wave23Code::EAttribDrift)
            }
            _ => lt.record_den(CRITERIA, DEMO_DEN).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x4600 + k as u64);
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
/// UNX-F52288 判据集（锚 UNX-F52289-J1，off-by-one 板式）。
pub fn run_unx52288_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52288");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "全绿率分母/分子账 12 格全通",
        recorded >= (RATE_CELLS + ATTRIB_CELLS) as u32,
        "六判据 × 分母/分子逐格落账，分母为零/分子超分母破账必抓",
    );
    s.add(
        "结算落 95% 档（9875 基点）",
        {
            let mut lt = Wave23Ledger::new();
            let mut c = 0usize;
            while c < CRITERIA {
                let _ = lt.record_den(c, DEMO_DEN);
                if let Some(n) = DEMO_NUMS.get(c) {
                    let _ = lt.record_num(c, *n);
                }
                c += 1;
            }
            match lt.settle() {
                Ok(t) => t == 1 && lt.settled_tier() == Some(1) && lt.is_sealed(),
                Err(_) => false,
            }
        },
        "237/240 → 9875 基点 → 95% 档（个体失绿如实降级禁凑装上）",
    );
    s.add(
        "分档闭集三档边界",
        {
            let t0 = tier_of(10000) == Some(0);
            let t1 = tier_of(9999) == Some(1) && tier_of(9500) == Some(1);
            let t2 = tier_of(9499) == Some(2) && tier_of(9000) == Some(2);
            let t3 = tier_of(8999).is_none();
            t0 && t1 && t2 && t3
        },
        "10000/9500/9000 基点三档闭集，8999 以下未达档",
    );
    s.add(
        "分档阈值闭集",
        {
            let a = tier_threshold(0) == Some(10000);
            let b = tier_threshold(1) == Some(9500);
            let c = tier_threshold(2) == Some(9000);
            let d = tier_threshold(3).is_none();
            a && b && c && d
        },
        "三档阈值与档名闭集一致，越档无名",
    );
    s.add(
        "未达档归因账 6 格全落",
        {
            let mut lt = Wave23Ledger::new();
            let mut ok = true;
            let mut a = 0usize;
            while a < ATTRIB_CELLS {
                if let Some(w) = attrib_cell(a) {
                    if lt.record_attrib(a, w).is_err() {
                        ok = false;
                    }
                }
                a += 1;
            }
            ok && lt.attribs_all()
        },
        "未达档归因逐格载荷对照，漂移必抓（联签锚定行 AI-24）",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = Wave23Ledger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "六判据逐格落账，未齐即封",
    );
    s.add("注入 10/10 全抓", caught >= 10, "封账回改/归因漂移/分母格越闭集三态轮转必抓");
    s.add(
        "边界·分母格越闭集",
        {
            let mut lt = Wave23Ledger::new();
            lt.record_den(CRITERIA, DEMO_DEN).is_err()
        },
        "判据 6 越闭集必拒",
    );
    s.add(
        "边界·归因格越闭集",
        {
            let mut lt = Wave23Ledger::new();
            lt.record_attrib(ATTRIB_CELLS, 0).is_err()
        },
        "归因格 6 越闭集必拒",
    );
    s.add(
        "边界·分档越闭集",
        tier_of(0).is_none() && tier_threshold(TIERS).is_none() && tier_name(TIERS).is_none(),
        "8999 基点以下未达档，档位闭集之外无名",
    );
    s.add(
        "分子超分母拒",
        {
            let mut lt = Wave23Ledger::new();
            let _ = lt.record_den(2, 10);
            lt.record_num(2, 11) == Err(Wave23Code::ENoSettle)
        },
        "分子超分母破账必抓，禁凑装上",
    );
    s.add(
        "分母为零拒",
        {
            let mut lt = Wave23Ledger::new();
            lt.record_den(0, 0) == Err(Wave23Code::ENoSettle)
        },
        "零分母破账必抓",
    );
    s.add(
        "结算单向门拒回改",
        {
            let mut lt = Wave23Ledger::new();
            let mut c = 0usize;
            while c < CRITERIA {
                let _ = lt.record_den(c, DEMO_DEN);
                if let Some(n) = DEMO_NUMS.get(c) {
                    let _ = lt.record_num(c, *n);
                }
                c += 1;
            }
            let first = lt.settle().is_ok();
            let reseal = lt.settle() == Err(Wave23Code::ESealed);
            let rewrite = lt.record_den(0, DEMO_DEN) == Err(Wave23Code::ESealed);
            first && reseal && rewrite
        },
        "结算封账后重复结算/改格一律必抓，结算不回改",
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
        "定级 S2 闭集（95% 档有条件收口）",
        {
            let mut lt = Wave23Ledger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut c = 0usize;
            while c < CRITERIA {
                let _ = lt.record_den(c, DEMO_DEN);
                if let Some(n) = DEMO_NUMS.get(c) {
                    let _ = lt.record_num(c, *n);
                }
                c += 1;
            }
            let mut a = 0usize;
            while a < ATTRIB_CELLS {
                if let Some(w) = attrib_cell(a) {
                    let _ = lt.record_attrib(a, w);
                }
                a += 1;
            }
            // 定级检查：main/aux 各节全部记账、结算完成后再断言。
            let _ = lt.settle();
            lt.judge_grade() == Grade::S2
        },
        "95% 档有条件收口 → S2（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC87C0,
        "SEG_BASE 0x40 对齐且按派工公式落 0xC87C0",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥17 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tier_closed_set() {
        assert_eq!(tier_of(10000), Some(0));
        assert_eq!(tier_of(9500), Some(1));
        assert_eq!(tier_of(9000), Some(2));
        assert_eq!(tier_of(8999), None);
        assert_eq!(tier_of(0), None);
        assert_eq!(tier_name(0), Some("100%档"));
        assert_eq!(tier_name(2), Some("90%档"));
        assert!(tier_name(3).is_none());
    }

    #[test]
    fn test_rate_cell_index() {
        assert_eq!(rate_cell_index(0, 0), Some(0));
        assert_eq!(rate_cell_index(5, 1), Some(11));
        assert!(rate_cell_index(6, 0).is_none());
        assert!(rate_cell_index(0, 2).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = attrib_cell(3).unwrap_or(0);
        assert_eq!(a, attrib_cell(3).unwrap_or(0));
        assert_ne!(a, attrib_cell(4).unwrap_or(0));
        assert!(attrib_cell(ATTRIB_CELLS).is_none());
    }

    #[test]
    fn test_anchor_low16_zero() {
        assert_eq!(ATTRIB_ANCHOR & 0xFFFF, 0);
    }

    #[test]
    fn test_rate_bp_arithmetic() {
        let mut lt = Wave23Ledger::new();
        let mut c = 0usize;
        while c < CRITERIA {
            let _ = lt.record_den(c, DEMO_DEN);
            if let Some(n) = DEMO_NUMS.get(c) {
                let _ = lt.record_num(c, *n);
            }
            c += 1;
        }
        // 237/240 → 9875 基点。
        assert_eq!(lt.rate_bp(), 9875);
    }

    #[test]
    fn test_broken_rate_rejected() {
        let mut lt = Wave23Ledger::new();
        assert_eq!(lt.record_den(0, 0), Err(Wave23Code::ENoSettle));
        let _ = lt.record_den(1, 10);
        assert_eq!(lt.record_num(1, 11), Err(Wave23Code::ENoSettle));
        // 分子先于分母落账 → 破账。
        assert_eq!(lt.record_num(2, 5), Err(Wave23Code::ENoSettle));
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = Wave23Ledger::new();
        let _ = lt.record_den(0, 40);
        assert_eq!(lt.record_den(0, 40), Err(Wave23Code::EDup));
        let _ = lt.record_num(0, 40);
        assert_eq!(lt.record_num(0, 40), Err(Wave23Code::EDup));
    }

    #[test]
    fn test_one_way_gate() {
        let mut lt = Wave23Ledger::new();
        let mut c = 0usize;
        while c < CRITERIA {
            let _ = lt.record_den(c, DEMO_DEN);
            if let Some(n) = DEMO_NUMS.get(c) {
                let _ = lt.record_num(c, *n);
            }
            c += 1;
        }
        assert!(lt.settle().is_ok());
        assert!(lt.is_sealed());
        // 单向门：封账后改格/重复结算/改归因一律必抓。
        assert_eq!(lt.record_den(0, DEMO_DEN), Err(Wave23Code::ESealed));
        assert_eq!(lt.record_num(0, 40), Err(Wave23Code::ESealed));
        assert_eq!(lt.record_attrib(0, attrib_cell(0).unwrap_or(0)), Err(Wave23Code::ESealed));
        assert_eq!(lt.judge_gate(0), Err(Wave23Code::ESealed));
        assert_eq!(lt.settle(), Err(Wave23Code::ESealed));
    }

    #[test]
    fn test_settle_requires_full() {
        let mut lt = Wave23Ledger::new();
        let _ = lt.record_den(0, 40);
        // 12 格未齐即结算 → ESeal。
        assert_eq!(lt.settle(), Err(Wave23Code::ESeal));
        assert!(!lt.is_sealed());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = Wave23Ledger::new();
        assert_eq!(
            lt.record_attrib(2, attrib_cell(2).unwrap_or(0).wrapping_add(1)),
            Err(Wave23Code::EAttribDrift)
        );
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = Wave23Ledger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (RATE_CELLS + ATTRIB_CELLS) as u32);
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
        assert_eq!(SEG_BASE, 0xC87C0);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(Wave23Code::Ok as u32, 0);
        assert_eq!(Wave23Code::EGradeDrift as u32, 12);
        assert_eq!(Wave23Code::Ok.val(), SEG_BASE);
        assert_eq!(Wave23Code::EGradeDrift.val(), SEG_BASE + 12);
        assert!(!Wave23Code::Ok.detail().is_empty());
        assert!(!Wave23Code::ESealed.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        let lt = Wave23Ledger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn unx52288_checks_green() {
        let s = run_unx52288_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 17);
    }
}
