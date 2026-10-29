//! UNX-F10848 TIOCSCTTY/TIOCNOTTY 控制终端矩阵（EPERM）（0x1c280 段）
//!
//! 锚点：UNX-F10848-J1 控制终端权矩阵——TIOCSCTTY 要求调用者必须是
//! 会话首（非首必 EPERM）且终端未被其他会话占用（被占必 EPERM）；
//! TIOCNOTTY 只能由当前持有会话放弃（未持有必 EPERM）；fd 非终端必
//! ENOTTY（fd 层先判）。正反双样本对账，注入核（无首进程查/无持有查）
//! 必检出。
//! 域：CoRun·UNX-C3（错误矩阵面谱系）。
//!
//! 结构：权场景（is_tty × cmd × 是否会话首 × 持有者）→ 三层判定器
//! （fd→首进程→占用）→ 控制终端账状态机（认领/放弃/持有账）→ 权样本
//! 矩阵 → 注入核（两坏实现必分歧）→ 生命周期回放（认领-读-放弃-再认领）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1c280 独占 0x20）。
pub const SEG_BASE: u32 = 0x1c280;
pub const CT_NOT_TTY: u32 = SEG_BASE + 0x01; // fd 非终端（ENOTTY 面）
pub const CT_NOT_LEADER: u32 = SEG_BASE + 0x02; // 非会话首（EPERM 面）
pub const CT_TAKEN: u32 = SEG_BASE + 0x03; // 终端被其他会话占用（EPERM 面）
pub const CT_NOT_HOLDER: u32 = SEG_BASE + 0x04; // 放弃者未持有（EPERM 面）
pub const CT_CLAIM_OK: u32 = SEG_BASE + 0x05; // 认领放行
pub const CT_DROP_OK: u32 = SEG_BASE + 0x06; // 放弃放行

/// 语义字面锚。
pub const ENOTTY: u32 = 25;
pub const EPERM: u32 = 1;

/// 诊断码语义文本。
pub fn diag_text(code: u32) -> &'static str {
    match code {
        CT_NOT_TTY => "fd 非终端 ENOTTY",
        CT_NOT_LEADER => "非会话首 EPERM",
        CT_TAKEN => "终端被占用 EPERM",
        CT_NOT_HOLDER => "未持有放弃 EPERM",
        CT_CLAIM_OK => "认领放行",
        CT_DROP_OK => "放弃放行",
        _ => "未知诊断码",
    }
}

/// ioctl 命令：TIOCSCTTY（认领）/ TIOCNOTTY（放弃）。
#[derive(Clone, PartialEq)]
pub enum CtCmd {
    Claim,
    Drop,
}

/// 权场景：fd 是否终端 × 命令 × 是否会话首 × 终端持有者（判定器四输入）。
#[derive(Clone)]
pub struct CtReq {
    pub is_tty: bool,
    pub cmd: CtCmd,
    pub caller_is_leader: bool,
    pub holder: u32,
    pub caller_session: u32,
}

/// 判定结果：放行携带会话号，拒绝携带诊断码。
#[derive(Clone, PartialEq, Debug)]
pub enum Res {
    Held(u32),
    Free(u32),
    Err(u32),
}

/// 三层判定器：fd 层 → 首进程层（认领）→ 持有层（认领占用/放弃未持有）
/// → 放行。全部分支依赖场景字段计算。
pub fn judge(req: &CtReq) -> Res {
    if !req.is_tty {
        return Res::Err(CT_NOT_TTY);
    }
    match req.cmd {
        CtCmd::Claim => {
            if !req.caller_is_leader {
                Res::Err(CT_NOT_LEADER)
            } else if req.holder != 0 && req.holder != req.caller_session {
                Res::Err(CT_TAKEN)
            } else {
                Res::Held(req.caller_session)
            }
        }
        CtCmd::Drop => {
            if req.holder != req.caller_session || req.holder == 0 {
                Res::Err(CT_NOT_HOLDER)
            } else {
                Res::Free(req.caller_session)
            }
        }
    }
}

/// 控制终端账状态机：持有者、认领账、放弃账。
pub struct CttyTable {
    pub is_tty: bool,
    pub holder: u32,
    pub claims: u32,
    pub drops: u32,
    pub note: String,
}

impl CttyTable {
    pub fn new(is_tty: bool) -> CttyTable {
        CttyTable { is_tty, holder: 0, claims: 0, drops: 0, note: "free".to_string() }
    }

    /// 认领：判定放行则落持有账。
    pub fn claim(&mut self, session: u32, is_leader: bool) -> Result<u32, u32> {
        let req = CtReq {
            is_tty: self.is_tty,
            cmd: CtCmd::Claim,
            caller_is_leader: is_leader,
            holder: self.holder,
            caller_session: session,
        };
        match judge(&req) {
            Res::Held(s) => {
                self.holder = s;
                self.claims += 1;
                self.note = "held".to_string();
                Ok(s)
            }
            Res::Free(_) => {
                self.note = "claim-denied".to_string();
                Err(CT_NOT_LEADER)
            }
            Res::Err(c) => {
                self.note = "claim-denied".to_string();
                Err(c)
            }
        }
    }

    /// 放弃：判定放行则清持有账。
    pub fn drop_ctty(&mut self, session: u32) -> Result<(), u32> {
        let req = CtReq {
            is_tty: self.is_tty,
            cmd: CtCmd::Drop,
            caller_is_leader: false,
            holder: self.holder,
            caller_session: session,
        };
        match judge(&req) {
            Res::Free(_) => {
                self.holder = 0;
                self.drops += 1;
                self.note = "free".to_string();
                Ok(())
            }
            Res::Held(_) => {
                self.note = "drop-denied".to_string();
                Err(CT_NOT_HOLDER)
            }
            Res::Err(c) => {
                self.note = "drop-denied".to_string();
                Err(c)
            }
        }
    }

    /// 持有账一致：认领-放弃对平，持有者唯一。
    pub fn ledger_ok(&self) -> bool {
        self.holder == 0 || self.drops < self.claims
    }
}

/// 样本矩阵容量。
pub const MATRIX_CAP: usize = 20;

/// 矩阵逐格判定：命中计数（期望码 0 表放行）。
pub fn verify_matrix(reqs: &[CtReq], wants: &[u32]) -> (usize, usize, bool) {
    let mut hits = 0usize;
    let total = reqs.len();
    let mut i = 0usize;
    while i < total && i < MATRIX_CAP {
        let want = if i < wants.len() { wants[i] } else { 0 };
        let hit = match judge(&reqs[i]) {
            Res::Held(_) => want == CT_CLAIM_OK,
            Res::Free(_) => want == CT_DROP_OK,
            Res::Err(c) => c == want,
        };
        if hit {
            hits += 1;
        }
        i += 1;
    }
    (hits, total.min(MATRIX_CAP), hits == total.min(MATRIX_CAP))
}

/// 样本矩阵：tty/非 tty × 认领/放弃 × 首/占用/持有，8 行全分账。
pub fn sample_matrix() -> (Vec<CtReq>, Vec<u32>) {
    let mut reqs: Vec<CtReq> = Vec::new();
    let mut wants: Vec<u32> = Vec::new();
    // R0 非 tty 认领 → ENOTTY
    reqs.push(CtReq { is_tty: false, cmd: CtCmd::Claim, caller_is_leader: true, holder: 0, caller_session: 7 });
    wants.push(CT_NOT_TTY);
    // R1 非 tty 放弃 → ENOTTY
    reqs.push(CtReq { is_tty: false, cmd: CtCmd::Drop, caller_is_leader: false, holder: 7, caller_session: 7 });
    wants.push(CT_NOT_TTY);
    // R2 非会话首认领空闲终端 → EPERM
    reqs.push(CtReq { is_tty: true, cmd: CtCmd::Claim, caller_is_leader: false, holder: 0, caller_session: 7 });
    wants.push(CT_NOT_LEADER);
    // R3 会话首认领被占终端 → EPERM
    reqs.push(CtReq { is_tty: true, cmd: CtCmd::Claim, caller_is_leader: true, holder: 3, caller_session: 7 });
    wants.push(CT_TAKEN);
    // R4 会话首认领空闲终端 → 放行
    reqs.push(CtReq { is_tty: true, cmd: CtCmd::Claim, caller_is_leader: true, holder: 0, caller_session: 7 });
    wants.push(CT_CLAIM_OK);
    // R5 会话首认领自持终端（重复认领幂等）→ 放行
    reqs.push(CtReq { is_tty: true, cmd: CtCmd::Claim, caller_is_leader: true, holder: 7, caller_session: 7 });
    wants.push(CT_CLAIM_OK);
    // R6 未持有会话放弃 → EPERM
    reqs.push(CtReq { is_tty: true, cmd: CtCmd::Drop, caller_is_leader: false, holder: 3, caller_session: 7 });
    wants.push(CT_NOT_HOLDER);
    // R7 持有会话放弃 → 放行
    reqs.push(CtReq { is_tty: true, cmd: CtCmd::Drop, caller_is_leader: false, holder: 7, caller_session: 7 });
    wants.push(CT_DROP_OK);
    (reqs, wants)
}

/// 注入 A：坏实现不查会话首（非首亦认领）。
pub fn faulty_no_leader(req: &CtReq) -> Res {
    let mut fixed = req.clone();
    fixed.caller_is_leader = true;
    judge(&fixed)
}

/// 注入 B：坏实现不查持有账（任意会话皆可放弃）。
pub fn faulty_release_skip(req: &CtReq) -> Res {
    match req.cmd {
        CtCmd::Drop if req.is_tty => Res::Free(req.caller_session),
        _ => judge(req),
    }
}

/// 注入核：两处坏账与判定器必分歧。
pub fn inject_two() -> [bool; 2] {
    let nonleader = CtReq { is_tty: true, cmd: CtCmd::Claim, caller_is_leader: false, holder: 0, caller_session: 7 };
    let outsider = CtReq { is_tty: true, cmd: CtCmd::Drop, caller_is_leader: false, holder: 3, caller_session: 7 };
    let d1 = judge(&nonleader) != faulty_no_leader(&nonleader);
    let d2 = judge(&outsider) != faulty_release_skip(&outsider);
    [d1, d2]
}

/// 生命周期回放：认领-重复认领-外部放弃拒-放弃-再认领。
pub fn lifecycle_ok() -> bool {
    let mut tb = CttyTable::new(true);
    let c1 = tb.claim(7, true);
    let c2 = tb.claim(7, true);
    let d1 = tb.drop_ctty(3);
    let d2 = tb.drop_ctty(7);
    let c3 = tb.claim(9, true);
    let d3 = tb.drop_ctty(9);
    c1.is_ok() && c2.is_ok() && d1.err() == Some(CT_NOT_HOLDER) && d2.is_ok()
        && c3.is_ok() && d3.is_ok() && tb.holder == 0 && tb.claims == 3 && tb.drops == 2
        && tb.ledger_ok()
}

/// 非终端核：认领/放弃双调皆 ENOTTY，账面零污染。
pub fn notty_face_ok() -> bool {
    let mut tb = CttyTable::new(false);
    let c = tb.claim(7, true);
    let d = tb.drop_ctty(7);
    c.err() == Some(CT_NOT_TTY) && d.err() == Some(CT_NOT_TTY)
        && tb.holder == 0 && tb.claims == 0 && tb.drops == 0
}

/// 权注记：判定渲染。
pub fn ct_note(r: &Res) -> String {
    match r {
        Res::Held(s) => format!("held-by={}", s),
        Res::Free(_) => "free".to_string(),
        Res::Err(c) => diag_text(*c).to_string(),
    }
}

/// 域判据（对应卡验收 J 判据）。
pub fn run_unx10848_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10848");
    let (reqs, wants) = sample_matrix();
    let (hits, total, cap_ok) = verify_matrix(&reqs, &wants);

    // J1 正路径：会话首认领空闲终端
    let j1 = judge(&CtReq { is_tty: true, cmd: CtCmd::Claim, caller_is_leader: true, holder: 0, caller_session: 7 })
        == Res::Held(7);
    s.add("J1-认领放行", j1, "会话首+空闲双条件");

    // J2 边界：自持重复认领幂等 / 他持必拒
    let j2 = judge(&CtReq { is_tty: true, cmd: CtCmd::Claim, caller_is_leader: true, holder: 7, caller_session: 7 })
        == Res::Held(7)
        && judge(&CtReq { is_tty: true, cmd: CtCmd::Claim, caller_is_leader: true, holder: 3, caller_session: 7 })
            == Res::Err(CT_TAKEN);
    s.add("J2-占用边界", j2, "自持幂等/他持 EPERM");

    // J3 首进程层：非首认领必 EPERM 面
    let j3 = judge(&CtReq { is_tty: true, cmd: CtCmd::Claim, caller_is_leader: false, holder: 0, caller_session: 7 })
        == Res::Err(CT_NOT_LEADER);
    s.add("J3-非首 EPERM", j3, "会话首前置");

    // J4 放弃层：未持有必拒 / 持有放行
    let j4 = judge(&CtReq { is_tty: true, cmd: CtCmd::Drop, caller_is_leader: false, holder: 3, caller_session: 7 })
        == Res::Err(CT_NOT_HOLDER)
        && judge(&CtReq { is_tty: true, cmd: CtCmd::Drop, caller_is_leader: false, holder: 7, caller_session: 7 })
            == Res::Free(7);
    s.add("J4-放弃账", j4, "持有核前置");

    // J5 生命周期回放
    s.add("J5-生命周期", lifecycle_ok(), "3 认领 2 放弃对平");

    // J6 非终端核（零污染）
    s.add("J6-非终端零污染", notty_face_ok(), "账面回零");

    // J7 样本矩阵全命中
    s.add("J7-矩阵对平", cap_ok, "8 行三层全命中");

    // J8 注入1 首进程漏查检出
    let inj = inject_two();
    s.add("J8-首进程漏查检出", inj[0], "非首误认领必分歧");

    // J9 注入2 持有账漏查检出
    s.add("J9-持有漏查检出", inj[1], "外人误放弃必分歧");

    // J10 持有账一致
    let mut tb = CttyTable::new(true);
    let _ = tb.claim(5, true);
    s.add("J10-持有账", tb.ledger_ok() && tb.holder == 5, "持有者唯一");

    // J11 字面锚：EPERM=1 / ENOTTY=25 / 空闲哨兵 0
    let j11 = EPERM == 1 && ENOTTY == 25 && CttyTable::new(true).holder == 0;
    s.add("J11-字面锚", j11, "两 errno 与哨兵");

    // J12 诊断码闭集
    let j12 = diag_text(CT_NOT_LEADER) == "非会话首 EPERM"
        && diag_text(CT_TAKEN) == "终端被占用 EPERM"
        && diag_text(4) == "未知诊断码";
    s.add("J12-码文本闭集", j12, "六码文本可辨");

    // J13 (dyn) 矩阵明细落账
    s.add_dyn("J13-矩阵明细", hits == total, &format!("hits={}/{} cap_ok={}", hits, total, cap_ok));

    // J14 (dyn) 权注记落账
    let note = ct_note(&judge(&CtReq { is_tty: true, cmd: CtCmd::Claim, caller_is_leader: true, holder: 0, caller_session: 42 }));
    s.add_dyn("J14-权注记", note == "held-by=42", &note);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10848_flow() {
        let mut tb = CttyTable::new(false);
        assert!(tb.claim(7, true).err() == Some(CT_NOT_TTY));
        assert!(tb.drop_ctty(7).err() == Some(CT_NOT_TTY));
        let mut tty = CttyTable::new(true);
        assert!(tty.claim(7, false).err() == Some(CT_NOT_LEADER));
        assert!(tty.claim(7, true).is_ok());
        assert!(tty.claim(3, true).err() == Some(CT_TAKEN));
        assert!(tty.drop_ctty(3).err() == Some(CT_NOT_HOLDER));
        assert!(tty.drop_ctty(7).is_ok());
        assert_eq!(tty.holder, 0);
        assert_eq!(judge(&CtReq { is_tty: true, cmd: CtCmd::Drop, caller_is_leader: false, holder: 0, caller_session: 7 }), Res::Err(CT_NOT_HOLDER));
    }

    #[test]
    fn unx10848_checks_green() {
        let s = run_unx10848_checks();
        assert!(s.all_passed(), "UNX-F10848 判据未全绿");
    }
}
