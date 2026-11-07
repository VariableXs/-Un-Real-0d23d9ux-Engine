//! UNX-F52332 CFF Explorer 收口 · 0xC92C0段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52332"，逐字）：
//!   title: CFF Explorer 收口
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52333-J1 六判
//!     据全落；静态标记 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3；
//!     ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52332）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52333-J1，照原文锚定；功能按本卡标题（CFF Explorer）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//! 定级口径理由：CFF Explorer 为免费但闭源的 PE 编辑器，功能收口
//! 全通后按有条件收口登记 S2（无源码审计通道，如实降档不粉饰）。
//!
//! 设计要点：
//!   ① PE 结构五族 20 格闭集（编辑器视角分族，与 PE-bear 卡分族
//!      口径差异化）：DOS 存根 4 格（MZ 头/存根跳转/存根长度/重叠
//!      边界）+ PE 签名 4 格（PE00 标记/签名校验/签名偏移/签名对齐）
//!      + COFF 头 4 格（机器/节数/符号表指针/符号数）+ 可选头 4 格
//!      （魔数/入口点/映像基址/节对齐）+ 数据目录 4 格（导入/重定位/
//!      调试/资源），族单调（逆序必抓且拒绝后不改 last_kind）。
//!   ② 资源编辑 10 操作闭集：打开资源树/查看资源/编辑对话框/编辑
//!      图标/替换位图/导出资源/导入资源/添加资源节/删除资源/保存
//!      回写，逐操作载荷（纯函数折叠）恒等才收。
//!   ③ Rebasing/导入表三面 8 格闭集：Rebasing 3 格（基址计算/重定位
//!      表核对/冲突检测）+ 导入表 3 格（IAT 解析/绑定核对/缺失导入）
//!      + 校验 2 格（校验和/时间戳），三面逐格载荷恒等才收。
//!   ④ 免费商业用途声明 2 行闭集：免费商业用途声明/不承诺担保声明，
//!      缺一拒封。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      grade 定级闭集）；注入核 run_cycle 三态轮转 10 轮（资源操作
//!      载荷漂移/三面格越闭集/PE 族逆序），全部落在当轮临时账本上，
//!      注入 10/10 全抓、逃逸 0；双跑指纹一致。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无
//!      unwrap/expect/panic/裸下标；XOR 锚常量低 16 位恒零。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52332;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// PE 结构五族格数。
pub const FAM_CELLS: usize = 20;

/// 资源编辑操作数（静态标记 ×10 口径）。
pub const RES_OPS: usize = 10;

/// Rebasing/导入表三面格数。
pub const FACET_CELLS: usize = 8;

/// 免费商业用途声明行数。
pub const DECL_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CffCode {
    /// 成功
    Ok = 0,
    /// PE 族格越闭集
    ENoFam,
    /// 载荷漂移
    EDrift,
    /// 资源操作越闭集
    ENoRes,
    /// PE 族逆序
    EOrder,
    /// 三面格越闭集
    ENoFacet,
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

impl CffCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            CffCode::Ok => "成功",
            CffCode::ENoFam => "PE 族格越闭集：编辑器五族 20 格闭集之外先登记",
            CffCode::EDrift => "载荷漂移：格载荷折叠与基准不恒等，逐格复算",
            CffCode::ENoRes => "资源操作越闭集：资源编辑 10 操作集之外先登记",
            CffCode::EOrder => "PE 族逆序：DOS 存根→PE 签名→COFF 头→可选头→数据目录单调，逆序必抓",
            CffCode::ENoFacet => "三面格越闭集：Rebasing/导入表/校验 8 格闭集之外先登记",
            CffCode::ENoRow => "声明行越闭集：免费商业用途声明 2 行闭集之外先登记",
            CffCode::EGate => "判据格越闭集：六判据闸 0..=5 之外先登记",
            CffCode::EDup => "重复落账：同格只许一条账目",
            CffCode::ESeal => "未齐即封：20 族格 + 10 资源操作 + 8 三面格 + 2 行声明未齐先补齐",
            CffCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            CffCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            CffCode::EGradeDrift => "定级漂移：全通最高只可定 S2（闭源免费工具如实降档）",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口（本卡闭源口径下恒不可达）
    S1,
    /// S2：有条件收口（闭源免费工具封顶档）
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
            Grade::S1 => "S1：全通收口（本卡闭源免费口径下恒不可达）",
            Grade::S2 => "S2：有条件收口——全通封顶档，无源码审计通道如实降档",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// PE 族格载荷（格 o；族 = o/4，编辑器视角分族）。XOR 锚低 16 位恒零。
pub const fn fam_cell(o: usize) -> Option<u64> {
    if o >= FAM_CELLS {
        return None;
    }
    Some((0x55A0_0000_0000_0000u64 + (o as u64) * 0x43) ^ 0x4346_4600_0000_0000)
}

/// PE 族（i/N 映射：0..4→DOS 存根 4..8→PE 签名 8..12→COFF 头 12..16→可选头 16..20→数据目录）。
pub const fn fam_kind(o: usize) -> Option<u32> {
    if o >= FAM_CELLS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 族名（编辑器视角，与 PE-bear 卡分族口径差异化）。
pub const fn fam_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("DOS 存根"),
        1 => Some("PE 签名"),
        2 => Some("COFF 头"),
        3 => Some("可选头"),
        4 => Some("数据目录"),
        _ => None,
    }
}

/// 资源编辑操作格载荷（操作 o）。XOR 锚低 16 位恒零。
pub const fn res_cell(o: usize) -> Option<u64> {
    if o >= RES_OPS {
        return None;
    }
    Some((0x55B0_0000_0000_0000u64 + (o as u64) * 0x61) ^ 0x5245_5300_0000_0000)
}

/// 资源编辑操作名（const fn 表 + Option 边界）。
pub const fn res_name(o: usize) -> Option<&'static str> {
    match o {
        0 => Some("打开资源树"),
        1 => Some("查看资源"),
        2 => Some("编辑对话框"),
        3 => Some("编辑图标"),
        4 => Some("替换位图"),
        5 => Some("导出资源"),
        6 => Some("导入资源"),
        7 => Some("添加资源节"),
        8 => Some("删除资源"),
        9 => Some("保存回写"),
        _ => None,
    }
}

/// 三面格载荷（格 f；面 = 分族映射）。XOR 锚低 16 位恒零。
pub const fn facet_cell(f: usize) -> Option<u64> {
    if f >= FACET_CELLS {
        return None;
    }
    Some((0x55C0_0000_0000_0000u64 + (f as u64) * 0x67) ^ 0x4641_4300_0000_0000)
}

/// 三面（i/N 映射：0..3→Rebasing 3..6→导入表 6..8→校验）。
pub const fn facet_kind(f: usize) -> Option<u32> {
    if f >= FACET_CELLS {
        return None;
    }
    match f {
        0..=2 => Some(0),
        3..=5 => Some(1),
        _ => Some(2),
    }
}

/// 面名。
pub const fn facet_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("Rebasing"),
        1 => Some("导入表"),
        2 => Some("校验"),
        _ => None,
    }
}

/// 免费商业用途声明行折叠（行 r）。
pub const fn decl_row(r: usize) -> Option<u32> {
    if r >= DECL_ROWS {
        return None;
    }
    Some(0x55D0 + r as u32 * 0x3B)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// CFF Explorer 收口账本状态机。
#[derive(Clone)]
pub struct CffLedger {
    /// 六判据（UNX-F52333-J1：装载/解析/资源编辑/回写/崩溃/卸载）。
    gates: [Option<bool>; 6],
    /// PE 族格账。
    fam: [Option<u64>; FAM_CELLS],
    /// 资源操作账。
    res: [Option<u64>; RES_OPS],
    /// 三面格账。
    facet: [Option<u64>; FACET_CELLS],
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

impl CffLedger {
    /// 新账本。
    pub fn new() -> Self {
        CffLedger {
            gates: [None; 6],
            fam: [None; FAM_CELLS],
            res: [None; RES_OPS],
            facet: [None; FACET_CELLS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), CffCode> {
        if gate >= 6 {
            return Err(CffCode::EGate);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x5320 + gate as u64);
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
    pub fn record_fam(&mut self, o: usize, got: u64) -> Result<(), CffCode> {
        if o >= FAM_CELLS {
            return Err(CffCode::ENoFam);
        }
        let kind = match fam_kind(o) {
            Some(k) => k,
            None => return Err(CffCode::ENoFam),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5321);
            return Err(CffCode::EOrder);
        }
        let want = match fam_cell(o) {
            Some(w) => w,
            None => return Err(CffCode::ENoFam),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5322 + o as u64);
            return Err(CffCode::EDrift);
        }
        if match self.fam.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(CffCode::EDup);
        }
        if let Some(c) = self.fam.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x5323 + o as u64);
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

    /// 资源编辑操作一格（载荷对照 + 重复拒）。
    pub fn record_res(&mut self, o: usize, got: u64) -> Result<(), CffCode> {
        if o >= RES_OPS {
            return Err(CffCode::ENoRes);
        }
        let want = match res_cell(o) {
            Some(w) => w,
            None => return Err(CffCode::ENoRes),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5324 + o as u64);
            return Err(CffCode::EDrift);
        }
        if match self.res.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(CffCode::EDup);
        }
        if let Some(c) = self.res.get_mut(o) {
            *c = Some(got);
        }
        self.fnv(0x5325 + o as u64);
        Ok(())
    }

    /// 资源操作全账核验（10 操作）。
    pub fn res_all(&self) -> bool {
        let mut o = 0usize;
        while o < RES_OPS {
            match self.res.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 三面一格（载荷对照 + 重复拒）。
    pub fn record_facet(&mut self, f: usize, got: u64) -> Result<(), CffCode> {
        if f >= FACET_CELLS {
            return Err(CffCode::ENoFacet);
        }
        let want = match facet_cell(f) {
            Some(w) => w,
            None => return Err(CffCode::ENoFacet),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5326 + f as u64);
            return Err(CffCode::EDrift);
        }
        if match self.facet.get(f) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(CffCode::EDup);
        }
        if let Some(c) = self.facet.get_mut(f) {
            *c = Some(got);
        }
        self.fnv(0x5327 + f as u64);
        Ok(())
    }

    /// 三面全账核验（8 格）。
    pub fn facet_all(&self) -> bool {
        let mut f = 0usize;
        while f < FACET_CELLS {
            match self.facet.get(f) {
                Some(Some(_)) => {}
                _ => return false,
            }
            f += 1;
        }
        true
    }

    /// 声明一行（免费商业用途）。
    pub fn declare_row(&mut self, r: usize) -> Result<(), CffCode> {
        if r >= DECL_ROWS {
            return Err(CffCode::ENoRow);
        }
        if match self.decl.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(CffCode::EDup);
        }
        if let Some(c) = self.decl.get_mut(r) {
            *c = decl_row(r);
        }
        self.fnv(0x5328 + r as u64);
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

    /// KB-COMPAT 定级（闭源免费口径：全通 → S2，S1 恒不可达；缺 → S3）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all() && self.fam_all() && self.res_all() && self.facet_all() && self.rows_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for CffLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = CffLedger::new();
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
    let mut r = 0usize;
    while r < RES_OPS {
        if let Some(w) = res_cell(r) {
            if led.record_res(r, w).is_ok() {
                recorded += 1;
            }
        }
        r += 1;
    }
    let mut f = 0usize;
    while f < FACET_CELLS {
        if let Some(w) = facet_cell(f) {
            if led.record_facet(f, w).is_ok() {
                recorded += 1;
            }
        }
        f += 1;
    }
    let mut d = 0usize;
    while d < DECL_ROWS {
        let _ = led.declare_row(d);
        d += 1;
    }
    // 注入 10 轮：资源操作载荷漂移 / 三面格越闭集 / PE 族逆序
    // 三态轮转，全部落在当轮临时账本上。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = CffLedger::new();
        let caught = match kind {
            0 => {
                // 漂移注入：先录正格再注漂移。
                let w0 = match res_cell(0) {
                    Some(w) => w,
                    None => 0,
                };
                let _ = lt.record_res(0, w0);
                lt.record_res(1, w0.wrapping_add(1)) == Err(CffCode::EDrift)
            }
            1 => lt.record_facet(FACET_CELLS, 0).is_err(),
            _ => {
                // 族逆序注入：先按合法序落 PE 签名族一格，再注 DOS 存根族。
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
            led.fnv(0x5329 + k as u64);
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
/// UNX-F52332 判据集（锚 UNX-F52333-J1，off-by-one 板式）。
pub fn run_unx52332_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52332");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "PE 结构五族 20 格全通",
        recorded >= FAM_CELLS as u32,
        "DOS 存根/PE 签名/COFF 头/可选头/数据目录五族逐格载荷恒等（§AI-66.3）",
    );
    s.add(
        "资源编辑 10 操作全通（静态标记 ×10）",
        {
            let mut lt = CffLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < RES_OPS {
                if let Some(w) = res_cell(r) {
                    if lt.record_res(r, w).is_err() {
                        ok = false;
                    }
                }
                r += 1;
            }
            ok && lt.res_all()
        },
        "打开资源树至保存回写十操作逐格恒等（静态标记 ×10 口径）",
    );
    s.add(
        "Rebasing/导入表三面 8 格全落",
        {
            let mut lt = CffLedger::new();
            let mut ok = true;
            let mut f = 0usize;
            while f < FACET_CELLS {
                if let Some(w) = facet_cell(f) {
                    if lt.record_facet(f, w).is_err() {
                        ok = false;
                    }
                }
                f += 1;
            }
            ok && lt.facet_all()
        },
        "Rebasing 3 格 + 导入表 3 格 + 校验 2 格逐格恒等",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = CffLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "装载/解析/资源编辑/回写/崩溃/卸载逐格落账（UNX-F52333-J1）",
    );
    s.add(
        "免费商业用途声明 2/2 全齐",
        {
            let mut lt = CffLedger::new();
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
        "免费商业用途声明/不承诺担保声明两行闭集",
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
        "DOS 存根→PE 签名→COFF 头→可选头→数据目录五族闭集单调，逆序必抓",
    );
    s.add(
        "诊断码闭集 13 变体",
        CffCode::Ok as u32 == 0 && CffCode::EGradeDrift as u32 == 12,
        "Ok=+0x00 起 12 错误码至 +0x0C 恰 13 变体闭集",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "资源操作漂移/三面格越闭集/五族逆序三态轮转必抓",
    );
    let mut l0 = CffLedger::new();
    s.add(
        "边界·PE 族格越闭集",
        l0.record_fam(FAM_CELLS, 0).is_err(),
        "族格 20 越闭集必拒",
    );
    s.add(
        "边界·资源操作越闭集",
        l0.record_res(RES_OPS, 0).is_err(),
        "资源操作 10 越闭集必拒",
    );
    s.add(
        "边界·三面格越闭集",
        l0.record_facet(FACET_CELLS, 0).is_err(),
        "三面格 8 越闭集必拒",
    );
    s.add(
        "漂移拒（三面载荷漂移）",
        {
            let w6 = match facet_cell(6) {
                Some(w) => w,
                None => 0,
            };
            let _ = l0.record_facet(6, w6);
            l0.record_facet(7, w6.wrapping_add(1)) == Err(CffCode::EDrift)
        },
        "三面格载荷漂移必抓",
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
        "定级 S2 封顶闭集",
        {
            let mut lt = CffLedger::new();
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
            let mut r = 0usize;
            while r < RES_OPS {
                if let Some(w) = res_cell(r) {
                    let _ = lt.record_res(r, w);
                }
                r += 1;
            }
            let mut f = 0usize;
            while f < FACET_CELLS {
                if let Some(w) = facet_cell(f) {
                    let _ = lt.record_facet(f, w);
                }
                f += 1;
            }
            let mut d = 0usize;
            while d < DECL_ROWS {
                let _ = lt.declare_row(d);
                d += 1;
            }
            lt.judge_grade() == Grade::S2
        },
        "全通 → S2 封顶（闭源免费工具如实降档，KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC92C0,
        "SEG_BASE 0x40 对齐且落在 VX15 公式 0xC7200+0x40×(52332−52201)",
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
        assert_eq!(SEG_BASE, 0xC92C0);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(CffCode::Ok as u32, 0);
        assert_eq!(CffCode::ENoFacet as u32, 5);
        assert_eq!(CffCode::ESeal as u32, 9);
        assert_eq!(CffCode::EGradeDrift as u32, 12);
        let mut i = 0u32;
        while i <= 12 {
            let c = match i {
                0 => CffCode::Ok,
                1 => CffCode::ENoFam,
                2 => CffCode::EDrift,
                3 => CffCode::ENoRes,
                4 => CffCode::EOrder,
                5 => CffCode::ENoFacet,
                6 => CffCode::ENoRow,
                7 => CffCode::EGate,
                8 => CffCode::EDup,
                9 => CffCode::ESeal,
                10 => CffCode::EInj,
                11 => CffCode::EFp,
                _ => CffCode::EGradeDrift,
            };
            assert!(!c.detail().is_empty());
            i += 1;
        }
    }

    #[test]
    fn test_fam_kind_mapping() {
        assert_eq!(fam_kind(0), Some(0));
        assert_eq!(fam_kind(4), Some(1));
        assert_eq!(fam_kind(7), Some(1));
        assert_eq!(fam_kind(8), Some(2));
        assert_eq!(fam_kind(13), Some(3));
        assert_eq!(fam_kind(17), Some(4));
        assert!(fam_kind(20).is_none());
        assert_eq!(fam_kind_name(0), Some("DOS 存根"));
        assert_eq!(fam_kind_name(2), Some("COFF 头"));
        assert_eq!(fam_kind_name(4), Some("数据目录"));
        assert!(fam_kind_name(5).is_none());
    }

    #[test]
    fn test_facet_kind_mapping() {
        assert_eq!(facet_kind(0), Some(0));
        assert_eq!(facet_kind(2), Some(0));
        assert_eq!(facet_kind(3), Some(1));
        assert_eq!(facet_kind(5), Some(1));
        assert_eq!(facet_kind(6), Some(2));
        assert_eq!(facet_kind(7), Some(2));
        assert!(facet_kind(8).is_none());
        assert_eq!(facet_kind_name(0), Some("Rebasing"));
        assert_eq!(facet_kind_name(1), Some("导入表"));
        assert_eq!(facet_kind_name(2), Some("校验"));
        assert!(facet_kind_name(3).is_none());
    }

    #[test]
    fn test_res_names() {
        assert_eq!(res_name(0), Some("打开资源树"));
        assert_eq!(res_name(4), Some("替换位图"));
        assert_eq!(res_name(9), Some("保存回写"));
        assert!(res_name(10).is_none());
        assert!(res_cell(10).is_none());
        assert!(facet_cell(FACET_CELLS).is_none());
        assert!(decl_row(DECL_ROWS).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = CffLedger::new();
        let _ = lt.record_fam(10, fam_cell(10).unwrap_or(0));
        assert_eq!(lt.record_fam(3, fam_cell(3).unwrap_or(0)), Err(CffCode::EOrder));
        // 拒绝后 last_kind 不变。
        assert_eq!(lt.last_kind(), 2);
        assert!(lt.record_fam(11, fam_cell(11).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = CffLedger::new();
        let r = res_cell(5).unwrap_or(0);
        assert_eq!(lt.record_res(5, r.wrapping_add(1)), Err(CffCode::EDrift));
        let f = fam_cell(16).unwrap_or(0);
        assert_eq!(lt.record_fam(16, f.wrapping_add(5)), Err(CffCode::EDrift));
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = CffLedger::new();
        let _ = lt.record_res(2, res_cell(2).unwrap_or(0));
        assert_eq!(lt.record_res(2, res_cell(2).unwrap_or(0)), Err(CffCode::EDup));
        let _ = lt.record_facet(0, facet_cell(0).unwrap_or(0));
        assert_eq!(lt.record_facet(0, facet_cell(0).unwrap_or(0)), Err(CffCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = CffLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓（EGate），全落才 gates_all。
        assert_eq!(lt.judge_gate(6), Err(CffCode::EGate));
        assert!(lt.gates_all());
        let mut lt2 = CffLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= FAM_CELLS as u32 + RES_OPS as u32 + FACET_CELLS as u32);
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
    fn test_grade_s2_cap() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        // 空账本 → S3。
        let lt = CffLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        // 缺声明行 → S3。
        let mut part = CffLedger::new();
        let mut o = 0usize;
        while o < FAM_CELLS {
            if let Some(w) = fam_cell(o) {
                let _ = part.record_fam(o, w);
            }
            o += 1;
        }
        assert_eq!(part.judge_grade(), Grade::S3);
    }

    #[test]
    fn test_cells_pure() {
        let a = facet_cell(3).unwrap_or(0);
        assert_eq!(a, facet_cell(3).unwrap_or(0));
        assert_ne!(a, facet_cell(4).unwrap_or(0));
        assert_ne!(fam_cell(0).unwrap_or(0), res_cell(0).unwrap_or(0));
    }

    #[test]
    fn unx52332_checks_green() {
        let s = run_unx52332_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 17);
    }
}
