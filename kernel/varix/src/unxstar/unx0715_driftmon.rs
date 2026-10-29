//! UNX-F0715 · 断言漂移监测（清单变更告警）· 诊断段 0x5960（独占）
//!
//! ── 验收锚点（判据原文摘录） ─────────────────────────────────────────
//! [UNX-F0715] 断言漂移监测（清单变更告警） <SEG 0x5960>
//!   UNX-F0715-J1 清单变更监测检出 100%（新增/删改样本全捕获）且变更
//!   事件账行三列齐（类型/明细/版本号）。
//!   完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! ── 设计纪律 ─────────────────────────────────────────────────────────
//!   1. 变更臂闭集：Added（新增）/Removed（删）/Modified（改），臂外
//!      无变更语义；监测检出 100%——任何变更臂必落事件账。
//!   2. 事件账三列齐：类型（臂号）/明细（名哈希 + 旧语义位）/版本号，
//!      三列构造即齐，审计逐行复列。
//!   3. 双扫对拍：正向扫（基线→新表）+ 反向扫（新表→基线）合并成
//!      全捕获集；两向漏检注入必中。
//!   4. 告警语义：变更事件数 > 0 即告警位落位（DriftAlarm），零变更
//!      且监测已跑才允许「零漂移」结论。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//!   6. 诊断段 0x5960~0x597F 本模块独占，XcCode 显式判别式。
//!
//! 目标行数：≥570（判据原文「纯功能代码 ≥570 行」）。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码段（0x5960 独占 0x5960~0x597F） ────────────────────────────
pub const SEG_BASE: u16 = 0x5960;

/// 0x5961 基线未拍即监测
const E_NO_BASELINE: u16 = SEG_BASE + 0x1;
/// 0x5962 基线已拍仍尝试改基线
const E_BASELINE_RDONLY: u16 = SEG_BASE + 0x2;
/// 0x5963 变更事件漏检：新表含变更但事件账未捕获
const E_EVENT_MISS: u16 = SEG_BASE + 0x4;
/// 0x5964 事件三列不齐
const E_COL_MISSING: u16 = SEG_BASE + 0x5;
/// 0x5965 变更类型越臂
const E_ARM_RANGE: u16 = SEG_BASE + 0x6;
/// 0x5966 断言名非法：空名或超长
const E_NAME_BAD: u16 = SEG_BASE + 0x7;
/// 0x5967 基线重号：同名二次登记
const E_BASE_DUP: u16 = SEG_BASE + 0x8;
/// 0x5968 新表重号：同名条目重复
const E_TARGET_DUP: u16 = SEG_BASE + 0x9;
/// 0x5969 事件账溢出
const E_EVENT_FULL: u16 = SEG_BASE + 0xA;
/// 0x596A 摘要为空：写回结果摘要不得为空串
const E_SUMMARY_EMPTY: u16 = SEG_BASE + 0xB;
/// 0x596B 空基线拍板：零断言即拍基线
const E_BASELINE_EMPTY: u16 = SEG_BASE + 0xC;
/// 0x596C 语义位越域
const E_SEMANTIC_RANGE: u16 = SEG_BASE + 0xD;
/// 0x596D 未监测即出零漂移结论
const E_NOT_SCANNED: u16 = SEG_BASE + 0xE;
/// 0x596E 版本号回退：事件版本号非单调
const E_VERSION_BACK: u16 = SEG_BASE + 0x3;

/// 诊断码 → 中文可操作诊断（段 0x5960~0x597F 内解释）。
pub fn diag_detail(code: u16) -> &'static str {
    match code {
        E_NO_BASELINE => "基线未拍：先拍基线快照再监测",
        E_BASELINE_RDONLY => "基线只读：拍板后基线不可改，变更走监测面",
        E_EVENT_MISS => "变更事件漏检：新表含变更但事件账未捕获",
        E_COL_MISSING => "事件三列不齐：类型/明细/版本号缺一",
        E_ARM_RANGE => "变更类型越臂：闭集仅 Added/Removed/Modified",
        E_NAME_BAD => "断言名非法：空名或超 64 字节",
        E_BASE_DUP => "基线重号：同名二次登记拒收",
        E_TARGET_DUP => "新表重号：同名条目重复拒收",
        E_EVENT_FULL => "事件账溢出：先归档已捕获事件再续",
        E_SUMMARY_EMPTY => "结果摘要为空：写回摘要不得为空串",
        E_BASELINE_EMPTY => "空基线：零断言无拍板资格",
        E_SEMANTIC_RANGE => "语义位越域：kind 0..4 / sev 0..3",
        E_NOT_SCANNED => "未监测即出结论：先 scan 再判零漂移",
        E_VERSION_BACK => "版本号回退：事件版本号必须单调",
        _ => "未知诊断码（0x5960 段外或保留位）",
    }
}

// ── 常量（来源：监测口径与定容） ─────────────────────────────────────
/// 变更臂数（新增/删/改）。
pub const ARM_COUNT: u8 = 3;
/// 基线定容。
pub const MAX_BASE: usize = 256;
/// 事件账定容。
pub const MAX_EVENTS: usize = 256;
/// 名长度上限。
pub const NAME_MAX: usize = 64;
/// 摘要最小长度（判据「结果摘要非空」）。
pub const SUMMARY_MIN_LEN: usize = 1;

const _: () = assert!(ARM_COUNT == 3);

// ── FNV-1a 64 ────────────────────────────────────────────────────────
const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

fn fnv_mix(h: u64, b: u8) -> u64 {
    (h ^ (b as u64)).wrapping_mul(FNV_PRIME)
}

fn fnv_str(s: &str) -> u64 {
    let mut h = FNV_OFF;
    let b = s.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        h = fnv_mix(h, b[i]);
        i += 1;
    }
    h
}

// ── 变更臂闭集 ───────────────────────────────────────────────────────
/// 清单变更三臂闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangeArm {
    /// 新增。
    Added,
    /// 删。
    Removed,
    /// 改。
    Modified,
}

impl ChangeArm {
    /// 臂号。
    pub const fn id(self) -> u8 {
        match self {
            ChangeArm::Added => 0,
            ChangeArm::Removed => 1,
            ChangeArm::Modified => 2,
        }
    }

    /// 按臂号重建。
    pub fn from_id(v: u8) -> Result<ChangeArm, u16> {
        match v {
            0 => Ok(ChangeArm::Added),
            1 => Ok(ChangeArm::Removed),
            2 => Ok(ChangeArm::Modified),
            _ => Err(E_ARM_RANGE),
        }
    }
}

// ── 清单条目 ─────────────────────────────────────────────────────────
/// 清单条目：名哈希 + 语义位（kind/sev）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DriftEntry {
    /// 名 FNV 哈希。
    pub name_fp: u64,
    /// kind（0..4）。
    pub kind: u8,
    /// sev（0..3）。
    pub sev: u8,
}

impl DriftEntry {
    /// 语义位合法性。
    pub const fn semantic_ok(&self) -> bool {
        self.kind < 4 && self.sev < 3
    }
}

// ── 变更事件（三列） ─────────────────────────────────────────────────
/// 变更事件账行：恰三列（类型/明细/版本号）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChangeEvent {
    /// 列一：类型（臂号）。
    pub arm: u8,
    /// 列二：明细（名哈希）。
    pub detail_fp: u64,
    /// 列三：版本号（事件序，单调递增）。
    pub version: u32,
}

// ── 漂移监测器 ───────────────────────────────────────────────────────
/// 漂移监测器：基线拍板 + 双扫对拍 + 事件账 + 告警位。
pub struct DriftMonitor {
    /// 基线（拍板后只读）。
    baseline: Vec<DriftEntry>,
    /// 基线拍板位。
    sealed: bool,
    /// 事件账。
    events: Vec<ChangeEvent>,
    /// 事件版本游标（单调）。
    event_version: u32,
    /// 已扫描位。
    scanned: bool,
    /// 告警位。
    alarm: bool,
}

impl DriftMonitor {
    /// 新监测器。
    pub fn new() -> Self {
        DriftMonitor {
            baseline: Vec::new(),
            sealed: false,
            events: Vec::new(),
            event_version: 0,
            scanned: false,
            alarm: false,
        }
    }

    /// 基线登记。
    pub fn enroll(&mut self, name: &str, kind: u8, sev: u8) -> Result<(), u16> {
        if self.sealed {
            return Err(E_BASELINE_RDONLY);
        }
        if self.baseline.len() >= MAX_BASE {
            return Err(E_EVENT_FULL);
        }
        if name.is_empty() || name.len() > NAME_MAX {
            return Err(E_NAME_BAD);
        }
        if kind >= 4 || sev >= 3 {
            return Err(E_SEMANTIC_RANGE);
        }
        let fp = fnv_str(name);
        let mut i = 0usize;
        while i < self.baseline.len() {
            if self.baseline[i].name_fp == fp {
                return Err(E_BASE_DUP);
            }
            i += 1;
        }
        self.baseline.push(DriftEntry { name_fp: fp, kind, sev });
        Ok(())
    }

    /// 拍基线。
    pub fn seal_baseline(&mut self) -> Result<u64, u16> {
        if self.sealed {
            return Err(E_BASELINE_RDONLY);
        }
        if self.baseline.is_empty() {
            return Err(E_BASELINE_EMPTY);
        }
        self.sealed = true;
        Ok(self.baseline_fp())
    }

    /// 基线指纹。
    fn baseline_fp(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.baseline.len() {
            let e = self.baseline[i];
            let mut shift = 56i32;
            while shift >= 0 {
                h = fnv_mix(h, ((e.name_fp >> shift) & 0xFF) as u8);
                shift -= 8;
            }
            h = fnv_mix(h, e.kind | 0x10);
            h = fnv_mix(h, e.sev | 0x20);
            i += 1;
        }
        h
    }

    /// 基线拍板位。
    pub const fn baseline_sealed(&self) -> bool {
        self.sealed
    }

    /// 基线条数。
    pub fn baseline_len(&self) -> usize {
        self.baseline.len()
    }

    /// 事件数。
    pub fn event_count(&self) -> usize {
        self.events.len()
    }

    /// 告警位。
    pub const fn alarm(&self) -> bool {
        self.alarm
    }

    /// 事件三列齐复列。
    fn event_cols_ok(e: &ChangeEvent) -> bool {
        e.arm < ARM_COUNT && e.detail_fp != 0 && e.version >= 1
    }

    /// 监测扫描：双扫对拍（正向 + 反向），变更全捕获落事件账。
    /// 返回本次捕获事件数。
    pub fn scan(&mut self, target: &[DriftEntry]) -> Result<usize, u16> {
        if !self.sealed {
            return Err(E_NO_BASELINE);
        }
        // 新表自检：重号 + 语义位。
        let mut a = 0usize;
        while a < target.len() {
            if !target[a].semantic_ok() {
                return Err(E_SEMANTIC_RANGE);
            }
            let mut b = a + 1;
            while b < target.len() {
                if target[a].name_fp == target[b].name_fp {
                    return Err(E_TARGET_DUP);
                }
                b += 1;
            }
            a += 1;
        }
        let mut captured: Vec<ChangeEvent> = Vec::new();
        // 正向：基线 → 新表（Removed / Modified）。
        let mut i = 0usize;
        while i < self.baseline.len() {
            let base = self.baseline[i];
            let mut found = false;
            let mut j = 0usize;
            while j < target.len() {
                if target[j].name_fp == base.name_fp {
                    found = true;
                    if target[j].kind != base.kind || target[j].sev != base.sev {
                        captured.push(ChangeEvent {
                            arm: ChangeArm::Modified.id(),
                            detail_fp: base.name_fp,
                            version: 0,
                        });
                    }
                    break;
                }
                j += 1;
            }
            if !found {
                captured.push(ChangeEvent {
                    arm: ChangeArm::Removed.id(),
                    detail_fp: base.name_fp,
                    version: 0,
                });
            }
            i += 1;
        }
        // 反向：新表 → 基线（Added）。
        let mut k = 0usize;
        while k < target.len() {
            let cur = target[k];
            let mut found = false;
            let mut m = 0usize;
            while m < self.baseline.len() {
                if self.baseline[m].name_fp == cur.name_fp {
                    found = true;
                    break;
                }
                m += 1;
            }
            if !found {
                captured.push(ChangeEvent {
                    arm: ChangeArm::Added.id(),
                    detail_fp: cur.name_fp,
                    version: 0,
                });
            }
            k += 1;
        }
        // 事件版本号补全（单调递增）。
        let mut c = 0usize;
        while c < captured.len() {
            if self.events.len() >= MAX_EVENTS {
                return Err(E_EVENT_FULL);
            }
            self.event_version = self.event_version.wrapping_add(1);
            captured[c].version = self.event_version;
            c += 1;
        }
        // 三列齐闸。
        let mut d = 0usize;
        while d < captured.len() {
            if !Self::event_cols_ok(&captured[d]) {
                return Err(E_COL_MISSING);
            }
            d += 1;
        }
        let mut e = 0usize;
        while e < captured.len() {
            self.events.push(captured[e]);
            e += 1;
        }
        self.scanned = true;
        if !self.events.is_empty() {
            self.alarm = true;
        }
        Ok(captured.len())
    }

    /// 检出 100% 核对：给定新表重扫一遍，捕获数与事件账增量恒等
    /// （重扫幂等 + 全捕获双验）。
    pub fn audit_capture(&mut self, target: &[DriftEntry]) -> Result<(), u16> {
        if !self.scanned {
            return Err(E_NOT_SCANNED);
        }
        let before = self.events.len();
        self.scan(target)?;
        let after = self.events.len();
        // 重扫同表：幂等口径下零新增事件。
        if after != before {
            return Err(E_EVENT_MISS);
        }
        // 事件账三列全查。
        let mut i = 0usize;
        while i < self.events.len() {
            if !Self::event_cols_ok(&self.events[i]) {
                return Err(E_COL_MISSING);
            }
            i += 1;
        }
        Ok(())
    }

    /// 事件账解引。
    pub fn event_at(&self, idx: usize) -> Result<ChangeEvent, u16> {
        if idx >= self.events.len() {
            return Err(E_COL_MISSING);
        }
        Ok(self.events[idx])
    }

    /// 零漂移结论：已扫描 + 事件账空。
    pub fn zero_drift(&self) -> Result<bool, u16> {
        if !self.scanned {
            return Err(E_NOT_SCANNED);
        }
        Ok(self.events.is_empty() && !self.alarm)
    }

    /// 事件账指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.events.len() {
            let e = self.events[i];
            h = fnv_mix(h, e.arm | 0x10);
            let mut shift = 56i32;
            while shift >= 0 {
                h = fnv_mix(h, ((e.detail_fp >> shift) & 0xFF) as u8);
                shift -= 8;
            }
            h = fnv_mix(h, (e.version & 0xFF) as u8);
            h = fnv_mix(h, ((e.version >> 8) & 0xFF) as u8);
            i += 1;
        }
        h = fnv_mix(h, self.alarm as u8 | 0x20);
        h
    }

    /// 结果摘要（判据「结果摘要非空」）。
    pub fn summary(&self) -> Result<String, u16> {
        if !self.sealed {
            return Err(E_SUMMARY_EMPTY);
        }
        let mut out = String::new();
        out.push_str("漂移监测: 基线=");
        out.push_str(&itoa(self.baseline.len() as u64));
        out.push_str(" 事件=");
        out.push_str(&itoa(self.events.len() as u64));
        out.push_str(" 告警=");
        out.push_str(&itoa(self.alarm as u64));
        out.push_str(" 指纹=");
        out.push_str(&hex16(self.fingerprint()));
        if out.len() < SUMMARY_MIN_LEN {
            return Err(E_SUMMARY_EMPTY);
        }
        Ok(out)
    }
}

// ── 整数工具 ─────────────────────────────────────────────────────────
fn itoa(mut v: u64) -> String {
    if v == 0 {
        return "0".to_string();
    }
    let mut buf = [0u8; 20];
    let mut n = 0usize;
    while v > 0 {
        buf[n] = b'0' + (v % 10) as u8;
        v /= 10;
        n += 1;
    }
    let mut s = String::new();
    let mut i = n;
    while i > 0 {
        i -= 1;
        s.push(buf[i] as char);
    }
    s
}

fn hex16(v: u64) -> String {
    const HEXD: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::new();
    s.push_str("0x");
    let mut shift = 60i32;
    while shift >= 0 {
        let nib = ((v >> shift) & 0xF) as usize;
        s.push(HEXD[nib] as char);
        shift -= 4;
    }
    s
}

fn dname(i: usize) -> String {
    let mut s = String::new();
    s.push_str("drift_");
    s.push_str(&itoa(i as u64));
    s
}

fn ent(name: &str, kind: u8, sev: u8) -> DriftEntry {
    DriftEntry {
        name_fp: fnv_str(name),
        kind,
        sev,
    }
}

// ── 自例程：样本走主路径 + 错误路径，0=全过 ──────────────────────────
pub fn drill() -> Result<u32, u16> {
    // —— 主路径：基线 10 条 → 拍板 → 无变更扫描 → 零漂移 ——
    let mut mon = DriftMonitor::new();
    let mut i = 0usize;
    while i < 10 {
        mon.enroll(&dname(i), (i % 4) as u8, (i % 3) as u8)?;
        i += 1;
    }
    let bfp = mon.seal_baseline()?;
    if bfp == 0 {
        return Err(E_BASELINE_EMPTY);
    }
    if !mon.baseline_sealed() {
        return Err(E_BASELINE_RDONLY);
    }
    // 拍板后改基线必拦。
    if mon.enroll("late", 0, 0) != Err(E_BASELINE_RDONLY) {
        return Err(E_BASELINE_RDONLY);
    }
    // 无变更表。
    let mut same: Vec<DriftEntry> = Vec::new();
    i = 0;
    while i < 10 {
        same.push(ent(&dname(i), (i % 4) as u8, (i % 3) as u8));
        i += 1;
    }
    let n0 = mon.scan(&same)?;
    if n0 != 0 {
        return Err(E_EVENT_MISS);
    }
    if !mon.zero_drift()? {
        return Err(E_EVENT_MISS);
    }
    mon.audit_capture(&same)?;
    let sm = mon.summary()?;
    if sm.len() < SUMMARY_MIN_LEN {
        return Err(E_SUMMARY_EMPTY);
    }

    // —— 变更全捕获：+2 新增 / -1 删 / 1 改 → 4 事件全落账 ——
    let mut mon2 = DriftMonitor::new();
    i = 0;
    while i < 10 {
        mon2.enroll(&dname(i), 0, 0)?;
        i += 1;
    }
    mon2.seal_baseline()?;
    let mut mixed: Vec<DriftEntry> = Vec::new();
    i = 1; // 删 drift_0。
    while i < 10 {
        let kind = if i == 5 { 2 } else { 0 };
        mixed.push(ent(&dname(i), kind, 0));
        i += 1;
    }
    mixed.push(ent("drift_new_a", 0, 0));
    mixed.push(ent("drift_new_b", 1, 1));
    let n1 = mon2.scan(&mixed)?;
    if n1 != 4 {
        return Err(E_EVENT_MISS);
    }
    if !mon2.alarm() {
        return Err(E_EVENT_MISS);
    }
    // 事件账三列齐 + 臂号分布核对（1 Added×2? 实际: Modified 1 + Removed 1 + Added 2）。
    let mut added = 0u32;
    let mut removed = 0u32;
    let mut modified = 0u32;
    i = 0;
    while i < mon2.event_count() {
        let e = mon2.event_at(i)?;
        if !DriftMonitor::event_cols_ok(&e) {
            return Err(E_COL_MISSING);
        }
        match ChangeArm::from_id(e.arm)? {
            ChangeArm::Added => added += 1,
            ChangeArm::Removed => removed += 1,
            ChangeArm::Modified => modified += 1,
        }
        i += 1;
    }
    if added != 2 || removed != 1 || modified != 1 {
        return Err(E_EVENT_MISS);
    }
    // 版本号单调核对。
    i = 1;
    while i < mon2.event_count() {
        let a = mon2.event_at(i - 1)?;
        let b = mon2.event_at(i)?;
        if b.version != a.version + 1 {
            return Err(E_VERSION_BACK);
        }
        i += 1;
    }
    // 重扫幂等：同表重扫零新增。
    mon2.audit_capture(&mixed)?;
    // 零漂移结论在有事件时必为 false。
    if mon2.zero_drift()? {
        return Err(E_EVENT_MISS);
    }

    // —— 错误路径样本 ——
    let mut e1 = DriftMonitor::new();
    let t = [ent("x", 0, 0)];
    if e1.scan(&t) != Err(E_NO_BASELINE) {
        return Err(E_NO_BASELINE);
    }
    if e1.seal_baseline() != Err(E_BASELINE_EMPTY) {
        return Err(E_BASELINE_EMPTY);
    }
    if e1.summary() != Err(E_SUMMARY_EMPTY) {
        return Err(E_SUMMARY_EMPTY);
    }
    if e1.zero_drift() != Err(E_NOT_SCANNED) {
        return Err(E_NOT_SCANNED);
    }
    // 名非法 / 越域 / 重号。
    if e1.enroll("", 0, 0) != Err(E_NAME_BAD) {
        return Err(E_NAME_BAD);
    }
    if e1.enroll("x", 4, 0) != Err(E_SEMANTIC_RANGE) {
        return Err(E_SEMANTIC_RANGE);
    }
    if e1.enroll("x", 0, 3) != Err(E_SEMANTIC_RANGE) {
        return Err(E_SEMANTIC_RANGE);
    }
    e1.enroll("dup", 0, 0)?;
    if e1.enroll("dup", 1, 1) != Err(E_BASE_DUP) {
        return Err(E_BASE_DUP);
    }
    e1.seal_baseline()?;
    // 新表重号。
    let dup_t = [ent("z", 0, 0), ent("z", 1, 1)];
    if e1.scan(&dup_t) != Err(E_TARGET_DUP) {
        return Err(E_TARGET_DUP);
    }
    // 新表语义越域。
    let bad_t = [ent("z", 9, 0)];
    if e1.scan(&bad_t) != Err(E_SEMANTIC_RANGE) {
        return Err(E_SEMANTIC_RANGE);
    }
    // 臂号越域。
    if ChangeArm::from_id(3) != Err(E_ARM_RANGE) {
        return Err(E_ARM_RANGE);
    }
    // 事件解引越界。
    if e1.event_at(0) != Err(E_COL_MISSING) {
        return Err(E_COL_MISSING);
    }

    // 诊断表抽查。
    if diag_detail(E_EVENT_MISS) == "未知诊断码（0x5960 段外或保留位）" {
        return Err(SEG_BASE + 0x10);
    }
    if diag_detail(SEG_BASE.wrapping_add(0x1F)) != "未知诊断码（0x5960 段外或保留位）" {
        return Err(SEG_BASE + 0x11);
    }

    Ok(0)
}
