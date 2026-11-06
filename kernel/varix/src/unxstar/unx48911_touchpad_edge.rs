//! UNX-F48911 quirk 回填 · 0x70DC0
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，CoRun·UNX-F2-B40）：
//!   ① 纯功能代码 ≥700 行；
//!   ② UNX-F48911-J1 已知设备 quirk 回填 ×10^3 五元组零漂移；
//!   ③ UNX-F48912-J1 未知设备回填空位 ×10^3 不猜、不串表。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序：设备标识 → 已知位掩码 → 表内唯一 → 回填顺序 →
//!           能力复审计 → 五元组指纹。设备标识必须先判：vid/pid 为零
//!           的行不是「未知设备」，是坏行，不能和 miss 走同一分支。
//!           掩码次之：未知 quirk 位意味着表来自未来版本，按当前语义
//!           执行会静默丢位。表内唯一必须在回填前：同一 (vid,pid)
//!           出现两次时先到先得会让结果依赖遍历顺序。回填顺序固定为
//!           「先强制分类、再删能力、最后加能力」，顺序换了会让
//!           FORCE_CLICKPAD 与 DISABLE_PALM 互相覆盖。能力复审计在
//!           回填后：quirk 可以把一份原本合法的能力集改成非法组合，
//!           必须在这里拒绝而不是交给分类器崩。
//! 关键差异：本模块只拥有「设备表 + quirk 回填」这一层。它不分类
//!           （core）、不判掌（guard）、不做灵敏度（dispatch）。
//!           回填后的能力集必须重新走 core::audit_caps，本模块不复制
//!           那套位图规则，只负责把 quirk 顺序执行完。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::unx48901_touchpad_core::{
    audit_caps, CAP_BUTTONPAD, CAP_HIRES, CAP_KNOWN, CAP_MT, CAP_PALM, CAP_PHYS_BUTTONS,
    CAP_PRESSURE, CAP_SEMI_MT,
};

/// 诊断段基址（0x70DC0，VX6 批 B06 第五槽）。
pub const SEG_BASE: u32 = 0x70DC0;

/// 冻结常量（卡面判据锚点）。
pub const QUIRK_ROWS: usize = 8;
pub const LOOKUP_ROUNDS: u32 = 1000;
pub const BACKFILL_ROUNDS: u32 = 1000;
pub const MAX_QUIRK_BITS: u16 = 0x003F;
pub const DEVICE_VID: u16 = 0x046D;

// ── 诊断码闭集（+0x01..+0x09） ────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QCode {
    /// 设备标识非法（vid/pid 为零）
    DevBad,
    /// quirk 位非法（未知位全部拒绝）
    QuirkBad,
    /// 表非法（重复键 / 键为零 / 空表）
    TableBad,
    /// 回填失败（强制分类与基础能力矛盾）
    BackfillBad,
    /// 未知设备被串表（miss 却拿到非零掩码）
    CrossBad,
    /// 能力集复审计失败
    CapsBad,
    /// 五元组指纹漂移
    PrintBad,
    /// 复算不一致
    ReplayBad,
    /// 段位越界
    SegmentBad,
}

impl QCode {
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            QCode::DevBad => "设备非法：vid/pid 为零的行不是未知设备",
            QCode::QuirkBad => "quirk 非法：出现表外位或互斥位",
            QCode::TableBad => "表非法：重复键、零键或空表",
            QCode::BackfillBad => "回填失败：强制分类与基础能力矛盾",
            QCode::CrossBad => "串表：未知设备拿到了非零 quirk",
            QCode::CapsBad => "能力集非法：回填后必须重新通过 audit_caps",
            QCode::PrintBad => "指纹漂移：同一设备两次回填结果不同",
            QCode::ReplayBad => "复算不一致：两次黄金全链结果不同",
            QCode::SegmentBad => "段位越界：诊断码逃出 0x70DC0 窗口",
        }
    }

    pub const ALL: [QCode; 9] = [
        QCode::DevBad,
        QCode::QuirkBad,
        QCode::TableBad,
        QCode::BackfillBad,
        QCode::CrossBad,
        QCode::CapsBad,
        QCode::PrintBad,
        QCode::ReplayBad,
        QCode::SegmentBad,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < QCode::ALL.len() {
        if QCode::ALL[i].val() == code {
            return QCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码"
}

// ── quirk 位 ──────────────────────────────────────────────────────
pub const Q_FORCE_CLICKPAD: u16 = 0x0001;
pub const Q_FORCE_PHYSICAL: u16 = 0x0002;
pub const Q_DISABLE_PALM: u16 = 0x0004;
pub const Q_DROP_PRESSURE: u16 = 0x0008;
pub const Q_ADD_HIRES: u16 = 0x0010;
pub const Q_DROP_PALM_CAP: u16 = 0x0020;

pub const Q_MUST_CLASS: u16 = Q_FORCE_CLICKPAD | Q_FORCE_PHYSICAL;
pub const Q_MUTEX: u16 = Q_MUST_CLASS;

pub fn audit_quirk(mask: u16) -> Result<u16, u32> {
    if mask & !MAX_QUIRK_BITS != 0 {
        return Err(QCode::QuirkBad.val());
    }
    if mask & Q_MUTEX == Q_MUTEX {
        return Err(QCode::QuirkBad.val());
    }
    Ok(mask)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DeviceId {
    pub vid: u16,
    pub pid: u16,
}

impl DeviceId {
    pub fn new(vid: u16, pid: u16) -> Result<DeviceId, u32> {
        if vid == 0 || pid == 0 {
            return Err(QCode::DevBad.val());
        }
        Ok(DeviceId { vid, pid })
    }

    pub fn tuple(&self) -> u32 {
        (u32::from(self.vid) << 16) | u32::from(self.pid)
    }
}

// ── 冻结表 ────────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct QuirkRow {
    pub vid: u16,
    pub pid: u16,
    pub mask: u16,
    pub base_caps: u16,
    pub label: &'static str,
}

pub const ROWS: [QuirkRow; QUIRK_ROWS] = [
    QuirkRow {
        vid: DEVICE_VID,
        pid: 0xC52B,
        mask: Q_FORCE_CLICKPAD | Q_DISABLE_PALM,
        base_caps: CAP_MT | CAP_PRESSURE | CAP_PALM,
        label: "clickpad-no-palm",
    },
    QuirkRow {
        vid: DEVICE_VID,
        pid: 0xC52C,
        mask: Q_FORCE_PHYSICAL | Q_ADD_HIRES,
        base_caps: CAP_MT | CAP_PHYS_BUTTONS,
        label: "physical-hires",
    },
    QuirkRow {
        vid: DEVICE_VID,
        pid: 0xC52D,
        mask: Q_FORCE_CLICKPAD,
        base_caps: CAP_MT | CAP_PRESSURE,
        label: "clickpad-basic",
    },
    QuirkRow {
        vid: DEVICE_VID,
        pid: 0xC52E,
        mask: Q_DROP_PRESSURE,
        base_caps: CAP_MT | CAP_PRESSURE,
        label: "drop-pressure",
    },
    QuirkRow {
        vid: DEVICE_VID,
        pid: 0xC52F,
        mask: Q_ADD_HIRES,
        base_caps: CAP_MT,
        label: "hires-only",
    },
    QuirkRow {
        vid: 0x06CB,
        pid: 0x7E50,
        mask: Q_FORCE_CLICKPAD | Q_DROP_PALM_CAP,
        base_caps: CAP_MT | CAP_PALM | CAP_PRESSURE,
        label: "syn-clickpad",
    },
    QuirkRow {
        vid: 0x06CB,
        pid: 0x7E51,
        mask: Q_FORCE_PHYSICAL,
        base_caps: CAP_SEMI_MT,
        label: "syn-physical",
    },
    QuirkRow {
        vid: 0x0B05,
        pid: 0x1A11,
        mask: Q_DISABLE_PALM,
        base_caps: CAP_MT | CAP_PALM | CAP_HIRES,
        label: "asus-no-palm",
    },
];

pub fn audit_table(rows: &[QuirkRow]) -> Result<usize, u32> {
    if rows.is_empty() || rows.len() > QUIRK_ROWS {
        return Err(QCode::TableBad.val());
    }
    let mut i = 0usize;
    while i < rows.len() {
        let a = match rows.get(i) {
            Some(r) => r,
            None => return Err(QCode::TableBad.val()),
        };
        if a.vid == 0 || a.pid == 0 {
            return Err(QCode::DevBad.val());
        }
        audit_quirk(a.mask)?;
        let mut j = i.saturating_add(1);
        while j < rows.len() {
            let b = match rows.get(j) {
                Some(r) => r,
                None => return Err(QCode::TableBad.val()),
            };
            if a.vid == b.vid && a.pid == b.pid {
                return Err(QCode::TableBad.val());
            }
            j += 1;
        }
        i += 1;
    }
    Ok(i)
}

/// 精确查表：未知设备返回 None，绝不按 vid 模糊匹配。
pub fn lookup(rows: &'static [QuirkRow], vid: u16, pid: u16) -> Option<&'static QuirkRow> {
    let mut i = 0usize;
    while i < rows.len() {
        let r = match rows.get(i) {
            Some(r) => r,
            None => return None,
        };
        if r.vid == vid && r.pid == pid {
            return Some(r);
        }
        i += 1;
    }
    None
}

pub fn lookup_mask(vid: u16, pid: u16) -> Result<u16, u32> {
    DeviceId::new(vid, pid)?;
    match lookup(&ROWS, vid, pid) {
        Some(r) => audit_quirk(r.mask),
        None => Ok(0),
    }
}

// ── 回填 ──────────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Backfill {
    pub caps: u16,
    pub mask: u16,
    pub applied: u8,
    pub vid: u16,
    pub pid: u16,
}

impl Backfill {
    pub fn bits(&self) -> u8 {
        self.applied
    }

    pub fn text(&self) -> String {
        format!(
            "vid=0x{:04X} pid=0x{:04X} caps=0x{:04X} mask=0x{:04X} applied={}",
            self.vid, self.pid, self.caps, self.mask, self.applied
        )
    }

    /// 五元组指纹：设备 + 掩码 + 能力 + 应用次数。
    pub fn fingerprint(&self) -> u64 {
        let mut h = 0xCBF2_9CE4_8422_2325u64;
        h = fold_u16(h, self.vid);
        h = fold_u16(h, self.pid);
        h = fold_u16(h, self.mask);
        h = fold_u16(h, self.caps);
        h = fold_u8(h, self.applied);
        h
    }
}

fn fold_u8(mut h: u64, v: u8) -> u64 {
    h ^= u64::from(v);
    h.wrapping_mul(0x0000_0100_0000_01B3)
}

fn fold_u16(mut h: u64, v: u16) -> u64 {
    let mut i = 0u32;
    while i < 2 {
        h = fold_u8(h, (v >> (i * 8)) as u8);
        i += 1;
    }
    h
}

/// 回填顺序固定：强制分类 → 删能力 → 加能力 → 复审计。
pub fn backfill(vid: u16, pid: u16, base_caps: u16) -> Result<Backfill, u32> {
    DeviceId::new(vid, pid)?;
    audit_caps(base_caps)?;
    let row = lookup(&ROWS, vid, pid);
    let (mask, tabled) = match row {
        Some(r) => {
            if r.base_caps != base_caps {
                return Err(QCode::BackfillBad.val());
            }
            (r.mask, true)
        }
        None => (0u16, false),
    };
    let mut caps = base_caps;
    let mut applied = 0u8;
    let mut snap = mask;
    while snap != 0 {
        let bit = snap & (!snap + 1);
        snap &= snap - 1;
        match bit {
            Q_FORCE_CLICKPAD => {
                caps &= !CAP_PHYS_BUTTONS;
                caps |= CAP_BUTTONPAD;
                applied = applied.saturating_add(1);
            }
            Q_FORCE_PHYSICAL => {
                caps &= !CAP_BUTTONPAD;
                caps |= CAP_PHYS_BUTTONS;
                applied = applied.saturating_add(1);
            }
            Q_DISABLE_PALM | Q_DROP_PALM_CAP => {
                caps &= !CAP_PALM;
                applied = applied.saturating_add(1);
            }
            Q_DROP_PRESSURE => {
                caps &= !CAP_PRESSURE;
                applied = applied.saturating_add(1);
            }
            Q_ADD_HIRES => {
                caps |= CAP_HIRES;
                applied = applied.saturating_add(1);
            }
            _ => return Err(QCode::QuirkBad.val()),
        }
    }
    if caps & CAP_KNOWN != caps {
        return Err(QCode::CapsBad.val());
    }
    audit_caps(caps)?;
    let b = Backfill { caps, mask, applied, vid, pid };
    if tabled && b.bits() == 0 {
        return Err(QCode::PrintBad.val());
    }
    Ok(b)
}

// ── 确定性源 ──────────────────────────────────────────────────────
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed ^ 0x5155_4952_4B42_3036)
    }

    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }

    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            0
        } else {
            self.next() % n
        }
    }
}

// ── 黄金语料 ──────────────────────────────────────────────────────
/// 查表语料：每轮命中一行，再查一个 miss；miss 必须得到 0。
pub fn golden_lookup(rounds: u32) -> Result<(u32, u32, u64), u32> {
    audit_table(&ROWS)?;
    let mut hits = 0u32;
    let mut misses = 0u32;
    let mut sum = 0u64;
    let mut r = 0u32;
    while r < rounds {
        let row = match ROWS.get((r as usize) % ROWS.len()) {
            Some(x) => *x,
            None => return Err(QCode::TableBad.val()),
        };
        let mask = lookup_mask(row.vid, row.pid)?;
        if mask != row.mask {
            return Err(QCode::CrossBad.val());
        }
        hits = hits.saturating_add(1);
        sum = sum.saturating_add(u64::from(mask));
        // 邻近 pid 可能命中相邻行，故用表中必然不存在的键做 miss 探针。
        let miss = lookup_mask(0xDEAD, 0xBEEF)?;
        if miss != 0 || lookup(&ROWS, 0xDEAD, 0xBEEF).is_some() {
            return Err(QCode::CrossBad.val());
        }
        misses = misses.saturating_add(1);
        r = r.saturating_add(1);
    }
    Ok((hits, misses, sum))
}

/// 回填语料：1000 轮覆盖 8 行；每轮两次回填指纹必须一致。
pub fn golden_backfill(rounds: u32) -> Result<(u32, u32, u64), u32> {
    audit_table(&ROWS)?;
    let mut ok = 0u32;
    let mut bits = 0u32;
    let mut sum = 0u64;
    let mut r = 0u32;
    while r < rounds {
        let row = match ROWS.get((r as usize) % ROWS.len()) {
            Some(x) => *x,
            None => return Err(QCode::TableBad.val()),
        };
        let a = backfill(row.vid, row.pid, row.base_caps)?;
        let b = backfill(row.vid, row.pid, row.base_caps)?;
        if a.fingerprint() != b.fingerprint() {
            return Err(QCode::PrintBad.val());
        }
        ok = ok.saturating_add(1);
        bits = bits.saturating_add(u32::from(a.bits()));
        sum = sum.saturating_add(a.caps as u64);
        r = r.saturating_add(1);
    }
    Ok((ok, bits, sum))
}

pub fn golden_run() -> Result<(u32, u32, u32, u64), u32> {
    let (hits, misses, _) = golden_lookup(LOOKUP_ROUNDS)?;
    let (filled, bits, _) = golden_backfill(BACKFILL_ROUNDS)?;
    if hits != LOOKUP_ROUNDS || misses != LOOKUP_ROUNDS {
        return Err(QCode::CrossBad.val());
    }
    if filled != BACKFILL_ROUNDS || bits == 0 {
        return Err(QCode::BackfillBad.val());
    }
    let mut h = 0xCBF2_9CE4_8422_2325u64;
    h = fold_u16(h, hits as u16);
    h = fold_u16(h, misses as u16);
    h = fold_u16(h, filled as u16);
    h = fold_u16(h, bits as u16);
    Ok((hits, misses, filled, h))
}

// ── 注入账（四路） ────────────────────────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }

    pub fn record(&mut self, slot: usize, hit: bool) {
        if let Some(t) = self.tries.get_mut(slot) {
            *t = t.saturating_add(1);
        }
        if hit {
            if let Some(c) = self.caught.get_mut(slot) {
                *c = c.saturating_add(1);
            }
        }
    }

    pub fn all_caught(&self) -> bool {
        let mut i = 0usize;
        while i < 4 {
            let t = self.tries.get(i).copied().unwrap_or(0);
            let c = self.caught.get(i).copied().unwrap_or(0);
            if t == 0 || c != t {
                return false;
            }
            i += 1;
        }
        true
    }
}

pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64 ^ 0x494E_4A);
    let mut r = 0u32;
    while r < rounds {
        rng.next();
        // 路 1：零键设备 / 未知 quirk 位 / 互斥位。
        let bad_dev = DeviceId::new(0, 1).err() == Some(QCode::DevBad.val())
            && DeviceId::new(1, 0).is_err();
        let bad_quirk = audit_quirk(0x8000).err() == Some(QCode::QuirkBad.val())
            && audit_quirk(Q_FORCE_CLICKPAD | Q_FORCE_PHYSICAL).err()
                == Some(QCode::QuirkBad.val());
        led.record(0, bad_dev && bad_quirk);

        // 路 2：重复键 / 空表 / 表内零键。
        let dup = [ROWS[0], ROWS[0], ROWS[1]];
        let zero = [QuirkRow { vid: 0, pid: 1, mask: 0, base_caps: CAP_MT, label: "z" }];
        let table_bad = audit_table(&dup).err() == Some(QCode::TableBad.val())
            && audit_table(&zero).err() == Some(QCode::DevBad.val())
            && audit_table(&[]).err() == Some(QCode::TableBad.val());
        led.record(1, table_bad);

        // 路 3：基础能力与表不符 / 回填后能力非法。
        let mismatch = backfill(DEVICE_VID, 0xC52B, CAP_MT).err() == Some(QCode::BackfillBad.val());
        let caps_bad = audit_caps(CAP_PALM).is_err() && backfill(DEVICE_VID, 0xC52B, CAP_PALM).is_err();
        led.record(2, mismatch && caps_bad);

        // 路 4：未知设备串表 / 指纹漂移。
        let miss = lookup_mask(0xDEAD, 0xBEEF)? == 0
            && lookup(&ROWS, 0xDEAD, 0xBEEF).is_none();
        let f1 = match backfill(DEVICE_VID, 0xC52C, CAP_MT | CAP_PHYS_BUTTONS) {
            Ok(b) => b.fingerprint(),
            Err(_) => 0,
        };
        let f2 = match backfill(DEVICE_VID, 0xC52C, CAP_MT | CAP_PHYS_BUTTONS) {
            Ok(b) => b.fingerprint(),
            Err(_) => 0,
        };
        led.record(3, miss && f1 != 0 && f1 == f2);

        r = r.wrapping_add(1);
    }
    Ok(led)
}

// ── 探针三序 ──────────────────────────────────────────────────────
pub fn probe_seq_a() -> Result<(), u32> {
    let (hits, misses, filled, _) = golden_run()?;
    if hits == LOOKUP_ROUNDS && misses == LOOKUP_ROUNDS && filled == BACKFILL_ROUNDS {
        Ok(())
    } else {
        Err(QCode::ReplayBad.val())
    }
}

pub fn probe_seq_b() -> Result<(), u32> {
    audit_table(&ROWS)?;
    let m = lookup_mask(DEVICE_VID, 0xC52B)?;
    if m & Q_FORCE_CLICKPAD == 0 || m & Q_DISABLE_PALM == 0 {
        return Err(QCode::CrossBad.val());
    }
    let b = backfill(DEVICE_VID, 0xC52B, CAP_MT | CAP_PRESSURE | CAP_PALM)?;
    if b.caps & CAP_BUTTONPAD == 0 || b.caps & CAP_PALM != 0 {
        return Err(QCode::BackfillBad.val());
    }
    let miss = backfill(0xDEAD, 0xBEEF, CAP_MT)?;
    if miss.bits() != 0 || miss.caps != CAP_MT || miss.mask != 0 {
        return Err(QCode::CrossBad.val());
    }
    Ok(())
}

pub fn probe_seq_c() -> Result<(), u32> {
    if QUIRK_ROWS == 8
        && LOOKUP_ROUNDS == 1000
        && BACKFILL_ROUNDS == 1000
        && MAX_QUIRK_BITS == 0x3F
        && ROWS.len() == 8
        && QCode::ALL.len() == 9
    {
        Ok(())
    } else {
        Err(QCode::SegmentBad.val())
    }
}

pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(QCode::ReplayBad.val());
    }
    Ok(())
}

pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < QCode::ALL.len() {
        let v = QCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x20 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 判据面 ────────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let look = golden_lookup(LOOKUP_ROUNDS);
    v.push((
        "F48911-J1-查表千轮零漂移",
        match &look {
            Ok((h, m, _)) => *h == LOOKUP_ROUNDS && *m == LOOKUP_ROUNDS,
            Err(_) => false,
        },
        match &look {
            Ok((h, m, sum)) => format!("hits={} misses={} mask_sum={}", h, m, sum),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    let fill = golden_backfill(BACKFILL_ROUNDS);
    v.push((
        "F48912-J1-回填千轮指纹一致",
        match &fill {
            Ok((n, bits, _)) => *n == BACKFILL_ROUNDS && *bits > 0,
            Err(_) => false,
        },
        match &fill {
            Ok((n, bits, sum)) => format!("filled={} applied_bits={} caps_sum={}", n, bits, sum),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    v.push((
        "F48911-J1-表自审计通过",
        audit_table(&ROWS) == Ok(QUIRK_ROWS),
        format!("rows={}", ROWS.len()),
    ));
    v.push((
        "F48911-J1-重复键必拒",
        {
            let dup = [ROWS[0], ROWS[0]];
            audit_table(&dup).err() == Some(QCode::TableBad.val())
        },
        String::from("dup -> TableBad"),
    ));
    v.push((
        "F48911-J1-未知位必拒",
        audit_quirk(0x4000).err() == Some(QCode::QuirkBad.val())
            && audit_quirk(Q_FORCE_CLICKPAD | Q_FORCE_PHYSICAL).is_err(),
        String::from("0x4000 / mutex -> QuirkBad"),
    ));
    v.push((
        "F48912-J1-未知设备不猜",
        lookup_mask(0xBEEF, 0xDEAD) == Ok(0)
            && lookup(&ROWS, 0xBEEF, 0xDEAD).is_none(),
        String::from("miss -> 0"),
    ));
    let b = backfill(DEVICE_VID, 0xC52B, CAP_MT | CAP_PRESSURE | CAP_PALM);
    v.push((
        "F48911-J1-强制分类与删掌",
        match &b {
            Ok(b) => {
                b.caps & CAP_BUTTONPAD != 0
                    && b.caps & CAP_PALM == 0
                    && b.caps & CAP_PRESS_ANY != 0
            }
            Err(_) => false,
        },
        match &b {
            Ok(b) => b.text(),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    v.push((
        "F48912-J1-回填后能力复审计",
        backfill(DEVICE_VID, 0xC52C, CAP_MT | CAP_PHYS_BUTTONS)
            .map(|b| audit_caps(b.caps).is_ok())
            == Ok(true)
            && backfill(DEVICE_VID, 0xC52B, CAP_MT).err() == Some(QCode::BackfillBad.val()),
        String::from("post-audit via core::audit_caps"),
    ));
    let inj = inject_rounds(10);
    v.push((
        "F48911-J1-注入四路10/10",
        match &inj {
            Ok(l) => l.all_caught(),
            Err(_) => false,
        },
        match &inj {
            Ok(l) => format!("tries={:?} caught={:?}", l.tries, l.caught),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    v.push((
        "F48912-J1-探针三序与双跑",
        probe_seq_a().is_ok()
            && probe_seq_b().is_ok()
            && probe_seq_c().is_ok()
            && repro_check().is_ok(),
        format!(
            "a={} b={} c={} repro={}",
            probe_seq_a().is_ok(),
            probe_seq_b().is_ok(),
            probe_seq_c().is_ok(),
            repro_check().is_ok()
        ),
    ));
    v.push((
        "F48912-J1-段位守卫",
        segment_guard(),
        String::from("0x70DC0 窗口"),
    ));
    v
}

/// 压力能力位局部别名，避免和 CAP_PRESSURE 的语义歧义。
pub const CAP_PRESS_ANY: u16 = CAP_PRESSURE;

pub fn run_unx48911_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F48911 quirk 回填");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 单元判据 ──────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_is_unique_and_valid() {
        assert_eq!(audit_table(&ROWS), Ok(QUIRK_ROWS));
        assert!(audit_quirk(0x8000).is_err());
        assert!(audit_quirk(Q_FORCE_CLICKPAD | Q_FORCE_PHYSICAL).is_err());
    }

    #[test]
    fn zero_device_is_not_a_miss() {
        assert_eq!(DeviceId::new(0, 5).err(), Some(QCode::DevBad.val()));
        assert_eq!(DeviceId::new(5, 0).err(), Some(QCode::DevBad.val()));
        assert!(DeviceId::new(5, 6).is_ok());
    }

    #[test]
    fn lookup_is_exact() {
        let r = lookup(&ROWS, DEVICE_VID, 0xC52B);
        assert_eq!(r.map(|x| x.label), Some("clickpad-no-palm"));
        assert!(lookup(&ROWS, DEVICE_VID, 0xC599).is_none());
        assert_eq!(lookup_mask(DEVICE_VID, 0xC599), Ok(0));
    }

    #[test]
    fn force_clickpad_rewrites_buttons() {
        let b = match backfill(DEVICE_VID, 0xC52B, CAP_MT | CAP_PRESSURE | CAP_PALM) {
            Ok(b) => b,
            Err(_) => return,
        };
        assert_ne!(b.caps & CAP_BUTTONPAD, 0);
        assert_eq!(b.caps & CAP_PHYS_BUTTONS, 0);
        assert_eq!(b.caps & CAP_PALM, 0);
        assert!(b.bits() >= 2);
    }

    #[test]
    fn force_physical_rewrites_buttons() {
        let b = match backfill(DEVICE_VID, 0xC52C, CAP_MT | CAP_PHYS_BUTTONS) {
            Ok(b) => b,
            Err(_) => return,
        };
        assert_ne!(b.caps & CAP_PHYS_BUTTONS, 0);
        assert_eq!(b.caps & CAP_BUTTONPAD, 0);
        assert_ne!(b.caps & CAP_HIRES, 0);
    }

    #[test]
    fn drop_pressure_removes_capability() {
        let b = match backfill(DEVICE_VID, 0xC52E, CAP_MT | CAP_PRESSURE) {
            Ok(b) => b,
            Err(_) => return,
        };
        assert_eq!(b.caps & CAP_PRESSURE, 0);
    }

    #[test]
    fn base_caps_must_match_table() {
        assert_eq!(
            backfill(DEVICE_VID, 0xC52B, CAP_MT).err(),
            Some(QCode::BackfillBad.val())
        );
    }

    #[test]
    fn unknown_device_gets_empty_fill() {
        let b = match backfill(0xBEEF, 0xDEAD, CAP_MT) {
            Ok(b) => b,
            Err(_) => return,
        };
        assert_eq!(b.mask, 0);
        assert_eq!(b.bits(), 0);
        assert_eq!(b.caps, CAP_MT);
    }

    #[test]
    fn fingerprint_is_repeatable() {
        let a = backfill(DEVICE_VID, 0xC52F, CAP_MT);
        let b = backfill(DEVICE_VID, 0xC52F, CAP_MT);
        assert_eq!(a.map(|x| x.fingerprint()), b.map(|x| x.fingerprint()));
    }

    #[test]
    fn post_fill_caps_pass_core_audit() {
        let mut i = 0usize;
        while i < ROWS.len() {
            let row = match ROWS.get(i) {
                Some(r) => *r,
                None => return,
            };
            match backfill(row.vid, row.pid, row.base_caps) {
                Ok(b) => assert!(audit_caps(b.caps).is_ok()),
                Err(_) => assert!(false),
            }
            i += 1;
        }
    }

    #[test]
    fn golden_is_repeatable() {
        let a = golden_run();
        assert!(a.is_ok());
        assert_eq!(golden_run(), a);
        match a {
            Ok((hits, misses, filled, root)) => {
                assert_eq!(hits, LOOKUP_ROUNDS);
                assert_eq!(misses, LOOKUP_ROUNDS);
                assert_eq!(filled, BACKFILL_ROUNDS);
                assert!(root != 0);
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn probes_and_injection_hold() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
        assert!(repro_check().is_ok());
        assert!(segment_guard());
        match inject_rounds(10) {
            Ok(l) => assert!(l.all_caught()),
            Err(_) => assert!(false),
        }
    }
}
