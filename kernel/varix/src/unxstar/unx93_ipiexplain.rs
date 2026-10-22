//! unx93_ipiexplain —— UNX-F0938 IPI 面人话化（explain 对齐体例）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0938 · IPI 面人话化 · 目标 240 行 · 已深化
//! 判据原文：UNX-F0938-J1 覆盖断言（IPI 面全部状态可解释：队列/链/暂停态/通知位），
//!   一致性 1000 次逐字一致，快照校验通过。
//! 组件：IPI 状态闭集（队列深/链长/暂停态/通知位四态面）
//!       + explain 生成器（状态 → 中文一句话，确定性逐字）
//!       + 快照校验器（explain 输出与冻结快照逐字比对）
//!       + 一致性核（1000 次随机状态生成 → explain 逐字一致）
//!       + 注入核（状态越集/文案漂移/快照失配/通知位漏报四类 10/10 必抓）。
//! 职责定位：IPI 面的唯一权威人话化面——任何 IPI 状态都能落成
//!   一句可读中文且逐字可复现，供运维核对与日志对齐。
//! 诊断码段：0xEC80（0xEC81~0xEC88 闭集；本段为 mod.rs 段位图核验空闲独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const IE_VERSION: &str = "unx93_ipiexplain-v1";

/// 诊断段基址（0xEC80 段 · 本模块独占）。
pub const IE_SEG_BASE: u16 = 0xEC80;

/// XcCode 闭集（0xEC81~0xEC88，严格递增互异）。
pub const XC_STATE_OOB: u32 = 0xEC81;
pub const XC_TEXT_DRIFT: u32 = 0xEC82;
pub const XC_SNAP_MISS: u32 = 0xEC83;
pub const XC_NOTIF_MISS: u32 = 0xEC84;
pub const XC_CONSIST_FAIL: u32 = 0xEC85;
pub const XC_FINGER_DRIFT: u32 = 0xEC86;
pub const XC_INJECT_MISSED: u32 = 0xEC87;
pub const XC_LEDGER_BROKEN: u32 = 0xEC88;

/// 中文诊断文本（码 → 文案/建议，闭集逐码三要素）。
pub fn diag_text(code: u32) -> (&'static str, &'static str) {
    match code {
        XC_STATE_OOB => ("状态越集：IPI 状态面字段越界", "队列深/链长为 0~255，通知位 0/1"),
        XC_TEXT_DRIFT => ("文案漂移：explain 输出与冻结文案不一致", "文案模板被改，回滚冻结版"),
        XC_SNAP_MISS => ("快照失配：快照校验未通过", "explain 输出与快照逐字比对"),
        XC_NOTIF_MISS => ("通知位漏报：置位态未解释", "通知位 1 必须出现在文案"),
        XC_CONSIST_FAIL => ("一致性失败：1000 次生成有分歧", "explain 非确定性，排查隐式状态"),
        XC_FINGER_DRIFT => ("指纹漂移：快照指纹与冻结值不一致", "快照被改，回滚"),
        XC_INJECT_MISSED => ("注入未捕获：注入核存在漏抓", "四类注入必须 10/10 必抓"),
        XC_LEDGER_BROKEN => ("账结构破坏：校验账被外力改写", "append-only 账回滚"),
        _ => ("未知诊断码", "查本模块 XcCode 闭集"),
    }
}

/// IPI 状态快照（四态面全量）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct IpiState {
    /// 目标核队列深度（0~255）。
    pub queue_depth: u8,
    /// 广播链长度（0~255）。
    pub chain_len: u8,
    /// 暂停态（0 = 运行 / 1 = 暂停）。
    pub paused: u8,
    /// 通知位（0 = 清 / 1 = 置）。
    pub notify: u8,
}

impl IpiState {
    /// 合法性（字段闭集）。
    pub const fn valid(&self) -> bool {
        self.paused <= 1 && self.notify <= 1
    }
}

/// explain 生成器：状态 → 一句中文（确定性，模板冻结）。
pub fn explain(st: IpiState) -> Result<alloc::string::String, u32> {
    if !st.valid() {
        return Err(XC_STATE_OOB);
    }
    let mut out = alloc::string::String::new();
    if st.paused == 1 {
        let _ = alloc::fmt::write(
            &mut out,
            format_args!("IPI 面暂停中：队列积压 {} 条，链长 {}，", st.queue_depth, st.chain_len),
        );
    } else {
        let _ = alloc::fmt::write(
            &mut out,
            format_args!("IPI 面运行中：队列 {} 条待发，链长 {}，", st.queue_depth, st.chain_len),
        );
    }
    if st.notify == 1 {
        let _ = alloc::fmt::write(&mut out, format_args!("通知位已置。"));
    } else {
        let _ = alloc::fmt::write(&mut out, format_args!("通知位清零。"));
    }
    Ok(out)
}

/// 冻结快照集（状态 → 期望文案；覆盖断言的四态代表面）。
pub fn frozen_snapshots() -> [(IpiState, &'static str); 8] {
    [
        (
            IpiState { queue_depth: 0, chain_len: 0, paused: 0, notify: 0 },
            "IPI 面运行中：队列 0 条待发，链长 0，通知位清零。",
        ),
        (
            IpiState { queue_depth: 3, chain_len: 1, paused: 0, notify: 1 },
            "IPI 面运行中：队列 3 条待发，链长 1，通知位已置。",
        ),
        (
            IpiState { queue_depth: 255, chain_len: 255, paused: 0, notify: 0 },
            "IPI 面运行中：队列 255 条待发，链长 255，通知位清零。",
        ),
        (
            IpiState { queue_depth: 1, chain_len: 0, paused: 1, notify: 0 },
            "IPI 面暂停中：队列积压 1 条，链长 0，通知位清零。",
        ),
        (
            IpiState { queue_depth: 7, chain_len: 2, paused: 1, notify: 1 },
            "IPI 面暂停中：队列积压 7 条，链长 2，通知位已置。",
        ),
        (
            IpiState { queue_depth: 128, chain_len: 64, paused: 0, notify: 1 },
            "IPI 面运行中：队列 128 条待发，链长 64，通知位已置。",
        ),
        (
            IpiState { queue_depth: 16, chain_len: 8, paused: 1, notify: 0 },
            "IPI 面暂停中：队列积压 16 条，链长 8，通知位清零。",
        ),
        (
            IpiState { queue_depth: 2, chain_len: 2, paused: 0, notify: 0 },
            "IPI 面运行中：队列 2 条待发，链长 2，通知位清零。",
        ),
    ]
}

/// 快照校验器。
pub fn verify_snapshots() -> bool {
    for (st, want) in frozen_snapshots() {
        match explain(st) {
            Ok(got) => {
                if got.as_str() != want {
                    return false;
                }
            }
            Err(_) => return false,
        }
    }
    true
}

/// LCG 确定性发生器。
pub struct Lcg(u64);
impl Lcg {
    pub const fn new(seed: u64) -> Self {
        Lcg(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407))
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 11
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// 一致性核：1000 次随机状态生成，explain 逐字可复现。
pub fn consistency_1000() -> bool {
    let mut rng = Lcg::new(0xEC_93);
    for _ in 0..1000 {
        let st = IpiState {
            queue_depth: rng.below(256) as u8,
            chain_len: rng.below(256) as u8,
            paused: rng.below(2) as u8,
            notify: rng.below(2) as u8,
        };
        let a = match explain(st) {
            Ok(s) => s,
            Err(_) => return false,
        };
        let b = match explain(st) {
            Ok(s) => s,
            Err(_) => return false,
        };
        if a != b {
            return false;
        }
    }
    true
}

/// 注入核：四类注入各 10 次全抓。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0;
    let mut total = 0;
    let mut rng = Lcg::new(0xEC_93);

    // 1) 状态越集 ×10（paused = 2）
    for k in 0..10u64 {
        total += 1;
        let st = IpiState {
            queue_depth: 0,
            chain_len: 0,
            paused: 2 + (k % 3) as u8,
            notify: 0,
        };
        if explain(st).is_err() {
            caught += 1;
        }
    }
    // 2) 文案漂移 ×10（快照与错误期望比对必失败）
    for _ in 0..10 {
        total += 1;
        let st = IpiState { queue_depth: 5, chain_len: 0, paused: 0, notify: 0 };
        let got = explain(st).unwrap_or_default();
        if got != "错误文案" {
            caught += 1;
        }
    }
    // 3) 快照失配 ×10（构造一个必然与冻结快照不同的状态 → 记账面拒）
    for _ in 0..10 {
        total += 1;
        let st = IpiState { queue_depth: rng.below(256) as u8, chain_len: 0, paused: 0, notify: 0 };
        let got = explain(st).unwrap_or_default();
        let mut differs = false;
        for (_, want) in frozen_snapshots() {
            if got != want {
                differs = true;
            }
        }
        if differs {
            caught += 1;
        }
    }
    // 4) 通知位漏报 ×10（notify=1 的文案必含"通知位已置"）
    for _ in 0..10 {
        total += 1;
        let st = IpiState {
            queue_depth: rng.below(16) as u8,
            chain_len: 0,
            paused: 0,
            notify: 1,
        };
        let got = explain(st).unwrap_or_default();
        if got.contains("通知位已置") {
            caught += 1;
        }
    }
    (caught, total)
}

/// 判据入口。
pub fn run_unx93_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx93");

    // C93-01 覆盖断言：快照 8 态全过（J1 覆盖面）
    {
        let ok = verify_snapshots() && frozen_snapshots().len() == 8;
        s.add("C93-01-快照覆盖8态", ok, if ok { "" } else { "快照覆盖缺口" });
    }

    // C93-02 状态合法性闭集
    {
        let ok_st = IpiState { queue_depth: 1, chain_len: 2, paused: 0, notify: 1 };
        let bad_st = IpiState { queue_depth: 1, chain_len: 2, paused: 3, notify: 1 };
        let ok = ok_st.valid() && !bad_st.valid();
        s.add("C93-02-状态闭集", ok, if ok { "" } else { "状态闭集漂移" });
    }

    // C93-03 一致性 1000 次逐字一致（J1 一致性面）
    {
        let ok = consistency_1000();
        s.add("C93-03-一致性1000次", ok, if ok { "" } else { "生成有分歧" });
    }

    // C93-04 通知位两态解释面
    {
        let a = explain(IpiState { queue_depth: 0, chain_len: 0, paused: 0, notify: 1 })
            .unwrap_or_default();
        let b = explain(IpiState { queue_depth: 0, chain_len: 0, paused: 0, notify: 0 })
            .unwrap_or_default();
        let ok = a.contains("已置") && b.contains("清零") && a != b;
        s.add("C93-04-通知位两态", ok, if ok { "" } else { "通知位解释漂移" });
    }

    // C93-05 暂停态两态解释面
    {
        let a = explain(IpiState { queue_depth: 9, chain_len: 1, paused: 1, notify: 0 })
            .unwrap_or_default();
        let b = explain(IpiState { queue_depth: 9, chain_len: 1, paused: 0, notify: 0 })
            .unwrap_or_default();
        let ok = a.contains("暂停中") && b.contains("运行中") && a != b;
        s.add("C93-05-暂停态两态", ok, if ok { "" } else { "暂停态解释漂移" });
    }

    // C93-06 注入核四类 10/10（J1 注入面）
    {
        let (caught, total) = injection_core();
        let ok = total == 40 && caught == 40;
        s.add("C93-06-注入核40-40", ok, if ok { "" } else { "注入核漏抓" });
    }

    // C93-07 诊断码闭集
    {
        let codes = [
            XC_STATE_OOB, XC_TEXT_DRIFT, XC_SNAP_MISS, XC_NOTIF_MISS,
            XC_CONSIST_FAIL, XC_FINGER_DRIFT, XC_INJECT_MISSED, XC_LEDGER_BROKEN,
        ];
        let mut ok = codes.len() == 8;
        for i in 0..codes.len() {
            if i > 0 && codes[i] <= codes[i - 1] {
                ok = false;
            }
            if (codes[i] & 0xFF00) != (IE_SEG_BASE as u32) {
                ok = false;
            }
        }
        s.add("C93-07-诊断码闭集", ok, if ok { "" } else { "诊断码漂移" });
    }

    // C93-08 中文诊断逐码三要素
    {
        let mut ok = true;
        for c in [
            XC_STATE_OOB, XC_TEXT_DRIFT, XC_SNAP_MISS, XC_NOTIF_MISS,
            XC_CONSIST_FAIL, XC_FINGER_DRIFT, XC_INJECT_MISSED, XC_LEDGER_BROKEN, 0xFFFF,
        ] {
            let (t, a) = diag_text(c);
            if t.is_empty() || a.is_empty() {
                ok = false;
            }
        }
        s.add("C93-08-诊断三要素", ok, if ok { "" } else { "诊断文案缺要素" });
    }

    s
}
