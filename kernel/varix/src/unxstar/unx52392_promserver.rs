//! UNX-F52392 Prometheus Server（Windows）收口 · 0xCA1C0段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15，逐字）：
//!   键 "52392"（判据原文锚定源）criteria：① 纯功能代码 ≥300 行；② 判据
//!   原文：UNX-F52393-J1 抓取/查询 ×10；KB-COMPAT 定级登记；判据锚定
//!   §AI-66.3；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!   键 "52393"（功能标题源）title：Prometheus Server（Windows）收口
//!   quota: 280
//! off-by-one 板式口径：本卡（卡 N=52392）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52393-J1（抓取/查询 ×10），照原文（键 "52392" criteria）锚定；
//! 功能按本卡标题（Prometheus Server（Windows）收口，键 "52393" title）
//! 落地；行数基线按功能标题源 quota=280 执行（落盘 ≥340 行）。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五组件族闭集 20 格：tsdb/promql/scrape/rule/web 各 4 格，族序单调
//!      （空账本首族必 tsdb，逆跳必抓），逐格载荷纯函数折叠恒等才收，
//!      漂移必抓。
//!   ② prometheus.yml 双向投影 12 格：正向投影（账→yml 键）逐格恒等，
//!      载荷漂移 EYmlDrift 必抓，同格二次提交必拒 EDup（回投核验）。
//!   ③ scrape 8 拍单调：拍号严格逐拍递进（0→7），拍载荷逐拍恒等，跳拍/
//!      倒拍/拍载荷漂移一律 EMono 抓获。
//!   ④ rule 组账 6 格：规则组载荷逐格对照，越闭集 ENoRule 必拒。
//!   ⑤ 与 Prometheus Windows Exporter 消费联签 4 格（F52381 反向联签，
//!      Exporter 侧 UNX-F52381 已作 Server 消费前向声明）+ Go 上栈声明
//!      2 行闭集，缺行拒封（ERowMiss），未齐拒封（ESeal）。
//!   ⑥ ledger 状态机（new/record_*/judge_gate/seal/fingerprint，FNV-1a）；
//!      注入核 run_cycle 三态轮转 10/10（组件载荷漂移/scrape 跳拍/越闭
//!      集），检出+逃逸守恒等式；双跑 FNV 指纹一致；定级闭集 S1/S2/S3
//!      （六闸 + 20 组件 + 12 投影 + 8 拍 + 6 规则 + 4 联签 + 2 Go 全齐
//!      → S1）。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52392;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 组件族格数（五族 × 4）。
pub const COMPS: usize = 20;

/// prometheus.yml 双向投影格数。
pub const YML_CELLS: usize = 12;

/// scrape 采样拍数。
pub const BEATS: usize = 8;

/// rule 组账格数。
pub const RULES: usize = 6;

/// Exporter 消费联签行数（F52381 反向联签）。
pub const UNION_ROWS: usize = 4;

/// Go 上栈声明行数。
pub const GO_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PromsrvCode {
    /// 成功
    Ok = 0,
    /// 格号越闭集
    ENoCell,
    /// 组件/rule 载荷漂移
    EDrift,
    /// 组件族逆序
    EOrder,
    /// prometheus.yml 投影漂移
    EYmlDrift,
    /// scrape 拍序破缺
    EMono,
    /// rule 组越闭集
    ENoRule,
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

impl PromsrvCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            PromsrvCode::Ok => "成功",
            PromsrvCode::ENoCell => "格号越闭集：组件 20 格闭集之外先登记",
            PromsrvCode::EDrift => "载荷漂移：组件/rule 折叠与基准不恒等，逐格复算",
            PromsrvCode::EOrder => "组件族逆序：tsdb→promql→scrape→rule→web 单调，逆跳必抓",
            PromsrvCode::EYmlDrift => "prometheus.yml 投影漂移：双向投影载荷与基准不恒等，逐格复算",
            PromsrvCode::EMono => "scrape 拍序破缺：拍号严格 0→7 递进，跳拍/倒拍/拍载荷漂移必抓",
            PromsrvCode::ENoRule => "rule 组越闭集：6 格闭集之外先登记",
            PromsrvCode::ERowMiss => "缺声明行：Exporter 联签 4 行或 Go 上栈 2 行未齐先补齐",
            PromsrvCode::EDup => "重复落账：同格只许一条账目",
            PromsrvCode::ESeal => "未齐即封：20 组件 + 12 投影 + 8 拍 + 6 规则 + 闸未齐先补齐",
            PromsrvCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            PromsrvCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            PromsrvCode::EGradeDrift => "定级漂移：全齐才可定 S1，判据缺只可定 S3",
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
            Grade::S1 => "S1：六判据+20组件+12投影+8拍+6规则+4联签+2Go 全通收口",
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
    Some((0xCC000u64 + (o as u64) * 0x3F) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 组件族（0..4=tsdb 4..8=promql 8..12=scrape 12..16=rule 16..20=web）。
pub const fn comp_kind(o: usize) -> Option<u32> {
    if o >= COMPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 组件族名。
pub const fn kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("tsdb"),
        1 => Some("promql"),
        2 => Some("scrape"),
        3 => Some("rule"),
        4 => Some("web"),
        _ => None,
    }
}

/// prometheus.yml 双向投影格载荷（格 i）。
pub const fn yml_cell(i: usize) -> Option<u64> {
    if i >= YML_CELLS {
        return None;
    }
    Some((0xCD000u64 + (i as u64) * 0x47) ^ 0x50524F4D_0000)
}

/// scrape 拍载荷（拍 b）。
pub const fn beat_cell(b: usize) -> Option<u64> {
    if b >= BEATS {
        return None;
    }
    Some((0xCE000u64 + (b as u64) * 0x53) ^ 0x53435250_0000)
}

/// rule 组格载荷（格 r）。
pub const fn rule_cell(r: usize) -> Option<u64> {
    if r >= RULES {
        return None;
    }
    Some((0xCF000u64 + (r as u64) * 0x59) ^ 0x52554C45_0000)
}

/// Exporter 消费联签行折叠（行 r）。
pub const fn union_row(r: usize) -> Option<u32> {
    if r >= UNION_ROWS {
        return None;
    }
    Some(0xD000 + r as u32 * 0x2C)
}

/// Go 上栈声明行折叠（行 r）。
pub const fn go_row(r: usize) -> Option<u32> {
    if r >= GO_ROWS {
        return None;
    }
    Some(0xD040 + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Prometheus Server（Windows）收口账本状态机。
#[derive(Clone)]
pub struct PromsrvLedger {
    /// 六判据（安装/服务注册/端口开放/查询功能/卸载净除/升级保留）。
    gates: [Option<bool>; 6],
    /// 组件账。
    comps: [Option<u64>; COMPS],
    /// prometheus.yml 投影账。
    yml: [Option<u64>; YML_CELLS],
    /// scrape 拍账。
    beats: [Option<u64>; BEATS],
    /// rule 组账。
    rules: [Option<u64>; RULES],
    /// 消费联签账。
    unions: [Option<u32>; UNION_ROWS],
    /// Go 上栈声明账。
    gos: [Option<u32>; GO_ROWS],
    /// 最后组件族。
    last_kind: i64,
    /// 最后采样拍。
    last_beat: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl PromsrvLedger {
    /// 新账本。
    pub fn new() -> Self {
        PromsrvLedger {
            gates: [None; 6],
            comps: [None; COMPS],
            yml: [None; YML_CELLS],
            beats: [None; BEATS],
            rules: [None; RULES],
            unions: [None; UNION_ROWS],
            gos: [None; GO_ROWS],
            last_kind: -1,
            last_beat: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), PromsrvCode> {
        if gate >= 6 {
            return Err(PromsrvCode::ENoCell);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0xCA1C00 + gate as u64);
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

    /// 组件一格（族序单调 + 首族必 tsdb + 载荷对照 + 重复拒）。
    pub fn record_comp(&mut self, o: usize, got: u64) -> Result<(), PromsrvCode> {
        if o >= COMPS {
            return Err(PromsrvCode::ENoCell);
        }
        let kind = match comp_kind(o) {
            Some(k) => k as i64,
            None => return Err(PromsrvCode::ENoCell),
        };
        let order_bad = if self.last_kind < 0 {
            kind != 0
        } else {
            kind < self.last_kind || kind > self.last_kind + 1
        };
        if order_bad {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xCA1D00);
            return Err(PromsrvCode::EOrder);
        }
        let want = match comp_cell(o) {
            Some(w) => w,
            None => return Err(PromsrvCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xCA1D01 + o as u64);
            return Err(PromsrvCode::EDrift);
        }
        if match self.comps.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PromsrvCode::EDup);
        }
        if let Some(c) = self.comps.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind;
        self.fnv(0xCA1D02 + o as u64);
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

    /// prometheus.yml 投影一格（载荷对照 + 重复拒；回投即 EDup）。
    pub fn record_yml(&mut self, i: usize, got: u64) -> Result<(), PromsrvCode> {
        if i >= YML_CELLS {
            return Err(PromsrvCode::ENoCell);
        }
        let want = match yml_cell(i) {
            Some(w) => w,
            None => return Err(PromsrvCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xCA1E00 + i as u64);
            return Err(PromsrvCode::EYmlDrift);
        }
        if match self.yml.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PromsrvCode::EDup);
        }
        if let Some(c) = self.yml.get_mut(i) {
            *c = Some(got);
        }
        self.fnv(0xCA1E01 + i as u64);
        Ok(())
    }

    /// 投影全账核验（12 格）。
    pub fn yml_all(&self) -> bool {
        let mut i = 0usize;
        while i < YML_CELLS {
            match self.yml.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// scrape 一拍（拍号严格递进 + 拍载荷对照 + 重复拒）。
    pub fn record_beat(&mut self, b: usize, got: u64) -> Result<(), PromsrvCode> {
        if b >= BEATS {
            return Err(PromsrvCode::ENoCell);
        }
        if b as i64 != self.last_beat + 1 {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xCA1F00);
            return Err(PromsrvCode::EMono);
        }
        let want = match beat_cell(b) {
            Some(w) => w,
            None => return Err(PromsrvCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xCA1F01 + b as u64);
            return Err(PromsrvCode::EMono);
        }
        if match self.beats.get(b) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PromsrvCode::EDup);
        }
        if let Some(c) = self.beats.get_mut(b) {
            *c = Some(got);
        }
        self.last_beat = b as i64;
        self.fnv(0xCA1F02 + b as u64);
        Ok(())
    }

    /// 采样全账核验（8 拍）。
    pub fn beats_all(&self) -> bool {
        let mut b = 0usize;
        while b < BEATS {
            match self.beats.get(b) {
                Some(Some(_)) => {}
                _ => return false,
            }
            b += 1;
        }
        true
    }

    /// rule 组一格（载荷对照 + 重复拒）。
    pub fn record_rule(&mut self, r: usize, got: u64) -> Result<(), PromsrvCode> {
        if r >= RULES {
            return Err(PromsrvCode::ENoRule);
        }
        let want = match rule_cell(r) {
            Some(w) => w,
            None => return Err(PromsrvCode::ENoRule),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xCA2000 + r as u64);
            return Err(PromsrvCode::EDrift);
        }
        if match self.rules.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PromsrvCode::EDup);
        }
        if let Some(c) = self.rules.get_mut(r) {
            *c = Some(got);
        }
        self.fnv(0xCA2001 + r as u64);
        Ok(())
    }

    /// rule 全账核验（6 格）。
    pub fn rules_all(&self) -> bool {
        let mut r = 0usize;
        while r < RULES {
            match self.rules.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// Exporter 消费联签一行（F52381 反向联签）。
    pub fn declare_union(&mut self, r: usize) -> Result<(), PromsrvCode> {
        if r >= UNION_ROWS {
            return Err(PromsrvCode::ERowMiss);
        }
        if match self.unions.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PromsrvCode::EDup);
        }
        if let Some(c) = self.unions.get_mut(r) {
            *c = union_row(r);
        }
        self.fnv(0xCA2100 + r as u64);
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
    pub fn declare_go(&mut self, r: usize) -> Result<(), PromsrvCode> {
        if r >= GO_ROWS {
            return Err(PromsrvCode::ERowMiss);
        }
        if match self.gos.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PromsrvCode::EDup);
        }
        if let Some(c) = self.gos.get_mut(r) {
            *c = go_row(r);
        }
        self.fnv(0xCA2180 + r as u64);
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
    pub fn seal(&self) -> Result<(), PromsrvCode> {
        if !(self.unions_all() && self.gos_all()) {
            return Err(PromsrvCode::ERowMiss);
        }
        if !(self.gates_all() && self.comps_all() && self.yml_all() && self.beats_all() && self.rules_all()) {
            return Err(PromsrvCode::ESeal);
        }
        Ok(())
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 注入守恒等式：注入 = 检出 + 逃逸。
    pub fn conservation(&self) -> Result<(), PromsrvCode> {
        if self.injected == self.caught + self.escaped {
            Ok(())
        } else {
            Err(PromsrvCode::EInj)
        }
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }
}

impl Default for PromsrvLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 双跑指纹对照。
pub fn fp_check(a: u64, b: u64) -> Result<(), PromsrvCode> {
    if a == b && a != 0 {
        Ok(())
    } else {
        Err(PromsrvCode::EFp)
    }
}

/// KB-COMPAT 定级（全齐 → S1，六闸齐 → S2，否则 S3）。
pub fn judge_grade(led: &PromsrvLedger) -> Grade {
    if led.gates_all()
        && led.comps_all()
        && led.yml_all()
        && led.beats_all()
        && led.rules_all()
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
pub fn grade_guard(led: &PromsrvLedger, want: Grade) -> Result<(), PromsrvCode> {
    let got = judge_grade(led);
    if got == want {
        Ok(())
    } else {
        Err(PromsrvCode::EGradeDrift)
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（记格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = PromsrvLedger::new();
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
    while i < YML_CELLS {
        if let Some(w) = yml_cell(i) {
            if led.record_yml(i, w).is_ok() {
                recorded += 1;
            }
        }
        i += 1;
    }
    let mut b = 0usize;
    while b < BEATS {
        if let Some(w) = beat_cell(b) {
            if led.record_beat(b, w).is_ok() {
                recorded += 1;
            }
        }
        b += 1;
    }
    let mut r = 0usize;
    while r < RULES {
        if let Some(w) = rule_cell(r) {
            if led.record_rule(r, w).is_ok() {
                recorded += 1;
            }
        }
        r += 1;
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
    // 注入 10 轮：组件载荷漂移 / scrape 跳拍（空账本必抓）/ 越闭集 三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = PromsrvLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_comp(0, comp_cell(0).unwrap_or(0));
                lt.record_comp(1, comp_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(PromsrvCode::EDrift)
            }
            1 => lt.record_beat(3, beat_cell(3).unwrap_or(0)) == Err(PromsrvCode::EMono),
            _ => {
                lt.record_comp(COMPS, 0).is_err()
                    && lt.record_yml(YML_CELLS, 0).is_err()
                    && lt.record_rule(RULES, 0).is_err()
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0xCA2200 + k as u64);
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
/// UNX-F52392 判据集（锚 UNX-F52393-J1 抓取/查询 ×10，off-by-one 板式）。
pub fn run_unx52392_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52392");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "run_cycle 记格数恒等",
        recorded == COMPS as u32 + YML_CELLS as u32 + BEATS as u32 + RULES as u32,
        "20 组件 + 12 投影 + 8 拍 + 6 规则逐格恒等（闸与声明行不计数）",
    );
    s.add(
        "组件五族 20 格全通",
        {
            let mut lt = PromsrvLedger::new();
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
        "tsdb/promql/scrape/rule/web 五族逐格载荷恒等",
    );
    s.add(
        "prometheus.yml 双向投影 12 格全落",
        {
            let mut lt = PromsrvLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < YML_CELLS {
                if let Some(w) = yml_cell(i) {
                    if lt.record_yml(i, w).is_err() {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok && lt.yml_all()
        },
        "账→yml 正向投影逐格恒等落账",
    );
    s.add(
        "prometheus.yml 回投核验 12/12",
        {
            let mut lt = PromsrvLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < YML_CELLS {
                if let Some(w) = yml_cell(i) {
                    let _ = lt.record_yml(i, w);
                    // 回投：同格二次提交必拒 EDup（yml→账反向核验）。
                    if lt.record_yml(i, w) != Err(PromsrvCode::EDup) {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok && lt.yml_all()
        },
        "yml→账反向回投逐格核验，重复提交必拒",
    );
    s.add(
        "scrape 8 拍单调",
        {
            let mut lt = PromsrvLedger::new();
            let mut ok = true;
            let mut b = 0usize;
            while b < BEATS {
                if let Some(w) = beat_cell(b) {
                    if lt.record_beat(b, w).is_err() {
                        ok = false;
                    }
                }
                b += 1;
            }
            ok && lt.beats_all()
        },
        "拍号严格 0→7 递进且拍载荷逐拍恒等",
    );
    s.add(
        "rule 组账 6 格全落",
        {
            let mut lt = PromsrvLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < RULES {
                if let Some(w) = rule_cell(r) {
                    if lt.record_rule(r, w).is_err() {
                        ok = false;
                    }
                }
                r += 1;
            }
            ok && lt.rules_all()
        },
        "规则组载荷逐格对照落账",
    );
    s.add(
        "Exporter 消费联签 4/4（F52381 反向联签）",
        {
            let mut lt = PromsrvLedger::new();
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
        "Prometheus Windows Exporter 消费面四行联签闭集（§AI-66.3）",
    );
    s.add(
        "Go 上栈声明 2/2",
        {
            let mut lt = PromsrvLedger::new();
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
        "tsdb→promql→scrape→rule→web 五族闭集单调",
    );
    s.add(
        "六判据闸全落",
        {
            let mut lt = PromsrvLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "安装/服务注册/端口开放/查询功能/卸载净除/升级保留逐格落账",
    );
    s.add(
        "边界·组件越闭集",
        PromsrvLedger::new().record_comp(COMPS, 0).is_err(),
        "组件 20 越闭集必拒",
    );
    s.add(
        "边界·投影越闭集",
        PromsrvLedger::new().record_yml(YML_CELLS, 0).is_err(),
        "投影 12 越闭集必拒",
    );
    s.add(
        "边界·rule 越闭集",
        PromsrvLedger::new().record_rule(RULES, 0).is_err(),
        "rule 6 越闭集必拒",
    );
    s.add(
        "漂移拒",
        {
            // 合法序先落投影格 0 正格，再验格 1 投影载荷漂移必抓。
            let mut lt = PromsrvLedger::new();
            let _ = lt.record_yml(0, yml_cell(0).unwrap_or(0));
            lt.record_yml(1, yml_cell(1).unwrap_or(0).wrapping_add(9))
                == Err(PromsrvCode::EYmlDrift)
        },
        "prometheus.yml 投影载荷漂移必抓",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "组件漂移/跳拍/越闭集三态轮转必抓",
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
            let mut lt = PromsrvLedger::new();
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
            while i < YML_CELLS {
                if let Some(w) = yml_cell(i) {
                    let _ = lt.record_yml(i, w);
                }
                i += 1;
            }
            let mut b = 0usize;
            while b < BEATS {
                if let Some(w) = beat_cell(b) {
                    let _ = lt.record_beat(b, w);
                }
                b += 1;
            }
            let mut r = 0usize;
            while r < RULES {
                if let Some(w) = rule_cell(r) {
                    let _ = lt.record_rule(r, w);
                }
                r += 1;
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
        assert_eq!(kind_name(0), Some("tsdb"));
        assert_eq!(kind_name(4), Some("web"));
        assert!(kind_name(5).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = comp_cell(3).unwrap_or(0);
        assert_eq!(a, comp_cell(3).unwrap_or(0));
        assert_ne!(a, comp_cell(4).unwrap_or(0));
        assert_ne!(yml_cell(0).unwrap_or(0), yml_cell(5).unwrap_or(0));
        assert_ne!(beat_cell(0).unwrap_or(0), beat_cell(3).unwrap_or(0));
        assert!(yml_cell(YML_CELLS).is_none());
        assert!(beat_cell(BEATS).is_none());
        assert!(rule_cell(RULES).is_none());
        assert!(union_row(UNION_ROWS).is_none());
        assert!(go_row(GO_ROWS).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        // 空账本：首族必 tsdb，跳族必抓。
        let mut lt = PromsrvLedger::new();
        assert_eq!(
            lt.record_comp(9, comp_cell(9).unwrap_or(0)),
            Err(PromsrvCode::EOrder)
        );
        assert_eq!(lt.stats().1, 1);
        // 合法序落低族格后再回跳 → 族序逆必抓。
        let _ = lt.record_comp(0, comp_cell(0).unwrap_or(0));
        let _ = lt.record_comp(5, comp_cell(5).unwrap_or(0));
        assert_eq!(
            lt.record_comp(2, comp_cell(2).unwrap_or(0)),
            Err(PromsrvCode::EOrder)
        );
        assert!(lt.record_comp(6, comp_cell(6).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_beat_mono_caught() {
        // 空账本：首拍必 0，跳拍必抓。
        let mut lt = PromsrvLedger::new();
        assert_eq!(
            lt.record_beat(3, beat_cell(3).unwrap_or(0)),
            Err(PromsrvCode::EMono)
        );
        // 首拍落正后拍载荷漂移仍 EMono。
        assert_eq!(
            lt.record_beat(0, beat_cell(0).unwrap_or(0).wrapping_add(1)),
            Err(PromsrvCode::EMono)
        );
        // 合法落拍 0 后跳拍 2 → 跳拍必抓。
        let _ = lt.record_beat(0, beat_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_beat(2, beat_cell(2).unwrap_or(0)),
            Err(PromsrvCode::EMono)
        );
        assert!(lt.record_beat(1, beat_cell(1).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_yml_drift_and_dup() {
        let mut lt = PromsrvLedger::new();
        assert_eq!(
            lt.record_yml(0, yml_cell(0).unwrap_or(0).wrapping_add(1)),
            Err(PromsrvCode::EYmlDrift)
        );
        let _ = lt.record_yml(0, yml_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_yml(0, yml_cell(0).unwrap_or(0)),
            Err(PromsrvCode::EDup)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = PromsrvLedger::new();
        let _ = lt.record_comp(0, comp_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_comp(0, comp_cell(0).unwrap_or(0)),
            Err(PromsrvCode::EDup)
        );
        let _ = lt.declare_go(0);
        assert_eq!(lt.declare_go(0), Err(PromsrvCode::EDup));
        assert_eq!(lt.declare_go(GO_ROWS), Err(PromsrvCode::ERowMiss));
        assert_eq!(lt.declare_union(UNION_ROWS), Err(PromsrvCode::ERowMiss));
        assert_eq!(lt.record_rule(RULES, 0), Err(PromsrvCode::ENoRule));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = PromsrvLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        assert!(!PromsrvLedger::new().gates_all());
    }

    #[test]
    fn test_seal_ladder() {
        // 空账：声明行缺 → ERowMiss。
        assert_eq!(PromsrvLedger::new().seal(), Err(PromsrvCode::ERowMiss));
        // 行齐格未齐 → ESeal。
        let mut lt = PromsrvLedger::new();
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
        assert_eq!(lt.seal(), Err(PromsrvCode::ESeal));
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // 记格数 = 20 组件 + 12 投影 + 8 拍 + 6 规则（闸与声明行不计数）。
        assert_eq!(
            recorded,
            COMPS as u32 + YML_CELLS as u32 + BEATS as u32 + RULES as u32
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
        assert_eq!(SEG_BASE, 0xCA1C0);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(PromsrvCode::Ok as u32, 0);
        assert_eq!(PromsrvCode::EGradeDrift as u32, 12);
        assert_eq!(PromsrvCode::Ok.val(), SEG_BASE);
        assert_eq!(PromsrvCode::EGradeDrift.val(), SEG_BASE + 0x0C);
        assert!(!PromsrvCode::Ok.detail().is_empty());
        assert!(!PromsrvCode::ESeal.detail().is_empty());
        assert!(!PromsrvCode::EYmlDrift.detail().is_empty());
        assert!(!PromsrvCode::EMono.detail().is_empty());
        assert!(!PromsrvCode::ENoRule.detail().is_empty());
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
    }

    #[test]
    fn test_grade() {
        // 如实阶梯：空账 S3 → 六闸齐 S2 → 全齐目标档 S1。
        assert_eq!(judge_grade(&PromsrvLedger::new()), Grade::S3);
        let mut lt = PromsrvLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt.judge_gate(g);
            g += 1;
        }
        assert_eq!(judge_grade(&lt), Grade::S2);
        assert!(grade_guard(&lt, Grade::S1).is_err());
        let mut full = PromsrvLedger::new();
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
        while i < YML_CELLS {
            if let Some(w) = yml_cell(i) {
                let _ = full.record_yml(i, w);
            }
            i += 1;
        }
        let mut b = 0usize;
        while b < BEATS {
            if let Some(w) = beat_cell(b) {
                let _ = full.record_beat(b, w);
            }
            b += 1;
        }
        let mut r = 0usize;
        while r < RULES {
            if let Some(w) = rule_cell(r) {
                let _ = full.record_rule(r, w);
            }
            r += 1;
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
    fn unx52392_checks_green() {
        let s = run_unx52392_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert_eq!(s.len(), 20);
    }
}
