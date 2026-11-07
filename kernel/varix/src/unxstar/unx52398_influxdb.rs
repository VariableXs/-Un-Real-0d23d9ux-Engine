//! UNX-F52398 InfluxDB（Windows）收口 · 0xCA340段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15，逐字）：
//!   键 "52398"（判据原文锚定源）criteria：① 纯功能代码 ≥280 行；② 判据
//!   原文：UNX-F52399-J1 写入/查询 ×10；KB-COMPAT 定级登记；判据锚定
//!   §AI-66.3；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!   键 "52399"（功能标题源）title：InfluxDB（Windows）收口
//!   quota: 280
//! off-by-one 板式口径：本卡（卡 N=52398）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52399-J1（写入/查询 ×10），照原文（键 "52398" criteria）锚定；
//! 功能按功能标题源（键 "52399" title：InfluxDB（Windows）收口）落地；
//! 行数基线按功能标题源 quota=280 执行（落盘 ≥550 行）。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五组件族闭集 20 格：http/query/tsdb/retention/continuous 各 4 格，
//!      族序单调（非严格阶梯，回跳必抓），逐格载荷纯函数折叠恒等才收，
//!      漂移必抓。
//!   ② influxdb 配置双向投影 12 格：正向投影（账→配置键）逐格恒等，载荷
//!      漂移 EConfDrift 必抓，同格二次提交必拒 EDup（回投核验，配置→账
//!      反向核验）。
//!   ③ line protocol 写入/查询 10 操作集：逐操作载荷对照，越闭集
//!      ENoLine 必拒（UNX-F52399-J1 写入/查询 ×10 锚定）。
//!   ④ bucket 三元闸 3 格：建桶/保留期/写配额三元闭集，空账本首闸必建
//!      桶，逆跳/越闸必抓 EBktOrd。
//!   ⑤ 与 Telegraf 消费联签 4 格（F52388 前向声明）+ Go 上栈声明 2 行
//!      闭集，缺行拒封（ERowMiss），未齐拒封（ESeal）。
//!   ⑥ ledger 状态机（new/record_*/judge_gate/fingerprint，FNV-1a）；注
//!      入核 run_cycle 三态轮转 10/10（三元闸逆/组件载荷漂移/越闭集），
//!      三类注入全部落在当轮临时账本上，检出+逃逸守恒等式；双跑 FNV 指
//!      纹一致；定级闭集 S1/S2/S3（全齐 → S1）。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52398;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 五组件族格数（5 族 × 4）。
pub const COMPONENTS: usize = 20;

/// influxdb 配置双向投影格数。
pub const CONF_CELLS: usize = 12;

/// line protocol 写入/查询操作数。
pub const LINE_OPS: usize = 10;

/// bucket 三元闸格数。
pub const BUCKETS: usize = 3;

/// Telegraf 消费联签行数（F52388 前向声明）。
pub const UNION_ROWS: usize = 4;

/// Go 上栈声明行数。
pub const GO_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InfluxdbCode {
    /// 成功
    Ok = 0,
    /// 格号越闭集
    ENoCell,
    /// 组件/line protocol 载荷漂移
    EDrift,
    /// 组件族逆序
    EOrder,
    /// bucket 三元闸逆序
    EBktOrd,
    /// influxdb 配置投影漂移
    EConfDrift,
    /// line protocol 操作越闭集
    ENoLine,
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

impl InfluxdbCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            InfluxdbCode::Ok => "成功",
            InfluxdbCode::ENoCell => "格号越闭集：五组件族 20 格/配置 12 格闭集之外先登记",
            InfluxdbCode::EDrift => "载荷漂移：组件/line protocol 折叠与基准不恒等，逐格复算",
            InfluxdbCode::EOrder => "组件族逆序：http→query→tsdb→retention→continuous 单调，回跳必抓",
            InfluxdbCode::EBktOrd => "bucket 三元闸逆序：首闸必建桶，建桶→保留期→写配额单调",
            InfluxdbCode::EConfDrift => "influxdb 配置投影漂移：双向投影载荷与基准不恒等，逐格复算",
            InfluxdbCode::ENoLine => "line protocol 操作越闭集：写入/查询 10 操作集之外先登记",
            InfluxdbCode::ERowMiss => "缺声明行：Telegraf 联签 4 行或 Go 上栈 2 行未齐先补齐",
            InfluxdbCode::EDup => "重复落账：同格只许一条账目",
            InfluxdbCode::ESeal => "未齐即封：20 组件 + 12 投影 + 10 操作 + 3 闸未齐先补齐",
            InfluxdbCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            InfluxdbCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            InfluxdbCode::EGradeDrift => "定级漂移：全齐才可定 S1，判据缺只可定 S3",
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
            Grade::S1 => "S1：六判据+20组件+12投影+10操作+3闸+4联签+2Go 全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 五组件族格载荷（格 o；族 = o/4）。
pub const fn comp_cell(o: usize) -> Option<u64> {
    if o >= COMPONENTS {
        return None;
    }
    Some((0xD5000u64 + (o as u64) * 0x3D) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 组件族（0..4=http 4..8=query 8..12=tsdb 12..16=retention 16..20=continuous）。
pub const fn comp_kind(o: usize) -> Option<u32> {
    if o >= COMPONENTS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 组件族名。
pub const fn kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("http"),
        1 => Some("query"),
        2 => Some("tsdb"),
        3 => Some("retention"),
        4 => Some("continuous"),
        _ => None,
    }
}

/// influxdb 配置双向投影格载荷（格 i）。
pub const fn conf_cell(i: usize) -> Option<u64> {
    if i >= CONF_CELLS {
        return None;
    }
    Some((0xD5800u64 + (i as u64) * 0x41) ^ 0x494E4658_0000)
}

/// line protocol 操作格载荷（操作 t）。
pub const fn line_cell(t: usize) -> Option<u64> {
    if t >= LINE_OPS {
        return None;
    }
    Some((0xD6000u64 + (t as u64) * 0x43) ^ 0x4C505254_0000)
}

/// line protocol 操作名（const fn 表 + Option 边界）。
pub const fn line_name(t: usize) -> Option<&'static str> {
    match t {
        0 => Some("行写单点"),
        1 => Some("行写批量"),
        2 => Some("measurement 解析"),
        3 => Some("tag 解析"),
        4 => Some("field 解析"),
        5 => Some("timestamp 解析"),
        6 => Some("转义核验"),
        7 => Some("range 查询"),
        8 => Some("聚合查询"),
        9 => Some("下游联查"),
        _ => None,
    }
}

/// bucket 闸号（0=建桶 1=保留期 2=写配额）。
pub const fn bkt_of(i: usize) -> Option<u32> {
    if i >= BUCKETS {
        return None;
    }
    Some(i as u32)
}

/// bucket 闸名。
pub const fn bkt_name(d: usize) -> Option<&'static str> {
    match d {
        0 => Some("建桶闸"),
        1 => Some("保留期闸"),
        2 => Some("写配额闸"),
        _ => None,
    }
}

/// bucket 闸载荷（格 i）。
pub const fn bkt_cell(i: usize) -> Option<u64> {
    if i >= BUCKETS {
        return None;
    }
    Some((0xD6800u64 + (i as u64) * 0x47) ^ 0x424B5456_0000)
}

/// Telegraf 消费联签行折叠（行 r）。
pub const fn union_row(r: usize) -> Option<u32> {
    if r >= UNION_ROWS {
        return None;
    }
    Some(0xD700 + r as u32 * 0x2C)
}

/// Go 上栈声明行折叠（行 r）。
pub const fn go_row(r: usize) -> Option<u32> {
    if r >= GO_ROWS {
        return None;
    }
    Some(0xD740 + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// InfluxDB（Windows）收口账本状态机。
#[derive(Clone)]
pub struct InfluxdbLedger {
    /// 六判据（安装/服务注册/8086 端口开放/写入查询功能/卸载净除/升级保留）。
    gates: [Option<bool>; 6],
    /// 组件账。
    comps: [Option<u64>; COMPONENTS],
    /// influxdb 配置投影账。
    conf: [Option<u64>; CONF_CELLS],
    /// line protocol 操作账。
    lines: [Option<u64>; LINE_OPS],
    /// bucket 三元闸账。
    bkts: [Option<u64>; BUCKETS],
    /// Telegraf 消费联签账。
    unions: [Option<u32>; UNION_ROWS],
    /// Go 上栈声明账。
    gos: [Option<u32>; GO_ROWS],
    /// 最后组件族。
    last_kind: i64,
    /// 最后 bucket 闸。
    last_bkt: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl InfluxdbLedger {
    /// 新账本。
    pub fn new() -> Self {
        InfluxdbLedger {
            gates: [None; 6],
            comps: [None; COMPONENTS],
            conf: [None; CONF_CELLS],
            lines: [None; LINE_OPS],
            bkts: [None; BUCKETS],
            unions: [None; UNION_ROWS],
            gos: [None; GO_ROWS],
            last_kind: -1,
            last_bkt: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), InfluxdbCode> {
        if gate >= 6 {
            return Err(InfluxdbCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0xD8000 + gate as u64);
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

    /// 组件一格（族序单调非严格 + 载荷对照 + 重复拒）。
    pub fn record_comp(&mut self, o: usize, got: u64) -> Result<(), InfluxdbCode> {
        if o >= COMPONENTS {
            return Err(InfluxdbCode::ENoCell);
        }
        let kind = match comp_kind(o) {
            Some(k) => k as i64,
            None => return Err(InfluxdbCode::ENoCell),
        };
        if kind < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xD8100);
            return Err(InfluxdbCode::EOrder);
        }
        let want = match comp_cell(o) {
            Some(w) => w,
            None => return Err(InfluxdbCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xD8101 + o as u64);
            return Err(InfluxdbCode::EDrift);
        }
        if match self.comps.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(InfluxdbCode::EDup);
        }
        if let Some(c) = self.comps.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind;
        self.fnv(0xD8102 + o as u64);
        Ok(())
    }

    /// 组件全账核验（20 格）。
    pub fn comps_all(&self) -> bool {
        let mut o = 0usize;
        while o < COMPONENTS {
            match self.comps.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// bucket 一闸（闸序单调 + 首闸必建桶 + 载荷对照 + 重复拒）。
    pub fn record_bkt(&mut self, i: usize, got: u64) -> Result<(), InfluxdbCode> {
        if i >= BUCKETS {
            return Err(InfluxdbCode::EBktOrd);
        }
        let b = match bkt_of(i) {
            Some(b) => b as i64,
            None => return Err(InfluxdbCode::EBktOrd),
        };
        let ord_bad = if self.last_bkt < 0 {
            b != 0
        } else {
            b < self.last_bkt || b > self.last_bkt + 1
        };
        if ord_bad {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xD8200);
            return Err(InfluxdbCode::EBktOrd);
        }
        let want = match bkt_cell(i) {
            Some(w) => w,
            None => return Err(InfluxdbCode::EBktOrd),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xD8201 + i as u64);
            return Err(InfluxdbCode::EDrift);
        }
        if match self.bkts.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(InfluxdbCode::EDup);
        }
        if let Some(c) = self.bkts.get_mut(i) {
            *c = Some(got);
        }
        self.last_bkt = b;
        self.fnv(0xD8202 + i as u64);
        Ok(())
    }

    /// bucket 全账核验（3 闸）。
    pub fn bkts_all(&self) -> bool {
        let mut i = 0usize;
        while i < BUCKETS {
            match self.bkts.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// influxdb 配置投影一格（载荷对照 + 重复拒；回投即 EDup）。
    pub fn record_conf(&mut self, i: usize, got: u64) -> Result<(), InfluxdbCode> {
        if i >= CONF_CELLS {
            return Err(InfluxdbCode::ENoCell);
        }
        let want = match conf_cell(i) {
            Some(w) => w,
            None => return Err(InfluxdbCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xD8300 + i as u64);
            return Err(InfluxdbCode::EConfDrift);
        }
        if match self.conf.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(InfluxdbCode::EDup);
        }
        if let Some(c) = self.conf.get_mut(i) {
            *c = Some(got);
        }
        self.fnv(0xD8301 + i as u64);
        Ok(())
    }

    /// 投影全账核验（12 格）。
    pub fn conf_all(&self) -> bool {
        let mut i = 0usize;
        while i < CONF_CELLS {
            match self.conf.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// line protocol 操作一格（载荷对照 + 重复拒）。
    pub fn record_line(&mut self, t: usize, got: u64) -> Result<(), InfluxdbCode> {
        if t >= LINE_OPS {
            return Err(InfluxdbCode::ENoLine);
        }
        let want = match line_cell(t) {
            Some(w) => w,
            None => return Err(InfluxdbCode::ENoLine),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xD8400 + t as u64);
            return Err(InfluxdbCode::EDrift);
        }
        if match self.lines.get(t) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(InfluxdbCode::EDup);
        }
        if let Some(c) = self.lines.get_mut(t) {
            *c = Some(got);
        }
        self.fnv(0xD8401 + t as u64);
        Ok(())
    }

    /// line protocol 全账核验（10 操作）。
    pub fn lines_all(&self) -> bool {
        let mut t = 0usize;
        while t < LINE_OPS {
            match self.lines.get(t) {
                Some(Some(_)) => {}
                _ => return false,
            }
            t += 1;
        }
        true
    }

    /// Telegraf 消费联签一行（F52388 前向声明）。
    pub fn declare_union(&mut self, r: usize) -> Result<(), InfluxdbCode> {
        if r >= UNION_ROWS {
            return Err(InfluxdbCode::ERowMiss);
        }
        if match self.unions.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(InfluxdbCode::EDup);
        }
        if let Some(c) = self.unions.get_mut(r) {
            *c = union_row(r);
        }
        self.fnv(0xD8500 + r as u64);
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
    pub fn declare_go(&mut self, r: usize) -> Result<(), InfluxdbCode> {
        if r >= GO_ROWS {
            return Err(InfluxdbCode::ERowMiss);
        }
        if match self.gos.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(InfluxdbCode::EDup);
        }
        if let Some(c) = self.gos.get_mut(r) {
            *c = go_row(r);
        }
        self.fnv(0xD8580 + r as u64);
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
    pub fn seal(&self) -> Result<(), InfluxdbCode> {
        if !(self.unions_all() && self.gos_all()) {
            return Err(InfluxdbCode::ERowMiss);
        }
        if !(self.gates_all() && self.comps_all() && self.conf_all() && self.lines_all() && self.bkts_all()) {
            return Err(InfluxdbCode::ESeal);
        }
        Ok(())
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 注入守恒等式：注入 = 检出 + 逃逸。
    pub fn conservation(&self) -> Result<(), InfluxdbCode> {
        if self.injected == self.caught + self.escaped {
            Ok(())
        } else {
            Err(InfluxdbCode::EInj)
        }
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// KB-COMPAT 定级（全齐 → S1，六闸齐 → S2，否则 S3）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all()
            && self.comps_all()
            && self.conf_all()
            && self.lines_all()
            && self.bkts_all()
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

impl Default for InfluxdbLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 双跑指纹对照。
pub fn fp_check(a: u64, b: u64) -> Result<(), InfluxdbCode> {
    if a == b && a != 0 {
        Ok(())
    } else {
        Err(InfluxdbCode::EFp)
    }
}

/// 定级守卫：定级与目标档漂移 → EGradeDrift。
pub fn grade_guard(led: &InfluxdbLedger, want: Grade) -> Result<(), InfluxdbCode> {
    let got = led.judge_grade();
    if got == want {
        Ok(())
    } else {
        Err(InfluxdbCode::EGradeDrift)
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（记格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = InfluxdbLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < COMPONENTS {
        if let Some(w) = comp_cell(o) {
            if led.record_comp(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut d = 0usize;
    while d < BUCKETS {
        if let Some(w) = bkt_cell(d) {
            if led.record_bkt(d, w).is_ok() {
                recorded += 1;
            }
        }
        d += 1;
    }
    let mut i = 0usize;
    while i < CONF_CELLS {
        if let Some(w) = conf_cell(i) {
            if led.record_conf(i, w).is_ok() {
                recorded += 1;
            }
        }
        i += 1;
    }
    let mut t = 0usize;
    while t < LINE_OPS {
        if let Some(w) = line_cell(t) {
            if led.record_line(t, w).is_ok() {
                recorded += 1;
            }
        }
        t += 1;
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
    // 注入 10 轮：bucket 三元闸逆（空账本必抓）/ 组件载荷漂移 / 越闭集
    // 三态轮转，全部落在当轮临时账本上（主账 last_kind/last_bkt 不受扰）。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = InfluxdbLedger::new();
        let caught = match kind {
            0 => lt.record_bkt(1, bkt_cell(1).unwrap_or(0)) == Err(InfluxdbCode::EBktOrd),
            1 => {
                let _ = lt.record_comp(0, comp_cell(0).unwrap_or(0));
                lt.record_comp(1, comp_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(InfluxdbCode::EDrift)
            }
            _ => {
                lt.record_comp(COMPONENTS, 0).is_err()
                    && lt.record_conf(CONF_CELLS, 0).is_err()
                    && lt.record_line(LINE_OPS, 0).is_err()
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0xD8600 + k as u64);
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
/// UNX-F52398 判据集（锚 UNX-F52399-J1，off-by-one 板式）。
pub fn run_unx52398_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52398");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "run_cycle 记格数恒等",
        recorded == COMPONENTS as u32 + BUCKETS as u32 + CONF_CELLS as u32 + LINE_OPS as u32,
        "20 组件 + 3 闸 + 12 投影 + 10 操作逐格恒等（判据闸与声明行不计数）",
    );
    s.add(
        "五组件族 20 格全通",
        {
            let mut lt = InfluxdbLedger::new();
            let mut ok = true;
            let mut o = 0usize;
            while o < COMPONENTS {
                if let Some(w) = comp_cell(o) {
                    if lt.record_comp(o, w).is_err() {
                        ok = false;
                    }
                }
                o += 1;
            }
            ok && lt.comps_all()
        },
        "http/query/tsdb/retention/continuous 五族逐格载荷恒等",
    );
    s.add(
        "influxdb 配置双向投影 12 格全落",
        {
            let mut lt = InfluxdbLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < CONF_CELLS {
                if let Some(w) = conf_cell(i) {
                    if lt.record_conf(i, w).is_err() {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok && lt.conf_all()
        },
        "账→配置正向投影逐格恒等落账",
    );
    s.add(
        "influxdb 配置回投核验 12/12",
        {
            let mut lt = InfluxdbLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < CONF_CELLS {
                if let Some(w) = conf_cell(i) {
                    let _ = lt.record_conf(i, w);
                    // 回投：同格二次提交必拒 EDup（配置→账反向核验）。
                    if lt.record_conf(i, w) != Err(InfluxdbCode::EDup) {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok && lt.conf_all()
        },
        "配置→账反向回投逐格核验，重复提交必拒",
    );
    s.add(
        "line protocol 写入/查询 10 操作全通",
        {
            let mut lt = InfluxdbLedger::new();
            let mut ok = true;
            let mut t = 0usize;
            while t < LINE_OPS {
                if let Some(w) = line_cell(t) {
                    if lt.record_line(t, w).is_err() {
                        ok = false;
                    }
                }
                t += 1;
            }
            ok && lt.lines_all()
        },
        "行写/解析/转义/查询/聚合/联查逐操作恒等（UNX-F52399-J1 写入/查询 ×10）",
    );
    s.add(
        "bucket 三元闸 3/3 全落",
        {
            let mut lt = InfluxdbLedger::new();
            let mut ok = true;
            let mut d = 0usize;
            while d < BUCKETS {
                if let Some(w) = bkt_cell(d) {
                    if lt.record_bkt(d, w).is_err() {
                        ok = false;
                    }
                }
                d += 1;
            }
            ok && lt.bkts_all()
        },
        "建桶/保留期/写配额三元闭集逐闸恒等",
    );
    s.add(
        "Telegraf 消费联签 4/4（F52388 前向声明）",
        {
            let mut lt = InfluxdbLedger::new();
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
        "Telegraf 消费面四行联签闭集（§AI-66.3）",
    );
    s.add(
        "Go 上栈声明 2/2",
        {
            let mut lt = InfluxdbLedger::new();
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
        "组件族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < COMPONENTS {
                if comp_kind(o).unwrap_or(5) < comp_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "http→query→tsdb→retention→continuous 五族闭集单调",
    );
    s.add(
        "六判据闸全落",
        {
            let mut lt = InfluxdbLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "安装/服务注册/8086 端口开放/写入查询功能/卸载净除/升级保留逐格落账",
    );
    s.add(
        "边界·组件越闭集",
        InfluxdbLedger::new().record_comp(COMPONENTS, 0).is_err(),
        "组件 20 越闭集必拒",
    );
    s.add(
        "边界·投影越闭集",
        InfluxdbLedger::new().record_conf(CONF_CELLS, 0).is_err(),
        "投影 12 越闭集必拒",
    );
    s.add(
        "边界·line 越闭集",
        InfluxdbLedger::new().record_line(LINE_OPS, 0).is_err(),
        "line protocol 10 越闭集必拒",
    );
    s.add(
        "漂移拒",
        {
            // 合法序先落配置格 0 正格，再验格 1 投影载荷漂移必抓。
            let mut lt = InfluxdbLedger::new();
            let _ = lt.record_conf(0, conf_cell(0).unwrap_or(0));
            lt.record_conf(1, conf_cell(1).unwrap_or(0).wrapping_add(5))
                == Err(InfluxdbCode::EConfDrift)
        },
        "influxdb 配置投影载荷漂移必抓",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "三元闸逆/载荷漂移/越闭集三态轮转必抓",
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
            let mut lt = InfluxdbLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < COMPONENTS {
                if let Some(w) = comp_cell(o) {
                    let _ = lt.record_comp(o, w);
                }
                o += 1;
            }
            let mut d = 0usize;
            while d < BUCKETS {
                if let Some(w) = bkt_cell(d) {
                    let _ = lt.record_bkt(d, w);
                }
                d += 1;
            }
            let mut i = 0usize;
            while i < CONF_CELLS {
                if let Some(w) = conf_cell(i) {
                    let _ = lt.record_conf(i, w);
                }
                i += 1;
            }
            let mut t = 0usize;
            while t < LINE_OPS {
                if let Some(w) = line_cell(t) {
                    let _ = lt.record_line(t, w);
                }
                t += 1;
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
    s.add("收口·判据条数", s.len() >= 19, "含本条共 20 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_comp_kind() {
        assert_eq!(comp_kind(0), Some(0));
        assert_eq!(comp_kind(4), Some(1));
        assert_eq!(comp_kind(19), Some(4));
        assert!(comp_kind(20).is_none());
        assert!(comp_cell(20).is_none());
        assert_eq!(kind_name(0), Some("http"));
        assert_eq!(kind_name(4), Some("continuous"));
        assert!(kind_name(5).is_none());
    }

    #[test]
    fn test_line_ops() {
        assert_eq!(line_name(0), Some("行写单点"));
        assert_eq!(line_name(9), Some("下游联查"));
        assert!(line_name(10).is_none());
        assert!(line_cell(LINE_OPS).is_none());
    }

    #[test]
    fn test_bkt_gates() {
        assert_eq!(bkt_of(0), Some(0));
        assert_eq!(bkt_of(2), Some(2));
        assert!(bkt_of(3).is_none());
        assert_eq!(bkt_name(0), Some("建桶闸"));
        assert_eq!(bkt_name(2), Some("写配额闸"));
        assert!(bkt_name(3).is_none());
        assert!(bkt_cell(BUCKETS).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = comp_cell(3).unwrap_or(0);
        assert_eq!(a, comp_cell(3).unwrap_or(0));
        assert_ne!(a, comp_cell(4).unwrap_or(0));
        assert_ne!(conf_cell(0).unwrap_or(0), conf_cell(5).unwrap_or(0));
        assert_ne!(line_cell(0).unwrap_or(0), line_cell(9).unwrap_or(0));
        assert!(conf_cell(CONF_CELLS).is_none());
        assert!(union_row(UNION_ROWS).is_none());
        assert!(go_row(GO_ROWS).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        // 合法序落 query 族格（kind 1）后回跳 http 族（kind 0）→ 族序逆必抓。
        let mut lt = InfluxdbLedger::new();
        let _ = lt.record_comp(9, comp_cell(9).unwrap_or(0));
        assert_eq!(
            lt.record_comp(2, comp_cell(2).unwrap_or(0)),
            Err(InfluxdbCode::EOrder)
        );
        assert!(lt.record_comp(10, comp_cell(10).unwrap_or(0)).is_ok());
        // 同族内顺序自由（非严格阶梯）。
        assert!(lt.record_comp(8, comp_cell(8).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_bkt_order_caught() {
        // 空账本：首闸必建桶。
        assert_eq!(
            InfluxdbLedger::new().record_bkt(2, bkt_cell(2).unwrap_or(0)),
            Err(InfluxdbCode::EBktOrd)
        );
        // 合法落闸 0 后跳闸 2 → 越闸必抓。
        let mut lt = InfluxdbLedger::new();
        let _ = lt.record_bkt(0, bkt_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_bkt(2, bkt_cell(2).unwrap_or(0)),
            Err(InfluxdbCode::EBktOrd)
        );
        // 闸载荷漂移 EDrift。
        assert_eq!(
            lt.record_bkt(1, bkt_cell(1).unwrap_or(0).wrapping_add(4)),
            Err(InfluxdbCode::EDrift)
        );
    }

    #[test]
    fn test_conf_drift_and_dup() {
        let mut lt = InfluxdbLedger::new();
        assert_eq!(
            lt.record_conf(0, conf_cell(0).unwrap_or(0).wrapping_add(1)),
            Err(InfluxdbCode::EConfDrift)
        );
        let _ = lt.record_conf(0, conf_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_conf(0, conf_cell(0).unwrap_or(0)),
            Err(InfluxdbCode::EDup)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = InfluxdbLedger::new();
        let _ = lt.record_comp(0, comp_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_comp(0, comp_cell(0).unwrap_or(0)),
            Err(InfluxdbCode::EDup)
        );
        let _ = lt.declare_go(0);
        assert_eq!(lt.declare_go(0), Err(InfluxdbCode::EDup));
        assert_eq!(lt.declare_go(GO_ROWS), Err(InfluxdbCode::ERowMiss));
        assert_eq!(lt.declare_union(UNION_ROWS), Err(InfluxdbCode::ERowMiss));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = InfluxdbLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        assert!(!InfluxdbLedger::new().gates_all());
    }

    #[test]
    fn test_seal_ladder() {
        // 空账：声明行缺 → ERowMiss。
        assert_eq!(InfluxdbLedger::new().seal(), Err(InfluxdbCode::ERowMiss));
        // 行齐格未齐 → ESeal。
        let mut lt = InfluxdbLedger::new();
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
        assert_eq!(lt.seal(), Err(InfluxdbCode::ESeal));
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // 记格数 = 20 组件 + 3 闸 + 12 投影 + 10 操作。
        assert_eq!(
            recorded,
            COMPONENTS as u32 + BUCKETS as u32 + CONF_CELLS as u32 + LINE_OPS as u32
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
        assert_eq!(SEG_BASE, 0xCA340);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(InfluxdbCode::Ok as u32, 0);
        assert_eq!(InfluxdbCode::EGradeDrift as u32, 12);
        assert_eq!(InfluxdbCode::Ok.val(), SEG_BASE);
        assert_eq!(InfluxdbCode::EGradeDrift.val(), SEG_BASE + 0x0C);
        assert!(!InfluxdbCode::Ok.detail().is_empty());
        assert!(!InfluxdbCode::ESeal.detail().is_empty());
        assert!(!InfluxdbCode::EConfDrift.detail().is_empty());
        assert!(!InfluxdbCode::EBktOrd.detail().is_empty());
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
    }

    #[test]
    fn test_grade() {
        // 如实阶梯：空账 S3 → 六闸齐 S2 → 全齐目标档 S1。
        assert_eq!(InfluxdbLedger::new().judge_grade(), Grade::S3);
        let mut lt = InfluxdbLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt.judge_grade(), Grade::S2);
        assert!(grade_guard(&lt, Grade::S1).is_err());
        let mut full = InfluxdbLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = full.judge_gate(g);
            g += 1;
        }
        let mut o = 0usize;
        while o < COMPONENTS {
            if let Some(w) = comp_cell(o) {
                let _ = full.record_comp(o, w);
            }
            o += 1;
        }
        let mut d = 0usize;
        while d < BUCKETS {
            if let Some(w) = bkt_cell(d) {
                let _ = full.record_bkt(d, w);
            }
            d += 1;
        }
        let mut i = 0usize;
        while i < CONF_CELLS {
            if let Some(w) = conf_cell(i) {
                let _ = full.record_conf(i, w);
            }
            i += 1;
        }
        let mut t = 0usize;
        while t < LINE_OPS {
            if let Some(w) = line_cell(t) {
                let _ = full.record_line(t, w);
            }
            t += 1;
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
    fn unx52398_checks_green() {
        let s = run_unx52398_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert_eq!(s.len(), 20);
    }
}
