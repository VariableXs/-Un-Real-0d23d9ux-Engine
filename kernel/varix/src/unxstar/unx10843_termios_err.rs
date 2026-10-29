//! UNX-F10843 tcgetattr/tcsetattr 错误矩阵（ENOTTY/EINVAL）（0x1c1e0 段）
//!
//! 锚点：UNX-F10843-J1 终端属性两调用错误面——fd 非终端必 ENOTTY
//! （25，fd 层先判）；optional_actions 越出 {TCSANOW,TCSADRAIN,
//! TCSAFLUSH} 必 EINVAL（参数层后判）；getattr 成功返全量属性。
//! 正反双样本对账，注入核（无 ENOTTY 层/动作放水）必检出。
//! 域：CoRun·UNX-C3（错误矩阵面谱系）。
//!
//! 结构：调用场景（is_tty × 动作 × 掩码）→ 两层判定器（fd 层→动作层）
//! → 终端控制状态机（get/set 账、生效掩码）→ 动作样本矩阵（容量封顶）
//! → 注入核（两层坏实现必分歧）→ 生效回放（置后读取对平）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1c1e0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1c1e0;
pub const TC_NOT_TTY: u32 = SEG_BASE + 0x01; // fd 非终端（ENOTTY 面）
pub const TC_BAD_ACTION: u32 = SEG_BASE + 0x02; // 动作子越界（EINVAL 面）
pub const TC_OK_GET: u32 = SEG_BASE + 0x03; // 读面放行
pub const TC_OK_SET: u32 = SEG_BASE + 0x04; // 写面放行
pub const TC_COMMIT_MISS: u32 = SEG_BASE + 0x05; // 生效账丢失
pub const TC_GET_SET_MISMATCH: u32 = SEG_BASE + 0x06; // 置读失平

/// 语义字面锚（POSIX errno）。
pub const ENOTTY: u32 = 25;
pub const EINVAL: u32 = 22;

/// optional_actions 子集。
pub const TCSANOW: u32 = 0;
pub const TCSADRAIN: u32 = 1;
pub const TCSAFLUSH: u32 = 2;

/// 诊断码语义文本。
pub fn diag_text(code: u32) -> &'static str {
    match code {
        TC_NOT_TTY => "fd 非终端 ENOTTY",
        TC_BAD_ACTION => "动作子越界 EINVAL",
        TC_OK_GET => "读面放行",
        TC_OK_SET => "写面放行",
        TC_COMMIT_MISS => "生效账丢失",
        TC_GET_SET_MISMATCH => "置读失平",
        _ => "未知诊断码",
    }
}

/// 调用种类：getattr / setattr(mask, action)。
#[derive(Clone, PartialEq)]
pub enum Call {
    Get,
    Set(u32, u32),
}

/// 调用场景：fd 是否终端 × 调用（判定器两输入）。
#[derive(Clone)]
pub struct TcReq {
    pub is_tty: bool,
    pub call: Call,
}

/// 判定结果：Get 返回属性掩码，Set 返回生效动作；否则诊断码。
#[derive(Clone, PartialEq, Debug)]
pub enum Res {
    Got(u32),
    Act(u32),
    Err(u32),
}

/// 动作合法性：0/1/2 之外必非法。
pub fn action_valid(a: u32) -> bool {
    a == TCSANOW || a == TCSADRAIN || a == TCSAFLUSH
}

/// 两层判定器：fd 层（ENOTTY）→ 动作层（EINVAL）→ 放行。
/// 全部分支依赖场景字段计算。
pub fn judge(req: &TcReq) -> Res {
    if !req.is_tty {
        return Res::Err(TC_NOT_TTY);
    }
    match &req.call {
        Call::Get => Res::Got(0x1a2b),
        Call::Set(mask, action) => {
            if !action_valid(*action) {
                Res::Err(TC_BAD_ACTION)
            } else {
                Res::Act(*mask)
            }
        }
    }
}

/// 终端控制状态机：is_tty 装配、生效掩码、get/set 账。
pub struct TermCtl {
    pub is_tty: bool,
    pub committed: u32,
    pub commits: u32,
    pub note: String,
}

impl TermCtl {
    pub fn new(is_tty: bool) -> TermCtl {
        TermCtl { is_tty, committed: 0, commits: 0, note: "idle".to_string() }
    }

    /// getattr：非终端 ENOTTY；否则返回当前生效掩码。
    pub fn getattr(&self) -> Result<u32, u32> {
        if !self.is_tty {
            return Err(TC_NOT_TTY);
        }
        Ok(self.committed)
    }

    /// setattr：非终端 ENOTTY → 动作越界 EINVAL → 生效并落账。
    pub fn setattr(&mut self, mask: u32, action: u32) -> Result<u32, u32> {
        if !self.is_tty {
            return Err(TC_NOT_TTY);
        }
        if !action_valid(action) {
            self.note = "bad-action".to_string();
            return Err(TC_BAD_ACTION);
        }
        self.committed = mask;
        self.commits += 1;
        self.note = "committed".to_string();
        Ok(action)
    }

    /// 生效账一致：置读对平（getattr 见掩码）+ 提交计数不虚。
    pub fn ledger_ok(&self) -> bool {
        if !self.is_tty {
            return self.committed == 0 && self.commits == 0;
        }
        let seen = match self.getattr() {
            Ok(m) => m,
            Err(_) => u32::MAX,
        };
        seen == self.committed && self.note != "bad-action"
    }
}

/// 样本矩阵容量。
pub const MATRIX_CAP: usize = 20;

/// 矩阵逐格判定：命中（与期望一致）计数。
pub fn verify_matrix(reqs: &[TcReq], wants: &[u32]) -> (usize, usize, bool) {
    let mut hits = 0usize;
    let total = reqs.len();
    let mut i = 0usize;
    while i < total && i < MATRIX_CAP {
        let want = if i < wants.len() { wants[i] } else { 0 };
        match (judge(&reqs[i]), want) {
            (Res::Err(c), w) if c == w => hits += 1,
            (Res::Got(_), w) if w == TC_OK_GET => hits += 1,
            (Res::Act(_), w) if w == TC_OK_SET => hits += 1,
            _ => {}
        }
        i += 1;
    }
    (hits, total.min(MATRIX_CAP), hits == total.min(MATRIX_CAP))
}

/// 样本矩阵：tty/非 tty × get/set × 动作三态，8 行全分账。
pub fn sample_matrix() -> (Vec<TcReq>, Vec<u32>) {
    let mut reqs: Vec<TcReq> = Vec::new();
    let mut wants: Vec<u32> = Vec::new();
    // R0 非 tty get → ENOTTY
    reqs.push(TcReq { is_tty: false, call: Call::Get });
    wants.push(TC_NOT_TTY);
    // R1 非 tty set → ENOTTY（动作合法也拒）
    reqs.push(TcReq { is_tty: false, call: Call::Set(0x11, TCSANOW) });
    wants.push(TC_NOT_TTY);
    // R2 非 tty set 且动作坏 → ENOTTY（fd 层先判）
    reqs.push(TcReq { is_tty: false, call: Call::Set(0x11, 9) });
    wants.push(TC_NOT_TTY);
    // R3 tty set 动作 3 → EINVAL
    reqs.push(TcReq { is_tty: true, call: Call::Set(0x11, 3) });
    wants.push(TC_BAD_ACTION);
    // R4 tty set 动作 u32::MAX → EINVAL
    reqs.push(TcReq { is_tty: true, call: Call::Set(0x11, u32::MAX) });
    wants.push(TC_BAD_ACTION);
    // R5 tty get → 放行
    reqs.push(TcReq { is_tty: true, call: Call::Get });
    wants.push(TC_OK_GET);
    // R6 tty set TCSADRAIN → 放行
    reqs.push(TcReq { is_tty: true, call: Call::Set(0x22, TCSADRAIN) });
    wants.push(TC_OK_SET);
    // R7 tty set TCSAFLUSH → 放行
    reqs.push(TcReq { is_tty: true, call: Call::Set(0x33, TCSAFLUSH) });
    wants.push(TC_OK_SET);
    (reqs, wants)
}

/// 注入 A：坏实现无 ENOTTY 层（非 tty 恒按 tty 判）。
pub fn faulty_no_enotty(req: &TcReq) -> Res {
    let mut fixed = req.clone();
    fixed.is_tty = true;
    judge(&fixed)
}

/// 注入 B：坏实现动作层放水（任意动作均放行）。
pub fn faulty_action_loose(req: &TcReq) -> Res {
    match &req.call {
        Call::Set(mask, _) if req.is_tty => Res::Act(*mask),
        _ => judge(req),
    }
}

/// 注入核：两处坏账与判定器必分歧。
pub fn inject_two() -> [bool; 2] {
    let notty = TcReq { is_tty: false, call: Call::Get };
    let badact = TcReq { is_tty: true, call: Call::Set(1, 7) };
    let d1 = judge(&notty) != faulty_no_enotty(&notty);
    let d2 = judge(&badact) != faulty_action_loose(&badact);
    [d1, d2]
}

/// 生效回放：置后读对平，坏动作不落账。
pub fn commit_replay_ok() -> bool {
    let mut ctl = TermCtl::new(true);
    let bad = ctl.setattr(0x44, 9);
    let s1 = ctl.setattr(0x55, TCSANOW);
    let g1 = ctl.getattr();
    let seen = match g1 {
        Ok(m) => m,
        Err(_) => 0,
    };
    bad.err() == Some(TC_BAD_ACTION) && s1.is_ok() && seen == 0x55
        && ctl.committed == 0x55 && ctl.note == "committed"
}

/// 非终端核：get/set 双调皆 ENOTTY，账面零污染。
pub fn notty_face_ok() -> bool {
    let mut ctl = TermCtl::new(false);
    let g = ctl.getattr();
    let st = ctl.setattr(0x66, TCSAFLUSH);
    g.err() == Some(TC_NOT_TTY) && st.err() == Some(TC_NOT_TTY)
        && ctl.committed == 0 && ctl.commits == 0 && ctl.ledger_ok()
}

/// 码面注记：判定渲染。
pub fn res_note(r: &Res) -> String {
    match r {
        Res::Got(m) => format!("got={:#x}", m),
        Res::Act(m) => format!("act={:#x}", m),
        Res::Err(c) => diag_text(*c).to_string(),
    }
}

/// 域判据（对应卡验收 J 判据）。
pub fn run_unx10843_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10843");
    let (reqs, wants) = sample_matrix();
    let (hits, total, cap_ok) = verify_matrix(&reqs, &wants);

    // J1 正路径：tty get 放行
    let g = judge(&TcReq { is_tty: true, call: Call::Get });
    s.add("J1-读面放行", g == Res::Got(0x1a2b), "tty 读属性恒有值");

    // J2 边界：动作恰 2 放行 / 恰 3 拒绝
    let j2 = judge(&TcReq { is_tty: true, call: Call::Set(1, TCSAFLUSH) }) == Res::Act(1)
        && judge(&TcReq { is_tty: true, call: Call::Set(1, 3) }) == Res::Err(TC_BAD_ACTION);
    s.add("J2-动作边界", j2, "TCSAFLUSH/3 两沿可辨");

    // J3 fd 层：非 tty get/set 皆 ENOTTY（动作坏也被 fd 层截）
    let j3 = judge(&TcReq { is_tty: false, call: Call::Get }) == Res::Err(TC_NOT_TTY)
        && judge(&TcReq { is_tty: false, call: Call::Set(1, 9) }) == Res::Err(TC_NOT_TTY);
    s.add("J3-fd 层先判", j3, "ENOTTY 覆盖 EINVAL 面");

    // J4 动作层：tty 上坏动作 EINVAL
    s.add(
        "J4-动作越界",
        judge(&TcReq { is_tty: true, call: Call::Set(1, 5) }) == Res::Err(TC_BAD_ACTION),
        "非法 optional_actions 必拒",
    );

    // J5 生效回放核
    s.add("J5-生效回放", commit_replay_ok(), "坏动作不落账/好动作落账");

    // J6 非终端核（零污染）
    s.add("J6-非终端零污染", notty_face_ok(), "账面回零");

    // J7 样本矩阵全命中
    s.add("J7-矩阵对平", cap_ok, "8 行 fd/动作两层全命中");

    // J8 注入1 ENOTTY 层缺失检出
    let inj = inject_two();
    s.add("J8-ENOTTY 缺失检出", inj[0], "非 tty 误判必分歧");

    // J9 注入2 动作层放水检出
    s.add("J9-动作放水检出", inj[1], "坏动作误放必分歧");

    // J10 生效账一致
    let mut ctl = TermCtl::new(true);
    let _ = ctl.setattr(0x77, TCSADRAIN);
    s.add("J10-生效账一致", ctl.ledger_ok() && ctl.commits == 1, "置读对平");

    // J11 字面锚：ENOTTY=25 / EINVAL=22 / 动作子 0..2
    let j11 = ENOTTY == 25 && EINVAL == 22 && action_valid(TCSAFLUSH) && !action_valid(3);
    s.add("J11-字面锚", j11, "两 errno 与动作域");

    // J12 诊断码闭集
    let j12 = diag_text(TC_NOT_TTY) == "fd 非终端 ENOTTY"
        && diag_text(TC_BAD_ACTION) == "动作子越界 EINVAL"
        && diag_text(1) == "未知诊断码";
    s.add("J12-码文本闭集", j12, "六码文本可辨");

    // J13 (dyn) 矩阵明细落账
    s.add_dyn("J13-矩阵明细", hits == total, &format!("hits={}/{} cap_ok={}", hits, total, cap_ok));

    // J14 (dyn) 判定注记落账
    let note = res_note(&judge(&TcReq { is_tty: false, call: Call::Set(9, TCSANOW) }));
    s.add_dyn("J14-判定注记", note.contains("ENOTTY"), &note);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10843_flow() {
        let mut ctl = TermCtl::new(false);
        assert!(ctl.getattr().err() == Some(TC_NOT_TTY));
        assert!(ctl.setattr(1, TCSANOW).err() == Some(TC_NOT_TTY));
        assert_eq!(judge(&TcReq { is_tty: false, call: Call::Get }), Res::Err(TC_NOT_TTY));
        assert_eq!(judge(&TcReq { is_tty: true, call: Call::Set(1, 4) }), Res::Err(TC_BAD_ACTION));
        let mut ok = TermCtl::new(true);
        assert!(ok.setattr(0x99, TCSANOW).is_ok());
        assert_eq!(ok.getattr(), Ok(0x99));
        assert!(action_valid(TCSANOW) && action_valid(TCSADRAIN) && !action_valid(7));
    }

    #[test]
    fn unx10843_checks_green() {
        let s = run_unx10843_checks();
        assert!(s.all_passed(), "UNX-F10843 判据未全绿");
    }
}
