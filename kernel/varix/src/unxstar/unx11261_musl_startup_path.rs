//! UNX-F11261 musl 静态运行时初始化路径（0x1f3d0 段）
//! 纯功能模型：静态启动四段 _start→__libc_start_main→init_array→main 的
//! 进入序台账，逐段记入口地址；init_array 指针必须严格升序消费，
//! 段序跳号、重入、零地址、封册后写账四路坏样本各报本模块自有码。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::string::String;
use alloc::vec::Vec;

/// 诊断段（0x1f3d0 独占 0x10）。
pub const SEG_BASE: u32 = 0x1f3d0;
/// 段号越出 0..=3 闭集。
pub const SI_PHASE: u32 = SEG_BASE + 0x01;
/// 段序跳号（非期望后继段）。
pub const SI_ORDER: u32 = SEG_BASE + 0x02;
/// 段重入或重复收尾。
pub const SI_DUP: u32 = SEG_BASE + 0x03;
/// 入口地址为零。
pub const SI_ADDR: u32 = SEG_BASE + 0x04;
/// init_array 指针非严格升序。
pub const SI_INIT: u32 = SEG_BASE + 0x05;
/// 封册后写账。
pub const SI_FROZEN: u32 = SEG_BASE + 0x06;
/// init_array 槽位已满。
pub const SI_CAP: u32 = SEG_BASE + 0x07;

/// 四段编号：_start 起，main 止。
pub const PH_START: u8 = 0;
pub const PH_LIBC: u8 = 1;
pub const PH_INIT: u8 = 2;
pub const PH_MAIN: u8 = 3;
/// 静态启动段数。
pub const STEPS: usize = 4;
/// init_array 槽位定容。
pub const INIT_CAP: usize = 16;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;
/// 静态启动链种子。
const TAG: u64 = 0xb04_61;

/// 单段进入记录：序号/段号/入口地址。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StageRec {
    pub seq: u32,
    pub phase: u8,
    pub entry: u64,
}

impl StageRec {
    /// 渲染：`S1@401080#1`。
    pub fn render(&self) -> String {
        format!("S{}@{:x}#{}", self.phase, self.entry, self.seq)
    }
}

/// 段名（诊断用）。
pub fn phase_name(p: u8) -> &'static str {
    match p {
        PH_START => "_start",
        PH_LIBC => "__libc_start_main",
        PH_INIT => "init_array",
        PH_MAIN => "main",
        _ => "unknown",
    }
}

/// 静态启动台账：段录入 → init_array 升序消费 → 收尾。
#[derive(Clone, Debug)]
pub struct StartupLedger {
    pub recs: Vec<StageRec>,
    pub init_fns: Vec<u64>,
    pub exit_code: Option<i32>,
    pub accepts: u32,
    pub rejects: u32,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl StartupLedger {
    pub fn new() -> StartupLedger {
        StartupLedger {
            recs: Vec::new(),
            init_fns: Vec::new(),
            exit_code: None,
            accepts: 0,
            rejects: 0,
            frozen: false,
            root: None,
        }
    }

    /// 录一段。判定序：封册 → 段号闭集 → 重入 → 段序 → 零地址。
    /// 重入早于段序：重复段报 SI_DUP，不被段序跳过号覆盖。
    pub fn record(&mut self, phase: u8, entry: u64) -> Result<u32, u32> {
        if self.frozen {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(SI_FROZEN);
        }
        if phase >= STEPS as u8 {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(SI_PHASE);
        }
        if self.recs.iter().any(|r| r.phase == phase) {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(SI_DUP);
        }
        let want = self.recs.len() as u8;
        if phase != want {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(SI_ORDER);
        }
        if entry == 0 {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(SI_ADDR);
        }
        let seq = self.recs.len() as u32;
        self.recs.push(StageRec { seq, phase, entry });
        self.accepts = self.accepts.wrapping_add(1);
        Ok(seq)
    }

    /// 消费一个 init_array 指针。判定序：封册 → 当前段须为 init_array
    /// → 零地址 → 严格升序 → 定容。
    pub fn add_init(&mut self, addr: u64) -> Result<usize, u32> {
        if self.frozen {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(SI_FROZEN);
        }
        let last = match self.recs.last() {
            Some(r) => r.phase,
            None => 0xff,
        };
        if last != PH_INIT {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(SI_ORDER);
        }
        if addr == 0 {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(SI_ADDR);
        }
        if let Some(prev) = self.init_fns.last() {
            if addr <= *prev {
                self.rejects = self.rejects.wrapping_add(1);
                return Err(SI_INIT);
            }
        }
        if self.init_fns.len() >= INIT_CAP {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(SI_CAP);
        }
        self.init_fns.push(addr);
        self.accepts = self.accepts.wrapping_add(1);
        Ok(self.init_fns.len().wrapping_sub(1))
    }

    /// 收尾：main 已录且只收一次。
    pub fn finish(&mut self, code: i32) -> Result<(), u32> {
        if self.frozen {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(SI_FROZEN);
        }
        if self.exit_code.is_some() {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(SI_DUP);
        }
        if self.recs.len() != STEPS {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(SI_ORDER);
        }
        self.exit_code = Some(code);
        self.accepts = self.accepts.wrapping_add(1);
        Ok(())
    }

    /// init_array 指针严格升序。
    pub fn ascending(&self) -> bool {
        let mut j = 1usize;
        while j < self.init_fns.len() {
            match (self.init_fns.get(j - 1), self.init_fns.get(j)) {
                (Some(a), Some(b)) => {
                    if b <= a {
                        return false;
                    }
                }
                _ => return false,
            }
            j = j.wrapping_add(1);
        }
        true
    }

    /// 全链完成判据：四段按序、init 升序、已收尾。
    pub fn complete(&self) -> bool {
        if self.recs.len() != STEPS || self.exit_code.is_none() {
            return false;
        }
        let mut k = 0usize;
        while k < STEPS {
            match self.recs.get(k) {
                Some(r) => {
                    if r.phase as usize != k || r.seq as usize != k {
                        return false;
                    }
                }
                None => return false,
            }
            k = k.wrapping_add(1);
        }
        self.ascending()
    }

    /// FNV-1a 链指纹：段序/入口/init 序/退出码序敏感。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF ^ TAG;
        let mut k = 0usize;
        while k < self.recs.len() {
            if let Some(r) = self.recs.get(k) {
                h = (h ^ r.seq as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ r.phase as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ r.entry).wrapping_mul(FNV_PRIME);
            }
            k = k.wrapping_add(1);
        }
        let mut j = 0usize;
        while j < self.init_fns.len() {
            if let Some(a) = self.init_fns.get(j) {
                h = (h ^ *a).wrapping_mul(FNV_PRIME);
            }
            j = j.wrapping_add(1);
        }
        let e = match self.exit_code {
            Some(c) => c as u32 as u64,
            None => 0xffff_ffff,
        };
        h ^ e
    }

    /// 封册：已有根原样返回，否则现算 fold 入账。
    pub fn freeze(&mut self) -> Option<u64> {
        if self.frozen {
            return self.root;
        }
        let r = match self.root {
            Some(v) => v,
            None => self.fold(),
        };
        self.root = Some(r);
        self.frozen = true;
        Some(r)
    }
}

/// 健康基准：完整四段 + 三个 init 指针 + exit 0。
pub fn boot_trace() -> StartupLedger {
    let mut l = StartupLedger::new();
    let _ = l.record(PH_START, 0x401000);
    let _ = l.record(PH_LIBC, 0x401080);
    let _ = l.record(PH_INIT, 0x401100);
    let _ = l.add_init(0x402000);
    let _ = l.add_init(0x402010);
    let _ = l.add_init(0x402020);
    let _ = l.record(PH_MAIN, 0x401200);
    let _ = l.finish(0);
    l
}

/// 注入核：四路坏样本。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // (a) 段号越闭集
    let mut a = StartupLedger::new();
    got[0] = a.record(9, 0x401000).err() == Some(SI_PHASE);
    // (b) 段重入
    let mut b = StartupLedger::new();
    let _ = b.record(PH_START, 0x401000);
    got[1] = b.record(PH_START, 0x401000).err() == Some(SI_DUP);
    // (c) 零地址（形状违规）
    let mut c = StartupLedger::new();
    got[2] = c.record(PH_START, 0).err() == Some(SI_ADDR);
    // (d) 封册后写账
    let mut d = boot_trace();
    let _ = d.freeze();
    got[3] = d.record(PH_START, 0x1).err() == Some(SI_FROZEN);
    got
}

/// 静态启动路径判据。
pub fn run_unx11261_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11261");

    // X1 四段按序完成
    let x1 = boot_trace().complete();
    s.add("X1-四段完成", x1, "_start→libc→init→main 且已收尾");

    // X2 段序与段名账一致
    let x2 = {
        let l = boot_trace();
        let names = format!("{}>{}>{}>{}", phase_name(0), phase_name(1), phase_name(2), phase_name(3));
        names.contains("_start") && names.contains("main") && l.recs.len() == STEPS
    };
    s.add("X2-段名账", x2, "段名串与四段登记一致");

    // X3 init_array 升序
    let x3 = {
        let l = boot_trace();
        l.ascending() && l.init_fns.len() == 3
    };
    s.add("X3-init升序", x3, "三个 init 指针严格升序消费");

    // X4 指纹确定
    let x4 = {
        let l = boot_trace();
        let a = l.fold();
        a == l.fold() && a != 0
    };
    s.add("X4-指纹确定", x4, "同账两次指纹一致且非零");

    // X5 封册幂等
    let x5 = {
        let mut l = boot_trace();
        let r1 = l.freeze();
        let r2 = l.freeze();
        let live = l.fold();
        l.recs.clear();
        let r3 = l.freeze();
        r1 == r2 && r2 == r3 && r1.is_some() && live != l.fold()
    };
    s.add("X5-封册幂等", x5, "二次封册回原根，账变不改根");

    // X6 封册三路拒写
    let x6 = {
        let mut l = boot_trace();
        let _ = l.freeze();
        l.record(PH_START, 0x1).err() == Some(SI_FROZEN)
            && l.add_init(0x9000).err() == Some(SI_FROZEN)
            && l.finish(0).err() == Some(SI_FROZEN)
    };
    s.add("X6-封册拒写", x6, "录段/加 init/收尾三路均拒");

    // X7 段重入
    let x7 = {
        let mut l = StartupLedger::new();
        let _ = l.record(PH_START, 0x401000);
        l.record(PH_START, 0x401004).err() == Some(SI_DUP)
    };
    s.add("X7-段重入", x7, "同段二次录入报重入");

    // X8 段号闭集
    let x8 = StartupLedger::new().record(STEPS as u8, 0x1).err() == Some(SI_PHASE);
    s.add("X8-段号闭集", x8, "段号 4 越出 0..=3 报拒");

    // X9 零入口
    let x9 = StartupLedger::new().record(PH_START, 0).err() == Some(SI_ADDR);
    s.add("X9-零入口", x9, "入口零地址不入账");

    // X10 段序跳号
    let x10 = {
        let mut l = StartupLedger::new();
        let _ = l.record(PH_START, 0x401000);
        l.record(PH_MAIN, 0x401200).err() == Some(SI_ORDER)
    };
    s.add("X10-段序跳号", x10, "跳号录入报段序错");

    // X11 init 前置与逆序
    let x11 = {
        let mut a = StartupLedger::new();
        let _ = a.record(PH_START, 0x401000);
        let pre = a.add_init(0x402000).err() == Some(SI_ORDER);
        let mut b = StartupLedger::new();
        let _ = b.record(PH_START, 0x401000);
        let _ = b.record(PH_LIBC, 0x401080);
        let _ = b.record(PH_INIT, 0x401100);
        let _ = b.add_init(0x402020);
        let back = b.add_init(0x402010).err() == Some(SI_INIT);
        pre && back
    };
    s.add("X11-init门禁", x11, "非 init 段加指针与逆序均拒");

    // X12 init 定容
    let x12 = {
        let mut l = StartupLedger::new();
        let _ = l.record(PH_START, 0x401000);
        let _ = l.record(PH_LIBC, 0x401080);
        let _ = l.record(PH_INIT, 0x401100);
        let mut i = 0u64;
        while i < INIT_CAP as u64 {
            let _ = l.add_init(0x402000u64.wrapping_add(i.wrapping_mul(8)));
            i = i.wrapping_add(1);
        }
        l.add_init(0x500000).err() == Some(SI_CAP)
    };
    s.add("X12-init定容", x12, "16 个指针后第 17 个报满");

    // X13 收尾门禁与明细
    let (x13, line13) = {
        let mut a = StartupLedger::new();
        let early = a.finish(0).err() == Some(SI_ORDER);
        let mut b = boot_trace();
        let dbl = b.finish(1).err() == Some(SI_DUP);
        let line = format!("early={} dbl={} acc={} rej={}", early, dbl, b.accepts, b.rejects);
        (early && dbl && b.accepts == 8 && b.rejects == 1, line)
    };
    s.add_dyn("X13-收尾门禁", x13, &line13);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11261_flow() {
        let mut l = boot_trace();
        assert!(l.complete());
        assert!(l.ascending());
        assert_eq!(l.recs.len(), 4);
        assert_eq!(l.init_fns.len(), 3);
        assert_eq!(l.exit_code, Some(0));
        assert_eq!(l.accepts, 8);
        assert_eq!(l.rejects, 0);
        let r1 = l.freeze();
        let r2 = l.freeze();
        assert_eq!(r1, r2);
        assert!(r1.is_some());
        assert!(l.root == r1);
        assert!(l.record(PH_START, 0x1).err() == Some(SI_FROZEN));
        assert!(l.finish(0).err() == Some(SI_FROZEN));
    }

    #[test]
    fn unx11261_gates() {
        let mut l = StartupLedger::new();
        assert!(l.record(PH_START, 0x1000).is_ok());
        assert!(l.record(PH_START, 0x1000).err() == Some(SI_DUP));
        assert!(l.record(9, 0x1000).err() == Some(SI_PHASE));
        assert!(l.record(PH_INIT, 0x1000).err() == Some(SI_ORDER));
        assert!(l.record(PH_LIBC, 0).err() == Some(SI_ADDR));
        assert!(l.record(PH_LIBC, 0x1010).is_ok());
        assert!(l.add_init(0x2000).err() == Some(SI_ORDER));
        assert!(l.record(PH_INIT, 0x1020).is_ok());
        assert!(l.add_init(0x2000).is_ok());
        assert!(l.add_init(0x1f00).err() == Some(SI_INIT));
        assert!(l.record(PH_MAIN, 0x1030).is_ok());
        assert!(l.finish(0).is_ok());
        assert!(l.finish(1).err() == Some(SI_DUP));
        assert!(l.complete());
        assert!(l.freeze().is_some());
        assert!(l.record(PH_START, 0x1).err() == Some(SI_FROZEN));
        assert!(l.add_init(0x9000).err() == Some(SI_FROZEN));
        assert!(l.finish(2).err() == Some(SI_FROZEN));
        assert_eq!(l.accepts, 6);
        assert_eq!(l.rejects, 10);
    }

    #[test]
    fn unx11261_checks_green() {
        let s = run_unx11261_checks();
        assert!(s.all_passed(), "UNX-F11261 判据未全绿");
    }
}
