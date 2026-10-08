//! F179 权限审计 · 批次七深化（v7）——异常评分卡、授权到期调度、
//! 导出脱敏面、审计完整性校验帧。零堆、no_std。

use crate::checks::CheckSet;

/// 异常评分上限。
pub const SCORE_MAX: u32 = 1_000;
/// 评分黄线（审计员要看的门槛）。
pub const SCORE_WARN: u32 = 300;
/// 评分红线。
pub const SCORE_CRIT: u32 = 700;
/// 到期调度槽容量。
pub const EXPIRY_CAP: usize = 12;
/// 默认授权有效期（天）。
pub const GRANT_TTL_DAYS: u32 = 90;

/// 异常评分卡：三因子加权（深夜操作、频繁拒绝、越权尝试）。
/// 权重：深夜 100/次、拒 20/次、越权 250/次——封顶 1000。
#[derive(Clone, Copy)]
pub struct AnomalyScorer {
    night_ops: u32,
    denials: u32,
    overrides: u32,
}

impl AnomalyScorer {
    pub const fn new() -> AnomalyScorer {
        AnomalyScorer { night_ops: 0, denials: 0, overrides: 0 }
    }

    pub fn on_night_op(&mut self) {
        self.night_ops += 1;
    }
    pub fn on_denial(&mut self) {
        self.denials += 1;
    }
    pub fn on_override(&mut self) {
        self.overrides += 1;
    }

    /// 评分（u64 中间量防溢出，封顶 SCORE_MAX）。
    pub fn score(&self) -> u32 {
        let raw = self.night_ops as u64 * 100 + self.denials as u64 * 20 + self.overrides as u64 * 250;
        raw.min(SCORE_MAX as u64) as u32
    }

    /// 分级：0 绿 / 黄线起 1 / 红线起 2（恰点归高档）。
    pub fn grade(&self) -> u8 {
        let s = self.score();
        if s >= SCORE_CRIT {
            2
        } else if s >= SCORE_WARN {
            1
        } else {
            0
        }
    }

    /// 归因分解：哪个因子贡献最大（评分不说理由 = 白评）。
    pub fn top_factor(&self) -> u8 {
        let n = self.night_ops as u64 * 100;
        let d = self.denials as u64 * 20;
        let o = self.overrides as u64 * 250;
        if o >= n && o >= d {
            2 // 越权
        } else if n >= d {
            0 // 深夜
        } else {
            1 // 拒绝
        }
    }
}

/// 授权到期调度：授权 (应用, 到期日) 环，按日扫描到期清单。
#[derive(Clone, Copy)]
pub struct ExpiryScheduler {
    app_ids: [u32; EXPIRY_CAP],
    expire_days: [u32; EXPIRY_CAP],
    pub n: usize,
}

impl ExpiryScheduler {
    pub const fn new() -> ExpiryScheduler {
        ExpiryScheduler { app_ids: [0; EXPIRY_CAP], expire_days: [0; EXPIRY_CAP], n: 0 }
    }

    /// 授予：到期日 = 当日 + TTL。同应用续授 → 原位刷新（不双账）。
    pub fn grant(&mut self, app_id: u32, today: u32) -> bool {
        let exp = today + GRANT_TTL_DAYS;
        for i in 0..self.n {
            if self.app_ids[i] == app_id {
                self.expire_days[i] = exp;
                return true;
            }
        }
        if self.n >= EXPIRY_CAP {
            return false;
        }
        self.app_ids[self.n] = app_id;
        self.expire_days[self.n] = exp;
        self.n += 1;
        true
    }

    /// 当日到期名单（填 out 返回数量）。
    pub fn due_today(&self, today: u32, out: &mut [u32]) -> usize {
        let mut k = 0;
        for i in 0..self.n {
            if self.expire_days[i] == today && k < out.len() {
                out[k] = self.app_ids[i];
                k += 1;
            }
        }
        k
    }

    /// 已过期但未清理的悬挂授权数（审计遗漏面）。
    pub fn lapsed_uncleaned(&self, today: u32) -> u32 {
        let mut c = 0;
        for i in 0..self.n {
            if self.expire_days[i] < today {
                c += 1;
            }
        }
        c
    }

    /// 清理到期授权（压实——不留悬挂账）。
    pub fn sweep(&mut self, today: u32) -> u32 {
        let mut removed = 0;
        let mut i = 0;
        while i < self.n {
            if self.expire_days[i] <= today {
                for j in i..self.n - 1 {
                    self.app_ids[j] = self.app_ids[j + 1];
                    self.expire_days[j] = self.expire_days[j + 1];
                }
                self.n -= 1;
                removed += 1;
            } else {
                i += 1;
            }
        }
        removed
    }
}

/// 导出脱敏：应用名只留首字符 + 通配（a** → 身份可辨度最低但可读）。
/// 返回写入长度（out 定长 16B）。
pub fn pseudonymize_app(name: &[u8], out: &mut [u8; 16]) -> usize {
    if name.is_empty() {
        out[0] = b'?';
        return 1;
    }
    out[0] = name[0];
    out[1] = b'*';
    out[2] = b'*';
    3
}

/// 审计完整性帧（10B）：
/// [0..2) "AE" · [2..4) 事件数 LE · [4..6) 越权数 LE · [6..8) 天数 LE ·
/// [8..10) 校验和（前 8B FNV-16）。
pub fn fnv16(data: &[u8]) -> u16 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in data {
        h = (h ^ b as u32).wrapping_mul(0x0100_0193);
    }
    (h & 0xFFFF) as u16
}

pub fn encode_integrity(events: u16, overrides: u16, days: u16, out: &mut [u8; 10]) -> bool {
    if overrides > events {
        return false; // 越权数 > 事件数 = 账不平（构造性矛盾拒绝）
    }
    out[0] = b'A';
    out[1] = b'E';
    out[2..4].copy_from_slice(&events.to_le_bytes());
    out[4..6].copy_from_slice(&overrides.to_le_bytes());
    out[6..8].copy_from_slice(&days.to_le_bytes());
    let c = fnv16(&out[..8]);
    out[8] = (c & 0xFF) as u8;
    out[9] = (c >> 8) as u8;
    true
}

pub fn decode_integrity(frame: &[u8; 10]) -> Option<(u16, u16, u16)> {
    if frame[0] != b'A' || frame[1] != b'E' {
        return None;
    }
    let want = (frame[9] as u16) << 8 | frame[8] as u16;
    if fnv16(&frame[..8]) != want {
        return None;
    }
    Some((
        u16::from_le_bytes(frame[2..4].try_into().ok()?),
        u16::from_le_bytes(frame[4..6].try_into().ok()?),
        u16::from_le_bytes(frame[6..8].try_into().ok()?),
    ))
}

#[inline(never)]
pub fn run_permaudit_b7_checks() -> CheckSet {
    let mut cs = CheckSet::new("F179-b7");

    // 1) 评分算术：1 深夜 + 5 拒 + 1 越权 = 100+100+250 = 450（黄段）。
    let mut s = AnomalyScorer::new();
    s.on_night_op();
    for _ in 0..5 {
        s.on_denial();
    }
    s.on_override();
    cs.add("score_arithmetic", s.score() == 450 && s.grade() == 1, "");

    // 2) 封顶：海量事件不破 1000（评分有界——不搞 99999 分吓人）。
    let mut s2 = AnomalyScorer::new();
    for _ in 0..20 {
        s2.on_override();
    }
    cs.add("score_capped", s2.score() == SCORE_MAX && s2.grade() == 2, "");

    // 3) 红线恰点：700 整 = 红、699 = 黄（线值归属高档）。
    let mut s3 = AnomalyScorer::new();
    s3.on_override(); // 250
    s3.on_override(); // 500
    let g500 = s3.grade();
    s3.on_night_op(); // 600
    s3.on_night_op(); // 700
    let g700 = s3.grade();
    cs.add("score_line_exact", g500 == 1 && g700 == 2 && s3.score() == 700, "");

    // 4) 归因分解：纯拒绝 → top=1；纯深夜 → top=0；纯越权 → top=2。
    let mut d = AnomalyScorer::new();
    for _ in 0..10 {
        d.on_denial();
    }
    let mut n = AnomalyScorer::new();
    n.on_night_op();
    let mut o = AnomalyScorer::new();
    o.on_override();
    cs.add(
        "score_top_factor",
        d.top_factor() == 1 && n.top_factor() == 0 && o.top_factor() == 2,
        "",
    );

    // 5) 授予+到期：day0 授 → day90 到期恰点（TTL 语义）。
    let mut x = ExpiryScheduler::new();
    assert!(x.grant(7, 0));
    let mut due = [0u32; EXPIRY_CAP];
    let k90 = x.due_today(90, &mut due);
    let k89 = x.due_today(89, &mut due);
    cs.add("expiry_ttl_exact", k89 == 0 && k90 == 1 && due[0] == 7, "");

    // 6) 续授原位刷新：同应用再授 → n 不变、到期日后移。
    let n_before = x.n;
    x.grant(7, 30);
    cs.add(
        "expiry_regrant_in_place",
        x.n == n_before && x.due_today(90, &mut due) == 0 && x.due_today(120, &mut due) == 1,
        "",
    );

    // 7) 悬挂账与清扫：过 91 天 → lapsed=1；sweep 后归零、在授不受影响。
    let mut x2 = ExpiryScheduler::new();
    x2.grant(1, 0);
    x2.grant(2, 50); // 到期 140——不受 day91 清扫影响
    let lapsed = x2.lapsed_uncleaned(91);
    let removed = x2.sweep(91);
    cs.add(
        "expiry_sweep",
        lapsed == 1 && removed == 1 && x2.lapsed_uncleaned(91) == 0 && x2.n == 1,
        "",
    );

    // 8) 脱敏：首字符 + ** 定宽 3B；空名 → ?（不给读屏空串）。
    let mut out = [0u8; 16];
    let n1 = pseudonymize_app(b"Notepad2", &mut out);
    let mut out2 = [0u8; 16];
    let n2 = pseudonymize_app(b"", &mut out2);
    cs.add(
        "pseudonymize_shape",
        n1 == 3 && &out[..3] == b"N**" && n2 == 1 && out2[0] == b'?',
        "",
    );

    // 9) 完整性帧：账平才编码（越权>事件构造性矛盾拒收）。
    let mut f = [0u8; 10];
    let ok = encode_integrity(500, 12, 90, &mut f);
    let bad = encode_integrity(10, 12, 90, &mut f);
    cs.add(
        "integrity_frame_accounted",
        ok && decode_integrity(&f) == Some((500, 12, 90)) && !bad,
        "",
    );

    // 10) 完整性帧撕裂拒：单字节翻转全拦（10 字节逐一）。
    let mut tear_ok = true;
    for i in 0..10 {
        let mut t = f;
        t[i] ^= 0x5C;
        if decode_integrity(&t).is_some() {
            tear_ok = false;
        }
    }
    cs.add("integrity_frame_tear_proof", tear_ok, "");

    // 11) 常量自洽：黄<红、TTL 90、槽 12。
    cs.add(
        "b7_constants",
        SCORE_WARN < SCORE_CRIT && GRANT_TTL_DAYS == 90 && EXPIRY_CAP == 12,
        "",
    );

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn score_zero_is_green() {
        // 零事件 → 0 分 0 级（干净不是 None——就是干净）。
        let s = AnomalyScorer::new();
        assert_eq!(s.score(), 0);
        assert_eq!(s.grade(), 0);
    }

    #[test]
    fn expiry_multiple_due_same_day() {
        // 同日多授 → 同日多到期（名单一次收齐，不漏）。
        let mut x = ExpiryScheduler::new();
        for i in 0..5u32 {
            x.grant(i, 10);
        }
        let mut due = [0u32; EXPIRY_CAP];
        assert_eq!(x.due_today(100, &mut due), 5);
        assert_eq!(x.sweep(100), 5);
        assert_eq!(x.n, 0);
    }

    #[test]
    fn pseudonymize_all_distinguishable_prefix() {
        // 不同首字符 → 不同假名（脱敏不等于全混同）。
        let mut o1 = [0u8; 16];
        let mut o2 = [0u8; 16];
        pseudonymize_app(b"Alpha", &mut o1);
        pseudonymize_app(b"Beta", &mut o2);
        assert_ne!(&o1[..3], &o2[..3]);
    }
}
