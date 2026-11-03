//! UNX-F0478 坏配置注入演练（烂设备语料注入） · 0x75940段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（DISPATCH_w7I 逐字锚定，AI-VX7-1104 批4 I 组）：
//!   三类烂设备（身份烂/头烂/BAR 槽错位）检出 3/3，好设备误伤 0，
//!   演练账只追加不抹账。
//! 逐卡要点（分派表逐字锚定）：
//!   三类烂设备注入闭集（身份烂=vendor 0xFFFF 带身份行/头烂=头类型>2/
//!   BAR 错位=槽索引越位）3/3 检出，12 好设备语料误伤 0；演练账
//!   （批次序+类型+命中位）只追加，批次单调，双跑一致。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   ① 样本模型：每个样本带（vendor, device, revision）身份行 + 头
//!     类型 + BAR 槽索引表（每槽 ≤6、升序不重复）——三面各设一道
//!     闸，坏样本至少破一道闸；
//!   ② 三类烂闭集：身份烂=vendor==0xFFFF 却带身份行（缺席口径与
//!     身份面自相矛盾）；头烂=头类型 >2（00/01/02 之外的非法型）；
//!     BAR 错位=任一槽索引 ≥6 或重复或乱序（槽索引越位）——诊断码
//!     逐类定位，检出不混类；
//!   ③ 误伤闸：12 好设备语料逐个过三闸必须全放行（GoodMarkedBad=
//!     误伤即红线），边界样本（vendor!=0xFFFF、头类型==2、槽索引
//!     ==5）显性覆盖；
//!   ④ 演练账只追加：账行为（批次序, 烂类, 命中位集），无删除/抹改
//!     入口——抹账调用显性拒（EraseDenied），批次号必须单调不减，
//!     倒批显性拒（BatchRegress）；
//!   ⑤ 状态机单调：Idle→Drilling→Closed 单向，Closed 后再演练拒，
//!     倒退显性拒；
//!   ⑥ 全确定性：LCG 种子固定驱动注入轮，零墙钟零随机零浮点，
//!     诊断码 0x40 窗口闭集，中文诊断文案带可操作建议。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x75940，批4 I 组独占窗口）。
pub const SEG_BASE: u32 = 0x75940;

/// 好设备语料数（冻结 12）。
pub const GOOD_CORPUS: usize = 12;
/// BAR 槽索引上限（冻结 6：类型 00h 六槽基数）。
pub const BAR_SLOT_LIMIT: usize = 6;
/// 每样本 BAR 槽位表容量（冻结 6）。
pub const BAR_TABLE_CAP: usize = 6;
/// 判据轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
/// 诊断码闭集：每码带可操作中文建议（detail 文案）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadCode {
    /// 身份烂：vendor 0xFFFF 却带身份行
    IdRotten,
    /// 头烂：头类型 >2 非法
    HdrRotten,
    /// BAR 错位：槽索引越位/重复/乱序
    BarMisalign,
    /// 好设备误伤：三闸错杀好样本
    GoodMarkedBad,
    /// 批次倒退：演练账批次号回退
    BatchRegress,
    /// 抹账拒：演练账只追加
    EraseDenied,
    /// 空样本拒：无身份无槽位的空壳样本
    EmptySample,
    /// 状态机倒退：Idle→Drilling→Closed 之外方向
    StateRegress,
    /// 演练未关账即对拍：Closed 前置缺失
    AuditBeforeClosed,
    /// 双跑不一致：复现红线
    NotReproducible,
}

impl BadCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            BadCode::IdRotten => {
                "身份烂：vendor=0xFFFF 却带身份行，缺席口径与身份面矛盾——请删身份行或改真实 vendor"
            }
            BadCode::HdrRotten => {
                "头烂：头类型 >2（非法型），请按 00/01/02 闭集改头类型或下架样本"
            }
            BadCode::BarMisalign => {
                "BAR 错位：槽索引越位（≥6）/重复/乱序，请按升序不重复口径重建槽位表"
            }
            BadCode::GoodMarkedBad => {
                "好设备误伤：三闸错杀好样本，请回查闸门边界（vendor/头类型/槽索引）"
            }
            BadCode::BatchRegress => {
                "批次倒退拒：演练账批次号必须单调，请用新批次号追加而非回写旧批"
            }
            BadCode::EraseDenied => {
                "抹账拒：演练账只追加不抹账，请开新账册而非删旧行"
            }
            BadCode::EmptySample => {
                "空样本拒：样本无身份行也无槽位表，无演练语义，请补样本字段"
            }
            BadCode::StateRegress => {
                "状态机倒退拒：Idle→Drilling→Closed 单向，请按序推进"
            }
            BadCode::AuditBeforeClosed => {
                "演练未关账即对拍：请先 close() 关账再审计对拍"
            }
            BadCode::NotReproducible => {
                "双跑不一致：复现红线，请检查是否有墙钟/随机参与取值"
            }
        }
    }
    pub const ALL: [BadCode; 10] = [
        BadCode::IdRotten,
        BadCode::HdrRotten,
        BadCode::BarMisalign,
        BadCode::GoodMarkedBad,
        BadCode::BatchRegress,
        BadCode::EraseDenied,
        BadCode::EmptySample,
        BadCode::StateRegress,
        BadCode::AuditBeforeClosed,
        BadCode::NotReproducible,
    ];
}

/// 段内查表闭集返回。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < BadCode::ALL.len() {
        if let Some(c) = BadCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x75940 段闭集内"
}

// ── 域模型：样本与烂类闭集 ──────────────────────────────────────
/// 三类烂设备闭集（演练账的类型列）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BadClass {
    /// 身份烂：vendor 0xFFFF 带身份行。
    IdRotten,
    /// 头烂：头类型 >2。
    HdrRotten,
    /// BAR 错位：槽索引越位。
    BarMisalign,
}

impl BadClass {
    pub fn name(self) -> &'static str {
        match self {
            BadClass::IdRotten => "身份烂",
            BadClass::HdrRotten => "头烂",
            BadClass::BarMisalign => "BAR错位",
        }
    }
    pub const ALL: [BadClass; 3] = [
        BadClass::IdRotten,
        BadClass::HdrRotten,
        BadClass::BarMisalign,
    ];
}

/// 设备样本（好样本与三类烂样本统一模型）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DevSample {
    /// 厂商 ID（0xFFFF 为缺席口径）。
    pub vendor: u16,
    /// 设备 ID。
    pub device_id: u16,
    /// 修订号（身份行存在的承载位：device_id != 0 即视为带身份行）。
    pub revision: u8,
    /// 头类型（闭集 0/1/2，>2 即头烂）。
    pub header_type: u8,
    /// BAR 槽索引表（升序不重复，值 <6）。
    pub bar_slots: [u8; BAR_TABLE_CAP],
    /// 槽位表实际长度（0 = 无槽位表）。
    pub bar_n: usize,
}

impl DevSample {
    /// 是否带身份行（device_id 非 0 即视为身份行在册）。
    pub fn has_identity_row(&self) -> bool {
        self.device_id != 0
    }
}

/// 演练判定结果（好放行 / 烂类定位）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// 好样本：三闸全放行。
    Good,
    /// 烂样本：定位到类。
    Bad(BadClass),
}

/// 三闸逐闸判定（闸序：身份闸→头闸→槽闸，先破先报）。
pub fn gate(sample: &DevSample) -> Verdict {
    // 空样本：无身份行且无槽位表，无演练语义。
    if !sample.has_identity_row() && sample.bar_n == 0 {
        return Verdict::Bad(BadClass::IdRotten);
    }
    // 闸 1：身份闸——vendor 0xFFFF 却带身份行。
    if sample.vendor == 0xFFFF && sample.has_identity_row() {
        return Verdict::Bad(BadClass::IdRotten);
    }
    // 闸 2：头闸——头类型 >2。
    if sample.header_type > 2 {
        return Verdict::Bad(BadClass::HdrRotten);
    }
    // 闸 3：槽闸——越位/重复/乱序。
    let mut k = 0usize;
    let mut prev: i32 = -1;
    while k < sample.bar_n && k < BAR_TABLE_CAP {
        if let Some(slot) = sample.bar_slots.get(k) {
            let s = *slot as i32;
            if s >= BAR_SLOT_LIMIT as i32 {
                return Verdict::Bad(BadClass::BarMisalign);
            }
            if s <= prev {
                return Verdict::Bad(BadClass::BarMisalign);
            }
            prev = s;
        }
        k += 1;
    }
    // bar_n 超容越位同样判错位。
    if sample.bar_n > BAR_TABLE_CAP {
        return Verdict::Bad(BadClass::BarMisalign);
    }
    Verdict::Good
}

// ── 12 好设备语料（冻结：批3 枚举面同源，三闸边界全过） ─────────
pub fn good_corpus() -> Vec<DevSample> {
    let rows: [(u16, u16, u8, u8, [u8; 6], usize); 12] = [
        (0x8086, 0x1237, 0x02, 0, [0, 1, 2, 0, 0, 0], 3),
        (0x8086, 0x7010, 0x00, 0, [0, 5, 0, 0, 0, 0], 2),
        (0x1013, 0x00B8, 0x47, 0, [0, 2, 4, 0, 0, 0], 3),
        (0x10EC, 0x8029, 0x20, 0, [0, 0, 0, 0, 0, 0], 1),
        (0x8086, 0x7113, 0x03, 0, [0, 3, 0, 0, 0, 0], 2),
        (0x1022, 0x2000, 0x10, 1, [0, 1, 2, 3, 0, 0], 4),
        (0x1022, 0x2001, 0x10, 1, [0, 0, 0, 0, 0, 0], 1),
        (0x15AD, 0x0740, 0x16, 0, [0, 1, 2, 3, 5, 0], 5),
        (0x15AD, 0x0774, 0x21, 0, [0, 4, 0, 0, 0, 0], 2),
        (0x15AD, 0x07A0, 0x22, 2, [0, 2, 0, 0, 0, 0], 2),
        (0x15AD, 0x07C0, 0x23, 2, [0, 0, 0, 0, 0, 0], 1),
        (0x1234, 0x1111, 0x01, 0, [0, 1, 2, 3, 4, 5], 6),
    ];
    let mut v: Vec<DevSample> = Vec::new();
    let mut i = 0usize;
    while i < rows.len() {
        if let Some(r) = rows.get(i) {
            let mut slots = [0u8; BAR_TABLE_CAP];
            let mut k = 0usize;
            while k < r.5 && k < BAR_TABLE_CAP {
                if let (Some(dst), Some(src)) = (slots.get_mut(k), r.4.get(k)) {
                    *dst = *src;
                }
                k += 1;
            }
            v.push(DevSample {
                vendor: r.0,
                device_id: r.1,
                revision: r.2,
                header_type: r.3,
                bar_slots: slots,
                bar_n: r.5,
            });
        }
        i += 1;
    }
    v
}

/// 三类烂样本（闭集 3 个，每类恰破一道闸）。
pub fn bad_corpus() -> Vec<(BadClass, DevSample)> {
    // 身份烂：vendor 0xFFFF 带身份行。
    let id_rot = DevSample {
        vendor: 0xFFFF,
        device_id: 0xBEEF,
        revision: 1,
        header_type: 0,
        bar_slots: [0, 1, 0, 0, 0, 0],
        bar_n: 2,
    };
    // 头烂：头类型 3。
    let hdr_rot = DevSample {
        vendor: 0x8086,
        device_id: 0xCAFE,
        revision: 2,
        header_type: 3,
        bar_slots: [0, 0, 0, 0, 0, 0],
        bar_n: 1,
    };
    // BAR 错位：槽索引 6 越位。
    let bar_rot = DevSample {
        vendor: 0x10EC,
        device_id: 0xF00D,
        revision: 3,
        header_type: 0,
        bar_slots: [0, 2, 6, 0, 0, 0],
        bar_n: 3,
    };
    vec![
        (BadClass::IdRotten, id_rot),
        (BadClass::HdrRotten, hdr_rot),
        (BadClass::BarMisalign, bar_rot),
    ]
}

// ── 演练账（只追加） ────────────────────────────────────────────
/// 演练状态机（Idle→Drilling→Closed 单调）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrillState {
    /// 空闲：账空。
    Idle,
    /// 演练中：追加中。
    Drilling,
    /// 已关账：只读对拍。
    Closed,
}

/// 单条演练账行（批次序 + 烂类 + 命中位）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DrillRow {
    /// 批次序（单调不减）。
    pub batch: u32,
    /// 烂类（0=身份烂 1=头烂 2=BAR错位）。
    pub class: u8,
    /// 命中位（true=检出）。
    pub hit: bool,
}

/// 演练账总成。
#[derive(Clone, Debug)]
pub struct DrillLedger {
    pub rows: Vec<DrillRow>,
    pub state: DrillState,
    pub last_batch: u32,
    /// 抹账拦截计数（生产路径恒 0）。
    pub erase_blocked: u32,
}

impl Default for DrillLedger {
    fn default() -> DrillLedger {
        DrillLedger {
            rows: Vec::new(),
            state: DrillState::Idle,
            last_batch: 0,
            erase_blocked: 0,
        }
    }
}

impl DrillLedger {
    pub fn new() -> DrillLedger {
        DrillLedger::default()
    }

    /// 开始演练（Idle→Drilling）。
    pub fn begin(&mut self) -> Result<(), u32> {
        if self.state == DrillState::Closed {
            return Err(BadCode::StateRegress.val());
        }
        if self.state == DrillState::Drilling {
            return Err(BadCode::StateRegress.val());
        }
        self.state = DrillState::Drilling;
        Ok(())
    }

    /// 追加账行（批次单调闸 + 只追加闸）。
    pub fn append(&mut self, batch: u32, class: u8, hit: bool) -> Result<usize, u32> {
        if self.state == DrillState::Closed {
            return Err(BadCode::StateRegress.val());
        }
        if class > 2 {
            return Err(BadCode::EmptySample.val());
        }
        if batch < self.last_batch {
            return Err(BadCode::BatchRegress.val());
        }
        self.rows.push(DrillRow { batch, class, hit });
        self.last_batch = batch;
        Ok(self.rows.len().saturating_sub(1))
    }

    /// 抹账显性拒（只追加红线；拦截计数入账显性）。
    pub fn erase(&mut self, _idx: usize) -> Result<(), u32> {
        self.erase_blocked = self.erase_blocked.saturating_add(1);
        Err(BadCode::EraseDenied.val())
    }

    /// 关账（Drilling→Closed）。
    pub fn close(&mut self) -> Result<(), u32> {
        if self.state != DrillState::Drilling {
            return Err(BadCode::AuditBeforeClosed.val());
        }
        self.state = DrillState::Closed;
        Ok(())
    }

    /// 关账后对拍：演练账行数 = 追加次数，且无未检出烂样本混入。
    pub fn audit(&self) -> Result<(usize, usize, usize), u32> {
        if self.state != DrillState::Closed {
            return Err(BadCode::AuditBeforeClosed.val());
        }
        let mut hits = 0usize;
        let mut per_class = [0usize; 3];
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                if r.hit {
                    hits += 1;
                }
                if let Some(c) = per_class.get_mut(r.class as usize) {
                    *c = c.saturating_add(1);
                }
            }
            i += 1;
        }
        Ok((self.rows.len(), hits, per_class[0] + per_class[1] + per_class[2]))
    }
}

// ── FNV-1a 64（演练账封口指纹，双跑一致口径） ───────────────────
pub const FNV_OFFSET: u64 = 0xCBF29CE484222325;
pub const FNV_PRIME: u64 = 0x00000100000001B3;

pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = FNV_OFFSET;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= *b as u64;
            h = h.wrapping_mul(FNV_PRIME);
        }
        i += 1;
    }
    h
}

/// 演练账指纹（行序 + 类 + 命中位逐行拼流）。
pub fn ledger_fp(l: &DrillLedger) -> u64 {
    let mut buf: [u8; 8] = [0u8; 8];
    let mut h: u64 = FNV_OFFSET;
    let mut i = 0usize;
    while i < l.rows.len() {
        if let Some(r) = l.rows.get(i) {
            buf[0] = (r.batch & 0xFF) as u8;
            buf[1] = ((r.batch >> 8) & 0xFF) as u8;
            buf[2] = r.class;
            buf[3] = r.hit as u8;
            h = fnv1a64_step(h, &buf[..4]);
        }
        i += 1;
    }
    h
}

fn fnv1a64_step(h: u64, bytes: &[u8]) -> u64 {
    let mut acc = h;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            acc ^= *b as u64;
            acc = acc.wrapping_mul(FNV_PRIME);
        }
        i += 1;
    }
    acc
}

// ── LCG ────────────────────────────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

// ── 演练全链：好语料 0 误伤 + 三烂 3/3 检出 + 账只追加 ───────────
pub fn drill_run() -> Result<(usize, usize, usize, u64), u32> {
    let mut led = DrillLedger::new();
    led.begin()?;
    let mut caught = 0usize;
    let mut good_run = 0usize;
    // 好语料：逐个过闸，必须全 Good（误伤即记账留痕）。
    let goods = good_corpus();
    let mut i = 0usize;
    while i < goods.len() {
        if let Some(s) = goods.get(i) {
            match gate(s) {
                Verdict::Good => {
                    good_run += 1;
                }
                Verdict::Bad(_) => {
                    // 误伤样本也入账（命中位 false：非检出，是错杀留痕）。
                    led.append(1, 0, false)?;
                }
            }
        }
        i += 1;
    }
    // 烂语料：逐个过闸，必须按类检出并入账。
    let bads = bad_corpus();
    let mut k = 0usize;
    while k < bads.len() {
        if let Some((cls, s)) = bads.get(k) {
            let hit = match gate(s) {
                Verdict::Bad(c) if c == *cls => true,
                Verdict::Bad(_) => false,
                Verdict::Good => false,
            };
            led.append(2, k as u8, hit)?;
            if hit {
                caught += 1;
            }
        }
        k += 1;
    }
    led.close()?;
    let (rows, _hits, _sum) = led.audit()?;
    Ok((good_run, caught, rows, ledger_fp(&led)))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：好 12/12 放行 + 烂 3/3 检出 + 账 3 行。
pub fn probe_seq_a() -> Result<(), u32> {
    match drill_run() {
        Ok((good_run, caught, rows, _)) if good_run == GOOD_CORPUS && caught == 3 && rows == 3 => {
            Ok(())
        }
        _ => Err(BadCode::NotReproducible.val()),
    }
}

/// 序 B：抹账拒 + 倒批拒 + 空样本拒 + 关账前对拍拒。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut led = DrillLedger::new();
    led.begin()?;
    led.append(5, 0, true)?;
    if led.append(4, 0, true) != Err(BadCode::BatchRegress.val()) {
        return Err(BadCode::NotReproducible.val());
    }
    if led.erase(0).is_ok() {
        return Err(BadCode::NotReproducible.val());
    }
    if led.erase_blocked != 1 {
        return Err(BadCode::NotReproducible.val());
    }
    led.close()?;
    if led.audit().is_err() {
        return Err(BadCode::NotReproducible.val());
    }
    // 关账后再追加拒。
    if led.append(9, 0, true).is_ok() {
        return Err(BadCode::NotReproducible.val());
    }
    // 空样本拒。
    let mut l2 = DrillLedger::new();
    l2.begin()?;
    if l2.append(1, 9, true) != Err(BadCode::EmptySample.val()) {
        return Err(BadCode::NotReproducible.val());
    }
    Ok(())
}

/// 序 C：冻结常量与段位守卫。
pub fn probe_seq_c() -> Result<(), u32> {
    let zone_ok = SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x75000 && SEG_BASE <= 0x781C0;
    match (GOOD_CORPUS == 12, BAR_SLOT_LIMIT == 6, zone_ok, segment_guard()) {
        (true, true, true, true) => Ok(()),
        _ => Err(BadCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 3 类 + 误伤红线轮） ────────────────
pub fn inject_rounds(rounds: u32) -> Result<(u32, u32), u32> {
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut tries = 0u32;
    let mut caught = 0u32;
    let bads = bad_corpus();
    let mut r = 0;
    while r < rounds {
        let pick = rng.below(3) as usize;
        let mut k = 0usize;
        while k < bads.len() {
            if let Some((cls, s)) = bads.get(k) {
                // 扰动：按轮次与类序改 revision（不碰三类破口，检出语义不变）。
                let mut s2 = *s;
                s2.revision = s2.revision.wrapping_add(r as u8).wrapping_add(pick as u8);
                let hit = match gate(&s2) {
                    Verdict::Bad(c) if c == *cls => true,
                    _ => false,
                };
                tries += 1;
                if hit {
                    caught += 1;
                }
            }
            k += 1;
        }
        r += 1;
    }
    Ok((tries, caught))
}

/// 好语料误伤红线轮：好样本扰动（revision 变化）不得误伤。
pub fn false_positive_rounds(rounds: u32) -> Result<u32, u32> {
    let mut rng = Lcg::new(SEG_BASE as u64 + 7);
    let mut false_hits = 0u32;
    let goods = good_corpus();
    let mut r = 0;
    while r < rounds {
        let pick = rng.below(goods.len() as u64) as usize;
        if let Some(s) = goods.get(pick) {
            let mut s2 = *s;
            s2.revision = s2.revision.wrapping_add(r as u8);
            if let Verdict::Bad(_) = gate(&s2) {
                false_hits += 1;
            }
        }
        r += 1;
    }
    Ok(false_hits)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = drill_run()?;
    let b = drill_run()?;
    if a != b {
        return Err(BadCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫 ────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < BadCode::ALL.len() {
        if let Some(c) = BadCode::ALL.get(i) {
            let v = c.val();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ── 判据面（私有谓词，cond 为违约条件） ─────────────────────────
fn three_bad_caught_ok() -> bool {
    let bads = bad_corpus();
    let mut n = 0usize;
    let mut i = 0usize;
    while i < bads.len() {
        if let Some((cls, s)) = bads.get(i) {
            let hit = match gate(s) {
                Verdict::Bad(c) if c == *cls => true,
                _ => false,
            };
            if !hit {
                return false;
            }
            n += 1;
        }
        i += 1;
    }
    n == 3
}

fn good_zero_false_ok() -> bool {
    let goods = good_corpus();
    if goods.len() != GOOD_CORPUS {
        return false;
    }
    let mut i = 0usize;
    while i < goods.len() {
        if let Some(s) = goods.get(i) {
            if let Verdict::Bad(_) = gate(s) {
                return false;
            }
        }
        i += 1;
    }
    true
}

fn ledger_append_only_ok() -> bool {
    let mut led = DrillLedger::new();
    if led.begin().is_err() {
        return false;
    }
    if led.append(1, 0, true).is_err() {
        return false;
    }
    if led.append(2, 1, true).is_err() {
        return false;
    }
    if led.append(2, 2, true).is_err() {
        return false;
    }
    if led.append(1, 0, true).is_ok() {
        return false;
    }
    if led.erase(0).is_ok() {
        return false;
    }
    if led.close().is_err() {
        return false;
    }
    match led.audit() {
        Ok((rows, hits, sum)) => rows == 3 && hits == 3 && sum == 3,
        Err(_) => false,
    }
}

fn inject_all_caught_ok() -> bool {
    match inject_rounds(JUDGE_ROUNDS) {
        Ok((t, c)) => t == JUDGE_ROUNDS * 3 && c == t,
        Err(_) => false,
    }
}

fn false_positive_zero_ok() -> bool {
    match false_positive_rounds(JUDGE_ROUNDS) {
        Ok(n) => n == 0,
        Err(_) => false,
    }
}

fn boundary_ok() -> bool {
    // 头类型闭集上沿 2 好、3 烂；槽索引上沿 5 好、6 烂。
    let goods = good_corpus();
    let mut hdr2 = match goods.get(9) {
        Some(s) => *s,
        None => return false,
    };
    hdr2.header_type = 2;
    if !matches!(gate(&hdr2), Verdict::Good) {
        return false;
    }
    hdr2.header_type = 3;
    if !matches!(gate(&hdr2), Verdict::Bad(BadClass::HdrRotten)) {
        return false;
    }
    let mut slot5 = match goods.get(11) {
        Some(s) => *s,
        None => return false,
    };
    if !matches!(gate(&slot5), Verdict::Good) {
        return false;
    }
    if let Some(s) = slot5.bar_slots.get_mut(5) {
        *s = 6;
    }
    matches!(gate(&slot5), Verdict::Bad(BadClass::BarMisalign))
}

fn empty_sample_ok() -> bool {
    // 空壳样本（无身份行 + 无槽位表）无演练语义，显性按身份烂口径拒。
    let empty = DevSample {
        vendor: 0xFFFF,
        device_id: 0,
        revision: 0,
        header_type: 0,
        bar_slots: [0; BAR_TABLE_CAP],
        bar_n: 0,
    };
    matches!(gate(&empty), Verdict::Bad(BadClass::IdRotten))
}

fn fp_repro_ok() -> bool {
    // 演练账指纹双跑一致（封口口径可复算）。
    match drill_run() {
        Ok((_, _, _, fp1)) => match drill_run() {
            Ok((_, _, _, fp2)) => fp1 == fp2 && fp1 != 0,
            Err(_) => false,
        },
        Err(_) => false,
    }
}

fn detail_closed_ok() -> bool {
    let mut i = 0;
    while i < BadCode::ALL.len() {
        if let Some(c) = BadCode::ALL.get(i) {
            if detail(c.val()).is_empty() {
                return false;
            }
        }
        i += 1;
    }
    detail(SEG_BASE + 0x3F).contains("未知")
}

// ── 检查节 ──────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    v.push((
        "J1-三类烂检出3/3",
        three_bad_caught_ok(),
        String::from("身份烂/头烂/BAR错位 逐类定位"),
    ));
    v.push((
        "J1-好语料误伤0",
        good_zero_false_ok(),
        format!("12 好设备三闸全放行 0 误伤"),
    ));
    v.push((
        "J1-演练全链",
        match drill_run() {
            Ok((g, c, r, _)) => g == GOOD_CORPUS && c == 3 && r == 3,
            Err(_) => false,
        },
        String::from("好12/12+烂3/3+账3行"),
    ));
    v.push((
        "J2-账只追加",
        ledger_append_only_ok(),
        String::from("倒批拒+抹账拒+关账对拍"),
    ));
    let inj = inject_rounds(JUDGE_ROUNDS);
    v.push((
        "J2-注入10/10",
        inject_all_caught_ok(),
        format!(
            "三类 t/c {}/{}",
            inj.as_ref().map(|(t, _)| *t).unwrap_or(0),
            inj.as_ref().map(|(_, c)| *c).unwrap_or(0)
        ),
    ));
    v.push((
        "J2-误伤红线轮0",
        false_positive_zero_ok(),
        String::from("好样本扰动 10 轮 0 误伤"),
    ));
    v.push(("J3-双跑一致", repro_check().is_ok(), String::from("drill_run 两跑同值")));
    v.push((
        "J1-边界样本",
        boundary_ok(),
        String::from("头类型2/3 与 槽索引5/6 上下沿"),
    ));
    v.push((
        "J1-空样本拒",
        empty_sample_ok(),
        String::from("无身份无槽位空壳样本显性拒"),
    ));
    v.push((
        "J2-演练账指纹",
        fp_repro_ok(),
        String::from("账行序+类+命中位 FNV 封口一致"),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x75940 窗口")));
    v.push(("J3-诊断码闭集", detail_closed_ok(), String::from("查表闭集返回")));
    v
}

pub fn run_unx0478_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0478 坏配置注入演练");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 测试区 ──────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_bad_classes_detected() {
        let bads = bad_corpus();
        assert_eq!(bads.len(), 3);
        let mut i = 0usize;
        while i < bads.len() {
            let (cls, s) = &bads[i];
            match gate(s) {
                Verdict::Bad(c) => assert_eq!(c, *cls, "第 {} 类须按类检出", i),
                Verdict::Good => panic!("烂样本不得放行"),
            }
            i += 1;
        }
    }

    #[test]
    fn good_corpus_zero_false_positive() {
        let goods = good_corpus();
        assert_eq!(goods.len(), GOOD_CORPUS);
        let mut i = 0usize;
        while i < goods.len() {
            assert!(
                matches!(gate(&goods[i]), Verdict::Good),
                "好样本 {} 不得误伤",
                i
            );
            i += 1;
        }
    }

    #[test]
    fn boundary_samples_hold() {
        // 头类型 2 是好（闭集上沿）；3 是烂。
        let mut s = good_corpus()[9];
        s.header_type = 2;
        assert!(matches!(gate(&s), Verdict::Good));
        s.header_type = 3;
        assert!(matches!(gate(&s), Verdict::Bad(BadClass::HdrRotten)));
        // 槽索引 5 是好（上沿）；6 是烂。
        let mut t = good_corpus()[11];
        assert_eq!(t.bar_slots[5], 5);
        t.bar_slots[5] = 6;
        assert!(matches!(gate(&t), Verdict::Bad(BadClass::BarMisalign)));
        // vendor 0xFFFF 无身份行（device_id=0）不判身份烂。
        let mut u = good_corpus()[0];
        u.vendor = 0xFFFF;
        u.device_id = 0;
        assert!(matches!(gate(&u), Verdict::Good));
        u.device_id = 0x1234;
        assert!(matches!(gate(&u), Verdict::Bad(BadClass::IdRotten)));
    }

    #[test]
    fn ledger_batch_monotonic() {
        let mut led = DrillLedger::new();
        led.begin().expect("begin");
        led.append(3, 0, true).expect("追加");
        led.append(3, 1, true).expect("同批追加");
        assert_eq!(led.append(2, 0, true), Err(BadCode::BatchRegress.val()));
        assert_eq!(led.rows.len(), 2, "倒批行不得入账");
    }

    #[test]
    fn ledger_erase_denied() {
        let mut led = DrillLedger::new();
        led.begin().expect("begin");
        led.append(1, 0, true).expect("追加");
        assert_eq!(led.erase(0), Err(BadCode::EraseDenied.val()));
        assert_eq!(led.erase_blocked, 1);
        assert_eq!(led.rows.len(), 1, "抹账不得生效");
    }

    #[test]
    fn state_machine_monotonic() {
        let mut led = DrillLedger::new();
        assert_eq!(led.close(), Err(BadCode::AuditBeforeClosed.val()));
        led.begin().expect("begin");
        led.close().expect("close");
        assert_eq!(led.begin(), Err(BadCode::StateRegress.val()));
        assert_eq!(led.append(1, 0, true), Err(BadCode::StateRegress.val()));
        assert!(led.audit().is_ok());
    }

    #[test]
    fn drill_run_full_chain() {
        let (g, c, r, _) = drill_run().expect("演练全链");
        assert_eq!(g, 12);
        assert_eq!(c, 3);
        assert_eq!(r, 3);
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
    }

    #[test]
    fn injection_and_fp_rounds() {
        let (t, c) = inject_rounds(JUDGE_ROUNDS).expect("注入轮");
        assert_eq!(t, JUDGE_ROUNDS * 3);
        assert_eq!(c, t);
        assert_eq!(false_positive_rounds(JUDGE_ROUNDS).expect("误伤轮"), 0);
    }

    #[test]
    fn dual_run_and_segment() {
        assert!(repro_check().is_ok());
        assert!(segment_guard());
        assert!(detail_closed_ok());
    }
}
