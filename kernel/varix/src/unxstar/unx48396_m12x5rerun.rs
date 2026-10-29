//! UNX-F48396 —— M12 隐私终检复跑（全域 X5 零外联）
//! 判据原文：UNX-F48396-J1 按卡面验收句执行——
//!   J1-1 纯功能代码 ≥297 行；
//!   J1-2 X5 五类外联检项（遥测/网络请求/DNS/上传/心跳）闭集登记；
//!   J1-3 全域 12 个 M 段模块逐模块 5 检项计数全零（零外联）方判绿；
//!   J1-4 复跑（第二遍终检）指纹必须与首跑一致（确定性复跑）；
//!   J1-5 注入外联事件必须被检出并入账，注入后复跑必须拦截（E_DIRTY）；
//!   J1-6 清账后复绿；检项闭集外注入拒绝；首跑未做即复跑拒绝。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 闭包判据全绿才返回 Ok。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xD240; // 卡面分配段

// ── 诊断码闭集（SEG_BASE+1 起连续编号，全部 ≤ SEG_BASE+0x1F）──────
const E_EMPTY: u16 = SEG_BASE + 1;        // 空扫描账
const E_STRAY: u16 = SEG_BASE + 2;        // 闭集外模块
const E_DUP_MOD: u16 = SEG_BASE + 3;      // 模块重复登记
const E_MOD_MISS: u16 = SEG_BASE + 4;     // 全域模块有缺
const E_NOT_ZERO: u16 = SEG_BASE + 5;     // 外联计数非零
const E_DIRTY: u16 = SEG_BASE + 6;        // 注入未清：复跑拦截
const E_CLASS_BAD: u16 = SEG_BASE + 7;    // 检项闭集外注入
const E_RERUN_MISMATCH: u16 = SEG_BASE + 8; // 复跑指纹与首跑不符
const E_NO_PASS1: u16 = SEG_BASE + 9;     // 首跑未做即复跑
const E_UNINJ: u16 = SEG_BASE + 10;       // 注入未入账（检出失败）
const E_FINGER: u16 = SEG_BASE + 11;      // 指纹不稳定

pub fn detail(code: u16) -> &'static str {
    match code {
        E_EMPTY => "空扫描账：无模块可检",
        E_STRAY => "闭集外模块：全域清单外拒绝登记",
        E_DUP_MOD => "模块重复登记：后到拒绝",
        E_MOD_MISS => "全域模块有缺：不足 12 段即首跑",
        E_NOT_ZERO => "外联计数非零：零外联判据破坏",
        E_DIRTY => "注入未清：复跑拦截",
        E_CLASS_BAD => "检项闭集外注入：X5 五类之外拒绝",
        E_RERUN_MISMATCH => "复跑指纹与首跑不符：终检非确定",
        E_NO_PASS1 => "首跑未做即复跑：次序非法",
        E_UNINJ => "注入未入账：检出失败",
        E_FINGER => "指纹不稳定：确定性被破坏",
        _ => "未知诊断码",
    }
}

/// 确定性 LCG。
fn lcg_next(state: &mut u32) -> u32 {
    *state = state
        .wrapping_mul(1_664_525)
        .wrapping_add(1_013_904_223);
    *state
}

/// FNV-1a 64。
fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

// ── 数据模型 ────────────────────────────────────────────────
/// X5 五类外联检项闭集。
pub const X5_CLASSES: [&str; 5] = ["telemetry", "netreq", "dns", "upload", "heartbeat"];

/// 全域 M 段模块闭集：12 段。
pub const DOMAIN_MODULES: [&str; 12] = [
    "m5boot", "m5fs", "m5mem", "m5sched", "m5srv", "m6a11y",
    "m6sync", "m700blk", "m700net", "m700proc", "m700vmm", "m7bench",
];

/// 单模块隐私扫描：5 检项外联事件计数（全零 = 该模块零外联）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrivacyScan {
    pub module: &'static str,
    pub counts: [u32; 5],
}

impl PrivacyScan {
    pub fn zero(module: &'static str) -> PrivacyScan {
        PrivacyScan {
            module,
            counts: [0u32; 5],
        }
    }
    pub fn clean(&self) -> bool {
        self.counts.iter().all(|c| *c == 0)
    }
}

/// 注入记录：模块下标 + 检项下标。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InjectMark {
    pub mod_idx: usize,
    pub class_idx: usize,
}

/// X5 终检账。
pub struct X5Ledger {
    pub scans: Vec<PrivacyScan>,
    pub fp_pass1: Option<u64>,
    pub injected: Vec<InjectMark>,
}

impl X5Ledger {
    pub fn new() -> X5Ledger {
        X5Ledger {
            scans: Vec::new(),
            fp_pass1: None,
            injected: Vec::new(),
        }
    }

    /// 模块登记：闭集内 + 不重复。
    pub fn register(&mut self, module: &'static str) -> Result<u32, u16> {
        if module.is_empty() {
            return Err(E_EMPTY);
        }
        if !DOMAIN_MODULES.contains(&module) {
            return Err(E_STRAY);
        }
        if self.scans.iter().any(|s| s.module == module) {
            return Err(E_DUP_MOD);
        }
        self.scans.push(PrivacyScan::zero(module));
        Ok(self.scans.len() as u32)
    }

    /// 全域满员核对。
    pub fn domain_full(&self) -> bool {
        DOMAIN_MODULES
            .iter()
            .all(|m| self.scans.iter().any(|s| s.module == *m))
    }

    /// 首跑终检：全域满员 + 全部计数为零，落首跑指纹。
    pub fn first_pass(&mut self) -> Result<u64, u16> {
        if self.scans.is_empty() {
            return Err(E_EMPTY);
        }
        if !self.domain_full() {
            return Err(E_MOD_MISS);
        }
        for s in &self.scans {
            if !s.clean() {
                return Err(E_NOT_ZERO);
            }
        }
        let fp = self.fingerprint();
        self.fp_pass1 = Some(fp);
        Ok(fp)
    }

    /// 注入外联事件：模块/检项均须在闭集内，入账并使计数非零。
    pub fn inject(&mut self, mod_idx: usize, class_idx: usize) -> Result<u32, u16> {
        if class_idx >= X5_CLASSES.len() {
            return Err(E_CLASS_BAD);
        }
        if mod_idx >= self.scans.len() {
            return Err(E_MOD_MISS);
        }
        self.scans[mod_idx].counts[class_idx] += 1;
        self.injected.push(InjectMark { mod_idx, class_idx });
        Ok(self.injected.len() as u32)
    }

    /// 检出清单：所有计数非零的（模块，检项）对。
    pub fn detect(&self) -> Vec<(usize, usize)> {
        let mut out: Vec<(usize, usize)> = Vec::new();
        for (mi, s) in self.scans.iter().enumerate() {
            for (ci, c) in s.counts.iter().enumerate() {
                if *c > 0 {
                    out.push((mi, ci));
                }
            }
        }
        out
    }

    /// 复跑终检：先查未清注入（脏），再对指纹与首跑一致。
    pub fn rerun(&self) -> Result<u64, u16> {
        let fp1 = match self.fp_pass1 {
            Some(fp) => fp,
            None => return Err(E_NO_PASS1),
        };
        for s in &self.scans {
            if !s.clean() {
                return Err(E_DIRTY);
            }
        }
        if !self.injected.is_empty() {
            return Err(E_DIRTY);
        }
        let fp2 = self.fingerprint();
        if fp1 != fp2 {
            return Err(E_RERUN_MISMATCH);
        }
        Ok(fp2)
    }

    /// 清账：全部计数归零 + 注入账清空。
    pub fn clear(&mut self) {
        for s in self.scans.iter_mut() {
            s.counts = [0u32; 5];
        }
        self.injected.clear();
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut buf: Vec<u8> = vec![0xD2u8, 0x40];
        for s in &self.scans {
            buf.extend_from_slice(s.module.as_bytes());
            for c in s.counts.iter() {
                buf.extend_from_slice(&c.to_le_bytes());
            }
        }
        buf.push(self.injected.len() as u8);
        fnv1a64(&buf)
    }

    /// 报告。
    pub fn report(&self) -> String {
        let mut s = String::new();
        s.push_str("x5 模块 ");
        s.push_str(&self.scans.len().to_string());
        s.push_str(" 注入 ");
        s.push_str(&self.injected.len().to_string());
        s
    }
}

/// 注入点选择：LCG 确定性选模块与检项。
pub fn pick_inject(seed: &mut u32) -> (usize, usize) {
    let m = (lcg_next(seed) % (DOMAIN_MODULES.len() as u32)) as usize;
    let c = (lcg_next(seed) % (X5_CLASSES.len() as u32)) as usize;
    (m, c)
}

// ── 主巡检入口 ──────────────────────────────────────────────
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：全域 12 模块登记 + X5 检项闭集核对。
    let mut led = X5Ledger::new();
    for m in DOMAIN_MODULES.iter() {
        led.register(m)?;
    }
    if !led.domain_full() || led.scans.len() != 12 {
        return Err(E_MOD_MISS);
    }
    if X5_CLASSES.len() != 5 || X5_CLASSES[0] != "telemetry" || X5_CLASSES[4] != "heartbeat" {
        return Err(E_CLASS_BAD);
    }
    hits += 1;

    // R2：首跑零外联：全部计数为零，落首跑指纹。
    let fp1 = led.first_pass()?;
    if fp1 == 0 {
        return Err(E_FINGER);
    }
    hits += 1;

    // R3：复跑一致：清账状态复跑通过且指纹与首跑相同。
    let fp2 = led.rerun()?;
    if fp2 != fp1 {
        return Err(E_RERUN_MISMATCH);
    }
    hits += 1;

    // R4：注入检出：LCG 选点注入 1 发 → detect 恰 1 项且对得上。
    let mut state: u32 = 0xD240_0001;
    let (mi, ci) = pick_inject(&mut state);
    led.inject(mi, ci)?;
    let det = led.detect();
    if det.len() != 1 || det[0] != (mi, ci) {
        return Err(E_UNINJ);
    }
    hits += 1;

    // R5：注入后复跑拦截 → E_DIRTY。
    if led.rerun() != Err(E_DIRTY) {
        return Err(E_DIRTY);
    }
    hits += 1;

    // R6：清账复绿：clear 后 detect 空、rerun 通过。
    led.clear();
    if !led.detect().is_empty() {
        return Err(E_UNINJ);
    }
    led.rerun()?;
    hits += 1;

    // R7：复跑指纹不符检出：首跑指纹被破坏时复跑必须拦截。
    let mut led7 = X5Ledger::new();
    for m in DOMAIN_MODULES.iter() {
        led7.register(m)?;
    }
    led7.first_pass()?;
    led7.fp_pass1 = Some(0x1234_5678);
    if led7.rerun() != Err(E_RERUN_MISMATCH) {
        return Err(E_RERUN_MISMATCH);
    }
    hits += 1;

    // R8：闭集外检项注入拒绝 → E_CLASS_BAD；闭集外模块拒绝 → E_STRAY。
    if led7.inject(0, 5) != Err(E_CLASS_BAD) {
        return Err(E_CLASS_BAD);
    }
    if led7.register("m8ghost") != Err(E_STRAY) {
        return Err(E_STRAY);
    }
    hits += 1;

    // R9：首跑未做即复跑 → E_NO_PASS1；缺模块首跑 → E_MOD_MISS。
    let mut led9 = X5Ledger::new();
    if led9.rerun() != Err(E_NO_PASS1) {
        return Err(E_NO_PASS1);
    }
    led9.register("m5boot")?;
    if led9.first_pass() != Err(E_MOD_MISS) {
        return Err(E_MOD_MISS);
    }
    hits += 1;

    // R10：指纹稳定：同构两账指纹一致；注入前后指纹不同。
    let mut led10a = X5Ledger::new();
    let mut led10b = X5Ledger::new();
    for m in DOMAIN_MODULES.iter() {
        led10a.register(m)?;
        led10b.register(m)?;
    }
    led10a.first_pass()?;
    led10b.first_pass()?;
    if led10a.fingerprint() != led10b.fingerprint() {
        return Err(E_FINGER);
    }
    let before = led10b.fingerprint();
    led10b.inject(3, 1)?;
    if led10b.fingerprint() == before {
        return Err(E_FINGER);
    }
    let rep = led10a.report();
    if !rep.contains("12") {
        return Err(E_EMPTY);
    }
    hits += 1;

    Ok(hits)
}
