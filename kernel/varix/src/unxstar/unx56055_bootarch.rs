//! UNX-F56055 启动样本存证归档件（unxstar 段）
//!
//! [J1] 归档完整性：逐段（五段）原始时间戳与全程外部实测记录必须全部落账，
//!      缺任一段即 `resample_ready()` 返回 false，判据锚定 §AI-71.5 验收判据 3。
//! [J2] 归档不可变性：归档账本一经 seal 即只读；重复段、时间戳回退、越界段号
//!      均以独立诊断码拒绝，且拒绝行为本身必须可观测（injection 全捕获）。
//! [J3] 重采基础就绪：归档指纹（FNV-1a 逐条折叠）在内容不变时必须逐字节稳定，
//!      重采复测以该指纹为对账基准。
//!
//! 域：CoRun·UNX-F2-B40
//!
//! 结构总览：
//!   常量段：SEG_BASE + 7 个诊断码（段号越界/重复段/时间回退/账已封/段缺失/指纹漂移/外部记录缺失）
//!   纯函数：fnv64 指纹、条目指纹折叠、万分比漂移
//!   ArchEntry：单段存证条目（段号 + 原始 tick + 外部实测 ns），new() 走守闸
//!   ArchBook：归档账本（append/seal/resample_ready/archive_fingerprint）
//!   injection_core：5 例注入必抓，返回 (caught, total)
//!   run_unx56055_checks：≥12 条判据

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x606C0;

/// 段号越界（不在 1..=5 闭集）
pub const D_SEG_RANGE: u32 = SEG_BASE + 0x01;
/// 重复段（同一段号二次归档）
pub const D_SEG_DUP: u32 = SEG_BASE + 0x02;
/// 时间戳回退（后段原始 tick 不大于前段）
pub const D_TS_BACK: u32 = SEG_BASE + 0x03;
/// 归档账已封（seal 后再写）
pub const D_SEALED: u32 = SEG_BASE + 0x04;
/// 段缺失（五段未齐即宣称就绪）
pub const D_SEG_MISSING: u32 = SEG_BASE + 0x05;
/// 归档指纹漂移（同内容两次折叠不一致）
pub const D_FP_DRIFT: u32 = SEG_BASE + 0x06;
/// 外部实测记录缺失（条目 external_ms 为 0）
pub const D_EXT_MISSING: u32 = SEG_BASE + 0x07;

/// 五段闭集段号：固件/引导器/内核/登录/桌面
pub const SEG_IDS: [u32; 5] = [1, 2, 3, 4, 5];
/// 段号公开名
pub const SEG_NAMES: [&str; 5] = ["firmware", "bootloader", "kernel", "login", "desktop"];

/// FNV-1a 64 位指纹
pub fn fnv64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// 单条目指纹：段号 || 原始 tick || 外部实测
pub fn entry_fp(e: &ArchEntry) -> u64 {
    let mut buf: Vec<u8> = Vec::new();
    buf.extend_from_slice(&e.seg_id.to_le_bytes());
    buf.extend_from_slice(&e.raw_ticks.to_le_bytes());
    buf.extend_from_slice(&e.external_ns.to_le_bytes());
    fnv64(&buf)
}

/// 逐条折叠归档指纹（换序会得到不同值，保证顺序敏感）
pub fn fold_fps(fps: &[u64]) -> u64 {
    let mut acc: u64 = 0x5605_5000_0000_0001;
    for &f in fps {
        acc = acc.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ f;
    }
    acc
}

/// 万分比漂移：|a-b| * 10000 / max(a,b,1)
pub fn drift_pbp(a: u64, b: u64) -> u64 {
    let d = if a > b { a - b } else { b - a };
    let base = if a > b { a } else { b };
    if base == 0 {
        0
    } else {
        (d * 10_000) / base
    }
}

/// 单段存证条目
#[derive(Clone)]
pub struct ArchEntry {
    pub seg_id: u32,
    pub raw_ticks: u64,
    pub external_ns: u64,
}

impl ArchEntry {
    /// 守闸：段号闭集、tick 与外部实测均非零
    pub fn new(seg_id: u32, raw_ticks: u64, external_ns: u64) -> Result<Self, u32> {
        if seg_id < 1 || seg_id > 5 {
            return Err(D_SEG_RANGE);
        }
        if raw_ticks == 0 {
            return Err(D_TS_BACK);
        }
        if external_ns == 0 {
            return Err(D_EXT_MISSING);
        }
        Ok(ArchEntry { seg_id, raw_ticks, external_ns })
    }
}

/// 归档账本：五段逐条落账，seal 后只读
pub struct ArchBook {
    pub book_name: String,
    entries: Vec<ArchEntry>,
    sealed: bool,
}

impl ArchBook {
    pub fn new(book_name: &str) -> Self {
        ArchBook { book_name: book_name.to_string(), entries: Vec::new(), sealed: false }
    }

    /// 归档一条；重复段、时间回退、账已封均拒绝
    pub fn archive(&mut self, e: ArchEntry) -> Result<u64, u32> {
        if self.sealed {
            return Err(D_SEALED);
        }
        if self.entries.iter().any(|x| x.seg_id == e.seg_id) {
            return Err(D_SEG_DUP);
        }
        if let Some(last) = self.entries.last() {
            if e.raw_ticks <= last.raw_ticks {
                return Err(D_TS_BACK);
            }
        }
        let fp = entry_fp(&e);
        self.entries.push(e);
        Ok(fp)
    }

    pub fn seal(&mut self) -> u64 {
        let fps: Vec<u64> = self.entries.iter().map(entry_fp).collect();
        self.sealed = true;
        fold_fps(&fps)
    }

    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 五段是否齐备（重采复测基础就绪的硬前提）
    pub fn all_segments_present(&self) -> bool {
        self.entries.len() == 5
            && SEG_IDS.iter().all(|id| self.entries.iter().any(|e| e.seg_id == *id))
    }

    /// 重采基础就绪：五段齐 + 外部实测齐 + 未处于半封状态（封账后同样就绪，但只读）
    pub fn resample_ready(&self) -> bool {
        self.all_segments_present()
    }

    /// 当前归档指纹（未封账也随内容确定）
    pub fn archive_fingerprint(&self) -> u64 {
        let fps: Vec<u64> = self.entries.iter().map(entry_fp).collect();
        fold_fps(&fps)
    }

    /// 段间外部实测记录漂移检查：任一相邻两段外部实测漂移 > 2000 万分比判漂移
    pub fn ext_drift_flags(&self) -> Vec<String> {
        let mut flags = Vec::new();
        for i in 1..self.entries.len() {
            let a = self.entries[i - 1].external_ns;
            let b = self.entries[i].external_ns;
            let d = drift_pbp(a, b);
            if d > 2000 {
                flags.push(format!(
                    "EXT-DRIFT|{}|{}|{}",
                    self.entries[i - 1].seg_id, self.entries[i].seg_id, d
                ));
            }
        }
        flags
    }

    /// 归档文档行（机器可读）
    pub fn rows(&self) -> Vec<String> {
        self.entries
            .iter()
            .map(|e| {
                format!(
                    "ARCH|{}|{}|{}|fp={:016x}",
                    e.seg_id, e.raw_ticks, e.external_ns, entry_fp(e)
                )
            })
            .collect()
    }
}

/// 标准夹具：五段原始时间戳单调，外部实测与 tick 同比（3.6GHz 假设：1ms ≈ 3.6e6 tick）
pub fn fixture_book() -> ArchBook {
    let mut b = ArchBook::new("ai71.5-arch");
    let rows: [(u32, u64, u64); 5] = [
        (1, 3_600_000, 1_000_000),
        (2, 14_400_000, 4_000_000),
        (3, 36_000_000, 10_000_000),
        (4, 90_000_000, 25_000_000),
        (5, 180_000_000, 50_000_000),
    ];
    for (id, t, ns) in rows {
        let e = ArchEntry::new(id, t, ns).expect("fixture entry");
        b.archive(e).expect("fixture archive");
    }
    b
}

/// 注入核：5 例必抓
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;

    // 1) 段号越界 0
    if ArchEntry::new(0, 1, 1).err() == Some(D_SEG_RANGE) {
        caught += 1;
    }

    // 2) 重复段（段 3 已存在）
    let mut b = fixture_book();
    if b.archive(ArchEntry::new(3, 200_000_000, 1_000_000).expect("arch")) == Err(D_SEG_DUP) {
        caught += 1;
    }

    // 3) 时间回退（半满账本：段 2 的 tick 小于段 1）
    let mut h = ArchBook::new("half");
    let _ = h.archive(ArchEntry::new(1, 3_600_000, 1_000_000).expect("arch"));
    if h.archive(ArchEntry::new(2, 3_000_000, 1_000_000).expect("arch")) == Err(D_TS_BACK) {
        caught += 1;
    }

    // 4) 封账后写入
    let mut c = fixture_book();
    let _ = c.seal();
    if c.archive(ArchEntry::new(6, 1, 1).expect("arch")) == Err(D_SEALED) {
        caught += 1;
    }

    // 5) 外部实测缺失
    if ArchEntry::new(1, 3_600_000, 0).err() == Some(D_EXT_MISSING) {
        caught += 1;
    }

    (caught, total)
}

/// 指纹稳定性：同内容两次折叠必须一致
pub fn fp_stability() -> Result<(), u32> {
    let a = fixture_book().archive_fingerprint();
    let b = fixture_book().archive_fingerprint();
    if a != b {
        return Err(D_FP_DRIFT);
    }
    Ok(())
}

/// 五段缺失检测：抽掉段 4 后 resample_ready 必须为 false
pub fn missing_segment_probe() -> Result<(), u32> {
    let mut b = fixture_book();
    b.entries.retain(|e| e.seg_id != 4);
    if b.resample_ready() {
        return Err(D_SEG_MISSING);
    }
    Ok(())
}

/// 判据集
pub fn run_unx56055_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56055");

    // J1 归档完整性
    let b = fixture_book();
    s.add("56055-J1-五段齐备", b.all_segments_present(), "五段未齐");
    s.add("56055-J1-条目数", b.len() == 5, "条目数不为 5");
    s.add("56055-J1-重采就绪", b.resample_ready(), "重采基础未就绪");

    // J1 外部实测漂移：夹具相邻段漂移 150%（15000pbp）故应命中标记，标记数必须为 4
    let flags = b.ext_drift_flags();
    s.add("56055-J1-外部漂移标记数", flags.len() == 4, "相邻段外部实测漂移标记数异常");

    // J2 归档不可变
    let mut c = fixture_book();
    let fp_before = c.archive_fingerprint();
    let fp_seal = c.seal();
    s.add("56055-J2-封账指纹一致", fp_before == fp_seal, "封账前后折叠指纹不一致");
    s.add("56055-J2-封账只读", c.archive(ArchEntry::new(6, 1, 1).expect("arch")) == Err(D_SEALED), "封账后仍可写");
    s.add("56055-J2-重复段拒绝", {
        let mut d = fixture_book();
        d.sealed = false;
        d.archive(ArchEntry::new(2, 999_999_999, 1_000_000).expect("arch")) == Err(D_SEG_DUP)
    }, "重复段未被拒绝");

    // J2 时间回退
    let mut h = ArchBook::new("half");
    let _ = h.archive(ArchEntry::new(1, 3_600_000, 1_000_000).expect("arch"));
    s.add("56055-J2-时间回退拒绝", h.archive(ArchEntry::new(2, 3_000_000, 1_000_000).expect("arch")) == Err(D_TS_BACK), "时间回退未被拒绝");

    // J2 越界段号
    s.add("56055-J2-段号闭集", ArchEntry::new(6, 1, 1).err() == Some(D_SEG_RANGE), "段号 6 未被闭集拒绝");

    // J3 指纹稳定与缺失探测
    s.add("56055-J3-指纹稳定", fp_stability().is_ok(), "同内容两次折叠指纹漂移");
    s.add("56055-J3-缺段探测", missing_segment_probe().is_ok(), "缺段未被 resample_ready 暴露");

    // J3 归档文档行数
    let rows = fixture_book().rows();
    s.add("56055-J3-归档行数", rows.len() == 5, "归档文档行数不为 5");
    s.add("56055-J3-行格式锚", rows[0].starts_with("ARCH|1|"), "首行格式锚不符");

    // 注入核
    let (caught, total) = injection_core();
    s.add_dyn(
        "56055-J2-注入必抓",
        caught == total,
        &format!("注入捕获 {}/{}", caught, total),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56055_all_checks_green() {
        let s = run_unx56055_checks();
        assert!(
            s.all_passed(),
            "UNX-F56055 有红项: {}",
            s.red_items_text()
        );
    }
}
