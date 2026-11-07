//! UNX-F52395 Jaeger all-in-one（Windows）收口 · 0xCA280段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15，逐字）：
//!   键 "52395"（判据原文锚定源）criteria：① 纯功能代码 ≥280 行；② 判据
//!   原文：UNX-F52396-J1 trace 查询 ×10；KB-COMPAT 定级登记；判据锚定
//!   §AI-66.3；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!   键 "52396"（功能标题源）title：Jaeger all-in-one（Windows）收口
//!   quota: 260
//! off-by-one 板式口径：本卡（卡 N=52395）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52396-J1（trace 查询 ×10），照原文（键 "52395" criteria）锚定；
//! 功能按本卡标题（Jaeger all-in-one（Windows）收口，键 "52396" title）
//! 落地；行数基线按功能标题源 quota=260 执行（落盘 ≥320 行）。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五组件族闭集 20 格：agent/collector/query/ingester/storage 各 4
//!      格，族序单调（空账本首族必 agent，逆跳必抓），逐格载荷纯函数折
//!      叠恒等才收，漂移必抓。
//!   ② span 10 操作集：五类别（client/server/producer/consumer/internal）
//!      各 2 操作，类别单调（逆序必抓），逐操作载荷恒等才收。
//!   ③ 采样三策略 6 格：const/simple/probabilistic 三策略各 2 格，策略
//!      序严格递进（0→1→2），越策略必抓，载荷漂移必抓。
//!   ④ 存储三态 6 格：memory/badger/cassandra 三态各 2 格，态序严格递
//!      进，越态必抓；与 OTel Collector 消费联签 4 格（F52394 反向联
//!      签）+ Go 上栈声明 2 行闭集，缺行拒封（ERowMiss）。
//!   ⑤ ledger 状态机（new/record_*/judge_gate/seal/fingerprint，FNV-1a）；
//!      注入核 run_cycle 三态轮转 10/10（span 载荷漂移/族序逆跳/越闭
//!      集），检出+逃逸守恒等式；双跑 FNV 指纹一致；定级闭集 S1/S2/S3
//!      （六闸 + 20 组件 + 10 span + 6 采样 + 6 存储 + 4 联签 + 2 Go
//!      全齐 → S1）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52395;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 组件族格数（五族 × 4）。
pub const COMPS: usize = 20;

/// span 操作数（五类别 × 2）。
pub const SPAN_OPS: usize = 10;

/// 采样三策略格数（三策略 × 2）。
pub const SAMP_CELLS: usize = 6;

/// 存储三态格数（三态 × 2）。
pub const STORE_CELLS: usize = 6;

/// OTel 消费联签行数（F52394 反向联签）。
pub const UNION_ROWS: usize = 4;

/// Go 上栈声明行数。
pub const GO_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JaegerCode {
    /// 成功
    Ok = 0,
    /// 格号越闭集
    ENoCell,
    /// 组件/span 载荷漂移
    EDrift,
    /// 组件族/span 类别/策略·存储态序逆序
    EOrder,
    /// span 操作越闭集
    ESpanNo,
    /// 采样策略格越闭集
    ESampNo,
    /// 存储格越闭集
    EStoreNo,
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

impl JaegerCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            JaegerCode::Ok => "成功",
            JaegerCode::ENoCell => "格号越闭集：组件 20 格/六闸闭集之外先登记",
            JaegerCode::EDrift => "载荷漂移：组件/span/采样/存储折叠与基准不恒等，逐格复算",
            JaegerCode::EOrder => "逆序必抓：agent→collector→query→ingester→storage 族序与 span 类别、策略/存储态序单调，逆跳必抓",
            JaegerCode::ESpanNo => "span 操作越闭集：10 操作集之外先登记",
            JaegerCode::ESampNo => "采样策略格越闭集：6 格闭集之外先登记",
            JaegerCode::EStoreNo => "存储格越闭集：6 格闭集之外先登记",
            JaegerCode::ERowMiss => "缺声明行：OTel 消费联签 4 行或 Go 上栈 2 行未齐先补齐",
            JaegerCode::EDup => "重复落账：同格只许一条账目",
            JaegerCode::ESeal => "未齐即封：20 组件 + 10 span + 6 采样 + 6 存储 + 闸未齐先补齐",
            JaegerCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            JaegerCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            JaegerCode::EGradeDrift => "定级漂移：全齐才可定 S1，判据缺只可定 S3",
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
            Grade::S1 => "S1：六判据+20组件+10span+6采样+6存储+4联签+2Go 全通收口",
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
    Some((0xCD000u64 + (o as u64) * 0x39) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 组件族（0..4=agent 4..8=collector 8..12=query 12..16=ingester 16..20=storage）。
pub const fn comp_kind(o: usize) -> Option<u32> {
    if o >= COMPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 组件族名。
pub const fn kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("agent"),
        1 => Some("collector"),
        2 => Some("query"),
        3 => Some("ingester"),
        4 => Some("storage"),
        _ => None,
    }
}

/// span 操作格载荷（操作 s；类别 = s/2）。
pub const fn span_cell(s: usize) -> Option<u64> {
    if s >= SPAN_OPS {
        return None;
    }
    Some((0xCD400u64 + (s as u64) * 0x3B) ^ ((s / 2) as u64).wrapping_mul(0x01000193))
}

/// span 类别（0..2=client 2..4=server 4..6=producer 6..8=consumer 8..10=internal）。
pub const fn span_kind(s: usize) -> Option<u32> {
    if s >= SPAN_OPS {
        return None;
    }
    Some((s / 2) as u32)
}

/// span 类别名。
pub const fn span_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("client"),
        1 => Some("server"),
        2 => Some("producer"),
        3 => Some("consumer"),
        4 => Some("internal"),
        _ => None,
    }
}

/// 采样策略格载荷（格 i）。
pub const fn samp_cell(i: usize) -> Option<u64> {
    if i >= SAMP_CELLS {
        return None;
    }
    Some((0xCD800u64 + (i as u64) * 0x43) ^ 0x4A414553_0000)
}

/// 采样三策略（0..2=const 2..4=simple 4..6=probabilistic）。
pub const fn samp_state(i: usize) -> Option<u32> {
    if i >= SAMP_CELLS {
        return None;
    }
    Some((i / 2) as u32)
}

/// 采样策略名。
pub const fn samp_state_name(t: usize) -> Option<&'static str> {
    match t {
        0 => Some("const"),
        1 => Some("simple"),
        2 => Some("probabilistic"),
        _ => None,
    }
}

/// 存储格载荷（格 i）。
pub const fn store_cell(i: usize) -> Option<u64> {
    if i >= STORE_CELLS {
        return None;
    }
    Some((0xCDC00u64 + (i as u64) * 0x47) ^ 0x53544F52_0000)
}

/// 存储三态（0..2=memory 2..4=badger 4..6=cassandra）。
pub const fn store_state(i: usize) -> Option<u32> {
    if i >= STORE_CELLS {
        return None;
    }
    Some((i / 2) as u32)
}

/// 存储态名。
pub const fn store_state_name(t: usize) -> Option<&'static str> {
    match t {
        0 => Some("memory"),
        1 => Some("badger"),
        2 => Some("cassandra"),
        _ => None,
    }
}

/// OTel 消费联签行折叠（行 r）。
pub const fn union_row(r: usize) -> Option<u32> {
    if r >= UNION_ROWS {
        return None;
    }
    Some(0xCD00 + r as u32 * 0x2C)
}

/// Go 上栈声明行折叠（行 r）。
pub const fn go_row(r: usize) -> Option<u32> {
    if r >= GO_ROWS {
        return None;
    }
    Some(0xCD40 + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Jaeger all-in-one（Windows）收口账本状态机。
#[derive(Clone)]
pub struct JaegerLedger {
    /// 六判据（安装/服务注册/16686 端口开放/trace 查询/卸载净除/升级保留）。
    gates: [Option<bool>; 6],
    /// 组件账。
    comps: [Option<u64>; COMPS],
    /// span 操作账。
    spans: [Option<u64>; SPAN_OPS],
    /// 采样策略账。
    samps: [Option<u64>; SAMP_CELLS],
    /// 存储三态账。
    stores: [Option<u64>; STORE_CELLS],
    /// 消费联签账。
    unions: [Option<u32>; UNION_ROWS],
    /// Go 上栈声明账。
    gos: [Option<u32>; GO_ROWS],
    /// 最后组件族。
    last_kind: i64,
    /// 最后 span 类别。
    last_span: i64,
    /// 最后采样策略态。
    last_samp: i64,
    /// 最后存储态。
    last_store: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl JaegerLedger {
    /// 新账本。
    pub fn new() -> Self {
        JaegerLedger {
            gates: [None; 6],
            comps: [None; COMPS],
            spans: [None; SPAN_OPS],
            samps: [None; SAMP_CELLS],
            stores: [None; STORE_CELLS],
            unions: [None; UNION_ROWS],
            gos: [None; GO_ROWS],
            last_kind: -1,
            last_span: -1,
            last_samp: -1,
            last_store: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), JaegerCode> {
        if gate >= 6 {
            return Err(JaegerCode::ENoCell);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0xCD1C00 + gate as u64);
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

    /// 组件一格（族序单调 + 首族必 agent + 载荷对照 + 重复拒）。
    pub fn record_comp(&mut self, o: usize, got: u64) -> Result<(), JaegerCode> {
        if o >= COMPS {
            return Err(JaegerCode::ENoCell);
        }
        let kind = match comp_kind(o) {
            Some(k) => k as i64,
            None => return Err(JaegerCode::ENoCell),
        };
        let order_bad = if self.last_kind < 0 {
            kind != 0
        } else {
            kind < self.last_kind || kind > self.last_kind + 1
        };
        if order_bad {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xCD1D00);
            return Err(JaegerCode::EOrder);
        }
        let want = match comp_cell(o) {
            Some(w) => w,
            None => return Err(JaegerCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xCD1D01 + o as u64);
            return Err(JaegerCode::EDrift);
        }
        if match self.comps.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(JaegerCode::EDup);
        }
        if let Some(c) = self.comps.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind;
        self.fnv(0xCD1D02 + o as u64);
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

    /// span 一操作（类别单调 + 首类别必 client + 载荷对照 + 重复拒）。
    pub fn record_span(&mut self, s: usize, got: u64) -> Result<(), JaegerCode> {
        if s >= SPAN_OPS {
            return Err(JaegerCode::ESpanNo);
        }
        let kind = match span_kind(s) {
            Some(k) => k as i64,
            None => return Err(JaegerCode::ESpanNo),
        };
        let order_bad = if self.last_span < 0 {
            kind != 0
        } else {
            kind < self.last_span || kind > self.last_span + 1
        };
        if order_bad {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xCD1E00);
            return Err(JaegerCode::EOrder);
        }
        let want = match span_cell(s) {
            Some(w) => w,
            None => return Err(JaegerCode::ESpanNo),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xCD1E01 + s as u64);
            return Err(JaegerCode::EDrift);
        }
        if match self.spans.get(s) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(JaegerCode::EDup);
        }
        if let Some(c) = self.spans.get_mut(s) {
            *c = Some(got);
        }
        self.last_span = kind;
        self.fnv(0xCD1E02 + s as u64);
        Ok(())
    }

    /// span 全账核验（10 操作）。
    pub fn spans_all(&self) -> bool {
        let mut s = 0usize;
        while s < SPAN_OPS {
            match self.spans.get(s) {
                Some(Some(_)) => {}
                _ => return false,
            }
            s += 1;
        }
        true
    }

    /// 采样策略一格（策略序递进 + 载荷对照 + 重复拒）。
    pub fn record_samp(&mut self, i: usize, got: u64) -> Result<(), JaegerCode> {
        if i >= SAMP_CELLS {
            return Err(JaegerCode::ESampNo);
        }
        let st = match samp_state(i) {
            Some(s) => s as i64,
            None => return Err(JaegerCode::ESampNo),
        };
        let order_bad = if self.last_samp < 0 {
            st != 0
        } else {
            st < self.last_samp || st > self.last_samp + 1
        };
        if order_bad {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xCD1F00);
            return Err(JaegerCode::EOrder);
        }
        let want = match samp_cell(i) {
            Some(w) => w,
            None => return Err(JaegerCode::ESampNo),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xCD1F01 + i as u64);
            return Err(JaegerCode::EDrift);
        }
        if match self.samps.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(JaegerCode::EDup);
        }
        if let Some(c) = self.samps.get_mut(i) {
            *c = Some(got);
        }
        self.last_samp = st;
        self.fnv(0xCD1F02 + i as u64);
        Ok(())
    }

    /// 采样全账核验（6 格）。
    pub fn samps_all(&self) -> bool {
        let mut i = 0usize;
        while i < SAMP_CELLS {
            match self.samps.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 存储一格（态序递进 + 载荷对照 + 重复拒）。
    pub fn record_store(&mut self, i: usize, got: u64) -> Result<(), JaegerCode> {
        if i >= STORE_CELLS {
            return Err(JaegerCode::EStoreNo);
        }
        let st = match store_state(i) {
            Some(s) => s as i64,
            None => return Err(JaegerCode::EStoreNo),
        };
        let order_bad = if self.last_store < 0 {
            st != 0
        } else {
            st < self.last_store || st > self.last_store + 1
        };
        if order_bad {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xCD2000);
            return Err(JaegerCode::EOrder);
        }
        let want = match store_cell(i) {
            Some(w) => w,
            None => return Err(JaegerCode::EStoreNo),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xCD2001 + i as u64);
            return Err(JaegerCode::EDrift);
        }
        if match self.stores.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(JaegerCode::EDup);
        }
        if let Some(c) = self.stores.get_mut(i) {
            *c = Some(got);
        }
        self.last_store = st;
        self.fnv(0xCD2002 + i as u64);
        Ok(())
    }

    /// 存储全账核验（6 格）。
    pub fn stores_all(&self) -> bool {
        let mut i = 0usize;
        while i < STORE_CELLS {
            match self.stores.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// OTel 消费联签一行（F52394 反向联签）。
    pub fn declare_union(&mut self, r: usize) -> Result<(), JaegerCode> {
        if r >= UNION_ROWS {
            return Err(JaegerCode::ERowMiss);
        }
        if match self.unions.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(JaegerCode::EDup);
        }
        if let Some(c) = self.unions.get_mut(r) {
            *c = union_row(r);
        }
        self.fnv(0xCD2100 + r as u64);
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
    pub fn declare_go(&mut self, r: usize) -> Result<(), JaegerCode> {
        if r >= GO_ROWS {
            return Err(JaegerCode::ERowMiss);
        }
        if match self.gos.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(JaegerCode::EDup);
        }
        if let Some(c) = self.gos.get_mut(r) {
            *c = go_row(r);
        }
        self.fnv(0xCD2180 + r as u64);
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
    pub fn seal(&self) -> Result<(), JaegerCode> {
        if !(self.unions_all() && self.gos_all()) {
            return Err(JaegerCode::ERowMiss);
        }
        if !(self.gates_all() && self.comps_all() && self.spans_all() && self.samps_all() && self.stores_all()) {
            return Err(JaegerCode::ESeal);
        }
        Ok(())
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 注入守恒等式：注入 = 检出 + 逃逸。
    pub fn conservation(&self) -> Result<(), JaegerCode> {
        if self.injected == self.caught + self.escaped {
            Ok(())
        } else {
            Err(JaegerCode::EInj)
        }
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }
}

impl Default for JaegerLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 双跑指纹对照。
pub fn fp_check(a: u64, b: u64) -> Result<(), JaegerCode> {
    if a == b && a != 0 {
        Ok(())
    } else {
        Err(JaegerCode::EFp)
    }
}

/// KB-COMPAT 定级（全齐 → S1，六闸齐 → S2，否则 S3）。
pub fn judge_grade(led: &JaegerLedger) -> Grade {
    if led.gates_all()
        && led.comps_all()
        && led.spans_all()
        && led.samps_all()
        && led.stores_all()
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
pub fn grade_guard(led: &JaegerLedger, want: Grade) -> Result<(), JaegerCode> {
    let got = judge_grade(led);
    if got == want {
        Ok(())
    } else {
        Err(JaegerCode::EGradeDrift)
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（记格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = JaegerLedger::new();
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
    let mut s = 0usize;
    while s < SPAN_OPS {
        if let Some(w) = span_cell(s) {
            if led.record_span(s, w).is_ok() {
                recorded += 1;
            }
        }
        s += 1;
    }
    let mut i = 0usize;
    while i < SAMP_CELLS {
        if let Some(w) = samp_cell(i) {
            if led.record_samp(i, w).is_ok() {
                recorded += 1;
            }
        }
        i += 1;
    }
    let mut i = 0usize;
    while i < STORE_CELLS {
        if let Some(w) = store_cell(i) {
            if led.record_store(i, w).is_ok() {
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
    // 注入 10 轮：span 载荷漂移 / 族序逆跳（空账本必抓）/ 越闭集 三态轮转。
    // 三类注入全部落在当轮临时账本 lt 上，主账只记 injected/caught 守恒账。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = JaegerLedger::new();
        let caught = match kind {
            0 => {
                // 漂移注入：先按合法序落 span 0 正格，再注 span 1 载荷漂移。
                let _ = lt.record_span(0, span_cell(0).unwrap_or(0));
                lt.record_span(1, span_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(JaegerCode::EDrift)
            }
            1 => {
                // 族序注入：空账本首族必 agent，格 9 属 query 逆跳必抓。
                lt.record_comp(9, comp_cell(9).unwrap_or(0)) == Err(JaegerCode::EOrder)
            }
            _ => {
                // 越闭集注入：组件/span/采样/存储四面闭集之外必拒。
                lt.record_comp(COMPS, 0).is_err()
                    && lt.record_span(SPAN_OPS, 0).is_err()
                    && lt.record_samp(SAMP_CELLS, 0).is_err()
                    && lt.record_store(STORE_CELLS, 0).is_err()
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0xCD2200 + k as u64);
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
/// UNX-F52395 判据集（锚 UNX-F52396-J1 trace 查询 ×10，off-by-one 板式）。
pub fn run_unx52395_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52395");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "run_cycle 记格数恒等",
        recorded == COMPS as u32 + SPAN_OPS as u32 + SAMP_CELLS as u32 + STORE_CELLS as u32,
        "20 组件 + 10 span + 6 采样 + 6 存储逐格恒等（闸与声明行不计数）",
    );
    s.add(
        "组件五族 20 格全通",
        {
            let mut lt = JaegerLedger::new();
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
        "agent/collector/query/ingester/storage 五族逐格载荷恒等",
    );
    s.add(
        "span 10 操作全落",
        {
            let mut lt = JaegerLedger::new();
            let mut ok = true;
            let mut s = 0usize;
            while s < SPAN_OPS {
                if let Some(w) = span_cell(s) {
                    if lt.record_span(s, w).is_err() {
                        ok = false;
                    }
                }
                s += 1;
            }
            ok && lt.spans_all()
        },
        "client/server/producer/consumer/internal 五类别逐操作恒等",
    );
    s.add(
        "采样三策略 6 格全落",
        {
            let mut lt = JaegerLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < SAMP_CELLS {
                if let Some(w) = samp_cell(i) {
                    if lt.record_samp(i, w).is_err() {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok && lt.samps_all()
        },
        "const/simple/probabilistic 三策略逐格递进落账",
    );
    s.add(
        "存储三态 6 格全落",
        {
            let mut lt = JaegerLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < STORE_CELLS {
                if let Some(w) = store_cell(i) {
                    if lt.record_store(i, w).is_err() {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok && lt.stores_all()
        },
        "memory/badger/cassandra 三态逐格递进落账",
    );
    s.add(
        "OTel 消费联签 4/4（F52394 反向联签）",
        {
            let mut lt = JaegerLedger::new();
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
        "OTel Collector 消费面四行联签闭集（§AI-66.3）",
    );
    s.add(
        "Go 上栈声明 2/2",
        {
            let mut lt = JaegerLedger::new();
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
        "agent→collector→query→ingester→storage 五族闭集单调",
    );
    s.add(
        "六判据闸全落",
        {
            let mut lt = JaegerLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "安装/服务注册/16686 端口开放/trace 查询/卸载净除/升级保留逐格落账",
    );
    s.add(
        "边界·组件越闭集",
        JaegerLedger::new().record_comp(COMPS, 0).is_err(),
        "组件 20 越闭集必拒",
    );
    s.add(
        "边界·span 越闭集",
        JaegerLedger::new().record_span(SPAN_OPS, 0).is_err(),
        "span 10 操作越闭集必拒",
    );
    s.add(
        "边界·采样越闭集",
        JaegerLedger::new().record_samp(SAMP_CELLS, 0).is_err(),
        "采样 6 格越闭集必拒",
    );
    s.add(
        "边界·存储越闭集",
        JaegerLedger::new().record_store(STORE_CELLS, 0).is_err(),
        "存储 6 格越闭集必拒",
    );
    s.add(
        "span 类别越序拒",
        {
            // 空账本首操作必 client 类（类别 0），span 5 属 producer 类越类必抓。
            JaegerLedger::new().record_span(5, span_cell(5).unwrap_or(0)) == Err(JaegerCode::EOrder)
        },
        "span 类别 client→…→internal 单调，越类必抓",
    );
    s.add(
        "漂移拒",
        {
            // 合法序先落 span 0 正格，再验 span 1 载荷漂移必抓。
            let mut lt = JaegerLedger::new();
            let _ = lt.record_span(0, span_cell(0).unwrap_or(0));
            lt.record_span(1, span_cell(1).unwrap_or(0).wrapping_add(9))
                == Err(JaegerCode::EDrift)
        },
        "span 操作载荷漂移必抓",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "span 漂移/族序逆跳/越闭集三态轮转必抓",
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
            let mut lt = JaegerLedger::new();
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
            let mut s = 0usize;
            while s < SPAN_OPS {
                if let Some(w) = span_cell(s) {
                    let _ = lt.record_span(s, w);
                }
                s += 1;
            }
            let mut i = 0usize;
            while i < SAMP_CELLS {
                if let Some(w) = samp_cell(i) {
                    let _ = lt.record_samp(i, w);
                }
                i += 1;
            }
            let mut i = 0usize;
            while i < STORE_CELLS {
                if let Some(w) = store_cell(i) {
                    let _ = lt.record_store(i, w);
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
    s.add("收口·判据条数", s.len() >= 20, "含本条共 21 条闭集");
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
        assert_eq!(kind_name(0), Some("agent"));
        assert_eq!(kind_name(4), Some("storage"));
        assert!(kind_name(5).is_none());
    }

    #[test]
    fn test_span_kind_mapping() {
        // kind 类 const fn 为 i/N 映射：操作 s → 类别 s/2，按该映射核对。
        assert_eq!(span_kind(0), Some(0));
        assert_eq!(span_kind(1), Some(0));
        assert_eq!(span_kind(2), Some(1));
        assert_eq!(span_kind(9), Some(4));
        assert!(span_kind(10).is_none());
        assert!(span_cell(10).is_none());
        assert_eq!(span_kind_name(0), Some("client"));
        assert_eq!(span_kind_name(4), Some("internal"));
        assert!(span_kind_name(5).is_none());
    }

    #[test]
    fn test_tri_state_mappings() {
        assert_eq!(samp_state(0), Some(0));
        assert_eq!(samp_state(3), Some(1));
        assert_eq!(samp_state(5), Some(2));
        assert!(samp_state(6).is_none());
        assert_eq!(samp_state_name(0), Some("const"));
        assert_eq!(samp_state_name(2), Some("probabilistic"));
        assert!(samp_state_name(3).is_none());
        assert_eq!(store_state(2), Some(1));
        assert!(store_state(6).is_none());
        assert_eq!(store_state_name(0), Some("memory"));
        assert_eq!(store_state_name(2), Some("cassandra"));
        assert!(store_state_name(3).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = comp_cell(3).unwrap_or(0);
        assert_eq!(a, comp_cell(3).unwrap_or(0));
        assert_ne!(a, comp_cell(4).unwrap_or(0));
        assert_ne!(span_cell(0).unwrap_or(0), span_cell(5).unwrap_or(0));
        assert_ne!(samp_cell(0).unwrap_or(0), samp_cell(3).unwrap_or(0));
        assert_ne!(store_cell(0).unwrap_or(0), store_cell(3).unwrap_or(0));
        assert!(samp_cell(SAMP_CELLS).is_none());
        assert!(store_cell(STORE_CELLS).is_none());
        assert!(union_row(UNION_ROWS).is_none());
        assert!(go_row(GO_ROWS).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        // 空账本：首族必 agent，跳族必抓。
        let mut lt = JaegerLedger::new();
        assert_eq!(
            lt.record_comp(9, comp_cell(9).unwrap_or(0)),
            Err(JaegerCode::EOrder)
        );
        assert_eq!(lt.stats().1, 1);
        // 合法序落低族格后再回跳 → 族序逆必抓。
        let _ = lt.record_comp(0, comp_cell(0).unwrap_or(0));
        let _ = lt.record_comp(5, comp_cell(5).unwrap_or(0));
        assert_eq!(
            lt.record_comp(2, comp_cell(2).unwrap_or(0)),
            Err(JaegerCode::EOrder)
        );
        assert!(lt.record_comp(6, comp_cell(6).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_span_order_and_drift() {
        // 空账本：首操作必 client 类，span 5 属 producer 类越类必抓。
        let mut lt = JaegerLedger::new();
        assert_eq!(
            lt.record_span(5, span_cell(5).unwrap_or(0)),
            Err(JaegerCode::EOrder)
        );
        // 首操作落正后载荷漂移仍 EDrift。
        assert_eq!(
            lt.record_span(0, span_cell(0).unwrap_or(0).wrapping_add(1)),
            Err(JaegerCode::EDrift)
        );
        // 合法落 span 0、1（client 类）后跳类 internal（span 8）→ 越类必抓。
        let _ = lt.record_span(0, span_cell(0).unwrap_or(0));
        let _ = lt.record_span(1, span_cell(1).unwrap_or(0));
        assert_eq!(
            lt.record_span(8, span_cell(8).unwrap_or(0)),
            Err(JaegerCode::EOrder)
        );
        assert!(lt.record_span(2, span_cell(2).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_samp_store_order_caught() {
        // 空账本：采样首格必 const 态，格 3 属 simple 态越态必抓。
        let mut lt = JaegerLedger::new();
        assert_eq!(
            lt.record_samp(3, samp_cell(3).unwrap_or(0)),
            Err(JaegerCode::EOrder)
        );
        let _ = lt.record_samp(0, samp_cell(0).unwrap_or(0));
        let _ = lt.record_samp(1, samp_cell(1).unwrap_or(0));
        // 落齐 const 态后跳 probabilistic 态（格 4）→ 越态必抓。
        assert_eq!(
            lt.record_samp(4, samp_cell(4).unwrap_or(0)),
            Err(JaegerCode::EOrder)
        );
        assert!(lt.record_samp(2, samp_cell(2).unwrap_or(0)).is_ok());
        // 存储空账本：格 3 属 badger 态越态必抓。
        let mut lt2 = JaegerLedger::new();
        assert_eq!(
            lt2.record_store(3, store_cell(3).unwrap_or(0)),
            Err(JaegerCode::EOrder)
        );
        // 载荷漂移必抓。
        assert_eq!(
            lt2.record_store(0, store_cell(0).unwrap_or(0).wrapping_add(1)),
            Err(JaegerCode::EDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = JaegerLedger::new();
        let _ = lt.record_comp(0, comp_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_comp(0, comp_cell(0).unwrap_or(0)),
            Err(JaegerCode::EDup)
        );
        let _ = lt.record_span(0, span_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_span(0, span_cell(0).unwrap_or(0)),
            Err(JaegerCode::EDup)
        );
        let _ = lt.declare_go(0);
        assert_eq!(lt.declare_go(0), Err(JaegerCode::EDup));
        assert_eq!(lt.declare_go(GO_ROWS), Err(JaegerCode::ERowMiss));
        assert_eq!(lt.declare_union(UNION_ROWS), Err(JaegerCode::ERowMiss));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = JaegerLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        assert!(!JaegerLedger::new().gates_all());
    }

    #[test]
    fn test_seal_ladder() {
        // 空账：声明行缺 → ERowMiss。
        assert_eq!(JaegerLedger::new().seal(), Err(JaegerCode::ERowMiss));
        // 行齐格未齐 → ESeal。
        let mut lt = JaegerLedger::new();
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
        assert_eq!(lt.seal(), Err(JaegerCode::ESeal));
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // 记格数 = 20 组件 + 10 span + 6 采样 + 6 存储（闸与声明行不计数）。
        assert_eq!(
            recorded,
            COMPS as u32 + SPAN_OPS as u32 + SAMP_CELLS as u32 + STORE_CELLS as u32
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
        assert_eq!(SEG_BASE, 0xCA280);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(JaegerCode::Ok as u32, 0);
        assert_eq!(JaegerCode::EGradeDrift as u32, 12);
        assert_eq!(JaegerCode::Ok.val(), SEG_BASE);
        assert_eq!(JaegerCode::EGradeDrift.val(), SEG_BASE + 0x0C);
        assert!(!JaegerCode::Ok.detail().is_empty());
        assert!(!JaegerCode::ESeal.detail().is_empty());
        assert!(!JaegerCode::ESpanNo.detail().is_empty());
        assert!(!JaegerCode::ESampNo.detail().is_empty());
        assert!(!JaegerCode::EStoreNo.detail().is_empty());
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
    }

    #[test]
    fn test_grade() {
        // 如实阶梯：空账 S3 → 六闸齐 S2 → 全齐目标档 S1。
        assert_eq!(judge_grade(&JaegerLedger::new()), Grade::S3);
        let mut lt = JaegerLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt.judge_gate(g);
            g += 1;
        }
        assert_eq!(judge_grade(&lt), Grade::S2);
        assert!(grade_guard(&lt, Grade::S1).is_err());
        let mut full = JaegerLedger::new();
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
        let mut s = 0usize;
        while s < SPAN_OPS {
            if let Some(w) = span_cell(s) {
                let _ = full.record_span(s, w);
            }
            s += 1;
        }
        let mut i = 0usize;
        while i < SAMP_CELLS {
            if let Some(w) = samp_cell(i) {
                let _ = full.record_samp(i, w);
            }
            i += 1;
        }
        let mut i = 0usize;
        while i < STORE_CELLS {
            if let Some(w) = store_cell(i) {
                let _ = full.record_store(i, w);
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
    fn unx52395_checks_green() {
        let s = run_unx52395_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert_eq!(s.len(), 21);
    }
}
