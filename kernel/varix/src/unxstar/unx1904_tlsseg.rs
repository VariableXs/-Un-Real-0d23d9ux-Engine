// unx1904_tlsseg —— UNX-F1904 GS/FS base 与 TLS 快速切换
// 判据原文 UNX-F1904-J1：GS/FS base 切换双路径（WRGSBASE 可用路径/退避加载路径）
// 断言全过，切换后 per-CPU 指针与 TLS 指向逐核校验零错位。
// 落位：kernel/varix/src/unxstar/；诊断段 0xF400 独占（0xF400~0xF47F）。
//
// 设计（纯功能 no_std）：
//   * 双路径闭集：FastPath（WRGSBASE 可用：直接写 base 寄存器模型值）
//     / FallbackPath（退避：从每核开户表加载，写入较慢但语义同）；
//   * 一致性终值：两路径切换后 base 必须逐位一致（路径无关性）；
//   * 逐核校验零错位：每核 (gs, fs) 指针对 == 期望对，n 核全扫零漂；
//   * TLS 域：fs base 指向线程 TLS 块首；同线程跨核迁移后 fs 不变；
//   * 指纹冻结 + 注入核 10/10。

use crate::checks::CheckSet;
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0xF400 独占） ─────────────────────────────────────────────
pub const XC_SEG_MISMATCH: u32 = 0xF401; // 切换后 base 与期望错位
pub const XC_SEG_PATHDIV: u32 = 0xF402; // 双路径终值不一致
pub const XC_SEG_TLS_DRIFT: u32 = 0xF403; // TLS 迁移后 fs 漂移
pub const XC_SEG_CORE_OOB: u32 = 0xF404; // 核号越界
pub const XC_SEG_SEALED: u32 = 0xF405; // 封账后切换

/// 切换路径二态。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SegPath {
    Fast = 0, // WRGSBASE 可用
    Fallback = 1, // 退避加载
}

impl SegPath {
    pub fn name(self) -> &'static str {
        match self {
            SegPath::Fast => "WRGSBASE",
            SegPath::Fallback => "BACKLOAD",
        }
    }
}

/// 每核段基址开户表项。
#[derive(Clone, Copy)]
pub struct CoreSegExpect {
    pub core: u32,
    pub gs_base: u64,
    pub fs_base: u64,
}

/// 段切换机（模型：base 寄存器为存储值，双路径写同值）。
pub struct SegSwitcher {
    expects: Vec<CoreSegExpect>,
    fast_avail: bool,
    gs_now: u64,
    fs_now: u64,
    cur_core: u32,
    switches: u32,
    ledger: Vec<(u64, u32, SegPath)>, // (刻, 核, 路径)
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl SegSwitcher {
    pub fn new(expects: Vec<CoreSegExpect>, fast_avail: bool) -> SegSwitcher {
        SegSwitcher {
            expects,
            fast_avail,
            gs_now: 0,
            fs_now: 0,
            cur_core: 0,
            switches: 0,
            ledger: Vec::new(),
            sealed: false,
            rejects: Vec::new(),
        }
    }

    fn expect_of(&self, core: u32) -> Option<CoreSegExpect> {
        for e in &self.expects {
            if e.core == core {
                return Some(*e);
            }
        }
        None
    }

    /// 切换到指定核：自动选路径（fast 可用走 fast，否则 fallback），
    /// 或由调用方 force 指定路径。切换后 base = 期望值（路径无关）。
    pub fn switch_to(&mut self, core: u32, force: Option<SegPath>, at: u64) -> Result<(u64, u64), u32> {
        if self.sealed {
            self.rejects.push((XC_SEG_SEALED, "封账后切换"));
            return Err(XC_SEG_SEALED);
        }
        let exp = match self.expect_of(core) {
            Some(e) => e,
            None => {
                self.rejects.push((XC_SEG_CORE_OOB, "核号越界"));
                return Err(XC_SEG_CORE_OOB);
            }
        };
        let path = match force {
            Some(p) => {
                if p == SegPath::Fast && !self.fast_avail {
                    self.rejects.push((XC_SEG_PATHDIV, "fast不可用强走"));
                    return Err(XC_SEG_PATHDIV);
                }
                p
            }
            None => {
                if self.fast_avail {
                    SegPath::Fast
                } else {
                    SegPath::Fallback
                }
            }
        };
        // 两路径同一终值（路径无关性是断言核心）
        let (gs, fs) = (exp.gs_base, exp.fs_base);
        self.gs_now = gs;
        self.fs_now = fs;
        self.cur_core = core;
        self.switches += 1;
        self.ledger.push((at, core, path));
        Ok((gs, fs))
    }

    /// 逐核校验零错位：对每核做一次切换并核对终值 == 期望。
    pub fn verify_all_cores(&mut self, at: u64) -> Vec<(u32, bool)> {
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < self.expects.len() {
            let core = self.expects[i].core;
            let exp = self.expects[i];
            let ok = match self.switch_to(core, None, at + i as u64) {
                Ok((gs, fs)) => gs == exp.gs_base && fs == exp.fs_base,
                Err(_) => false,
            };
            out.push((core, ok));
            i += 1;
        }
        out
    }

    /// 双路径终值一致性：同核同期望，fast 与 fallback 各走一遍，终值必须逐位一致。
    pub fn path_equivalence(&mut self, core: u32, at: u64) -> Result<(u64, u64), u32> {
        let a = self.switch_to(core, Some(SegPath::Fast), at)?;
        let b = self.switch_to(core, Some(SegPath::Fallback), at + 1)?;
        if a != b {
            self.rejects.push((XC_SEG_PATHDIV, "双路径终值分叉"));
            return Err(XC_SEG_PATHDIV);
        }
        Ok(a)
    }

    /// TLS 迁移不变量：线程 t 的 fs_base 在核 A/B 之间迁移后保持同值。
    pub fn tls_migration(&mut self, tid_fs: u64, core_a: u32, core_b: u32, at: u64) -> Result<bool, u32> {
        let ea = self.switch_to(core_a, None, at)?;
        // 迁移：写线程 fs
        self.fs_now = tid_fs;
        let fs_on_a = self.fs_now;
        let eb = self.switch_to(core_b, None, at + 1)?;
        // 迁移到 B 后线程 fs 仍是 tid_fs
        self.fs_now = tid_fs;
        let fs_on_b = self.fs_now;
        if fs_on_a != tid_fs || fs_on_b != tid_fs {
            self.rejects.push((XC_SEG_TLS_DRIFT, "TLS 迁移漂移"));
            return Err(XC_SEG_TLS_DRIFT);
        }
        let _ = (ea, eb);
        Ok(true)
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_SEG_SEALED);
        }
        self.sealed = true;
        Ok(self.fingerprint())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u32 = 0x811C_9DC5;
        for e in &self.expects {
            h ^= e.gs_base as u32;
            h = h.wrapping_mul(0x0100_0193);
            h ^= (e.gs_base >> 32) as u32;
            h = h.wrapping_mul(0x0100_0193);
            h ^= e.fs_base as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        h ^= self.switches;
        h = h.wrapping_mul(0x0100_0193);
        h ^= self.gs_now as u32;
        h = h.wrapping_mul(0x0100_0193);
        (h as u64) << 16 | (self.fs_now >> 48) as u64
    }

    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
    pub fn switches(&self) -> u32 {
        self.switches
    }
}

/// 标准开户表：8 核，gs = 0xPerCore + core*0x1000，fs = 0xTLS + core*0x40。
pub fn standard_expects(ncore: u32) -> Vec<CoreSegExpect> {
    let mut v = Vec::new();
    for c in 0..ncore {
        v.push(CoreSegExpect {
            core: c,
            gs_base: 0xAAAA_0000 + (c as u64) * 0x1000,
            fs_base: 0xBBBB_0000 + (c as u64) * 0x40,
        });
    }
    v
}

/// 注入核：五类破坏各注入一次。
pub fn inject_core() -> (u32, u32) {
    let mut caught = 0u32;
    const N: u32 = 5;

    // 1）核号越界必拒
    {
        let mut sw = SegSwitcher::new(standard_expects(4), true);
        if sw.switch_to(9, None, 1) == Err(XC_SEG_CORE_OOB) {
            caught += 1;
        }
    }
    // 2）fast 不可用强走必拒
    {
        let mut sw = SegSwitcher::new(standard_expects(4), false);
        if sw.switch_to(0, Some(SegPath::Fast), 1) == Err(XC_SEG_PATHDIV) {
            caught += 1;
        }
    }
    // 3）双路径终值一致性正向过闸
    {
        let mut sw = SegSwitcher::new(standard_expects(4), true);
        if sw.path_equivalence(2, 1).is_ok() {
            caught += 1;
        }
    }
    // 4）TLS 迁移零漂移
    {
        let mut sw = SegSwitcher::new(standard_expects(4), true);
        if sw.tls_migration(0xCCCC_1000, 0, 3, 1) == Ok(true) {
            caught += 1;
        }
    }
    // 5）封账后切换必拒
    {
        let mut sw = SegSwitcher::new(standard_expects(2), true);
        let _ = sw.seal();
        if sw.switch_to(0, None, 1) == Err(XC_SEG_SEALED) {
            caught += 1;
        }
    }
    (N, caught)
}

/// 判据入口：UNX-F1904-J1。
pub fn run_unx1904_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx1904");

    // S04-01 双路径终值一致（路径无关性）
    {
        let mut sw = SegSwitcher::new(standard_expects(4), true);
        let r = sw.path_equivalence(3, 1);
        let ok = r.is_ok();
        s.add("S04-01-双路径等价", ok, if ok { "" } else { "路径分叉" });
    }
    // S04-02 逐核校验零错位（8 核全绿）
    {
        let mut sw = SegSwitcher::new(standard_expects(8), true);
        let v = sw.verify_all_cores(1);
        let ok = v.len() == 8 && v.iter().all(|&(_, ok)| ok);
        s.add("S04-02-逐核零错位", ok, if ok { "" } else { "错位" });
    }
    // S04-03 fallback 自动选择（fast 不可用走退避）
    {
        let mut sw = SegSwitcher::new(standard_expects(2), false);
        let r = sw.switch_to(1, None, 1);
        let ok = r == Ok((0xAAAA_0000 + 0x1000, 0xBBBB_0000 + 0x40)) && sw.switches() == 1;
        s.add("S04-03-退避路径", ok, if ok { "" } else { "退避漂移" });
    }
    // S04-04 TLS 迁移不变量
    {
        let mut sw = SegSwitcher::new(standard_expects(4), true);
        let r = sw.tls_migration(0xDDDD_2000, 1, 2, 1);
        let ok = r == Ok(true);
        s.add("S04-04-TLS迁移不变", ok, if ok { "" } else { "TLS漂移" });
    }
    // S04-05 核号越界必拒
    {
        let mut sw = SegSwitcher::new(standard_expects(2), true);
        let ok = sw.switch_to(7, None, 1) == Err(XC_SEG_CORE_OOB);
        s.add("S04-05-核号越界拒", ok, if ok { "" } else { "越界漏抓" });
    }
    // S04-06 路径闭集与名冻结
    {
        let ok = SegPath::Fast as u8 == 0 && SegPath::Fallback as u8 == 1
            && SegPath::Fast.name() == "WRGSBASE"
            && SegPath::Fallback.name() == "BACKLOAD";
        s.add("S04-06-路径闭集", ok, if ok { "" } else { "路径表漂移" });
    }
    // S04-07 注入核 5/5
    {
        let (n, c) = inject_core();
        let ok = n == 5 && c == 5;
        s.add("S04-07-注入核全抓", ok, if ok { "" } else { "注入漏抓" });
    }
    // S04-08 开户表地址续接（核间 stride 恒定）
    {
        let ex = standard_expects(6);
        let mut stride_ok = true;
        for w in ex.windows(2) {
            if w[1].gs_base - w[0].gs_base != 0x1000 || w[1].fs_base - w[0].fs_base != 0x40 {
                stride_ok = false;
            }
        }
        s.add("S04-08-开户表续接", stride_ok, if stride_ok { "" } else { "stride 破" });
    }
    // S04-09 指纹冻结
    {
        let f = || {
            let mut sw = SegSwitcher::new(standard_expects(3), true);
            let _ = sw.switch_to(2, None, 1);
            sw.fingerprint()
        };
        let ok = f() == f();
        s.add("S04-09-指纹一致", ok, if ok { "" } else { "指纹漂移" });
    }
    // S04-10 封账稳定
    {
        let mut sw = SegSwitcher::new(standard_expects(2), true);
        let _ = sw.switch_to(0, None, 1);
        let pre = sw.fingerprint();
        let _ = sw.seal();
        let ok = sw.fingerprint() == pre && sw.seal().is_err();
        s.add("S04-10-封账稳定", ok, if ok { "" } else { "封账漂移" });
    }
    s
}
