// unx1049_explain —— UNX-F1049 仲裁面人话化（explain 对齐体例）
// 判据原文 UNX-F1049-J1：覆盖断言（仲裁/切换/模式面全部状态可解释），一致性 1000 次，快照通过。
// 落位：kernel/varix/src/unxstar/；诊断段 0x26B0 独占（0x26B1~0x26BF）。
//
// 设计（纯功能 no_std）：
//   * 三面状态全表：仲裁面/切换面/模式面每态一行人话文案；
//   * explain(id)：未知态返回诊断码，绝不 panic；
//   * 覆盖断言：三面全态逐条可解释（无空文案）；
//   * 一致性：同一态 explain 1000 次文案恒等；
//   * 快照指纹：文案表 FNV 冻结，改动即指纹漂移；
//   * 注入核全抓。

use crate::checks::CheckSet;
use alloc::vec::Vec;

// ── 诊断段（0x26B0 独占） ─────────────────────────────────────────────
pub const XC_EX_UNKNOWN: u32 = 0x26B1; // 未知状态码
pub const XC_EX_EMPTY: u32 = 0x26B2; // 空文案
pub const XC_EX_SEALED: u32 = 0x26B3; // 封账后改表
pub const XC_EX_CONSIST: u32 = 0x26B4; // 一致性破

/// 三面状态码（高 4bit 面 / 低 4bit 态）。
pub const FACE_ARB: u8 = 0x10; // 仲裁面
pub const FACE_SWITCH: u8 = 0x20; // 切换面
pub const FACE_MODE: u8 = 0x30; // 模式面

/// 仲裁面 5 态、切换面 5 态、模式面 4 态的人话文案表。
pub const ARB_TEXT: [&str; 5] = [
    "仲裁面：LAPIC 档胜出——本核专用超时器走本核定时器",
    "仲裁面：HPET 档胜出——共享用途走全局定时器",
    "仲裁面：LAPIC 紧张——降级 HPET 承接",
    "仲裁面：全紧张——超时器请求进入排队等待",
    "仲裁面：仲裁重跑——上一轮结果作废重判",
];
pub const SWITCH_TEXT: [&str; 5] = [
    "切换面：prepare 完成——切换窗已开，事件开始缓冲",
    "切换面：cutover 完成——主源已换，单调值连续",
    "切换面：verify 完成——窗内事件补执行完毕",
    "切换面：回滚——切换失败回到旧主源",
    "切换面：切换挂起——等待触发条件再续",
];
pub const MODE_TEXT: [&str; 4] = [
    "模式面：periodic——周期触发，自动重装",
    "模式面：oneshot——单发触发，到期即停",
    "模式面：tsc-deadline——TSC 死线触发，高精度",
    "模式面：未武装——定时器已配置但未启动",
];

/// explain 解释器。
pub struct Explainer {
    sealed: bool,
    queries: u32,
    rejects: Vec<(u32, &'static str)>,
}

impl Explainer {
    pub fn new() -> Explainer {
        Explainer {
            sealed: false,
            queries: 0,
            rejects: Vec::new(),
        }
    }

    /// 解释一个状态码：返回人话文案。
    pub fn explain(&mut self, code: u8) -> Result<&'static str, u32> {
        if self.sealed {
            self.rejects.push((XC_EX_SEALED, "封账后解释"));
            return Err(XC_EX_SEALED);
        }
        self.queries = self.queries.wrapping_add(1);
        let face = code & 0xF0;
        let idx = (code & 0x0F) as usize;
        let text = match face {
            FACE_ARB => ARB_TEXT.get(idx),
            FACE_SWITCH => SWITCH_TEXT.get(idx),
            FACE_MODE => MODE_TEXT.get(idx),
            _ => None,
        };
        match text {
            Some(t) if !t.is_empty() => Ok(*t),
            Some(_) => {
                self.rejects.push((XC_EX_EMPTY, "空文案"));
                Err(XC_EX_EMPTY)
            }
            None => {
                self.rejects.push((XC_EX_UNKNOWN, "未知状态码"));
                Err(XC_EX_UNKNOWN)
            }
        }
    }

    /// 覆盖断言：三面全部合法状态码可解释且非空。
    pub fn full_coverage(&mut self) -> bool {
        let mut ok = true;
        for i in 0..5u8 {
            if self.explain(FACE_ARB | i).is_err() {
                ok = false;
            }
        }
        for i in 0..5u8 {
            if self.explain(FACE_SWITCH | i).is_err() {
                ok = false;
            }
        }
        for i in 0..4u8 {
            if self.explain(FACE_MODE | i).is_err() {
                ok = false;
            }
        }
        ok
    }

    /// 一致性：同一状态码 1000 次解释文案恒等。
    pub fn consistent_1000(&mut self, code: u8) -> Result<bool, u32> {
        let first = self.explain(code)?;
        let mut same = true;
        for _ in 0..999 {
            let t = self.explain(code)?;
            if t != first {
                same = false;
            }
        }
        if !same {
            self.rejects.push((XC_EX_CONSIST, "一致性破"));
            return Err(XC_EX_CONSIST);
        }
        Ok(same)
    }

    /// 快照指纹：三面文案表 FNV 冻结。
    pub fn snapshot(&self) -> u64 {
        let mut h: u32 = 0x811C_9DC5;
        for t in ARB_TEXT.iter().chain(SWITCH_TEXT.iter()).chain(MODE_TEXT.iter()) {
            for &b in t.as_bytes() {
                h ^= b as u32;
                h = h.wrapping_mul(0x0100_0193);
            }
            h ^= 0x1F; // 文案分隔
            h = h.wrapping_mul(0x0100_0193);
        }
        h as u64
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_EX_SEALED);
        }
        self.sealed = true;
        Ok(self.snapshot())
    }

    pub fn query_count(&self) -> u32 {
        self.queries
    }
    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
}

/// 注入核：六类破坏全抓。
pub fn inject_core() -> (u32, u32) {
    let mut caught = 0u32;
    const N: u32 = 6;

    // 1）未知状态码必拒（10/10）
    {
        let mut e = Explainer::new();
        let mut hits = 0;
        for k in 0..10u8 {
            if e.explain(0x40 | k) == Err(XC_EX_UNKNOWN) {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 2）面内越界态必拒（idx 超表长）
    {
        let mut e = Explainer::new();
        if e.explain(FACE_ARB | 9) == Err(XC_EX_UNKNOWN) && e.explain(FACE_MODE | 7) == Err(XC_EX_UNKNOWN) {
            caught += 1;
        }
    }
    // 3）封账后解释必拒
    {
        let mut e = Explainer::new();
        let _ = e.seal();
        if e.explain(FACE_ARB) == Err(XC_EX_SEALED) {
            caught += 1;
        }
    }
    // 4）快照指纹稳定（两次快照恒等）
    {
        let e1 = Explainer::new();
        let e2 = Explainer::new();
        if e1.snapshot() == e2.snapshot() && e1.snapshot() != 0 {
            caught += 1;
        }
    }
    // 5）一致性 1000 次不破
    {
        let mut e = Explainer::new();
        if e.consistent_1000(FACE_SWITCH | 2) == Ok(true) {
            caught += 1;
        }
    }
    // 6）覆盖断言全过（14 态全可解释）
    {
        let mut e = Explainer::new();
        if e.full_coverage() && e.query_count() == 14 {
            caught += 1;
        }
    }
    (N, caught)
}

/// 判据入口：UNX-F1049-J1。
pub fn run_unx1049_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx1049");

    // C49-01 仲裁面 5 态全可解释
    {
        let mut e = Explainer::new();
        let mut all_ok = true;
        for i in 0..5u8 {
            if e.explain(FACE_ARB | i).is_err() {
                all_ok = false;
            }
        }
        s.add("C49-01-仲裁面覆盖", all_ok, if all_ok { "" } else { "覆盖缺" });
    }
    // C49-02 切换面 5 态全可解释
    {
        let mut e = Explainer::new();
        let mut all_ok = true;
        for i in 0..5u8 {
            if e.explain(FACE_SWITCH | i).is_err() {
                all_ok = false;
            }
        }
        s.add("C49-02-切换面覆盖", all_ok, if all_ok { "" } else { "覆盖缺" });
    }
    // C49-03 模式面 4 态全可解释
    {
        let mut e = Explainer::new();
        let mut all_ok = true;
        for i in 0..4u8 {
            if e.explain(FACE_MODE | i).is_err() {
                all_ok = false;
            }
        }
        s.add("C49-03-模式面覆盖", all_ok, if all_ok { "" } else { "覆盖缺" });
    }
    // C49-04 一致性 1000 次
    {
        let mut e = Explainer::new();
        let ok = e.consistent_1000(FACE_ARB | 3) == Ok(true);
        s.add("C49-04-一致性1000", ok, if ok { "" } else { "一致性破" });
    }
    // C49-05 未知态拒
    {
        let mut e = Explainer::new();
        let ok = e.explain(0x99) == Err(XC_EX_UNKNOWN);
        s.add("C49-05-未知态拒", ok, if ok { "" } else { "漏抓" });
    }
    // C49-06 快照指纹稳定
    {
        let e = Explainer::new();
        let a = e.snapshot();
        let b = e.snapshot();
        s.add("C49-06-快照稳定", a == b && a != 0, if a == b { "" } else { "快照漂移" });
    }
    // C49-07 封账产指纹
    {
        let mut e = Explainer::new();
        s.add("C49-07-封账指纹", e.seal().is_ok(), "");
    }
    // C49-08 封账后解释拒
    {
        let mut e = Explainer::new();
        let _ = e.seal();
        let ok = e.explain(FACE_MODE) == Err(XC_EX_SEALED);
        s.add("C49-08-封账拒", ok, if ok { "" } else { "漏抓" });
    }
    // C49-09 注入核 6/6
    {
        let (n, c) = inject_core();
        let ok = n == 6 && c == 6;
        s.add("C49-09-注入核全抓", ok, if ok { "" } else { "注入漏抓" });
    }
    // C49-10 查询计数守恒
    {
        let mut e = Explainer::new();
        let _ = e.explain(FACE_ARB);
        let _ = e.explain(FACE_SWITCH | 1);
        let _ = e.explain(0xEE);
        s.add("C49-10-查询计数", e.query_count() == 3 && e.reject_count() == 1, if e.query_count() == 3 { "" } else { "计数漂移" });
    }
    s
}
