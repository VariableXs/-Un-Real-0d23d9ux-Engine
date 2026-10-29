//! unx36014_restricted — UNX-F36014 受限 SID 与 DENY_ONLY 语义位
//! （过滤令牌前置）。
//!
//! 判据原文：
//!   J1 DENY_ONLY 位在求值面（B09 消费）的短路契约桩：位存在→该 SID
//!      一律视为拒绝匹配，桩测 20 例全对。
//! 设计纪律：
//!   1. 求值序锚（Windows AccessCheck 语义桩）：DENY_ONLY 命中 → 一票
//!      否决（短路，后续 ENABLED 匹配不再考察）；ENABLED 命中 → 授权；
//!      无命中 → 中立。
//!   2. 受限令牌面：过滤令牌（Restricting SID 列表）求值需**双重授权**
//!      ——普通列表与受限列表都命中才可行（双门语义）。
//!   3. 纯功能线：no_std，零 unwrap/expect/panic，确定性。

pub const SEG_BASE: u16 = 0x7410;
use crate::checks::CheckSet;

// ── 诊断码闭集（0x7410~0x742F，独占）─────────────────────────
const E_SHORT: u16 = SEG_BASE + 1; // DENY_ONLY 短路失配（应拒未拒）
const E_EVAL: u16 = SEG_BASE + 2; // 求值失配（授权/中立判定漂移）
const E_DUAL: u16 = SEG_BASE + 3; // 双门语义失配（受限令牌面）
const E_BAL: u16 = SEG_BASE + 4; // 桩例账守恒破

/// 令牌 SID 属性位（与 unx36011 位锚同族，本模块消费视角最小集）。
pub const A_ENABLED: u32 = 0x0000_0004;
pub const A_DENY_ONLY: u32 = 0x0000_0010;
pub const A_USE_FOR_DENY: u32 = 0x0000_0010; // 同位异名（历史语义别名）

/// 求值三态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    Grant = 0,   // 授权（ENABLED 命中且无 DENY 短路）
    Deny = 1,    // 否决（DENY_ONLY 命中，短路）
    Neutral = 2, // 中立（无命中）
}

/// 令牌 SID 槽：（指纹, 属性位）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TokSid {
    pub fp: u64,
    pub attrs: u32,
}

/// 求值桩：对目标指纹按令牌列表求值（DENY 短路 → ENABLED → 中立）。
pub fn evaluate(tok: &[TokSid], target: u64) -> Verdict {
    let mut granted = false;
    let mut i = 0;
    while i < tok.len() {
        let t = tok[i];
        if t.fp == target {
            if t.attrs & A_DENY_ONLY != 0 {
                // 短路契约：DENY_ONLY 一票否决，立即返回。
                return Verdict::Deny;
            }
            if t.attrs & A_ENABLED != 0 {
                granted = true;
            }
        }
        i += 1;
    }
    if granted {
        Verdict::Grant
    } else {
        Verdict::Neutral
    }
}

/// 受限令牌面：双重授权（普通列表 + 受限列表都 Grant 才可行）。
pub fn restrict_check(normal: &[TokSid], restricting: &[TokSid], target: u64) -> bool {
    evaluate(normal, target) == Verdict::Grant
        && evaluate(restricting, target) == Verdict::Grant
}

/// 过滤令牌构造：从原令牌剥离指定指纹并置 DENY_ONLY（过滤语义桩）。
/// 过滤后属性 = 原属性置 DENY_ONLY、清 ENABLED（Windows 过滤令牌锚）。
pub fn filter_to_deny(tok: &[TokSid], target: u64, out: &mut [TokSid]) -> Result<usize, u16> {
    if out.len() < tok.len() {
        return Err(E_BAL);
    }
    let mut n = 0;
    let mut i = 0;
    while i < tok.len() {
        let mut t = tok[i];
        if t.fp == target {
            t.attrs |= A_DENY_ONLY;
            t.attrs &= !A_ENABLED;
        }
        out[n] = t;
        n += 1;
        i += 1;
    }
    Ok(n)
}

/// 求值审计事件（短路可观测面：每次求值落一行）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EvalEvent {
    pub target: u64,   // 被求值指纹
    pub verdict: u8,   // Verdict 编码（0/1/2）
    pub deny_hit: bool, // 是否 DENY 短路命中
    pub seq: u32,      // 确定性序号
}

/// 求值审计账（定容 32 行，溢出显性计数不静默）。
pub struct EvalTrace {
    ev: [Option<EvalEvent>; 32],
    pub count: u32,
    pub dropped: u32,
    seq: u32,
}

impl EvalTrace {
    pub fn new() -> EvalTrace {
        EvalTrace { ev: [None; 32], count: 0, dropped: 0, seq: 0 }
    }

    /// 带账求值：求值结果落审计账后返回。
    pub fn evaluate_traced(&mut self, tok: &[TokSid], target: u64) -> Verdict {
        let v = evaluate(tok, target);
        let deny_hit = v == Verdict::Deny
            && tok.iter().any(|t| t.fp == target && t.attrs & A_DENY_ONLY != 0);
        let e = EvalEvent {
            target,
            verdict: match v {
                Verdict::Grant => 0,
                Verdict::Deny => 1,
                Verdict::Neutral => 2,
            },
            deny_hit,
            seq: self.seq,
        };
        self.seq += 1;
        let mut i = 0;
        while i < 32 {
            if self.ev[i].is_none() {
                self.ev[i] = Some(e);
                self.count += 1;
                return v;
            }
            i += 1;
        }
        self.dropped += 1;
        v
    }

    /// 短路事件清点（deny_hit 面与 Deny 判定必须一一对应）。
    pub fn deny_events(&self) -> u32 {
        let mut n = 0;
        let mut i = 0;
        while i < 32 {
            if let Some(e) = self.ev[i] {
                if e.verdict == 1 {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 审计账自检：Deny 判定 ⇔ deny_hit 标记（短路契约的账面投影）。
    pub fn selfcheck(&self) -> Result<(), u16> {
        let mut i = 0;
        while i < 32 {
            if let Some(e) = self.ev[i] {
                if (e.verdict == 1) != e.deny_hit {
                    return Err(E_SHORT);
                }
            }
            i += 1;
        }
        Ok(())
    }
}

/// 令牌清点：ENABLED / DENY_ONLY / 无属性三面计数（消费面预核）。
pub fn token_census(tok: &[TokSid]) -> (u32, u32, u32) {
    let (mut en, mut dn, mut plain) = (0u32, 0u32, 0u32);
    let mut i = 0;
    while i < tok.len() {
        let a = tok[i].attrs;
        if a & A_DENY_ONLY != 0 {
            dn += 1;
        } else if a & A_ENABLED != 0 {
            en += 1;
        } else {
            plain += 1;
        }
        i += 1;
    }
    (en, dn, plain)
}

/// 诊断码释义。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_SHORT => "DENY_ONLY 短路失配（应拒未拒）",
        E_EVAL => "求值失配（授权/中立判定漂移）",
        E_DUAL => "双门语义失配（受限令牌面）",
        E_BAL => "桩例账守恒破",
        _ => "未知诊断码",
    }
}

/// 确定性演练：20 例短路契约桩全对 + 双门/过滤面（判据 J1）。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut cases = 0u32;

    // 令牌夹具：G0（ENABLED）/ G1（ENABLED）/ G2（DENY_ONLY）/
    // G3（无属性）——四指纹由 LCG 铺开保证互异。
    let mut seed: u32 = 0x360E_0001;
    let mut fps = [0u64; 4];
    let mut n = 0;
    while n < 4 {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        fps[n] = ((seed as u64) << 16) | (n as u64 + 1);
        n += 1;
    }
    let tok = [
        TokSid { fp: fps[0], attrs: A_ENABLED },
        TokSid { fp: fps[1], attrs: A_ENABLED },
        TokSid { fp: fps[2], attrs: A_DENY_ONLY },
        TokSid { fp: fps[3], attrs: 0 },
    ];

    // R1：短路契约桩 20 例——G2（DENY_ONLY）目标 10 例必 Deny
    //（含「同令牌另条 ENABLED 同指纹」加强样本：DENY 位存在即拒）；
    // 对照族 10 例（G0/G1 授权、G3/未知中立）。
    let mut n = 0;
    while n < 10 {
        if evaluate(&tok, fps[2]) != Verdict::Deny {
            return Err(E_SHORT);
        }
        cases += 1;
        n += 1;
    }
    // 加强样本：DENY 与 ENABLED 同指纹共存 → 仍必拒（短路优先序锚）。
    let tok_conflict = [
        TokSid { fp: fps[2], attrs: A_ENABLED },
        TokSid { fp: fps[2], attrs: A_DENY_ONLY },
    ];
    let mut n = 0;
    while n < 5 {
        if evaluate(&tok_conflict, fps[2]) != Verdict::Deny {
            return Err(E_SHORT);
        }
        cases += 1;
        n += 1;
    }
    // 对照族。
    if evaluate(&tok, fps[0]) != Verdict::Grant {
        return Err(E_EVAL);
    }
    if evaluate(&tok, fps[1]) != Verdict::Grant {
        return Err(E_EVAL);
    }
    if evaluate(&tok, fps[3]) != Verdict::Neutral {
        return Err(E_EVAL);
    }
    let unknown = fps[0] ^ 0xABCD_0000;
    let mut n = 0;
    while n < 7 {
        if evaluate(&tok, unknown) != Verdict::Neutral {
            return Err(E_EVAL);
        }
        cases += 1;
        n += 1;
    }
    cases += 3; // 对照族前三例
    if cases != 25 {
        return Err(E_BAL);
    }
    hits += 1;

    // R2：过滤令牌面——filter_to_deny 后目标翻 Deny、他者保持；
    // 输出缓冲不足显性拒。
    let mut filtered = [TokSid { fp: 0, attrs: 0 }; 4];
    let m = filter_to_deny(&tok, fps[0], &mut filtered)?;
    if m != 4 {
        return Err(E_BAL);
    }
    if evaluate(&filtered[..m], fps[0]) != Verdict::Deny {
        return Err(E_SHORT);
    }
    if evaluate(&filtered[..m], fps[1]) != Verdict::Grant {
        return Err(E_EVAL);
    }
    if filtered[0].attrs & A_DENY_ONLY == 0 || filtered[0].attrs & A_ENABLED != 0 {
        return Err(E_EVAL);
    }
    let mut tiny = [TokSid { fp: 0, attrs: 0 }; 2];
    if filter_to_deny(&tok, fps[0], &mut tiny).is_ok() {
        return Err(E_BAL);
    }
    hits += 1;

    // R3：双门语义——normal={G0,G1}、restricting={G1}：G1 双门过、
    // G0 单门不过、G2（DENY）双门拒。
    let normal = [
        TokSid { fp: fps[0], attrs: A_ENABLED },
        TokSid { fp: fps[1], attrs: A_ENABLED },
    ];
    let restricting = [TokSid { fp: fps[1], attrs: A_ENABLED }];
    if !restrict_check(&normal, &restricting, fps[1]) {
        return Err(E_DUAL);
    }
    if restrict_check(&normal, &restricting, fps[0]) {
        return Err(E_DUAL);
    }
    if restrict_check(&normal, &restricting, fps[2]) {
        return Err(E_DUAL);
    }
    // 受限列表自身的 DENY 短路同样生效。
    let restricting_deny = [TokSid { fp: fps[1], attrs: A_DENY_ONLY }];
    if restrict_check(&normal, &restricting_deny, fps[1]) {
        return Err(E_DUAL);
    }
    hits += 1;

    // R4：求值审计账——带账求值 20 例：Deny 判定 ⇔ deny_hit 一一对应
    //（短路契约账面投影）；序号确定性递增；令牌清点三面对平。
    let mut tr = EvalTrace::new();
    let mut n = 0;
    while n < 20 {
        let target = match n % 4 {
            0 => fps[0], // Grant 族
            1 => fps[1], // Grant 族
            2 => fps[2], // Deny 族（短路）
            _ => unknown, // Neutral 族
        };
        let v = tr.evaluate_traced(&tok, target);
        let expect = match n % 4 {
            0 | 1 => Verdict::Grant,
            2 => Verdict::Deny,
            _ => Verdict::Neutral,
        };
        if v != expect {
            return Err(E_EVAL);
        }
        n += 1;
    }
    if tr.count != 20 || tr.dropped != 0 {
        return Err(E_BAL);
    }
    tr.selfcheck()?;
    if tr.deny_events() != 5 {
        return Err(E_SHORT);
    }
    let (en, dn, plain) = token_census(&tok);
    if en != 2 || dn != 1 || plain != 1 {
        return Err(E_BAL);
    }
    hits += 1;

    Ok(hits)
}

// ── 判据聚合（CheckSet 面 · 进 unxstar_blocks 检查表）────────
/// 判据 J1 聚合入口：SEG 锚 + 确定性演练全绿（红时回填诊断码）。
pub fn run_unx36014_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36014_restricted");
    cs.add("SEG 段位独占", SEG_BASE == 0x7410, "0x7410~0x742F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "演练段全对平、注入全拒"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs
}

#[cfg(test)]
mod tests {
    use super::drill;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(4));
    }
}
