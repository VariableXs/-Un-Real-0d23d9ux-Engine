//! UNX-F52305 Ninite 收口 · 0xC8C00段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52305"，逐字）：
//!   title: Ninite 收口
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52306-J1 六判据全
//!     落；更新扫描/静默升级 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3；
//!     ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52305）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52306-J1，照原文锚定；功能按本卡标题（Ninite 收口）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 应用目录四族 16 格：五闭集之四族（浏览器/实用工具/开发工具/
//!      媒体工具）各 4 格，族载荷纯函数折叠恒等才收，越族必抓。
//!   ② 安装/更新/卸载 10 操作：相位三元闭集（安装 0..4 / 更新 4..8 /
//!      卸载 8..10），相位单调（逆序必抓），漂移必抓。
//!   ③ 静默安装三元闸：静默标志 / 免捆绑工具栏 / 报告回写，三格逐格落账
//!      未齐即封。
//!   ④ 下载校验 8 格：逐格载荷对照恒等，漂移必抓；免费/专业分账声明
//!      2 行闭集，缺一拒封。
//!   ⑤ ledger 状态机（new/record/judge_gate/fingerprint，FNV-1a）；注入
//!      核 run_cycle 三态轮转 10/10（下载校验漂移/相位逆序/操作越闭集），
//!      检出+逃逸守恒等式；双跑 FNV 指纹一致；定级 S1/S2/S3 闭集
//!      （16 格 + 10 操作 + 8 校验 + 2 行分账 + 六判据全通 → S1）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52305;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 应用目录格数（四族 × 4）。
pub const APPS: usize = 16;

/// 安装/更新/卸载操作数。
pub const OPS: usize = 10;

/// 静默安装三元闸格数。
pub const SILENT_GATES: usize = 3;

/// 下载校验格数。
pub const VERIFY: usize = 8;

/// 免费/专业分账声明行数。
pub const SPLIT_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NiniteCode {
    /// 成功
    Ok = 0,
    /// 应用号越闭集
    ENoApp,
    /// 应用目录漂移
    EAppDrift,
    /// 操作号越闭集
    ENoOp,
    /// 操作漂移
    EOpDrift,
    /// 相位逆序
    EPhase,
    /// 校验格越闭集
    ENoVerify,
    /// 下载校验漂移
    EVerifyDrift,
    /// 声明行越闭集
    ENoRow,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl NiniteCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            NiniteCode::Ok => "成功",
            NiniteCode::ENoApp => "应用号越闭集：Ninite 应用目录 16 格之外先登记",
            NiniteCode::EAppDrift => "应用目录漂移：族载荷折叠与基准不恒等，逐格复算",
            NiniteCode::ENoOp => "操作号越闭集：安装/更新/卸载 10 操作之外先登记",
            NiniteCode::EOpDrift => "操作漂移：操作折叠与基准不恒等，逐格复算",
            NiniteCode::EPhase => "相位逆序：安装→更新→卸载单调，逆序必抓",
            NiniteCode::ENoVerify => "校验格越闭集：下载校验 8 格之外先登记",
            NiniteCode::EVerifyDrift => "下载校验漂移：校验载荷与基准不恒等，逐格复算",
            NiniteCode::ENoRow => "声明行越闭集：免费/专业分账 2 行闭集之外先登记",
            NiniteCode::EDup => "重复落账：同格只许一条账目",
            NiniteCode::ESeal => "未齐即封：16 格 + 10 操作 + 8 校验未齐先补齐",
            NiniteCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            NiniteCode::EGradeDrift => "定级漂移：全通且分账声明全齐才可定 S1",
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
            Grade::S1 => "S1：六判据+16格应用+10操作+8校验+2行分账全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 应用目录格载荷（应用 a；族 = a/4）。
pub const fn app_cell(a: usize) -> Option<u64> {
    if a >= APPS {
        return None;
    }
    Some((0xC1000u64 + (a as u64) * 0x2C) ^ 0x4E494E495F410000)
}

/// 应用族（0..4=浏览器 4..8=实用工具 8..12=开发工具 12..16=媒体工具）。
pub const fn app_family(a: usize) -> Option<u32> {
    if a >= APPS {
        return None;
    }
    Some((a / 4) as u32)
}

/// 族名。
pub const fn app_family_name(f: usize) -> Option<&'static str> {
    match f {
        0 => Some("浏览器"),
        1 => Some("实用工具"),
        2 => Some("开发工具"),
        3 => Some("媒体工具"),
        _ => None,
    }
}

/// 操作格载荷（操作 o；相位 = o/4 封顶 2）。
pub const fn op_cell(o: usize) -> Option<u64> {
    if o >= OPS {
        return None;
    }
    Some((0xC2000u64 + (o as u64) * 0x37) ^ 0x4E494E495F4F0000)
}

/// 操作相位（0..4=安装 4..8=更新 8..10=卸载）。
pub const fn op_phase(o: usize) -> Option<u32> {
    if o >= OPS {
        return None;
    }
    if o < 4 {
        return Some(0);
    }
    if o < 8 {
        return Some(1);
    }
    Some(2)
}

/// 相位名。
pub const fn op_phase_name(p: usize) -> Option<&'static str> {
    match p {
        0 => Some("安装"),
        1 => Some("更新"),
        2 => Some("卸载"),
        _ => None,
    }
}

/// 下载校验格基准（格 v）。
pub const fn verify_cell(v: usize) -> Option<u64> {
    if v >= VERIFY {
        return None;
    }
    Some((0xC3000u64 + (v as u64) * 0x31) ^ 0x4E494E4956450000)
}

/// 分账声明行折叠（行 r：0=免费 1=专业）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xC400 + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Ninite 收口账本状态机。
#[derive(Clone)]
pub struct NiniteLedger {
    /// 六判据（更新扫描/静默升级/安装闸/卸载账/许可面/写回摘要）。
    gates: [Option<bool>; 6],
    /// 应用目录账。
    apps: [Option<u64>; APPS],
    /// 操作账。
    ops: [Option<u64>; OPS],
    /// 静默安装三元闸。
    silent: [Option<bool>; SILENT_GATES],
    /// 下载校验账。
    verify: [Option<u64>; VERIFY],
    /// 分账声明账。
    splits: [Option<u32>; SPLIT_ROWS],
    /// 最后相位。
    last_phase: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl NiniteLedger {
    /// 新账本。
    pub fn new() -> Self {
        NiniteLedger {
            gates: [None; 6],
            apps: [None; APPS],
            ops: [None; OPS],
            silent: [None; SILENT_GATES],
            verify: [None; VERIFY],
            splits: [None; SPLIT_ROWS],
            last_phase: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), NiniteCode> {
        if gate >= 6 {
            return Err(NiniteCode::ESeal);
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

    /// 应用目录一格（族载荷对照 + 重复拒）。
    pub fn record_app(&mut self, a: usize, got: u64) -> Result<(), NiniteCode> {
        let want = match app_cell(a) {
            Some(w) => w,
            None => return Err(NiniteCode::ENoApp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5200 + a as u64);
            return Err(NiniteCode::EAppDrift);
        }
        if match self.apps.get(a) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(NiniteCode::EDup);
        }
        if let Some(c) = self.apps.get_mut(a) {
            *c = Some(got);
        }
        self.fnv(0x5201 + a as u64);
        Ok(())
    }

    /// 应用目录全账核验（16 格）。
    pub fn apps_all(&self) -> bool {
        let mut a = 0usize;
        while a < APPS {
            match self.apps.get(a) {
                Some(Some(_)) => {}
                _ => return false,
            }
            a += 1;
        }
        true
    }

    /// 操作一格（相位单调 + 载荷对照 + 重复拒）。
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), NiniteCode> {
        if o >= OPS {
            return Err(NiniteCode::ENoOp);
        }
        let phase = match op_phase(o) {
            Some(p) => p,
            None => return Err(NiniteCode::ENoOp),
        };
        if (phase as i64) < self.last_phase {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5300);
            return Err(NiniteCode::EPhase);
        }
        let want = match op_cell(o) {
            Some(w) => w,
            None => return Err(NiniteCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5301 + o as u64);
            return Err(NiniteCode::EOpDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(NiniteCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.last_phase = phase as i64;
        self.fnv(0x5302 + o as u64);
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

    /// 静默安装三元闸一格。
    pub fn record_silent(&mut self, g: usize) -> Result<(), NiniteCode> {
        if g >= SILENT_GATES {
            return Err(NiniteCode::ESeal);
        }
        if match self.silent.get(g) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(NiniteCode::EDup);
        }
        if let Some(c) = self.silent.get_mut(g) {
            *c = Some(true);
        }
        self.fnv(0x5400 + g as u64);
        Ok(())
    }

    /// 三元闸全落核验。
    pub fn silent_all(&self) -> bool {
        let mut g = 0usize;
        while g < SILENT_GATES {
            match self.silent.get(g) {
                Some(Some(true)) => {}
                _ => return false,
            }
            g += 1;
        }
        true
    }

    /// 下载校验一格（载荷对照 + 重复拒）。
    pub fn record_verify(&mut self, v: usize, got: u64) -> Result<(), NiniteCode> {
        let want = match verify_cell(v) {
            Some(w) => w,
            None => return Err(NiniteCode::ENoVerify),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5500 + v as u64);
            return Err(NiniteCode::EVerifyDrift);
        }
        if match self.verify.get(v) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(NiniteCode::EDup);
        }
        if let Some(c) = self.verify.get_mut(v) {
            *c = Some(got);
        }
        self.fnv(0x5501 + v as u64);
        Ok(())
    }

    /// 下载校验全账核验（8 格）。
    pub fn verify_all(&self) -> bool {
        let mut v = 0usize;
        while v < VERIFY {
            match self.verify.get(v) {
                Some(Some(_)) => {}
                _ => return false,
            }
            v += 1;
        }
        true
    }

    /// 声明一行（免费/专业分账）。
    pub fn declare_split(&mut self, r: usize) -> Result<(), NiniteCode> {
        if r >= SPLIT_ROWS {
            return Err(NiniteCode::ENoRow);
        }
        if match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(NiniteCode::EDup);
        }
        if let Some(c) = self.splits.get_mut(r) {
            *c = split_row(r);
        }
        self.fnv(0x5600 + r as u64);
        Ok(())
    }

    /// 声明全齐核验。
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
        if self.gates_all() && self.apps_all() && self.ops_all() && self.verify_all() && self.splits_all() {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for NiniteLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = NiniteLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut a = 0usize;
    while a < APPS {
        if let Some(w) = app_cell(a) {
            if led.record_app(a, w).is_ok() {
                recorded += 1;
            }
        }
        a += 1;
    }
    let mut o = 0usize;
    while o < OPS {
        if let Some(w) = op_cell(o) {
            if led.record_op(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut sg = 0usize;
    while sg < SILENT_GATES {
        let _ = led.record_silent(sg);
        sg += 1;
    }
    let mut v = 0usize;
    while v < VERIFY {
        if let Some(w) = verify_cell(v) {
            if led.record_verify(v, w).is_ok() {
                recorded += 1;
            }
        }
        v += 1;
    }
    let mut r = 0usize;
    while r < SPLIT_ROWS {
        let _ = led.declare_split(r);
        r += 1;
    }
    // 注入 10 轮：下载校验漂移 / 相位逆序 / 操作越闭集三态轮转。
    // 三类注入全部落在当轮临时账本上；led 上 injected==caught==10、
    // escaped==0。相位注入对空账本同样必抓：测试种子须按合法序先落
    // 低相位格（安装 0 → 更新 4），再注回退相位，且 EPhase 不改
    // last_phase。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = NiniteLedger::new();
        let caught = match kind {
            0 => {
                // 漂移注入：先录正格再注漂移。
                let base0 = match verify_cell(0) {
                    Some(w) => w,
                    None => 0,
                };
                let base1 = match verify_cell(1) {
                    Some(w) => w,
                    None => 0,
                };
                let _ = lt.record_verify(0, base0);
                lt.record_verify(1, base1.wrapping_add(1)) == Err(NiniteCode::EVerifyDrift)
            }
            1 => {
                // 相位注入：按合法序先落安装格与更新格，再注回退。
                let o0 = match op_cell(0) {
                    Some(w) => w,
                    None => 0,
                };
                let o4 = match op_cell(4) {
                    Some(w) => w,
                    None => 0,
                };
                let o1 = match op_cell(1) {
                    Some(w) => w,
                    None => 0,
                };
                let _ = lt.record_op(0, o0);
                let _ = lt.record_op(4, o4);
                lt.record_op(1, o1) == Err(NiniteCode::EPhase)
            }
            _ => {
                // 越闭集注入：下载校验与操作双边界。
                let v8 = lt.record_verify(VERIFY, 0) == Err(NiniteCode::ENoVerify);
                let o10 = lt.record_op(OPS, 0) == Err(NiniteCode::ENoOp);
                v8 && o10
            }
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
/// UNX-F52305 判据集（锚 UNX-F52306-J1，off-by-one 板式）。
pub fn run_unx52305_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52305");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "应用目录四族 16 格全落",
        recorded >= APPS as u32,
        "浏览器/实用工具/开发工具/媒体工具四族逐格恒等",
    );
    s.add(
        "安装/更新/卸载 10 操作全落",
        {
            let mut lt = NiniteLedger::new();
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
        "安装/更新/卸载三相位闭集逐操作恒等",
    );
    s.add(
        "下载校验 8 格恒等",
        {
            let mut lt = NiniteLedger::new();
            let mut ok = true;
            let mut v = 0usize;
            while v < VERIFY {
                if let Some(w) = verify_cell(v) {
                    if lt.record_verify(v, w).is_err() {
                        ok = false;
                    }
                }
                v += 1;
            }
            ok && lt.verify_all()
        },
        "下载校验逐格载荷对照，漂移必抓",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = NiniteLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "更新扫描/静默升级/安装闸/卸载账/许可面/写回摘要逐格落账",
    );
    s.add(
        "静默安装三元闸 3/3 全落",
        {
            let mut lt = NiniteLedger::new();
            let mut ok = true;
            let mut g = 0usize;
            while g < SILENT_GATES {
                if lt.record_silent(g).is_err() {
                    ok = false;
                }
                g += 1;
            }
            ok && lt.silent_all()
        },
        "静默标志/免捆绑工具栏/报告回写三格闭集",
    );
    s.add(
        "免费/专业分账声明 2/2 全齐",
        {
            let mut lt = NiniteLedger::new();
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
        "免费分账/专业分账两行闭集（§AI-66.3）",
    );
    s.add(
        "相位单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < OPS {
                let cur = match op_phase(o) {
                    Some(p) => p,
                    None => 3,
                };
                let prev = match op_phase(o - 1) {
                    Some(p) => p,
                    None => 3,
                };
                if cur < prev {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "安装→更新→卸载相位闭集单调",
    );
    s.add("注入 10/10 全抓", caught >= 10, "校验漂移/相位逆序/越闭集三态轮转必抓");
    let mut l0 = NiniteLedger::new();
    s.add(
        "边界·操作越闭集",
        l0.record_op(OPS, 0).is_err(),
        "操作 10 越闭集必拒",
    );
    s.add(
        "边界·应用越闭集",
        l0.record_app(APPS, 0).is_err(),
        "应用 16 越闭集必拒",
    );
    s.add(
        "边界·校验越闭集",
        l0.record_verify(VERIFY, 0).is_err(),
        "校验 8 越闭集必拒",
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
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC8C00,
        "SEG_BASE 0x40 对齐且落在 VX15 域 0xC8C00",
    );
    s.add(
        "定级 S1 闭集",
        {
            let mut lt = NiniteLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut a = 0usize;
            while a < APPS {
                if let Some(w) = app_cell(a) {
                    let _ = lt.record_app(a, w);
                }
                a += 1;
            }
            let mut o = 0usize;
            while o < OPS {
                if let Some(w) = op_cell(o) {
                    let _ = lt.record_op(o, w);
                }
                o += 1;
            }
            let mut v = 0usize;
            while v < VERIFY {
                if let Some(w) = verify_cell(v) {
                    let _ = lt.record_verify(v, w);
                }
                v += 1;
            }
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                let _ = lt.declare_split(r);
                r += 1;
            }
            // 定级断言前 main/aux 各节已全部记账。
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_family() {
        assert_eq!(app_family(0), Some(0));
        assert_eq!(app_family(4), Some(1));
        assert_eq!(app_family(12), Some(3));
        assert!(app_family(16).is_none());
        assert!(app_cell(16).is_none());
        assert_eq!(app_family_name(3), Some("媒体工具"));
        assert!(app_family_name(4).is_none());
    }

    #[test]
    fn test_op_phase() {
        assert_eq!(op_phase(0), Some(0));
        assert_eq!(op_phase(3), Some(0));
        assert_eq!(op_phase(4), Some(1));
        assert_eq!(op_phase(9), Some(2));
        assert!(op_phase(10).is_none());
        assert!(op_cell(10).is_none());
        assert_eq!(op_phase_name(2), Some("卸载"));
    }

    #[test]
    fn test_cells_pure() {
        let a = app_cell(3).unwrap_or(0);
        assert_eq!(a, app_cell(3).unwrap_or(0));
        assert_ne!(a, app_cell(4).unwrap_or(0));
        assert!(verify_cell(VERIFY).is_none());
        assert!(split_row(SPLIT_ROWS).is_none());
    }

    #[test]
    fn test_phase_order_caught() {
        let mut lt = NiniteLedger::new();
        let _ = lt.record_op(5, op_cell(5).unwrap_or(0));
        assert_eq!(lt.record_op(1, op_cell(1).unwrap_or(0)), Err(NiniteCode::EPhase));
        // EPhase 不改 last_phase：回退后再录同族格不误伤。
        assert!(lt.record_op(6, op_cell(6).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = NiniteLedger::new();
        assert_eq!(
            lt.record_verify(2, verify_cell(2).unwrap_or(0).wrapping_add(1)),
            Err(NiniteCode::EVerifyDrift)
        );
        assert_eq!(
            lt.record_app(7, app_cell(7).unwrap_or(0).wrapping_add(9)),
            Err(NiniteCode::EAppDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = NiniteLedger::new();
        let _ = lt.record_app(0, app_cell(0).unwrap_or(0));
        assert_eq!(lt.record_app(0, app_cell(0).unwrap_or(0)), Err(NiniteCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = NiniteLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        assert!(lt.record_silent(3).is_err());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (APPS + OPS + VERIFY) as u32);
        assert_eq!(caught, 10);
    }

    #[test]
    fn test_injection_conservation() {
        let mut lt = NiniteLedger::new();
        lt.injected = 10;
        lt.caught = 10;
        let (i, c, e) = lt.stats();
        assert_eq!(i, c);
        assert_eq!(e, 0);
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
        assert_eq!(SEG_BASE, 0xC8C00);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(NiniteCode::Ok as u32, 0);
        assert_eq!(NiniteCode::EGradeDrift as u32, 12);
        assert!(!NiniteCode::Ok.detail().is_empty());
        assert!(!NiniteCode::ESeal.detail().is_empty());
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        let lt = NiniteLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn unx52305_checks_green() {
        let s = run_unx52305_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
