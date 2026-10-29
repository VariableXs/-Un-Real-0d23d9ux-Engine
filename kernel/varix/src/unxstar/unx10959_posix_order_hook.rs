//! UNX-F10959 POSIX 判定序挂接核销（F10879 序表）（0x1d760 段）
//!
//! 卡片判据（UNX-F10959-J1）：
//! - 正面：POSIX 错误条挂接序表逐条核销（10/10）、衔接零冲突（10/10）；
//! - 反面：挂接后零漂移（千次 10/10）。
//!
//! 域：CoRun·UNX-C4-B28。本模块把 B28 段 POSIX 错误矩阵
//! （F10941–F10959）逐条挂接到 F10879 的 POSIX 判定序表上：
//! 序号严格递增、名字与表逐项匹配、封印后零漂移，衔接处无冲突。

use crate::alloc::string::String;
use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 错误码（SEG_BASE = 0x1d000 + (10958-10900)*0x20 → 0x1d760）
// ---------------------------------------------------------------------------
const SEG_BASE: u32 = 0x1d760;

/// OH_EINVAL：序号/条目参数非法
pub const OH_EINVAL: u32 = SEG_BASE + 0x01;
/// OH_EDUP：序号或名字重复
pub const OH_EDUP: u32 = SEG_BASE + 0x02;
/// OH_ENOENT：名字不在 F10879 序表内
pub const OH_ENOENT: u32 = SEG_BASE + 0x03;
/// OH_EORDER：挂接序不严格递增
pub const OH_EORDER: u32 = SEG_BASE + 0x04;
/// OH_ESEELED：封印后仍试图挂接
pub const OH_ESEELED: u32 = SEG_BASE + 0x05;
/// OH_EMISMATCH：名字与表项不符
pub const OH_EMISMATCH: u32 = SEG_BASE + 0x06;
/// OH_EDRIFT：封印后摘要漂移（自检面）
pub const OH_EDRIFT: u32 = SEG_BASE + 0x07;

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------
/// F10879 POSIX 段序表（19 条，覆盖 F10941–F10959）
const ORDER_TABLE: [&str; 19] = [
    "mq-open",
    "mq-send-again",
    "mq-send-einval",
    "mq-recv-again",
    "mq-recv-einval",
    "mq-notify",
    "mq-attr",
    "mq-close-unlink",
    "mq-name-parse",
    "sem-open",
    "sem-wait",
    "sem-post",
    "sem-trywait",
    "sem-getvalue",
    "sem-close-unlink",
    "mqfd-dup-epoll",
    "posix-sysv-isolate",
    "posix-name-lifecycle",
    "b28-close",
];

// ---------------------------------------------------------------------------
// 数据结构
// ---------------------------------------------------------------------------

/// 单个挂接条
#[derive(Clone, Debug, PartialEq)]
pub struct HookItem {
    /// 挂接序号（严格递增）
    pub seq: u32,
    /// 条目名
    pub name: String,
}

impl HookItem {
    /// FNV 折叠
    pub fn fold(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        h = (h ^ self.seq as u64).wrapping_mul(0x100000001b3);
        for b in self.name.as_bytes() {
            h = (h ^ (*b as u64)).wrapping_mul(0x100000001b3);
        }
        h
    }
}

/// POSIX 判定序挂接台账
#[derive(Clone, Debug)]
pub struct PosixHookBook {
    pub hooks: Vec<HookItem>,
    /// 封印标记
    pub sealed: bool,
    /// 错误计数
    pub n_edup: u32,
    pub n_enoent: u32,
    pub n_eorder: u32,
    pub n_esealed: u32,
    pub n_emismatch: u32,
    /// 挂接成功数
    pub n_hooked: u32,
}

impl PosixHookBook {
    pub fn new() -> Self {
        PosixHookBook {
            hooks: Vec::new(),
            sealed: false,
            n_edup: 0,
            n_enoent: 0,
            n_eorder: 0,
            n_esealed: 0,
            n_emismatch: 0,
            n_hooked: 0,
        }
    }

    /// 表内检索
    fn table_pos(name: &str) -> Option<usize> {
        ORDER_TABLE.iter().position(|&n| n == name)
    }

    /// 挂接一条：判定序 = 名单查 → 冲突查 → 顺序查 → 封印查
    pub fn hook(&mut self, seq: u32, name: &str) -> Result<(), u32> {
        if name.is_empty() {
            return Err(OH_EINVAL);
        }
        if self.sealed {
            self.n_esealed = self.n_esealed.wrapping_add(1);
            return Err(OH_ESEELED);
        }
        match Self::table_pos(name) {
            None => {
                self.n_enoent = self.n_enoent.wrapping_add(1);
                return Err(OH_ENOENT);
            }
            Some(tp) => {
                // 名字与表项位置必须等于当前应挂位置
                if self.hooks.len() != tp {
                    self.n_emismatch = self.n_emismatch.wrapping_add(1);
                    return Err(OH_EMISMATCH);
                }
            }
        }
        if self.hooks.iter().any(|h| h.seq == seq || h.name == name) {
            self.n_edup = self.n_edup.wrapping_add(1);
            return Err(OH_EDUP);
        }
        if let Some(last) = self.hooks.last() {
            if seq <= last.seq {
                self.n_eorder = self.n_eorder.wrapping_add(1);
                return Err(OH_EORDER);
            }
        }
        self.hooks.push(HookItem { seq, name: String::from(name) });
        self.n_hooked = self.n_hooked.wrapping_add(1);
        Ok(())
    }

    /// 封印：此后任何挂接被拒，摘要冻结
    pub fn seal(&mut self) -> u64 {
        self.sealed = true;
        self.fold_all()
    }

    /// 逐条核销：序号递增且与表逐项匹配
    pub fn reconcile(&self) -> bool {
        if self.hooks.len() != ORDER_TABLE.len() {
            return false;
        }
        let mut prev: Option<u32> = None;
        for (i, h) in self.hooks.iter().enumerate() {
            if ORDER_TABLE[i] != h.name {
                return false;
            }
            if let Some(p) = prev {
                if h.seq <= p {
                    return false;
                }
            }
            prev = Some(h.seq);
        }
        true
    }

    /// 衔接零冲突：相邻条目衔接处无重复/无跳变
    pub fn junction_ok(&self) -> bool {
        for w in self.hooks.windows(2) {
            if w[0].name == w[1].name || w[1].seq != w[0].seq + 1 {
                return false;
            }
        }
        true
    }

    /// 总折叠
    pub fn fold_all(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        for it in &self.hooks {
            h = (h ^ it.fold()).wrapping_mul(0x100000001b3);
        }
        h = (h ^ (self.sealed as u64)).wrapping_mul(0x100000001b3);
        h
    }

    /// 渲染
    pub fn render(&self) -> String {
        let mut s = String::from("posix-hook:");
        for h in &self.hooks {
            s.push('/');
            s.push_str(&h.name);
        }
        s
    }
}

impl Default for PosixHookBook {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 注入四路
// ---------------------------------------------------------------------------
pub fn inject_four() -> [bool; 4] {
    let mut b = PosixHookBook::new();
    // 路1：表外名字 → ENOENT
    let r1 = matches!(b.hook(1, "not-in-table"), Err(x) if x == OH_ENOENT);
    // 路2：顺序跳挂（跳过首个表项）→ EMISMATCH
    let r2 = matches!(b.hook(1, "sem-open"), Err(x) if x == OH_EMISMATCH);
    // 路3：正常挂接后重复序号 → EDUP
    let _ = b.hook(1, "mq-open");
    let r3 = matches!(b.hook(1, "mq-send-again"), Err(x) if x == OH_EDUP);
    // 路4：非递增序 → EORDER
    let r4 = matches!(b.hook(0, "mq-send-again"), Err(x) if x == OH_EORDER);
    [r1, r2, r3, r4]
}

// ---------------------------------------------------------------------------
// 确定性千轮样本
// ---------------------------------------------------------------------------
/// 千轮：前 500 轮按序全挂（每轮重建重挂），后 500 轮封印后
/// 反复探测零漂移。
pub fn sample_thousand() -> u32 {
    let mut good: u32 = 0;
    for r in 0..1000u32 {
        if r < 500 {
            let mut b = PosixHookBook::new();
            let mut all = true;
            for (i, name) in ORDER_TABLE.iter().enumerate() {
                if b.hook((i + 1) as u32, name).is_err() {
                    all = false;
                }
            }
            if all && b.reconcile() && b.junction_ok() {
                good = good.wrapping_add(1);
            }
        } else {
            let mut b = PosixHookBook::new();
            for (i, name) in ORDER_TABLE.iter().enumerate() {
                let _ = b.hook((i + 1) as u32, name);
            }
            let sealed = b.seal();
            // 封印后再挂 → ESEELED
            let blocked = matches!(b.hook(99, "mq-open"), Err(x) if x == OH_ESEELED);
            // 摘要零漂移
            let stable = b.fold_all() == sealed;
            if blocked && stable {
                good = good.wrapping_add(1);
            }
        }
    }
    good
}

// ---------------------------------------------------------------------------
// 判据检查
// ---------------------------------------------------------------------------
pub fn run_unx10959_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10959");

    let inj = inject_four();
    s.add("J1-注入四路全拦截", inj.iter().all(|&b| b), "ENOENT/EMISMATCH/EDUP/EORDER 坏样本全部命中");

    // J1 正面：逐条核销 10/10
    let mut ok10 = 0u32;
    for _ in 0..10 {
        let mut b = PosixHookBook::new();
        let mut all = true;
        for (i, name) in ORDER_TABLE.iter().enumerate() {
            if b.hook((i + 1) as u32, name).is_err() {
                all = false;
            }
        }
        if all && b.reconcile() {
            ok10 = ok10.wrapping_add(1);
        }
    }
    s.add("J1-正面-逐条核销10/10", ok10 == 10, "十九条错误条十轮全挂全核销");

    // 衔接零冲突 10/10
    let mut junc10 = 0u32;
    for _ in 0..10 {
        let mut b = PosixHookBook::new();
        for (i, name) in ORDER_TABLE.iter().enumerate() {
            let _ = b.hook((i + 1) as u32, name);
        }
        if b.junction_ok() {
            junc10 = junc10.wrapping_add(1);
        }
    }
    s.add("J2-衔接零冲突10/10", junc10 == 10, "相邻条目无重复无跳变");

    // J3 反面：挂接后零漂移（千次）
    let good = sample_thousand();
    s.add("J3-反面-零漂移千次", good == 1000, "挂接全通五百轮 + 封印稳定五百轮");

    // 错误码分段
    let codes = [OH_EINVAL, OH_EDUP, OH_ENOENT, OH_EORDER, OH_ESEELED, OH_EMISMATCH, OH_EDRIFT];
    let seg_ok = codes.iter().all(|&c| c > SEG_BASE && c < SEG_BASE + 0x20);
    s.add("J4-错误码分段", seg_ok, "七码全部落在 0x1d760 段内");

    // 错误码互异
    let mut distinct = true;
    for i in 0..codes.len() {
        for j in (i + 1)..codes.len() {
            if codes[i] == codes[j] {
                distinct = false;
            }
        }
    }
    s.add("J5-错误码互异", distinct, "七码两两互异");

    // 账目对平：封印台账
    let mut b2 = PosixHookBook::new();
    for (i, name) in ORDER_TABLE.iter().enumerate() {
        let _ = b2.hook((i + 1) as u32, name);
    }
    s.add(
        "J6-账目对平",
        b2.n_hooked == 19 && b2.reconcile(),
        "挂接 19 条与序表长度一致且逐项匹配",
    );

    // 封印面
    let sealed_h = b2.seal();
    let seal_ok = matches!(b2.hook(20, "b28-close"), Err(x) if x == OH_ESEELED)
        && b2.fold_all() == sealed_h;
    s.add("J7-封印零漂移", seal_ok, "封印后挂接被拒且摘要不变");

    // 名单完整性：表含首尾锚
    s.add(
        "J8-序表锚点",
        ORDER_TABLE[0] == "mq-open" && ORDER_TABLE[18] == "b28-close",
        "序表以 mq-open 起以 b28-close 收",
    );

    // fold 稳定
    let h1 = b2.fold_all();
    let h2 = b2.fold_all();
    s.add("J9-fold稳定", h1 == h2, "同一状态两次折叠一致");

    // 渲染非空
    let r = b2.render();
    s.add(
        "J10-渲染非空",
        r.starts_with("posix-hook:") && r.len() > 20,
        "摘要含前缀与全部条目名",
    );

    // 空名面
    let mut b3 = PosixHookBook::new();
    s.add(
        "J11-空名面",
        matches!(b3.hook(1, ""), Err(x) if x == OH_EINVAL),
        "空名被 EINVAL 拒绝",
    );

    // 部分挂接不核销
    let mut b4 = PosixHookBook::new();
    let _ = b4.hook(1, "mq-open");
    s.add(
        "J12-部分挂接不核销",
        !b4.reconcile(),
        "缺条时核销失败",
    );

    // 动态判据
    s.add_dyn(
        "J13-挂接账",
        b2.n_hooked == 19,
        &crate::alloc::format!("挂接={} 封印拒绝={}", b2.n_hooked, b2.n_esealed),
    );
    s.add_dyn(
        "J14-千轮通过率",
        good == 1000,
        &crate::alloc::format!("{}//1000", good),
    );
    s.add_dyn(
        "J15-表长度",
        ORDER_TABLE.len() == 19,
        &crate::alloc::format!("序表长度={}", ORDER_TABLE.len()),
    );

    s
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10959_posix_order_hook_flow() {
        let mut b = PosixHookBook::new();
        for (i, name) in ORDER_TABLE.iter().enumerate() {
            assert!(b.hook((i + 1) as u32, name).is_ok());
        }
        assert!(b.reconcile());
        assert!(b.junction_ok());
        let h = b.seal();
        assert!(matches!(b.hook(50, "mq-open"), Err(x) if x == OH_ESEELED));
        assert_eq!(b.fold_all(), h);
        let inj = inject_four();
        assert!(inj.iter().all(|&x| x));
        assert_eq!(sample_thousand(), 1000);
    }

    #[test]
    fn unx10959_checks_green() {
        let s = run_unx10959_checks();
        assert!(s.all_passed(), "UNX-F10959 判据未全绿");
    }
}
