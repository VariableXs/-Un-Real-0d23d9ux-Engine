// unx294_panicrepro.rs ↔ UNX-F0294 panic 复现模式（重启自动复现开关）
//
// 判据原文：UNX-F0294-J1 cmdline 开关生效（同参数自动重启），复现计数与门限齐。
// 难点分析：复现开关必须从 cmdline 解析（引导早期可用），且要有门限——
//          无限复现循环是灾难。计数持久化到停机戳才能跨重启。
// 纪律约束：cmdline 开关生效；复现计数与门限；超限自动关闭。
// 成本记账：解析 O(cmdline_len)；计数 O(1)。

#![allow(dead_code)]

use alloc::vec::Vec;

/// cmdline 开关关键字。
pub const REPRO_KEY: &str = "varix.panic_repro=";
/// 计数关键字（持久化）。
pub const REPRO_COUNT_KEY: &str = "varix.panic_repro_count=";
/// 门限关键字。
pub const REPRO_LIMIT_KEY: &str = "varix.panic_repro_limit=";

/// 复现模式状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReproMode {
    /// 关闭。
    Off,
    /// 开启（自动复现）。
    Armed,
    /// 达到门限自动关闭。
    Exhausted,
}

/// 复现配置。
#[derive(Debug, Clone)]
pub struct ReproConfig {
    pub mode: ReproMode,
    /// 已复现次数。
    pub count: u32,
    /// 门限（超过即关闭）。
    pub limit: u32,
    /// 复现参数指纹（同参数才自动重启）。
    pub fingerprint: u64,
}

/// cmdline 解析器。
pub struct CmdlineParser {
    /// 解析结果。
    config: ReproConfig,
    /// 解析错误。
    errors: Vec<&'static str>,
}

impl CmdlineParser {
    /// 新建。
    pub fn new() -> Self {
        CmdlineParser {
            config: ReproConfig {
                mode: ReproMode::Off,
                count: 0,
                limit: 3,
                fingerprint: 0,
            },
            errors: Vec::new(),
        }
    }

    /// 解析 cmdline 字符串（空格分隔）。
    ///
    /// 语法：`varix.panic_repro=1 varix.panic_repro_count=2 varix.panic_repro_limit=5`
    pub fn parse(&mut self, cmdline: &str) -> &ReproConfig {
        for token in cmdline.split_whitespace() {
            if let Some(v) = token.strip_prefix(REPRO_KEY) {
                self.config.mode = if v == "1" || v == "on" {
                    ReproMode::Armed
                } else {
                    ReproMode::Off
                };
            } else if let Some(v) = token.strip_prefix(REPRO_COUNT_KEY) {
                match v.parse::<u32>() {
                    Ok(n) => self.config.count = n,
                    Err(_) => self.errors.push("count 解析失败"),
                }
            } else if let Some(v) = token.strip_prefix(REPRO_LIMIT_KEY) {
                match v.parse::<u32>() {
                    Ok(n) => self.config.limit = n.max(1),
                    Err(_) => self.errors.push("limit 解析失败"),
                }
            }
        }
        // 门限检查。
        if self.config.count >= self.config.limit {
            self.config.mode = ReproMode::Exhausted;
        }
        &self.config
    }

    /// 指纹计算（同参数自动重启的判据）。
    ///
    /// FNV-1a 64bit。
    pub fn compute_fingerprint(params: &str) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in params.as_bytes() {
            h ^= *b as u64;
            h = h.wrapping_mul(0x100_0000_01b3);
        }
        h
    }

    /// 记录一次复现（计数+1，检查门限）。
    pub fn record_repro(&mut self) -> ReproMode {
        self.config.count += 1;
        if self.config.count >= self.config.limit {
            self.config.mode = ReproMode::Exhausted;
        }
        self.config.mode
    }

    /// 当前配置。
    pub fn config(&self) -> &ReproConfig {
        &self.config
    }

    /// 是否应自动重启复现。
    pub fn should_reboot(&self, current_fingerprint: u64) -> bool {
        self.config.mode == ReproMode::Armed
            && self.config.count < self.config.limit
            && current_fingerprint == self.config.fingerprint
    }

    /// 设指纹。
    pub fn set_fingerprint(&mut self, fp: u64) {
        self.config.fingerprint = fp;
    }

    /// 解析错误。
    pub fn errors(&self) -> &[&'static str] {
        &self.errors
    }
}

impl Default for CmdlineParser {
    fn default() -> Self {
        Self::new()
    }
}

/// 复现计数持久化（停机戳三要素之一）。
#[derive(Debug, Clone, Copy)]
pub struct ReproPersist {
    pub count: u32,
    pub limit: u32,
    pub fingerprint: u64,
}

impl ReproPersist {
    /// 编码为 u128（停机戳可存）。
    pub fn encode(&self) -> u128 {
        ((self.count as u128) << 96) | ((self.limit as u128) << 64) | self.fingerprint as u128
    }
    /// 解码。
    pub fn decode(v: u128) -> Self {
        ReproPersist {
            count: (v >> 96) as u32,
            limit: ((v >> 64) & 0xFFFF_FFFF) as u32,
            fingerprint: (v & 0xFFFF_FFFF_FFFF_FFFF) as u64,
        }
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

use super::super::checks::CheckSet;

pub fn run_unx294_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0294 panic 复现模式");

    // --- cmdline 开关生效 ---
    let mut p = CmdlineParser::new();
    let cfg = p.parse("console=serial varix.panic_repro=1 varix.panic_repro_limit=5");
    cs.add("开关生效", cfg.mode == ReproMode::Armed, "repro=1 → Armed");
    cs.add("门限解析", cfg.limit == 5, "limit=5");

    // --- 计数解析 ---
    let mut p2 = CmdlineParser::new();
    let cfg2 = p2.parse("varix.panic_repro=1 varix.panic_repro_count=2 varix.panic_repro_limit=3");
    cs.add("计数解析", cfg2.count == 2, "count=2");

    // --- 门限触顶 ---
    let mut p3 = CmdlineParser::new();
    p3.parse("varix.panic_repro=1 varix.panic_repro_count=3 varix.panic_repro_limit=3");
    cs.add("门限触顶 → Exhausted", p3.config().mode == ReproMode::Exhausted, "count>=limit");

    // --- 记录复现 ---
    let mut p4 = CmdlineParser::new();
    p4.parse("varix.panic_repro=1 varix.panic_repro_limit=2");
    let m1 = p4.record_repro();
    cs.add("第一次复现仍 Armed", m1 == ReproMode::Armed, "count=1<2");
    let m2 = p4.record_repro();
    cs.add("第二次触顶 Exhausted", m2 == ReproMode::Exhausted, "count=2>=2");

    // --- 指纹 ---
    let fp1 = CmdlineParser::compute_fingerprint("param_a=1");
    let fp2 = CmdlineParser::compute_fingerprint("param_a=1");
    let fp3 = CmdlineParser::compute_fingerprint("param_a=2");
    cs.add("同参指纹一致", fp1 == fp2, "确定性");
    cs.add("异参指纹不同", fp1 != fp3, "可区分");

    // --- should_reboot ---
    let mut p5 = CmdlineParser::new();
    p5.parse("varix.panic_repro=1 varix.panic_repro_limit=3");
    p5.set_fingerprint(fp1);
    cs.add("同参应重启", p5.should_reboot(fp1), "指纹匹配");
    cs.add("异参不重启", !p5.should_reboot(fp3), "指纹不匹配");

    // --- 持久化编解码 ---
    let rp = ReproPersist { count: 2, limit: 5, fingerprint: fp1 };
    let enc = rp.encode();
    let dec = ReproPersist::decode(enc);
    cs.add("持久化 roundtrip", dec.count == 2 && dec.limit == 5, "计数门限保留");
    cs.add("指纹 roundtrip", dec.fingerprint == fp1, "指纹保留");

    // --- 关闭模式 ---
    let mut p6 = CmdlineParser::new();
    let cfg6 = p6.parse("console=serial");
    cs.add("默认关闭", cfg6.mode == ReproMode::Off, "无开关");

    cs
}
