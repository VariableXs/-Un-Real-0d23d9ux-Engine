//! unx0458_acpize —— UNX-F0458 ACPI 账对冲总账。
//! 锚点（CoRun Unxreal 主册）：UNX-F0458 · ACPI 账对冲总账 · 目标 250 行
//! 判据原文：UNX-F0458-J1 前序各单产出的账互相对冲（三账
//!           齐平）：快照账（0451 快照行数）≡ 封账账（0453
//!           封存条目数）≡ 交接账（0456 信包表数）；任何一
//!           处不一致 → 对冲立案（列出差值方向）；账二 对
//!           冲维度：数目维（三账行数）、签名维（快照签名
//!           集合 ≡ 封账签名集合，允许多实例展开）、尺寸维
//!           （封账 len 合计 ≡ 五数账 max 上界合理性）；账
//!           三 对冲纪律：对冲是只读行为（不修账、不补账、
//!           不删账——只立案）；账四 分差归因：差值带方向
//!           （快照多/封账多/信包多），供人工追因；账五 幂
//!           等：同输入两次对冲结论一致。
//! 组件：三账对冲器 + 方向归因器 + 签名集合展开器 + 幂等核。
//! 职责定位：对冲总账是 ACPI 世界的「资产负债表平账」——快
//!   照说有 7 张表、封账只封了 6 张、信包报 7 张，中间必有
//!   一张表「失踪」；对冲器的工作就是让失踪无法藏身。
//! 性能逐项分解：对冲 O(n²)（n ≤ 32）；展开 O(n)。
//! 诊断码段：0x343C0（0x343C1~0x343C9 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use alloc::vec;
use crate::checks::CheckSet;

/// 诊断码段位（u32 高段独占 0x343C0 起）。
pub const SEG_BASE: u32 = 0x343C0;
pub const XC_CNT_DIR: u32 = SEG_BASE + 1; // 数目维不齐
pub const XC_SIGN_DIR: u32 = SEG_BASE + 2; // 签名维不齐
pub const XC_SIZE_DIR: u32 = SEG_BASE + 3; // 尺寸维不齐

/// 对冲方向。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dir {
    SnapshotMore,
    PinMore,
    HandoffMore,
    Aligned,
}

/// 对冲输入投影（三方账的纯数据截面）。
#[derive(Clone, Debug, Default)]
pub struct Hedge {
    /// 快照账：签名行。
    pub snap_signs: Vec<[u8; 4]>,
    /// 封账账：（签名、长度）行。
    pub pin_rows: Vec<([u8; 4], u32)>,
    /// 交接账：信包表数。
    pub handoff_count: usize,
}

impl Hedge {
    /// 数目维对冲。
    pub fn count_dir(&self) -> Dir {
        let s = self.snap_signs.len();
        let p = self.pin_rows.len();
        let h = self.handoff_count;
        if s == p && p == h {
            Dir::Aligned
        } else if s > p && s >= h {
            Dir::SnapshotMore
        } else if p > h {
            Dir::PinMore
        } else {
            Dir::HandoffMore
        }
    }

    /// 数目维对冲核。
    pub fn count_ok(&self) -> Result<(), u32> {
        match self.count_dir() {
            Dir::Aligned => Ok(()),
            _ => Err(XC_CNT_DIR),
        }
    }

    /// 签名集合展开（多实例展开为多重集）后对冲。
    pub fn signs_ok(&self) -> Result<(), u32> {
        let mut snap = self.snap_signs.clone();
        snap.sort();
        let mut pins: Vec<[u8; 4]> = self.pin_rows.iter().map(|(s, _)| *s).collect();
        pins.sort();
        if snap != pins {
            return Err(XC_SIGN_DIR);
        }
        Ok(())
    }

    /// 尺寸维对冲：封账 len 合计 ≤ 16MB 表区上界（合理性核）。
    pub fn size_ok(&self, cap: u64) -> Result<(), u32> {
        let total: u64 = self.pin_rows.iter().map(|(_, l)| *l as u64).sum();
        if total > cap {
            return Err(XC_SIZE_DIR);
        }
        Ok(())
    }
}

/// 三账齐平总核：三维度全过才放行。
pub fn hedge_all(h: &Hedge, cap: u64) -> Result<(), u32> {
    h.count_ok()?;
    h.signs_ok()?;
    h.size_ok(cap)
}

/// 方向归因文本（立案明细）。
pub fn dir_text(d: Dir) -> &'static str {
    match d {
        Dir::SnapshotMore => "快照多（封账漏封）",
        Dir::PinMore => "封账多（快照漏行）",
        Dir::HandoffMore => "信包多（交接虚报）",
        Dir::Aligned => "三账齐平",
    }
}

// ─────────────────────────── 判据面 ───────────────────────────

/// 判据入口：UNX-F0458 全族核。
pub fn run_unx0458_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0458");

    // 夹具：三方齐平（5 表）。
    let h_ok = Hedge {
        snap_signs: vec![*b"FACP", *b"APIC", *b"HPET", *b"MCFG", *b"DSDT"],
        pin_rows: vec![
            (*b"FACP", 148),
            (*b"APIC", 94),
            (*b"HPET", 56),
            (*b"MCFG", 60),
            (*b"DSDT", 10_000),
        ],
        handoff_count: 5,
    };

    // 族一：齐平三维度。
    cs.add("数目齐平", h_ok.count_ok().is_ok(), "5=5=5");
    cs.add("签名齐平", h_ok.signs_ok().is_ok(), "多重集等");
    cs.add("尺寸合理", h_ok.size_ok(16 * 1024 * 1024).is_ok(), "≤16MB");
    cs.add("总核放行", hedge_all(&h_ok, 16 * 1024 * 1024).is_ok(), "三账平");
    cs.add("方向 Aligned", h_ok.count_dir() == Dir::Aligned, "齐向");

    // 族二：快照多（封账漏封）。
    let h1 = Hedge { snap_signs: h_ok.snap_signs.clone(), pin_rows: h_ok.pin_rows[..4].to_vec(), handoff_count: 5 };
    cs.add("快照多立案", h1.count_ok() == Err(XC_CNT_DIR), "4<5");
    cs.add("归因快照向", h1.count_dir() == Dir::SnapshotMore, "漏封");
    cs.add("归因文本", dir_text(h1.count_dir()) == "快照多（封账漏封）", "明细");

    // 族三：封账多（快照漏行）。
    let h2 = Hedge { snap_signs: h_ok.snap_signs[..4].to_vec(), pin_rows: h_ok.pin_rows.clone(), handoff_count: 5 };
    cs.add("封账多立案", h2.count_ok() == Err(XC_CNT_DIR), "5>4");
    cs.add("归因封账向", h2.count_dir() == Dir::PinMore, "漏行");

    // 族四：信包虚报。
    let h3 = Hedge { snap_signs: h_ok.snap_signs.clone(), pin_rows: h_ok.pin_rows.clone(), handoff_count: 7 };
    cs.add("信包多立案", h3.count_ok() == Err(XC_CNT_DIR), "7>5");
    cs.add("归因信包向", h3.count_dir() == Dir::HandoffMore, "虚报");

    // 族五：签名维（同数不同名）。
    let h4 = Hedge {
        snap_signs: vec![*b"FACP", *b"APIC", *b"HPET", *b"MCFG", *b"XXXX"],
        pin_rows: h_ok.pin_rows.clone(),
        handoff_count: 5,
    };
    cs.add("签名维立案", h4.signs_ok() == Err(XC_SIGN_DIR), "同名数异");
    cs.add("签名维总核拒", hedge_all(&h4, 16 * 1024 * 1024) == Err(XC_SIGN_DIR), "数目过但签名破");

    // 族六：多实例展开对冲（APIC 双例）。
    let h5 = Hedge {
        snap_signs: vec![*b"FACP", *b"APIC", *b"APIC"],
        pin_rows: vec![(*b"FACP", 148), (*b"APIC", 94), (*b"APIC", 94)],
        handoff_count: 3,
    };
    cs.add("双例齐平", hedge_all(&h5, 16 * 1024 * 1024).is_ok(), "多重集等");

    // 族七：尺寸维。
    let h6 = Hedge {
        snap_signs: vec![*b"DSDT"],
        pin_rows: vec![(*b"DSDT", 32 * 1024 * 1024)],
        handoff_count: 1,
    };
    cs.add("尺寸超限立案", h6.size_ok(16 * 1024 * 1024) == Err(XC_SIZE_DIR), ">16MB");
    cs.add("尺寸总核拒", hedge_all(&h6, 16 * 1024 * 1024) == Err(XC_SIZE_DIR), "数目签名过尺寸破");

    // 族八：幂等双拍。
    let r1 = hedge_all(&h_ok, 16 * 1024 * 1024);
    let r2 = hedge_all(&h_ok, 16 * 1024 * 1024);
    cs.add("对冲幂等", r1 == r2, "结论稳定");

    // 族九：空账零表齐平。
    let h0 = Hedge::default();
    cs.add("空账齐平", hedge_all(&h0, 16 * 1024 * 1024).is_ok(), "0=0=0");
    cs.add("空账方向", h0.count_dir() == Dir::Aligned, "零向");

    cs
}
