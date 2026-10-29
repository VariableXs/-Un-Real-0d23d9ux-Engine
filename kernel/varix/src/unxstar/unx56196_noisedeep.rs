//! UNX-F56196 启动噪声源登记深化件（0x61880 段）
//!
//! 锚点：UNX-F56196-J1 噪声源深化账（噪声源登记簿深化——源号 × 源
//! 名 × 噪声类型 × 影响段 × 处置策略逐源落账）；J2 登记语义（类型闭
//! 集 + 影响段合法 + 处置策略非空三性质）；J3 登记指纹（源账折叠指
//! 纹）三判据全绿。
//! 域：CoRun·UNX-F2-B40（启动域·噪声源登记深化面）。
//!
//! 结构：噪声类型闭集 + 影响段闭集 + 深化登记簿 + 登记核（类型闸 +
//! 段闸 + 策略闸）+ 登记指纹 + 注入核（未知类型/未知段/空策略/重源
//! 号/印后登记必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x61880 独占 0x20）。
pub const SEG_BASE: u32 = 0x61880;
pub const NZ_UNK_TYPE: u32 = SEG_BASE + 0x01; // 未知噪声类型
pub const NZ_UNK_SEG: u32 = SEG_BASE + 0x02; // 未知影响段
pub const NZ_EMPTY_POLICY: u32 = SEG_BASE + 0x03; // 空处置策略
pub const NZ_DUP_SRC: u32 = SEG_BASE + 0x04; // 重源号
pub const NZ_FROZEN: u32 = SEG_BASE + 0x05; // 印后登记

/// 噪声类型闭集。
pub const NT_TIMER_JIT: usize = 0; // 计时器抖动
pub const NT_DISK_CACHE: usize = 1; // 磁盘缓存扰动
pub const NT_THERMAL: usize = 2; // 热节流
pub const NT_COUNT: usize = 3;

pub const TYPE_NAMES: [&str; NT_COUNT] = ["timer-jitter", "disk-cache", "thermal"];

/// 影响段闭集（启动链四段）。
pub const NS_BOOT: usize = 0;
pub const NS_KERNEL: usize = 1;
pub const NS_LOGIN: usize = 2;
pub const NS_INTERACT: usize = 3;
pub const NS_COUNT: usize = 4;

pub const SEG_NAMES: [&str; NS_COUNT] = ["boot", "kernel", "login", "interact"];

/// 一条深化噪声源。
#[derive(Clone)]
pub struct NoiseSource {
    pub src_no: u32,
    pub name: String,
    pub ntype: usize,
    pub seg: usize,
    pub policy: String,
}

impl NoiseSource {
    /// 登记合法性：类型/段闭集 + 策略/名非空。
    pub fn entry_ok(&self) -> bool {
        self.ntype < NT_COUNT
            && self.seg < NS_COUNT
            && !self.policy.is_empty()
            && !self.name.is_empty()
    }

    fn render_line(&self) -> String {
        format!(
            "NS{}|{}|{}@{}|{}",
            self.src_no, self.name, TYPE_NAMES[self.ntype], SEG_NAMES[self.seg], self.policy
        )
    }
}

/// 深化噪声登记簿。
pub struct NoiseRegistry {
    srcs: Vec<NoiseSource>,
    sealed: bool,
}

impl NoiseRegistry {
    pub fn new() -> Self {
        NoiseRegistry {
            srcs: Vec::new(),
            sealed: false,
        }
    }

    /// 登记源：合法性闸 + 重号拒 + 印后拒。
    pub fn register(&mut self, src: NoiseSource) -> Result<(), u32> {
        if self.sealed {
            return Err(NZ_FROZEN);
        }
        if src.ntype >= NT_COUNT {
            return Err(NZ_UNK_TYPE);
        }
        if src.seg >= NS_COUNT {
            return Err(NZ_UNK_SEG);
        }
        if src.policy.is_empty() {
            return Err(NZ_EMPTY_POLICY);
        }
        if self.srcs.iter().any(|x| x.src_no == src.src_no) {
            return Err(NZ_DUP_SRC);
        }
        self.srcs.push(src);
        Ok(())
    }

    pub fn src(&self, no: u32) -> Option<&NoiseSource> {
        self.srcs.iter().find(|x| x.src_no == no)
    }

    pub fn len(&self) -> usize {
        self.srcs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.srcs.is_empty()
    }

    /// 按影响段查源。
    pub fn by_seg(&self, seg: usize) -> Vec<&NoiseSource> {
        self.srcs.iter().filter(|x| x.seg == seg).collect()
    }

    /// 各类型计数。
    pub fn type_counts(&self) -> [usize; NT_COUNT] {
        let mut c = [0usize; NT_COUNT];
        for x in &self.srcs {
            if x.ntype < NT_COUNT {
                c[x.ntype] += 1;
            }
        }
        c
    }

    /// 登记指纹。
    pub fn reg_fp(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for x in &self.srcs {
            h = h.wrapping_mul(0x0100_0193) ^ x.src_no;
            for b in x.name.bytes() {
                h = h.wrapping_mul(0x0100_0193) ^ b as u32;
            }
            h = h.wrapping_mul(0x0100_0193) ^ x.ntype as u32;
            h = h.wrapping_mul(0x0100_0193) ^ x.seg as u32;
            for b in x.policy.bytes() {
                h = h.wrapping_mul(0x0100_0193) ^ b as u32;
            }
        }
        h ^= self.sealed as u32;
        h = h.wrapping_mul(0x0100_0193);
        h
    }

    /// 封簿印。
    pub fn seal(&mut self) -> bool {
        if self.sealed || self.srcs.is_empty() || !self.srcs.iter().all(|x| x.entry_ok()) {
            return false;
        }
        self.sealed = true;
        true
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 渲染导出。
    pub fn render_reg(&self) -> String {
        let mut out = String::new();
        for x in &self.srcs {
            out.push_str(&x.render_line());
            out.push('\n');
        }
        if self.sealed {
            out.push_str("NZREG-SEAL\n");
        }
        out
    }
}

impl Default for NoiseRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：四源（三类齐 + kernel 两个热源）。
fn standard_reg() -> NoiseRegistry {
    let mut r = NoiseRegistry::new();
    let _ = r.register(NoiseSource {
        src_no: 1,
        name: "tsc-drift".to_string(),
        ntype: NT_TIMER_JIT,
        seg: NS_KERNEL,
        policy: "absorb-window".to_string(),
    });
    let _ = r.register(NoiseSource {
        src_no: 2,
        name: "page-cache-warm".to_string(),
        ntype: NT_DISK_CACHE,
        seg: NS_LOGIN,
        policy: "prefetch-warmup".to_string(),
    });
    let _ = r.register(NoiseSource {
        src_no: 3,
        name: "pkg-thermal".to_string(),
        ntype: NT_THERMAL,
        seg: NS_KERNEL,
        policy: "clock-clamp".to_string(),
    });
    let _ = r.register(NoiseSource {
        src_no: 4,
        name: "menu-keystroke".to_string(),
        ntype: NT_TIMER_JIT,
        seg: NS_BOOT,
        policy: "auto-select-timeout".to_string(),
    });
    r
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    let mk = |no: u32, nt: usize, seg: usize, policy: &str| NoiseSource {
        src_no: no,
        name: "n".to_string(),
        ntype: nt,
        seg,
        policy: policy.to_string(),
    };
    // 注 1：未知类型
    let mut r = NoiseRegistry::new();
    if r.register(mk(1, 9, NS_BOOT, "p")) == Err(NZ_UNK_TYPE) {
        caught += 1;
    }
    // 注 2：未知段
    let mut r = NoiseRegistry::new();
    if r.register(mk(1, NT_THERMAL, 9, "p")) == Err(NZ_UNK_SEG) {
        caught += 1;
    }
    // 注 3：空策略
    let mut r = NoiseRegistry::new();
    if r.register(mk(1, NT_THERMAL, NS_BOOT, "")) == Err(NZ_EMPTY_POLICY) {
        caught += 1;
    }
    // 注 4：重源号
    let mut r = standard_reg();
    if r.register(mk(1, NT_THERMAL, NS_BOOT, "p")) == Err(NZ_DUP_SRC) {
        caught += 1;
    }
    // 注 5：印后登记
    let mut r = standard_reg();
    let _ = r.seal();
    if r.register(mk(9, NT_THERMAL, NS_BOOT, "p")) == Err(NZ_FROZEN) {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 噪声源深化账；J2 登记语义；J3 登记指纹。
pub fn run_unx56196_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56196");
    // J1-1 四源全登记
    let r = standard_reg();
    let ok = r.len() == 4 && (1..=4).all(|n| r.src(n).is_some());
    s.add("56196-J1-四源全登记", ok, if ok { "" } else { "源账不全" });
    // J1-2 渲染导出齐
    let mut r2 = standard_reg();
    let _ = r2.seal();
    let text = r2.render_reg();
    let ok = text.contains("NS1|tsc-drift|timer-jitter@kernel|absorb-window")
        && text.contains("NS4|menu-keystroke|timer-jitter@boot|auto-select-timeout")
        && text.contains("NZREG-SEAL");
    s.add("56196-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 登记合法性（全部）
    let ok = (1..=4).all(|n| r.src(n).map(|x| x.entry_ok()).unwrap_or(false));
    s.add("56196-J2-登记合法性", ok, if ok { "" } else { "合法性漂移" });
    // J2-2 类型计数（jitter 2 / cache 1 / thermal 1）
    let tc = r.type_counts();
    let ok = tc == [2, 1, 1];
    let detail = if ok { String::new() } else { format!("{},{},{}", tc[0], tc[1], tc[2]) };
    s.add_dyn("56196-J2-类型计数", tc == [2, 1, 1], &detail);
    // J2-3 按段查源（kernel 2 个）
    let ok = r.by_seg(NS_KERNEL).len() == 2;
    s.add("56196-J2-按段查源", ok, if ok { "" } else { "段查漂移" });
    // J2-4 段闭集合法
    let ok = r.srcs.iter().all(|x| x.seg < NS_COUNT);
    s.add("56196-J2-段闭集", ok, if ok { "" } else { "段越界" });
    // J2-5 策略非空抽查
    let ok = r.src(3).map(|x| x.policy == "clock-clamp").unwrap_or(false);
    s.add("56196-J2-策略抽查", ok, if ok { "" } else { "策略漂移" });
    // J2-6 重号拒
    let mut r3 = standard_reg();
    let ok = r3
        .register(NoiseSource {
            src_no: 2,
            name: "dup".to_string(),
            ntype: NT_THERMAL,
            seg: NS_BOOT,
            policy: "p".to_string(),
        })
        == Err(NZ_DUP_SRC);
    s.add("56196-J2-重号拒", ok, if ok { "" } else { "重号闸失效" });
    // J3-1 登记指纹确定
    let ok = standard_reg().reg_fp() == standard_reg().reg_fp();
    s.add("56196-J3-指纹确定", ok, if ok { "" } else { "指纹漂移" });
    // J3-2 源变签变
    let mut r4 = standard_reg();
    let fp0 = r4.reg_fp();
    if let Some(x) = r4.srcs.first_mut() {
        x.policy.push('x');
    }
    let ok = r4.reg_fp() != fp0;
    s.add("56196-J3-源变签变", ok, if ok { "" } else { "折叠退化" });
    // J3-3 封簿印
    let mut r5 = standard_reg();
    let ok = r5.seal() && r5.sealed();
    s.add("56196-J3-封簿印", ok, if ok { "" } else { "封印失败" });
    // J3-4 印后拒登记
    let ok = r5
        .register(NoiseSource {
            src_no: 9,
            name: "late".to_string(),
            ntype: NT_THERMAL,
            seg: NS_BOOT,
            policy: "p".to_string(),
        })
        == Err(NZ_FROZEN);
    s.add("56196-J3-印后拒登记", ok, if ok { "" } else { "只读闸失效" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56196-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56196_all_checks_green() {
        let set = run_unx56196_checks();
        assert!(set.all_passed(), "UNX-F56196 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56196_reg_stable() {
        let mut r = standard_reg();
        assert!(r.seal());
    }
}
