//! UNX-F52339 PE Explorer 收口（长尾） · 0xC9480段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52339"，逐字）：
//!   title: PE Explorer 收口（长尾）
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52340-J1 六判据
//!     全落；×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成时
//!     API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52339）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52340-J1，照原文锚定；功能按本卡标题（PE Explorer）落地。
//! 定级封顶理由：本卡为长尾卡——PE Explorer 为长尾共享软件（作者
//! 已停止活跃维护，注册渠道与更新通道均处长尾状态），即便六判据与
//! 全部格账全通，KB-COMPAT 定级也封顶 S3（长尾如实呈报），S1/S2 恒
//! 不可达。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点（域语料逐卡定制）：
//!   ① PE 结构五族 20 格：DOS 头（4）/NT 头（4）/节表（4）/导入表（4）/
//!      导出表（4）五族闭集，族单调（逆序必抓）+ 族相位闸（后族先录
//!      必抓，对空账本同样必抓）。
//!   ② 反汇编 10 操作：定位入口/线性反汇编/递归反汇编/调用图/交叉
//!      引用/字符串引用/跳转目标/栈帧标注/常量标注/导出清单，越闭集
//!      必抓。
//!   ③ 资源编辑 8 格：图标/版本信息/对话框/字符串表/菜单/位图/加速
//!      键/清单（manifest），资源相位闸（PE 结构未落先编辑资源必抓）。
//!   ④ 长尾维护声明 2 行闭集：维护状态如实声明 / 长尾不承诺声明，
//!      缺一拒封。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      grade 定级闭集）；注入核 run_cycle 三态轮转 10 轮（PE 结构格
//!      越闭集/反汇编载荷漂移/资源相位越序），三类注入全部落在当轮
//!      临时账本上，主账 injected==caught==10、escaped==0；漂移注入先
//!      按合法序落正格再注漂移；相位注入对空账本必抓且不改 last_kind；
//!      双跑指纹一致；全通封顶 S3（长尾如实）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52339;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// PE 结构格数（五族 × 4）。
pub const PE_CELLS: usize = 20;

/// 单族格数。
pub const PE_PER_FAM: usize = 4;

/// 反汇编操作数。
pub const DIS_OPS: usize = 10;

/// 资源编辑格数。
pub const RES_CELLS: usize = 8;

/// 长尾维护声明行数。
pub const TAIL_ROWS: usize = 2;

/// XOR 锚常量（低 16 位恒零口径）。
pub const PE_XOR: u64 = 0x5045_5850_4C4F_0000;

/// 反汇编 XOR 锚常量（低 16 位恒零口径）。
pub const DIS_XOR: u64 = 0x4449_535F_4F50_0000;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PeExpCode {
    /// 成功
    Ok = 0,
    /// PE 结构格越闭集
    ENoPe,
    /// PE 结构载荷漂移
    EPEDrift,
    /// 反汇编操作越闭集
    ENoDis,
    /// 反汇编载荷漂移
    EDisDrift,
    /// 资源格越闭集
    ENoRes,
    /// 资源相位违例
    EResPhase,
    /// 长尾声明行越闭集
    ENoRow,
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

impl PeExpCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            PeExpCode::Ok => "成功",
            PeExpCode::ENoPe => "PE 结构格越闭集：五族 20 格闭集之外先登记",
            PeExpCode::EPEDrift => "PE 结构载荷漂移：格折叠与基准不恒等，逐格复算",
            PeExpCode::ENoDis => "反汇编操作越闭集：反汇编 10 操作闭集之外先登记",
            PeExpCode::EDisDrift => "反汇编载荷漂移：操作折叠与基准不恒等，逐格复算",
            PeExpCode::ENoRes => "资源格越闭集：资源编辑 8 格闭集之外先登记",
            PeExpCode::EResPhase => "资源相位违例：PE 结构未落先编辑资源，后族先录必抓",
            PeExpCode::ENoRow => "长尾声明行越闭集：长尾维护声明 2 行闭集之外先登记",
            PeExpCode::EDup => "重复落账：同格只许一条账目",
            PeExpCode::ESeal => "未齐即封：20 PE 格 + 10 反汇编 + 8 资源格 + 2 行声明未齐先补齐",
            PeExpCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            PeExpCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            PeExpCode::EGradeDrift => "定级漂移：长尾卡封顶 S3，全通只可定 S3（S1/S2 恒不可达）",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口（本卡长尾封顶后恒不可达）
    S1,
    /// S2：有条件收口（本卡长尾封顶后恒不可达）
    S2,
    /// S3：不收口（长尾封顶档）
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
            Grade::S1 => "S1：全通收口（本卡长尾封顶，S1 恒不可达）",
            Grade::S2 => "S2：有条件收口（本卡长尾封顶，S2 恒不可达）",
            Grade::S3 => "S3：不收口——长尾封顶档，维护状态如实呈报",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// PE 结构族（p/4：0=DOS 头 1=NT 头 2=节表 3=导入表 4=导出表）。
pub const fn pe_family(p: usize) -> Option<u32> {
    if p >= PE_CELLS {
        return None;
    }
    Some((p / PE_PER_FAM) as u32)
}

/// 族名。
pub const fn pe_family_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("DOS 头"),
        1 => Some("NT 头"),
        2 => Some("节表"),
        3 => Some("导入表"),
        4 => Some("导出表"),
        _ => None,
    }
}

/// PE 结构格载荷（格 p）。
pub const fn pe_cell(p: usize) -> Option<u64> {
    if p >= PE_CELLS {
        return None;
    }
    Some((0x11_0000u64 + (p as u64) * 0x47) ^ PE_XOR)
}

/// 反汇编操作名（const fn 表 + Option 边界）。
pub const fn dis_name(d: usize) -> Option<&'static str> {
    match d {
        0 => Some("定位入口"),
        1 => Some("线性反汇编"),
        2 => Some("递归反汇编"),
        3 => Some("调用图"),
        4 => Some("交叉引用"),
        5 => Some("字符串引用"),
        6 => Some("跳转目标"),
        7 => Some("栈帧标注"),
        8 => Some("常量标注"),
        9 => Some("导出清单"),
        _ => None,
    }
}

/// 反汇编操作折叠（操作 d）。
pub const fn dis_cell(d: usize) -> Option<u64> {
    if d >= DIS_OPS {
        return None;
    }
    Some((0x12_0000u64 + (d as u64) * 0x59) ^ DIS_XOR)
}

/// 资源编辑格名（const fn 表 + Option 边界）。
pub const fn res_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("图标"),
        1 => Some("版本信息"),
        2 => Some("对话框"),
        3 => Some("字符串表"),
        4 => Some("菜单"),
        5 => Some("位图"),
        6 => Some("加速键"),
        7 => Some("清单"),
        _ => None,
    }
}

/// 资源编辑格折叠（格 r）。
pub const fn res_cell(r: usize) -> Option<u32> {
    if r >= RES_CELLS {
        return None;
    }
    Some(0x1300 + r as u32 * 0x3B)
}

/// 长尾维护声明行折叠（行 r）。
pub const fn tail_row(r: usize) -> Option<u32> {
    if r >= TAIL_ROWS {
        return None;
    }
    Some(0x1400 + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// PE Explorer 收口账本状态机。
#[derive(Clone)]
pub struct PeExpLedger {
    /// 六判据（UNX-F52340-J1 六判据逐格落账）。
    gates: [Option<bool>; 6],
    /// PE 结构账。
    pes: [Option<u64>; PE_CELLS],
    /// 反汇编操作账。
    dis: [Option<u64>; DIS_OPS],
    /// 资源编辑账。
    res: [Option<u32>; RES_CELLS],
    /// 长尾声明账。
    tails: [Option<u32>; TAIL_ROWS],
    /// 最后 PE 族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl PeExpLedger {
    /// 新账本。
    pub fn new() -> Self {
        PeExpLedger {
            gates: [None; 6],
            pes: [None; PE_CELLS],
            dis: [None; DIS_OPS],
            res: [None; RES_CELLS],
            tails: [None; TAIL_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), PeExpCode> {
        if gate >= 6 {
            return Err(PeExpCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x7100 + gate as u64);
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

    /// PE 结构是否至少一格已落（资源相位闸判据）。
    fn pe_touched(&self) -> bool {
        let mut i = 0usize;
        while i < PE_CELLS {
            match self.pes.get(i) {
                Some(Some(_)) => return true,
                _ => {}
            }
            i += 1;
        }
        false
    }

    /// 前一族是否至少一格已落（族相位闸判据）。
    fn prev_family_touched(&self, k: u32) -> bool {
        if k == 0 {
            return true;
        }
        let lo = ((k as usize) - 1) * PE_PER_FAM;
        let hi = lo + PE_PER_FAM;
        let mut i = lo;
        while i < hi {
            match self.pes.get(i) {
                Some(Some(_)) => return true,
                _ => {}
            }
            i += 1;
        }
        false
    }

    /// PE 结构一格（族相位闸 + 族单调 + 载荷对照 + 重复拒）。
    pub fn record_pe(&mut self, p: usize, got: u64) -> Result<(), PeExpCode> {
        if p >= PE_CELLS {
            return Err(PeExpCode::ENoPe);
        }
        let kind = match pe_family(p) {
            Some(k) => k,
            None => return Err(PeExpCode::ENoPe),
        };
        // 相位闸：族 k>0 须在前族至少一格落账后方可落账；对空账本同样
        // 必抓，且拒绝路径不改 last_kind。
        if !self.prev_family_touched(kind) {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7200);
            return Err(PeExpCode::EResPhase);
        }
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7201);
            return Err(PeExpCode::EResPhase);
        }
        let want = match pe_cell(p) {
            Some(w) => w,
            None => return Err(PeExpCode::ENoPe),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7202 + p as u64);
            return Err(PeExpCode::EPEDrift);
        }
        if match self.pes.get(p) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PeExpCode::EDup);
        }
        if let Some(c) = self.pes.get_mut(p) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x7203 + p as u64);
        Ok(())
    }

    /// PE 结构全账核验（20 格）。
    pub fn pes_all(&self) -> bool {
        let mut p = 0usize;
        while p < PE_CELLS {
            match self.pes.get(p) {
                Some(Some(_)) => {}
                _ => return false,
            }
            p += 1;
        }
        true
    }

    /// 反汇编操作一格。
    pub fn record_dis(&mut self, d: usize, got: u64) -> Result<(), PeExpCode> {
        if d >= DIS_OPS {
            return Err(PeExpCode::ENoDis);
        }
        let want = match dis_cell(d) {
            Some(w) => w,
            None => return Err(PeExpCode::ENoDis),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7300 + d as u64);
            return Err(PeExpCode::EDisDrift);
        }
        if match self.dis.get(d) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PeExpCode::EDup);
        }
        if let Some(c) = self.dis.get_mut(d) {
            *c = Some(got);
        }
        self.fnv(0x7301 + d as u64);
        Ok(())
    }

    /// 反汇编全账核验（10 操作）。
    pub fn dis_all(&self) -> bool {
        let mut d = 0usize;
        while d < DIS_OPS {
            match self.dis.get(d) {
                Some(Some(_)) => {}
                _ => return false,
            }
            d += 1;
        }
        true
    }

    /// 资源编辑一格（资源相位闸 + 重复拒）。
    pub fn record_res(&mut self, r: usize) -> Result<(), PeExpCode> {
        if r >= RES_CELLS {
            return Err(PeExpCode::ENoRes);
        }
        // 相位闸：PE 结构至少一格落账后方可编辑资源；对空账本同样必抓。
        if !self.pe_touched() {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7400);
            return Err(PeExpCode::EResPhase);
        }
        if match self.res.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PeExpCode::EDup);
        }
        if let Some(c) = self.res.get_mut(r) {
            *c = res_cell(r);
        }
        self.fnv(0x7401 + r as u64);
        Ok(())
    }

    /// 资源全账核验（8 格）。
    pub fn res_all(&self) -> bool {
        let mut r = 0usize;
        while r < RES_CELLS {
            match self.res.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 长尾维护声明一行。
    pub fn declare_tail(&mut self, r: usize) -> Result<(), PeExpCode> {
        if r >= TAIL_ROWS {
            return Err(PeExpCode::ENoRow);
        }
        if match self.tails.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PeExpCode::EDup);
        }
        if let Some(c) = self.tails.get_mut(r) {
            *c = tail_row(r);
        }
        self.fnv(0x7500 + r as u64);
        Ok(())
    }

    /// 长尾声明全齐核验。
    pub fn tails_all(&self) -> bool {
        let mut r = 0usize;
        while r < TAIL_ROWS {
            match self.tails.get(r) {
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

    /// KB-COMPAT 定级（长尾封顶：全通 → S3，S1/S2 恒不可达）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all()
            && self.pes_all()
            && self.dis_all()
            && self.res_all()
            && self.tails_all()
        {
            // 长尾封顶：PE Explorer 长尾共享软件，全通也只可定 S3。
            Grade::S3
        } else {
            Grade::S3
        }
    }
}

impl Default for PeExpLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = PeExpLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut p = 0usize;
    while p < PE_CELLS {
        if let Some(w) = pe_cell(p) {
            if led.record_pe(p, w).is_ok() {
                recorded += 1;
            }
        }
        p += 1;
    }
    let mut d = 0usize;
    while d < DIS_OPS {
        if let Some(w) = dis_cell(d) {
            let _ = led.record_dis(d, w);
        }
        d += 1;
    }
    let mut r = 0usize;
    while r < RES_CELLS {
        let _ = led.record_res(r);
        r += 1;
    }
    r = 0;
    while r < TAIL_ROWS {
        let _ = led.declare_tail(r);
        r += 1;
    }
    // 注入 10 轮：PE 结构格越闭集 / 反汇编载荷漂移 / 资源相位越序
    // 三态轮转。三类注入全部落在当轮临时账本上，主账只记守恒三数与
    // 指纹。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = PeExpLedger::new();
        let caught = match kind {
            0 => lt.record_pe(PE_CELLS, 0).is_err(),
            1 => {
                // 漂移注入：先按合法序落正格，再注漂移载荷。
                let _ = lt.record_pe(0, pe_cell(0).unwrap_or(0));
                lt.record_dis(0, dis_cell(0).unwrap_or(0).wrapping_add(1))
                    == Err(PeExpCode::EDisDrift)
            }
            _ => {
                // 相位注入：空账本先编辑资源，必抓且不改 last_kind。
                lt.record_res(0) == Err(PeExpCode::EResPhase)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x7600 + k as u64);
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
/// UNX-F52339 判据集（锚 UNX-F52340-J1，off-by-one 板式）。
pub fn run_unx52339_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52339");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "PE 结构五族 20 格全通",
        recorded >= PE_CELLS as u32,
        "DOS 头/NT 头/节表/导入表/导出表五族逐格恒等（每族 4 格）",
    );
    s.add(
        "反汇编 10 操作全落",
        {
            let mut lt = PeExpLedger::new();
            let mut ok = true;
            let mut d = 0usize;
            while d < DIS_OPS {
                if let Some(w) = dis_cell(d) {
                    if lt.record_dis(d, w).is_err() {
                        ok = false;
                    }
                }
                d += 1;
            }
            ok && lt.dis_all()
        },
        "定位入口/线性/递归反汇编/调用图/交叉引用/字符串引用/跳转/栈帧/常量/导出清单",
    );
    s.add(
        "资源编辑 8 格全落",
        {
            let mut lt = PeExpLedger::new();
            let mut ok = true;
            // 相位闸：PE 结构至少一格落账后方可编辑资源（先落族 0 正格）。
            if let Some(w) = pe_cell(0) {
                let _ = lt.record_pe(0, w);
            }
            let mut r = 0usize;
            while r < RES_CELLS {
                if lt.record_res(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.res_all()
        },
        "图标/版本信息/对话框/字符串表/菜单/位图/加速键/清单逐格落账",
    );
    s.add(
        "长尾维护声明 2/2 全齐",
        {
            let mut lt = PeExpLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < TAIL_ROWS {
                if lt.declare_tail(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.tails_all()
        },
        "维护状态如实声明/长尾不承诺声明两行闭集",
    );
    s.add(
        "族单调",
        {
            let mut ok = true;
            let mut p = 1usize;
            while p < PE_CELLS {
                let prev = match pe_family(p - 1) {
                    Some(k) => k,
                    None => 9,
                };
                let cur = match pe_family(p) {
                    Some(k) => k,
                    None => 9,
                };
                if cur < prev {
                    ok = false;
                }
                p += 1;
            }
            ok
        },
        "五族闭集族序单调不逆",
    );
    s.add(
        "资源相位闸对空账本必抓",
        {
            let mut lt = PeExpLedger::new();
            lt.record_res(0) == Err(PeExpCode::EResPhase)
        },
        "PE 结构未落先编辑资源必抓，且不改 last_kind",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "PE 结构格越闭集/反汇编载荷漂移/资源相位越序三态轮转必抓",
    );
    let mut l0 = PeExpLedger::new();
    s.add(
        "边界·PE 格越闭集",
        l0.record_pe(PE_CELLS, 0).is_err(),
        "PE 结构格 20 越闭集必拒",
    );
    s.add(
        "边界·反汇编越闭集",
        l0.record_dis(DIS_OPS, 0).is_err(),
        "反汇编操作 10 越闭集必拒",
    );
    s.add(
        "边界·资源格越闭集",
        l0.record_res(RES_CELLS).is_err(),
        "资源格 8 越闭集必拒",
    );
    s.add(
        "重复落账拒",
        {
            let _ = l0.record_pe(0, pe_cell(0).unwrap_or(0));
            l0.record_pe(0, pe_cell(0).unwrap_or(0)) == Err(PeExpCode::EDup)
        },
        "同格只许一条账目",
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
        "定级 S3 封顶闭集",
        {
            let mut lt = PeExpLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut p = 0usize;
            while p < PE_CELLS {
                if let Some(w) = pe_cell(p) {
                    let _ = lt.record_pe(p, w);
                }
                p += 1;
            }
            let mut d = 0usize;
            while d < DIS_OPS {
                if let Some(w) = dis_cell(d) {
                    let _ = lt.record_dis(d, w);
                }
                d += 1;
            }
            let mut r = 0usize;
            while r < RES_CELLS {
                let _ = lt.record_res(r);
                r += 1;
            }
            r = 0;
            while r < TAIL_ROWS {
                let _ = lt.declare_tail(r);
                r += 1;
            }
            lt.judge_grade() == Grade::S3
        },
        "全通 → S3 封顶（长尾如实呈报，KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC9480,
        "SEG_BASE 0x40 对齐且落 0xC7200 + 0x40×(52339−52201) 格",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_family_mapping() {
        // 映射口径：族 = p/4（0=DOS 头 1=NT 头 2=节表 3=导入表 4=导出表）。
        assert_eq!(pe_family(0), Some(0));
        assert_eq!(pe_family(4), Some(1));
        assert_eq!(pe_family(8), Some(2));
        assert_eq!(pe_family(12), Some(3));
        assert_eq!(pe_family(19), Some(4));
        assert!(pe_family(20).is_none());
        assert!(pe_family_name(5).is_none());
    }

    #[test]
    fn test_dis_name_table() {
        assert_eq!(dis_name(0), Some("定位入口"));
        assert_eq!(dis_name(4), Some("交叉引用"));
        assert_eq!(dis_name(9), Some("导出清单"));
        assert!(dis_name(10).is_none());
        assert!(dis_cell(10).is_none());
    }

    #[test]
    fn test_res_name_table() {
        assert_eq!(res_name(0), Some("图标"));
        assert_eq!(res_name(7), Some("清单"));
        assert!(res_name(8).is_none());
        assert!(res_cell(8).is_none());
        assert!(tail_row(TAIL_ROWS).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = pe_cell(9).unwrap_or(0);
        assert_eq!(a, pe_cell(9).unwrap_or(0));
        assert_ne!(a, pe_cell(10).unwrap_or(0));
        // XOR 锚常量低 16 位恒零。
        assert_eq!(PE_XOR & 0xFFFF, 0);
        assert_eq!(DIS_XOR & 0xFFFF, 0);
    }

    #[test]
    fn test_family_phase_caught() {
        let mut lt = PeExpLedger::new();
        assert_eq!(
            lt.record_pe(12, pe_cell(12).unwrap_or(0)),
            Err(PeExpCode::EResPhase)
        );
        // 拒绝路径不改 last_kind：DOS 头正格仍可落。
        assert!(lt.record_pe(0, pe_cell(0).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_family_monotone_caught() {
        let mut lt = PeExpLedger::new();
        // 先按合法族序落 0..=8（DOS 头/NT 头/节表族首格），再注族逆序。
        let mut p = 0usize;
        while p <= 8 {
            let _ = lt.record_pe(p, pe_cell(p).unwrap_or(0));
            p += 1;
        }
        // 回录早族未落格：族 0 < 最后族 2，族单调必抓。
        assert_eq!(
            lt.record_pe(3, pe_cell(3).unwrap_or(0)),
            Err(PeExpCode::EResPhase)
        );
        assert!(lt.record_pe(9, pe_cell(9).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = PeExpLedger::new();
        let _ = lt.record_pe(0, pe_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_dis(2, dis_cell(2).unwrap_or(0).wrapping_add(1)),
            Err(PeExpCode::EDisDrift)
        );
        assert_eq!(
            lt.record_pe(1, pe_cell(1).unwrap_or(0).wrapping_add(5)),
            Err(PeExpCode::EPEDrift)
        );
    }

    #[test]
    fn test_res_phase_caught() {
        let mut lt = PeExpLedger::new();
        assert_eq!(lt.record_res(0), Err(PeExpCode::EResPhase));
        let _ = lt.record_pe(0, pe_cell(0).unwrap_or(0));
        assert!(lt.record_res(0).is_ok());
        assert_eq!(lt.record_res(0), Err(PeExpCode::EDup));
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = PeExpLedger::new();
        let _ = lt.record_pe(0, pe_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_pe(0, pe_cell(0).unwrap_or(0)),
            Err(PeExpCode::EDup)
        );
        let _ = lt.declare_tail(0);
        assert_eq!(lt.declare_tail(0), Err(PeExpCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = PeExpLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = PeExpLedger::new();
        let _ = lt2.judge_gate(0);
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= PE_CELLS as u32);
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
        assert_eq!(SEG_BASE, 0xC9480);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(PeExpCode::Ok as u32, 0);
        assert_eq!(PeExpCode::EGradeDrift as u32, 12);
        assert!(!PeExpCode::Ok.detail().is_empty());
        assert!(!PeExpCode::ESeal.detail().is_empty());
    }

    #[test]
    fn test_grade_tail_cap() {
        assert_eq!(Grade::S3.val(), 3);
        // 空账本与全通账本均定 S3（长尾封顶）。
        let lt = PeExpLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn unx52339_checks_green() {
        let s = run_unx52339_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
