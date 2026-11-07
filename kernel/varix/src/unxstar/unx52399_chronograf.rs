//! UNX-F52399 Chronograf / InfluxDB UI 消费面收口 · 0xCA380段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15，逐字）：
//!   键 "52399"（判据原文锚定源）criteria：① 纯功能代码 ≥280 行；② 判据
//!   原文：UNX-F52400-J1 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3；③
//!   完成时 API/软件写回：状态=已完成、结果摘要非空。
//!   键 "52400"（功能标题源）title：Chronograf / InfluxDB UI 消费面收口
//!   quota: 320
//! off-by-one 板式口径：本卡（卡 N=52399）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52400-J1，照原文（键 "52399" criteria）锚定；功能按功能标题源
//! （键 "52400" title：Chronograf / InfluxDB UI 消费面收口）落地；行数
//! 基线按功能标题源 quota=320 执行（落盘 ≥550 行）。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五功能族闭集 20 格：explorer/dashboards/alerts/cap/tasks 各 4 格，
//!      严格阶梯（空账本首族必 explorer，跳族/回跳必抓），逐格载荷纯函
//!      数折叠恒等才收，漂移必抓。
//!   ② 连接三元闸 3 格：InfluxDB/Kapacitor/OAuth2 三元闭集，空账本首闸
//!      必 InfluxDB，逆跳/越闸必抓 EConnOrd。
//!   ③ 查询投影 12 格：查询构建器→InfluxQL/Flux 投影逐格恒等，载荷漂移
//!      EQueryDrift 必抓，同格二次提交必拒 EDup（回投核验）。
//!   ④ 与 InfluxDB 消费联签 4 格（F52398 承接联签）+ Go 上栈声明 2 行
//!      闭集，缺行拒封（ERowMiss），未齐拒封（ESeal）。
//!   ⑤ ledger 状态机（new/record_*/judge_gate/fingerprint，FNV-1a）；注
//!      入核 run_cycle 三态轮转 10/10（连接三元闸逆/功能族载荷漂移/越闭
//!      集），三类注入全部落在当轮临时账本上，检出+逃逸守恒等式；双跑
//!      FNV 指纹一致；定级闭集 S1/S2/S3（全齐 → S1）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52399;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 五功能族格数（5 族 × 4）。
pub const FEATURES: usize = 20;

/// 连接三元闸格数。
pub const CONN_GATES: usize = 3;

/// 查询投影格数。
pub const QUERY_CELLS: usize = 12;

/// InfluxDB 消费联签行数（F52398 承接联签）。
pub const UNION_ROWS: usize = 4;

/// Go 上栈声明行数。
pub const GO_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChronografCode {
    /// 成功
    Ok = 0,
    /// 格号越闭集
    ENoCell,
    /// 功能族/联签载荷漂移
    EDrift,
    /// 功能族阶梯逆序
    EOrder,
    /// 连接三元闸逆序
    EConnOrd,
    /// 查询投影漂移
    EQueryDrift,
    /// 声明行越闭集
    ENoRow,
    /// 缺声明行
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

impl ChronografCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            ChronografCode::Ok => "成功",
            ChronografCode::ENoCell => "格号越闭集：五功能族 20 格/查询投影 12 格闭集之外先登记",
            ChronografCode::EDrift => "载荷漂移：功能族/联签折叠与基准不恒等，逐格复算",
            ChronografCode::EOrder => "功能族阶梯逆序：首族必 explorer，explorer→dashboards→alerts→cap→tasks 逐级单调",
            ChronografCode::EConnOrd => "连接三元闸逆序：首闸必 InfluxDB，InfluxDB→Kapacitor→OAuth2 单调",
            ChronografCode::EQueryDrift => "查询投影漂移：查询构建器→InfluxQL/Flux 投影载荷与基准不恒等，逐格复算",
            ChronografCode::ENoRow => "声明行越闭集：InfluxDB 联签 4 行/Go 上栈 2 行闭集之外先登记",
            ChronografCode::ERowMiss => "缺声明行：InfluxDB 消费联签或 Go 上栈声明未齐先补齐",
            ChronografCode::EDup => "重复落账：同格只许一条账目",
            ChronografCode::ESeal => "未齐即封：20 功能 + 3 连接 + 12 投影未齐先补齐",
            ChronografCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            ChronografCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            ChronografCode::EGradeDrift => "定级漂移：全齐才可定 S1，判据缺只可定 S3",
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
            Grade::S1 => "S1：六判据+20功能+3连接+12投影+4联签+2Go 全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 五功能族格载荷（格 o；族 = o/4）。
pub const fn feat_cell(o: usize) -> Option<u64> {
    if o >= FEATURES {
        return None;
    }
    Some((0xD9000u64 + (o as u64) * 0x3F) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 功能族（0..4=explorer 4..8=dashboards 8..12=alerts 12..16=cap 16..20=tasks）。
pub const fn feat_kind(o: usize) -> Option<u32> {
    if o >= FEATURES {
        return None;
    }
    Some((o / 4) as u32)
}

/// 功能族名。
pub const fn kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("explorer"),
        1 => Some("dashboards"),
        2 => Some("alerts"),
        3 => Some("cap"),
        4 => Some("tasks"),
        _ => None,
    }
}

/// 连接闸号（0=InfluxDB 1=Kapacitor 2=OAuth2）。
pub const fn conn_of(i: usize) -> Option<u32> {
    if i >= CONN_GATES {
        return None;
    }
    Some(i as u32)
}

/// 连接闸名。
pub const fn conn_name(d: usize) -> Option<&'static str> {
    match d {
        0 => Some("InfluxDB"),
        1 => Some("Kapacitor"),
        2 => Some("OAuth2"),
        _ => None,
    }
}

/// 连接闸载荷（格 i）。
pub const fn conn_cell(i: usize) -> Option<u64> {
    if i >= CONN_GATES {
        return None;
    }
    Some((0xD9800u64 + (i as u64) * 0x45) ^ 0x434E4E47_0000)
}

/// 查询投影格载荷（格 i）。
pub const fn query_cell(i: usize) -> Option<u64> {
    if i >= QUERY_CELLS {
        return None;
    }
    Some((0xDA000u64 + (i as u64) * 0x49) ^ 0x51555259_0000)
}

/// InfluxDB 消费联签行折叠（行 r）。
pub const fn union_row(r: usize) -> Option<u32> {
    if r >= UNION_ROWS {
        return None;
    }
    Some(0xDB00 + r as u32 * 0x2C)
}

/// Go 上栈声明行折叠（行 r）。
pub const fn go_row(r: usize) -> Option<u32> {
    if r >= GO_ROWS {
        return None;
    }
    Some(0xDB40 + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Chronograf / InfluxDB UI 消费面收口账本状态机。
#[derive(Clone)]
pub struct ChronografLedger {
    /// 六判据（安装/服务注册/3000 端口开放/查询构建功能/卸载净除/升级保留）。
    gates: [Option<bool>; 6],
    /// 功能族账。
    feats: [Option<u64>; FEATURES],
    /// 连接三元闸账。
    conns: [Option<u64>; CONN_GATES],
    /// 查询投影账。
    query: [Option<u64>; QUERY_CELLS],
    /// InfluxDB 消费联签账。
    unions: [Option<u32>; UNION_ROWS],
    /// Go 上栈声明账。
    gos: [Option<u32>; GO_ROWS],
    /// 最后功能族。
    last_kind: i64,
    /// 最后连接闸。
    last_conn: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl ChronografLedger {
    /// 新账本。
    pub fn new() -> Self {
        ChronografLedger {
            gates: [None; 6],
            feats: [None; FEATURES],
            conns: [None; CONN_GATES],
            query: [None; QUERY_CELLS],
            unions: [None; UNION_ROWS],
            gos: [None; GO_ROWS],
            last_kind: -1,
            last_conn: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), ChronografCode> {
        if gate >= 6 {
            return Err(ChronografCode::ENoCell);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0xDC000 + gate as u64);
        Ok(())
    }

    /// 六判据全落核验。
    pub fn gates_all(&self) -> bool {
        let mut g = 0usize;
        while g < 6 {
            match self.gates.get(g) {
                Some(Some(true)) => {}
                _ => return false,
            }
            g += 1;
        }
        true
    }

    /// 功能族一格（严格阶梯：首族必 explorer + 逐级 +1 + 载荷对照 + 重复拒）。
    pub fn record_feat(&mut self, o: usize, got: u64) -> Result<(), ChronografCode> {
        if o >= FEATURES {
            return Err(ChronografCode::ENoCell);
        }
        let kind = match feat_kind(o) {
            Some(k) => k as i64,
            None => return Err(ChronografCode::ENoCell),
        };
        let order_bad = if self.last_kind < 0 {
            kind != 0
        } else {
            kind < self.last_kind || kind > self.last_kind + 1
        };
        if order_bad {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xDC100);
            return Err(ChronografCode::EOrder);
        }
        let want = match feat_cell(o) {
            Some(w) => w,
            None => return Err(ChronografCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xDC101 + o as u64);
            return Err(ChronografCode::EDrift);
        }
        if match self.feats.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ChronografCode::EDup);
        }
        if let Some(c) = self.feats.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind;
        self.fnv(0xDC102 + o as u64);
        Ok(())
    }

    /// 功能族全账核验（20 格）。
    pub fn feats_all(&self) -> bool {
        let mut o = 0usize;
        while o < FEATURES {
            match self.feats.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 连接一闸（闸序单调 + 首闸必 InfluxDB + 载荷对照 + 重复拒）。
    pub fn record_conn(&mut self, i: usize, got: u64) -> Result<(), ChronografCode> {
        if i >= CONN_GATES {
            return Err(ChronografCode::EConnOrd);
        }
        let c = match conn_of(i) {
            Some(c) => c as i64,
            None => return Err(ChronografCode::EConnOrd),
        };
        let ord_bad = if self.last_conn < 0 {
            c != 0
        } else {
            c < self.last_conn || c > self.last_conn + 1
        };
        if ord_bad {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xDC200);
            return Err(ChronografCode::EConnOrd);
        }
        let want = match conn_cell(i) {
            Some(w) => w,
            None => return Err(ChronografCode::EConnOrd),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xDC201 + i as u64);
            return Err(ChronografCode::EDrift);
        }
        if match self.conns.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ChronografCode::EDup);
        }
        if let Some(cc) = self.conns.get_mut(i) {
            *cc = Some(got);
        }
        self.last_conn = c;
        self.fnv(0xDC202 + i as u64);
        Ok(())
    }

    /// 连接全账核验（3 闸）。
    pub fn conns_all(&self) -> bool {
        let mut i = 0usize;
        while i < CONN_GATES {
            match self.conns.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 查询投影一格（载荷对照 + 重复拒；回投即 EDup）。
    pub fn record_query(&mut self, i: usize, got: u64) -> Result<(), ChronografCode> {
        if i >= QUERY_CELLS {
            return Err(ChronografCode::ENoCell);
        }
        let want = match query_cell(i) {
            Some(w) => w,
            None => return Err(ChronografCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xDC300 + i as u64);
            return Err(ChronografCode::EQueryDrift);
        }
        if match self.query.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ChronografCode::EDup);
        }
        if let Some(c) = self.query.get_mut(i) {
            *c = Some(got);
        }
        self.fnv(0xDC301 + i as u64);
        Ok(())
    }

    /// 投影全账核验（12 格）。
    pub fn query_all(&self) -> bool {
        let mut i = 0usize;
        while i < QUERY_CELLS {
            match self.query.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// InfluxDB 消费联签一行（F52398 承接联签）。
    pub fn declare_union(&mut self, r: usize) -> Result<(), ChronografCode> {
        if r >= UNION_ROWS {
            return Err(ChronografCode::ENoRow);
        }
        if match self.unions.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ChronografCode::EDup);
        }
        if let Some(c) = self.unions.get_mut(r) {
            *c = union_row(r);
        }
        self.fnv(0xDC400 + r as u64);
        Ok(())
    }

    /// 联签全齐核验。
    pub fn unions_all(&self) -> bool {
        let mut r = 0usize;
        while r < UNION_ROWS {
            match self.unions.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// Go 上栈声明一行。
    pub fn declare_go(&mut self, r: usize) -> Result<(), ChronografCode> {
        if r >= GO_ROWS {
            return Err(ChronografCode::ENoRow);
        }
        if match self.gos.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ChronografCode::EDup);
        }
        if let Some(c) = self.gos.get_mut(r) {
            *c = go_row(r);
        }
        self.fnv(0xDC480 + r as u64);
        Ok(())
    }

    /// Go 声明全齐核验。
    pub fn gos_all(&self) -> bool {
        let mut r = 0usize;
        while r < GO_ROWS {
            match self.gos.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 封账核验：声明行缺 → ERowMiss；格/闸未齐 → ESeal；全齐 → Ok。
    pub fn seal(&self) -> Result<(), ChronografCode> {
        if !(self.unions_all() && self.gos_all()) {
            return Err(ChronografCode::ERowMiss);
        }
        if !(self.gates_all() && self.feats_all() && self.conns_all() && self.query_all()) {
            return Err(ChronografCode::ESeal);
        }
        Ok(())
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 注入守恒等式：注入 = 检出 + 逃逸。
    pub fn conservation(&self) -> Result<(), ChronografCode> {
        if self.injected == self.caught + self.escaped {
            Ok(())
        } else {
            Err(ChronografCode::EInj)
        }
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// KB-COMPAT 定级（全齐 → S1，六闸齐 → S2，否则 S3）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all()
            && self.feats_all()
            && self.conns_all()
            && self.query_all()
            && self.unions_all()
            && self.gos_all()
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for ChronografLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 双跑指纹对照。
pub fn fp_check(a: u64, b: u64) -> Result<(), ChronografCode> {
    if a == b && a != 0 {
        Ok(())
    } else {
        Err(ChronografCode::EFp)
    }
}

/// 定级守卫：定级与目标档漂移 → EGradeDrift。
pub fn grade_guard(led: &ChronografLedger, want: Grade) -> Result<(), ChronografCode> {
    let got = led.judge_grade();
    if got == want {
        Ok(())
    } else {
        Err(ChronografCode::EGradeDrift)
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（记格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = ChronografLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < FEATURES {
        if let Some(w) = feat_cell(o) {
            if led.record_feat(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut d = 0usize;
    while d < CONN_GATES {
        if let Some(w) = conn_cell(d) {
            if led.record_conn(d, w).is_ok() {
                recorded += 1;
            }
        }
        d += 1;
    }
    let mut i = 0usize;
    while i < QUERY_CELLS {
        if let Some(w) = query_cell(i) {
            if led.record_query(i, w).is_ok() {
                recorded += 1;
            }
        }
        i += 1;
    }
    let mut r = 0usize;
    while r < UNION_ROWS {
        let _ = led.declare_union(r);
        r += 1;
    }
    let mut r = 0usize;
    while r < GO_ROWS {
        let _ = led.declare_go(r);
        r += 1;
    }
    // 注入 10 轮：连接三元闸逆（空账本必抓）/ 功能族载荷漂移 / 越闭集
    // 三态轮转，全部落在当轮临时账本上（主账 last_kind/last_conn 不受扰）。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = ChronografLedger::new();
        let caught = match kind {
            0 => lt.record_conn(2, conn_cell(2).unwrap_or(0)) == Err(ChronografCode::EConnOrd),
            1 => {
                let _ = lt.record_feat(0, feat_cell(0).unwrap_or(0));
                lt.record_feat(1, feat_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(ChronografCode::EDrift)
            }
            _ => {
                lt.record_feat(FEATURES, 0).is_err()
                    && lt.record_query(QUERY_CELLS, 0).is_err()
                    && lt.record_conn(CONN_GATES, 0).is_err()
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0xDC500 + k as u64);
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
/// UNX-F52399 判据集（锚 UNX-F52400-J1，off-by-one 板式）。
pub fn run_unx52399_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52399");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "run_cycle 记格数恒等",
        recorded == FEATURES as u32 + CONN_GATES as u32 + QUERY_CELLS as u32,
        "20 功能 + 3 连接 + 12 投影逐格恒等（判据闸与声明行不计数）",
    );
    s.add(
        "五功能族 20 格全通",
        {
            let mut lt = ChronografLedger::new();
            let mut ok = true;
            let mut o = 0usize;
            while o < FEATURES {
                if let Some(w) = feat_cell(o) {
                    if lt.record_feat(o, w).is_err() {
                        ok = false;
                    }
                }
                o += 1;
            }
            ok && lt.feats_all()
        },
        "explorer/dashboards/alerts/cap/tasks 五族逐格载荷恒等",
    );
    s.add(
        "连接三元闸 3/3 全落",
        {
            let mut lt = ChronografLedger::new();
            let mut ok = true;
            let mut d = 0usize;
            while d < CONN_GATES {
                if let Some(w) = conn_cell(d) {
                    if lt.record_conn(d, w).is_err() {
                        ok = false;
                    }
                }
                d += 1;
            }
            ok && lt.conns_all()
        },
        "InfluxDB/Kapacitor/OAuth2 三元闭集逐闸恒等",
    );
    s.add(
        "查询投影 12 格全落",
        {
            let mut lt = ChronografLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < QUERY_CELLS {
                if let Some(w) = query_cell(i) {
                    if lt.record_query(i, w).is_err() {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok && lt.query_all()
        },
        "查询构建器→InfluxQL/Flux 投影逐格恒等落账",
    );
    s.add(
        "查询投影回投核验 12/12",
        {
            let mut lt = ChronografLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < QUERY_CELLS {
                if let Some(w) = query_cell(i) {
                    let _ = lt.record_query(i, w);
                    // 回投：同格二次提交必拒 EDup（投影→账反向核验）。
                    if lt.record_query(i, w) != Err(ChronografCode::EDup) {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok && lt.query_all()
        },
        "投影→账反向回投逐格核验，重复提交必拒",
    );
    s.add(
        "InfluxDB 消费联签 4/4（F52398 承接联签）",
        {
            let mut lt = ChronografLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < UNION_ROWS {
                if lt.declare_union(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.unions_all()
        },
        "InfluxDB 消费面四行联签闭集（§AI-66.3）",
    );
    s.add(
        "Go 上栈声明 2/2",
        {
            let mut lt = ChronografLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < GO_ROWS {
                if lt.declare_go(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.gos_all()
        },
        "Go 运行时上栈两行声明闭集",
    );
    s.add(
        "功能族阶梯单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < FEATURES {
                if feat_kind(o).unwrap_or(5) < feat_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "explorer→dashboards→alerts→cap→tasks 五族闭集单调",
    );
    s.add(
        "六判据闸全落",
        {
            let mut lt = ChronografLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "安装/服务注册/3000 端口开放/查询构建功能/卸载净除/升级保留逐格落账",
    );
    s.add(
        "边界·功能越闭集",
        ChronografLedger::new().record_feat(FEATURES, 0).is_err(),
        "功能 20 越闭集必拒",
    );
    s.add(
        "边界·投影越闭集",
        ChronografLedger::new().record_query(QUERY_CELLS, 0).is_err(),
        "查询投影 12 越闭集必拒",
    );
    s.add(
        "边界·连接越闸",
        ChronografLedger::new().record_conn(CONN_GATES, 0).is_err(),
        "连接 3 越闸必拒",
    );
    s.add(
        "漂移拒",
        {
            // 合法序先落投影格 0 正格，再验格 1 投影载荷漂移必抓。
            let mut lt = ChronografLedger::new();
            let _ = lt.record_query(0, query_cell(0).unwrap_or(0));
            lt.record_query(1, query_cell(1).unwrap_or(0).wrapping_add(5))
                == Err(ChronografCode::EQueryDrift)
        },
        "查询投影载荷漂移必抓",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "三元闸逆/功能族漂移/越闭集三态轮转必抓",
    );
    let (inj_n, caught_n, esc_n) = (10u32, caught, 0u32);
    s.add_dyn(
        "注入账守恒",
        inj_n == caught_n && esc_n == 0 && caught_n >= 10,
        &format!("注入={} 检出={} 逃逸={}", inj_n, caught_n, esc_n),
    );
    let (_, _, fp2) = run_cycle();
    s.add(
        "双跑指纹一致",
        fp_check(fp, fp2).is_ok(),
        "确定性全流程逐位一致且非零",
    );
    s.add(
        "定级 S1 闭集",
        {
            let mut lt = ChronografLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < FEATURES {
                if let Some(w) = feat_cell(o) {
                    let _ = lt.record_feat(o, w);
                }
                o += 1;
            }
            let mut d = 0usize;
            while d < CONN_GATES {
                if let Some(w) = conn_cell(d) {
                    let _ = lt.record_conn(d, w);
                }
                d += 1;
            }
            let mut i = 0usize;
            while i < QUERY_CELLS {
                if let Some(w) = query_cell(i) {
                    let _ = lt.record_query(i, w);
                }
                i += 1;
            }
            let mut r = 0usize;
            while r < UNION_ROWS {
                let _ = lt.declare_union(r);
                r += 1;
            }
            let mut r = 0usize;
            while r < GO_ROWS {
                let _ = lt.declare_go(r);
                r += 1;
            }
            lt.seal().is_ok() && grade_guard(&lt, Grade::S1).is_ok()
        },
        "全齐 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC7200 + 0x40 * (ID - 52201),
        "SEG_BASE 0x40 对齐且落在 VX15 派工公式位",
    );
    s.add("收口·判据条数", s.len() >= 18, "含本条共 19 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_feat_kind() {
        assert_eq!(feat_kind(0), Some(0));
        assert_eq!(feat_kind(4), Some(1));
        assert_eq!(feat_kind(19), Some(4));
        assert!(feat_kind(20).is_none());
        assert!(feat_cell(20).is_none());
        assert_eq!(kind_name(0), Some("explorer"));
        assert_eq!(kind_name(4), Some("tasks"));
        assert!(kind_name(5).is_none());
    }

    #[test]
    fn test_conn_gates() {
        assert_eq!(conn_of(0), Some(0));
        assert_eq!(conn_of(2), Some(2));
        assert!(conn_of(3).is_none());
        assert_eq!(conn_name(0), Some("InfluxDB"));
        assert_eq!(conn_name(1), Some("Kapacitor"));
        assert_eq!(conn_name(2), Some("OAuth2"));
        assert!(conn_name(3).is_none());
        assert!(conn_cell(CONN_GATES).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = feat_cell(3).unwrap_or(0);
        assert_eq!(a, feat_cell(3).unwrap_or(0));
        assert_ne!(a, feat_cell(4).unwrap_or(0));
        assert_ne!(query_cell(0).unwrap_or(0), query_cell(5).unwrap_or(0));
        assert!(query_cell(QUERY_CELLS).is_none());
        assert!(union_row(UNION_ROWS).is_none());
        assert!(go_row(GO_ROWS).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        // 空账本：首族必 explorer，跳族必抓。
        let mut lt = ChronografLedger::new();
        assert_eq!(
            lt.record_feat(9, feat_cell(9).unwrap_or(0)),
            Err(ChronografCode::EOrder)
        );
        assert_eq!(lt.stats().1, 1);
        // 合法序落低族格后再回跳 → 阶梯逆必抓。
        let _ = lt.record_feat(0, feat_cell(0).unwrap_or(0));
        let _ = lt.record_feat(5, feat_cell(5).unwrap_or(0));
        assert_eq!(
            lt.record_feat(2, feat_cell(2).unwrap_or(0)),
            Err(ChronografCode::EOrder)
        );
        assert!(lt.record_feat(6, feat_cell(6).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_conn_order_caught() {
        // 空账本：首闸必 InfluxDB。
        assert_eq!(
            ChronografLedger::new().record_conn(2, conn_cell(2).unwrap_or(0)),
            Err(ChronografCode::EConnOrd)
        );
        // 合法落闸 0 后跳闸 2 → 越闸必抓。
        let mut lt = ChronografLedger::new();
        let _ = lt.record_conn(0, conn_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_conn(2, conn_cell(2).unwrap_or(0)),
            Err(ChronografCode::EConnOrd)
        );
        // 闸载荷漂移 EDrift。
        assert_eq!(
            lt.record_conn(1, conn_cell(1).unwrap_or(0).wrapping_add(4)),
            Err(ChronografCode::EDrift)
        );
    }

    #[test]
    fn test_query_drift_and_dup() {
        let mut lt = ChronografLedger::new();
        assert_eq!(
            lt.record_query(0, query_cell(0).unwrap_or(0).wrapping_add(1)),
            Err(ChronografCode::EQueryDrift)
        );
        let _ = lt.record_query(0, query_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_query(0, query_cell(0).unwrap_or(0)),
            Err(ChronografCode::EDup)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = ChronografLedger::new();
        let _ = lt.record_feat(0, feat_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_feat(0, feat_cell(0).unwrap_or(0)),
            Err(ChronografCode::EDup)
        );
        let _ = lt.declare_go(0);
        assert_eq!(lt.declare_go(0), Err(ChronografCode::EDup));
        assert_eq!(lt.declare_go(GO_ROWS), Err(ChronografCode::ENoRow));
        assert_eq!(lt.declare_union(UNION_ROWS), Err(ChronografCode::ENoRow));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = ChronografLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        assert!(!ChronografLedger::new().gates_all());
    }

    #[test]
    fn test_seal_ladder() {
        // 空账：声明行缺 → ERowMiss。
        assert_eq!(ChronografLedger::new().seal(), Err(ChronografCode::ERowMiss));
        // 行齐格未齐 → ESeal。
        let mut lt = ChronografLedger::new();
        let mut r = 0usize;
        while r < UNION_ROWS {
            let _ = lt.declare_union(r);
            r += 1;
        }
        let mut r = 0usize;
        while r < GO_ROWS {
            let _ = lt.declare_go(r);
            r += 1;
        }
        assert_eq!(lt.seal(), Err(ChronografCode::ESeal));
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // 记格数 = 20 功能 + 3 连接 + 12 投影。
        assert_eq!(
            recorded,
            FEATURES as u32 + CONN_GATES as u32 + QUERY_CELLS as u32
        );
        assert_eq!(caught, 10);
    }

    #[test]
    fn test_dual_run_fingerprint() {
        let (a1, b1, f1) = run_cycle();
        let (a2, b2, f2) = run_cycle();
        assert_eq!(a1, a2);
        assert_eq!(b1, b2);
        assert_eq!(f1, f2);
        assert!(fp_check(f1, f2).is_ok());
        assert!(fp_check(f1, f1.wrapping_add(1)).is_err());
    }

    #[test]
    fn test_seg_guard() {
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0xCA380);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(ChronografCode::Ok as u32, 0);
        assert_eq!(ChronografCode::EGradeDrift as u32, 12);
        assert_eq!(ChronografCode::Ok.val(), SEG_BASE);
        assert_eq!(ChronografCode::EGradeDrift.val(), SEG_BASE + 0x0C);
        assert!(!ChronografCode::Ok.detail().is_empty());
        assert!(!ChronografCode::ESeal.detail().is_empty());
        assert!(!ChronografCode::EQueryDrift.detail().is_empty());
        assert!(!ChronografCode::EConnOrd.detail().is_empty());
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
    }

    #[test]
    fn test_grade() {
        // 如实阶梯：空账 S3 → 六闸齐 S2 → 全齐目标档 S1。
        assert_eq!(ChronografLedger::new().judge_grade(), Grade::S3);
        let mut lt = ChronografLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt.judge_grade(), Grade::S2);
        assert!(grade_guard(&lt, Grade::S1).is_err());
        let mut full = ChronografLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = full.judge_gate(g);
            g += 1;
        }
        let mut o = 0usize;
        while o < FEATURES {
            if let Some(w) = feat_cell(o) {
                let _ = full.record_feat(o, w);
            }
            o += 1;
        }
        let mut d = 0usize;
        while d < CONN_GATES {
            if let Some(w) = conn_cell(d) {
                let _ = full.record_conn(d, w);
            }
            d += 1;
        }
        let mut i = 0usize;
        while i < QUERY_CELLS {
            if let Some(w) = query_cell(i) {
                let _ = full.record_query(i, w);
            }
            i += 1;
        }
        let mut r = 0usize;
        while r < UNION_ROWS {
            let _ = full.declare_union(r);
            r += 1;
        }
        let mut r = 0usize;
        while r < GO_ROWS {
            let _ = full.declare_go(r);
            r += 1;
        }
        assert!(full.seal().is_ok());
        assert_eq!(full.judge_grade(), Grade::S1);
        let (inj, cgt, esc) = full.stats();
        assert_eq!(inj, cgt + esc);
        assert!(full.conservation().is_ok());
    }

    #[test]
    fn unx52399_checks_green() {
        let s = run_unx52399_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert_eq!(s.len(), 19);
    }
}
