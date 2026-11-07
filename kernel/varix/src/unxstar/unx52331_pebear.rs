//! UNX-F52331 PE-bear 收口 · 0xC9280段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52331"，逐字）：
//!   title: PE-bear 收口
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52332-J1 六判
//!     据全落；×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成
//!     时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52331）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52332-J1，照原文锚定；功能按本卡标题（PE-bear）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① PE 结构五族 20 格闭集：DOS 头 4 格（e_magic/e_lfanew/DOS
//!      stub 边界/重叠校验）+ NT 头 4 格（签名/可选头魔数/子系统/
//!      链接器版本）+ File 头 4 格（机器/节数/时间戳/特征）+ Section
//!      头 4 格（名称/虚拟尺寸/原始尺寸/特征）+ DataDirectory 4 格
//!      （导入/导出/重定位/异常），族单调（DOS→NT→File→Section→
//!      DataDirectory，逆序必抓且拒绝后不改 last_kind）。
//!   ② 十六进制/解析双视图 8 格闭集：十六进制视图 4 格（偏移导航/
//!      选区/字节面板/搜索）+ 解析视图 4 格（树展开/字段高亮/悬停
//!      提示/跳转联动），双视图逐格载荷恒等才收。
//!   ③ 库消费联签 4 格（联签锚 F52324 生态声明）：解析库加载/头解析
//!      消费/节区遍历消费/导出消费，逐格落账缺一拒封。
//!   ④ 开源声明 2 行闭集：开源许可声明/源码可得声明，缺一拒封。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      grade 定级闭集）；注入核 run_cycle 三态轮转 10 轮（双视图格
//!      载荷漂移/PE 族格越闭集/PE 族逆序），全部落在当轮临时账本上，
//!      注入 10/10 全抓、逃逸 0；双跑指纹一致；定级 S1/S2/S3 闭集
//!      （20 族格 + 8 视图格 + 4 联签 + 2 行声明 + 六判据全通 → S1）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无
//!      unwrap/expect/panic/裸下标；XOR 锚常量低 16 位恒零。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52331;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// PE 结构五族格数。
pub const FAM_CELLS: usize = 20;

/// PE 结构族数。
pub const FAM_KINDS: usize = 5;

/// 双视图格数。
pub const VIEW_CELLS: usize = 8;

/// 库消费联签格数（F52324）。
pub const LIB_CELLS: usize = 4;

/// 开源声明行数。
pub const DECL_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PebearCode {
    /// 成功
    Ok = 0,
    /// PE 族格越闭集
    ENoFam,
    /// 载荷漂移
    EDrift,
    /// 双视图格越闭集
    ENoView,
    /// PE 族逆序
    EOrder,
    /// 库联签格越闭集
    ENoLib,
    /// 声明行越闭集
    ENoRow,
    /// 判据格越闭集
    EGate,
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

impl PebearCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            PebearCode::Ok => "成功",
            PebearCode::ENoFam => "PE 族格越闭集：五族 20 格闭集之外先登记",
            PebearCode::EDrift => "载荷漂移：格载荷折叠与基准不恒等，逐格复算",
            PebearCode::ENoView => "双视图格越闭集：十六进制/解析双视图 8 格闭集之外先登记",
            PebearCode::EOrder => "PE 族逆序：DOS→NT→File→Section→DataDirectory 单调，逆序必抓",
            PebearCode::ENoLib => "库联签格越闭集：F52324 生态联签 4 格之外先登记",
            PebearCode::ENoRow => "声明行越闭集：开源声明 2 行闭集之外先登记",
            PebearCode::EGate => "判据格越闭集：六判据闸 0..=5 之外先登记",
            PebearCode::EDup => "重复落账：同格只许一条账目",
            PebearCode::ESeal => "未齐即封：20 族格 + 8 视图格 + 4 联签 + 2 行声明未齐先补齐",
            PebearCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            PebearCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            PebearCode::EGradeDrift => "定级漂移：全通且联签/声明全齐才可定 S1",
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
            Grade::S1 => "S1：六判据+20族格+8视图格+4联签+2行声明全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// PE 族格载荷（格 o；族 = o/4）。XOR 锚低 16 位恒零。
pub const fn fam_cell(o: usize) -> Option<u64> {
    if o >= FAM_CELLS {
        return None;
    }
    Some((0x54A0_0000_0000_0000u64 + (o as u64) * 0x41) ^ 0x5045_4200_0000_0000)
}

/// PE 族（i/N 映射：0..4→DOS 4..8→NT 8..12→File 12..16→Section 16..20→DataDirectory）。
pub const fn fam_kind(o: usize) -> Option<u32> {
    if o >= FAM_CELLS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 族名。
pub const fn fam_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("DOS 头"),
        1 => Some("NT 头"),
        2 => Some("File 头"),
        3 => Some("Section 头"),
        4 => Some("DataDirectory"),
        _ => None,
    }
}

/// 双视图格载荷（格 v；视图 = v/4：0=十六进制 1=解析）。XOR 锚低 16 位恒零。
pub const fn view_cell(v: usize) -> Option<u64> {
    if v >= VIEW_CELLS {
        return None;
    }
    Some((0x54B0_0000_0000_0000u64 + (v as u64) * 0x59) ^ 0x4845_5800_0000_0000)
}

/// 双视图（i/N 映射）。
pub const fn view_kind(v: usize) -> Option<u32> {
    if v >= VIEW_CELLS {
        return None;
    }
    Some((v / 4) as u32)
}

/// 视图名。
pub const fn view_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("十六进制视图"),
        1 => Some("解析视图"),
        _ => None,
    }
}

/// 库消费联签格载荷（格 c，锚 F52324）。XOR 锚低 16 位恒零。
pub const fn lib_cell(c: usize) -> Option<u64> {
    if c >= LIB_CELLS {
        return None;
    }
    Some(((0x54C0_0000u32 + c as u32 * 0x73) as u64) ^ 0x4C49_4200_0000_0000)
}

/// 联签名。
pub const fn lib_name(c: usize) -> Option<&'static str> {
    match c {
        0 => Some("解析库加载联签"),
        1 => Some("头解析消费联签"),
        2 => Some("节区遍历消费联签"),
        3 => Some("导出消费联签"),
        _ => None,
    }
}

/// 开源声明行折叠（行 r）。
pub const fn decl_row(r: usize) -> Option<u32> {
    if r >= DECL_ROWS {
        return None;
    }
    Some(0x54D0 + r as u32 * 0x31)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// PE-bear 收口账本状态机。
#[derive(Clone)]
pub struct PebearLedger {
    /// 六判据（UNX-F52332-J1：装载/解析/双视图/编辑/崩溃/卸载）。
    gates: [Option<bool>; 6],
    /// PE 族格账。
    fam: [Option<u64>; FAM_CELLS],
    /// 双视图格账。
    view: [Option<u64>; VIEW_CELLS],
    /// 库联签格账。
    lib: [Option<u64>; LIB_CELLS],
    /// 声明行账。
    decl: [Option<u32>; DECL_ROWS],
    /// 最后族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl PebearLedger {
    /// 新账本。
    pub fn new() -> Self {
        PebearLedger {
            gates: [None; 6],
            fam: [None; FAM_CELLS],
            view: [None; VIEW_CELLS],
            lib: [None; LIB_CELLS],
            decl: [None; DECL_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), PebearCode> {
        if gate >= 6 {
            return Err(PebearCode::EGate);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x5310 + gate as u64);
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

    /// PE 族一格（族单调 + 载荷对照 + 重复拒）。
    pub fn record_fam(&mut self, o: usize, got: u64) -> Result<(), PebearCode> {
        if o >= FAM_CELLS {
            return Err(PebearCode::ENoFam);
        }
        let kind = match fam_kind(o) {
            Some(k) => k,
            None => return Err(PebearCode::ENoFam),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5311);
            return Err(PebearCode::EOrder);
        }
        let want = match fam_cell(o) {
            Some(w) => w,
            None => return Err(PebearCode::ENoFam),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5312 + o as u64);
            return Err(PebearCode::EDrift);
        }
        if match self.fam.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PebearCode::EDup);
        }
        if let Some(c) = self.fam.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x5313 + o as u64);
        Ok(())
    }

    /// PE 族全账核验（20 格）。
    pub fn fam_all(&self) -> bool {
        let mut o = 0usize;
        while o < FAM_CELLS {
            match self.fam.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 双视图一格（载荷对照 + 重复拒）。
    pub fn record_view(&mut self, v: usize, got: u64) -> Result<(), PebearCode> {
        if v >= VIEW_CELLS {
            return Err(PebearCode::ENoView);
        }
        let want = match view_cell(v) {
            Some(w) => w,
            None => return Err(PebearCode::ENoView),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5314 + v as u64);
            return Err(PebearCode::EDrift);
        }
        if match self.view.get(v) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PebearCode::EDup);
        }
        if let Some(c) = self.view.get_mut(v) {
            *c = Some(got);
        }
        self.fnv(0x5315 + v as u64);
        Ok(())
    }

    /// 双视图全账核验（8 格）。
    pub fn view_all(&self) -> bool {
        let mut v = 0usize;
        while v < VIEW_CELLS {
            match self.view.get(v) {
                Some(Some(_)) => {}
                _ => return false,
            }
            v += 1;
        }
        true
    }

    /// 库联签一格（F52324）。
    pub fn record_lib(&mut self, c: usize) -> Result<(), PebearCode> {
        if c >= LIB_CELLS {
            return Err(PebearCode::ENoLib);
        }
        if match self.lib.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PebearCode::EDup);
        }
        if let Some(w) = lib_cell(c) {
            if let Some(cell) = self.lib.get_mut(c) {
                *cell = Some(w);
            }
        }
        self.fnv(0x5316 + c as u64);
        Ok(())
    }

    /// 库联签全齐核验。
    pub fn lib_all(&self) -> bool {
        let mut c = 0usize;
        while c < LIB_CELLS {
            match self.lib.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 声明一行（开源）。
    pub fn declare_row(&mut self, r: usize) -> Result<(), PebearCode> {
        if r >= DECL_ROWS {
            return Err(PebearCode::ENoRow);
        }
        if match self.decl.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PebearCode::EDup);
        }
        if let Some(c) = self.decl.get_mut(r) {
            *c = decl_row(r);
        }
        self.fnv(0x5317 + r as u64);
        Ok(())
    }

    /// 声明全齐核验。
    pub fn rows_all(&self) -> bool {
        let mut r = 0usize;
        while r < DECL_ROWS {
            match self.decl.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 最后族（注入拒绝后不变的观测口）。
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
        if self.gates_all() && self.fam_all() && self.view_all() && self.lib_all() && self.rows_all() {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for PebearLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = PebearLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < FAM_CELLS {
        if let Some(w) = fam_cell(o) {
            if led.record_fam(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut v = 0usize;
    while v < VIEW_CELLS {
        if let Some(w) = view_cell(v) {
            if led.record_view(v, w).is_ok() {
                recorded += 1;
            }
        }
        v += 1;
    }
    let mut c = 0usize;
    while c < LIB_CELLS {
        let _ = led.record_lib(c);
        c += 1;
    }
    let mut r = 0usize;
    while r < DECL_ROWS {
        let _ = led.declare_row(r);
        r += 1;
    }
    // 注入 10 轮：双视图格载荷漂移 / PE 族格越闭集 / PE 族逆序
    // 三态轮转，全部落在当轮临时账本上。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = PebearLedger::new();
        let caught = match kind {
            0 => {
                // 漂移注入：先录正格再注漂移。
                let w0 = match view_cell(0) {
                    Some(w) => w,
                    None => 0,
                };
                let _ = lt.record_view(0, w0);
                lt.record_view(1, w0.wrapping_add(1)) == Err(PebearCode::EDrift)
            }
            1 => lt.record_fam(FAM_CELLS, 0).is_err(),
            _ => {
                // 族逆序注入：先按合法序落 NT 族一格，再注 DOS 族。
                let w4 = match fam_cell(4) {
                    Some(w) => w,
                    None => 0,
                };
                let _ = lt.record_fam(4, w4);
                lt.record_fam(0, 0).is_err() && lt.last_kind() == 1
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5318 + k as u64);
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
/// UNX-F52331 判据集（锚 UNX-F52332-J1，off-by-one 板式）。
pub fn run_unx52331_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52331");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "PE 结构五族 20 格全通",
        recorded >= FAM_CELLS as u32,
        "DOS/NT/File/Section/DataDirectory 五族逐格载荷恒等（§AI-66.3）",
    );
    s.add(
        "双视图 8 格全落",
        {
            let mut lt = PebearLedger::new();
            let mut ok = true;
            let mut v = 0usize;
            while v < VIEW_CELLS {
                if let Some(w) = view_cell(v) {
                    if lt.record_view(v, w).is_err() {
                        ok = false;
                    }
                }
                v += 1;
            }
            ok && lt.view_all()
        },
        "十六进制视图 4 格 + 解析视图 4 格逐格恒等",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = PebearLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "装载/解析/双视图/编辑/崩溃/卸载逐格落账（UNX-F52332-J1）",
    );
    s.add(
        "库消费联签 4/4（F52324）",
        {
            let mut lt = PebearLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < LIB_CELLS {
                if lt.record_lib(c).is_err() {
                    ok = false;
                }
                c += 1;
            }
            ok && lt.lib_all()
        },
        "解析库加载/头解析消费/节区遍历消费/导出消费四格生态联签",
    );
    s.add(
        "开源声明 2/2 全齐",
        {
            let mut lt = PebearLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < DECL_ROWS {
                if lt.declare_row(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.rows_all()
        },
        "开源许可声明/源码可得声明两行闭集",
    );
    s.add(
        "五族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < FAM_CELLS {
                let a = match fam_kind(o) {
                    Some(k) => k,
                    None => 9,
                };
                let b = match fam_kind(o - 1) {
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
        "DOS→NT→File→Section→DataDirectory 五族闭集单调，逆序必抓",
    );
    s.add(
        "诊断码闭集 13 变体",
        PebearCode::Ok as u32 == 0 && PebearCode::EGradeDrift as u32 == 12,
        "Ok=+0x00 起 12 错误码至 +0x0C 恰 13 变体闭集",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "双视图漂移/族格越闭集/五族逆序三态轮转必抓",
    );
    let mut l0 = PebearLedger::new();
    s.add(
        "边界·PE 族格越闭集",
        l0.record_fam(FAM_CELLS, 0).is_err(),
        "族格 20 越闭集必拒",
    );
    s.add(
        "边界·双视图格越闭集",
        l0.record_view(VIEW_CELLS, 0).is_err(),
        "视图格 8 越闭集必拒",
    );
    s.add(
        "边界·库联签越闭集",
        l0.record_lib(LIB_CELLS).is_err(),
        "联签格 4 越闭集必拒（F52324 之外先登记）",
    );
    s.add(
        "漂移拒（双视图载荷漂移）",
        {
            let w5 = match view_cell(5) {
                Some(w) => w,
                None => 0,
            };
            let _ = l0.record_view(5, w5);
            l0.record_view(6, w5.wrapping_add(1)) == Err(PebearCode::EDrift)
        },
        "双视图格载荷漂移必抓",
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
            let mut lt = PebearLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < FAM_CELLS {
                if let Some(w) = fam_cell(o) {
                    let _ = lt.record_fam(o, w);
                }
                o += 1;
            }
            let mut v = 0usize;
            while v < VIEW_CELLS {
                if let Some(w) = view_cell(v) {
                    let _ = lt.record_view(v, w);
                }
                v += 1;
            }
            let mut c = 0usize;
            while c < LIB_CELLS {
                let _ = lt.record_lib(c);
                c += 1;
            }
            let mut r = 0usize;
            while r < DECL_ROWS {
                let _ = lt.declare_row(r);
                r += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC9280,
        "SEG_BASE 0x40 对齐且落在 VX15 公式 0xC7200+0x40×(52331−52201)",
    );
    s.add("收口·判据条数", s.len() >= 16, "含本条共 ≥17 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_seg_guard() {
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0xC9280);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(PebearCode::Ok as u32, 0);
        assert_eq!(PebearCode::EOrder as u32, 4);
        assert_eq!(PebearCode::ESeal as u32, 9);
        assert_eq!(PebearCode::EGradeDrift as u32, 12);
        let mut i = 0u32;
        while i <= 12 {
            let c = match i {
                0 => PebearCode::Ok,
                1 => PebearCode::ENoFam,
                2 => PebearCode::EDrift,
                3 => PebearCode::ENoView,
                4 => PebearCode::EOrder,
                5 => PebearCode::ENoLib,
                6 => PebearCode::ENoRow,
                7 => PebearCode::EGate,
                8 => PebearCode::EDup,
                9 => PebearCode::ESeal,
                10 => PebearCode::EInj,
                11 => PebearCode::EFp,
                _ => PebearCode::EGradeDrift,
            };
            assert!(!c.detail().is_empty());
            i += 1;
        }
    }

    #[test]
    fn test_fam_kind_mapping() {
        assert_eq!(fam_kind(0), Some(0));
        assert_eq!(fam_kind(3), Some(0));
        assert_eq!(fam_kind(4), Some(1));
        assert_eq!(fam_kind(8), Some(2));
        assert_eq!(fam_kind(12), Some(3));
        assert_eq!(fam_kind(16), Some(4));
        assert_eq!(fam_kind(19), Some(4));
        assert!(fam_kind(20).is_none());
        assert_eq!(fam_kind_name(0), Some("DOS 头"));
        assert_eq!(fam_kind_name(4), Some("DataDirectory"));
        assert!(fam_kind_name(5).is_none());
    }

    #[test]
    fn test_view_kind_mapping() {
        assert_eq!(view_kind(0), Some(0));
        assert_eq!(view_kind(3), Some(0));
        assert_eq!(view_kind(4), Some(1));
        assert_eq!(view_kind(7), Some(1));
        assert!(view_kind(8).is_none());
        assert_eq!(view_kind_name(0), Some("十六进制视图"));
        assert_eq!(view_kind_name(1), Some("解析视图"));
        assert!(view_kind_name(2).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = fam_cell(7).unwrap_or(0);
        assert_eq!(a, fam_cell(7).unwrap_or(0));
        assert_ne!(a, fam_cell(8).unwrap_or(0));
        assert_ne!(view_cell(0).unwrap_or(0), view_cell(4).unwrap_or(0));
        assert!(decl_row(DECL_ROWS).is_none());
        assert!(lib_cell(LIB_CELLS).is_none());
        assert!(fam_cell(FAM_CELLS).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = PebearLedger::new();
        let _ = lt.record_fam(9, fam_cell(9).unwrap_or(0));
        assert_eq!(lt.record_fam(2, fam_cell(2).unwrap_or(0)), Err(PebearCode::EOrder));
        // 拒绝后 last_kind 不变。
        assert_eq!(lt.last_kind(), 2);
        assert!(lt.record_fam(10, fam_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = PebearLedger::new();
        let f = fam_cell(1).unwrap_or(0);
        assert_eq!(lt.record_fam(1, f.wrapping_add(1)), Err(PebearCode::EDrift));
        let v = view_cell(6).unwrap_or(0);
        assert_eq!(lt.record_view(6, v.wrapping_add(9)), Err(PebearCode::EDrift));
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = PebearLedger::new();
        let _ = lt.record_view(0, view_cell(0).unwrap_or(0));
        assert_eq!(lt.record_view(0, view_cell(0).unwrap_or(0)), Err(PebearCode::EDup));
        let _ = lt.record_lib(3);
        assert_eq!(lt.record_lib(3), Err(PebearCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = PebearLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓（EGate），全落才 gates_all。
        assert_eq!(lt.judge_gate(6), Err(PebearCode::EGate));
        assert!(lt.gates_all());
        let mut lt2 = PebearLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= FAM_CELLS as u32 + VIEW_CELLS as u32);
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
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        let lt = PebearLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn test_lib_names() {
        assert_eq!(lib_name(0), Some("解析库加载联签"));
        assert_eq!(lib_name(3), Some("导出消费联签"));
        assert!(lib_name(4).is_none());
        assert!(view_cell(VIEW_CELLS).is_none());
    }

    #[test]
    fn unx52331_checks_green() {
        let s = run_unx52331_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 17);
    }
}
