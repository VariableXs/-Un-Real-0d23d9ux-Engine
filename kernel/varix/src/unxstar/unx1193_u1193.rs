// unx1193_u1193 —— UNX-F1193 亲和面人话化（策略与公平 explain 报告）
// 判据原文 UNX-F1193-J1：explain 报告全过（白名单字段制、两次生成逐字节
// 相同、隐私红线（只报计数不报事件号）、分级判定、指纹冻结）。
// 落位：kernel/varix/src/unxstar/；诊断段 0x8e70 独占（0x8e70~0x8eaf）。
//
// 设计（纯功能 no_std）：
//   * 快照：核数/迁移数/拦截数/基尼/alarms/预算 bps 六字段白名单；
//   * 确定性导出（稳定序拼接、无时戳）；分级（GREEN/YELLOW/RED）；
//   * FNV 指纹冻结（冻结后拒再生成）。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0x8e70 独占） ─────────────────────────────────────────────
pub const XC_EX2_SEALED: u32 = 0x8E71; // 冻结后再生成
pub const XC_EX2_BAD_FIELD: u32 = 0x8E72; // 越白名单
pub const XC_EX2_BAD_SNAP: u32 = 0x8E73; // 快照字段间关系破

/// 白名单字段数。
pub const EX2_FIELDS: usize = 6;

/// 亲和面快照。
#[derive(Clone, Copy)]
pub struct AffSnap {
    pub cores: u32,
    pub migrations: u32,
    pub blocked: u32,
    /// 基尼（千分比）。
    pub gini_pm: u32,
    /// 扰动立案数。
    pub alarms: u32,
    /// 健康 bps。
    pub health_bps: u32,
}

impl AffSnap {
    pub fn fields(&self) -> [u64; EX2_FIELDS] {
        [
            self.cores as u64,
            self.migrations as u64,
            self.blocked as u64,
            self.gini_pm as u64,
            self.alarms as u64,
            self.health_bps as u64,
        ]
    }
    pub fn valid(&self) -> bool {
        self.gini_pm <= 1000 && self.health_bps <= 10000
    }
}

/// explain 生成器。
pub struct AffExplainer {
    whitelist: [bool; EX2_FIELDS],
    frozen: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl AffExplainer {
    pub fn new() -> AffExplainer {
        AffExplainer { whitelist: [true; EX2_FIELDS], frozen: false, rejects: vec![] }
    }

    pub fn deny_field(&mut self, idx: usize) -> Result<(), u32> {
        if idx >= EX2_FIELDS {
            self.rejects.push((XC_EX2_BAD_FIELD, "越白名单"));
            return Err(XC_EX2_BAD_FIELD);
        }
        self.whitelist[idx] = false;
        Ok(())
    }

    /// 生成报告（确定性）。
    pub fn explain(&mut self, s: &AffSnap) -> Result<Vec<u8>, u32> {
        if self.frozen {
            self.rejects.push((XC_EX2_SEALED, "冻结后再生成"));
            return Err(XC_EX2_SEALED);
        }
        if !s.valid() {
            self.rejects.push((XC_EX2_BAD_SNAP, "快照字段关系破"));
            return Err(XC_EX2_BAD_SNAP);
        }
        let grade: &[u8] = if s.alarms == 0 && s.blocked == 0 {
            b"GREEN"
        } else if s.alarms + s.blocked <= 4 {
            b"YELLOW"
        } else {
            b"RED"
        };
        let mut out: Vec<u8> = vec![];
        out.extend_from_slice(b"== Affinity Explain ==\n");
        for (i, v) in s.fields().iter().enumerate() {
            if !self.whitelist[i] {
                self.rejects.push((XC_EX2_BAD_FIELD, "越白名单"));
                return Err(XC_EX2_BAD_FIELD);
            }
            match i {
                0 => out.extend_from_slice(b"cores="),
                1 => out.extend_from_slice(b"migrations="),
                2 => out.extend_from_slice(b"blocked="),
                3 => out.extend_from_slice(b"gini_pm="),
                4 => out.extend_from_slice(b"alarms="),
                _ => out.extend_from_slice(b"health_bps="),
            }
            out.extend_from_slice(&ae_utoa(*v));
            out.push(b'\n');
        }
        out.extend_from_slice(b"grade=");
        out.extend_from_slice(grade);
        out.push(b'\n');
        Ok(out)
    }

    pub fn freeze(&mut self, report: &[u8]) -> u64 {
        self.frozen = true;
        let mut h: u64 = 0x811C_9DC5;
        for &b in report {
            h ^= b as u64;
            h = h.wrapping_mul(0x0100_0193);
        }
        h
    }

    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
}

fn ae_utoa(mut v: u64) -> Vec<u8> {
    if v == 0 {
        return vec![b'0'];
    }
    let mut buf: Vec<u8> = vec![];
    while v > 0 {
        buf.push(b'0' + (v % 10) as u8);
        v /= 10;
    }
    buf.reverse();
    buf
}

/// 标准场景：快照 → 两次生成逐字节相同 + 分级 + 冻结。
pub fn explain2_scenario() -> Result<(usize, u64), u32> {
    let s = AffSnap { cores: 4, migrations: 12, blocked: 1, gini_pm: 120, alarms: 0, health_bps: 9500 };
    let mut e = AffExplainer::new();
    let r1 = e.explain(&s)?;
    let r2 = e.explain(&s)?;
    if r1 != r2 {
        return Err(XC_EX2_SEALED);
    }
    if ae_find(&r1, b"grade=YELLOW").is_none() {
        return Err(XC_EX2_BAD_FIELD);
    }
    let fp = e.freeze(&r1);
    Ok((r1.len(), fp))
}

fn ae_find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.len() > hay.len() {
        return None;
    }
    for i in 0..=(hay.len() - needle.len()) {
        if &hay[i..i + needle.len()] == needle {
            return Some(i);
        }
    }
    None
}

/// 注入核：五类破坏各注入一次，全部必抓。
pub fn inject_core() -> (u32, u32) {
    let mut caught = 0u32;
    const N: u32 = 5;

    // 1）冻结后再生成必拒
    {
        let mut e = AffExplainer::new();
        let s = AffSnap { cores: 1, migrations: 0, blocked: 0, gini_pm: 0, alarms: 0, health_bps: 10000 };
        let r = e.explain(&s).unwrap_or_default();
        let _ = e.freeze(&r);
        if e.explain(&s) == Err(XC_EX2_SEALED) {
            caught += 1;
        }
    }
    // 2）越白名单下标必拒
    {
        let mut e = AffExplainer::new();
        if e.deny_field(9) == Err(XC_EX2_BAD_FIELD) {
            caught += 1;
        }
    }
    // 3）deny 后生成必拒
    {
        let mut e = AffExplainer::new();
        let _ = e.deny_field(3);
        let s = AffSnap { cores: 1, migrations: 0, blocked: 0, gini_pm: 5, alarms: 0, health_bps: 10000 };
        if e.explain(&s) == Err(XC_EX2_BAD_FIELD) {
            caught += 1;
        }
    }
    // 4）快照关系破（基尼>1000）必拒
    {
        let mut e = AffExplainer::new();
        let s = AffSnap { cores: 1, migrations: 0, blocked: 0, gini_pm: 1001, alarms: 0, health_bps: 10000 };
        if e.explain(&s) == Err(XC_EX2_BAD_SNAP) {
            caught += 1;
        }
    }
    // 5）GREEN/RED 分级
    {
        let mut e = AffExplainer::new();
        let g = AffSnap { cores: 1, migrations: 0, blocked: 0, gini_pm: 0, alarms: 0, health_bps: 10000 };
        let r1 = e.explain(&g);
        let rd = AffSnap { cores: 4, migrations: 9, blocked: 9, gini_pm: 900, alarms: 9, health_bps: 100 };
        let r2 = e.explain(&rd);
        if let (Ok(a), Ok(b)) = (r1, r2) {
            if ae_find(&a, b"grade=GREEN").is_some() && ae_find(&b, b"grade=RED").is_some() {
                caught += 1;
            }
        }
    }
    (caught, N)
}
