//! UNX-F56143 verdict 四态判定总闸（过/不过/N/A/样本不足）（0x611E0 段）
//!
//! 锚点：UNX-F56143-J1 四态判定总闸账（过 PASS / 不过 FAIL / 不适用
//! NA / 样本不足 INSUF 四态闭集，逐判定落账：对象 × 态 × 佐证量 × 判
//! 据 ID 四元组）；J2 态语义闸（PASS 必须佐证量达阈 + FAIL 可零佐证 +
//! NA 必须带缺侧原因 + INSUF 佐证量必须在 (0, 阈) 开区间）；J3 判定
//! 唯一性（同对象同判据同时段唯一态，重复判定拒）三判据全绿。
//! 域：CoRun·UNX-F2-B40（账本域·判定总闸面）。
//!
//! 结构：四态枚举闭集 + 判定总闸（态语义四闸逐态校验）+ 判定账（对
//! 象键唯一索引）+ 态分布统计（四态计数核）+ 注入核（PASS 零佐证/NA
//! 无原因/INSUF 满佐证/重复判定/未知态码必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x611E0 独占 0x20）。
pub const SEG_BASE: u32 = 0x611E0;
pub const VD_PASS_NOEV: u32 = SEG_BASE + 0x01; // PASS 零佐证
pub const VD_NA_NOREASON: u32 = SEG_BASE + 0x02; // NA 无缺侧原因
pub const VD_INSUF_FULL: u32 = SEG_BASE + 0x03; // INSUF 满佐证（自相矛盾态）
pub const VD_DUP_OBJ: u32 = SEG_BASE + 0x04; // 重复判定
pub const VD_UNK_STATE: u32 = SEG_BASE + 0x05; // 未知态码
pub const VD_FAIL_NOJID: u32 = SEG_BASE + 0x06; // FAIL 无判据 ID（不可追溯）

/// 四态闭集。
pub const V_PASS: u8 = 1;
pub const V_FAIL: u8 = 2;
pub const V_NA: u8 = 3;
pub const V_INSUF: u8 = 4;

/// 态名渲染。
pub fn verdict_name(v: u8) -> &'static str {
    match v {
        V_PASS => "PASS",
        V_FAIL => "FAIL",
        V_NA => "NA",
        V_INSUF => "INSUF",
        _ => "?",
    }
}

/// 态合法性核：仅四态闭集内合法。
pub fn state_valid(v: u8) -> bool {
    matches!(v, V_PASS | V_FAIL | V_NA | V_INSUF)
}

/// PASS 佐证量阈值。
pub const PASS_EVIDENCE_MIN: u32 = 3;

/// 单条判定。
#[derive(Clone)]
pub struct Verdict {
    pub obj: String,
    pub state: u8,
    pub evidence: u32,
    pub jid: String,
    pub reason: String,
}

/// 判定总闸 + 账。
pub struct VerdictGate {
    items: Vec<Verdict>,
}

impl VerdictGate {
    pub fn new() -> Self {
        VerdictGate { items: Vec::new() }
    }

    /// 判定落账：四态语义闸逐态校验。
    pub fn judge(
        &mut self,
        obj: &str,
        state: u8,
        evidence: u32,
        jid: &str,
        reason: &str,
    ) -> Result<(), u32> {
        if !state_valid(state) {
            return Err(VD_UNK_STATE);
        }
        if self.items.iter().any(|v| v.obj == obj) {
            return Err(VD_DUP_OBJ);
        }
        match state {
            V_PASS => {
                if evidence < PASS_EVIDENCE_MIN {
                    return Err(VD_PASS_NOEV);
                }
            }
            V_NA => {
                if reason.is_empty() {
                    return Err(VD_NA_NOREASON);
                }
            }
            V_INSUF => {
                if evidence >= PASS_EVIDENCE_MIN {
                    return Err(VD_INSUF_FULL);
                }
                if evidence == 0 {
                    return Err(VD_INSUF_FULL);
                }
            }
            V_FAIL => {
                if jid.is_empty() {
                    return Err(VD_FAIL_NOJID);
                }
            }
            _ => return Err(VD_UNK_STATE),
        }
        self.items.push(Verdict {
            obj: obj.to_string(),
            state,
            evidence,
            jid: jid.to_string(),
            reason: reason.to_string(),
        });
        Ok(())
    }

    pub fn verdict(&self, obj: &str) -> Option<&Verdict> {
        self.items.iter().find(|v| v.obj == obj)
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 四态分布统计（计数四元组）。
    pub fn state_dist(&self) -> [usize; 4] {
        let mut d = [0; 4];
        for v in &self.items {
            match v.state {
                V_PASS => d[0] += 1,
                V_FAIL => d[1] += 1,
                V_NA => d[2] += 1,
                V_INSUF => d[3] += 1,
                _ => {}
            }
        }
        d
    }

    /// 总闸核：全部判定态合法 + 佐证语义自洽复算。
    pub fn gate_ok(&self) -> bool {
        self.items.iter().all(|v| {
            if !state_valid(v.state) {
                return false;
            }
            match v.state {
                V_PASS => v.evidence >= PASS_EVIDENCE_MIN,
                V_NA => !v.reason.is_empty(),
                V_INSUF => v.evidence > 0 && v.evidence < PASS_EVIDENCE_MIN,
                V_FAIL => !v.jid.is_empty(),
                _ => false,
            }
        })
    }

    /// 渲染导出。
    pub fn render_gate(&self) -> String {
        let mut out = String::new();
        for v in &self.items {
            out.push_str(&format!(
                "{}|{}|ev{}|{}|{}",
                v.obj,
                verdict_name(v.state),
                v.evidence,
                v.jid,
                v.reason
            ));
            out.push('\n');
        }
        out
    }
}

impl Default for VerdictGate {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：四态各一判定。
fn standard_gate() -> VerdictGate {
    let mut g = VerdictGate::new();
    let _ = g.judge("fps_full", V_PASS, 5, "J1", "");
    let _ = g.judge("fps_mid", V_FAIL, 2, "J3", "");
    let _ = g.judge("gpu_side", V_NA, 0, "", "missing gpu host");
    let _ = g.judge("fps_idle", V_INSUF, 1, "", "");
    g
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：PASS 零佐证
    let mut g = VerdictGate::new();
    if g.judge("x", V_PASS, 0, "J1", "") == Err(VD_PASS_NOEV) {
        caught += 1;
    }
    // 注 2：NA 无原因
    let mut g = VerdictGate::new();
    if g.judge("x", V_NA, 0, "", "") == Err(VD_NA_NOREASON) {
        caught += 1;
    }
    // 注 3：INSUF 满佐证
    let mut g = VerdictGate::new();
    if g.judge("x", V_INSUF, 9, "", "") == Err(VD_INSUF_FULL) {
        caught += 1;
    }
    // 注 4：重复判定
    let mut g = standard_gate();
    if g.judge("fps_full", V_FAIL, 0, "J1", "") == Err(VD_DUP_OBJ) {
        caught += 1;
    }
    // 注 5：未知态码
    let mut g = VerdictGate::new();
    if g.judge("x", 9, 1, "J", "") == Err(VD_UNK_STATE) {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 四态账；J2 态语义闸；J3 唯一性与分布。
pub fn run_unx56143_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56143");
    // J1-1 四态各一落账
    let g = standard_gate();
    let ok = g.len() == 4 && g.gate_ok();
    s.add("56143-J1-四态全落账", ok, if ok { "" } else { "判定账不全" });
    // J1-2 渲染导出齐
    let text = standard_gate().render_gate();
    let ok = text.contains("fps_full|PASS|ev5|J1|") && text.contains("gpu_side|NA|") && text.contains("fps_idle|INSUF|ev1|");
    s.add("56143-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 PASS 阈上放行
    let mut g2 = VerdictGate::new();
    let ok = g2.judge("a", V_PASS, PASS_EVIDENCE_MIN, "J1", "").is_ok();
    s.add("56143-J2-阈上放行", ok, if ok { "" } else { "PASS 闸漂移" });
    // J2-2 PASS 阈下拒（阈-1 边界）
    let mut g2 = VerdictGate::new();
    let ok = g2.judge("a", V_PASS, PASS_EVIDENCE_MIN - 1, "J1", "") == Err(VD_PASS_NOEV);
    s.add("56143-J2-阈下拒", ok, if ok { "" } else { "边界穿透" });
    // J2-3 INSUF 开区间（1..阈值-1 合法）
    let mut g2 = VerdictGate::new();
    let ok = g2.judge("a", V_INSUF, PASS_EVIDENCE_MIN - 1, "", "").is_ok()
        && g2.judge("b", V_INSUF, 1, "", "").is_ok();
    s.add("56143-J2-INSUF开区间", ok, if ok { "" } else { "INSUF 闸漂移" });
    // J2-4 INSUF 零佐证拒（零佐证应走 NA 而非 INSUF）
    let mut g2 = VerdictGate::new();
    let ok = g2.judge("a", V_INSUF, 0, "", "") == Err(VD_INSUF_FULL);
    s.add("56143-J2-INSUF零拒", ok, if ok { "" } else { "零佐证穿透" });
    // J2-5 FAIL 无判据 ID 拒（不可追溯）
    let mut g2 = VerdictGate::new();
    let ok = g2.judge("a", V_FAIL, 0, "", "") == Err(VD_FAIL_NOJID);
    s.add("56143-J2-FAIL需判据", ok, if ok { "" } else { "追溯面漂移" });
    // J3-1 同对象唯一
    let mut g2 = standard_gate();
    let ok = g2.judge("fps_full", V_NA, 0, "", "r") == Err(VD_DUP_OBJ);
    s.add("56143-J3-同对象唯一", ok, if ok { "" } else { "唯一闸失效" });
    // J3-2 异对象同态可并存
    let mut g2 = VerdictGate::new();
    let ok = g2.judge("a", V_NA, 0, "", "r1").is_ok() && g2.judge("b", V_NA, 0, "", "r2").is_ok();
    s.add("56143-J3-异对象并存", ok, if ok { "" } else { "并存面漂移" });
    // J3-3 四态分布核
    let g = standard_gate();
    let ok = g.state_dist() == [1, 1, 1, 1];
    s.add("56143-J3-分布核", ok, if ok { "" } else { "分布漂移" });
    // J3-4 态名渲染闭集
    let ok = verdict_name(V_PASS) == "PASS" && verdict_name(V_FAIL) == "FAIL"
        && verdict_name(V_NA) == "NA" && verdict_name(V_INSUF) == "INSUF"
        && verdict_name(9) == "?";
    s.add("56143-J3-态名闭集", ok, if ok { "" } else { "态名漂移" });
    // J3-5 总闸复算全绿
    let ok = standard_gate().gate_ok();
    s.add("56143-J3-总闸复算", ok, if ok { "" } else { "闸复算漂移" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56143-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56143_all_checks_green() {
        let set = run_unx56143_checks();
        assert!(set.all_passed(), "UNX-F56143 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56143_dist_stable() {
        assert_eq!(standard_gate().state_dist(), [1, 1, 1, 1]);
    }
}
