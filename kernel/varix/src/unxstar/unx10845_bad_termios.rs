//! UNX-F10845 非法 termios 参数矩阵（cflag/cc 组合）（0x1c220 段）
//!
//! 锚点：UNX-F10845-J1 termios 参数合法性矩阵——c_cflag 越出已定义
//! 位掩码必 EINVAL；c_cc 下标越出 NCCS 必 EINVAL（数组界先于位域判）；
//! CSIZE 位域必须落在 CS5..CS8 四值内；合法组合生效并落账。正反双
//! 样本对账，注入核（无位掩码查/无下标查）必检出。
//! 域：CoRun·UNX-C3（错误矩阵面谱系）。
//!
//! 结构：参数场景（cflag × cc 下标 × cc 值）→ 三层判定器（下标→掩码
//! →位域）→ termios 装配状态机（生效掩码/拒绝账）→ 组合样本矩阵
//! → 注入核（两层漏查坏实现必分歧）→ 装配回放（生效/拒绝账对平）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1c220 独占 0x20）。
pub const SEG_BASE: u32 = 0x1c220;
pub const BT_BAD_CC_IDX: u32 = SEG_BASE + 0x01; // c_cc 下标越界
pub const BT_BAD_CFLAG: u32 = SEG_BASE + 0x02; // cflag 未定义位
pub const BT_BAD_CSIZE: u32 = SEG_BASE + 0x03; // CSIZE 位域越界
pub const BT_ACCEPT: u32 = SEG_BASE + 0x04; // 组合放行
pub const BT_REJECT_LOST: u32 = SEG_BASE + 0x05; // 拒绝账丢失
pub const BT_COMMIT_DUP: u32 = SEG_BASE + 0x06; // 生效重复落账

/// 参数常量（POSIX termios 位域，先定后用）。
pub const NCCS: usize = 11;
pub const CSIZE: u32 = 0o0060;
pub const CS5: u32 = 0o0000;
pub const CS8: u32 = 0o0060;
pub const CSTOPB: u32 = 0o0100;
pub const CREAD: u32 = 0o0200;
pub const PARENB: u32 = 0o0400;
pub const PARODD: u32 = 0o1000;
pub const CLOCAL: u32 = 0o4000;

/// 已定义 cflag 位掩码。
pub const CFLAG_VALID: u32 =
    CSIZE | CSTOPB | CREAD | PARENB | PARODD | CLOCAL;

/// 诊断码语义文本。
pub fn diag_text(code: u32) -> &'static str {
    match code {
        BT_BAD_CC_IDX => "c_cc 下标越界 EINVAL",
        BT_BAD_CFLAG => "cflag 未定义位 EINVAL",
        BT_BAD_CSIZE => "CSIZE 位域越界 EINVAL",
        BT_ACCEPT => "组合放行",
        BT_REJECT_LOST => "拒绝账丢失",
        BT_COMMIT_DUP => "生效重复落账",
        _ => "未知诊断码",
    }
}

/// 参数场景：cflag × cc 下标 × cc 值（判定器三输入）。
#[derive(Clone)]
pub struct TioReq {
    pub cflag: u32,
    pub cc_idx: usize,
    pub cc_val: u8,
}

/// 判定结果：放行携带生效 cflag，拒绝携带诊断码。
#[derive(Clone, PartialEq, Debug)]
pub enum Res {
    Accept(u32),
    Err(u32),
}

/// CSIZE 位域核：取 CSIZE 段并验四值域。
pub fn csize_field(cflag: u32) -> u32 {
    (cflag & CSIZE) >> 4
}

/// 三层判定器：cc 下标 → cflag 掩码 → CSIZE 位域 → 放行。
/// 全部分支依赖场景字段计算。
pub fn judge(req: &TioReq) -> Res {
    if req.cc_idx >= NCCS {
        return Res::Err(BT_BAD_CC_IDX);
    }
    if (req.cflag & !CFLAG_VALID) != 0 {
        return Res::Err(BT_BAD_CFLAG);
    }
    if csize_field(req.cflag) > 3 {
        return Res::Err(BT_BAD_CSIZE);
    }
    Res::Accept(req.cflag & CFLAG_VALID)
}

/// termios 装配状态机：生效掩码 + 拒绝账 + 提交账。
pub struct TioSet {
    pub committed: u32,
    pub commits: u32,
    pub rejects: u32,
    pub cc: [u8; NCCS],
    pub note: String,
}

impl TioSet {
    pub fn new() -> TioSet {
        TioSet {
            committed: 0,
            commits: 0,
            rejects: 0,
            cc: [0u8; NCCS],
            note: "fresh".to_string(),
        }
    }

    /// 装配一次：判定放行则提交（掩码+cc 双落账）；拒绝只入拒绝账。
    pub fn apply(&mut self, req: &TioReq) -> Result<u32, u32> {
        match judge(req) {
            Res::Accept(cf) => {
                self.committed = cf;
                self.commits += 1;
                if req.cc_idx < NCCS {
                    self.cc[req.cc_idx] = req.cc_val;
                }
                self.note = "committed".to_string();
                Ok(cf)
            }
            Res::Err(c) => {
                self.rejects += 1;
                self.note = "rejected".to_string();
                Err(c)
            }
        }
    }

    /// 生效账一致：提交数+拒绝数=装配总数，cc 界内恒可读。
    pub fn ledger_ok(&self, total: u32) -> bool {
        self.commits + self.rejects == total && self.commits <= total && self.rejects <= total
    }

    /// 生效掩码回读：与 committed 对平。
    pub fn echo_ok(&self) -> bool {
        match judge(&TioReq { cflag: self.committed, cc_idx: 0, cc_val: 0 }) {
            Res::Accept(cf) => cf == self.committed,
            Res::Err(_) => false,
        }
    }
}

/// 样本矩阵容量。
pub const MATRIX_CAP: usize = 20;

/// 矩阵逐格判定：命中计数（期望码 0 表放行）。
pub fn verify_matrix(reqs: &[TioReq], wants: &[u32]) -> (usize, usize, bool) {
    let mut hits = 0usize;
    let total = reqs.len();
    let mut i = 0usize;
    while i < total && i < MATRIX_CAP {
        let want = if i < wants.len() { wants[i] } else { 0 };
        let hit = match judge(&reqs[i]) {
            Res::Accept(_) => want == BT_ACCEPT,
            Res::Err(c) => c == want,
        };
        if hit {
            hits += 1;
        }
        i += 1;
    }
    (hits, total.min(MATRIX_CAP), hits == total.min(MATRIX_CAP))
}

/// 样本矩阵：cc 界内/界外 × 位域合法/未定义位/CSIZE 边界，8 行全分账。
pub fn sample_matrix() -> (Vec<TioReq>, Vec<u32>) {
    let mut reqs: Vec<TioReq> = Vec::new();
    let mut wants: Vec<u32> = Vec::new();
    // R0 下标恰界外 → 越界（下标层先判）
    reqs.push(TioReq { cflag: CREAD, cc_idx: NCCS, cc_val: 1 });
    wants.push(BT_BAD_CC_IDX);
    // R1 下标远界外且 cflag 坏 → 仍越界码
    reqs.push(TioReq { cflag: 0o100000, cc_idx: NCCS + 7, cc_val: 2 });
    wants.push(BT_BAD_CC_IDX);
    // R2 未定义位 0o100000 → 掩码码
    reqs.push(TioReq { cflag: CREAD | 0o100000, cc_idx: 0, cc_val: 3 });
    wants.push(BT_BAD_CFLAG);
    // R3 未定义位 0o2（低位噪声）→ 掩码码
    reqs.push(TioReq { cflag: PARENB | 0o2, cc_idx: 1, cc_val: 4 });
    wants.push(BT_BAD_CFLAG);
    // R4 CS5 纯净组合 → 放行
    reqs.push(TioReq { cflag: CS5 | CREAD | CLOCAL, cc_idx: 2, cc_val: 5 });
    wants.push(BT_ACCEPT);
    // R5 CS8 + 回显位 → 放行
    reqs.push(TioReq { cflag: CS8 | CREAD | PARENB, cc_idx: 6, cc_val: 6 });
    wants.push(BT_ACCEPT);
    // R6 掩码全量合法位 → 放行
    reqs.push(TioReq { cflag: CFLAG_VALID, cc_idx: NCCS - 1, cc_val: 7 });
    wants.push(BT_ACCEPT);
    // R7 零 cflag → 放行（全清也是合法组合）
    reqs.push(TioReq { cflag: 0, cc_idx: 3, cc_val: 8 });
    wants.push(BT_ACCEPT);
    (reqs, wants)
}

/// 注入 A：坏实现不查 cflag 未定义位。
pub fn faulty_no_mask(req: &TioReq) -> Res {
    if req.cc_idx >= NCCS {
        return Res::Err(BT_BAD_CC_IDX);
    }
    Res::Accept(req.cflag & CFLAG_VALID)
}

/// 注入 B：坏实现不查 cc 下标。
pub fn faulty_no_cc(req: &TioReq) -> Res {
    if (req.cflag & !CFLAG_VALID) != 0 {
        return Res::Err(BT_BAD_CFLAG);
    }
    Res::Accept(req.cflag & CFLAG_VALID)
}

/// 注入核：两处漏查与判定器必分歧。
pub fn inject_two() -> [bool; 2] {
    let badflag = TioReq { cflag: 0o100000, cc_idx: 0, cc_val: 0 };
    let badidx = TioReq { cflag: CREAD, cc_idx: NCCS + 3, cc_val: 0 };
    let d1 = judge(&badflag) != faulty_no_mask(&badflag);
    let d2 = judge(&badidx) != faulty_no_cc(&badidx);
    [d1, d2]
}

/// 装配回放：拒/收交替落账，账面对平。
pub fn apply_replay_ok() -> bool {
    let mut st = TioSet::new();
    let mut i = 0u32;
    let mut applied = 0u32;
    while i < 6 {
        let req = if i % 2 == 0 {
            TioReq { cflag: CREAD | CS8, cc_idx: (i % NCCS as u32) as usize, cc_val: i as u8 }
        } else {
            TioReq { cflag: 0o100000, cc_idx: 0, cc_val: i as u8 }
        };
        match st.apply(&req) {
            Ok(_) => applied += 1,
            Err(_) => {}
        }
        i += 1;
    }
    st.commits == applied && st.rejects == 6 - applied && st.ledger_ok(6) && st.echo_ok()
}

/// 边界核：下标 NCCS-1 可落 / NCCS 必拒；掩码恰全量放行。
pub fn bound_face_ok() -> bool {
    let mut st = TioSet::new();
    let lo = st.apply(&TioReq { cflag: CREAD, cc_idx: NCCS - 1, cc_val: 9 });
    let hi = st.apply(&TioReq { cflag: CREAD, cc_idx: NCCS, cc_val: 9 });
    lo.is_ok() && hi.err() == Some(BT_BAD_CC_IDX)
        && st.cc[NCCS - 1] == 9 && st.commits == 1 && st.rejects == 1
}

/// 组合注记：cflag 位域渲染。
pub fn cflag_note(cf: u32) -> String {
    format!(
        "cflag={:#o} csize={} parenb={}",
        cf,
        csize_field(cf),
        (cf & PARENB) != 0
    )
}

/// 域判据（对应卡验收 J 判据）。
pub fn run_unx10845_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10845");
    let (reqs, wants) = sample_matrix();
    let (hits, total, cap_ok) = verify_matrix(&reqs, &wants);

    // J1 正路径：CS8+CREAD+PARENB 放行
    let ok = judge(&TioReq { cflag: CS8 | CREAD | PARENB, cc_idx: 0, cc_val: 0 });
    s.add("J1-合法组合", ok == Res::Accept(CS8 | CREAD | PARENB), "纯净位域放行");

    // J2 边界：cc 下标 NCCS-1/NCCS 两沿
    let j2 = judge(&TioReq { cflag: 0, cc_idx: NCCS - 1, cc_val: 0 }) != Res::Err(BT_BAD_CC_IDX)
        && judge(&TioReq { cflag: 0, cc_idx: NCCS, cc_val: 0 }) == Res::Err(BT_BAD_CC_IDX);
    s.add("J2-下标边界", j2, "界内/界外两沿可辨");

    // J3 掩码边界：恰全量合法位放行 / 附加一位必拒
    let j3 = judge(&TioReq { cflag: CFLAG_VALID, cc_idx: 0, cc_val: 0 }) == Res::Accept(CFLAG_VALID)
        && judge(&TioReq { cflag: CFLAG_VALID | 0o1, cc_idx: 0, cc_val: 0 })
            == Res::Err(BT_BAD_CFLAG);
    s.add("J3-掩码边界", j3, "全量/溢出一位两沿可辨");

    // J4 层序：下标越界先于掩码（双坏仍报下标码）
    s.add(
        "J4-层序核",
        judge(&TioReq { cflag: 0o100000, cc_idx: NCCS, cc_val: 0 }) == Res::Err(BT_BAD_CC_IDX),
        "下标层先判",
    );

    // J5 装配回放（拒/收账对平）
    s.add("J5-装配回放", apply_replay_ok(), "3 收 3 拒账面对平");

    // J6 边界核（界内可落/界外必拒）
    s.add("J6-界内落账", bound_face_ok(), "NCCS-1 落账 NCCS 拒");

    // J7 样本矩阵全命中
    s.add("J7-矩阵对平", cap_ok, "8 行三层全命中");

    // J8 注入1 掩码漏查检出
    let inj = inject_two();
    s.add("J8-掩码漏查检出", inj[0], "未定义位误放必分歧");

    // J9 注入2 下标漏查检出
    s.add("J9-下标漏查检出", inj[1], "界外误放必分歧");

    // J10 生效账一致
    let mut st = TioSet::new();
    let _ = st.apply(&TioReq { cflag: CS8 | CREAD, cc_idx: 4, cc_val: 1 });
    s.add(
        "J10-生效账",
        st.ledger_ok(1) && st.committed == (CS8 | CREAD) && st.cc[4] == 1,
        "掩码+cc 双落账",
    );

    // J11 字面锚：NCCS=11 / CSIZE=0o60 / CS8=0o60
    let j11 = NCCS == 11 && CSIZE == 0o60 && CS8 == 0o60 && CS5 == 0 && csize_field(CS8) == 3;
    s.add("J11-字面锚", j11, "位域常量对");

    // J12 诊断码闭集
    let j12 = diag_text(BT_BAD_CC_IDX) == "c_cc 下标越界 EINVAL"
        && diag_text(BT_BAD_CFLAG) == "cflag 未定义位 EINVAL"
        && diag_text(0xabc) == "未知诊断码";
    s.add("J12-码文本闭集", j12, "六码文本可辨");

    // J13 (dyn) 矩阵明细落账
    s.add_dyn("J13-矩阵明细", hits == total, &format!("hits={}/{} cap_ok={}", hits, total, cap_ok));

    // J14 (dyn) 组合注记落账
    let note = cflag_note(CS8 | CREAD | PARENB);
    s.add_dyn("J14-组合注记", note.contains("csize=3") && note.contains("parenb=true"), &note);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10845_flow() {
        assert_eq!(judge(&TioReq { cflag: 0o100000, cc_idx: 0, cc_val: 0 }), Res::Err(BT_BAD_CFLAG));
        assert_eq!(judge(&TioReq { cflag: CREAD, cc_idx: NCCS, cc_val: 0 }), Res::Err(BT_BAD_CC_IDX));
        assert_eq!(judge(&TioReq { cflag: CS8 | CREAD, cc_idx: 1, cc_val: 2 }), Res::Accept(CS8 | CREAD));
        let mut st = TioSet::new();
        assert!(st.apply(&TioReq { cflag: 0o100000, cc_idx: 0, cc_val: 0 }).err() == Some(BT_BAD_CFLAG));
        assert!(st.apply(&TioReq { cflag: CS8, cc_idx: 2, cc_val: 5 }).is_ok());
        assert_eq!(st.cc[2], 5);
        assert!(st.ledger_ok(2) && st.echo_ok());
        assert_eq!(csize_field(0o7777 & !CSIZE), 0);
    }

    #[test]
    fn unx10845_checks_green() {
        let s = run_unx10845_checks();
        assert!(s.all_passed(), "UNX-F10845 判据未全绿");
    }
}
