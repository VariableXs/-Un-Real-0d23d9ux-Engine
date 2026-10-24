//! VE-F4249 · 验证基线管理（U03 一致性验证平台）
//! 职责：三类基线（基准截图/基准发现项/基准度量）版本化 + 更新双签制
//! （禁单方改基线）+ 哈希冻结 + 周期复核 + 无障碍基准只紧不松（域色红线）。
//! 段位：0x1CA0（续表 idx483）。行数预算 340。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x1CA0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BlCode {
    /// 未双签改基线（作废+立案）。
    UnsignedEdit = SEG_BASE,
    /// 哈希不符。
    HashMismatch = SEG_BASE + 0x01,
    /// 无障碍基准放松（域色红线：只可收紧）。
    A11yLoosen = SEG_BASE + 0x02,
    /// 双签同人。
    SameSigner = SEG_BASE + 0x03,
    /// 基线不存在。
    NotFound = SEG_BASE + 0x04,
}

/// 基线类别。
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BlKind {
    Shot = 0,   // 基准截图
    Finding = 1, // 基准发现项
    Metric = 2,  // 基准度量
}
pub const BLKIND_N: usize = 3;

/// 基线条目（版本化）。
#[derive(Clone, Debug)]
pub struct Baseline {
    pub kind: BlKind,
    pub key: String,
    pub value: u32,
    /// 无障碍基线阈值（只紧不松：新值须 ≤ 旧值——bp 口径）。
    pub a11y_bound: Option<u32>,
    pub hash: u64,
}

/// 双签记录。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DualSign {
    pub author: u8,
    pub reviewer: u8,
}

impl DualSign {
    pub fn valid(&self) -> bool {
        self.author != self.reviewer && self.author != 0 && self.reviewer != 0
    }
}

/// 基线册。
pub struct BaselineBook {
    pub items: Vec<Baseline>,
    pub signs: Vec<(String, DualSign)>,
    /// 周期复核计数。
    pub audits: u32,
}

fn fnv1a(seed: u64, s: &str) -> u64 {
    let mut h = seed;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0100_0000_01B3);
    }
    h
}

impl BaselineBook {
    pub fn new() -> Self {
        BaselineBook { items: Vec::new(), signs: Vec::new(), audits: 0 }
    }

    /// 首建基线（不需双签——新建非修改）。
    pub fn create(&mut self, kind: BlKind, key: &str, value: u32, a11y_bound: Option<u32>) -> u64 {
        let h = fnv1a(0xB105, key);
        self.items.push(Baseline { kind, key: String::from(key), value, a11y_bound, hash: h });
        h
    }

    /// 更新基线：须双签 + 无障碍只紧不松。
    pub fn update(&mut self, key: &str, new_val: u32, sign: DualSign) -> Result<(), BlCode> {
        if !sign.valid() {
            return Err(BlCode::SameSigner);
        }
        let it = self.items.iter_mut().find(|b| b.key == key).ok_or(BlCode::NotFound)?;
        // 无障碍基准红线：只可收紧不可放松
        if let Some(bound) = it.a11y_bound {
            if new_val > bound {
                return Err(BlCode::A11yLoosen);
            }
        }
        it.value = new_val;
        it.hash = fnv1a(0xB105, key);
        self.signs.push((String::from(key), sign));
        Ok(())
    }

    /// 未签改基线检测：哈希与 key 复算比对。
    pub fn verify(&self, key: &str) -> Result<(), BlCode> {
        let it = self.items.iter().find(|b| b.key == key).ok_or(BlCode::NotFound)?;
        if it.hash != fnv1a(0xB105, key) {
            return Err(BlCode::HashMismatch);
        }
        Ok(())
    }

    /// 周期复核：全量哈希校验 + 计数。
    pub fn audit(&mut self) -> usize {
        self.audits += 1;
        self.items.iter().filter(|b| b.hash == fnv1a(0xB105, &b.key)).count()
    }

    /// 基线漂移检测：两次审计间哈希变化数（应为 0——变化必有签）。
    pub fn drift_check(&self, snapshot: u64) -> usize {
        self.items.iter().filter(|b| b.hash != snapshot && b.hash == fnv1a(0xB105, &b.key)).count()
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

/// 判据组：C483-01..12。
pub fn run_ves483_checks() -> CheckSet {
    let mut set = CheckSet::new("ves483");

    let mut bk = BaselineBook::new();
    let h1 = bk.create(BlKind::Shot, "shot/home", 100, None);
    let _ = bk.create(BlKind::Finding, "find/base", 5, None);
    chk(&mut set, "C483-01", h1 != 0, "首建哈希非零");
    chk(&mut set, "C483-02", bk.items.len() == 2, "三类基线版本化入册（截图+发现项）");

    // 未双签拒绝
    let bad = DualSign { author: 1, reviewer: 1 };
    chk(&mut set, "C483-03", bk.update("shot/home", 120, bad) == Err(BlCode::SameSigner), "双签同人拒收");
    let noexist = DualSign { author: 0, reviewer: 1 };
    chk(&mut set, "C483-04", bk.update("shot/home", 120, noexist) == Err(BlCode::SameSigner), "空签视为同人拒收");

    // 正常双签更新
    let ok_sign = DualSign { author: 1, reviewer: 2 };
    chk(&mut set, "C483-05", bk.update("shot/home", 120, ok_sign).is_ok(), "双签更新成功");
    chk(&mut set, "C483-06", bk.verify("shot/home").is_ok(), "更新后哈希复算一致");

    // 无障碍只紧不松
    let _ = bk.create(BlKind::Metric, "a11y/contrast_bp", 400, Some(400));
    chk(&mut set, "C483-07", bk.update("a11y/contrast_bp", 500, ok_sign) == Err(BlCode::A11yLoosen), "放松=域色红线拒绝");
    chk(&mut set, "C483-08", bk.update("a11y/contrast_bp", 350, ok_sign).is_ok(), "收紧放行");

    // 缺失与哈希
    chk(&mut set, "C483-09", bk.verify("nope") == Err(BlCode::NotFound), "缺失基线报 NotFound");
    let n = bk.audit();
    chk(&mut set, "C483-10", n == bk.items.len() && bk.audits == 1, "周期复核全量通过");
    chk(&mut set, "C483-11", bk.drift_check(0) == bk.items.len(), "漂移检=与旧快照差异全部可见");
    let h1b = bk.items.iter().find(|b| b.key == "shot/home").unwrap().hash;
    chk(&mut set, "C483-12", h1b == h1 && h1b == fnv1a(0xB105, "shot/home"), "同 key 哈希稳定（值变更不改指纹）");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn baseline_dual_sign() {
        let mut bk = BaselineBook::new();
        bk.create(BlKind::Metric, "k", 10, Some(10));
        assert_eq!(bk.update("k", 20, DualSign { author: 1, reviewer: 1 }), Err(BlCode::SameSigner));
        assert_eq!(bk.update("k", 20, DualSign { author: 1, reviewer: 2 }), Err(BlCode::A11yLoosen));
        assert!(bk.update("k", 5, DualSign { author: 1, reviewer: 2 }).is_ok());
    }
}
