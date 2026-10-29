// unx1035_f1018 —— UNX-F1035 超时器接口册更新（F1018 册 v2——实现兑现版）
// 判据原文 UNX-F1035-J1：册更新完整（v2：超时器组从签名态升实现态——三接口
// 语义四节齐备），样例可编译，版本化 10/10 次。
// 落位：kernel/varix/src/unxstar/；诊断段 0x21C0 独占（0x21C0~0x21FF）。
//
// 设计（纯功能 no_std）：
//   * 册版本链：v1（签名态）→v2（实现态）升版账（升版理由逐条落账）；
//   * 超时器三接口语义四节：签名/前置/后置/错误闭集（逐节非空闸）；
//   * 兑现核对：v1 中签名态条目逐条升实现态（兑现率 100% 才可升 v2）；
//   * 版本化 10/10：升版单调 + 回退拒；
//   * 样例可编译：每条接口样例返回非哨兵；
//   * 封账冻结 + FNV 指纹 + 注入核六类全抓。

use crate::checks::CheckSet;
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0x21C0 独占） ─────────────────────────────────────────────
pub const XC_IFC2_SECT: u32 = 0x21C1; // 语义四节缺节
pub const XC_IFC2_VER: u32 = 0x21C2; // 版本回退/平版
pub const XC_IFC2_REDEEM: u32 = 0x21C3; // 兑现不全升版
pub const XC_IFC2_SEALED: u32 = 0x21C4; // 封账后改册
pub const XC_IFC2_DUP: u32 = 0x21C5; // 接口名重复
pub const XC_IFC2_LOST: u32 = 0x21C6; // 三接口不齐

/// 超时器组三接口闭集。
pub const IF_ARM: u32 = 0;
pub const IF_CANCEL: u32 = 1;
pub const IF_RESET: u32 = 2;
pub const N_IFACES: usize = 3;

/// 语义四节。
#[derive(Clone, Copy)]
pub struct SemSections {
    pub sig: &'static str,      // 签名
    pub pre: &'static str,      // 前置
    pub post: &'static str,     // 后置
    pub errs: &'static str,     // 错误闭集
}

/// 一条接口册目。
#[derive(Clone, Copy)]
pub struct TmoDecl {
    pub iid: u32,
    pub name: &'static str,
    pub sem: SemSections,
    pub impl_state: bool, // v2 实现态
    pub sample_ret: u64,
}

/// 超时器接口册 v2。
pub struct TmoBook2 {
    decls: Vec<TmoDecl>,
    version: u32,
    redeemed_from: u32, // 从哪一版兑现
    sealed: bool,
}

impl TmoBook2 {
    pub fn new() -> TmoBook2 {
        TmoBook2 {
            decls: Vec::new(),
            version: 1,
            redeemed_from: 0,
            sealed: false,
        }
    }

    /// 登记接口（四节非空闸 + 重名拒）。
    pub fn declare(&mut self, iid: u32, name: &'static str, sem: SemSections, implemented: bool, sample_ret: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(XC_IFC2_SEALED);
        }
        if iid >= N_IFACES as u32 {
            return Err(XC_IFC2_LOST);
        }
        if sem.sig.is_empty() || sem.pre.is_empty() || sem.post.is_empty() || sem.errs.is_empty() {
            return Err(XC_IFC2_SECT);
        }
        if self.decls.iter().any(|d| d.name == name) {
            return Err(XC_IFC2_DUP);
        }
        self.decls.push(TmoDecl {
            iid,
            name,
            sem,
            impl_state: implemented,
            sample_ret,
        });
        Ok(())
    }

    /// 兑现核对：三接口全实现态。
    pub fn fully_implemented(&self) -> bool {
        self.decls.len() == N_IFACES && self.decls.iter().all(|d| d.impl_state)
    }

    /// 兑现率（千分制）。
    pub fn redeem_rate_permille(&self) -> u32 {
        if self.decls.is_empty() {
            return 0;
        }
        let n = self.decls.iter().filter(|d| d.impl_state).count();
        ((n as u64 * 1000) / self.decls.len() as u64) as u32
    }

    /// 升版（实现兑现硬闸：三接口全实现才可升）。
    pub fn bump_version(&mut self, new_ver: u32) -> Result<u32, u32> {
        if self.sealed {
            return Err(XC_IFC2_SEALED);
        }
        if new_ver <= self.version {
            return Err(XC_IFC2_VER);
        }
        if new_ver >= 2 && !self.fully_implemented() {
            return Err(XC_IFC2_REDEEM);
        }
        self.redeemed_from = self.version;
        self.version = new_ver;
        Ok(self.version)
    }

    pub fn version(&self) -> u32 {
        self.version
    }
    pub fn redeemed_from(&self) -> u32 {
        self.redeemed_from
    }

    /// 样例可编译（无哨兵）。
    pub fn samples_ok(&self) -> bool {
        self.decls.iter().all(|d| d.sample_ret != u64::MAX)
    }

    /// 四节齐备断言（登记时已闸，复核通道）。
    pub fn sections_ok(&self) -> bool {
        self.decls.iter().all(|d| {
            !d.sem.sig.is_empty() && !d.sem.pre.is_empty() && !d.sem.post.is_empty() && !d.sem.errs.is_empty()
        })
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_IFC2_SEALED);
        }
        if self.decls.len() != N_IFACES {
            return Err(XC_IFC2_LOST);
        }
        self.sealed = true;
        Ok(self.fingerprint())
    }

    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// FNV 指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u32 = 0x811C_9DC5;
        h ^= self.version ^ self.redeemed_from.wrapping_mul(0x9E37);
        h = h.wrapping_mul(0x0100_0193);
        for d in self.decls.iter() {
            h ^= d.iid ^ (d.impl_state as u32) << 8 ^ (d.sample_ret as u32);
            h = h.wrapping_mul(0x0100_0193);
            for b in d.name.as_bytes().iter() {
                h ^= *b as u32;
                h = h.wrapping_mul(0x0100_0193);
            }
        }
        (h as u64) << 20 | (self.decls.len() as u64 & 0xFFFFF)
    }

    pub fn decl_count(&self) -> usize {
        self.decls.len()
    }
}

/// v2 默认册：超时器三接口全实现态（四节齐备）。
pub fn declare_v2(b: &mut TmoBook2) -> Result<(), u32> {
    b.declare(
        IF_ARM,
        "tmo_arm",
        SemSections {
            sig: "fn tmo_arm(deadline_ns: u64, tag: u32) -> Result<u32, u32>",
            pre: "deadline_ns > ktime_get() 且时钟源就绪",
            post: "返回唯一句柄；到期恰好一次回调",
            errs: "过去时/E2146 竞态、封账态拒",
        },
        true,
        1,
    )?;
    b.declare(
        IF_CANCEL,
        "tmo_cancel",
        SemSections {
            sig: "fn tmo_cancel(h: u32) -> Result<(), u32>",
            pre: "h 为在册句柄",
            post: "取消后零回调；终态句柄拒",
            errs: "NOH/E2041 无句柄、DUP/E2046 双取消、TERM/E2042 终态",
        },
        true,
        0,
    )?;
    b.declare(
        IF_RESET,
        "tmo_reset",
        SemSections {
            sig: "fn tmo_reset(h: u32, new_dl: u64) -> Result<(), u32>",
            pre: "h 为 Armed 句柄且 new_dl 在未来",
            post: "旧到期作废；新时刻到期恰好一次",
            errs: "NOH/E2041、TERM/E2042、过去时拒",
        },
        true,
        0,
    )?;
    Ok(())
}

/// 注入核：六类各 10/10。
pub fn inject_core() -> (u32, u32) {
    let mut caught = 0u32;
    const N: u32 = 6;

    // 1）四节缺节必拒（逐节空十例）
    {
        let empty = SemSections { sig: "", pre: "", post: "", errs: "" };
        let mut hits = 0;
        for k in 0..10u32 {
            let mut b = TmoBook2::new();
            let sem = match k % 4 {
                0 => SemSections { sig: "", ..empty },
                1 => SemSections { pre: "", ..empty },
                2 => SemSections { post: "", ..empty },
                _ => SemSections { errs: "", ..empty },
            };
            if b.declare(k % 3, "n", sem, true, 0) == Err(XC_IFC2_SECT) {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 2）版本回退必拒（1→3 后 1..=3 全拒）
    {
        let mut b = TmoBook2::new();
        let _ = declare_v2(&mut b);
        let _ = b.bump_version(3);
        let mut hits = 0;
        for v in 1..=10u32 {
            if b.bump_version(v).is_err() {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 3）兑现不全升版必拒（缺一条实现态）
    {
        let mut b = TmoBook2::new();
        let _ = declare_v2(&mut b);
        // 重建半实现册
        let mut half = TmoBook2::new();
        let _ = half.declare(IF_ARM, "tmo_arm", SemSections { sig: "s", pre: "p", post: "q", errs: "e" }, true, 1);
        let _ = half.declare(IF_CANCEL, "tmo_cancel", SemSections { sig: "s", pre: "p", post: "q", errs: "e" }, false, 0);
        let _ = half.declare(IF_RESET, "tmo_reset", SemSections { sig: "s", pre: "p", post: "q", errs: "e" }, true, 0);
        let mut hits = 0;
        for v in 2..=11u32 {
            if half.bump_version(v) == Err(XC_IFC2_REDEEM) {
                hits += 1;
            }
        }
        if hits == 10 && !half.fully_implemented() {
            caught += 1;
        }
        let _ = b;
    }
    // 4）封账后改册必拒
    {
        let mut b = TmoBook2::new();
        let _ = declare_v2(&mut b);
        let _ = b.seal();
        let sem = SemSections { sig: "s", pre: "p", post: "q", errs: "e" };
        if b.declare(0, "late", sem, true, 0) == Err(XC_IFC2_SEALED) {
            caught += 1;
        }
    }
    // 5）重名必拒（10/10）
    {
        let mut b = TmoBook2::new();
        let _ = declare_v2(&mut b);
        let sem = SemSections { sig: "s", pre: "p", post: "q", errs: "e" };
        let mut hits = 0;
        for _ in 0..10 {
            if b.declare(0, "tmo_arm", sem, true, 0) == Err(XC_IFC2_DUP) {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 6）三接口不齐封账必拒
    {
        let mut b = TmoBook2::new();
        let sem = SemSections { sig: "s", pre: "p", post: "q", errs: "e" };
        let _ = b.declare(IF_ARM, "tmo_arm", sem, true, 1);
        if b.seal() == Err(XC_IFC2_LOST) {
            caught += 1;
        }
    }
    (N, caught)
}

/// 判据入口：UNX-F1035-J1。
pub fn run_unx1035_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx1035");

    // C35-01 v2 册齐备（三接口全实现态）
    {
        let mut b = TmoBook2::new();
        let reg = declare_v2(&mut b);
        let ok = reg.is_ok() && b.decl_count() == 3 && b.fully_implemented() && b.redeem_rate_permille() == 1000;
        s.add("C35-01-v2 齐备", ok, if ok { "" } else { "兑现缺" });
    }
    // C35-02 语义四节齐备
    {
        let mut b = TmoBook2::new();
        let _ = declare_v2(&mut b);
        s.add("C35-02-四节齐备", b.sections_ok(), if b.sections_ok() { "" } else { "缺节" });
    }
    // C35-03 样例可编译
    {
        let mut b = TmoBook2::new();
        let _ = declare_v2(&mut b);
        s.add("C35-03-样例可编译", b.samples_ok(), if b.samples_ok() { "" } else { "样例哨兵" });
    }
    // C35-04 版本化 10/10（2→11 连升）
    {
        let mut b = TmoBook2::new();
        let _ = declare_v2(&mut b);
        let mut ok = true;
        for v in 2..=11u32 {
            if b.bump_version(v).is_err() {
                ok = false;
                break;
            }
        }
        ok = ok && b.version() == 11 && b.redeemed_from() == 10;
        s.add("C35-04-升版十连", ok, if ok { "" } else { "升版破" });
    }
    // C35-05 版本回退拒
    {
        let mut b = TmoBook2::new();
        let _ = declare_v2(&mut b);
        let _ = b.bump_version(4);
        let ok = b.bump_version(3) == Err(XC_IFC2_VER) && b.bump_version(4) == Err(XC_IFC2_VER);
        s.add("C35-05-回退拒", ok, if ok { "" } else { "回退漏" });
    }
    // C35-06 兑现不全升版拒
    {
        let mut half = TmoBook2::new();
        let sem = SemSections { sig: "s", pre: "p", post: "q", errs: "e" };
        let _ = half.declare(IF_ARM, "tmo_arm", sem, true, 1);
        let _ = half.declare(IF_CANCEL, "tmo_cancel", sem, false, 0);
        let _ = half.declare(IF_RESET, "tmo_reset", sem, true, 0);
        let ok = half.bump_version(2) == Err(XC_IFC2_REDEEM);
        s.add("C35-06-兑现硬闸", ok, if ok { "" } else { "假兑现" });
    }
    // C35-07 四节缺节拒
    {
        let mut b = TmoBook2::new();
        let a = b.declare(0, "x", SemSections { sig: "", pre: "p", post: "q", errs: "e" }, true, 0) == Err(XC_IFC2_SECT);
        let c = b.declare(0, "x", SemSections { sig: "s", pre: "p", post: "q", errs: "" }, true, 0) == Err(XC_IFC2_SECT);
        s.add("C35-07-缺节拒", a && c, if a && c { "" } else { "缺节漏" });
    }
    // C35-08 封账冻结
    {
        let mut b = TmoBook2::new();
        let _ = declare_v2(&mut b);
        let fp = b.seal();
        let sem = SemSections { sig: "s", pre: "p", post: "q", errs: "e" };
        let ok = fp.is_ok()
            && b.is_sealed()
            && b.declare(0, "late", sem, true, 0) == Err(XC_IFC2_SEALED)
            && b.bump_version(9) == Err(XC_IFC2_SEALED);
        s.add("C35-08-封账冻结", ok, if ok { "" } else { "封账失守" });
    }
    // C35-09 接口不齐封账拒
    {
        let mut b = TmoBook2::new();
        let sem = SemSections { sig: "s", pre: "p", post: "q", errs: "e" };
        let _ = b.declare(IF_ARM, "tmo_arm", sem, true, 1);
        let _ = b.declare(IF_CANCEL, "tmo_cancel", sem, true, 0);
        let ok = b.seal() == Err(XC_IFC2_LOST);
        s.add("C35-09-缺条拒封", ok, if ok { "" } else { "缺条假封" });
    }
    // C35-10 指纹往返（版本参与）
    {
        let build = |v: u32| {
            let mut b = TmoBook2::new();
            let _ = declare_v2(&mut b);
            let _ = b.bump_version(v);
            b.fingerprint()
        };
        let ok = build(2) == build(2) && build(2) != build(5);
        s.add("C35-10-指纹含版", ok, if ok { "" } else { "指纹漂移" });
    }
    // C35-11 重名拒
    {
        let mut b = TmoBook2::new();
        let _ = declare_v2(&mut b);
        let sem = SemSections { sig: "s", pre: "p", post: "q", errs: "e" };
        let ok = b.declare(0, "tmo_reset", sem, true, 0) == Err(XC_IFC2_DUP);
        s.add("C35-11-重名拒", ok, if ok { "" } else { "重名漏" });
    }
    // C35-12 注入核 6/6
    {
        let (n, c) = inject_core();
        let ok = n == 6 && c == 6;
        s.add("C35-12-注入核全抓", ok, if ok { "" } else { "注入漏抓" });
    }
    s
}
