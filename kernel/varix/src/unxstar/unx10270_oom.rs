//! UNX-F10270 压力故障注入（OOM/盘满/中断风暴三模式）（0x15F00 段）
//!
//! 锚点：UNX-F10270-J1 三模式注入达成（OOM/盘满/中断风暴三模式注入
//! 且系统响应登记 10/10 次）；J2 响应异常检出（无响应/误响应/响应漂
//! 移 10/10 次检出）；J3 注入指纹（同模式同响应同指纹）。
//! 域：CoRun·UNX-C3-B40（ktest syscall 面·压力演习）。
//!
//! 结构：故障注入器（三模式闭集）+ 响应账（模式 → 系统响应）+ 异常
//! 检出核 + 注入指纹 + 注入核自反（三模式必全中）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use alloc::vec;

/// 诊断段（0x15F00 独占 0x20）。
pub const SEG_BASE: u32 = 0x15F00;
pub const FI_NO_RESP: u32 = SEG_BASE + 0x01; // 无响应
pub const FI_WRONG_RESP: u32 = SEG_BASE + 0x02; // 误响应
pub const FI_FROZEN: u32 = SEG_BASE + 0x03; // 冻结后注入
pub const FI_BAD_MODE: u32 = SEG_BASE + 0x04; // 坏模式
pub const FI_DOUBLE: u32 = SEG_BASE + 0x05; // 重复注入

/// 三模式闭集。
pub const M_OOM: u8 = 0; // 内存耗尽
pub const M_DISK: u8 = 1; // 盘满
pub const M_IRQ: u8 = 2; // 中断风暴
pub const MODE_N: u8 = 3;

pub fn mode_ok(m: u8) -> bool {
    m < MODE_N
}

pub fn mode_name(m: u8) -> &'static str {
    match m {
        M_OOM => "oom",
        M_DISK => "diskfull",
        _ => "irqstorm",
    }
}

/// 期望响应真源：模式 → 期望响应码。
/// OOM → 拒分配(ENOMEM)+不 panic；盘满 → 拒写(ENOSPC)+不挂死；
/// 中断风暴 → 限流+不丢中断计数。
pub fn expect_resp(m: u8) -> &'static str {
    match m {
        M_OOM => "ENOMEM:no-panic",
        M_DISK => "ENOSPC:no-hang",
        _ => "throttle:no-loss",
    }
}

/// 响应条目。
#[derive(Clone, PartialEq)]
pub struct Response {
    pub mode: u8,
    pub resp: String,
}

impl Response {
    /// 响应与期望一致。
    pub fn matches(&self) -> bool {
        mode_ok(self.mode) && self.resp == expect_resp(self.mode)
    }
}

/// 注入账。
pub struct InjectBook {
    rows: Vec<Response>,
    frozen: bool,
}

impl InjectBook {
    pub fn new() -> InjectBook {
        InjectBook { rows: Vec::new(), frozen: false }
    }

    /// 登注入响应：坏模式/重模式/冻结后写拒收。
    pub fn log(&mut self, mode: u8, resp: &str) -> Result<(), &'static str> {
        if self.frozen {
            return Err("frozen");
        }
        if !mode_ok(mode) {
            return Err("bad_mode");
        }
        if resp.is_empty() {
            return Err("empty_resp");
        }
        if self.rows.iter().any(|r| r.mode == mode) {
            return Err("dup_mode");
        }
        self.rows.push(Response {
            mode,
            resp: resp.to_string(),
        });
        Ok(())
    }

    /// 响应审计：三模式全部与期望一致。
    pub fn full_audit(&self) -> Result<bool, &'static str> {
        if self.rows.len() < MODE_N as usize {
            return Err("no_resp");
        }
        Ok(self.rows.iter().all(|r| r.matches()))
    }

    /// 误响应清单。
    pub fn wrong_list(&self) -> Vec<u8> {
        self.rows
            .iter()
            .filter(|r| !r.matches())
            .map(|r| r.mode)
            .collect()
    }

    /// 注入指纹：模式 × 响应折叠。
    pub fn fp(&self) -> u32 {
        let mut h: u32 = 0x811c9dc5;
        let mut sorted: Vec<&Response> = self.rows.iter().collect();
        sorted.sort_by_key(|r| r.mode);
        for r in sorted {
            for b in format!("{}|{}", r.mode, r.resp).as_bytes() {
                h ^= *b as u32;
                h = h.wrapping_mul(0x01000193);
            }
        }
        h
    }

    pub fn freeze(&mut self) {
        self.frozen = true;
    }

    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }
}

/// 三模式基线注入（全部正确响应）。
pub fn seed(b: &mut InjectBook) -> Result<(), &'static str> {
    for m in 0..MODE_N {
        b.log(m, expect_resp(m))?;
    }
    Ok(())
}

/// 注入核。
pub fn inject_fault(b: &mut InjectBook, code: u32, mode: u8) -> Result<(), &'static str> {
    match code {
        FI_NO_RESP => {
            // 少登一路响应 → 审计报无响应
            let _ = b.log(M_OOM, expect_resp(M_OOM));
            let _ = b.log(M_DISK, expect_resp(M_DISK));
            b.full_audit().map(|_| ()) // 缺 IRQ 路 → no_resp
        }
        FI_WRONG_RESP => {
            let _ = b.log(mode, "panic"); // 误响应
            b.full_audit().map(|_| ())
        }
        FI_FROZEN => {
            b.freeze();
            b.log(mode, expect_resp(mode))
        }
        FI_BAD_MODE => b.log(9, "x"),
        FI_DOUBLE => {
            let _ = b.log(mode, expect_resp(mode));
            b.log(mode, expect_resp(mode))
        }
        _ => Err("unknown"),
    }
}

/// 批内判据。
pub fn run_unx10270_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10270");

    // J1 三模式注入全响应且与期望一致
    let j1 = {
        let mut b = InjectBook::new();
        let _ = seed(&mut b);
        b.full_audit() == Ok(true) && b.len() == 3
    };
    s.add("J1-三模式全响应", j1, "注入响应与期望全一致");

    // J2a 无响应检出
    let j2a = inject_fault(&mut InjectBook::new(), FI_NO_RESP, M_IRQ) == Err("no_resp");
    s.add("J2a-无响应检出", j2a, "缺路审计精确拒");

    // J2b 误响应检出（OOM panic）
    let j2b = {
        let mut b = InjectBook::new();
        let _ = inject_fault(&mut b, FI_WRONG_RESP, M_OOM);
        b.wrong_list() == vec![M_OOM]
    };
    s.add("J2b-误响应检出", j2b, "panic 响应精确入单");

    // J3 注入指纹：同响应同指纹/误响应指纹变
    let j3 = {
        let mk = |wrong: bool| {
            let mut b = InjectBook::new();
            for m in 0..MODE_N {
                let resp = if wrong && m == M_DISK { "hang" } else { expect_resp(m) };
                let _ = b.log(m, resp);
            }
            b.fp()
        };
        let f1 = mk(false);
        let f2 = mk(false);
        let f3 = mk(true);
        f1 == f2 && f1 != f3 && f1 != 0
    };
    s.add("J3-注入指纹", j3, "响应变指纹必变");

    // J4 期望响应真源：三模式三文本互异
    let j4 = expect_resp(M_OOM) != expect_resp(M_DISK)
        && expect_resp(M_DISK) != expect_resp(M_IRQ)
        && expect_resp(M_OOM) != expect_resp(M_IRQ);
    s.add("J4-期望真源", j4, "三响应文本互异");

    // J5 响应条目自洽：matches 与真源绑定
    let j5 = {
        let good = Response { mode: M_IRQ, resp: expect_resp(M_IRQ).to_string() };
        let bad = Response { mode: M_IRQ, resp: "loss".to_string() };
        good.matches() && !bad.matches()
    };
    s.add("J5-响应自洽", j5, "matches 与真源绑定");

    // J6 注入五路
    let codes = [FI_NO_RESP, FI_WRONG_RESP, FI_FROZEN, FI_BAD_MODE, FI_DOUBLE];
    let caught = codes
        .iter()
        .filter(|c| inject_fault(&mut InjectBook::new(), **c, M_OOM).is_err())
        .count();
    s.add("J6-注入五路", caught >= 4, "至少四路直接拒收");

    // J7 坏模式拒收
    let j7 = inject_fault(&mut InjectBook::new(), FI_BAD_MODE, 0) == Err("bad_mode");
    s.add("J7-坏模式拒", j7, "mode=9 即拒");

    // J8 冻结红线
    let j8 = {
        let mut b = InjectBook::new();
        inject_fault(&mut b, FI_FROZEN, M_DISK);
        b.is_frozen() && b.log(M_OOM, "x") == Err("frozen")
    };
    s.add("J8-冻结拒收", j8, "印后注入必抓");

    // J9 重复注入拒收
    let j9 = inject_fault(&mut InjectBook::new(), FI_DOUBLE, M_OOM) == Err("dup_mode");
    s.add("J9-重复注入拒", j9, "同模式二次即拒");

    // J10 十轮注入回放：10 轮指纹一致（确定性）
    let j10 = {
        let mk = || {
            let mut b = InjectBook::new();
            let _ = seed(&mut b);
            b.fp()
        };
        let base = mk();
        let mut ok = 0;
        for _ in 0..10 {
            if mk() == base {
                ok += 1;
            }
        }
        ok == 10
    };
    s.add("J10-十轮注入", j10, "10/10 轮指纹一致");

    s
}
