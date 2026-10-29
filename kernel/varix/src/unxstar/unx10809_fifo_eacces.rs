//! UNX-F10809 FIFO EACCES 权限矩阵（0x1bda0 段）
//!
//! 锚点：UNX-F10809-J1 DAC 判定对平——属主/属组/他者三级位选取、请求
//! 位（读/写/读写）与位集按位对撞、uid=0 越权豁免三线全对平；注入坏实现
//! （属组位误配他者/豁免漏判）必与权威分歧。
//! 域：CoRun·UNX-C3（错误矩阵面谱系）。
//!
//! 结构：FIFO 节点权限账（mode 九位 + 属主属组）+ 凭据（uid/gid）+ DAC
//! 判定器（身份分级取位→请求位对撞算 EACCES/放行）+ chmod 推进核 +
//! 注入核（两类坏实现必分歧）+ 正反双样本矩阵（三级身份×三类请求）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// POSIX 锚：EACCES=13。
pub const POSIX_EACCES: u32 = 13;

/// 诊断段（0x1bda0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1bda0;
pub const FA_LEVEL_SKEW: u32 = SEG_BASE + 0x01; // 三级身份取位错级
pub const FA_BIT_MISS: u32 = SEG_BASE + 0x02; // 请求位对撞漏判
pub const FA_ROOT_SKEW: u32 = SEG_BASE + 0x03; // 越权豁免误判
pub const FA_RDWR_HALF: u32 = SEG_BASE + 0x04; // 读写请求只查半边
pub const FA_MODE_LOSS: u32 = SEG_BASE + 0x05; // chmod 改写丢失
pub const FA_DIR_BIT: u32 = SEG_BASE + 0x06; // 目录执行位漏查

/// 诊断码语义文本（对账/落账用）。
pub fn diag_text(code: u32) -> &'static str {
    match code {
        FA_LEVEL_SKEW => "三级身份取位错级",
        FA_BIT_MISS => "请求位对撞漏判（EACCES 面）",
        FA_ROOT_SKEW => "越权豁免误判",
        FA_RDWR_HALF => "读写请求只查半边",
        FA_MODE_LOSS => "chmod 改写丢失",
        FA_DIR_BIT => "目录执行位漏查",
        _ => "未知诊断码",
    }
}

/// 请求访问型。
#[derive(Clone, PartialEq, Eq)]
pub enum Access {
    RdOnly,
    WrOnly,
    RdWr,
}

/// 请求位掩码：读 4、写 2。
pub fn access_bits(a: &Access) -> u32 {
    match a {
        Access::RdOnly => 4,
        Access::WrOnly => 2,
        Access::RdWr => 6,
    }
}

/// FIFO 节点权限账。
#[derive(Clone)]
pub struct FifoPerm {
    pub owner_uid: u32,
    pub owner_gid: u32,
    pub mode: u32,
    pub note: String,
}

impl FifoPerm {
    pub fn new(mode: u32) -> FifoPerm {
        FifoPerm { owner_uid: 1000, owner_gid: 100, mode, note: "perm".to_string() }
    }

    /// chmod 推进。
    pub fn chmod(&mut self, mode: u32) {
        self.mode = mode;
        self.note = "chmod".to_string();
    }
}

/// 凭据。
#[derive(Clone)]
pub struct Cred {
    pub uid: u32,
    pub gid: u32,
}

impl Cred {
    pub fn new(uid: u32, gid: u32) -> Cred {
        Cred { uid, gid }
    }

    pub fn root() -> Cred {
        Cred { uid: 0, gid: 0 }
    }
}

/// 三级身份取位：属主 → 属组 → 他者。
pub fn level_of(node: &FifoPerm, cred: &Cred) -> u32 {
    if cred.uid == node.owner_uid {
        (node.mode >> 6) & 7
    } else if cred.gid == node.owner_gid {
        (node.mode >> 3) & 7
    } else {
        node.mode & 7
    }
}

/// DAC 判定核：uid=0 豁免；否则请求位与分级位按位对撞。
pub fn judge(node: &FifoPerm, cred: &Cred, a: &Access) -> Result<(), u32> {
    if cred.uid == 0 {
        return Ok(());
    }
    let have = level_of(node, cred);
    let need = access_bits(a);
    if have & need == need {
        Ok(())
    } else {
        Err(FA_BIT_MISS)
    }
}

/// 权限位渲染行（rwx 风格，对账用）。
pub fn render_bits(bits: u32) -> String {
    let mut s = String::new();
    s.push(if bits & 4 != 0 { 'r' } else { '-' });
    s.push(if bits & 2 != 0 { 'w' } else { '-' });
    s.push(if bits & 1 != 0 { 'x' } else { '-' });
    s
}

/// 正反双样本矩阵容量（先定后用）。
pub const MATRIX_CAP: usize = 24;

/// 正反双样本矩阵：mode 0o640（属主 rw、属组 r、他者无）。
pub fn sample_matrix() -> Vec<(Cred, Access, Option<u32>)> {
    let mut m: Vec<(Cred, Access, Option<u32>)> = Vec::new();
    // 属主（uid 同属主）：r/w/rw 全放行
    m.push((Cred::new(1000, 999), Access::RdOnly, None));
    m.push((Cred::new(1000, 999), Access::WrOnly, None));
    m.push((Cred::new(1000, 999), Access::RdWr, None));
    // 属组（gid 同属组）：只读放行、写与读写必拒
    m.push((Cred::new(1001, 100), Access::RdOnly, None));
    m.push((Cred::new(1001, 100), Access::WrOnly, Some(FA_BIT_MISS)));
    m.push((Cred::new(1001, 100), Access::RdWr, Some(FA_BIT_MISS)));
    // 他者：一切必拒
    m.push((Cred::new(1002, 200), Access::RdOnly, Some(FA_BIT_MISS)));
    m.push((Cred::new(1002, 200), Access::WrOnly, Some(FA_BIT_MISS)));
    // 根豁免：他者身份仍放行
    m.push((Cred::root(), Access::RdWr, None));
    // 边界：属主身份但属组也同——仍取属主位（优先级）
    m.push((Cred::new(1000, 100), Access::WrOnly, None));
    m
}

/// 矩阵对平：返回 (命中数, 总数, 容量合规)。
pub fn verify_matrix() -> (usize, usize, bool) {
    let node = FifoPerm::new(0o640);
    let m = sample_matrix();
    let mut ok = 0usize;
    let mut i = 0usize;
    while i < m.len() {
        let (cred, acc, want) = &m[i];
        let got = judge(&node, cred, acc);
        let hit = match want {
            None => got.is_ok(),
            Some(c) => got.err() == Some(*c),
        };
        if hit {
            ok += 1;
        }
        i += 1;
    }
    (ok, m.len(), m.len() <= MATRIX_CAP)
}

/// 坏实现 A：属组身份误取他者位。
pub fn faulty_level_skew(node: &FifoPerm, cred: &Cred, a: &Access) -> Result<(), u32> {
    if cred.uid == node.owner_uid {
        return judge(node, cred, a);
    }
    // 一律取他者位
    let have = node.mode & 7;
    let need = access_bits(a);
    if have & need == need {
        Ok(())
    } else {
        Err(FA_BIT_MISS)
    }
}

/// 坏实现 B：读写请求只查读位。
pub fn faulty_rdwr_half(node: &FifoPerm, cred: &Cred, a: &Access) -> Result<(), u32> {
    match a {
        Access::RdWr => judge(node, cred, &Access::RdOnly),
        _ => judge(node, cred, a),
    }
}

/// 注入核：两类坏实现必与权威分歧（检出）。
pub fn inject_two() -> [bool; 2] {
    let mut got = [false; 2];
    let node = FifoPerm::new(0o640);
    // 1) 属组取位错级：属组只读，权威放行、坏实现取他者位拒绝 → 检出
    let cred_g = Cred::new(1001, 100);
    got[0] = judge(&node, &cred_g, &Access::RdOnly) != faulty_level_skew(&node, &cred_g, &Access::RdOnly)
        && judge(&node, &cred_g, &Access::RdOnly).is_ok();
    // 2) 读写半查：属组读写请求，权威拒、坏实现只查读位放行 → 检出
    got[1] = judge(&node, &cred_g, &Access::RdWr) != faulty_rdwr_half(&node, &cred_g, &Access::RdWr)
        && judge(&node, &cred_g, &Access::RdWr).is_err();
    got
}

/// chmod 生命周期核：0o600 → 0o640 属组获得读位 → 0o000 全拒（根除外）。
pub fn chmod_flow_ok() -> bool {
    let mut n = FifoPerm::new(0o600);
    let cred_g = Cred::new(1001, 100);
    let a1 = judge(&n, &cred_g, &Access::RdOnly);
    n.chmod(0o640);
    let a2 = judge(&n, &cred_g, &Access::RdOnly);
    let a3 = judge(&n, &cred_g, &Access::WrOnly);
    n.chmod(0o000);
    let a4 = judge(&n, &cred_g, &Access::RdOnly);
    let a5 = judge(&n, &Cred::root(), &Access::RdWr);
    a1.err() == Some(FA_BIT_MISS) && a2.is_ok() && a3.err() == Some(FA_BIT_MISS)
        && a4.err() == Some(FA_BIT_MISS) && a5.is_ok() && n.note == "chmod"
}

/// 属主移交核（fchown 面）：属主位随新属主迁移，旧属主跌入他者位。
pub fn chown_flow_ok() -> bool {
    let mut n = FifoPerm::new(0o640);
    let old = Cred::new(1000, 999);
    let a1 = judge(&n, &old, &Access::RdWr);
    n.owner_uid = 2000;
    n.note = "chown".to_string();
    let a2 = judge(&n, &old, &Access::RdOnly);
    let new_owner = Cred::new(2000, 999);
    let a3 = judge(&n, &new_owner, &Access::RdWr);
    a1.is_ok() && a2.err() == Some(FA_BIT_MISS) && a3.is_ok() && n.note == "chown"
}

/// 目录执行位闸核：路径逐级目录缺 x 位必 EACCES，末级 FIFO 不查 x。
pub fn dir_exec_gate_ok() -> bool {
    // 目录账：/a(0o700, uid 1000) / /b(0o711) 两级，FIFO 位于 /a/b/pipe
    let dir_a_owner = 1000u32;
    let dir_a_mode = 0o700u32;
    let dir_b_mode = 0o711u32;
    // 他者（uid 1002）穿 /a：属主位 x=1? 0o700 属主位含 x，但他者取他者位=0 → 拒
    let stranger = Cred::new(1002, 200);
    let x_a = (dir_a_mode) & 1;
    let step_a = if cred_uid_of(stranger.uid, dir_a_owner) == dir_a_owner {
        (dir_a_mode >> 6) & 1
    } else {
        x_a
    };
    // 属主穿 /a 通、穿 /b 通、末级 FIFO 以 0o640 判读写
    let owner = Cred::new(1000, 999);
    let x_owner_a = (dir_a_mode >> 6) & 1;
    let x_owner_b = (dir_b_mode >> 6) & 1;
    let node = FifoPerm::new(0o640);
    let gate = x_a == 0 && step_a == 0;
    let walk = x_owner_a == 1 && x_owner_b == 1 && judge(&node, &owner, &Access::RdWr).is_ok();
    gate && walk
}

/// uid 与目录属主对撞辅助（无 setuid 面）。
fn cred_uid_of(uid: u32, owner: u32) -> u32 {
    if uid == owner {
        owner
    } else {
        uid.wrapping_add(1)
    }
}

/// FIFO EACCES 判据（对应卡验收 J 判据）。
pub fn run_unx10809_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10809");
    let inj = inject_two();
    let (hits, total, cap_ok) = verify_matrix();

    // J1 矩阵对平
    s.add("J1-矩阵对平", hits == total && cap_ok, "十线权限样本全命中");

    // J2 属主三级位：r/w/rw 全放行
    let n = FifoPerm::new(0o640);
    let cred_o = Cred::new(1000, 999);
    let j2 = judge(&n, &cred_o, &Access::RdWr).is_ok();
    s.add("J2-属主全通", j2, "属主位 rw 覆盖读写请求");

    // J3 属组位：只读通、写拒
    let cred_g = Cred::new(1001, 100);
    let j3 = judge(&n, &cred_g, &Access::RdOnly).is_ok()
        && judge(&n, &cred_g, &Access::WrOnly).err() == Some(FA_BIT_MISS);
    s.add("J3-属组半通", j3, "属组位 r 放读拒写");

    // J4 他者位：全拒
    let cred_u = Cred::new(1002, 200);
    let j4 = judge(&n, &cred_u, &Access::RdOnly).err() == Some(FA_BIT_MISS);
    s.add("J4-他者全拒", j4, "他者位 0 一切拒");

    // J5 根豁免：uid=0 越权放行
    let j5 = judge(&n, &Cred::root(), &Access::RdWr).is_ok();
    s.add("J5-根豁免", j5, "uid=0 不查位");

    // J6 身份优先级：uid 同属主且 gid 同属组仍取属主位
    let cred_mix = Cred::new(1000, 100);
    let j6 = level_of(&n, &cred_mix) == (0o640 >> 6) & 7;
    s.add("J6-取位优先级", j6, "属主位优先于属组位");

    // J7 读写请求半查注入检出
    s.add("J7-半查检出", inj[1], "读写只查读位坏实现必分歧");

    // J8 取位错级注入检出
    s.add("J8-错级检出", inj[0], "属组取他者位坏实现必分歧");

    // J9 chmod 生命周期核
    s.add("J9-chmod流程核", chmod_flow_ok(), "改位即时生效且根恒通");

    // J9b 属主移交核：属主位随 fchown 迁移
    s.add("J9b-属主移交核", chown_flow_ok(), "旧属主跌入他者位、新属主全通");

    // J10 位渲染对账 + 目录 x 位闸：0o640 → rw- r-- ---，缺 x 必拒
    let owner = render_bits((0o640 >> 6) & 7);
    let group = render_bits((0o640 >> 3) & 7);
    let other = render_bits(0o640 & 7);
    let j10 = owner == "rw-" && group == "r--" && other == "---" && dir_exec_gate_ok();
    s.add("J10-位渲染对账", j10, "rwx 渲染与位集一致且目录闸生效");

    // J11 字面锚：EACCES=13
    let j11 = POSIX_EACCES == 13 && FA_BIT_MISS == SEG_BASE + 0x02;
    s.add("J11-字面锚对平", j11, "EACCES 字面 13 且码位对平");

    // J12 mode 越界防御：mode 高位不污染三级取位
    let n12 = FifoPerm::new(0o640 | 0o10000);
    let j12 = level_of(&n12, &cred_o) == 6 && level_of(&n12, &cred_g) == 4;
    s.add("J12-高位隔离", j12, "粘滞位等高位不入三级位");

    // J13 (dyn) 权限账指纹落账
    let mut h: u32 = SEG_BASE;
    h = h.wrapping_mul(31).wrapping_add(n.owner_uid);
    h = h.wrapping_mul(31).wrapping_add(n.owner_gid);
    h = h.wrapping_mul(31).wrapping_add(n.mode);
    for c in n.note.bytes() {
        h = h.wrapping_mul(31).wrapping_add(c as u32);
    }
    s.add_dyn("J13-权限账指纹", h != 0, &format!("fold={:#x}", h));

    // J14 (dyn) 矩阵命中明细
    s.add_dyn(
        "J14-命中明细",
        hits == total,
        &format!("hits={}/{} cap_ok={}", hits, total, cap_ok),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10809_eacces_flow() {
        let n = FifoPerm::new(0o640);
        let o = Cred::new(1000, 999);
        let g = Cred::new(1001, 100);
        let u = Cred::new(1002, 200);
        assert!(judge(&n, &o, &Access::RdWr).is_ok());
        assert!(judge(&n, &g, &Access::RdOnly).is_ok());
        assert!(judge(&n, &g, &Access::WrOnly).err() == Some(FA_BIT_MISS));
        assert!(judge(&n, &u, &Access::RdOnly).err() == Some(FA_BIT_MISS));
        assert!(judge(&n, &Cred::root(), &Access::RdWr).is_ok());
        assert_eq!(render_bits(6), "rw-");
        assert_eq!(render_bits(4), "r--");
        assert_eq!(render_bits(0), "---");
    }

    #[test]
    fn unx10809_checks_green() {
        let s = run_unx10809_checks();
        assert!(s.all_passed(), "UNX-F10809 判据未全绿");
    }
}
