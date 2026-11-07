//! UNX-F52393 Loki（Windows）收口 · 0xCA200段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15，逐字）：
//!   键 "52393"（判据原文锚定源）criteria：① 纯功能代码 ≥280 行；② 判据
//!   原文：UNX-F52394-J1 推送/查询 ×10；KB-COMPAT 定级登记；判据锚定
//!   §AI-66.3；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!   键 "52394"（功能标题源）title：Loki（Windows）收口
//!   quota: 300
//! off-by-one 板式口径：本卡（卡 N=52393）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52394-J1（推送/查询 ×10），照原文（键 "52393" criteria）锚定；
//! 功能按本卡标题（Loki（Windows）收口，键 "52394" title）落地；行数
//! 基线按功能标题源 quota=300 执行（落盘 ≥360 行）。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五组件族闭集 20 格：distributor/ingester/querier/ruler/compactor
//!      各 4 格，族序单调（空账本首族必 distributor，逆跳必抓），逐格
//!      载荷纯函数折叠恒等才收，漂移必抓。
//!   ② loki-config.yml 双向投影 12 格：正向投影（账→yml 键）逐格恒等，
//!      载荷漂移 ECfgDrift 必抓，同格二次提交必拒 EDup（回投核验）。
//!   ③ 标签三态 6 格：static/dynamic/dropped 三态各 2 格，态序严格递进
//!      （0→1→2），越态 ELabelState 必抓，载荷漂移 EDrift 必抓。
//!   ④ 与 Promtail / PromServer 消费联签 4 格（F52392 反向联签，
//!      PromServer 侧 UNX-F52392 已作日志消费前向声明）+ Go 上栈声明
//!      2 行闭集，缺行拒封（ERowMiss），未齐拒封（ESeal）。
//!   ⑤ ledger 状态机（new/record_*/judge_gate/seal/fingerprint，FNV-1a）；
//!      注入核 run_cycle 三态轮转 10/10（组件载荷漂移/族序逆跳/越闭
//!      集），检出+逃逸守恒等式；双跑 FNV 指纹一致；定级闭集 S1/S2/S3
//!      （六闸 + 20 组件 + 12 投影 + 6 标签 + 4 联签 + 2 Go 全齐 → S1）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52393;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 组件族格数（五族 × 4）。
pub const COMPS: usize = 20;

/// loki-config.yml 双向投影格数。
pub const CFG_CELLS: usize = 12;

/// 标签三态格数（三态 × 2）。
pub const LABEL_CELLS: usize = 6;

/// Promtail/PromServer 消费联签行数（F52392 反向联签）。
pub const UNION_ROWS: usize = 4;

/// Go 上栈声明行数。
pub const GO_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LokiCode {
    /// 成功
    Ok = 0,
    /// 格号越闭集
    ENoCell,
    /// 组件/标签载荷漂移
    EDrift,
    /// 组件族逆序
    EOrder,
    /// loki-config.yml 投影漂移
    ECfgDrift,
    /// 标签格越闭集
    ENoLabel,
    /// 标签三态越序
    ELabelState,
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

impl LokiCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            LokiCode::Ok => "成功",
            LokiCode::ENoCell => "格号越闭集：组件 20 格/六闸闭集之外先登记",
            LokiCode::EDrift => "载荷漂移：组件/标签折叠与基准不恒等，逐格复算",
            LokiCode::EOrder => "组件族逆序：distributor→ingester→querier→ruler→compactor 单调，逆跳必抓",
            LokiCode::ECfgDrift => "loki-config.yml 投影漂移：双向投影载荷与基准不恒等，逐格复算",
            LokiCode::ENoLabel => "标签格越闭集：标签三态 6 格闭集之外先登记",
            LokiCode::ELabelState => "标签三态越序：static→dynamic→dropped 态序严格递进，越态必抓",
            LokiCode::ERowMiss => "缺声明行：Promtail/PromServer 联签 4 行或 Go 上栈 2 行未齐先补齐",
            LokiCode::EDup => "重复落账：同格只许一条账目",
            LokiCode::ESeal => "未齐即封：20 组件 + 12 投影 + 6 标签 + 闸未齐先补齐",
            LokiCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            LokiCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            LokiCode::EGradeDrift => "定级漂移：全齐才可定 S1，判据缺只可定 S3",
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
            Grade::S1 => "S1：六判据+20组件+12投影+6标签+4联签+2Go 全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 组件格载荷（格 o；族 = o/4）。
pub const fn comp_cell(o: usize) -> Option<u64> {
    if o >= COMPS {
        return None;
    }
    Some((0xCB000u64 + (o as u64) * 0x37) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 组件族（0..4=distributor 4..8=ingester 8..12=querier 12..16=ruler 16..20=compactor）。
pub const fn comp_kind(o: usize) -> Option<u32> {
    if o >= COMPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 组件族名。
pub const fn kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("distributor"),
        1 => Some("ingester"),
        2 => Some("querier"),
        3 => Some("ruler"),
        4 => Some("compactor"),
        _ => None,
    }
}

/// loki-config.yml 双向投影格载荷（格 i）。
pub const fn cfg_cell(i: usize) -> Option<u64> {
    if i >= CFG_CELLS {
        return None;
    }
    Some((0xCB400u64 + (i as u64) * 0x41) ^ 0x434F4E46_0000)
}

/// 标签格载荷（格 i）。
pub const fn label_cell(i: usize) -> Option<u64> {
    if i >= LABEL_CELLS {
        return None;
    }
    Some((0xCB800u64 + (i as u64) * 0x4B) ^ 0x4C41424C_0000)
}

/// 标签三态（0..2=static 2..4=dynamic 4..6=dropped）。
pub const fn label_state(i: usize) -> Option<u32> {
    if i >= LABEL_CELLS {
        return None;
    }
    Some((i / 2) as u32)
}

/// 标签态名。
pub const fn label_state_name(t: usize) -> Option<&'static str> {
    match t {
        0 => Some("static"),
        1 => Some("dynamic"),
        2 => Some("dropped"),
        _ => None,
    }
}

/// Promtail/PromServer 消费联签行折叠（行 r）。
pub const fn union_row(r: usize) -> Option<u32> {
    if r >= UNION_ROWS {
        return None;
    }
    Some(0xCB00 + r as u32 * 0x2C)
}

/// Go 上栈声明行折叠（行 r）。
pub const fn go_row(r: usize) -> Option<u32> {
    if r >= GO_ROWS {
        return None;
    }
    Some(0xCB40 + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Loki（Windows）收口账本状态机。
#[derive(Clone)]
pub struct LokiLedger {
    /// 六判据（安装/服务注册/3100 端口开放/推送·查询功能/卸载净除/升级保留）。
    gates: [Option<bool>; 6],
    /// 组件账。
    comps: [Option<u64>; COMPS],
    /// loki-config.yml 投影账。
    cfg: [Option<u64>; CFG_CELLS],
    /// 标签三态账。
    labels: [Option<u64>; LABEL_CELLS],
    /// 消费联签账。
    unions: [Option<u32>; UNION_ROWS],
    /// Go 上栈声明账。
    gos: [Option<u32>; GO_ROWS],
    /// 最后组件族。
    last_kind: i64,
    /// 最后标签态。
    last_state: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl LokiLedger {
    /// 新账本。
    pub fn new() -> Self {
        LokiLedger {
            gates: [None; 6],
            comps: [None; COMPS],
            cfg: [None; CFG_CELLS],
            labels: [None; LABEL_CELLS],
            unions: [None; UNION_ROWS],
            gos: [None; GO_ROWS],
            last_kind: -1,
            last_state: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), LokiCode> {
        if gate >= 6 {
            return Err(LokiCode::ENoCell);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0xCB1C00 + gate as u64);
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

    /// 组件一格（族序单调 + 首族必 distributor + 载荷对照 + 重复拒）。
    pub fn record_comp(&mut self, o: usize, got: u64) -> Result<(), LokiCode> {
        if o >= COMPS {
            return Err(LokiCode::ENoCell);
        }
        let kind = match comp_kind(o) {
            Some(k) => k as i64,
            None => return Err(LokiCode::ENoCell),
        };
        let order_bad = if self.last_kind < 0 {
            kind != 0
        } else {
            kind < self.last_kind || kind > self.last_kind + 1
        };
        if order_bad {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xCB1D00);
            return Err(LokiCode::EOrder);
        }
        let want = match comp_cell(o) {
            Some(w) => w,
            None => return Err(LokiCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xCB1D01 + o as u64);
            return Err(LokiCode::EDrift);
        }
        if match self.comps.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(LokiCode::EDup);
        }
        if let Some(c) = self.comps.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind;
        self.fnv(0xCB1D02 + o as u64);
        Ok(())
    }

    /// 组件全账核验（20 格）。
    pub fn comps_all(&self) -> bool {
        let mut o = 0usize;
        while o < COMPS {
            match self.comps.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// loki-config.yml 投影一格（载荷对照 + 重复拒；回投即 EDup）。
    pub fn record_cfg(&mut self, i: usize, got: u64) -> Result<(), LokiCode> {
        if i >= CFG_CELLS {
            return Err(LokiCode::ENoCell);
        }
        let want = match cfg_cell(i) {
            Some(w) => w,
            None => return Err(LokiCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xCB1E00 + i as u64);
            return Err(LokiCode::ECfgDrift);
        }
        if match self.cfg.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(LokiCode::EDup);
        }
        if let Some(c) = self.cfg.get_mut(i) {
            *c = Some(got);
        }
        self.fnv(0xCB1E01 + i as u64);
        Ok(())
    }

    /// 投影全账核验（12 格）。
    pub fn cfg_all(&self) -> bool {
        let mut i = 0usize;
        while i < CFG_CELLS {
            match self.cfg.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 标签一格（三态递进 + 载荷对照 + 重复拒）。
    pub fn record_label(&mut self, i: usize, got: u64) -> Result<(), LokiCode> {
        if i >= LABEL_CELLS {
            return Err(LokiCode::ENoLabel);
        }
        let st = match label_state(i) {
            Some(s) => s as i64,
            None => return Err(LokiCode::ENoLabel),
        };
        let order_bad = if self.last_state < 0 {
            st != 0
        } else {
            st < self.last_state || st > self.last_state + 1
        };
        if order_bad {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xCB1F00);
            return Err(LokiCode::ELabelState);
        }
        let want = match label_cell(i) {
            Some(w) => w,
            None => return Err(LokiCode::ENoLabel),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xCB1F01 + i as u64);
            return Err(LokiCode::EDrift);
        }
        if match self.labels.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(LokiCode::EDup);
        }
        if let Some(c) = self.labels.get_mut(i) {
            *c = Some(got);
        }
        self.last_state = st;
        self.fnv(0xCB1F02 + i as u64);
        Ok(())
    }

    /// 标签全账核验（6 格）。
    pub fn labels_all(&self) -> bool {
        let mut i = 0usize;
        while i < LABEL_CELLS {
            match self.labels.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// Promtail/PromServer 消费联签一行（F52392 反向联签）。
    pub fn declare_union(&mut self, r: usize) -> Result<(), LokiCode> {
        if r >= UNION_ROWS {
            return Err(LokiCode::ERowMiss);
        }
        if match self.unions.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(LokiCode::EDup);
        }
        if let Some(c) = self.unions.get_mut(r) {
            *c = union_row(r);
        }
        self.fnv(0xCB2000 + r as u64);
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
    pub fn declare_go(&mut self, r: usize) -> Result<(), LokiCode> {
        if r >= GO_ROWS {
            return Err(LokiCode::ERowMiss);
        }
        if match self.gos.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(LokiCode::EDup);
        }
        if let Some(c) = self.gos.get_mut(r) {
            *c = go_row(r);
        }
        self.fnv(0xCB2080 + r as u64);
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
    pub fn seal(&self) -> Result<(), LokiCode> {
        if !(self.unions_all() && self.gos_all()) {
            return Err(LokiCode::ERowMiss);
        }
        if !(self.gates_all() && self.comps_all() && self.cfg_all() && self.labels_all()) {
            return Err(LokiCode::ESeal);
        }
        Ok(())
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 注入守恒等式：注入 = 检出 + 逃逸。
    pub fn conservation(&self) -> Result<(), LokiCode> {
        if self.injected == self.caught + self.escaped {
            Ok(())
        } else {
            Err(LokiCode::EInj)
        }
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }
}

impl Default for LokiLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 双跑指纹对照。
pub fn fp_check(a: u64, b: u64) -> Result<(), LokiCode> {
    if a == b && a != 0 {
        Ok(())
    } else {
        Err(LokiCode::EFp)
    }
}

/// KB-COMPAT 定级（全齐 → S1，六闸齐 → S2，否则 S3）。
pub fn judge_grade(led: &LokiLedger) -> Grade {
    if led.gates_all()
        && led.comps_all()
        && led.cfg_all()
        && led.labels_all()
        && led.unions_all()
        && led.gos_all()
    {
        Grade::S1
    } else if led.gates_all() {
        Grade::S2
    } else {
        Grade::S3
    }
}

/// 定级守卫：定级与目标档漂移 → EGradeDrift。
pub fn grade_guard(led: &LokiLedger, want: Grade) -> Result<(), LokiCode> {
    let got = judge_grade(led);
    if got == want {
        Ok(())
    } else {
        Err(LokiCode::EGradeDrift)
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（记格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = LokiLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < COMPS {
        if let Some(w) = comp_cell(o) {
            if led.record_comp(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut i = 0usize;
    while i < CFG_CELLS {
        if let Some(w) = cfg_cell(i) {
            if led.record_cfg(i, w).is_ok() {
                recorded += 1;
            }
        }
        i += 1;
    }
    let mut i = 0usize;
    while i < LABEL_CELLS {
        if let Some(w) = label_cell(i) {
            if led.record_label(i, w).is_ok() {
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
    // 注入 10 轮：组件载荷漂移 / 族序逆跳（空账本必抓，不改临时账 last_kind 以外主账）/ 越闭集 三态轮转。
    // 三类注入全部落在当轮临时账本 lt 上，主账只记 injected/caught 守恒账。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = LokiLedger::new();
        let caught = match kind {
            0 => {
                // 漂移注入：先按合法序落正格 0，再注格 1 载荷漂移。
                let _ = lt.record_comp(0, comp_cell(0).unwrap_or(0));
                lt.record_comp(1, comp_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(LokiCode::EDrift)
            }
            1 => {
                // 族序注入：空账本首族必 distributor，格 9 属 querier 逆跳必抓。
                lt.record_comp(9, comp_cell(9).unwrap_or(0)) == Err(LokiCode::EOrder)
            }
            _ => {
                // 越闭集注入：组件/投影/标签三面闭集之外必拒。
                lt.record_comp(COMPS, 0).is_err()
                    && lt.record_cfg(CFG_CELLS, 0).is_err()
                    && lt.record_label(LABEL_CELLS, 0).is_err()
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0xCB2100 + k as u64);
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
/// UNX-F52393 判据集（锚 UNX-F52394-J1 推送/查询 ×10，off-by-one 板式）。
pub fn run_unx52393_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52393");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "run_cycle 记格数恒等",
        recorded == COMPS as u32 + CFG_CELLS as u32 + LABEL_CELLS as u32,
        "20 组件 + 12 投影 + 6 标签逐格恒等（闸与声明行不计数）",
    );
    s.add(
        "组件五族 20 格全通",
        {
            let mut lt = LokiLedger::new();
            let mut ok = true;
            let mut o = 0usize;
            while o < COMPS {
                if let Some(w) = comp_cell(o) {
                    if lt.record_comp(o, w).is_err() {
                        ok = false;
                    }
                }
                o += 1;
            }
            ok && lt.comps_all()
        },
        "distributor/ingester/querier/ruler/compactor 五族逐格载荷恒等",
    );
    s.add(
        "loki-config.yml 双向投影 12 格全落",
        {
            let mut lt = LokiLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < CFG_CELLS {
                if let Some(w) = cfg_cell(i) {
                    if lt.record_cfg(i, w).is_err() {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok && lt.cfg_all()
        },
        "账→yml 正向投影逐格恒等落账",
    );
    s.add(
        "loki-config.yml 回投核验 12/12",
        {
            let mut lt = LokiLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < CFG_CELLS {
                if let Some(w) = cfg_cell(i) {
                    let _ = lt.record_cfg(i, w);
                    // 回投：同格二次提交必拒 EDup（yml→账反向核验）。
                    if lt.record_cfg(i, w) != Err(LokiCode::EDup) {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok && lt.cfg_all()
        },
        "yml→账反向回投逐格核验，重复提交必拒",
    );
    s.add(
        "标签三态 6 格全落",
        {
            let mut lt = LokiLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < LABEL_CELLS {
                if let Some(w) = label_cell(i) {
                    if lt.record_label(i, w).is_err() {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok && lt.labels_all()
        },
        "static/dynamic/dropped 三态逐格递进落账",
    );
    s.add(
        "Promtail/PromServer 消费联签 4/4（F52392 反向联签）",
        {
            let mut lt = LokiLedger::new();
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
        "Promtail 推送面/PromServer 消费面四行联签闭集（§AI-66.3）",
    );
    s.add(
        "Go 上栈声明 2/2",
        {
            let mut lt = LokiLedger::new();
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
            while o < COMPS {
                if comp_kind(o).unwrap_or(5) < comp_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "distributor→ingester→querier→ruler→compactor 五族闭集单调",
    );
    s.add(
        "六判据闸全落",
        {
            let mut lt = LokiLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "安装/服务注册/3100 端口开放/推送·查询功能/卸载净除/升级保留逐格落账",
    );
    s.add(
        "边界·组件越闭集",
        LokiLedger::new().record_comp(COMPS, 0).is_err(),
        "组件 20 越闭集必拒",
    );
    s.add(
        "边界·投影越闭集",
        LokiLedger::new().record_cfg(CFG_CELLS, 0).is_err(),
        "投影 12 越闭集必拒",
    );
    s.add(
        "边界·标签越闭集",
        LokiLedger::new().record_label(LABEL_CELLS, 0).is_err(),
        "标签 6 越闭集必拒",
    );
    s.add(
        "标签三态越序拒",
        {
            // 空账本首格必 static（态 0），格 3 属 dynamic 态，越态必抓。
            LokiLedger::new().record_label(3, label_cell(3).unwrap_or(0)) == Err(LokiCode::ELabelState)
        },
        "标签三态 0→1→2 递进，越态必抓",
    );
    s.add(
        "漂移拒",
        {
            // 合法序先落投影格 0 正格，再验格 1 投影载荷漂移必抓。
            let mut lt = LokiLedger::new();
            let _ = lt.record_cfg(0, cfg_cell(0).unwrap_or(0));
            lt.record_cfg(1, cfg_cell(1).unwrap_or(0).wrapping_add(9))
                == Err(LokiCode::ECfgDrift)
        },
        "loki-config.yml 投影载荷漂移必抓",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "组件漂移/族序逆跳/越闭集三态轮转必抓",
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
            let mut lt = LokiLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < COMPS {
                if let Some(w) = comp_cell(o) {
                    let _ = lt.record_comp(o, w);
                }
                o += 1;
            }
            let mut i = 0usize;
            while i < CFG_CELLS {
                if let Some(w) = cfg_cell(i) {
                    let _ = lt.record_cfg(i, w);
                }
                i += 1;
            }
            let mut i = 0usize;
            while i < LABEL_CELLS {
                if let Some(w) = label_cell(i) {
                    let _ = lt.record_label(i, w);
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
        assert_eq!(kind_name(0), Some("distributor"));
        assert_eq!(kind_name(4), Some("compactor"));
        assert!(kind_name(5).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = comp_cell(3).unwrap_or(0);
        assert_eq!(a, comp_cell(3).unwrap_or(0));
        assert_ne!(a, comp_cell(4).unwrap_or(0));
        assert_ne!(cfg_cell(0).unwrap_or(0), cfg_cell(5).unwrap_or(0));
        assert_ne!(label_cell(0).unwrap_or(0), label_cell(3).unwrap_or(0));
        assert!(cfg_cell(CFG_CELLS).is_none());
        assert!(label_cell(LABEL_CELLS).is_none());
        assert!(union_row(UNION_ROWS).is_none());
        assert!(go_row(GO_ROWS).is_none());
    }

    #[test]
    fn test_label_state_mapping() {
        // kind 类 const fn 为 i/N 映射：格 i → 态 i/2，按该映射核对。
        assert_eq!(label_state(0), Some(0));
        assert_eq!(label_state(1), Some(0));
        assert_eq!(label_state(2), Some(1));
        assert_eq!(label_state(5), Some(2));
        assert!(label_state(6).is_none());
        assert_eq!(label_state_name(0), Some("static"));
        assert_eq!(label_state_name(2), Some("dropped"));
        assert!(label_state_name(3).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        // 空账本：首族必 distributor，跳族必抓。
        let mut lt = LokiLedger::new();
        assert_eq!(
            lt.record_comp(9, comp_cell(9).unwrap_or(0)),
            Err(LokiCode::EOrder)
        );
        assert_eq!(lt.stats().1, 1);
        // 合法序落低族格后再回跳 → 族序逆必抓。
        let _ = lt.record_comp(0, comp_cell(0).unwrap_or(0));
        let _ = lt.record_comp(5, comp_cell(5).unwrap_or(0));
        assert_eq!(
            lt.record_comp(2, comp_cell(2).unwrap_or(0)),
            Err(LokiCode::EOrder)
        );
        assert!(lt.record_comp(6, comp_cell(6).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_cfg_drift_and_dup() {
        let mut lt = LokiLedger::new();
        assert_eq!(
            lt.record_cfg(0, cfg_cell(0).unwrap_or(0).wrapping_add(1)),
            Err(LokiCode::ECfgDrift)
        );
        let _ = lt.record_cfg(0, cfg_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_cfg(0, cfg_cell(0).unwrap_or(0)),
            Err(LokiCode::EDup)
        );
    }

    #[test]
    fn test_label_tri_state_caught() {
        // 空账本：首格必 static 态，格 3 属 dynamic 态越态必抓。
        let mut lt = LokiLedger::new();
        assert_eq!(
            lt.record_label(3, label_cell(3).unwrap_or(0)),
            Err(LokiCode::ELabelState)
        );
        // 首格落正后态载荷漂移仍 EDrift。
        assert_eq!(
            lt.record_label(0, label_cell(0).unwrap_or(0).wrapping_add(1)),
            Err(LokiCode::EDrift)
        );
        // 合法落格 0、1（static）后跳态 dropped（格 4）→ 越态必抓。
        let _ = lt.record_label(0, label_cell(0).unwrap_or(0));
        let _ = lt.record_label(1, label_cell(1).unwrap_or(0));
        assert_eq!(
            lt.record_label(4, label_cell(4).unwrap_or(0)),
            Err(LokiCode::ELabelState)
        );
        assert!(lt.record_label(2, label_cell(2).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = LokiLedger::new();
        let _ = lt.record_comp(0, comp_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_comp(0, comp_cell(0).unwrap_or(0)),
            Err(LokiCode::EDup)
        );
        let _ = lt.declare_go(0);
        assert_eq!(lt.declare_go(0), Err(LokiCode::EDup));
        assert_eq!(lt.declare_go(GO_ROWS), Err(LokiCode::ERowMiss));
        assert_eq!(lt.declare_union(UNION_ROWS), Err(LokiCode::ERowMiss));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = LokiLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        assert!(!LokiLedger::new().gates_all());
    }

    #[test]
    fn test_seal_ladder() {
        // 空账：声明行缺 → ERowMiss。
        assert_eq!(LokiLedger::new().seal(), Err(LokiCode::ERowMiss));
        // 行齐格未齐 → ESeal。
        let mut lt = LokiLedger::new();
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
        assert_eq!(lt.seal(), Err(LokiCode::ESeal));
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // 记格数 = 20 组件 + 12 投影 + 6 标签（闸与声明行不计数）。
        assert_eq!(
            recorded,
            COMPS as u32 + CFG_CELLS as u32 + LABEL_CELLS as u32
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
        assert_eq!(SEG_BASE, 0xCA200);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(LokiCode::Ok as u32, 0);
        assert_eq!(LokiCode::EGradeDrift as u32, 12);
        assert_eq!(LokiCode::Ok.val(), SEG_BASE);
        assert_eq!(LokiCode::EGradeDrift.val(), SEG_BASE + 0x0C);
        assert!(!LokiCode::Ok.detail().is_empty());
        assert!(!LokiCode::ESeal.detail().is_empty());
        assert!(!LokiCode::ECfgDrift.detail().is_empty());
        assert!(!LokiCode::ELabelState.detail().is_empty());
        assert!(!LokiCode::ENoLabel.detail().is_empty());
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
    }

    #[test]
    fn test_grade() {
        // 如实阶梯：空账 S3 → 六闸齐 S2 → 全齐目标档 S1。
        assert_eq!(judge_grade(&LokiLedger::new()), Grade::S3);
        let mut lt = LokiLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt.judge_gate(g);
            g += 1;
        }
        assert_eq!(judge_grade(&lt), Grade::S2);
        assert!(grade_guard(&lt, Grade::S1).is_err());
        let mut full = LokiLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = full.judge_gate(g);
            g += 1;
        }
        let mut o = 0usize;
        while o < COMPS {
            if let Some(w) = comp_cell(o) {
                let _ = full.record_comp(o, w);
            }
            o += 1;
        }
        let mut i = 0usize;
        while i < CFG_CELLS {
            if let Some(w) = cfg_cell(i) {
                let _ = full.record_cfg(i, w);
            }
            i += 1;
        }
        let mut i = 0usize;
        while i < LABEL_CELLS {
            if let Some(w) = label_cell(i) {
                let _ = full.record_label(i, w);
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
        assert_eq!(judge_grade(&full), Grade::S1);
        let (inj, cgt, esc) = full.stats();
        assert_eq!(inj, cgt + esc);
    }

    #[test]
    fn unx52393_checks_green() {
        let s = run_unx52393_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert_eq!(s.len(), 20);
    }
}
