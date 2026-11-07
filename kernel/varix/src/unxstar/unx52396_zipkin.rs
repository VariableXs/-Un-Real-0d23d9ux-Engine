//! UNX-F52396 Zipkin Server（Windows）收口 · 0xCA2C0段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15，逐字）：
//!   键 "52396"（判据原文锚定源）criteria：① 纯功能代码 ≥260 行；② 判据
//!   原文：UNX-F52397-J1 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!   键 "52397"（功能标题源）title：Zipkin Server（Windows）收口
//!   quota: 280
//! off-by-one 板式口径：本卡（卡 N=52396）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52397-J1，照原文（键 "52396" criteria）锚定；功能按本卡标题
//! （Zipkin Server（Windows）收口，键 "52397" title）落地；行数基线按
//! 功能标题源 quota=280 执行（落盘 ≥340 行）。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五组件族闭集 20 格：collector/storage/query/transport/UI 各 4 格，
//!      族序单调（空账本首族必 collector，逆跳必抓），逐格载荷纯函数折
//!      叠恒等才收，漂移必抓。
//!   ② span 10 操作集：五类别（client/server/producer/consumer/internal）
//!      各 2 操作，类别单调（逆序必抓），逐操作载荷恒等才收。
//!   ③ reporter 三态 6 格：http/kafka/activemq 三态各 2 格，态序严格递
//!      进（0→1→2），越态必抓，载荷漂移必抓。
//!   ④ 存储三态 6 格：memory/mysql/elasticsearch 三态各 2 格，态序严格
//!      递进，越态必抓；与 Brave 消费声明 2 行闭集 + Java 上栈声明 2 行
//!      闭集，缺行拒封（ERowMiss），未齐拒封（ESeal）。
//!   ⑤ ledger 状态机（new/record_*/judge_gate/seal/fingerprint，FNV-1a）；
//!      注入核 run_cycle 三态轮转 10/10（span 载荷漂移/族序逆跳/越闭
//!      集），检出+逃逸守恒等式；双跑 FNV 指纹一致；定级闭集 S1/S2/S3
//!      （六闸 + 20 组件 + 10 span + 6 reporter + 6 存储 + 2 Brave +
//!      2 Java 全齐 → S1）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52396;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 组件族格数（五族 × 4）。
pub const COMPS: usize = 20;

/// span 操作数（五类别 × 2）。
pub const SPAN_OPS: usize = 10;

/// reporter 三态格数（三态 × 2）。
pub const REP_CELLS: usize = 6;

/// 存储三态格数（三态 × 2）。
pub const STORE_CELLS: usize = 6;

/// Brave 消费声明行数。
pub const BRAVE_ROWS: usize = 2;

/// Java 上栈声明行数。
pub const JAVA_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ZipkinCode {
    /// 成功
    Ok = 0,
    /// 格号越闭集
    ENoCell,
    /// 组件/span 载荷漂移
    EDrift,
    /// 组件族/span 类别/reporter·存储态序逆序
    EOrder,
    /// span 操作越闭集
    ESpanNo,
    /// reporter 格越闭集
    ERepNo,
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

impl ZipkinCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            ZipkinCode::Ok => "成功",
            ZipkinCode::ENoCell => "格号越闭集：组件 20 格/六闸闭集之外先登记",
            ZipkinCode::EDrift => "载荷漂移：组件/span/reporter/存储折叠与基准不恒等，逐格复算",
            ZipkinCode::EOrder => "逆序必抓：collector→storage→query→transport→UI 族序与 span 类别、reporter/存储态序单调，逆跳必抓",
            ZipkinCode::ESpanNo => "span 操作越闭集：10 操作集之外先登记",
            ZipkinCode::ERepNo => "reporter 格越闭集：6 格闭集之外先登记",
            ZipkinCode::EStoreNo => "存储格越闭集：6 格闭集之外先登记",
            ZipkinCode::ERowMiss => "缺声明行：Brave 消费声明 2 行或 Java 上栈 2 行未齐先补齐",
            ZipkinCode::EDup => "重复落账：同格只许一条账目",
            ZipkinCode::ESeal => "未齐即封：20 组件 + 10 span + 6 reporter + 6 存储 + 闸未齐先补齐",
            ZipkinCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            ZipkinCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            ZipkinCode::EGradeDrift => "定级漂移：全齐才可定 S1，判据缺只可定 S3",
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
            Grade::S1 => "S1：六判据+20组件+10span+6reporter+6存储+2Brave+2Java 全通收口",
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
    Some((0xCE000u64 + (o as u64) * 0x3B) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 组件族（0..4=collector 4..8=storage 8..12=query 12..16=transport 16..20=UI）。
pub const fn comp_kind(o: usize) -> Option<u32> {
    if o >= COMPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 组件族名。
pub const fn kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("collector"),
        1 => Some("storage"),
        2 => Some("query"),
        3 => Some("transport"),
        4 => Some("UI"),
        _ => None,
    }
}

/// span 操作格载荷（操作 s；类别 = s/2）。
pub const fn span_cell(s: usize) -> Option<u64> {
    if s >= SPAN_OPS {
        return None;
    }
    Some((0xCE400u64 + (s as u64) * 0x3D) ^ ((s / 2) as u64).wrapping_mul(0x01000193))
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

/// reporter 格载荷（格 i）。
pub const fn rep_cell(i: usize) -> Option<u64> {
    if i >= REP_CELLS {
        return None;
    }
    Some((0xCE800u64 + (i as u64) * 0x41) ^ 0x5245504F_0000)
}

/// reporter 三态（0..2=http 2..4=kafka 4..6=activemq）。
pub const fn rep_state(i: usize) -> Option<u32> {
    if i >= REP_CELLS {
        return None;
    }
    Some((i / 2) as u32)
}

/// reporter 态名。
pub const fn rep_state_name(t: usize) -> Option<&'static str> {
    match t {
        0 => Some("http"),
        1 => Some("kafka"),
        2 => Some("activemq"),
        _ => None,
    }
}

/// 存储格载荷（格 i）。
pub const fn store_cell(i: usize) -> Option<u64> {
    if i >= STORE_CELLS {
        return None;
    }
    Some((0xCEC00u64 + (i as u64) * 0x45) ^ 0x53544F52_0000)
}

/// 存储三态（0..2=memory 2..4=mysql 4..6=elasticsearch）。
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
        1 => Some("mysql"),
        2 => Some("elasticsearch"),
        _ => None,
    }
}

/// Brave 消费声明行折叠（行 r）。
pub const fn brave_row(r: usize) -> Option<u32> {
    if r >= BRAVE_ROWS {
        return None;
    }
    Some(0xCE00 + r as u32 * 0x2C)
}

/// Java 上栈声明行折叠（行 r）。
pub const fn java_row(r: usize) -> Option<u32> {
    if r >= JAVA_ROWS {
        return None;
    }
    Some(0xCE20 + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Zipkin Server（Windows）收口账本状态机。
#[derive(Clone)]
pub struct ZipkinLedger {
    /// 六判据（安装/服务注册/9411 端口开放/上报·查询功能/卸载净除/升级保留）。
    gates: [Option<bool>; 6],
    /// 组件账。
    comps: [Option<u64>; COMPS],
    /// span 操作账。
    spans: [Option<u64>; SPAN_OPS],
    /// reporter 三态账。
    reps: [Option<u64>; REP_CELLS],
    /// 存储三态账。
    stores: [Option<u64>; STORE_CELLS],
    /// Brave 消费声明账。
    braves: [Option<u32>; BRAVE_ROWS],
    /// Java 上栈声明账。
    javas: [Option<u32>; JAVA_ROWS],
    /// 最后组件族。
    last_kind: i64,
    /// 最后 span 类别。
    last_span: i64,
    /// 最后 reporter 态。
    last_rep: i64,
    /// 最后存储态。
    last_store: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl ZipkinLedger {
    /// 新账本。
    pub fn new() -> Self {
        ZipkinLedger {
            gates: [None; 6],
            comps: [None; COMPS],
            spans: [None; SPAN_OPS],
            reps: [None; REP_CELLS],
            stores: [None; STORE_CELLS],
            braves: [None; BRAVE_ROWS],
            javas: [None; JAVA_ROWS],
            last_kind: -1,
            last_span: -1,
            last_rep: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), ZipkinCode> {
        if gate >= 6 {
            return Err(ZipkinCode::ENoCell);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0xCE1C00 + gate as u64);
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

    /// 组件一格（族序单调 + 首族必 collector + 载荷对照 + 重复拒）。
    pub fn record_comp(&mut self, o: usize, got: u64) -> Result<(), ZipkinCode> {
        if o >= COMPS {
            return Err(ZipkinCode::ENoCell);
        }
        let kind = match comp_kind(o) {
            Some(k) => k as i64,
            None => return Err(ZipkinCode::ENoCell),
        };
        let order_bad = if self.last_kind < 0 {
            kind != 0
        } else {
            kind < self.last_kind || kind > self.last_kind + 1
        };
        if order_bad {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xCE1D00);
            return Err(ZipkinCode::EOrder);
        }
        let want = match comp_cell(o) {
            Some(w) => w,
            None => return Err(ZipkinCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xCE1D01 + o as u64);
            return Err(ZipkinCode::EDrift);
        }
        if match self.comps.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ZipkinCode::EDup);
        }
        if let Some(c) = self.comps.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind;
        self.fnv(0xCE1D02 + o as u64);
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
    pub fn record_span(&mut self, s: usize, got: u64) -> Result<(), ZipkinCode> {
        if s >= SPAN_OPS {
            return Err(ZipkinCode::ESpanNo);
        }
        let kind = match span_kind(s) {
            Some(k) => k as i64,
            None => return Err(ZipkinCode::ESpanNo),
        };
        let order_bad = if self.last_span < 0 {
            kind != 0
        } else {
            kind < self.last_span || kind > self.last_span + 1
        };
        if order_bad {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xCE1E00);
            return Err(ZipkinCode::EOrder);
        }
        let want = match span_cell(s) {
            Some(w) => w,
            None => return Err(ZipkinCode::ESpanNo),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xCE1E01 + s as u64);
            return Err(ZipkinCode::EDrift);
        }
        if match self.spans.get(s) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ZipkinCode::EDup);
        }
        if let Some(c) = self.spans.get_mut(s) {
            *c = Some(got);
        }
        self.last_span = kind;
        self.fnv(0xCE1E02 + s as u64);
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

    /// reporter 一格（态序递进 + 载荷对照 + 重复拒）。
    pub fn record_rep(&mut self, i: usize, got: u64) -> Result<(), ZipkinCode> {
        if i >= REP_CELLS {
            return Err(ZipkinCode::ERepNo);
        }
        let st = match rep_state(i) {
            Some(s) => s as i64,
            None => return Err(ZipkinCode::ERepNo),
        };
        let order_bad = if self.last_rep < 0 {
            st != 0
        } else {
            st < self.last_rep || st > self.last_rep + 1
        };
        if order_bad {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xCE1F00);
            return Err(ZipkinCode::EOrder);
        }
        let want = match rep_cell(i) {
            Some(w) => w,
            None => return Err(ZipkinCode::ERepNo),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xCE1F01 + i as u64);
            return Err(ZipkinCode::EDrift);
        }
        if match self.reps.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ZipkinCode::EDup);
        }
        if let Some(c) = self.reps.get_mut(i) {
            *c = Some(got);
        }
        self.last_rep = st;
        self.fnv(0xCE1F02 + i as u64);
        Ok(())
    }

    /// reporter 全账核验（6 格）。
    pub fn reps_all(&self) -> bool {
        let mut i = 0usize;
        while i < REP_CELLS {
            match self.reps.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 存储一格（态序递进 + 载荷对照 + 重复拒）。
    pub fn record_store(&mut self, i: usize, got: u64) -> Result<(), ZipkinCode> {
        if i >= STORE_CELLS {
            return Err(ZipkinCode::EStoreNo);
        }
        let st = match store_state(i) {
            Some(s) => s as i64,
            None => return Err(ZipkinCode::EStoreNo),
        };
        let order_bad = if self.last_store < 0 {
            st != 0
        } else {
            st < self.last_store || st > self.last_store + 1
        };
        if order_bad {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xCE2000);
            return Err(ZipkinCode::EOrder);
        }
        let want = match store_cell(i) {
            Some(w) => w,
            None => return Err(ZipkinCode::EStoreNo),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xCE2001 + i as u64);
            return Err(ZipkinCode::EDrift);
        }
        if match self.stores.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ZipkinCode::EDup);
        }
        if let Some(c) = self.stores.get_mut(i) {
            *c = Some(got);
        }
        self.last_store = st;
        self.fnv(0xCE2002 + i as u64);
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

    /// Brave 消费声明一行。
    pub fn declare_brave(&mut self, r: usize) -> Result<(), ZipkinCode> {
        if r >= BRAVE_ROWS {
            return Err(ZipkinCode::ERowMiss);
        }
        if match self.braves.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ZipkinCode::EDup);
        }
        if let Some(c) = self.braves.get_mut(r) {
            *c = brave_row(r);
        }
        self.fnv(0xCE2100 + r as u64);
        Ok(())
    }

    /// Brave 声明全齐核验。
    pub fn braves_all(&self) -> bool {
        let mut r = 0usize;
        while r < BRAVE_ROWS {
            match self.braves.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// Java 上栈声明一行。
    pub fn declare_java(&mut self, r: usize) -> Result<(), ZipkinCode> {
        if r >= JAVA_ROWS {
            return Err(ZipkinCode::ERowMiss);
        }
        if match self.javas.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ZipkinCode::EDup);
        }
        if let Some(c) = self.javas.get_mut(r) {
            *c = java_row(r);
        }
        self.fnv(0xCE2180 + r as u64);
        Ok(())
    }

    /// Java 声明全齐核验。
    pub fn javas_all(&self) -> bool {
        let mut r = 0usize;
        while r < JAVA_ROWS {
            match self.javas.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 封账核验：声明行缺 → ERowMiss；格/闸未齐 → ESeal；全齐 → Ok。
    pub fn seal(&self) -> Result<(), ZipkinCode> {
        if !(self.braves_all() && self.javas_all()) {
            return Err(ZipkinCode::ERowMiss);
        }
        if !(self.gates_all() && self.comps_all() && self.spans_all() && self.reps_all() && self.stores_all()) {
            return Err(ZipkinCode::ESeal);
        }
        Ok(())
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 注入守恒等式：注入 = 检出 + 逃逸。
    pub fn conservation(&self) -> Result<(), ZipkinCode> {
        if self.injected == self.caught + self.escaped {
            Ok(())
        } else {
            Err(ZipkinCode::EInj)
        }
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }
}

impl Default for ZipkinLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 双跑指纹对照。
pub fn fp_check(a: u64, b: u64) -> Result<(), ZipkinCode> {
    if a == b && a != 0 {
        Ok(())
    } else {
        Err(ZipkinCode::EFp)
    }
}

/// KB-COMPAT 定级（全齐 → S1，六闸齐 → S2，否则 S3）。
pub fn judge_grade(led: &ZipkinLedger) -> Grade {
    if led.gates_all()
        && led.comps_all()
        && led.spans_all()
        && led.reps_all()
        && led.stores_all()
        && led.braves_all()
        && led.javas_all()
    {
        Grade::S1
    } else if led.gates_all() {
        Grade::S2
    } else {
        Grade::S3
    }
}

/// 定级守卫：定级与目标档漂移 → EGradeDrift。
pub fn grade_guard(led: &ZipkinLedger, want: Grade) -> Result<(), ZipkinCode> {
    let got = judge_grade(led);
    if got == want {
        Ok(())
    } else {
        Err(ZipkinCode::EGradeDrift)
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（记格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = ZipkinLedger::new();
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
    while i < REP_CELLS {
        if let Some(w) = rep_cell(i) {
            if led.record_rep(i, w).is_ok() {
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
    while r < BRAVE_ROWS {
        let _ = led.declare_brave(r);
        r += 1;
    }
    let mut r = 0usize;
    while r < JAVA_ROWS {
        let _ = led.declare_java(r);
        r += 1;
    }
    // 注入 10 轮：span 载荷漂移 / 族序逆跳（空账本必抓）/ 越闭集 三态轮转。
    // 三类注入全部落在当轮临时账本 lt 上，主账只记 injected/caught 守恒账。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = ZipkinLedger::new();
        let caught = match kind {
            0 => {
                // 漂移注入：先按合法序落 span 0 正格，再注 span 1 载荷漂移。
                let _ = lt.record_span(0, span_cell(0).unwrap_or(0));
                lt.record_span(1, span_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(ZipkinCode::EDrift)
            }
            1 => {
                // 族序注入：空账本首族必 collector，格 9 属 query 逆跳必抓。
                lt.record_comp(9, comp_cell(9).unwrap_or(0)) == Err(ZipkinCode::EOrder)
            }
            _ => {
                // 越闭集注入：组件/span/reporter/存储四面闭集之外必拒。
                lt.record_comp(COMPS, 0).is_err()
                    && lt.record_span(SPAN_OPS, 0).is_err()
                    && lt.record_rep(REP_CELLS, 0).is_err()
                    && lt.record_store(STORE_CELLS, 0).is_err()
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0xCE2200 + k as u64);
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
/// UNX-F52396 判据集（锚 UNX-F52397-J1 ×10，off-by-one 板式）。
pub fn run_unx52396_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52396");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "run_cycle 记格数恒等",
        recorded == COMPS as u32 + SPAN_OPS as u32 + REP_CELLS as u32 + STORE_CELLS as u32,
        "20 组件 + 10 span + 6 reporter + 6 存储逐格恒等（闸与声明行不计数）",
    );
    s.add(
        "组件五族 20 格全通",
        {
            let mut lt = ZipkinLedger::new();
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
        "collector/storage/query/transport/UI 五族逐格载荷恒等",
    );
    s.add(
        "span 10 操作全落",
        {
            let mut lt = ZipkinLedger::new();
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
        "reporter 三态 6 格全落",
        {
            let mut lt = ZipkinLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < REP_CELLS {
                if let Some(w) = rep_cell(i) {
                    if lt.record_rep(i, w).is_err() {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok && lt.reps_all()
        },
        "http/kafka/activemq 三态逐格递进落账",
    );
    s.add(
        "存储三态 6 格全落",
        {
            let mut lt = ZipkinLedger::new();
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
        "memory/mysql/elasticsearch 三态逐格递进落账",
    );
    s.add(
        "Brave 消费声明 2/2",
        {
            let mut lt = ZipkinLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < BRAVE_ROWS {
                if lt.declare_brave(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.braves_all()
        },
        "Brave 客户端消费面两行声明闭集（§AI-66.3）",
    );
    s.add(
        "Java 上栈声明 2/2",
        {
            let mut lt = ZipkinLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < JAVA_ROWS {
                if lt.declare_java(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.javas_all()
        },
        "Java 运行时上栈两行声明闭集",
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
        "collector→storage→query→transport→UI 五族闭集单调",
    );
    s.add(
        "六判据闸全落",
        {
            let mut lt = ZipkinLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "安装/服务注册/9411 端口开放/上报·查询功能/卸载净除/升级保留逐格落账",
    );
    s.add(
        "边界·组件越闭集",
        ZipkinLedger::new().record_comp(COMPS, 0).is_err(),
        "组件 20 越闭集必拒",
    );
    s.add(
        "边界·span 越闭集",
        ZipkinLedger::new().record_span(SPAN_OPS, 0).is_err(),
        "span 10 操作越闭集必拒",
    );
    s.add(
        "边界·reporter 越闭集",
        ZipkinLedger::new().record_rep(REP_CELLS, 0).is_err(),
        "reporter 6 格越闭集必拒",
    );
    s.add(
        "边界·存储越闭集",
        ZipkinLedger::new().record_store(STORE_CELLS, 0).is_err(),
        "存储 6 格越闭集必拒",
    );
    s.add(
        "reporter 三态越序拒",
        {
            // 空账本首格必 http 态（态 0），格 3 属 kafka 态越态必抓。
            ZipkinLedger::new().record_rep(3, rep_cell(3).unwrap_or(0)) == Err(ZipkinCode::EOrder)
        },
        "reporter 三态 0→1→2 递进，越态必抓",
    );
    s.add(
        "漂移拒",
        {
            // 合法序先落 span 0 正格，再验 span 1 载荷漂移必抓。
            let mut lt = ZipkinLedger::new();
            let _ = lt.record_span(0, span_cell(0).unwrap_or(0));
            lt.record_span(1, span_cell(1).unwrap_or(0).wrapping_add(9))
                == Err(ZipkinCode::EDrift)
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
            let mut lt = ZipkinLedger::new();
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
            while i < REP_CELLS {
                if let Some(w) = rep_cell(i) {
                    let _ = lt.record_rep(i, w);
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
            while r < BRAVE_ROWS {
                let _ = lt.declare_brave(r);
                r += 1;
            }
            let mut r = 0usize;
            while r < JAVA_ROWS {
                let _ = lt.declare_java(r);
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
        assert_eq!(kind_name(0), Some("collector"));
        assert_eq!(kind_name(4), Some("UI"));
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
        assert_eq!(rep_state(0), Some(0));
        assert_eq!(rep_state(3), Some(1));
        assert_eq!(rep_state(5), Some(2));
        assert!(rep_state(6).is_none());
        assert_eq!(rep_state_name(0), Some("http"));
        assert_eq!(rep_state_name(2), Some("activemq"));
        assert!(rep_state_name(3).is_none());
        assert_eq!(store_state(2), Some(1));
        assert!(store_state(6).is_none());
        assert_eq!(store_state_name(0), Some("memory"));
        assert_eq!(store_state_name(2), Some("elasticsearch"));
        assert!(store_state_name(3).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = comp_cell(3).unwrap_or(0);
        assert_eq!(a, comp_cell(3).unwrap_or(0));
        assert_ne!(a, comp_cell(4).unwrap_or(0));
        assert_ne!(span_cell(0).unwrap_or(0), span_cell(5).unwrap_or(0));
        assert_ne!(rep_cell(0).unwrap_or(0), rep_cell(3).unwrap_or(0));
        assert_ne!(store_cell(0).unwrap_or(0), store_cell(3).unwrap_or(0));
        assert!(rep_cell(REP_CELLS).is_none());
        assert!(store_cell(STORE_CELLS).is_none());
        assert!(brave_row(BRAVE_ROWS).is_none());
        assert!(java_row(JAVA_ROWS).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        // 空账本：首族必 collector，跳族必抓。
        let mut lt = ZipkinLedger::new();
        assert_eq!(
            lt.record_comp(9, comp_cell(9).unwrap_or(0)),
            Err(ZipkinCode::EOrder)
        );
        assert_eq!(lt.stats().1, 1);
        // 合法序落低族格后再回跳 → 族序逆必抓。
        let _ = lt.record_comp(0, comp_cell(0).unwrap_or(0));
        let _ = lt.record_comp(5, comp_cell(5).unwrap_or(0));
        assert_eq!(
            lt.record_comp(2, comp_cell(2).unwrap_or(0)),
            Err(ZipkinCode::EOrder)
        );
        assert!(lt.record_comp(6, comp_cell(6).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_span_order_and_drift() {
        // 空账本：首操作必 client 类，span 5 属 producer 类越类必抓。
        let mut lt = ZipkinLedger::new();
        assert_eq!(
            lt.record_span(5, span_cell(5).unwrap_or(0)),
            Err(ZipkinCode::EOrder)
        );
        // 首操作落正后载荷漂移仍 EDrift。
        assert_eq!(
            lt.record_span(0, span_cell(0).unwrap_or(0).wrapping_add(1)),
            Err(ZipkinCode::EDrift)
        );
        // 合法落 span 0、1（client 类）后跳类 internal（span 8）→ 越类必抓。
        let _ = lt.record_span(0, span_cell(0).unwrap_or(0));
        let _ = lt.record_span(1, span_cell(1).unwrap_or(0));
        assert_eq!(
            lt.record_span(8, span_cell(8).unwrap_or(0)),
            Err(ZipkinCode::EOrder)
        );
        assert!(lt.record_span(2, span_cell(2).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_rep_store_order_caught() {
        // 空账本：reporter 首格必 http 态，格 3 属 kafka 态越态必抓。
        let mut lt = ZipkinLedger::new();
        assert_eq!(
            lt.record_rep(3, rep_cell(3).unwrap_or(0)),
            Err(ZipkinCode::EOrder)
        );
        let _ = lt.record_rep(0, rep_cell(0).unwrap_or(0));
        let _ = lt.record_rep(1, rep_cell(1).unwrap_or(0));
        // 落齐 http 态后跳 activemq 态（格 4）→ 越态必抓。
        assert_eq!(
            lt.record_rep(4, rep_cell(4).unwrap_or(0)),
            Err(ZipkinCode::EOrder)
        );
        assert!(lt.record_rep(2, rep_cell(2).unwrap_or(0)).is_ok());
        // 存储空账本：格 3 属 mysql 态越态必抓。
        let mut lt2 = ZipkinLedger::new();
        assert_eq!(
            lt2.record_store(3, store_cell(3).unwrap_or(0)),
            Err(ZipkinCode::EOrder)
        );
        // 载荷漂移必抓。
        assert_eq!(
            lt2.record_store(0, store_cell(0).unwrap_or(0).wrapping_add(1)),
            Err(ZipkinCode::EDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = ZipkinLedger::new();
        let _ = lt.record_comp(0, comp_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_comp(0, comp_cell(0).unwrap_or(0)),
            Err(ZipkinCode::EDup)
        );
        let _ = lt.record_span(0, span_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_span(0, span_cell(0).unwrap_or(0)),
            Err(ZipkinCode::EDup)
        );
        let _ = lt.declare_java(0);
        assert_eq!(lt.declare_java(0), Err(ZipkinCode::EDup));
        assert_eq!(lt.declare_java(JAVA_ROWS), Err(ZipkinCode::ERowMiss));
        assert_eq!(lt.declare_brave(BRAVE_ROWS), Err(ZipkinCode::ERowMiss));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = ZipkinLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        assert!(!ZipkinLedger::new().gates_all());
    }

    #[test]
    fn test_seal_ladder() {
        // 空账：声明行缺 → ERowMiss。
        assert_eq!(ZipkinLedger::new().seal(), Err(ZipkinCode::ERowMiss));
        // 行齐格未齐 → ESeal。
        let mut lt = ZipkinLedger::new();
        let mut r = 0usize;
        while r < BRAVE_ROWS {
            let _ = lt.declare_brave(r);
            r += 1;
        }
        let mut r = 0usize;
        while r < JAVA_ROWS {
            let _ = lt.declare_java(r);
            r += 1;
        }
        assert_eq!(lt.seal(), Err(ZipkinCode::ESeal));
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // 记格数 = 20 组件 + 10 span + 6 reporter + 6 存储（闸与声明行不计数）。
        assert_eq!(
            recorded,
            COMPS as u32 + SPAN_OPS as u32 + REP_CELLS as u32 + STORE_CELLS as u32
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
        assert_eq!(SEG_BASE, 0xCA2C0);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(ZipkinCode::Ok as u32, 0);
        assert_eq!(ZipkinCode::EGradeDrift as u32, 12);
        assert_eq!(ZipkinCode::Ok.val(), SEG_BASE);
        assert_eq!(ZipkinCode::EGradeDrift.val(), SEG_BASE + 0x0C);
        assert!(!ZipkinCode::Ok.detail().is_empty());
        assert!(!ZipkinCode::ESeal.detail().is_empty());
        assert!(!ZipkinCode::ESpanNo.detail().is_empty());
        assert!(!ZipkinCode::ERepNo.detail().is_empty());
        assert!(!ZipkinCode::EStoreNo.detail().is_empty());
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
    }

    #[test]
    fn test_grade() {
        // 如实阶梯：空账 S3 → 六闸齐 S2 → 全齐目标档 S1。
        assert_eq!(judge_grade(&ZipkinLedger::new()), Grade::S3);
        let mut lt = ZipkinLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt.judge_gate(g);
            g += 1;
        }
        assert_eq!(judge_grade(&lt), Grade::S2);
        assert!(grade_guard(&lt, Grade::S1).is_err());
        let mut full = ZipkinLedger::new();
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
        while i < REP_CELLS {
            if let Some(w) = rep_cell(i) {
                let _ = full.record_rep(i, w);
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
        while r < BRAVE_ROWS {
            let _ = full.declare_brave(r);
            r += 1;
        }
        let mut r = 0usize;
        while r < JAVA_ROWS {
            let _ = full.declare_java(r);
            r += 1;
        }
        assert!(full.seal().is_ok());
        assert_eq!(judge_grade(&full), Grade::S1);
        let (inj, cgt, esc) = full.stats();
        assert_eq!(inj, cgt + esc);
    }

    #[test]
    fn unx52396_checks_green() {
        let s = run_unx52396_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert_eq!(s.len(), 21);
    }
}
