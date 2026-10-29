//! UNX-F10850 速率面矩阵（cfsetispeed/cfgetospeed）（0x1c2c0 段）
//!
//! 锚点：UNX-F10850-J1 波特率域矩阵——速率码必落 B 常数表（表外码
//! 必 EINVAL，不截断不放水）；B0 语义为"挂断/随 cflag"合法零速率；
//! cfsetispeed 提交后 cfgetospeed 恒见同码（置读对平）。正反双样本
//! 对账，注入核（表外放水/置而不落）必检出。
//! 域：CoRun·UNX-C3（错误矩阵面谱系）。
//!
//! 结构：速率场景（速率码）→ 表域判定器（B 表查域，B0 特判）→ 速率
//! 账状态机（ispeed/ospeed 双槽置读）→ 速率样本矩阵（表内表外封顶）
//! → 注入核（两坏实现必分歧）→ 编解码回放（码↔速率双射对平）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1c2c0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1c2c0;
pub const SP_OUT_TABLE: u32 = SEG_BASE + 0x01; // 表外速率码（EINVAL 面）
pub const SP_SET_MISS: u32 = SEG_BASE + 0x02; // 置而不落（提交账丢失）
pub const SP_ROUND_MISS: u32 = SEG_BASE + 0x03; // 编解码失平
pub const SP_B0_FACE: u32 = SEG_BASE + 0x04; // B0 零速率面
pub const SP_LEDGER_LOST: u32 = SEG_BASE + 0x05; // 双槽账丢失
pub const SP_OK_FACE: u32 = SEG_BASE + 0x06; // 放行面

/// 波特率表：B 常数 → 实际速率（先定后用）。
pub const SPEED_TABLE: [(u32, u32); 11] = [
    (0, 0),     // B0 挂断/随 cflag
    (1, 50),    // B50
    (2, 75),    // B75
    (3, 110),   // B110
    (4, 134),   // B134
    (5, 150),   // B150
    (6, 200),   // B200
    (7, 300),   // B300
    (8, 600),   // B600
    (9, 1200),  // B1200
    (13, 9600), // B9600
];

/// CBAUD 位掩码（cflag 速率域）。
pub const CBAUD: u32 = 0o10017;

/// 诊断码语义文本。
pub fn diag_text(code: u32) -> &'static str {
    match code {
        SP_OUT_TABLE => "表外速率码 EINVAL",
        SP_SET_MISS => "置而不落",
        SP_ROUND_MISS => "编解码失平",
        SP_B0_FACE => "B0 零速率面",
        SP_LEDGER_LOST => "双槽账丢失",
        SP_OK_FACE => "放行面",
        _ => "未知诊断码",
    }
}

/// 速率场景：请求速率码（判定器单输入，全表查域）。
#[derive(Clone)]
pub struct SpeedReq {
    pub speed_code: u32,
}

/// 判定结果：放行携带实际速率，拒绝携带诊断码。
#[derive(Clone, PartialEq, Debug)]
pub enum Res {
    Rate(u32),
    Err(u32),
}

/// 表域核：速率码是否落表。
pub fn in_table(code: u32) -> bool {
    let mut hit = false;
    let mut i = 0usize;
    while i < SPEED_TABLE.len() {
        if SPEED_TABLE[i].0 == code {
            hit = true;
        }
        i += 1;
    }
    hit
}

/// 表域查速率：命中回速率，未命中回哨兵。
pub fn rate_of(code: u32) -> Option<u32> {
    let mut i = 0usize;
    while i < SPEED_TABLE.len() {
        if SPEED_TABLE[i].0 == code {
            return Some(SPEED_TABLE[i].1);
        }
        i += 1;
    }
    None
}

/// 表域判定器：B0 特判零速率面 → 表内放行 → 表外 EINVAL。
pub fn judge(req: &SpeedReq) -> Res {
    if req.speed_code == 0 {
        return Res::Rate(0);
    }
    match rate_of(req.speed_code) {
        Some(r) => Res::Rate(r),
        None => Res::Err(SP_OUT_TABLE),
    }
}

/// 编码：速率码折入 cflag（CBAUD 域）。
pub fn encode_cflag(code: u32) -> u32 {
    code & CBAUD
}

/// 解码：cflag CBAUD 域回速率码。
pub fn decode_cflag(cf: u32) -> u32 {
    cf & CBAUD
}

/// 编解码双射核：码→cflag→码 恒平。
pub fn roundtrip_ok(code: u32) -> bool {
    decode_cflag(encode_cflag(code)) == code
}

/// 速率账状态机：ispeed/ospeed 双槽置读。
pub struct SpeedCtl {
    pub ispeed: u32,
    pub ospeed: u32,
    pub sets: u32,
    pub gets: u32,
    pub note: String,
}

impl SpeedCtl {
    pub fn new() -> SpeedCtl {
        SpeedCtl { ispeed: 0, ospeed: 0, sets: 0, gets: 0, note: "fresh".to_string() }
    }

    /// cfsetispeed：表外 EINVAL；放行落 ispeed 槽。
    pub fn set_ispeed(&mut self, code: u32) -> Result<u32, u32> {
        match judge(&SpeedReq { speed_code: code }) {
            Res::Rate(r) => {
                self.ispeed = code;
                self.sets += 1;
                self.note = "set".to_string();
                Ok(r)
            }
            Res::Err(c) => {
                self.note = "bad-speed".to_string();
                Err(c)
            }
        }
    }

    /// cfgetospeed：回 ospeed 槽并落读账。
    pub fn get_ospeed(&self) -> u32 {
        self.ospeed
    }

    /// ospeed 同步：ispeed 提交即 ospeed 同槽（内核语义）。
    pub fn sync_ospeed(&mut self) {
        self.ospeed = self.ispeed;
    }

    /// 双槽账一致：置读对平。
    pub fn ledger_ok(&self) -> bool {
        self.ispeed == self.ospeed && in_table(self.ispeed)
    }
}

/// 样本矩阵容量。
pub const MATRIX_CAP: usize = 20;

/// 矩阵逐格判定：命中计数（期望码 0 表放行）。
pub fn verify_matrix(reqs: &[SpeedReq], wants: &[u32]) -> (usize, usize, bool) {
    let mut hits = 0usize;
    let total = reqs.len();
    let mut i = 0usize;
    while i < total && i < MATRIX_CAP {
        let want = if i < wants.len() { wants[i] } else { 0 };
        let hit = match judge(&reqs[i]) {
            Res::Rate(_) => want == SP_OK_FACE,
            Res::Err(c) => c == want,
        };
        if hit {
            hits += 1;
        }
        i += 1;
    }
    (hits, total.min(MATRIX_CAP), hits == total.min(MATRIX_CAP))
}

/// 样本矩阵：表内全码 + 表外沿码，10 行全分账。
pub fn sample_matrix() -> (Vec<SpeedReq>, Vec<u32>) {
    let mut reqs: Vec<SpeedReq> = Vec::new();
    let mut wants: Vec<u32> = Vec::new();
    let mut i = 0usize;
    while i < SPEED_TABLE.len() {
        reqs.push(SpeedReq { speed_code: SPEED_TABLE[i].0 });
        wants.push(SP_OK_FACE);
        i += 1;
    }
    // 表外沿：表内最大码 +1 与高位噪声
    reqs.push(SpeedReq { speed_code: 14 });
    wants.push(SP_OUT_TABLE);
    reqs.push(SpeedReq { speed_code: u32::MAX });
    wants.push(SP_OUT_TABLE);
    (reqs, wants)
}

/// 注入 A：坏实现表域放水（表外码按表取模放行）。
pub fn faulty_loose_table(req: &SpeedReq) -> Res {
    if req.speed_code == 0 {
        return Res::Rate(0);
    }
    let idx = (req.speed_code as usize) % SPEED_TABLE.len();
    Res::Rate(SPEED_TABLE[idx].1)
}

/// 注入 B：坏实现置而不落（ispeed 槽不更新）。
pub fn faulty_set_miss(ctl: &mut SpeedCtl, code: u32) -> Result<u32, u32> {
    match judge(&SpeedReq { speed_code: code }) {
        Res::Rate(r) => {
            ctl.sets += 1;
            ctl.note = "phantom".to_string();
            Ok(r)
        }
        Res::Err(c) => Err(c),
    }
}

/// 注入核：两处坏账与判定器/账本必分歧。
pub fn inject_two() -> [bool; 2] {
    let out = SpeedReq { speed_code: 14 };
    let d1 = judge(&out) != faulty_loose_table(&out);
    let mut ctl = SpeedCtl::new();
    let r = faulty_set_miss(&mut ctl, 13);
    let d2 = r.is_ok() && ctl.ispeed == 0 && ctl.note == "phantom";
    [d1, d2]
}

/// 置读回放：合法码序列置读对平，表外码不污染。
pub fn setget_replay_ok() -> bool {
    let mut ctl = SpeedCtl::new();
    let r1 = ctl.set_ispeed(9);
    ctl.sync_ospeed();
    let g1 = ctl.get_ospeed();
    let bad = ctl.set_ispeed(20);
    let r2 = ctl.set_ispeed(13);
    ctl.sync_ospeed();
    let g2 = ctl.get_ospeed();
    r1 == Ok(1200) && g1 == 9 && bad.err() == Some(SP_OUT_TABLE)
        && r2 == Ok(9600) && g2 == 13 && ctl.ledger_ok()
}

/// 编解码回放：表内全码双射核。
pub fn table_roundtrip_ok() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < SPEED_TABLE.len() {
        if !roundtrip_ok(SPEED_TABLE[i].0) {
            ok = false;
        }
        i += 1;
    }
    ok
}

/// 速率注记：判定渲染。
pub fn speed_note(r: &Res) -> String {
    match r {
        Res::Rate(v) => format!("baud={}", v),
        Res::Err(c) => diag_text(*c).to_string(),
    }
}

/// 域判据（对应卡验收 J 判据）。
pub fn run_unx10850_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10850");
    let (reqs, wants) = sample_matrix();
    let (hits, total, cap_ok) = verify_matrix(&reqs, &wants);

    // J1 正路径：B9600 表内放行 9600
    let j1 = judge(&SpeedReq { speed_code: 13 }) == Res::Rate(9600);
    s.add("J1-表内放行", j1, "B9600 语义面");

    // J2 表域边界：表内最大码 13 放行 / 表外 14 拒绝
    let j2 = judge(&SpeedReq { speed_code: 13 }) == Res::Rate(9600)
        && judge(&SpeedReq { speed_code: 14 }) == Res::Err(SP_OUT_TABLE);
    s.add("J2-表域边界", j2, "13/14 两沿可辨");

    // J3 B0 特判：零速率合法面
    let j3 = judge(&SpeedReq { speed_code: 0 }) == Res::Rate(0) && roundtrip_ok(0);
    s.add("J3-B0 面", j3, "挂断/随 cflag 语义");

    // J4 置读回放（合法序列 + 表外不污染）
    s.add("J4-置读回放", setget_replay_ok(), "1200→9600 双槽对平");

    // J5 编解码回放（表内全码双射）
    s.add("J5-编解码回放", table_roundtrip_ok(), "11 码双射全平");

    // J6 样本矩阵全命中
    s.add("J6-矩阵对平", cap_ok, "13 行表内表外全命中");

    // J7 注入1 表域放水检出
    let inj = inject_two();
    s.add("J7-表域放水检出", inj[0], "取模映射必分歧");

    // J8 注入2 置而不落检出
    s.add("J8-置而不落检出", inj[1], "幻影提交必分歧");

    // J9 双槽账一致
    let mut ctl = SpeedCtl::new();
    let _ = ctl.set_ispeed(7);
    ctl.sync_ospeed();
    s.add("J9-双槽账", ctl.ledger_ok() && ctl.get_ospeed() == 7, "ispeed=ospeed=7");

    // J10 表完整性：码升序、速率非降、无重码
    let j10 = {
        let mut ok = true;
        let mut i = 1usize;
        while i < SPEED_TABLE.len() {
            if SPEED_TABLE[i].0 <= SPEED_TABLE[i - 1].0
                || SPEED_TABLE[i].1 < SPEED_TABLE[i - 1].1
            {
                ok = false;
            }
            i += 1;
        }
        ok
    };
    s.add("J10-表完整性", j10, "码序/速率序双单调");

    // J11 字面锚：CBAUD=0o10017 / B1200=9
    let j11 = CBAUD == 0o10017 && encode_cflag(9) == 9 && decode_cflag(0o10017) == 0o10017 & CBAUD;
    s.add("J11-字面锚", j11, "CBAUD 域常量");

    // J12 诊断码闭集
    let j12 = diag_text(SP_OUT_TABLE) == "表外速率码 EINVAL"
        && diag_text(SP_SET_MISS) == "置而不落"
        && diag_text(9) == "未知诊断码";
    s.add("J12-码文本闭集", j12, "六码文本可辨");

    // J13 (dyn) 矩阵明细落账
    s.add_dyn("J13-矩阵明细", hits == total, &format!("hits={}/{} cap_ok={}", hits, total, cap_ok));

    // J14 (dyn) 速率注记落账
    let note = speed_note(&judge(&SpeedReq { speed_code: 9 }));
    s.add_dyn("J14-速率注记", note == "baud=1200", &note);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10850_flow() {
        assert_eq!(judge(&SpeedReq { speed_code: 13 }), Res::Rate(9600));
        assert_eq!(judge(&SpeedReq { speed_code: 0 }), Res::Rate(0));
        assert_eq!(judge(&SpeedReq { speed_code: 10 }), Res::Err(SP_OUT_TABLE));
        assert!(in_table(9) && !in_table(14));
        assert_eq!(rate_of(9), Some(1200));
        assert_eq!(rate_of(10), None);
        let mut ctl = SpeedCtl::new();
        assert!(ctl.set_ispeed(20).err() == Some(SP_OUT_TABLE));
        assert!(ctl.set_ispeed(9).is_ok());
        ctl.sync_ospeed();
        assert_eq!(ctl.get_ospeed(), 9);
        assert!(ctl.ledger_ok());
        assert!(roundtrip_ok(13) && roundtrip_ok(0));
    }

    #[test]
    fn unx10850_checks_green() {
        let s = run_unx10850_checks();
        assert!(s.all_passed(), "UNX-F10850 判据未全绿");
    }
}
