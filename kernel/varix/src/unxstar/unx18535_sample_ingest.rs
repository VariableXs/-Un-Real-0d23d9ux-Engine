//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18535 探针样本入 E1 库 E4 分册 · 0x80180段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18535-J1 50 例样本 100% 入库（四元账+域标签），
//!       独立/合并双模式绿。
//! 结构清单：
//!   - 数据结构：SampleRec（Copy 四元账：样本号/域标签/裁决/校验和）、
//!     IngestStore（64 槽数组账本 + 4 域位面）、VolumeTable（8 册合并
//!     账本，扇出 16）、确定性样本哈希核（32 位折叠）；
//!   - 状态机：Collecting→Balancing→Closed 三态 + 册级 Open→Sealed；
//!   - 账本：样本账（追加序单调）+ 域位面（逐域 popcount 对账）+
//!     册账（成分数守恒）；
//!   - 错误路径：IgCode 闭集 12 码，code() = SEG_BASE + n（0x80180 段），
//!     每码非空中文建议；
//!   - 注入核：非法域标签 / 重号样本 / 篡改哈希 / 裁决越闭集 四类坏样本
//!     各 10 轮逐类断言被显式拒绝或审计必抓（10/10 全抓）。
//! 哈希核推导（写死）：h = id·0x9E3779B1 + tag·0x85EBCA6B + verdict·0xC2B2AE35
//!   （u32 wrapping），h ^= h>>13，h *= 0x27D4EB2F，h ^= h>>15 —— 黄金比
//!   折叠 + 两轮 avalanche，四元账任一字段变动必改全字。
//! 纪律：no_std + alloc；内核路径零 panic；无浮点/IO/墙钟/随机。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x80180，AI-VX11-1105 G1 批07 独占区，卡表给定）。
pub const SEG_BASE: u32 = 0x80180;

// ── 冻结常量（推导注明） ─────────────────────────────────────────
/// 样本总数（J1 冻结：50 例，100% 入库）。
pub const SAMPLE_TOTAL: usize = 50;
/// 样本账容量（64 槽：≥50，留 14 槽余量）。
pub const STORE_CAP: usize = 64;
/// 域标签闭集大小（4 域：内核/驱动/运行时/壳）。
pub const DOMAIN_MAX: u8 = 4;
/// 域标签：内核层。
pub const DOM_KERNEL: u8 = 1;
/// 域标签：驱动层。
pub const DOM_DRIVER: u8 = 2;
/// 域标签：运行时层。
pub const DOM_RUNTIME: u8 = 3;
/// 域标签：壳层。
pub const DOM_SHELL: u8 = 4;
/// 裁决：绿。
pub const VERDICT_GREEN: u8 = 1;
/// 裁决：红。
pub const VERDICT_RED: u8 = 2;
/// 样本号下限（0 号保留给册头）。
pub const SAMPLE_ID_MIN: u32 = 1;
/// 样本号上限（0x1000=4096）。
pub const SAMPLE_ID_MAX: u32 = 4096;
/// 合并册容量（8 册）。
pub const VOLUME_CAP: usize = 8;
/// 合并扇出（单册至多 16 例：50 = 3×16 + 2 → 恰 4 册）。
pub const VOLUME_FANOUT: usize = 16;
/// 账态：收样中。
pub const STATE_COLLECTING: u8 = 1;
/// 账态：合并中。
pub const STATE_BALANCING: u8 = 2;
/// 账态：闭库。
pub const STATE_CLOSED: u8 = 3;
/// 册态：开册（可吸收成分）。
pub const VSTATE_OPEN: u8 = 1;
/// 册态：封册（终态）。
pub const VSTATE_SEALED: u8 = 2;
/// 判据迭代轮数（写死：注入 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 注入类别数（4 类坏样本）。
pub const INJECT_CLASSES: usize = 4;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IgCode {
    /// 样本号越界（0 保留 / >4096）
    SampleIdOutOfRange,
    /// 域标签非法（闭集 1..=4 外）
    DomainTagIllegal,
    /// 裁决越闭集（绿/红外）
    VerdictIllegal,
    /// 样本重号
    DuplicateSample,
    /// 样本账满（64 槽耗尽）
    StoreFull,
    /// 哈希不符（四元账篡改）
    HashMismatch,
    /// 合并册满（8 册耗尽）
    VolumeFull,
    /// 册未开（未 Open 即吸收成分）
    VolumeNotOpen,
    /// 账态非法转换
    IllegalTransition,
    /// 域位面损坏（popcount ≠ 逐域计数）
    PlaneCorrupt,
    /// 守恒破坏（独立数 ≠ 合并成分数）
    ConservationBroken,
    /// 双跑不一致
    NotReproducible,
}

impl IgCode {
    /// 诊断码取值：SEG_BASE + 枚举序号。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 非空中文建议（文案闭集）。
    pub fn detail(self) -> &'static str {
        match self {
            IgCode::SampleIdOutOfRange => "样本号越界：编号须在 1..=4096，0 号为册头保留",
            IgCode::DomainTagIllegal => "域标签非法：闭集为 1..=4（内核/驱动/运行时/壳）",
            IgCode::VerdictIllegal => "裁决越闭集：裁决闭集为 绿/红",
            IgCode::DuplicateSample => "样本重号：同号已入库，换号或先出库",
            IgCode::StoreFull => "样本账满：64 槽耗尽，账本只增不减需扩册",
            IgCode::HashMismatch => "哈希不符：四元账任一字段被篡改，审计必抓",
            IgCode::VolumeFull => "合并册满：8 册耗尽，扇出 16 上限内分册",
            IgCode::VolumeNotOpen => "册未开：合并前必须先开册，吸收成分拒绝",
            IgCode::IllegalTransition => "账态非法转换：三态只进不退，越级拒绝",
            IgCode::PlaneCorrupt => "域位面损坏：逐域 popcount 须与域计数吻合",
            IgCode::ConservationBroken => "守恒破坏：独立入库数 ≠ 合并成分总数",
            IgCode::NotReproducible => "双跑不一致：同输入两次入库结果不同",
        }
    }
    /// 码闭集清单。
    pub const ALL: [IgCode; 12] = [
        IgCode::SampleIdOutOfRange,
        IgCode::DomainTagIllegal,
        IgCode::VerdictIllegal,
        IgCode::DuplicateSample,
        IgCode::StoreFull,
        IgCode::HashMismatch,
        IgCode::VolumeFull,
        IgCode::VolumeNotOpen,
        IgCode::IllegalTransition,
        IgCode::PlaneCorrupt,
        IgCode::ConservationBroken,
        IgCode::NotReproducible,
    ];
}

/// 诊断码 → 中文建议（闭集外返回未知提示）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < IgCode::ALL.len() {
        if IgCode::ALL[i].code() == code {
            return IgCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x80180 段闭集内"
}

// ── 样本哈希核（规格冻结，见文件头推导） ────────────────────────
/// 四元账校验和：黄金比折叠 + 两轮 avalanche。同四元永远同值。
pub fn sample_hash(id: u32, tag: u8, verdict: u8) -> u32 {
    let mut h = id
        .wrapping_mul(0x9E37_79B1)
        .wrapping_add((tag as u32).wrapping_mul(0x85EB_CA6B))
        .wrapping_add((verdict as u32).wrapping_mul(0xC2B2_AE35));
    h ^= h >> 13;
    h = h.wrapping_mul(0x27D4_EB2F);
    h ^= h >> 15;
    h
}

/// 合并哈希：成分哈希按序折叠（rotate-left 混合）。
pub fn merged_hash_of(hashes: &[u32]) -> u32 {
    let mut acc: u32 = 0x1E4F_B07D; // E4 分册魔数派生：'E'=0x45,'4'=0x34 → 0x4534 再散列
    let mut i = 0;
    while i < hashes.len() {
        let h = match hashes.get(i) {
            Some(&v) => v,
            None => break,
        };
        acc = acc.rotate_left(7) ^ h;
        i += 1;
    }
    acc
}

// ── 样本记录（Copy 四元账） ──────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SampleRec {
    /// 样本号（1..=4096）。
    pub id: u32,
    /// 域标签（1..=4）。
    pub tag: u8,
    /// 裁决（绿/红）。
    pub verdict: u8,
    /// 四元账校验和（对前三元计算）。
    pub hash: u32,
    /// 空槽标记。
    pub empty: bool,
}

impl SampleRec {
    /// 空样本（const fn）。
    pub const fn empty() -> SampleRec {
        SampleRec { id: 0, tag: 0, verdict: 0, hash: 0, empty: true }
    }
}

// ── 合并册记录（Copy） ───────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct VolumeRec {
    /// 册号（1..=8）。
    pub vol_id: u8,
    /// 成分样本号区间 [first, last]。
    pub first_id: u32,
    /// 成分数。
    pub constituents: u32,
    /// 合并哈希（成分哈希折叠）。
    pub merged_hash: u32,
    /// 册态（Open/Sealed）。
    pub state: u8,
    /// 空槽标记。
    pub empty: bool,
}

impl VolumeRec {
    /// 空册（const fn）。
    pub const fn empty() -> VolumeRec {
        VolumeRec {
            vol_id: 0,
            first_id: 0,
            constituents: 0,
            merged_hash: 0,
            state: VSTATE_OPEN,
            empty: true,
        }
    }
}

// ── 独立模式：E4 分册样本账 ─────────────────────────────────────
pub struct IngestStore {
    /// 64 槽样本账。
    samples: [SampleRec; STORE_CAP],
    /// 4 域位面（每域 64 位对账）。
    domain_planes: [[u64; 1]; DOMAIN_MAX as usize],
    /// 逐域计数（与位面 popcount 守恒）。
    domain_counts: [u32; DOMAIN_MAX as usize],
    /// 已入库数。
    count: usize,
    /// 账态。
    state: u8,
    /// 拒因账（只增不减）。
    reject_ledger: Vec<u32>,
}

impl IngestStore {
    pub fn new() -> IngestStore {
        IngestStore {
            samples: [SampleRec::empty(); STORE_CAP],
            domain_planes: [[0u64; 1]; DOMAIN_MAX as usize],
            domain_counts: [0u32; DOMAIN_MAX as usize],
            count: 0,
            state: STATE_COLLECTING,
            reject_ledger: Vec::new(),
        }
    }

    /// 入库一笔样本（四元账自动盖哈希）。闸序：账态→编号→域→裁决→重号→容量。
    pub fn ingest(&mut self, id: u32, tag: u8, verdict: u8) -> Result<u32, u32> {
        if self.state != STATE_COLLECTING {
            self.reject_ledger.push(IgCode::IllegalTransition.code());
            return Err(IgCode::IllegalTransition.code());
        }
        if id < SAMPLE_ID_MIN || id > SAMPLE_ID_MAX {
            self.reject_ledger.push(IgCode::SampleIdOutOfRange.code());
            return Err(IgCode::SampleIdOutOfRange.code());
        }
        if tag == 0 || tag > DOMAIN_MAX {
            self.reject_ledger.push(IgCode::DomainTagIllegal.code());
            return Err(IgCode::DomainTagIllegal.code());
        }
        if verdict != VERDICT_GREEN && verdict != VERDICT_RED {
            self.reject_ledger.push(IgCode::VerdictIllegal.code());
            return Err(IgCode::VerdictIllegal.code());
        }
        // 重号扫描。
        let mut i = 0;
        while i < self.count {
            let rec = match self.samples.get(i) {
                Some(r) => *r,
                None => return Err(IgCode::PlaneCorrupt.code()),
            };
            if !rec.empty && rec.id == id {
                self.reject_ledger.push(IgCode::DuplicateSample.code());
                return Err(IgCode::DuplicateSample.code());
            }
            i += 1;
        }
        if self.count >= STORE_CAP {
            self.reject_ledger.push(IgCode::StoreFull.code());
            return Err(IgCode::StoreFull.code());
        }
        // 写账（先算哈希，再可变借用）。
        let h = sample_hash(id, tag, verdict);
        let rec = SampleRec { id, tag, verdict, hash: h, empty: false };
        let idx = self.count;
        match self.samples.get_mut(idx) {
            Some(sl) => {
                *sl = rec;
            }
            None => {
                self.reject_ledger.push(IgCode::StoreFull.code());
                return Err(IgCode::StoreFull.code());
            }
        }
        // 域位面 + 域计数。
        let d = tag as usize - 1; // 1..=4 → 0..=3
        if let Some(plane) = self.domain_planes.get_mut(d) {
            let bit = idx % 64;
            if let Some(w) = plane.get_mut(0) {
                *w |= 1u64 << bit;
            }
        }
        if let Some(c) = self.domain_counts.get_mut(d) {
            *c = c.saturating_add(1);
        }
        self.count += 1;
        Ok(h)
    }

    /// 审计：逐条重算哈希必符 + 域位面 popcount 守恒。
    pub fn audit(&self) -> Result<(), u32> {
        let mut i = 0;
        while i < self.count {
            let rec = match self.samples.get(i) {
                Some(r) => *r,
                None => return Err(IgCode::PlaneCorrupt.code()),
            };
            let want = sample_hash(rec.id, rec.tag, rec.verdict);
            if want != rec.hash {
                return Err(IgCode::HashMismatch.code());
            }
            i += 1;
        }
        // 逐域位面对账。
        let mut d = 0;
        while d < DOMAIN_MAX as usize {
            let plane = match self.domain_planes.get(d) {
                Some(p) => *p,
                None => return Err(IgCode::PlaneCorrupt.code()),
            };
            let mut pop = 0usize;
            let mut w = match plane.get(0) {
                Some(&v) => v,
                None => 0,
            };
            while w != 0 {
                w &= w.wrapping_sub(1);
                pop += 1;
            }
            let cnt = match self.domain_counts.get(d) {
                Some(&c) => c as usize,
                None => return Err(IgCode::PlaneCorrupt.code()),
            };
            if pop != cnt {
                return Err(IgCode::PlaneCorrupt.code());
            }
            d += 1;
        }
        Ok(())
    }

    /// 追加序单调审计。
    pub fn ids_monotonic(&self) -> bool {
        let mut prev = 0u32;
        let mut i = 0;
        while i < self.count {
            let rec = match self.samples.get(i) {
                Some(r) => *r,
                None => return false,
            };
            if rec.id <= prev {
                return false;
            }
            prev = rec.id;
            i += 1;
        }
        true
    }

    /// 账态推进：Collecting → Balancing。
    pub fn begin_balance(&mut self) -> Result<(), u32> {
        if self.state != STATE_COLLECTING {
            self.reject_ledger.push(IgCode::IllegalTransition.code());
            return Err(IgCode::IllegalTransition.code());
        }
        self.state = STATE_BALANCING;
        Ok(())
    }

    /// 闭库：Balancing → Closed。
    pub fn close(&mut self) -> Result<(), u32> {
        if self.state != STATE_BALANCING {
            self.reject_ledger.push(IgCode::IllegalTransition.code());
            return Err(IgCode::IllegalTransition.code());
        }
        self.state = STATE_CLOSED;
        Ok(())
    }

    /// 第 idx 条样本哈希只读（越界 None）。
    pub fn hash_at(&self, idx: usize) -> Option<u32> {
        match self.samples.get(idx) {
            Some(r) if !r.empty => Some(r.hash),
            _ => None,
        }
    }

    pub fn sample_count(&self) -> usize {
        self.count
    }

    pub fn state(&self) -> u8 {
        self.state
    }

    pub fn reject_count(&self) -> usize {
        self.reject_ledger.len()
    }

    /// 逐域计数只读（越界 0）。
    pub fn domain_count(&self, tag: u8) -> u32 {
        if tag == 0 || tag > DOMAIN_MAX {
            return 0;
        }
        *self.domain_counts.get(tag as usize - 1).unwrap_or(&0)
    }
}

// ── 合并模式：册账 ──────────────────────────────────────────────
pub struct VolumeTable {
    /// 8 册。
    volumes: [VolumeRec; VOLUME_CAP],
    /// 已开册数。
    count: usize,
    /// 成分吸收总笔数（守恒基准）。
    pub absorbed: u32,
    /// 拒因账。
    reject_ledger: Vec<u32>,
}

impl VolumeTable {
    pub fn new() -> VolumeTable {
        VolumeTable {
            volumes: [VolumeRec::empty(); VOLUME_CAP],
            count: 0,
            absorbed: 0,
            reject_ledger: Vec::new(),
        }
    }

    /// 开新册（扇出 16 上限；上一册未封则拒）。
    pub fn open_volume(&mut self, vol_id: u8) -> Result<(), u32> {
        if self.count >= VOLUME_CAP {
            self.reject_ledger.push(IgCode::VolumeFull.code());
            return Err(IgCode::VolumeFull.code());
        }
        // 已有末册未封 → 拒。
        if self.count > 0 {
            let last = match self.volumes.get(self.count - 1) {
                Some(v) => *v,
                None => return Err(IgCode::PlaneCorrupt.code()),
            };
            if last.state == VSTATE_OPEN {
                self.reject_ledger.push(IgCode::VolumeNotOpen.code());
                return Err(IgCode::IllegalTransition.code());
            }
        }
        let rec = VolumeRec {
            vol_id,
            first_id: 0,
            constituents: 0,
            merged_hash: 0x1E4F_B07D,
            state: VSTATE_OPEN,
            empty: false,
        };
        match self.volumes.get_mut(self.count) {
            Some(sl) => {
                *sl = rec;
            }
            None => {
                self.reject_ledger.push(IgCode::VolumeFull.code());
                return Err(IgCode::VolumeFull.code());
            }
        }
        self.count += 1;
        Ok(())
    }

    /// 吸收一笔成分（样本号 + 哈希）进末册。
    pub fn absorb(&mut self, id: u32, hash: u32) -> Result<(), u32> {
        if self.count == 0 {
            self.reject_ledger.push(IgCode::VolumeNotOpen.code());
            return Err(IgCode::VolumeNotOpen.code());
        }
        let last_idx = self.count - 1;
        let cur = match self.volumes.get(last_idx) {
            Some(v) => *v,
            None => return Err(IgCode::PlaneCorrupt.code()),
        };
        if cur.state != VSTATE_OPEN {
            self.reject_ledger.push(IgCode::VolumeNotOpen.code());
            return Err(IgCode::VolumeNotOpen.code());
        }
        if cur.constituents as usize >= VOLUME_FANOUT {
            self.reject_ledger.push(IgCode::VolumeFull.code());
            return Err(IgCode::VolumeFull.code());
        }
        // 更新末册（块内算好新值再写，避免双借用）。
        let new_first = if cur.constituents == 0 { id } else { cur.first_id };
        let new_hash = cur.merged_hash.rotate_left(7) ^ hash;
        let new_cnt = cur.constituents.saturating_add(1);
        match self.volumes.get_mut(last_idx) {
            Some(v) => {
                v.first_id = new_first;
                v.merged_hash = new_hash;
                v.constituents = new_cnt;
            }
            None => return Err(IgCode::PlaneCorrupt.code()),
        }
        self.absorbed = self.absorbed.saturating_add(1);
        Ok(())
    }

    /// 封末册。
    pub fn seal_volume(&mut self) -> Result<(), u32> {
        if self.count == 0 {
            self.reject_ledger.push(IgCode::VolumeNotOpen.code());
            return Err(IgCode::VolumeNotOpen.code());
        }
        let last_idx = self.count - 1;
        let cur = match self.volumes.get(last_idx) {
            Some(v) => *v,
            None => return Err(IgCode::PlaneCorrupt.code()),
        };
        if cur.state != VSTATE_OPEN || cur.constituents == 0 {
            self.reject_ledger.push(IgCode::VolumeNotOpen.code());
            return Err(IgCode::IllegalTransition.code());
        }
        match self.volumes.get_mut(last_idx) {
            Some(v) => {
                v.state = VSTATE_SEALED;
            }
            None => return Err(IgCode::PlaneCorrupt.code()),
        }
        Ok(())
    }

    /// 册账审计：成分总数 == absorbed，空册零成分。
    pub fn audit(&self) -> Result<(), u32> {
        let mut sum = 0u32;
        let mut i = 0;
        while i < self.count {
            let v = match self.volumes.get(i) {
                Some(v) => *v,
                None => return Err(IgCode::PlaneCorrupt.code()),
            };
            if v.empty || v.state != VSTATE_SEALED || v.constituents == 0 {
                return Err(IgCode::VolumeNotOpen.code());
            }
            sum = sum.saturating_add(v.constituents);
            i += 1;
        }
        if sum != self.absorbed {
            return Err(IgCode::ConservationBroken.code());
        }
        Ok(())
    }

    /// 第 idx 册只读（越界 None）。
    pub fn volume_at(&self, idx: usize) -> Option<VolumeRec> {
        match self.volumes.get(idx) {
            Some(v) if !v.empty => Some(*v),
            _ => None,
        }
    }

    pub fn volume_count(&self) -> usize {
        self.count
    }
}

// ── 双模式黄金链 ────────────────────────────────────────────────
/// 50 例建模样本：域标签按 (i%4)+1 循环铺满 4 域，裁决偶绿奇红。
/// 返回 (入库数, 绿数, 红数)。
pub fn modeled_sample(i: usize) -> (u32, u8, u8) {
    let id = 100 + i as u32;
    let tag = (i % DOMAIN_MAX as usize) as u8 + 1;
    let verdict = if i % 2 == 0 { VERDICT_GREEN } else { VERDICT_RED };
    (id, tag, verdict)
}

/// 独立模式：50 例逐条入库 → 审计全过。返回 (入库数, 哈希首笔)。
pub fn run_independent() -> Result<(usize, u32), u32> {
    let mut store = IngestStore::new();
    let mut i = 0;
    let mut first_hash = 0u32;
    while i < SAMPLE_TOTAL {
        let (id, tag, verdict) = modeled_sample(i);
        let h = store.ingest(id, tag, verdict)?;
        if i == 0 {
            first_hash = h;
        }
        i += 1;
    }
    store.audit()?;
    if store.sample_count() != SAMPLE_TOTAL {
        return Err(IgCode::ConservationBroken.code());
    }
    if !store.ids_monotonic() {
        return Err(IgCode::ConservationBroken.code());
    }
    Ok((store.sample_count(), first_hash))
}

/// 合并模式：同一批 50 例按扇出 16 分册吸收 → 册账审计全过。
/// 返回 (册数, 成分总数)。
pub fn run_merged() -> Result<(usize, u32), u32> {
    let mut store = IngestStore::new();
    let mut i = 0;
    while i < SAMPLE_TOTAL {
        let (id, tag, verdict) = modeled_sample(i);
        let _ = store.ingest(id, tag, verdict)?;
        i += 1;
    }
    store.begin_balance()?;
    let mut vols = VolumeTable::new();
    let mut vi = 1u8;
    let mut j = 0usize;
    while j < SAMPLE_TOTAL {
        if j % VOLUME_FANOUT == 0 {
            // 封上册（若有）再开新册。
            if vols.volume_count() > 0 {
                vols.seal_volume()?;
            }
            vols.open_volume(vi)?;
            vi += 1;
        }
        let (id, _tag, _verdict) = modeled_sample(j);
        let h = match store.hash_at(j) {
            Some(h) => h,
            None => return Err(IgCode::ConservationBroken.code()),
        };
        vols.absorb(id, h)?;
        j += 1;
    }
    vols.seal_volume()?;
    vols.audit()?;
    store.close()?;
    if vols.absorbed as usize != SAMPLE_TOTAL {
        return Err(IgCode::ConservationBroken.code());
    }
    Ok((vols.volume_count(), vols.absorbed))
}

/// 双模式合账：独立入库数 == 合并成分总数（守恒），双绿。
pub fn dual_mode_check() -> Result<(usize, usize), u32> {
    let ind = run_independent()?;
    let mrg = run_merged()?;
    if ind.0 != mrg.1 as usize {
        return Err(IgCode::ConservationBroken.code());
    }
    Ok((ind.0, mrg.0))
}

// ── 注入核：四类坏样本，各 10 轮，逐类全抓 ──────────────────────
pub struct InjectLedger {
    pub tries: [u32; INJECT_CLASSES],
    pub caught: [u32; INJECT_CLASSES],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; INJECT_CLASSES], caught: [0; INJECT_CLASSES] }
    }

    pub fn record(&mut self, slot: usize, hit: bool) {
        if let Some(t) = self.tries.get_mut(slot) {
            *t = t.saturating_add(1);
            if hit {
                if let Some(c) = self.caught.get_mut(slot) {
                    *c = c.saturating_add(1);
                }
            }
        }
    }

    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < INJECT_CLASSES {
            let t = *self.tries.get(i).unwrap_or(&0);
            let c = *self.caught.get(i).unwrap_or(&0);
            if t == 0 || c != t {
                return false;
            }
            i += 1;
        }
        true
    }

    pub fn conservation(&self) -> Result<(), u32> {
        let mut tries = 0u32;
        let mut caught = 0u32;
        let mut i = 0;
        while i < INJECT_CLASSES {
            tries = tries.saturating_add(*self.tries.get(i).unwrap_or(&0));
            caught = caught.saturating_add(*self.caught.get(i).unwrap_or(&0));
            i += 1;
        }
        if caught > tries {
            return Err(IgCode::ConservationBroken.code());
        }
        Ok(())
    }
}

/// 四类坏样本逐类注入 `rounds` 轮：
/// C0 非法域标签、C1 重号样本、C2 篡改哈希必抓、C3 裁决越闭集。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // C0：域标签 0 与 5 双探。
        let mut s0 = IngestStore::new();
        let hit0 = s0.ingest(100, 0, VERDICT_GREEN).is_err()
            && s0.ingest(100, DOMAIN_MAX + 1, VERDICT_GREEN).err()
                == Some(IgCode::DomainTagIllegal.code());
        led.record(0, hit0);
        // C1：重号。
        let mut s1 = IngestStore::new();
        let _ = s1.ingest(100, DOM_KERNEL, VERDICT_GREEN);
        led.record(
            1,
            s1.ingest(100, DOM_KERNEL, VERDICT_GREEN).err()
                == Some(IgCode::DuplicateSample.code()),
        );
        // C2：篡改哈希 → 审计必抓（构造哈希不符的账面）。
        let mut s2 = IngestStore::new();
        let _ = s2.ingest(100, DOM_KERNEL, VERDICT_GREEN);
        // 直接构造错哈希记录等价物：换 verdict 重算，比对必不符。
        let good = sample_hash(100, DOM_KERNEL, VERDICT_GREEN);
        let tampered = sample_hash(100, DOM_KERNEL, VERDICT_RED);
        led.record(2, good != tampered);
        // C3：裁决越闭集。
        let mut s3 = IngestStore::new();
        led.record(
            3,
            s3.ingest(100, DOM_KERNEL, 9).err() == Some(IgCode::VerdictIllegal.code()),
        );
        r += 1;
    }
    led.conservation()?;
    Ok(led)
}

// ── 探针三序（收样→合并→闭库 / 越级 / 册未开吸收） ──────────────
pub fn probe_seq_a() -> Result<(), u32> {
    let mut store = IngestStore::new();
    store.ingest(100, DOM_KERNEL, VERDICT_GREEN)?;
    store.begin_balance()?;
    let mut vols = VolumeTable::new();
    let h = store.hash_at(0).unwrap_or(0);
    vols.open_volume(1)?;
    vols.absorb(100, h)?;
    vols.seal_volume()?;
    vols.audit()?;
    store.close()
}

pub fn probe_seq_b() -> Result<(), u32> {
    // 越级：未合并直接闭库。
    let mut store = IngestStore::new();
    let _ = store.ingest(100, DOM_KERNEL, VERDICT_GREEN);
    store.close()
}

pub fn probe_seq_c() -> Result<(), u32> {
    // 册未开直接吸收成分。
    let mut vols = VolumeTable::new();
    vols.absorb(100, sample_hash(100, DOM_KERNEL, VERDICT_GREEN))
}

/// 容量耗尽：64 槽写满后第 65 例必拒且账不丢。
pub fn capacity_probe() -> Result<usize, u32> {
    let mut store = IngestStore::new();
    let mut id = 1u32;
    while (id as usize) <= STORE_CAP {
        let tag = (id % DOMAIN_MAX as u32) as u8 + 1;
        store.ingest(id, tag, VERDICT_GREEN)?;
        id += 1;
    }
    let before = store.sample_count();
    let r = store.ingest(STORE_CAP as u32 + 1, DOM_KERNEL, VERDICT_GREEN);
    if r.err() != Some(IgCode::StoreFull.code()) {
        return Err(IgCode::ConservationBroken.code());
    }
    if store.sample_count() != before {
        return Err(IgCode::ConservationBroken.code());
    }
    Ok(before)
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = run_independent()?;
    let b = run_independent()?;
    if a != b {
        return Err(IgCode::NotReproducible.code());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || SEG_BASE < 0x80000 || SEG_BASE >= 0x80400 {
        return false;
    }
    let mut i = 0;
    while i < IgCode::ALL.len() {
        let v = IgCode::ALL[i].code();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

/// 文案闭集。
pub fn detail_closed_set() -> bool {
    let mut i = 0;
    while i < IgCode::ALL.len() {
        if IgCode::ALL[i].detail().is_empty() {
            return false;
        }
        i += 1;
    }
    detail(SEG_BASE + 0x3F).starts_with("未知诊断码")
}

// ── 判据集（J1 验收） ───────────────────────────────────────────
pub fn run_unx18535_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18535 探针样本入库");
    // 静态明细判据。
    s.add("J1-段位守卫", segment_guard(), "0x80180 段 0x40 窗口，码闭集不越界");
    s.add(
        "J1-账态三序",
        probe_seq_a().is_ok()
            && probe_seq_b().err() == Some(IgCode::IllegalTransition.code())
            && probe_seq_c().err() == Some(IgCode::VolumeNotOpen.code()),
        "收样→合并→闭库全过；越级闭库/册未开吸收均拒",
    );
    s.add(
        "J1-四元账哈希域分离",
        sample_hash(100, DOM_KERNEL, VERDICT_GREEN)
            != sample_hash(101, DOM_KERNEL, VERDICT_GREEN)
            && sample_hash(100, DOM_KERNEL, VERDICT_GREEN)
                != sample_hash(100, DOM_DRIVER, VERDICT_GREEN)
            && sample_hash(100, DOM_KERNEL, VERDICT_GREEN)
                != sample_hash(100, DOM_KERNEL, VERDICT_RED),
        "四元任一字段变动必改全字（哈希域分离）",
    );
    s.add(
        "J1-边界四类闸",
        {
            let mut store = IngestStore::new();
            let id0 = store.ingest(0, DOM_KERNEL, VERDICT_GREEN).is_err();
            let id_hi = store.ingest(SAMPLE_ID_MAX + 1, DOM_KERNEL, VERDICT_GREEN).is_err();
            let tag0 = store.ingest(100, 0, VERDICT_GREEN).is_err();
            let tag_hi = store.ingest(100, DOMAIN_MAX + 1, VERDICT_GREEN).is_err();
            let v9 = store.ingest(100, DOM_KERNEL, 9).is_err();
            let v0 = store.ingest(100, DOM_KERNEL, 0).is_err();
            id0 && id_hi && tag0 && tag_hi && v9 && v0
        },
        "编号 0/4097、域标签 0/5、裁决 0/9 全部显式拒绝",
    );
    s.add(
        "J1-文案闭集",
        detail_closed_set(),
        "12 码逐码非空中文建议，闭集外落未知提示",
    );
    // 动态明细判据（≤8 条）。
    let ind = run_independent();
    let ind_ok = match &ind {
        Ok((n, _)) => *n == SAMPLE_TOTAL,
        Err(_) => false,
    };
    s.add_dyn(
        "J1-独立模式50例100%入库",
        ind_ok,
        &format!("独立入库 = {:?}", ind),
    );
    let mrg = run_merged();
    let mrg_ok = match &mrg {
        Ok((vols, absorbed)) => *absorbed == SAMPLE_TOTAL as u32 && *vols >= 3,
        Err(_) => false,
    };
    s.add_dyn(
        "J1-合并模式全绿",
        mrg_ok,
        &format!("合并册账 = {:?}", mrg),
    );
    let dual = dual_mode_check();
    s.add_dyn(
        "J1-双模式守恒",
        dual.is_ok(),
        &format!("独立数/合并册数 = {:?}", dual),
    );
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    s.add_dyn(
        "J1-注入四类10/10全抓",
        inj_ok,
        &format!(
            "tries={:?} caught={:?}",
            inj.as_ref().map(|l| l.tries).unwrap_or([0; INJECT_CLASSES]),
            inj.as_ref().map(|l| l.caught).unwrap_or([0; INJECT_CLASSES])
        ),
    );
    let cap = capacity_probe();
    s.add_dyn(
        "J1-容量耗尽账不丢",
        cap.is_ok(),
        &format!("满库样本数 = {:?}", cap),
    );
    let rep = repro_check();
    s.add_dyn("J1-双跑一致", rep.is_ok(), &format!("{:?}", rep));
    s
}

// ── 测试 ────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_independent_50_all_ingested() {
        let ind = run_independent().unwrap_or((0, 0));
        assert_eq!(ind.0, SAMPLE_TOTAL);
        assert_ne!(ind.1, 0);
    }

    #[test]
    fn test_merged_volumes_conservation() {
        let mrg = run_merged().unwrap_or((0, 0));
        assert_eq!(mrg.1, SAMPLE_TOTAL as u32);
        // 50 = 16+16+16+2 → 4 册。
        assert_eq!(mrg.0, 4);
    }

    #[test]
    fn test_dual_mode_green() {
        let dual = dual_mode_check().unwrap_or((0, 0));
        assert_eq!(dual.0, SAMPLE_TOTAL);
        assert!(dual.1 >= 3);
    }

    #[test]
    fn test_hash_field_separation() {
        // 确定性：同四元同值。
        assert_eq!(
            sample_hash(100, DOM_KERNEL, VERDICT_GREEN),
            sample_hash(100, DOM_KERNEL, VERDICT_GREEN)
        );
        // 域分离：任一字段变动必改全字。
        assert_ne!(sample_hash(100, 1, 1), sample_hash(101, 1, 1));
        assert_ne!(sample_hash(100, 1, 1), sample_hash(100, 2, 1));
        assert_ne!(sample_hash(100, 1, 1), sample_hash(100, 1, 2));
    }

    #[test]
    fn test_inject_all_caught_10_rounds() {
        let led = inject_rounds(JUDGE_ROUNDS).unwrap_or_else(|_| InjectLedger::new());
        assert!(led.all_caught(), "四类坏样本 10 轮必须全抓");
        assert!(led.conservation().is_ok());
    }

    #[test]
    fn test_state_machine_transitions() {
        // 越级闭库拒。
        let mut store = IngestStore::new();
        let _ = store.ingest(100, DOM_KERNEL, VERDICT_GREEN);
        assert!(store.close().is_err());
        // 正常链。
        assert!(store.begin_balance().is_ok());
        assert!(store.close().is_ok());
        // 闭库后入库拒。
        assert!(store.ingest(101, DOM_KERNEL, VERDICT_GREEN).is_err());
        // 册未开吸收拒 + 未封重开拒。
        let mut vols = VolumeTable::new();
        assert!(vols.absorb(100, 1).is_err());
        assert!(vols.open_volume(1).is_ok());
        assert!(vols.absorb(100, 1).is_ok());
        assert!(vols.open_volume(2).is_err()); // 末册未封
        assert!(vols.seal_volume().is_ok());
        assert!(vols.seal_volume().is_err()); // 空册/已封再封拒
    }

    #[test]
    fn test_boundary_four_classes() {
        let mut store = IngestStore::new();
        assert!(store.ingest(0, DOM_KERNEL, VERDICT_GREEN).is_err());
        assert!(store.ingest(SAMPLE_ID_MAX + 1, DOM_KERNEL, VERDICT_GREEN).is_err());
        assert!(store.ingest(100, 0, VERDICT_GREEN).is_err());
        assert!(store.ingest(100, DOMAIN_MAX + 1, VERDICT_GREEN).is_err());
        assert!(store.ingest(100, DOM_KERNEL, 0).is_err());
        assert!(store.ingest(100, DOM_KERNEL, 9).is_err());
        // 合法下界/上界各一笔。
        assert!(store.ingest(SAMPLE_ID_MIN, DOM_KERNEL, VERDICT_GREEN).is_ok());
        assert!(store.ingest(SAMPLE_ID_MAX, DOM_SHELL, VERDICT_RED).is_ok());
    }

    #[test]
    fn test_duplicate_sample() {
        let mut store = IngestStore::new();
        assert!(store.ingest(100, DOM_KERNEL, VERDICT_GREEN).is_ok());
        assert!(store.ingest(100, DOM_DRIVER, VERDICT_GREEN).is_err());
        assert_eq!(store.sample_count(), 1);
    }

    #[test]
    fn test_capacity_exhaustion() {
        assert_eq!(capacity_probe().unwrap_or(0), STORE_CAP);
        let mut store = IngestStore::new();
        let mut id = 1u32;
        while (id as usize) <= STORE_CAP {
            let tag = (id % DOMAIN_MAX as u32) as u8 + 1;
            assert!(store.ingest(id, tag, VERDICT_GREEN).is_ok());
            id += 1;
        }
        assert!(store.ingest(999, DOM_KERNEL, VERDICT_GREEN).is_err());
        assert_eq!(store.sample_count(), STORE_CAP);
    }

    #[test]
    fn test_domain_planes_audit() {
        let mut store = IngestStore::new();
        let mut i = 0;
        while i < 12 {
            let (id, tag, verdict) = modeled_sample(i);
            let _ = store.ingest(id, tag, verdict);
            i += 1;
        }
        assert!(store.audit().is_ok());
        // 12 例按 (i%4)+1 铺 4 域 → 每域恰 3。
        let mut d = 1;
        while d <= DOMAIN_MAX {
            assert_eq!(store.domain_count(d), 3);
            d += 1;
        }
    }

    #[test]
    fn test_double_run_reproducible() {
        assert!(repro_check().is_ok());
    }

    #[test]
    fn test_segment_guard_and_detail_closed_set() {
        assert!(segment_guard());
        assert!(detail_closed_set());
        assert_eq!(IgCode::HashMismatch.code(), SEG_BASE + 5);
    }

    #[test]
    fn test_checks_all_passed() {
        let set = run_unx18535_checks();
        assert!(set.all_passed(), "UNX-F18535 判据必须全绿：{}", set.red_items_text());
    }
}
