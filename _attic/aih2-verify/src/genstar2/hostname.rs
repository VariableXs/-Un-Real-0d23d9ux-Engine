//! F479 主机名与设备名（genstar2 · I 域通用·二分队 · AI-U2）。
//!
//! 主册判据（验收标准第一句）：
//! **默认名规则；改名校验与生效；三处同源审计；F321 发现列表联动；非法
//! 字符拒绝用例。**
//!
//! 功能定义（主册批次三）：默认名规则化（VARIX-随机四码，防尴尬默认名）；
//! 改名向导（合法字符校验/长度限制/即时生效说明——「改名后局域网设备看到
//! 的名字会变，F321 就近共享发现列表同步更新」）；名字显示三处（系统信息页
//! F199/就近共享 F321/网络浮层 F242）同源。
//!
//! 零堆纪律：定长名称缓冲，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 名称长度限制（1-15，与主流局域网发现协议兼容）。
pub const NAME_LEN_MIN: usize = 1;
pub const NAME_LEN_MAX: usize = 15;
/// 三处显示面（系统信息页 F199 / 就近共享 F321 / 网络浮层 F242）。
pub const DISPLAY_SURFACES: usize = 3;

/// 默认名生成：VARIX-XXXX（四码取 0-9A-Z 去易混淆字符——防尴尬也防误读）。
/// 种子注入（内核无 rand——种子来自硬件熵池；此处为纯函数便于验收）。
pub fn default_name(seed: u32) -> [u8; 10] {
    const ALPHABET: &[u8] = b"23456789ABCDEFGHJKMNPQRSTUVWXYZ"; // 去 0/O/1/I/L
    let mut out = *b"VARIX-0000";
    let mut s = seed;
    for i in 0..4 {
        s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223); // LCG
        out[6 + i] = ALPHABET[((s >> 16) as usize) % ALPHABET.len()];
    }
    out
}

/// 改名校验：字母/数字（含中文等 Unicode 字母）+ 连字符/下划线；字节长度
/// 1-15（与主流局域网发现协议兼容）；首尾不得为连字符。
pub fn validate_name(name: &str) -> Result<(), &'static str> {
    if name.is_empty() {
        return Err("名字不能为空");
    }
    if name.len() > NAME_LEN_MAX {
        return Err("名字最长 15 个字符");
    }
    let b = name.as_bytes();
    if b[0] == b'-' || b[b.len() - 1] == b'-' {
        return Err("名字不能以连字符开头或结尾");
    }
    for c in name.chars() {
        if !(c.is_alphanumeric() || c == '-' || c == '_') {
            return Err("只能使用字母、数字、连字符或下划线");
        }
    }
    Ok(())
}

/// 设备名状态（三处同源——单一真相存储，三面只读投影）。
pub struct DeviceName {
    name: [u8; NAME_LEN_MAX],
    n: usize,
    /// 三面同源位图（改名的显示面同步账）。
    synced: [bool; DISPLAY_SURFACES],
}

impl DeviceName {
    /// 出厂默认名（VARIX-四码）。
    pub fn with_default(seed: u32) -> Self {
        let buf = default_name(seed);
        let mut name = [0u8; NAME_LEN_MAX];
        name[..10].copy_from_slice(&buf);
        DeviceName {
            name,
            n: 10,
            synced: [true; DISPLAY_SURFACES],
        }
    }

    pub fn name_str(&self) -> &str {
        core::str::from_utf8(&self.name[..self.n]).unwrap_or("")
    }

    /// 改名（校验 + 即时生效 + F321 发现列表同步——三面失效重刷）。
    pub fn rename(&mut self, new_name: &str) -> Result<(), &'static str> {
        validate_name(new_name)?;
        let b = new_name.as_bytes();
        self.name = [0; NAME_LEN_MAX];
        self.name[..b.len()].copy_from_slice(b);
        self.n = b.len();
        self.synced = [false; DISPLAY_SURFACES]; // 三处全失效待刷
        Ok(())
    }

    /// 面同步消费（F199/F321/F242 各自刷新一次；返回是否确有待刷）。
    pub fn consume_sync(&mut self, s: usize) -> bool {
        let i = s % DISPLAY_SURFACES;
        let pending = !self.synced[i];
        self.synced[i] = true;
        pending
    }

    /// 三处同源审计（主册：三处显示永不打架——同源位图全真）。
    pub fn same_source_audit(&self) -> bool {
        self.synced.iter().all(|&x| x)
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

pub fn run_hostname_checks() -> CheckSet {
    let mut cs = CheckSet::new("F479-hostname");
    // 1) 默认名规则（VARIX-四码；四码字符表无易混淆字符）。
    let n1 = default_name(42);
    let n2 = default_name(43);
    cs.add("default_format", n1.len() == 10 && &n1[..6] == b"VARIX-", "");
    cs.add("default_varies", n1 != n2, "");
    cs.add("default_no_ambiguous", n1[6..].iter().all(|c| !b"0O1IL".contains(c)), "");
    // 2) 改名校验与生效。
    let mut d = DeviceName::with_default(7);
    cs.add("rename_ok", d.rename("我的星舰-x9").is_ok() && d.name_str() == "我的星舰-x9", "");
    // 3) 非法字符拒绝用例（主册：非法字符拒绝）。
    cs.add("reject_space", d.rename("my star").is_err(), "");
    cs.add("reject_empty", d.rename("").is_err(), "");
    cs.add("reject_too_long", d.rename("1234567890123456").is_err(), "");
    cs.add("reject_edge_dash", d.rename("-lead") .is_err() && d.rename("trail-").is_err(), "");
    // 4) 改名后三面失效并逐面刷新（F321 发现列表联动）。
    d.rename("nova").ok();
    cs.add("rename_invalidates_three", (0..3).all(|s| d.consume_sync(s)), "");
    cs.add("same_source_after_sync", d.same_source_audit(), "");
    // 5) 三处同源（单一存储——三面只读投影同一名）。
    let d2 = DeviceName::with_default(99);
    cs.add("single_source", d2.name_str().len() == 10 && d2.name_str().starts_with("VARIX-"), "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_names_unique_across_seeds() {
        let mut last = default_name(0);
        for seed in 1..100u32 {
            let n = default_name(seed);
            assert_ne!(n, last, "相邻种子不得同名");
            assert_eq!(&n[..6], b"VARIX-");
            last = n;
        }
    }

    #[test]
    fn rename_validation_matrix() {
        let ok = ["my-ship", "Star9", "a", "x_y", "端脑-01"];
        let bad = ["", " has space", "-x", "y-", "1234567890123456", "bad$name"];
        let mut d = DeviceName::with_default(1);
        for n in ok {
            assert!(d.rename(n).is_ok(), "应通过: {n}");
        }
        for n in bad {
            assert!(d.rename(n).is_err(), "应拒绝: {n}");
        }
    }

    #[test]
    fn discovery_list_follows_rename() {
        let mut d = DeviceName::with_default(5);
        d.rename("workbench").ok();
        // F321 就近共享面刷新时拿到新名（同源）；三面全部刷新后才算同步完成。
        for s in 0..DISPLAY_SURFACES {
            assert!(d.consume_sync(s));
        }
        assert!(d.same_source_audit());
    }
}
