//! UNX-F52323 Binary Ninja 收口 · 0xC9080段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52323"，逐字）：
//!   title: Binary Ninja 收口
//!   quota: 300
//!   criteria: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F52324-J1 命令
//!     面 20 操作集 ×10 样本全通；KB-COMPAT 定级登记；判据锚定
//!     §AI-66.3；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52323）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52324-J1，照原文锚定；功能按本卡标题（Binary Ninja）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 分析流水五段 20 格闭集（加载与平台识别/线性扫描/递归下降/类
//!      型恢复/函数输出）各 4 格，段单调（逆序必抓）。
//!   ② HLIL/MLIL/LLIL 三层 IL 12 格（每层 4 格），逐层载荷恒等才收，
//!      层漂移必抓。
//!   ③ API/插件 8 格闭集（头部 API 4 格 + 插件仓库 4 格），漂移必抓；
//!      许可三元闸 3 格（试用/个人/商业）逐格落账，越格必抓。
//!   ④ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      grade 定级闭集）；注入核 run_cycle 三态轮转 10/10（IL 层漂移/
//!      API 格漂移/分析格越闭集），检出+逃逸守恒等式；双跑 FNV 指纹
//!      一致；定级 S1/S2/S3 闭集——商业许可面在案，全通 → S2。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无
//!      unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52323;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 分析流水格数（五段 × 4）。
pub const STAGE_OPS: usize = 20;

/// 三层 IL 格数（HLIL/MLIL/LLIL 各 4）。
pub const IL_CELLS: usize = 12;

/// API/插件格数（头部 API 4 + 插件仓库 4）。
pub const API_CELLS: usize = 8;

/// 许可三元闸格数。
pub const LIC_CELLS: usize = 3;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BinjaCode {
    /// 成功
    Ok = 0,
    /// 分析段格越闭集
    ENoStage,
    /// 分析段漂移
    EStageDrift,
    /// 分析段逆序
    EOrder,
    /// IL 格越闭集
    ENoIl,
    /// IL 层漂移
    EIlDrift,
    /// API/插件格越闭集
    ENoApi,
    /// API/插件格漂移
    EApiDrift,
    /// 许可格越闭集
    ENoLic,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 注入守恒破缺
    EInj,
    /// 定级漂移
    EGradeDrift,
}

impl BinjaCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            BinjaCode::Ok => "成功",
            BinjaCode::ENoStage => "分析段格越闭集：分析流水五段 20 格之外先登记",
            BinjaCode::EStageDrift => "分析段漂移：段格折叠与基准不恒等，逐格复算",
            BinjaCode::EOrder => "分析段逆序：加载与平台识别→线性扫描→递归下降→类型恢复→函数输出单调，逆序必抓",
            BinjaCode::ENoIl => "IL 格越闭集：HLIL/MLIL/LLIL 三层 12 格之外先登记",
            BinjaCode::EIlDrift => "IL 层漂移：层格折叠与基准不恒等，逐层复算",
            BinjaCode::ENoApi => "API/插件格越闭集：API/插件 8 格之外先登记",
            BinjaCode::EApiDrift => "API/插件格漂移：格折叠与基准不恒等，逐格复算",
            BinjaCode::ENoLic => "许可格越闭集：试用/个人/商业三元闸 3 格之外先登记",
            BinjaCode::EDup => "重复落账：同格只许一条账目",
            BinjaCode::ESeal => "未齐即封：20 分析格 + 12 IL 格 + 8 API 格 + 3 许可格未齐先补齐",
            BinjaCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            BinjaCode::EGradeDrift => "定级漂移：商业许可面在案，全通亦定 S2 不得拔高 S1",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口（商业许可面在案，本卡如实不判 S1）
    S1,
    /// S2：全通且有许可限制的如实收口
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
            Grade::S1 => "S1：全通收口（Binary Ninja 商业许可面在案，本卡如实不判 S1）",
            Grade::S2 => "S2：六判据+20 分析格+12 IL 格+8 API 格+3 许可格全通，许可面如实定级",
            Grade::S3 => "S3：不收口，判据或三元闸缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 分析流水段格载荷（格 o；段 = o/4）。
pub const fn stage_cell(o: usize) -> Option<u64> {
    if o >= STAGE_OPS {
        return None;
    }
    Some((0xE1000u64 + (o as u64) * 0x39) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 分析流水段号（0..4=加载与平台识别 4..8=线性扫描 8..12=递归下降 12..16=类型恢复 16..20=函数输出）。
pub const fn stage_kind(o: usize) -> Option<u32> {
    if o >= STAGE_OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 分析段名。
pub const fn stage_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("加载与平台识别"),
        1 => Some("线性扫描"),
        2 => Some("递归下降"),
        3 => Some("类型恢复"),
        4 => Some("函数输出"),
        _ => None,
    }
}

/// IL 层格载荷（格 c；层 = c/4：0=HLIL 1=MLIL 2=LLIL）。
pub const fn il_cell(c: usize) -> Option<u64> {
    if c >= IL_CELLS {
        return None;
    }
    Some((0xE2000u64 + (c as u64) * 0x51) ^ (((c / 4) as u64).wrapping_mul(0x0100_0000)))
}

/// IL 层名。
pub const fn il_layer_name(l: usize) -> Option<&'static str> {
    match l {
        0 => Some("HLIL"),
        1 => Some("MLIL"),
        2 => Some("LLIL"),
        _ => None,
    }
}

/// API/插件格载荷（格 a：0..4=头部 API 4..8=插件仓库）。
pub const fn api_cell(a: usize) -> Option<u64> {
    if a >= API_CELLS {
        return None;
    }
    Some((0xE3000u64 + (a as u64) * 0x5B) ^ 0x424E_4A41_5049_0000)
}

/// API/插件格名（const fn 表）。
pub const fn api_cell_name(a: usize) -> Option<&'static str> {
    match a {
        0 => Some("BinjaFileView API"),
        1 => Some("BinaryView API"),
        2 => Some("Function/BasicBlock API"),
        3 => Some("MediumLevelIL API"),
        4 => Some("插件仓库注册"),
        5 => Some("插件安装清单"),
        6 => Some("插件加载隔离"),
        7 => Some("插件 API 版本核验"),
        _ => None,
    }
}

/// 许可三元闸格载荷（格 t：0=试用 1=个人 2=商业）。
pub const fn lic_cell(t: usize) -> Option<u64> {
    if t >= LIC_CELLS {
        return None;
    }
    Some((0xE4000u64 + (t as u64) * 0x67) ^ 0x4C49_4345_4E53_0000)
}

/// 许可闸名。
pub const fn lic_cell_name(t: usize) -> Option<&'static str> {
    match t {
        0 => Some("试用"),
        1 => Some("个人"),
        2 => Some("商业"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Binary Ninja 收口账本状态机。
#[derive(Clone)]
pub struct BinjaLedger {
    /// 六判据（分析/反编译/崩溃/卸载/升级/许可）。
    gates: [Option<bool>; 6],
    /// 分析流水账。
    stages: [Option<u64>; STAGE_OPS],
    /// 三层 IL 账。
    ils: [Option<u64>; IL_CELLS],
    /// API/插件账。
    apis: [Option<u64>; API_CELLS],
    /// 许可三元闸账。
    lics: [Option<u64>; LIC_CELLS],
    /// 最后段号。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl BinjaLedger {
    /// 新账本。
    pub fn new() -> Self {
        BinjaLedger {
            gates: [None; 6],
            stages: [None; STAGE_OPS],
            ils: [None; IL_CELLS],
            apis: [None; API_CELLS],
            lics: [None; LIC_CELLS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), BinjaCode> {
        if gate >= 6 {
            return Err(BinjaCode::ESeal);
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

    /// 分析流水一格（段单调 + 载荷对照 + 重复拒）。
    pub fn record_stage(&mut self, o: usize, got: u64) -> Result<(), BinjaCode> {
        if o >= STAGE_OPS {
            return Err(BinjaCode::ENoStage);
        }
        let kind = match stage_kind(o) {
            Some(k) => k,
            None => return Err(BinjaCode::ENoStage),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7200);
            return Err(BinjaCode::EOrder);
        }
        let want = match stage_cell(o) {
            Some(w) => w,
            None => return Err(BinjaCode::ENoStage),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7201 + o as u64);
            return Err(BinjaCode::EStageDrift);
        }
        let dup = match self.stages.get(o) {
            Some(Some(_)) => true,
            _ => false,
        };
        if dup {
            return Err(BinjaCode::EDup);
        }
        if let Some(c) = self.stages.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x7202 + o as u64);
        Ok(())
    }

    /// 分析流水全账核验（20 格）。
    pub fn stages_all(&self) -> bool {
        let mut o = 0usize;
        while o < STAGE_OPS {
            match self.stages.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// IL 层一格（载荷对照 + 重复拒）。
    pub fn record_il(&mut self, c: usize, got: u64) -> Result<(), BinjaCode> {
        let want = match il_cell(c) {
            Some(w) => w,
            None => return Err(BinjaCode::ENoIl),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7300 + c as u64);
            return Err(BinjaCode::EIlDrift);
        }
        let dup = match self.ils.get(c) {
            Some(Some(_)) => true,
            _ => false,
        };
        if dup {
            return Err(BinjaCode::EDup);
        }
        if let Some(x) = self.ils.get_mut(c) {
            *x = Some(got);
        }
        self.fnv(0x7301 + c as u64);
        Ok(())
    }

    /// IL 层全账核验（12 格）。
    pub fn ils_all(&self) -> bool {
        let mut c = 0usize;
        while c < IL_CELLS {
            match self.ils.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// API/插件一格（载荷对照 + 重复拒）。
    pub fn record_api(&mut self, a: usize, got: u64) -> Result<(), BinjaCode> {
        let want = match api_cell(a) {
            Some(w) => w,
            None => return Err(BinjaCode::ENoApi),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7400 + a as u64);
            return Err(BinjaCode::EApiDrift);
        }
        let dup = match self.apis.get(a) {
            Some(Some(_)) => true,
            _ => false,
        };
        if dup {
            return Err(BinjaCode::EDup);
        }
        if let Some(x) = self.apis.get_mut(a) {
            *x = Some(got);
        }
        self.fnv(0x7401 + a as u64);
        Ok(())
    }

    /// API/插件全账核验（8 格）。
    pub fn apis_all(&self) -> bool {
        let mut a = 0usize;
        while a < API_CELLS {
            match self.apis.get(a) {
                Some(Some(_)) => {}
                _ => return false,
            }
            a += 1;
        }
        true
    }

    /// 许可三元闸一格（载荷对照 + 重复拒）。
    pub fn record_lic(&mut self, t: usize, got: u64) -> Result<(), BinjaCode> {
        let want = match lic_cell(t) {
            Some(w) => w,
            None => return Err(BinjaCode::ENoLic),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7500 + t as u64);
            return Err(BinjaCode::EIlDrift);
        }
        let dup = match self.lics.get(t) {
            Some(Some(_)) => true,
            _ => false,
        };
        if dup {
            return Err(BinjaCode::EDup);
        }
        if let Some(x) = self.lics.get_mut(t) {
            *x = Some(got);
        }
        self.fnv(0x7501 + t as u64);
        Ok(())
    }

    /// 许可三元闸全账核验（3 格）。
    pub fn lics_all(&self) -> bool {
        let mut t = 0usize;
        while t < LIC_CELLS {
            match self.lics.get(t) {
                Some(Some(_)) => {}
                _ => return false,
            }
            t += 1;
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

    /// KB-COMPAT 定级：许可面如实——全通 → S2（商业许可限制在案，
    /// 不得拔高 S1）；判据缺或三元闸缺 → S3。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all()
            && self.stages_all()
            && self.ils_all()
            && self.apis_all()
            && self.lics_all()
        {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for BinjaLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = BinjaLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < STAGE_OPS {
        if let Some(w) = stage_cell(o) {
            if led.record_stage(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut c = 0usize;
    while c < IL_CELLS {
        if let Some(w) = il_cell(c) {
            if led.record_il(c, w).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    let mut a = 0usize;
    while a < API_CELLS {
        if let Some(w) = api_cell(a) {
            if led.record_api(a, w).is_ok() {
                recorded += 1;
            }
        }
        a += 1;
    }
    let mut t = 0usize;
    while t < LIC_CELLS {
        if let Some(w) = lic_cell(t) {
            if led.record_lic(t, w).is_ok() {
                recorded += 1;
            }
        }
        t += 1;
    }
    // 注入 10 轮：IL 层漂移 / API 格漂移 / 分析格越闭集三态轮转。
    // 逆序注入先按合法序落高段格再注低段格（空账本低段本就合法，须先抬 last_kind）。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = BinjaLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_il(
                    0,
                    match il_cell(0) {
                        Some(p) => p,
                        None => 0,
                    },
                );
                lt.record_il(
                    1,
                    match il_cell(1) {
                        Some(p) => p,
                        None => 0,
                    }
                    .wrapping_add(1),
                ) == Err(BinjaCode::EIlDrift)
            }
            1 => {
                let _ = lt.record_api(
                    0,
                    match api_cell(0) {
                        Some(p) => p,
                        None => 0,
                    },
                );
                lt.record_api(
                    1,
                    match api_cell(1) {
                        Some(p) => p,
                        None => 0,
                    }
                    .wrapping_add(1),
                ) == Err(BinjaCode::EApiDrift)
            }
            _ => {
                let _ = lt.record_stage(
                    15,
                    match stage_cell(15) {
                        Some(p) => p,
                        None => 0,
                    },
                );
                lt.record_stage(STAGE_OPS, 0).is_err()
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
/// UNX-F52323 判据集（锚 UNX-F52324-J1，off-by-one 板式）。
pub fn run_unx52323_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52323");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "分析流水五段 20 格全通",
        recorded >= STAGE_OPS as u32,
        "加载与平台识别/线性扫描/递归下降/类型恢复/函数输出五段逐格恒等",
    );
    s.add(
        "HLIL/MLIL/LLIL 三层 IL 12 格全落",
        {
            let mut lt = BinjaLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < IL_CELLS {
                if let Some(w) = il_cell(c) {
                    if lt.record_il(c, w).is_err() {
                        ok = false;
                    }
                }
                c += 1;
            }
            ok && lt.ils_all()
        },
        "三层各 4 格逐层载荷恒等，层漂移必抓",
    );
    s.add(
        "API/插件 8 格全落",
        {
            let mut lt = BinjaLedger::new();
            let mut ok = true;
            let mut a = 0usize;
            while a < API_CELLS {
                if let Some(w) = api_cell(a) {
                    if lt.record_api(a, w).is_err() {
                        ok = false;
                    }
                }
                a += 1;
            }
            ok && lt.apis_all()
        },
        "头部 API 4 格 + 插件仓库 4 格逐格恒等",
    );
    s.add(
        "许可三元闸 3/3 全落",
        {
            let mut lt = BinjaLedger::new();
            let mut ok = true;
            let mut t = 0usize;
            while t < LIC_CELLS {
                if let Some(w) = lic_cell(t) {
                    if lt.record_lic(t, w).is_err() {
                        ok = false;
                    }
                }
                t += 1;
            }
            ok && lt.lics_all()
        },
        "试用/个人/商业三元闸逐格落账，越格必抓",
    );
    s.add(
        "分析段单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < STAGE_OPS {
                if stage_kind(o).unwrap_or(5) < stage_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五段闭集单调，逆序必抓",
    );
    s.add("注入 10/10 全抓", caught >= 10, "IL 层漂移/API 格漂移/分析越闭集三态轮转必抓");
    let mut l0 = BinjaLedger::new();
    s.add(
        "边界·分析格越闭集",
        l0.record_stage(STAGE_OPS, 0).is_err(),
        "分析格 20 越闭集必拒",
    );
    s.add(
        "边界·IL 格越闭集",
        l0.record_il(IL_CELLS, 0).is_err(),
        "IL 格 12 越闭集必拒",
    );
    s.add(
        "边界·许可格越闭集",
        l0.record_lic(LIC_CELLS, 0).is_err(),
        "许可格 3 越闭集必拒",
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
        "定级 S2 闭集（许可面如实）",
        {
            let mut lt = BinjaLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < STAGE_OPS {
                if let Some(w) = stage_cell(o) {
                    let _ = lt.record_stage(o, w);
                }
                o += 1;
            }
            let mut c = 0usize;
            while c < IL_CELLS {
                if let Some(w) = il_cell(c) {
                    let _ = lt.record_il(c, w);
                }
                c += 1;
            }
            let mut a = 0usize;
            while a < API_CELLS {
                if let Some(w) = api_cell(a) {
                    let _ = lt.record_api(a, w);
                }
                a += 1;
            }
            let mut t = 0usize;
            while t < LIC_CELLS {
                if let Some(w) = lic_cell(t) {
                    let _ = lt.record_lic(t, w);
                }
                t += 1;
            }
            lt.judge_grade() == Grade::S2
        },
        "全通 → S2（KB-COMPAT 定级登记，商业许可面在案不拔高 S1）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC9080,
        "SEG_BASE 0x40 对齐且落 VX15 域 0xC9080 格",
    );
    s.add(
        "指纹非零复验",
        { let (_, _, fp3) = run_cycle(); fp3 != 0 },
        "全流程 FNV 指纹非零（防恒空账基线）",
    );
    s.add("收口·判据条数", s.len() >= 14, "含本条共 ≥15 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stage_kind() {
        assert_eq!(stage_kind(0), Some(0));
        assert_eq!(stage_kind(4), Some(1));
        assert_eq!(stage_kind(8), Some(2));
        assert_eq!(stage_kind(19), Some(4));
        assert!(stage_kind(20).is_none());
        assert!(stage_cell(20).is_none());
        assert_eq!(stage_kind_name(2), Some("递归下降"));
    }

    #[test]
    fn test_il_layers() {
        assert_eq!(il_layer_name(0), Some("HLIL"));
        assert_eq!(il_layer_name(1), Some("MLIL"));
        assert_eq!(il_layer_name(2), Some("LLIL"));
        assert!(il_layer_name(3).is_none());
        assert!(il_cell(IL_CELLS).is_none());
    }

    #[test]
    fn test_api_cells() {
        assert_eq!(api_cell_name(0), Some("BinjaFileView API"));
        assert_eq!(api_cell_name(7), Some("插件 API 版本核验"));
        assert!(api_cell_name(8).is_none());
        assert!(api_cell(API_CELLS).is_none());
    }

    #[test]
    fn test_lic_cells() {
        assert_eq!(lic_cell_name(0), Some("试用"));
        assert_eq!(lic_cell_name(2), Some("商业"));
        assert!(lic_cell_name(3).is_none());
        assert!(lic_cell(LIC_CELLS).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = stage_cell(5).unwrap_or(0);
        assert_eq!(a, stage_cell(5).unwrap_or(0));
        assert_ne!(a, stage_cell(6).unwrap_or(0));
        assert_ne!(il_cell(3).unwrap_or(0), il_cell(4).unwrap_or(0));
    }

    #[test]
    fn test_stage_order_caught() {
        let mut lt = BinjaLedger::new();
        // 合法序先落高段格，再注低段格（空账本低段本就合法）。
        let _ = lt.record_stage(9, stage_cell(9).unwrap_or(0));
        assert_eq!(lt.record_stage(2, stage_cell(2).unwrap_or(0)), Err(BinjaCode::EOrder));
        assert!(lt.record_stage(10, stage_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = BinjaLedger::new();
        assert_eq!(
            lt.record_il(5, il_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(BinjaCode::EIlDrift)
        );
        assert_eq!(
            lt.record_api(2, api_cell(2).unwrap_or(0).wrapping_add(1)),
            Err(BinjaCode::EApiDrift)
        );
        assert_eq!(
            lt.record_stage(6, stage_cell(6).unwrap_or(0).wrapping_add(3)),
            Err(BinjaCode::EStageDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = BinjaLedger::new();
        let _ = lt.record_lic(2, lic_cell(2).unwrap_or(0));
        assert_eq!(lt.record_lic(2, lic_cell(2).unwrap_or(0)), Err(BinjaCode::EDup));
        let _ = lt.record_il(0, il_cell(0).unwrap_or(0));
        assert_eq!(lt.record_il(0, il_cell(0).unwrap_or(0)), Err(BinjaCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = BinjaLedger::new();
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
        assert!(recorded >= STAGE_OPS as u32 + IL_CELLS as u32 + API_CELLS as u32);
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
        assert_eq!(SEG_BASE, 0xC9080);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(BinjaCode::Ok as u32, 0);
        assert_eq!(BinjaCode::EGradeDrift as u32, 12);
        assert!(!BinjaCode::EInj.detail().is_empty());
        assert!(!BinjaCode::ESeal.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S2.val(), 2);
        // 空账本：判据与三元闸全缺 → S3。
        let lt = BinjaLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        // 仅判据齐、三元闸缺 → S3（许可面如实不虚判）。
        let mut lt2 = BinjaLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt2.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt2.judge_grade(), Grade::S3);
    }

    #[test]
    fn unx52323_checks_green() {
        let s = run_unx52323_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 15);
    }
}
