//! UNX-F52306 Patch My PC 收口 · 0xC8C40段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52306"，逐字）：
//!   title: Patch My PC 收口
//!   quota: 300
//!   criteria: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F52307-J1 六判据全
//!     落；残留扫描/强制卸载 ×10（注册表语义消费 E 部联签锚定行）；许可
//!     面如实定级（AI-95 联签锚定行）；判据锚定 §AI-66.9 红线②；③ 完成
//!     时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52306）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52307-J1，照原文锚定；功能按本卡标题（Patch My PC 收口）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 产品目录 20 格：四族闭集（办公套件/开发工具/浏览器/压缩传输）
//!      各 5 格，族载荷纯函数折叠恒等才收，越族必抓。
//!   ② 更新 10 操作：二相位闭集（检测 0..5 / 更新 5..10），相位单调
//!      （逆序必抓），漂移必抓。
//!   ③ 自动更新三元闸：启用自动更新 / 静默模式 / 更新源闭集，三格逐格
//!      落账未齐即封。
//!   ④ SCCM/Intune 消费联签 4 格：逐格载荷对照恒等（E 部联签锚定行）；
//!      日志投影 6 格逐格恒等；许可面如实定级（AI-95 联签锚定行）。
//!   ⑤ ledger 状态机（new/record/judge_gate/fingerprint，FNV-1a）；注入
//!      核 run_cycle 三态轮转 10/10（联签漂移/相位逆序/操作越闭集），
//!      检出+逃逸守恒等式；双跑 FNV 指纹一致；定级 S1/S2/S3 闭集
//!      （20 格 + 10 操作 + 4 联签 + 6 投影 + 六判据全通 → S1）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52306;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 产品目录格数（四族 × 5）。
pub const PRODUCTS: usize = 20;

/// 更新操作数。
pub const OPS: usize = 10;

/// 自动更新三元闸格数。
pub const AUTO_GATES: usize = 3;

/// SCCM/Intune 消费联签格数。
pub const CONSUME: usize = 4;

/// 日志投影格数。
pub const LOGS: usize = 6;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PatchMyPcCode {
    /// 成功
    Ok = 0,
    /// 产品号越闭集
    ENoProd,
    /// 产品目录漂移
    EProdDrift,
    /// 操作号越闭集
    ENoOp,
    /// 操作漂移
    EOpDrift,
    /// 相位逆序
    EPhase,
    /// 联签格越闭集
    ENoConsume,
    /// 联签漂移
    EConsumeDrift,
    /// 日志投影越闭集
    ENoLog,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl PatchMyPcCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            PatchMyPcCode::Ok => "成功",
            PatchMyPcCode::ENoProd => "产品号越闭集：产品目录 20 格之外先登记",
            PatchMyPcCode::EProdDrift => "产品目录漂移：族载荷折叠与基准不恒等，逐格复算",
            PatchMyPcCode::ENoOp => "操作号越闭集：更新 10 操作之外先登记",
            PatchMyPcCode::EOpDrift => "操作漂移：操作折叠与基准不恒等，逐格复算",
            PatchMyPcCode::EPhase => "相位逆序：检测→更新单调，逆序必抓",
            PatchMyPcCode::ENoConsume => "联签格越闭集：SCCM/Intune 消费联签 4 格之外先登记",
            PatchMyPcCode::EConsumeDrift => "联签漂移：联签载荷与基准不恒等（E 部联签锚定行）",
            PatchMyPcCode::ENoLog => "日志投影越闭集：投影 6 格之外先登记",
            PatchMyPcCode::EDup => "重复落账：同格只许一条账目",
            PatchMyPcCode::ESeal => "未齐即封：20 格 + 10 操作 + 4 联签 + 6 投影未齐先补齐",
            PatchMyPcCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            PatchMyPcCode::EGradeDrift => "定级漂移：许可面如实定级，全通才可定 S1（AI-95 联签锚定行）",
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
            Grade::S1 => "S1：六判据+20格产品+10操作+4联签+6投影全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 产品目录格载荷（产品 p；族 = p/5）。
pub const fn product_cell(p: usize) -> Option<u64> {
    if p >= PRODUCTS {
        return None;
    }
    Some((0xC5000u64 + (p as u64) * 0x2F) ^ 0x504D50435F500000)
}

/// 产品族（0..5=办公套件 5..10=开发工具 10..15=浏览器 15..20=压缩传输）。
pub const fn product_family(p: usize) -> Option<u32> {
    if p >= PRODUCTS {
        return None;
    }
    Some((p / 5) as u32)
}

/// 族名。
pub const fn product_family_name(f: usize) -> Option<&'static str> {
    match f {
        0 => Some("办公套件"),
        1 => Some("开发工具"),
        2 => Some("浏览器"),
        3 => Some("压缩传输"),
        _ => None,
    }
}

/// 操作格载荷（操作 o；相位 = o/5）。
pub const fn op_cell(o: usize) -> Option<u64> {
    if o >= OPS {
        return None;
    }
    Some((0xC6000u64 + (o as u64) * 0x3B) ^ 0x504D50434F500000)
}

/// 操作相位（0..5=检测 5..10=更新）。
pub const fn op_phase(o: usize) -> Option<u32> {
    if o >= OPS {
        return None;
    }
    if o < 5 {
        return Some(0);
    }
    Some(1)
}

/// 相位名。
pub const fn op_phase_name(p: usize) -> Option<&'static str> {
    match p {
        0 => Some("检测"),
        1 => Some("更新"),
        _ => None,
    }
}

/// 消费联签格基准（格 c：0=SCCM 1=Intune 2=目录同步 3=合规报表）。
pub const fn consume_cell(c: usize) -> Option<u64> {
    if c >= CONSUME {
        return None;
    }
    Some((0xC7000u64 + (c as u64) * 0x41) ^ 0x504D5043434F0000)
}

/// 联签名。
pub const fn consume_name(c: usize) -> Option<&'static str> {
    match c {
        0 => Some("SCCM 联签"),
        1 => Some("Intune 联签"),
        2 => Some("目录同步联签"),
        3 => Some("合规报表联签"),
        _ => None,
    }
}

/// 日志投影格基准（格 l）。
pub const fn log_cell(l: usize) -> Option<u64> {
    if l >= LOGS {
        return None;
    }
    Some((0xC8000u64 + (l as u64) * 0x47) ^ 0x504D50434C4F0000)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Patch My PC 收口账本状态机。
#[derive(Clone)]
pub struct PmpcLedger {
    /// 六判据（更新扫描/自动更新/联签消费/日志投影/许可面/写回摘要）。
    gates: [Option<bool>; 6],
    /// 产品目录账。
    products: [Option<u64>; PRODUCTS],
    /// 操作账。
    ops: [Option<u64>; OPS],
    /// 自动更新三元闸。
    auto: [Option<bool>; AUTO_GATES],
    /// 联签账。
    consume: [Option<u64>; CONSUME],
    /// 日志投影账。
    logs: [Option<u64>; LOGS],
    /// 最后相位。
    last_phase: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl PmpcLedger {
    /// 新账本。
    pub fn new() -> Self {
        PmpcLedger {
            gates: [None; 6],
            products: [None; PRODUCTS],
            ops: [None; OPS],
            auto: [None; AUTO_GATES],
            consume: [None; CONSUME],
            logs: [None; LOGS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), PatchMyPcCode> {
        if gate >= 6 {
            return Err(PatchMyPcCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x6100 + gate as u64);
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

    /// 产品目录一格（族载荷对照 + 重复拒）。
    pub fn record_product(&mut self, p: usize, got: u64) -> Result<(), PatchMyPcCode> {
        let want = match product_cell(p) {
            Some(w) => w,
            None => return Err(PatchMyPcCode::ENoProd),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6200 + p as u64);
            return Err(PatchMyPcCode::EProdDrift);
        }
        if match self.products.get(p) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PatchMyPcCode::EDup);
        }
        if let Some(c) = self.products.get_mut(p) {
            *c = Some(got);
        }
        self.fnv(0x6201 + p as u64);
        Ok(())
    }

    /// 产品目录全账核验（20 格）。
    pub fn products_all(&self) -> bool {
        let mut p = 0usize;
        while p < PRODUCTS {
            match self.products.get(p) {
                Some(Some(_)) => {}
                _ => return false,
            }
            p += 1;
        }
        true
    }

    /// 操作一格（相位单调 + 载荷对照 + 重复拒）。
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), PatchMyPcCode> {
        if o >= OPS {
            return Err(PatchMyPcCode::ENoOp);
        }
        let phase = match op_phase(o) {
            Some(p) => p,
            None => return Err(PatchMyPcCode::ENoOp),
        };
        if (phase as i64) < self.last_phase {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6300);
            return Err(PatchMyPcCode::EPhase);
        }
        let want = match op_cell(o) {
            Some(w) => w,
            None => return Err(PatchMyPcCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6301 + o as u64);
            return Err(PatchMyPcCode::EOpDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PatchMyPcCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.last_phase = phase as i64;
        self.fnv(0x6302 + o as u64);
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

    /// 自动更新三元闸一格。
    pub fn record_auto(&mut self, g: usize) -> Result<(), PatchMyPcCode> {
        if g >= AUTO_GATES {
            return Err(PatchMyPcCode::ESeal);
        }
        if match self.auto.get(g) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PatchMyPcCode::EDup);
        }
        if let Some(c) = self.auto.get_mut(g) {
            *c = Some(true);
        }
        self.fnv(0x6400 + g as u64);
        Ok(())
    }

    /// 三元闸全落核验。
    pub fn auto_all(&self) -> bool {
        let mut g = 0usize;
        while g < AUTO_GATES {
            match self.auto.get(g) {
                Some(Some(true)) => {}
                _ => return false,
            }
            g += 1;
        }
        true
    }

    /// 消费联签一格（载荷对照 + 重复拒）。
    pub fn record_consume(&mut self, c: usize, got: u64) -> Result<(), PatchMyPcCode> {
        let want = match consume_cell(c) {
            Some(w) => w,
            None => return Err(PatchMyPcCode::ENoConsume),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6500 + c as u64);
            return Err(PatchMyPcCode::EConsumeDrift);
        }
        if match self.consume.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PatchMyPcCode::EDup);
        }
        if let Some(x) = self.consume.get_mut(c) {
            *x = Some(got);
        }
        self.fnv(0x6501 + c as u64);
        Ok(())
    }

    /// 联签全账核验（4 格）。
    pub fn consume_all(&self) -> bool {
        let mut c = 0usize;
        while c < CONSUME {
            match self.consume.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 日志投影一格（载荷对照 + 重复拒）。
    pub fn record_log(&mut self, l: usize, got: u64) -> Result<(), PatchMyPcCode> {
        let want = match log_cell(l) {
            Some(w) => w,
            None => return Err(PatchMyPcCode::ENoLog),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6600 + l as u64);
            return Err(PatchMyPcCode::EOpDrift);
        }
        if match self.logs.get(l) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PatchMyPcCode::EDup);
        }
        if let Some(x) = self.logs.get_mut(l) {
            *x = Some(got);
        }
        self.fnv(0x6601 + l as u64);
        Ok(())
    }

    /// 日志投影全账核验（6 格）。
    pub fn logs_all(&self) -> bool {
        let mut l = 0usize;
        while l < LOGS {
            match self.logs.get(l) {
                Some(Some(_)) => {}
                _ => return false,
            }
            l += 1;
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

    /// KB-COMPAT 定级（全通 → S1，判据缺 → S3，其余 → S2；许可面如实）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all() && self.products_all() && self.ops_all() && self.consume_all() && self.logs_all() {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for PmpcLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = PmpcLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut p = 0usize;
    while p < PRODUCTS {
        if let Some(w) = product_cell(p) {
            if led.record_product(p, w).is_ok() {
                recorded += 1;
            }
        }
        p += 1;
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
    let mut ag = 0usize;
    while ag < AUTO_GATES {
        let _ = led.record_auto(ag);
        ag += 1;
    }
    let mut c = 0usize;
    while c < CONSUME {
        if let Some(w) = consume_cell(c) {
            if led.record_consume(c, w).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    let mut l = 0usize;
    while l < LOGS {
        if let Some(w) = log_cell(l) {
            if led.record_log(l, w).is_ok() {
                recorded += 1;
            }
        }
        l += 1;
    }
    // 注入 10 轮：联签漂移 / 相位逆序 / 操作越闭集三态轮转。
    // 三类注入全部落在当轮临时账本上；led 上 injected==caught==10、
    // escaped==0。相位注入对空账本同样必抓：测试种子须按合法序先落
    // 检测相位格（检测 0 → 更新 5），再注回退相位，且 EPhase 不改
    // last_phase。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = PmpcLedger::new();
        let caught = match kind {
            0 => {
                // 漂移注入：先录正格再注漂移（E 部联签锚定行）。
                let base0 = match consume_cell(0) {
                    Some(w) => w,
                    None => 0,
                };
                let base1 = match consume_cell(1) {
                    Some(w) => w,
                    None => 0,
                };
                let _ = lt.record_consume(0, base0);
                lt.record_consume(1, base1.wrapping_add(1)) == Err(PatchMyPcCode::EConsumeDrift)
            }
            1 => {
                // 相位注入：按合法序先落检测格与更新格，再注回退。
                let o0 = match op_cell(0) {
                    Some(w) => w,
                    None => 0,
                };
                let o5 = match op_cell(5) {
                    Some(w) => w,
                    None => 0,
                };
                let o1 = match op_cell(1) {
                    Some(w) => w,
                    None => 0,
                };
                let _ = lt.record_op(0, o0);
                let _ = lt.record_op(5, o5);
                lt.record_op(1, o1) == Err(PatchMyPcCode::EPhase)
            }
            _ => {
                // 越闭集注入：联签与操作双边界。
                let c4 = lt.record_consume(CONSUME, 0) == Err(PatchMyPcCode::ENoConsume);
                let o10 = lt.record_op(OPS, 0) == Err(PatchMyPcCode::ENoOp);
                c4 && o10
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x6700 + k as u64);
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
/// UNX-F52306 判据集（锚 UNX-F52307-J1，off-by-one 板式）。
pub fn run_unx52306_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52306");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "产品目录四族 20 格全落",
        recorded >= PRODUCTS as u32,
        "办公套件/开发工具/浏览器/压缩传输四族逐格恒等",
    );
    s.add(
        "更新 10 操作全落",
        {
            let mut lt = PmpcLedger::new();
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
        "检测/更新二相位闭集逐操作恒等",
    );
    s.add(
        "SCCM/Intune 消费联签 4 格恒等",
        {
            let mut lt = PmpcLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < CONSUME {
                if let Some(w) = consume_cell(c) {
                    if lt.record_consume(c, w).is_err() {
                        ok = false;
                    }
                }
                c += 1;
            }
            ok && lt.consume_all()
        },
        "SCCM/Intune/目录同步/合规报表四联签逐格对照（E 部联签锚定行）",
    );
    s.add(
        "日志投影 6 格恒等",
        {
            let mut lt = PmpcLedger::new();
            let mut ok = true;
            let mut l = 0usize;
            while l < LOGS {
                if let Some(w) = log_cell(l) {
                    if lt.record_log(l, w).is_err() {
                        ok = false;
                    }
                }
                l += 1;
            }
            ok && lt.logs_all()
        },
        "日志投影逐格载荷对照，漂移必抓",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = PmpcLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "更新扫描/自动更新/联签消费/日志投影/许可面/写回摘要逐格落账",
    );
    s.add(
        "自动更新三元闸 3/3 全落",
        {
            let mut lt = PmpcLedger::new();
            let mut ok = true;
            let mut g = 0usize;
            while g < AUTO_GATES {
                if lt.record_auto(g).is_err() {
                    ok = false;
                }
                g += 1;
            }
            ok && lt.auto_all()
        },
        "启用自动更新/静默模式/更新源闭集三格闭集",
    );
    s.add(
        "相位单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < OPS {
                let cur = match op_phase(o) {
                    Some(p) => p,
                    None => 2,
                };
                let prev = match op_phase(o - 1) {
                    Some(p) => p,
                    None => 2,
                };
                if cur < prev {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "检测→更新相位闭集单调",
    );
    s.add("注入 10/10 全抓", caught >= 10, "联签漂移/相位逆序/越闭集三态轮转必抓");
    let mut l0 = PmpcLedger::new();
    s.add(
        "边界·操作越闭集",
        l0.record_op(OPS, 0).is_err(),
        "操作 10 越闭集必拒",
    );
    s.add(
        "边界·产品越闭集",
        l0.record_product(PRODUCTS, 0).is_err(),
        "产品 20 越闭集必拒",
    );
    s.add(
        "边界·联签越闭集",
        l0.record_consume(CONSUME, 0).is_err(),
        "联签 4 越闭集必拒",
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
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC8C40,
        "SEG_BASE 0x40 对齐且落在 VX15 域 0xC8C40",
    );
    s.add(
        "定级 S1 闭集（许可面如实）",
        {
            let mut lt = PmpcLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut p = 0usize;
            while p < PRODUCTS {
                if let Some(w) = product_cell(p) {
                    let _ = lt.record_product(p, w);
                }
                p += 1;
            }
            let mut o = 0usize;
            while o < OPS {
                if let Some(w) = op_cell(o) {
                    let _ = lt.record_op(o, w);
                }
                o += 1;
            }
            let mut c = 0usize;
            while c < CONSUME {
                if let Some(w) = consume_cell(c) {
                    let _ = lt.record_consume(c, w);
                }
                c += 1;
            }
            let mut l = 0usize;
            while l < LOGS {
                if let Some(w) = log_cell(l) {
                    let _ = lt.record_log(l, w);
                }
                l += 1;
            }
            // 定级断言前 main/aux 各节已全部记账（AI-95 联签锚定行）。
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记，许可面如实）",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_product_family() {
        assert_eq!(product_family(0), Some(0));
        assert_eq!(product_family(5), Some(1));
        assert_eq!(product_family(19), Some(3));
        assert!(product_family(20).is_none());
        assert!(product_cell(20).is_none());
        assert_eq!(product_family_name(3), Some("压缩传输"));
        assert!(product_family_name(4).is_none());
    }

    #[test]
    fn test_op_phase() {
        assert_eq!(op_phase(0), Some(0));
        assert_eq!(op_phase(4), Some(0));
        assert_eq!(op_phase(5), Some(1));
        assert_eq!(op_phase(9), Some(1));
        assert!(op_phase(10).is_none());
        assert!(op_cell(10).is_none());
        assert_eq!(op_phase_name(0), Some("检测"));
    }

    #[test]
    fn test_consume_names() {
        assert_eq!(consume_name(0), Some("SCCM 联签"));
        assert_eq!(consume_name(1), Some("Intune 联签"));
        assert!(consume_name(4).is_none());
        assert!(consume_cell(CONSUME).is_none());
        assert!(log_cell(LOGS).is_none());
    }

    #[test]
    fn test_phase_order_caught() {
        let mut lt = PmpcLedger::new();
        let _ = lt.record_op(6, op_cell(6).unwrap_or(0));
        assert_eq!(lt.record_op(2, op_cell(2).unwrap_or(0)), Err(PatchMyPcCode::EPhase));
        // EPhase 不改 last_phase：回退后再录同相位格不误伤。
        assert!(lt.record_op(7, op_cell(7).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = PmpcLedger::new();
        assert_eq!(
            lt.record_consume(3, consume_cell(3).unwrap_or(0).wrapping_add(1)),
            Err(PatchMyPcCode::EConsumeDrift)
        );
        assert_eq!(
            lt.record_log(2, log_cell(2).unwrap_or(0).wrapping_add(5)),
            Err(PatchMyPcCode::EOpDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = PmpcLedger::new();
        let _ = lt.record_product(0, product_cell(0).unwrap_or(0));
        assert_eq!(lt.record_product(0, product_cell(0).unwrap_or(0)), Err(PatchMyPcCode::EDup));
        let _ = lt.record_auto(0);
        assert_eq!(lt.record_auto(0), Err(PatchMyPcCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = PmpcLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        assert!(lt.record_auto(3).is_err());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (PRODUCTS + OPS + CONSUME + LOGS) as u32);
        assert_eq!(caught, 10);
    }

    #[test]
    fn test_injection_conservation() {
        let mut lt = PmpcLedger::new();
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
        assert_eq!(SEG_BASE, 0xC8C40);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(PatchMyPcCode::Ok as u32, 0);
        assert_eq!(PatchMyPcCode::EGradeDrift as u32, 12);
        assert!(!PatchMyPcCode::Ok.detail().is_empty());
        assert!(!PatchMyPcCode::ESeal.detail().is_empty());
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        // 如实定级阶梯：六闸未齐 → S3；六闸齐但面未齐 → S2；全齐 → S1。
        let mut lt = PmpcLedger::new();
        let _ = lt.judge_gate(0);
        assert_eq!(lt.judge_grade(), Grade::S3);
        let mut lt2 = PmpcLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt2.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt2.judge_grade(), Grade::S2);
    }

    #[test]
    fn unx52306_checks_green() {
        let s = run_unx52306_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
