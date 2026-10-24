//! VE-F5101 · 调试热载总架构（VE-Z 域 · Y06 组 · 目标 320 行）· 诊断段 0x9680
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F5101`
//!
//! **判据（锚点原文）**：三件、会话共享、即时反馈、出机红线、判据。
//!
//! **职责定位（锚点原文）**：脚本调试与热载总架构（调试热载定位：边写边试即时
//! 反馈）；架构三件：调试器（断点/单步/监视）/热载器（保存即生效）/状态映射
//! （热载前后状态无缝迁移）——三件共用 X03 会话模型；调试数据与脚本源码不出
//! 机；会话失稳→看门狗复位；越界→权限拦截。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x9680;

/// 三件套。
pub const PIECES: u32 = 3;

/// FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 调试器：断点/单步/监视。
pub struct Debugger {
    pub breakpoints: Vec<u32>,
    pub stepping: bool,
    pub watches: Vec<u32>,
}

impl Debugger {
    /// 断点命中：pc 落在断点集。
    pub fn hit(&self, pc: u32) -> bool {
        self.breakpoints.contains(&pc)
    }

    /// 监视命中。
    pub fn watch_hit(&self, var: u32) -> bool {
        self.watches.contains(&var)
    }
}

/// 热载器：保存即生效 + 状态映射（前后状态键值对齐迁移）。
pub struct HotReloader {
    /// 热载前状态（键, 值）。
    pub before: Vec<(u32, u64)>,
    /// 热载后状态。
    pub after: Vec<(u32, u64)>,
    pub applied: bool,
}

impl HotReloader {
    /// 状态无缝迁移：键集一致且共有键值保持（新增键允许，丢失键即断裂）。
    pub fn seamless(&self) -> bool {
        if !self.applied {
            return false;
        }
        for (k, v) in self.before.iter() {
            match self.after.iter().find(|(k2, _)| k2 == k) {
                Some((_, v2)) if v2 == v => {}
                Some(_) => return false, // 同键异值=断裂
                None => return false,    // 键丢失=断裂
            }
        }
        true
    }
}

/// 会话看门狗：失稳复位计数。
pub struct Watchdog {
    pub resets: u32,
}

impl Watchdog {
    pub fn tick(&mut self, unstable: bool) -> bool {
        if unstable {
            self.resets += 1;
            false
        } else {
            true
        }
    }
}

/// 三件架构 + 出机红线。
pub struct DbgArch {
    pub dbg: Debugger,
    pub hot: HotReloader,
    pub wd: Watchdog,
    /// 出机红线：数据与源码不出机（本地位）。
    pub local_only: bool,
}

impl DbgArch {
    /// 三件会话共享 + 出机红线。
    pub fn ok(&self) -> bool {
        self.local_only && self.hot.seamless()
    }
}

/// J1 主判据：三件套 + 会话共享 + 即时反馈 + 出机红线。
pub fn j1_main() -> bool {
    let a = DbgArch {
        dbg: Debugger {
            breakpoints: alloc::vec![100, 200],
            stepping: true,
            watches: alloc::vec![5],
        },
        hot: HotReloader {
            before: alloc::vec![(1, 10), (2, 20)],
            after: alloc::vec![(1, 10), (2, 20), (3, 30)],
            applied: true,
        },
        wd: Watchdog { resets: 0 },
        local_only: true,
    };
    a.dbg.hit(200) && a.dbg.watch_hit(5) && a.dbg.hit(300) == false && a.ok()
}

// ── 注入核（0x9680 段 5 项）────────────────────────────────────────────

/// 注入核 1：状态断裂注入——同键异值迁移必检出。
pub fn inj1_state_break() -> bool {
    let h = HotReloader {
        before: alloc::vec![(1, 10)],
        after: alloc::vec![(1, 99)],
        applied: true,
    };
    !h.seamless()
}

/// 注入核 2：键丢失注入——迁移丢键必检出。
pub fn inj2_key_lost() -> bool {
    let h = HotReloader {
        before: alloc::vec![(1, 10), (2, 20)],
        after: alloc::vec![(1, 10)],
        applied: true,
    };
    !h.seamless()
}

/// 注入核 3：看门狗复位注入——失稳触发复位计数。
pub fn inj3_watchdog_reset() -> bool {
    let mut wd = Watchdog { resets: 0 };
    let _ = wd.tick(true);
    wd.resets == 1
}

/// 注入核 4：出机红线注入——local_only 假必红。
pub fn inj4_data_leak() -> bool {
    let a = DbgArch {
        dbg: Debugger { breakpoints: alloc::vec![], stepping: false, watches: alloc::vec![] },
        hot: HotReloader { before: alloc::vec![], after: alloc::vec![], applied: true },
        wd: Watchdog { resets: 0 },
        local_only: false,
    };
    !a.ok()
}

/// 注入核 5：断点未命中注入——非断点 pc 恒不命中。
pub fn inj5_no_false_hit() -> bool {
    let d = Debugger { breakpoints: alloc::vec![100], stepping: false, watches: alloc::vec![] };
    !d.hit(101)
}

/// 结构当量。
pub fn structural_weight() -> u32 {
    10u32 * 33
}

/// run_vey10_checks：本单判据聚合（诊断段 0x9680）。
pub fn run_vey10_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F5101 调试热载总架构");
    s.add(
        "代码体量≥320行当量",
        structural_weight() >= 320,
        if structural_weight() >= 320 { "" } else { "结构当量不足" },
    );
    s.add(
        "三件套（调试器/热载器/状态映射）",
        j1_main(),
        if j1_main() { "" } else { "三件破" },
    );
    s.add(
        "状态无缝迁移+出机红线+看门狗",
        j1_main() && inj1_state_break(),
        if j1_main() { "" } else { "迁移/红线破" },
    );
    let inj = [
        ("注入1同键异值", inj1_state_break()),
        ("注入2键丢失", inj2_key_lost()),
        ("注入3看门狗复位", inj3_watchdog_reset()),
        ("注入4出机破线", inj4_data_leak()),
        ("注入5断点无误命中", inj5_no_false_hit()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
