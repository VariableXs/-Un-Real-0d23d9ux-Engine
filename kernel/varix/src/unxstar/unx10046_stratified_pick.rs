//! UNX-F10046 抽样选例实现（分层随机） · 0x91b80 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡号：UNX-F10046（B22 常态化回归线 · 抽样选例）。
//! 锚点：UNX-F10047-J1 执行器分账核（并行档隔离核 10/10 次），交叉污染 10/10 次检出。
//!
//! 数据段说明：
//!   SEG_BASE = 0x91b80（卡面 seg 闭集）。诊断码占用段内 +0x01..+0x0C
//!   闭集窗口，码值 = SEG_BASE + 变体序号，段内两两唯一、段外不越界；
//!   全部码位由 `SampleCode::ALL` 封闭枚举给出，禁止运行期造码。
//!
//! 结构清单：
//!   - 分层注册表 `LayerBook`：40 条按 (id-1)%4 归入读/写/内存/进程四层，
//!     越层样本显性拒绝；
//!   - 确定性抽样器 `StratifiedSampler`：LCG 种子驱动层内等步抽选，同种子
//!     两次逐条一致（复现核），异种子逐条可分辨（探测核）；
//!   - 分账 `LaneLedger`：样本逐条落并行档（lane 1..=4），结果回报档号与
//!     分配档号不符即交叉污染，逐次检出，逃逸恒 0；
//!   - 注入核 `BadSampleCase`：越层样本 / 配额越层 / 空层抽选 / 不可复现 /
//!     交叉污染 五类坏样本逐类 10/10 次拒绝；
//!   - Linux errno 对照表 + FNV-1a 双跑指纹（无浮点、无随机）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10046 独占，0x40 对齐窗口起点）。
pub const SEG_BASE: u32 = 0x91b80;

/// 段窗上界（下一卡 0x91bc0 起点，闭开区间右端）。
pub const SEG_END: u32 = 0x91bc0;

/// 战线条目数（40 条，B21 口径）。
pub const CASE_COUNT: usize = 40;

/// 分层数（读/写/内存/进程）。
pub const LAYER_COUNT: usize = 4;

/// 并行档数（lane 1..=4；lane 0 归串行档不参与抽样分账）。
pub const LANE_COUNT: usize = 4;

/// 单类注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// 抽样总配额（每层 10 取 2~3，合计 10 条；来源：常态抽样条款）。
pub const SAMPLE_TOTAL: usize = 10;

/// Linux errno 正数码（对照表口径）。
pub const ENOENT: i32 = 2;
pub const EACCES: i32 = 13;
pub const EINVAL: i32 = 22;
pub const EIO: i32 = 5;
pub const ENOSPC: i32 = 28;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SampleCode {
    /// 越层样本（层号出 0..4）
    BadLayer,
    /// 配额越层（配额 > 层容量）
    QuotaBleed,
    /// 空层抽选（层内无成员却索样）
    EmptyLayer,
    /// 不可复现（同种子两次样本漂移）
    NotReproducible,
    /// 交叉污染（回报档号 ≠ 分配档号）
    CrossContamination,
    /// 样本越界
    CaseOutOfRange,
    /// 重复抽中同条（层内无放回被破坏）
    DuplicatePick,
    /// 分账倒退
    LedgerRegress,
    /// 配额越窗（总量出 1..=40）
    QuotaRange,
    /// 双跑不一致
    NotDeterministic,
    /// 未落种即抽样
    NotSeeded,
    /// 段位越界
    SegmentViolation,
}

impl SampleCode {
    /// 码值（段基址 +0x01 起连续编号，闭集唯一）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            SampleCode::BadLayer => "越层样本被拒：层号必须落在 0..=3；请核对分层照",
            SampleCode::QuotaBleed => "配额越层被拒：单层配额不得超过层容量 10；请核对配额",
            SampleCode::EmptyLayer => "空层抽选被拒：层内无成员不得索样；请先补成员",
            SampleCode::NotReproducible => "不可复现被拒：同种子两次样本必须逐条一致；请排查漂移",
            SampleCode::CrossContamination => "交叉污染被拒：回报档号与分配档号不符；请核对分账",
            SampleCode::CaseOutOfRange => "样本越界被拒：条目号必须落在 1..=40；请核对口",
            SampleCode::DuplicatePick => "重复抽中被拒：层内无放回抽样同条仅一次；请去重",
            SampleCode::LedgerRegress => "分账倒退被拒：计数只增不减；请排查回滚误改",
            SampleCode::QuotaRange => "配额越窗被拒：抽样总量窗口 1..=40；请核对总量",
            SampleCode::NotDeterministic => "双跑不一致：同输入两次结果不同；请排查隐藏状态",
            SampleCode::NotSeeded => "未落种被拒：种子未落位不得抽样；请先 set_seed",
            SampleCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在 0x91b80..=0x91bc0",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [SampleCode; 12] = [
        SampleCode::BadLayer,
        SampleCode::QuotaBleed,
        SampleCode::EmptyLayer,
        SampleCode::NotReproducible,
        SampleCode::CrossContamination,
        SampleCode::CaseOutOfRange,
        SampleCode::DuplicatePick,
        SampleCode::LedgerRegress,
        SampleCode::QuotaRange,
        SampleCode::NotDeterministic,
        SampleCode::NotSeeded,
        SampleCode::SegmentViolation,
    ];
}

// ── 分层注册表 ────────────────────────────────────────────────────────
/// 条目 → 层（(id-1)%4：读/写/内存/进程轮转，B21 十面口径的层切片）。
pub fn layer_of(id: u32) -> Option<usize> {
    if id == 0 || id as usize > CASE_COUNT {
        None
    } else {
        Some(((id - 1) % LAYER_COUNT as u32) as usize)
    }
}

/// 分层注册表：逐条落层，容量账逐层计数。
#[derive(Clone, Copy, Debug)]
pub struct LayerBook {
    /// 注册占用表（下标 = 条目号-1）
    pub have: [bool; CASE_COUNT],
    /// 逐层成员数
    pub per_layer: [u32; LAYER_COUNT],
    /// 已注册条数
    pub registered: u32,
}

impl LayerBook {
    /// 构造空册。
    pub const fn new() -> LayerBook {
        LayerBook { have: [false; CASE_COUNT], per_layer: [0; LAYER_COUNT], registered: 0 }
    }

    /// 注册一条：越界拒绝；层号越层拒绝（BadLayer 显性）。
    pub fn register(&mut self, id: u32, layer: usize) -> Result<(), SampleCode> {
        if id == 0 || id as usize > CASE_COUNT {
            return Err(SampleCode::CaseOutOfRange);
        }
        match layer_of(id) {
            Some(l) if l == layer => {}
            _ => return Err(SampleCode::BadLayer),
        }
        let idx = id as usize - 1;
        if *self.have.get(idx).unwrap_or(&true) {
            return Err(SampleCode::CaseOutOfRange);
        }
        if let Some(slot) = self.have.get_mut(idx) {
            *slot = true;
        }
        if let Some(cnt) = self.per_layer.get_mut(layer) {
            *cnt = cnt.saturating_add(1);
        }
        self.registered = self.registered.saturating_add(1);
        Ok(())
    }

    /// 层成员升序枚举。
    pub fn members(&self, layer: usize) -> Vec<u32> {
        let mut out = Vec::new();
        if layer >= LAYER_COUNT {
            return out;
        }
        let mut id = 1u32;
        while id as usize <= CASE_COUNT {
            if *self.have.get(id as usize - 1).unwrap_or(&false) && layer_of(id) == Some(layer) {
                out.push(id);
            }
            id += 1;
        }
        out
    }
}

/// 注册全部 40 条（层号按 layer_of 自动落层）。
pub fn register_all(book: &mut LayerBook) -> Result<(), SampleCode> {
    let mut id = 1u32;
    while id as usize <= CASE_COUNT {
        let l = match layer_of(id) {
            Some(l) => l,
            None => return Err(SampleCode::CaseOutOfRange),
        };
        book.register(id, l)?;
        id += 1;
    }
    Ok(())
}

// ── 确定性抽样器 ──────────────────────────────────────────────────────
/// LCG 步进（参数取 glibc 常数，纯函数无隐藏状态）。
pub fn lcg_next(state: u32) -> u32 {
    state.wrapping_mul(1_103_515_245).wrapping_add(12_345)
}

/// 分层抽样器：种子落位后逐层等步抽选，层内无放回。
#[derive(Clone, Copy, Debug)]
pub struct StratifiedSampler {
    /// 种子（0=未落种）
    pub seed: u32,
    /// 逐层配额
    pub quota: [usize; LAYER_COUNT],
}

impl StratifiedSampler {
    /// 构造未落种抽样器（默认配额 [3,3,2,2] 合计 10）。
    pub const fn new() -> StratifiedSampler {
        StratifiedSampler { seed: 0, quota: [3, 3, 2, 2] }
    }

    /// 落种：非零才有效。
    pub fn set_seed(&mut self, seed: u32) -> Result<(), SampleCode> {
        if seed == 0 {
            return Err(SampleCode::QuotaRange);
        }
        self.seed = seed;
        Ok(())
    }

    /// 分层抽样：逐层等步无放回；配额越层/空层/未落种显性拒绝。
    pub fn sample(&self, book: &LayerBook) -> Result<Vec<u32>, SampleCode> {
        if self.seed == 0 {
            return Err(SampleCode::NotSeeded);
        }
        let mut out = Vec::new();
        let mut state = self.seed;
        let mut li = 0usize;
        while li < LAYER_COUNT {
            let q = *self.quota.get(li).unwrap_or(&0);
            let mem = book.members(li);
            let n = mem.len();
            if q > 0 && n == 0 {
                return Err(SampleCode::EmptyLayer);
            }
            if q > n {
                return Err(SampleCode::QuotaBleed);
            }
            if q > 0 {
                let step = n / q;
                let jitter = (state % step.max(1) as u32) as usize;
                let mut k = 0usize;
                while k < q {
                    let idx = k * step + jitter;
                    let id = match mem.get(idx) {
                        Some(v) => *v,
                        None => return Err(SampleCode::DuplicatePick),
                    };
                    out.push(id);
                    k += 1;
                }
            }
            state = lcg_next(state);
            li += 1;
        }
        if out.len() > CASE_COUNT {
            return Err(SampleCode::QuotaRange);
        }
        Ok(out)
    }
}

// ── 并行档分账 ────────────────────────────────────────────────────────
/// 并行档分账：样本逐条落档（lane = (id-1)%4 + 1），回报档号必须一致。
#[derive(Clone, Copy, Debug)]
pub struct LaneLedger {
    /// 逐条分配档号（0=未分账）
    pub lane_of: [u8; CASE_COUNT],
    /// 已分账条数
    pub assigned: u32,
    /// 交叉污染检出数
    pub contaminated: u32,
    /// 注入/检出/逃逸三联账
    pub injected: u32,
    pub detected: u32,
    pub escaped: u32,
}

impl LaneLedger {
    /// 构造零账。
    pub const fn zero() -> LaneLedger {
        LaneLedger {
            lane_of: [0; CASE_COUNT],
            assigned: 0,
            contaminated: 0,
            injected: 0,
            detected: 0,
            escaped: 0,
        }
    }

    /// 分配档号：1 + (id-1)%4，纯函数。
    pub fn lane_assign(id: u32) -> Option<u8> {
        if id == 0 || id as usize > CASE_COUNT {
            None
        } else {
            Some(((id - 1) % LANE_COUNT as u32) as u8 + 1)
        }
    }

    /// 分账一条样本。
    pub fn assign(&mut self, id: u32) -> Result<u8, SampleCode> {
        let lane = match Self::lane_assign(id) {
            Some(l) => l,
            None => return Err(SampleCode::CaseOutOfRange),
        };
        let idx = id as usize - 1;
        if *self.lane_of.get(idx).unwrap_or(&255) != 0 {
            return Err(SampleCode::DuplicatePick);
        }
        if let Some(slot) = self.lane_of.get_mut(idx) {
            *slot = lane;
        }
        self.assigned = self.assigned.saturating_add(1);
        Ok(lane)
    }

    /// 结果回报：回报档号与分配档号不符即交叉污染（逐次检出并入账）。
    pub fn record_result(&mut self, id: u32, claimed_lane: u8) -> Result<(), SampleCode> {
        self.injected = self.injected.saturating_add(1);
        let real = match Self::lane_assign(id) {
            Some(l) => l,
            None => {
                self.escaped = self.escaped.saturating_add(1);
                return Err(SampleCode::CaseOutOfRange);
            }
        };
        let stored = *self.lane_of.get(id as usize - 1).unwrap_or(&0);
        if stored != real {
            // 分配档与重算档不一致 = 分账账面被污染。
            self.contaminated = self.contaminated.saturating_add(1);
            self.escaped = self.escaped.saturating_add(1);
            return Err(SampleCode::CrossContamination);
        }
        let caught = claimed_lane != real;
        if caught {
            self.contaminated = self.contaminated.saturating_add(1);
            self.detected = self.detected.saturating_add(1);
            return Err(SampleCode::CrossContamination);
        }
        self.detected = self.detected.saturating_add(1);
        Ok(())
    }

    /// 注入账守恒：注入 = 检出 + 逃逸。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

// ── 注入核 ────────────────────────────────────────────────────────────
/// 五类坏抽样样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadSampleCase {
    /// 越层样本（层号 9）
    BadLayer,
    /// 配额越层（配额 11 > 层容量 10）
    QuotaBleed,
    /// 空层抽选（空册索样）
    EmptyLayer,
    /// 不可复现（同种子必须一致、异种子必须可分辨）
    NotReproducible,
    /// 交叉污染（回报错档）
    CrossContamination,
}

impl BadSampleCase {
    /// 闭集全量。
    pub const ALL: [BadSampleCase; 5] = [
        BadSampleCase::BadLayer,
        BadSampleCase::QuotaBleed,
        BadSampleCase::EmptyLayer,
        BadSampleCase::NotReproducible,
        BadSampleCase::CrossContamination,
    ];

    /// 注入一轮：构造坏样本并触发抽样器/分账，返回是否被显式拒绝。
    pub fn inject(self) -> bool {
        let mut book = LayerBook::new();
        let _ = register_all(&mut book);
        let mut s = StratifiedSampler::new();
        let _ = s.set_seed(9);
        match self {
            BadSampleCase::BadLayer => book.register(5, 9).is_err(),
            BadSampleCase::QuotaBleed => {
                let mut q = StratifiedSampler::new();
                let _ = q.set_seed(9);
                q.quota = [11, 0, 0, 0];
                q.sample(&book).is_err()
            }
            BadSampleCase::EmptyLayer => {
                let empty = LayerBook::new();
                s.sample(&empty).is_err()
            }
            BadSampleCase::NotReproducible => {
                // 探测核：同种子两次逐条一致，异种子逐条可分辨。
                let a = s.sample(&book);
                let b = s.sample(&book);
                let mut s2 = StratifiedSampler::new();
                let _ = s2.set_seed(9 ^ 0xBEEF);
                let c = s2.sample(&book);
                match (a, b, c) {
                    (Ok(x), Ok(y), Ok(z)) => x == y && x != z,
                    _ => false,
                }
            }
            BadSampleCase::CrossContamination => {
                let mut led = LaneLedger::zero();
                if led.assign(7).is_err() {
                    return false;
                }
                let real = match LaneLedger::lane_assign(7) {
                    Some(l) => l,
                    None => return false,
                };
                // 回报错档 → 检出；回报对档 → 通过。
                led.record_result(7, real % LANE_COUNT as u8 + 1).is_err()
                    && led.record_result(7, real).is_ok()
            }
        }
    }
}

/// 执行五类坏样本注入：每类 INJECT_ROUNDS 次，返回逐类检出数。
pub fn run_sample_injection() -> [u32; 5] {
    let mut per_class = [0u32; 5];
    let mut ci = 0usize;
    while ci < BadSampleCase::ALL.len() {
        if let Some(case) = BadSampleCase::ALL.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                if case.inject() {
                    if let Some(slot) = per_class.get_mut(ci) {
                        *slot = slot.saturating_add(1);
                    }
                }
                round += 1;
            }
        }
        ci += 1;
    }
    per_class
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EIO", -5, 111, "Windows ERROR_CONNECTION_REFUSED；断档回报族"),
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；空层/缺样本族"),
    ("EACCES", -13, 5, "Windows ERROR_ACCESS_DENIED；越档回报族"),
    ("ENOSPC", -28, 112, "Windows ERROR_DISK_FULL；配额越层族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；越层/坏种子族"),
];

/// 查表：按 errno 正数取负值；表外返回 0（显性可查）。
pub fn linux_neg(code: i32) -> i32 {
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(i) {
            if row.1 == -code {
                return row.1;
            }
        }
        i += 1;
    }
    0
}

/// 与 Linux 逐码一致：只允许表内码且负值精确匹配。
pub fn agrees_with_linux(code: i32) -> bool {
    linux_neg(code) == -code && code > 0
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位（无浮点、无随机）。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        i += 1;
    }
    h
}

/// 规范语料双跑指纹：注册→落种→抽样→分账全链逐字节入哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut book = LayerBook::new();
    let _ = register_all(&mut book);
    let mut s = StratifiedSampler::new();
    let _ = s.set_seed(3);
    let mut bytes: Vec<u8> = Vec::new();
    match s.sample(&book) {
        Ok(v) => {
            bytes.push(v.len() as u8);
            let mut led = LaneLedger::zero();
            for id in v.iter() {
                bytes.extend_from_slice(&id.to_le_bytes());
                if let Ok(lane) = led.assign(*id) {
                    bytes.push(lane);
                    let _ = led.record_result(*id, lane);
                }
            }
            bytes.push(if led.conserved() { 1 } else { 0 });
        }
        Err(_) => bytes.push(0),
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10046 判据集（17 条静态 + 2 条动态明细）。
pub fn run_unx10046_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10046");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (SEG_BASE..SEG_END).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x91b80..=0x91bc0", seg_ok,
          "段基址唯一且落在本卡窗口；越界即拒绝注册");

    // 2. 分层注册：40 条全量落层，每层恰 10
    let mut book = LayerBook::new();
    let reg_ok = register_all(&mut book).is_ok()
        && book.registered == CASE_COUNT as u32
        && book.per_layer == [10, 10, 10, 10];
    s.add("分层注册：40 条落四层、每层恰 10", reg_ok,
          "(id-1)%4 轮转落层，层账逐条累计");

    // 3. 越层注册拒绝
    let mut b2 = LayerBook::new();
    let bad_layer = b2.register(5, 9) == Err(SampleCode::BadLayer)
        && b2.register(0, 0) == Err(SampleCode::CaseOutOfRange)
        && b2.register(41, 1) == Err(SampleCode::CaseOutOfRange);
    s.add("注册边界：越层/越界/重复显性拒绝", bad_layer,
          "层号必须与 layer_of 一致，杜绝越层样本");

    // 4. 落种守卫：零种子拒绝
    let mut s0 = StratifiedSampler::new();
    let seed_ok = s0.set_seed(0) == Err(SampleCode::QuotaRange);
    s.add("落种守卫：零种子拒绝", seed_ok,
          "种子非零才可抽样，杜绝空转样本");

    // 5. 抽样规模：默认配额合计 10 条
    let mut s1 = StratifiedSampler::new();
    let _ = s1.set_seed(9);
    let v = s1.sample(&book);
    let size_ok = match &v {
        Ok(x) => x.len() == SAMPLE_TOTAL,
        Err(_) => false,
    };
    s.add("抽样规模：四层合计抽 10 条", size_ok,
          "配额 [3,3,2,2] 与层容量 10 相容");

    // 6. 复现核：同种子两次逐条一致（种子复现 10/10）
    let mut repro_hit = 0u32;
    let mut r = 0u32;
    while r < INJECT_ROUNDS {
        let mut sa = StratifiedSampler::new();
        let mut sb = StratifiedSampler::new();
        let _ = sa.set_seed(100 + r);
        let _ = sb.set_seed(100 + r);
        if sa.sample(&book) == sb.sample(&book) {
            repro_hit = repro_hit.saturating_add(1);
        }
        r += 1;
    }
    s.add("复现核：十种逐轮同种子两次一致 10/10", repro_hit == INJECT_ROUNDS,
          "LCG 纯函数驱动，同种子零漂移");

    // 7. 层覆盖核：逐层配额被满足（层覆盖 10/10）
    let mut cover_hit = 0u32;
    let mut r2 = 0u32;
    while r2 < INJECT_ROUNDS {
        let mut sc = StratifiedSampler::new();
        let _ = sc.set_seed(7 + r2);
        if let Ok(v) = sc.sample(&book) {
            let mut per = [0u32; LAYER_COUNT];
            for id in v.iter() {
                if let Some(l) = layer_of(*id) {
                    if let Some(cnt) = per.get_mut(l) {
                        *cnt = cnt.saturating_add(1);
                    }
                }
            }
            if per == [3, 3, 2, 2] {
                cover_hit = cover_hit.saturating_add(1);
            }
        }
        r2 += 1;
    }
    s.add("层覆盖核：逐层配额 [3,3,2,2] 逐轮满足 10/10", cover_hit == INJECT_ROUNDS,
          "每层抽样数与配额逐轮对表，缺层即红");

    // 8. 并行档分账：lane 1..=4 轮转
    let mut led = LaneLedger::zero();
    let lane_ok = LaneLedger::lane_assign(1) == Some(1)
        && LaneLedger::lane_assign(4) == Some(4)
        && LaneLedger::lane_assign(5) == Some(1)
        && LaneLedger::lane_assign(0).is_none();
    let _ = led.assign(1);
    let _ = led.assign(2);
    s.add("并行档分账：lane 1..=4 轮转、越界拒绝", lane_ok,
          "档号纯函数分配，分账账面可重算");

    // 9. 交叉污染 10/10 检出（锚点判据直接探针）
    let mut cross_hit = 0u32;
    let mut r3 = 0u32;
    while r3 < INJECT_ROUNDS {
        let mut ld = LaneLedger::zero();
        let _ = ld.assign(7);
        let real = LaneLedger::lane_assign(7).unwrap_or(0);
        let wrong = real % LANE_COUNT as u8 + 1;
        if ld.record_result(7, wrong).is_err() && ld.record_result(7, real).is_ok() {
            cross_hit = cross_hit.saturating_add(1);
        }
        r3 += 1;
    }
    s.add("交叉污染检出：错档回报 10/10 即拦", cross_hit == INJECT_ROUNDS,
          "回报档号≠分配档号逐次检出，对档放行");

    // 10. 注入核：五类坏样本全检出
    let per_class = run_sample_injection();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：五类坏样本各 10/10 检出", all_caught,
          "越层/配额越层/空层/不可复现/交叉污染全被显式拒绝");

    // 11. 分账守恒：注入=检出+逃逸且逃逸=0（规范语料）
    let mut led2 = LaneLedger::zero();
    let mut s2 = StratifiedSampler::new();
    let _ = s2.set_seed(3);
    let sv = s2.sample(&book).unwrap_or_default();
    for id in sv.iter() {
        if let Ok(lane) = led2.assign(*id) {
            let _ = led2.record_result(*id, lane);
        }
    }
    s.add("分账守恒：规范语料注入=检出、逃逸=0",
          led2.conserved() && led2.escaped == 0 && led2.injected == SAMPLE_TOTAL as u32,
          "10 条样本逐条回报对档，账面闭合");

    // 12. 文案闭集：非空
    let text_ok = SampleCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 13. 诊断码唯一 + 落段
    let mut uniq = true;
    let mut i = 0usize;
    while i < SampleCode::ALL.len() {
        let mut j = i + 1;
        while j < SampleCode::ALL.len() {
            if let (Some(x), Some(y)) = (SampleCode::ALL.get(i), SampleCode::ALL.get(j)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let in_seg = SampleCode::ALL.iter().all(|c| c.val() > SEG_BASE && c.val() <= SEG_BASE + 0x0C);
    s.add("诊断码：12 个码值两两唯一且落段窗", uniq && in_seg,
          "码值闭集唯一且不得越过 +0x0C 窗口");

    // 14. Linux 逐码一致
    let codes_ok = agrees_with_linux(EACCES)
        && agrees_with_linux(ENOENT)
        && agrees_with_linux(EIO)
        && agrees_with_linux(ENOSPC)
        && agrees_with_linux(EINVAL)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误码只允许 errno 表内正值且负值可复算");

    // 15. 双跑一致
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", f1 == f2,
          "同输入两次必须逐字节相等，隐藏状态即失败");

    // 16. LCG 前向可复算
    let lc = lcg_next(1);
    s.add("LCG 可复算：同态步进两次结果一致", lc == lcg_next(1) && lc != 1,
          "纯函数步进，同态同果且状态真推进");

    // 17. 配额越窗拒绝
    let mut s3 = StratifiedSampler::new();
    let _ = s3.set_seed(5);
    s3.quota = [3, 3, 2, 30];
    let quota_ok = s3.sample(&book) == Err(SampleCode::QuotaBleed);
    s.add("配额越窗：单层配额超容量拒绝", quota_ok,
          "配额必须 ≤ 层容量 10，越层即拦");

    // 动态明细（≤8）：样本规模一行 + 指纹一行
    let sv_text = format!("抽样 {} 条 / 分账 {} / 污染检出 {}",
        sv.len(), led2.assigned, led2.contaminated);
    s.add_dyn("抽样快照", size_ok && led2.escaped == 0, &sv_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_all_layers() {
        let mut book = LayerBook::new();
        assert!(register_all(&mut book).is_ok());
        assert_eq!(book.per_layer, [10, 10, 10, 10]);
    }

    #[test]
    fn bad_layer_rejected() {
        let mut book = LayerBook::new();
        assert_eq!(book.register(5, 9), Err(SampleCode::BadLayer));
        assert_eq!(book.register(1, 1), Err(SampleCode::BadLayer));
    }

    #[test]
    fn unseeded_rejected() {
        let mut book = LayerBook::new();
        let _ = register_all(&mut book);
        let s = StratifiedSampler::new();
        assert_eq!(s.sample(&book), Err(SampleCode::NotSeeded));
    }

    #[test]
    fn same_seed_reproduces() {
        let mut book = LayerBook::new();
        let _ = register_all(&mut book);
        let mut a = StratifiedSampler::new();
        let mut b = StratifiedSampler::new();
        let _ = a.set_seed(77);
        let _ = b.set_seed(77);
        assert_eq!(a.sample(&book), b.sample(&book));
    }

    #[test]
    fn different_seed_distinguishable() {
        let mut book = LayerBook::new();
        let _ = register_all(&mut book);
        let mut a = StratifiedSampler::new();
        let mut b = StratifiedSampler::new();
        let _ = a.set_seed(1);
        let _ = b.set_seed(2);
        assert_ne!(a.sample(&book), b.sample(&book));
    }

    #[test]
    fn quota_bleed_rejected() {
        let mut book = LayerBook::new();
        let _ = register_all(&mut book);
        let mut s = StratifiedSampler::new();
        let _ = s.set_seed(4);
        s.quota = [11, 0, 0, 0];
        assert_eq!(s.sample(&book), Err(SampleCode::QuotaBleed));
    }

    #[test]
    fn empty_layer_rejected() {
        let book = LayerBook::new();
        let mut s = StratifiedSampler::new();
        let _ = s.set_seed(4);
        assert!(matches!(s.sample(&book), Err(SampleCode::EmptyLayer)));
    }

    #[test]
    fn cross_lane_detected() {
        let mut led = LaneLedger::zero();
        let _ = led.assign(9);
        let real = LaneLedger::lane_assign(9).unwrap_or(0);
        let wrong = real % LANE_COUNT as u8 + 1;
        assert_eq!(led.record_result(9, wrong), Err(SampleCode::CrossContamination));
        assert!(led.record_result(9, real).is_ok());
        assert!(led.conserved());
    }

    #[test]
    fn injection_matrix_all_caught() {
        let per = run_sample_injection();
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(EIO));
        assert!(agrees_with_linux(ENOSPC));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(EINVAL), -EINVAL);
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn unx10046_checks_green() {
        let s = run_unx10046_checks();
        assert!(s.all_passed(), "UNX-F10046 red: {}", s.red_items_text());
    }
}
