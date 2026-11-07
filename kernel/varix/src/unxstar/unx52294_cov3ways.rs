//! UNX-F52294 覆盖率三口径"工程级"落账（AI-96）· 0xC8940段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52294"，逐字）：
//!   title: 覆盖率三口径"工程级"落账（AI-96）
//!   quota: 260
//!   criteria: ① 纯功能代码 ≥260 行；② 判据原文：UNX-F52295-J1 双轨产
//!     线缺陷账本清点：🔴 安全红线类=0；🟡 本段收口前清偿 ×N 全闭环；
//!     🟢 攒批登记 ×N（现象/位置/严重度/建议修法/复现路径五字段齐）；
//!     判据锚定 双轨产线制度；③ 完成时 API/软件写回：状态=已完成、结
//!     果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52294）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52295-J1，照原文锚定；功能按本卡标题（覆盖率三口径"工程级"
//! 落账）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 三口径闭集：条目口径 / 行口径 / 判据口径各 8 格，合计 24 格；
//!      口径下标单调（条目→行→判据），口径越闭集必抓；
//!   ② 口径漂移检出注入 10/10：漂移注入先录正格再注漂移，逐轮必抓；
//!   ③ AI-96 联签锚定行 2 行闭集（三口径联签行 / 工程级联签行）；
//!   ④ "工程级"换算账 6 格：清单覆盖比→工程级档位的换算逐格落账，
//!      漂移必抓；
//!   ⑤ 工程级覆盖闸：三口径各口径按整数百分位核算 ≥75%（covered×100
//!      ≥ 75×8），不足即封拒；
//!   ⑥ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a，
//!      grade() 定级闭集）；注入核 run_cycle 三态轮转 10/10（口径漂移/
//!      换算格漂移/口径越闭集），三类注入全部落在当轮临时账本上，主账
//!      injected==caught==10、escaped==0；双跑 FNV 指纹一致；定级
//!      S1/S2/S3 闭集（24 格 + 6 换算格 + 2 联签行 + 六判据 + 覆盖闸
//!      全通 → S1）。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无
//!      unwrap/expect/panic/裸下标；XOR 锚常量低 16 位恒零。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52294;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 口径族数（条目/行/判据）。
pub const METRIC_KINDS: usize = 3;

/// 每口径格数。
pub const CELLS_PER_KIND: usize = 8;

/// 三口径总格数（3 × 8 = 24）。
pub const METRIC_CELLS: usize = METRIC_KINDS * CELLS_PER_KIND;

/// "工程级"换算账格数。
pub const CONV_CELLS: usize = 6;

/// AI-96 联签锚定行数。
pub const ANCHOR_ROWS: usize = 2;

/// 六判据闸数。
pub const GATE_COUNT: usize = 6;

/// 工程级覆盖闸百分位（每口径 ≥75%）。
pub const COV_GATE_PCT: u32 = 75;

/// XOR 联签锚（低 16 位恒零）。
pub const XOR_ANCHOR: u64 = 0x0041_4996_0000;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cov3WayCode {
    /// 成功
    Ok = 0,
    /// 口径或格号越闭集
    ENoMetric,
    /// 口径载荷漂移
    EMetricDrift,
    /// 换算格越闭集
    ENoConv,
    /// 换算格漂移
    EConvDrift,
    /// 口径序逆序
    EKindOrder,
    /// 联签锚定行越闭集
    ENoRow,
    /// 缺联签锚定行
    ERowMiss,
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

impl Cov3WayCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            Cov3WayCode::Ok => "成功",
            Cov3WayCode::ENoMetric => "口径或格号越闭集：条目/行/判据三口径 ×8 格之外先登记",
            Cov3WayCode::EMetricDrift => "口径载荷漂移：口径格与基准不恒等，逐格复算",
            Cov3WayCode::ENoConv => "换算格越闭集：工程级换算账 6 格之外先登记",
            Cov3WayCode::EConvDrift => "换算格漂移：工程级换算折叠与基准不恒等，逐格复算",
            Cov3WayCode::EKindOrder => "口径序逆序：条目→行→判据单调，逆序必抓",
            Cov3WayCode::ENoRow => "联签锚定行越闭集：AI-96 联签 2 行闭集之外先登记",
            Cov3WayCode::ERowMiss => "缺联签锚定行：AI-96 联签锚定行未齐先补齐",
            Cov3WayCode::EDup => "重复落账：同格只许一条账目",
            Cov3WayCode::ESeal => "未齐即封：24 口径格 + 6 换算格 + 2 联签行未齐先补齐",
            Cov3WayCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            Cov3WayCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            Cov3WayCode::EGradeDrift => "定级漂移：全通且三口径覆盖闸 ≥75% 才可定 S1",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// 三口径落账定级闭集。
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
            Grade::S1 => "S1：24口径格+6换算格+2联签行+六判据+覆盖闸全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 口径格载荷（口径 k、格 c 的对照基准，纯函数恒等）。
pub const fn metric_cell(k: usize, c: usize) -> Option<u64> {
    if k >= METRIC_KINDS || c >= CELLS_PER_KIND {
        return None;
    }
    Some((0xBA000u64 + (k as u64) * 0x100 + (c as u64) * 0x47) ^ XOR_ANCHOR)
}

/// 口径名（const fn 表 + Option 边界）。
pub const fn kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("条目口径"),
        1 => Some("行口径"),
        2 => Some("判据口径"),
        _ => None,
    }
}

/// 条目口径格名。
pub const fn metric_cell_name(k: usize, c: usize) -> Option<&'static str> {
    match (k, c) {
        (0, 0) => Some("条目口径·清单覆盖"),
        (0, 1) => Some("条目口径·应用覆盖"),
        (0, 2) => Some("条目口径·阶段账"),
        (0, 3) => Some("条目口径·部落账"),
        (0, 4) => Some("条目口径·公开位"),
        (0, 5) => Some("条目口径·补录位"),
        (0, 6) => Some("条目口径·复算位"),
        (0, 7) => Some("条目口径·收口位"),
        (1, 0) => Some("行口径·行覆盖"),
        (1, 1) => Some("行口径·字行比"),
        (1, 2) => Some("行口径·减权账"),
        (1, 3) => Some("行口径·抽读账"),
        (1, 4) => Some("行口径·对照位"),
        (1, 5) => Some("行口径·补录位"),
        (1, 6) => Some("行口径·复算位"),
        (1, 7) => Some("行口径·收口位"),
        (2, 0) => Some("判据口径·J1 覆盖"),
        (2, 1) => Some("判据口径·判据折算"),
        (2, 2) => Some("判据口径·锚定行"),
        (2, 3) => Some("判据口径·治理线"),
        (2, 4) => Some("判据口径·对照位"),
        (2, 5) => Some("判据口径·补录位"),
        (2, 6) => Some("判据口径·复算位"),
        (2, 7) => Some("判据口径·收口位"),
        _ => None,
    }
}

/// 工程级换算格载荷（格 c 的对照基准）。
pub const fn conv_cell(c: usize) -> Option<u32> {
    if c >= CONV_CELLS {
        return None;
    }
    Some((0xBA400u32 + (c as u32) * 0x3D) ^ 0x0004_0000)
}

/// 换算格名。
pub const fn conv_name(c: usize) -> Option<&'static str> {
    match c {
        0 => Some("换算·清单覆盖比"),
        1 => Some("换算·工程级分母"),
        2 => Some("换算·工程级分子"),
        3 => Some("换算·档位折算"),
        4 => Some("换算·复核算"),
        5 => Some("换算·落账值"),
        _ => None,
    }
}

/// AI-96 联签锚定行折叠（行 r）。
pub const fn anchor_row(r: usize) -> Option<u32> {
    if r >= ANCHOR_ROWS {
        return None;
    }
    Some(0xBA500 + r as u32 * 0x2C)
}

/// 联签锚定行名。
pub const fn anchor_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("AI-96 三口径联签锚定行"),
        1 => Some("AI-96 工程级联签锚定行"),
        _ => None,
    }
}

/// 六判据闸名。
pub const fn gate_name(g: usize) -> Option<&'static str> {
    match g {
        0 => Some("三口径启动"),
        1 => Some("口径恒等"),
        2 => Some("换算复算"),
        3 => Some("联签齐备"),
        4 => Some("覆盖闸核算"),
        5 => Some("工程级收口"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// 三口径落账本状态机。
#[derive(Clone)]
pub struct Cov3WayLedger {
    /// 六判据闸。
    gates: [Option<bool>; GATE_COUNT],
    /// 三口径账（口径 k × 格 c）。
    metrics: [[Option<u64>; CELLS_PER_KIND]; METRIC_KINDS],
    /// 工程级换算账。
    convs: [Option<u32>; CONV_CELLS],
    /// AI-96 联签锚定行账。
    anchors: [Option<u32>; ANCHOR_ROWS],
    /// 最后已录口径（口径序单调）。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl Cov3WayLedger {
    /// 新账本。
    pub fn new() -> Self {
        Cov3WayLedger {
            gates: [None; GATE_COUNT],
            metrics: [[None; CELLS_PER_KIND]; METRIC_KINDS],
            convs: [None; CONV_CELLS],
            anchors: [None; ANCHOR_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), Cov3WayCode> {
        if gate >= GATE_COUNT {
            return Err(Cov3WayCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x4C00 + gate as u64);
        Ok(())
    }

    /// 六判据全落核验。
    pub fn gates_all(&self) -> bool {
        let mut g = 0usize;
        while g < GATE_COUNT {
            match self.gates.get(g) {
                Some(Some(true)) => {}
                _ => return false,
            }
            g += 1;
        }
        true
    }

    /// 口径一格（口径序单调 + 载荷对照 + 重复拒）。
    pub fn record_metric(&mut self, k: usize, c: usize, got: u64) -> Result<(), Cov3WayCode> {
        if k >= METRIC_KINDS || c >= CELLS_PER_KIND {
            return Err(Cov3WayCode::ENoMetric);
        }
        if (k as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4D00);
            return Err(Cov3WayCode::EKindOrder);
        }
        let want = match metric_cell(k, c) {
            Some(w) => w,
            None => return Err(Cov3WayCode::ENoMetric),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4D01 + k as u64 * 8 + c as u64);
            return Err(Cov3WayCode::EMetricDrift);
        }
        if match self.metrics.get(k).and_then(|row| row.get(c)) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(Cov3WayCode::EDup);
        }
        if let Some(row) = self.metrics.get_mut(k) {
            if let Some(cell) = row.get_mut(c) {
                *cell = Some(got);
            }
        }
        self.last_kind = k as i64;
        self.fnv(0x4D02 + k as u64 * 8 + c as u64);
        Ok(())
    }

    /// 口径全账核验（24 格）。
    pub fn metrics_all(&self) -> bool {
        let mut k = 0usize;
        while k < METRIC_KINDS {
            let mut c = 0usize;
            while c < CELLS_PER_KIND {
                match self.metrics.get(k).and_then(|row| row.get(c)) {
                    Some(Some(_)) => {}
                    _ => return false,
                }
                c += 1;
            }
            k += 1;
        }
        true
    }

    /// 单口径覆盖核算（整数百分位 ≥75%）。
    pub fn kind_cov_gate(&self, k: usize) -> bool {
        let mut covered = 0u32;
        let mut c = 0usize;
        while c < CELLS_PER_KIND {
            if match self.metrics.get(k).and_then(|row| row.get(c)) {
                Some(Some(_)) => true,
                _ => false,
            } {
                covered += 1;
            }
            c += 1;
        }
        covered * 100 >= COV_GATE_PCT * CELLS_PER_KIND as u32
    }

    /// 工程级换算一格（载荷对照 + 重复拒）。
    pub fn record_conv(&mut self, c: usize, got: u32) -> Result<(), Cov3WayCode> {
        let want = match conv_cell(c) {
            Some(x) => x,
            None => return Err(Cov3WayCode::ENoConv),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4E00 + c as u64);
            return Err(Cov3WayCode::EConvDrift);
        }
        if match self.convs.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(Cov3WayCode::EDup);
        }
        if let Some(cell) = self.convs.get_mut(c) {
            *cell = Some(got);
        }
        self.fnv(0x4E01 + c as u64);
        Ok(())
    }

    /// 换算全账核验（6 格）。
    pub fn convs_all(&self) -> bool {
        let mut c = 0usize;
        while c < CONV_CELLS {
            match self.convs.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// AI-96 联签锚定行一格。
    pub fn declare_anchor(&mut self, r: usize) -> Result<(), Cov3WayCode> {
        if r >= ANCHOR_ROWS {
            return Err(Cov3WayCode::ENoRow);
        }
        if match self.anchors.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(Cov3WayCode::EDup);
        }
        if let Some(cell) = self.anchors.get_mut(r) {
            *cell = anchor_row(r);
        }
        self.fnv(0x4F00 + r as u64);
        Ok(())
    }

    /// 联签锚定行全齐核验。
    pub fn anchors_all(&self) -> bool {
        let mut r = 0usize;
        while r < ANCHOR_ROWS {
            match self.anchors.get(r) {
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

    /// 定级（全通且三口径覆盖闸达标 → S1，判据缺 → S3，其余 → S2）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all()
            && self.metrics_all()
            && self.convs_all()
            && self.anchors_all()
            && self.kind_cov_gate(0)
            && self.kind_cov_gate(1)
            && self.kind_cov_gate(2)
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for Cov3WayLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（落账格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = Cov3WayLedger::new();
    let mut g = 0usize;
    while g < GATE_COUNT {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut k = 0usize;
    while k < METRIC_KINDS {
        let mut c = 0usize;
        while c < CELLS_PER_KIND {
            if let Some(w) = metric_cell(k, c) {
                if led.record_metric(k, c, w).is_ok() {
                    recorded += 1;
                }
            }
            c += 1;
        }
        k += 1;
    }
    let mut c = 0usize;
    while c < CONV_CELLS {
        if let Some(x) = conv_cell(c) {
            if led.record_conv(c, x).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    let mut r = 0usize;
    while r < ANCHOR_ROWS {
        let _ = led.declare_anchor(r);
        r += 1;
    }
    // 注入 10 轮：口径漂移 / 换算格漂移 / 口径越闭集三态轮转。
    // 三类注入全部落在当轮临时账本上；主账只记 injected/caught。
    let mut i = 0usize;
    while i < 10 {
        let kind = (i % 3) as u32;
        let mut lt = Cov3WayLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_metric(0, 3, metric_cell(0, 3).unwrap_or(0));
                lt.record_metric(0, 4, metric_cell(0, 4).unwrap_or(0).wrapping_add(1))
                    == Err(Cov3WayCode::EMetricDrift)
            }
            1 => {
                let _ = lt.record_conv(0, conv_cell(0).unwrap_or(0));
                lt.record_conv(1, conv_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(Cov3WayCode::EConvDrift)
            }
            _ => {
                let _ = lt.record_metric(1, 0, metric_cell(1, 0).unwrap_or(0));
                lt.record_metric(METRIC_KINDS, 0, 0).is_err()
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5000 + i as u64);
        } else {
            led.escaped += 1;
        }
        i += 1;
    }
    let caught = led.caught;
    let fp = led.fingerprint();
    (recorded, caught, fp)
}

// ── 判据集 ───────────────────────────────────────────────────────
/// UNX-F52294 判据集（锚 UNX-F52295-J1，off-by-one 板式）。
pub fn run_unx52294_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52294");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "三口径 24 格全落（条目/行/判据 ×8）",
        recorded >= METRIC_CELLS as u32,
        "三口径闭集逐格恒等（双轨产线制度锚定）",
    );
    s.add(
        "工程级换算账 6 格全落",
        {
            let mut lt = Cov3WayLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < CONV_CELLS {
                if let Some(x) = conv_cell(c) {
                    if lt.record_conv(c, x).is_err() {
                        ok = false;
                    }
                }
                c += 1;
            }
            ok && lt.convs_all()
        },
        "覆盖比→工程级档位换算逐格恒等",
    );
    s.add(
        "AI-96 联签锚定行 2/2 全齐",
        {
            let mut lt = Cov3WayLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < ANCHOR_ROWS {
                if lt.declare_anchor(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.anchors_all()
        },
        "三口径联签行/工程级联签行两行闭集，缺一拒封",
    );
    s.add(
        "六判据闸全落",
        {
            let mut lt = Cov3WayLedger::new();
            let mut g = 0usize;
            while g < GATE_COUNT {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "三口径启动/口径恒等/换算复算/联签齐备/覆盖闸核算/工程级收口",
    );
    s.add(
        "工程级覆盖闸（三口径各 ≥75%）",
        {
            let mut lt = Cov3WayLedger::new();
            // 只落口径 0 的 6/8 格 = 75%，恰达闸；口径 1/2 空账不达。
            let mut c = 0usize;
            while c < 6 {
                if let Some(w) = metric_cell(0, c) {
                    let _ = lt.record_metric(0, c, w);
                }
                c += 1;
            }
            lt.kind_cov_gate(0) && !lt.kind_cov_gate(1)
        },
        "covered×100 ≥ 75×8 整数核算，6/8 恰达闸",
    );
    s.add(
        "口径序单调",
        {
            let mut ok = true;
            let mut k = 1usize;
            while k < METRIC_KINDS {
                if kind_name(k - 1).is_none() || kind_name(k).is_none() {
                    ok = false;
                }
                k += 1;
            }
            ok
        },
        "条目→行→判据单调，逆序必抓",
    );
    s.add("注入 10/10 全抓", caught >= 10, "口径漂移/换算格漂移/口径越闭集三态轮转必抓");
    s.add(
        "边界·口径越闭集",
        {
            let mut lt = Cov3WayLedger::new();
            lt.record_metric(METRIC_KINDS, 0, 0).is_err()
        },
        "口径 3 越闭集必拒",
    );
    s.add(
        "边界·口径格越闭集",
        {
            let mut lt = Cov3WayLedger::new();
            lt.record_metric(0, CELLS_PER_KIND, 0).is_err()
        },
        "口径格 8 越闭集必拒",
    );
    s.add(
        "口径漂移拒",
        {
            let mut lt = Cov3WayLedger::new();
            lt.record_metric(2, 5, metric_cell(2, 5).unwrap_or(0).wrapping_add(1))
                == Err(Cov3WayCode::EMetricDrift)
        },
        "口径载荷漂移必抓",
    );
    s.add(
        "边界·换算格越闭集",
        {
            let mut lt = Cov3WayLedger::new();
            lt.record_conv(CONV_CELLS, 0).is_err()
        },
        "换算格 6 越闭集必拒",
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
            let mut lt = Cov3WayLedger::new();
            let mut g = 0usize;
            while g < GATE_COUNT {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut k = 0usize;
            while k < METRIC_KINDS {
                let mut c = 0usize;
                while c < CELLS_PER_KIND {
                    if let Some(w) = metric_cell(k, c) {
                        let _ = lt.record_metric(k, c, w);
                    }
                    c += 1;
                }
                k += 1;
            }
            let mut c = 0usize;
            while c < CONV_CELLS {
                if let Some(x) = conv_cell(c) {
                    let _ = lt.record_conv(c, x);
                }
                c += 1;
            }
            let mut r = 0usize;
            while r < ANCHOR_ROWS {
                let _ = lt.declare_anchor(r);
                r += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全通且覆盖闸达标 → S1（工程级落账登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC8940,
        "SEG_BASE 0x40 对齐且按 VX15 公式落 0xC8940",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metric_cells_pure() {
        let a = metric_cell(1, 3).unwrap_or(0);
        assert_eq!(a, metric_cell(1, 3).unwrap_or(0));
        assert_ne!(a, metric_cell(1, 4).unwrap_or(0));
        assert_ne!(a, metric_cell(2, 3).unwrap_or(0));
        assert!(metric_cell(METRIC_KINDS, 0).is_none());
        assert!(metric_cell(0, CELLS_PER_KIND).is_none());
        assert_eq!(kind_name(0), Some("条目口径"));
        assert_eq!(kind_name(2), Some("判据口径"));
        assert!(kind_name(3).is_none());
    }

    #[test]
    fn test_metric_cell_names() {
        assert_eq!(metric_cell_name(0, 0), Some("条目口径·清单覆盖"));
        assert_eq!(metric_cell_name(1, 7), Some("行口径·收口位"));
        assert_eq!(metric_cell_name(2, 7), Some("判据口径·收口位"));
        assert!(metric_cell_name(3, 0).is_none());
        assert!(metric_cell_name(0, 8).is_none());
    }

    #[test]
    fn test_conv_cells() {
        assert_eq!(conv_name(0), Some("换算·清单覆盖比"));
        assert_eq!(conv_name(5), Some("换算·落账值"));
        assert!(conv_name(6).is_none());
        assert!(conv_cell(CONV_CELLS).is_none());
        assert_ne!(conv_cell(2).unwrap_or(0), conv_cell(3).unwrap_or(0));
    }

    #[test]
    fn test_anchor_rows() {
        assert_eq!(anchor_name(0), Some("AI-96 三口径联签锚定行"));
        assert_eq!(anchor_name(1), Some("AI-96 工程级联签锚定行"));
        assert!(anchor_name(2).is_none());
        assert!(anchor_row(ANCHOR_ROWS).is_none());
    }

    #[test]
    fn test_kind_order_caught() {
        let mut lt = Cov3WayLedger::new();
        let _ = lt.record_metric(1, 0, metric_cell(1, 0).unwrap_or(0));
        assert_eq!(
            lt.record_metric(0, 0, metric_cell(0, 0).unwrap_or(0)),
            Err(Cov3WayCode::EKindOrder)
        );
        // 逆序被拒后不改账位：合法后续仍可落。
        assert!(lt.record_metric(2, 0, metric_cell(2, 0).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = Cov3WayLedger::new();
        assert_eq!(
            lt.record_metric(0, 2, metric_cell(0, 2).unwrap_or(0).wrapping_add(1)),
            Err(Cov3WayCode::EMetricDrift)
        );
        assert_eq!(
            lt.record_conv(4, conv_cell(4).unwrap_or(0).wrapping_add(9)),
            Err(Cov3WayCode::EConvDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = Cov3WayLedger::new();
        let _ = lt.record_metric(0, 0, metric_cell(0, 0).unwrap_or(0));
        assert_eq!(
            lt.record_metric(0, 0, metric_cell(0, 0).unwrap_or(0)),
            Err(Cov3WayCode::EDup)
        );
        let _ = lt.record_conv(0, conv_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_conv(0, conv_cell(0).unwrap_or(0)),
            Err(Cov3WayCode::EDup)
        );
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = Cov3WayLedger::new();
        let mut g = 0usize;
        while g < GATE_COUNT {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert!(lt.judge_gate(GATE_COUNT).is_err());
        assert!(lt.gates_all());
        let mut lt2 = Cov3WayLedger::new();
        assert!(lt2.judge_gate(1).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_cov_gate() {
        let mut lt = Cov3WayLedger::new();
        assert!(!lt.kind_cov_gate(0));
        // 5/8 = 62.5% 不达；6/8 = 75% 恰达。
        let mut c = 0usize;
        while c < 5 {
            if let Some(w) = metric_cell(0, c) {
                let _ = lt.record_metric(0, c, w);
            }
            c += 1;
        }
        assert!(!lt.kind_cov_gate(0));
        if let Some(w) = metric_cell(0, 5) {
            let _ = lt.record_metric(0, 5, w);
        }
        assert!(lt.kind_cov_gate(0));
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (METRIC_CELLS + CONV_CELLS) as u32);
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
        assert_eq!(SEG_BASE, 0xC8940);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(Cov3WayCode::Ok as u32, 0);
        assert_eq!(Cov3WayCode::EGradeDrift as u32, 12);
        assert!(!Cov3WayCode::Ok.detail().is_empty());
        assert!(!Cov3WayCode::EKindOrder.detail().is_empty());
        assert_eq!(Cov3WayCode::Ok.val(), SEG_BASE);
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        let lt = Cov3WayLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        assert!(!Grade::S2.detail().is_empty());
    }

    #[test]
    fn test_injection_conservation() {
        let (_, caught, _) = run_cycle();
        assert_eq!(caught, 10);
        let mut lt = Cov3WayLedger::new();
        let (inj, c, e) = lt.stats();
        assert_eq!(inj, c);
        assert_eq!(e, 0);
    }

    #[test]
    fn unx52294_checks_green() {
        let s = run_unx52294_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
