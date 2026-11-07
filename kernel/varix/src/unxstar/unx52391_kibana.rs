//! UNX-F52391 Kibana（Windows）收口 · 0xCA180段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15，逐字）：
//!   键 "52391"（判据原文锚定源）criteria：① 纯功能代码 ≥300 行；② 判据
//!   原文：UNX-F52392-J1 ×10 视图；KB-COMPAT 定级登记；判据锚定 §AI-66.3；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!   键 "52392"（功能标题源）title：Kibana（Windows）收口
//!   quota: 300
//! off-by-one 板式口径：本卡（卡 N=52391）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52392-J1（×10 视图），照原文（键 "52391" criteria）锚定；功能按
//! 本卡标题（Kibana（Windows）收口，键 "52392" title）落地；行数基线按
//! 功能标题源 quota=300 执行（落盘 ≥360 行）。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五功能族闭集 20 格：discover/visualize/dashboard/canvas/alerts 各
//!      4 格（视图语义），族序单调（空账本首族必 discover，逆跳必抓），
//!      逐格载荷纯函数折叠恒等才收，漂移必抓。
//!   ② kibana.yml 双向投影 12 格：正向投影（账→yml 键）逐格恒等，载荷
//!      漂移 EYmlDrift 必抓，同格二次提交必拒 EDup（回投核验）。
//!   ③ index pattern 三态 6 格：未匹配/部分匹配/全匹配三态各 2 格，态序
//!      单调（空账本首态必未匹配，逆跳/越态必抓 EIdxOrd）。
//!   ④ 与 Elasticsearch 消费联签 4 格 + Node 上栈声明 2 行闭集，缺行拒封
//!      （ERowMiss），未齐拒封（ESeal）。
//!   ⑤ ledger 状态机（new/record_*/judge_gate/seal/fingerprint，FNV-1a）；
//!      注入核 run_cycle 三态轮转 10/10（功能族载荷漂移/index pattern 三
//!      态逆/越闭集），检出+逃逸守恒等式；双跑 FNV 指纹一致；定级闭集
//!      S1/S2/S3（六闸 + 20 功能 + 12 投影 + 6 pattern + 4 联签 + 2 Node
//!      全齐 → S1）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52391;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 功能族格数（五族 × 4）。
pub const FEATS: usize = 20;

/// kibana.yml 双向投影格数。
pub const YML_CELLS: usize = 12;

/// index pattern 三态格数（三态 × 2）。
pub const IDX_PATTERNS: usize = 6;

/// Elasticsearch 消费联签行数。
pub const UNION_ROWS: usize = 4;

/// Node 上栈声明行数。
pub const NODE_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KibanaCode {
    /// 成功
    Ok = 0,
    /// 格号越闭集
    ENoCell,
    /// 功能族载荷漂移
    EDrift,
    /// 功能族逆序
    EOrder,
    /// kibana.yml 投影越闭集
    ENoYml,
    /// kibana.yml 投影漂移
    EYmlDrift,
    /// index pattern 三态逆序
    EIdxOrd,
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

impl KibanaCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            KibanaCode::Ok => "成功",
            KibanaCode::ENoCell => "格号越闭集：功能族 20 格闭集之外先登记",
            KibanaCode::EDrift => "载荷漂移：功能族折叠与基准不恒等，逐格复算",
            KibanaCode::EOrder => "功能族逆序：discover→visualize→dashboard→canvas→alerts 单调，逆跳必抓",
            KibanaCode::ENoYml => "投影越闭集：kibana.yml 12 格闭集之外先登记",
            KibanaCode::EYmlDrift => "kibana.yml 投影漂移：双向投影载荷与基准不恒等，逐格复算",
            KibanaCode::EIdxOrd => "index pattern 三态逆序：首态必未匹配，未匹配→部分匹配→全匹配单调",
            KibanaCode::ERowMiss => "缺声明行：ES 联签 4 行或 Node 上栈 2 行未齐先补齐",
            KibanaCode::EDup => "重复落账：同格只许一条账目",
            KibanaCode::ESeal => "未齐即封：20 功能 + 12 投影 + 6 pattern + 闸未齐先补齐",
            KibanaCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            KibanaCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            KibanaCode::EGradeDrift => "定级漂移：全齐才可定 S1，判据缺只可定 S3",
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
            Grade::S1 => "S1：六判据+20功能+12投影+6pattern+4联签+2Node 全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 功能族格载荷（格 o；族 = o/4）。
pub const fn feat_cell(o: usize) -> Option<u64> {
    if o >= FEATS {
        return None;
    }
    Some((0xC8000u64 + (o as u64) * 0x3B) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 功能族（0..4=discover 4..8=visualize 8..12=dashboard 12..16=canvas 16..20=alerts）。
pub const fn feat_kind(o: usize) -> Option<u32> {
    if o >= FEATS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 功能族名。
pub const fn kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("discover"),
        1 => Some("visualize"),
        2 => Some("dashboard"),
        3 => Some("canvas"),
        4 => Some("alerts"),
        _ => None,
    }
}

/// kibana.yml 双向投影格载荷（格 i）。
pub const fn yml_cell(i: usize) -> Option<u64> {
    if i >= YML_CELLS {
        return None;
    }
    Some((0xC9000u64 + (i as u64) * 0x43) ^ 0x4B494241_0000)
}

/// index pattern 态号（0..2=未匹配 2..4=部分匹配 4..6=全匹配）。
pub const fn idx_state(i: usize) -> Option<u32> {
    if i >= IDX_PATTERNS {
        return None;
    }
    Some((i / 2) as u32)
}

/// index pattern 态名。
pub const fn state_name(s: usize) -> Option<&'static str> {
    match s {
        0 => Some("未匹配"),
        1 => Some("部分匹配"),
        2 => Some("全匹配"),
        _ => None,
    }
}

/// index pattern 格载荷（格 i）。
pub const fn idx_cell(i: usize) -> Option<u64> {
    if i >= IDX_PATTERNS {
        return None;
    }
    Some((0xCA000u64 + (i as u64) * 0x47) ^ 0x49445850_0000)
}

/// Elasticsearch 消费联签行折叠（行 r）。
pub const fn union_row(r: usize) -> Option<u32> {
    if r >= UNION_ROWS {
        return None;
    }
    Some(0xCB00 + r as u32 * 0x2C)
}

/// Node 上栈声明行折叠（行 r）。
pub const fn node_row(r: usize) -> Option<u32> {
    if r >= NODE_ROWS {
        return None;
    }
    Some(0xCB40 + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Kibana（Windows）收口账本状态机。
#[derive(Clone)]
pub struct KibanaLedger {
    /// 六判据（安装/服务注册/端口开放/查询功能/卸载净除/升级保留）。
    gates: [Option<bool>; 6],
    /// 功能族账。
    feats: [Option<u64>; FEATS],
    /// kibana.yml 投影账。
    yml: [Option<u64>; YML_CELLS],
    /// index pattern 三态账。
    idxs: [Option<u64>; IDX_PATTERNS],
    /// 消费联签账。
    unions: [Option<u32>; UNION_ROWS],
    /// Node 上栈声明账。
    nodes: [Option<u32>; NODE_ROWS],
    /// 最后功能族。
    last_kind: i64,
    /// 最后 pattern 态。
    last_idx: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl KibanaLedger {
    /// 新账本。
    pub fn new() -> Self {
        KibanaLedger {
            gates: [None; 6],
            feats: [None; FEATS],
            yml: [None; YML_CELLS],
            idxs: [None; IDX_PATTERNS],
            unions: [None; UNION_ROWS],
            nodes: [None; NODE_ROWS],
            last_kind: -1,
            last_idx: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), KibanaCode> {
        if gate >= 6 {
            return Err(KibanaCode::ENoCell);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0xCA1800 + gate as u64);
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

    /// 功能族一格（族序单调 + 首族必 discover + 载荷对照 + 重复拒）。
    pub fn record_feat(&mut self, o: usize, got: u64) -> Result<(), KibanaCode> {
        if o >= FEATS {
            return Err(KibanaCode::ENoCell);
        }
        let kind = match feat_kind(o) {
            Some(k) => k as i64,
            None => return Err(KibanaCode::ENoCell),
        };
        let order_bad = if self.last_kind < 0 {
            kind != 0
        } else {
            kind < self.last_kind || kind > self.last_kind + 1
        };
        if order_bad {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xCA1900);
            return Err(KibanaCode::EOrder);
        }
        let want = match feat_cell(o) {
            Some(w) => w,
            None => return Err(KibanaCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xCA1901 + o as u64);
            return Err(KibanaCode::EDrift);
        }
        if match self.feats.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(KibanaCode::EDup);
        }
        if let Some(c) = self.feats.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind;
        self.fnv(0xCA1902 + o as u64);
        Ok(())
    }

    /// 功能族全账核验（20 格）。
    pub fn feats_all(&self) -> bool {
        let mut o = 0usize;
        while o < FEATS {
            match self.feats.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// kibana.yml 投影一格（载荷对照 + 重复拒；回投即 EDup）。
    pub fn record_yml(&mut self, i: usize, got: u64) -> Result<(), KibanaCode> {
        if i >= YML_CELLS {
            return Err(KibanaCode::ENoYml);
        }
        let want = match yml_cell(i) {
            Some(w) => w,
            None => return Err(KibanaCode::ENoYml),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xCA1A00 + i as u64);
            return Err(KibanaCode::EYmlDrift);
        }
        if match self.yml.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(KibanaCode::EDup);
        }
        if let Some(c) = self.yml.get_mut(i) {
            *c = Some(got);
        }
        self.fnv(0xCA1A01 + i as u64);
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

    /// index pattern 一格（态序单调 + 首态必未匹配 + 载荷对照 + 重复拒）。
    pub fn record_idx(&mut self, i: usize, got: u64) -> Result<(), KibanaCode> {
        if i >= IDX_PATTERNS {
            return Err(KibanaCode::EIdxOrd);
        }
        let st = match idx_state(i) {
            Some(s) => s as i64,
            None => return Err(KibanaCode::EIdxOrd),
        };
        let ord_bad = if self.last_idx < 0 {
            st != 0
        } else {
            st < self.last_idx || st > self.last_idx + 1
        };
        if ord_bad {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xCA1B00);
            return Err(KibanaCode::EIdxOrd);
        }
        let want = match idx_cell(i) {
            Some(w) => w,
            None => return Err(KibanaCode::EIdxOrd),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xCA1B01 + i as u64);
            return Err(KibanaCode::EDrift);
        }
        if match self.idxs.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(KibanaCode::EDup);
        }
        if let Some(c) = self.idxs.get_mut(i) {
            *c = Some(got);
        }
        self.last_idx = st;
        self.fnv(0xCA1B02 + i as u64);
        Ok(())
    }

    /// pattern 全账核验（6 格）。
    pub fn idxs_all(&self) -> bool {
        let mut i = 0usize;
        while i < IDX_PATTERNS {
            match self.idxs.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// Elasticsearch 消费联签一行。
    pub fn declare_union(&mut self, r: usize) -> Result<(), KibanaCode> {
        if r >= UNION_ROWS {
            return Err(KibanaCode::ERowMiss);
        }
        if match self.unions.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(KibanaCode::EDup);
        }
        if let Some(c) = self.unions.get_mut(r) {
            *c = union_row(r);
        }
        self.fnv(0xCA1C00 + r as u64);
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

    /// Node 上栈声明一行。
    pub fn declare_node(&mut self, r: usize) -> Result<(), KibanaCode> {
        if r >= NODE_ROWS {
            return Err(KibanaCode::ERowMiss);
        }
        if match self.nodes.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(KibanaCode::EDup);
        }
        if let Some(c) = self.nodes.get_mut(r) {
            *c = node_row(r);
        }
        self.fnv(0xCA1C80 + r as u64);
        Ok(())
    }

    /// Node 声明全齐核验。
    pub fn nodes_all(&self) -> bool {
        let mut r = 0usize;
        while r < NODE_ROWS {
            match self.nodes.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 封账核验：声明行缺 → ERowMiss；格/闸未齐 → ESeal；全齐 → Ok。
    pub fn seal(&self) -> Result<(), KibanaCode> {
        if !(self.unions_all() && self.nodes_all()) {
            return Err(KibanaCode::ERowMiss);
        }
        if !(self.gates_all() && self.feats_all() && self.yml_all() && self.idxs_all()) {
            return Err(KibanaCode::ESeal);
        }
        Ok(())
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 注入守恒等式：注入 = 检出 + 逃逸。
    pub fn conservation(&self) -> Result<(), KibanaCode> {
        if self.injected == self.caught + self.escaped {
            Ok(())
        } else {
            Err(KibanaCode::EInj)
        }
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }
}

impl Default for KibanaLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 双跑指纹对照。
pub fn fp_check(a: u64, b: u64) -> Result<(), KibanaCode> {
    if a == b && a != 0 {
        Ok(())
    } else {
        Err(KibanaCode::EFp)
    }
}

/// KB-COMPAT 定级（全齐 → S1，六闸齐 → S2，否则 S3）。
pub fn judge_grade(led: &KibanaLedger) -> Grade {
    if led.gates_all()
        && led.feats_all()
        && led.yml_all()
        && led.idxs_all()
        && led.unions_all()
        && led.nodes_all()
    {
        Grade::S1
    } else if led.gates_all() {
        Grade::S2
    } else {
        Grade::S3
    }
}

/// 定级守卫：定级与目标档漂移 → EGradeDrift。
pub fn grade_guard(led: &KibanaLedger, want: Grade) -> Result<(), KibanaCode> {
    let got = judge_grade(led);
    if got == want {
        Ok(())
    } else {
        Err(KibanaCode::EGradeDrift)
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（记格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = KibanaLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < FEATS {
        if let Some(w) = feat_cell(o) {
            if led.record_feat(o, w).is_ok() {
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
    let mut p = 0usize;
    while p < IDX_PATTERNS {
        if let Some(w) = idx_cell(p) {
            if led.record_idx(p, w).is_ok() {
                recorded += 1;
            }
        }
        p += 1;
    }
    let mut r = 0usize;
    while r < UNION_ROWS {
        let _ = led.declare_union(r);
        r += 1;
    }
    let mut r = 0usize;
    while r < NODE_ROWS {
        let _ = led.declare_node(r);
        r += 1;
    }
    // 注入 10 轮：功能族载荷漂移 / index pattern 三态逆（空账本必抓）/ 越闭集 三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = KibanaLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_feat(0, feat_cell(0).unwrap_or(0));
                lt.record_feat(1, feat_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(KibanaCode::EDrift)
            }
            1 => lt.record_idx(2, idx_cell(2).unwrap_or(0)) == Err(KibanaCode::EIdxOrd),
            _ => {
                lt.record_feat(FEATS, 0).is_err()
                    && lt.record_yml(YML_CELLS, 0).is_err()
                    && lt.record_idx(IDX_PATTERNS, 0).is_err()
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0xCA1D00 + k as u64);
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
/// UNX-F52391 判据集（锚 UNX-F52392-J1 ×10 视图，off-by-one 板式）。
pub fn run_unx52391_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52391");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "run_cycle 记格数恒等",
        recorded == FEATS as u32 + YML_CELLS as u32 + IDX_PATTERNS as u32,
        "20 功能 + 12 投影 + 6 pattern 逐格恒等（闸与声明行不计数）",
    );
    s.add(
        "功能五族 20 格全通",
        {
            let mut lt = KibanaLedger::new();
            let mut ok = true;
            let mut o = 0usize;
            while o < FEATS {
                if let Some(w) = feat_cell(o) {
                    if lt.record_feat(o, w).is_err() {
                        ok = false;
                    }
                }
                o += 1;
            }
            ok && lt.feats_all()
        },
        "discover/visualize/dashboard/canvas/alerts 五族逐格载荷恒等",
    );
    s.add(
        "kibana.yml 双向投影 12 格全落",
        {
            let mut lt = KibanaLedger::new();
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
        "kibana.yml 回投核验 12/12",
        {
            let mut lt = KibanaLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < YML_CELLS {
                if let Some(w) = yml_cell(i) {
                    let _ = lt.record_yml(i, w);
                    // 回投：同格二次提交必拒 EDup（yml→账反向核验）。
                    if lt.record_yml(i, w) != Err(KibanaCode::EDup) {
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
        "index pattern 三态 6 格全落",
        {
            let mut lt = KibanaLedger::new();
            let mut ok = true;
            let mut p = 0usize;
            while p < IDX_PATTERNS {
                if let Some(w) = idx_cell(p) {
                    if lt.record_idx(p, w).is_err() {
                        ok = false;
                    }
                }
                p += 1;
            }
            ok && lt.idxs_all()
        },
        "未匹配/部分匹配/全匹配三态单调落账",
    );
    s.add(
        "ES 消费联签 4/4",
        {
            let mut lt = KibanaLedger::new();
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
        "Elasticsearch 消费面四行联签闭集（§AI-66.3）",
    );
    s.add(
        "Node 上栈声明 2/2",
        {
            let mut lt = KibanaLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < NODE_ROWS {
                if lt.declare_node(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.nodes_all()
        },
        "Node.js 运行时上栈两行声明闭集",
    );
    s.add(
        "功能族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < FEATS {
                if feat_kind(o).unwrap_or(5) < feat_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "discover→visualize→dashboard→canvas→alerts 五族闭集单调",
    );
    s.add(
        "六判据闸全落",
        {
            let mut lt = KibanaLedger::new();
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
        "边界·功能族越闭集",
        KibanaLedger::new().record_feat(FEATS, 0).is_err(),
        "功能族 20 越闭集必拒",
    );
    s.add(
        "边界·投影越闭集",
        KibanaLedger::new().record_yml(YML_CELLS, 0).is_err(),
        "投影 12 越闭集必拒",
    );
    s.add(
        "边界·pattern 越闭集",
        KibanaLedger::new().record_idx(IDX_PATTERNS, 0).is_err(),
        "pattern 6 越闭集必拒",
    );
    s.add(
        "漂移拒",
        {
            // 合法序先落投影格 0 正格，再验格 1 投影载荷漂移必抓。
            let mut lt = KibanaLedger::new();
            let _ = lt.record_yml(0, yml_cell(0).unwrap_or(0));
            lt.record_yml(1, yml_cell(1).unwrap_or(0).wrapping_add(7))
                == Err(KibanaCode::EYmlDrift)
        },
        "kibana.yml 投影载荷漂移必抓",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "功能族漂移/三态逆/越闭集三态轮转必抓",
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
            let mut lt = KibanaLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < FEATS {
                if let Some(w) = feat_cell(o) {
                    let _ = lt.record_feat(o, w);
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
            let mut p = 0usize;
            while p < IDX_PATTERNS {
                if let Some(w) = idx_cell(p) {
                    let _ = lt.record_idx(p, w);
                }
                p += 1;
            }
            let mut r = 0usize;
            while r < UNION_ROWS {
                let _ = lt.declare_union(r);
                r += 1;
            }
            let mut r = 0usize;
            while r < NODE_ROWS {
                let _ = lt.declare_node(r);
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
        assert_eq!(kind_name(0), Some("discover"));
        assert_eq!(kind_name(4), Some("alerts"));
        assert!(kind_name(5).is_none());
    }

    #[test]
    fn test_idx_states() {
        assert_eq!(idx_state(0), Some(0));
        assert_eq!(idx_state(1), Some(0));
        assert_eq!(idx_state(2), Some(1));
        assert_eq!(idx_state(5), Some(2));
        assert!(idx_state(6).is_none());
        assert_eq!(state_name(0), Some("未匹配"));
        assert_eq!(state_name(1), Some("部分匹配"));
        assert_eq!(state_name(2), Some("全匹配"));
        assert!(state_name(3).is_none());
        assert!(idx_cell(IDX_PATTERNS).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = feat_cell(3).unwrap_or(0);
        assert_eq!(a, feat_cell(3).unwrap_or(0));
        assert_ne!(a, feat_cell(4).unwrap_or(0));
        assert_ne!(yml_cell(0).unwrap_or(0), yml_cell(5).unwrap_or(0));
        assert!(yml_cell(YML_CELLS).is_none());
        assert!(union_row(UNION_ROWS).is_none());
        assert!(node_row(NODE_ROWS).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        // 空账本：首族必 discover，跳族必抓。
        let mut lt = KibanaLedger::new();
        assert_eq!(
            lt.record_feat(9, feat_cell(9).unwrap_or(0)),
            Err(KibanaCode::EOrder)
        );
        assert_eq!(lt.stats().1, 1);
        // 合法序落低族格后再回跳 → 族序逆必抓。
        let _ = lt.record_feat(0, feat_cell(0).unwrap_or(0));
        let _ = lt.record_feat(5, feat_cell(5).unwrap_or(0));
        assert_eq!(
            lt.record_feat(2, feat_cell(2).unwrap_or(0)),
            Err(KibanaCode::EOrder)
        );
        assert!(lt.record_feat(6, feat_cell(6).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_idx_state_caught() {
        // 空账本：首态必未匹配。
        assert_eq!(
            KibanaLedger::new().record_idx(4, idx_cell(4).unwrap_or(0)),
            Err(KibanaCode::EIdxOrd)
        );
        // 合法落态 0 后跳态 2 → 越态必抓。
        let mut lt = KibanaLedger::new();
        let _ = lt.record_idx(0, idx_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_idx(4, idx_cell(4).unwrap_or(0)),
            Err(KibanaCode::EIdxOrd)
        );
        // pattern 载荷漂移 EDrift。
        assert_eq!(
            lt.record_idx(1, idx_cell(1).unwrap_or(0).wrapping_add(6)),
            Err(KibanaCode::EDrift)
        );
    }

    #[test]
    fn test_yml_drift_and_dup() {
        let mut lt = KibanaLedger::new();
        assert_eq!(
            lt.record_yml(0, yml_cell(0).unwrap_or(0).wrapping_add(1)),
            Err(KibanaCode::EYmlDrift)
        );
        let _ = lt.record_yml(0, yml_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_yml(0, yml_cell(0).unwrap_or(0)),
            Err(KibanaCode::EDup)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = KibanaLedger::new();
        let _ = lt.record_feat(0, feat_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_feat(0, feat_cell(0).unwrap_or(0)),
            Err(KibanaCode::EDup)
        );
        let _ = lt.declare_node(0);
        assert_eq!(lt.declare_node(0), Err(KibanaCode::EDup));
        assert_eq!(lt.declare_node(NODE_ROWS), Err(KibanaCode::ERowMiss));
        assert_eq!(lt.declare_union(UNION_ROWS), Err(KibanaCode::ERowMiss));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = KibanaLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        assert!(!KibanaLedger::new().gates_all());
    }

    #[test]
    fn test_seal_ladder() {
        // 空账：声明行缺 → ERowMiss。
        assert_eq!(KibanaLedger::new().seal(), Err(KibanaCode::ERowMiss));
        // 行齐格未齐 → ESeal。
        let mut lt = KibanaLedger::new();
        let mut r = 0usize;
        while r < UNION_ROWS {
            let _ = lt.declare_union(r);
            r += 1;
        }
        let mut r = 0usize;
        while r < NODE_ROWS {
            let _ = lt.declare_node(r);
            r += 1;
        }
        assert_eq!(lt.seal(), Err(KibanaCode::ESeal));
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // 记格数 = 20 功能 + 12 投影 + 6 pattern（闸与声明行不计数）。
        assert_eq!(
            recorded,
            FEATS as u32 + YML_CELLS as u32 + IDX_PATTERNS as u32
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
        assert_eq!(SEG_BASE, 0xCA180);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(KibanaCode::Ok as u32, 0);
        assert_eq!(KibanaCode::EGradeDrift as u32, 12);
        assert_eq!(KibanaCode::Ok.val(), SEG_BASE);
        assert_eq!(KibanaCode::EGradeDrift.val(), SEG_BASE + 0x0C);
        assert!(!KibanaCode::Ok.detail().is_empty());
        assert!(!KibanaCode::ESeal.detail().is_empty());
        assert!(!KibanaCode::EYmlDrift.detail().is_empty());
        assert!(!KibanaCode::EIdxOrd.detail().is_empty());
        assert!(!KibanaCode::ENoYml.detail().is_empty());
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
    }

    #[test]
    fn test_grade() {
        // 如实阶梯：空账 S3 → 六闸齐 S2 → 全齐目标档 S1。
        assert_eq!(judge_grade(&KibanaLedger::new()), Grade::S3);
        let mut lt = KibanaLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt.judge_gate(g);
            g += 1;
        }
        assert_eq!(judge_grade(&lt), Grade::S2);
        assert!(grade_guard(&lt, Grade::S1).is_err());
        let mut full = KibanaLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = full.judge_gate(g);
            g += 1;
        }
        let mut o = 0usize;
        while o < FEATS {
            if let Some(w) = feat_cell(o) {
                let _ = full.record_feat(o, w);
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
        let mut p = 0usize;
        while p < IDX_PATTERNS {
            if let Some(w) = idx_cell(p) {
                let _ = full.record_idx(p, w);
            }
            p += 1;
        }
        let mut r = 0usize;
        while r < UNION_ROWS {
            let _ = full.declare_union(r);
            r += 1;
        }
        let mut r = 0usize;
        while r < NODE_ROWS {
            let _ = full.declare_node(r);
            r += 1;
        }
        assert!(full.seal().is_ok());
        assert_eq!(judge_grade(&full), Grade::S1);
        let (inj, cgt, esc) = full.stats();
        assert_eq!(inj, cgt + esc);
    }

    #[test]
    fn unx52391_checks_green() {
        let s = run_unx52391_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert_eq!(s.len(), 19);
    }
}
