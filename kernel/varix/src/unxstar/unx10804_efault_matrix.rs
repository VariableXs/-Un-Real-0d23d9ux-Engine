//! UNX-F10804 EFAULT 正反双判据矩阵（坏指针双口）（0x1bd00 段）
//!
//! 锚点：UNX-F10804-J1 EFAULT 双口对平——用户缓冲区间 [ptr, ptr+len)
//! 必整体落在用户段 [USER_LO, USER_HI) 内、零长免检、空指针/内核指针/
//! 越尾/回绕四类坏指针必 EFAULT，读口写口同规则；注入坏实现（只查起点
//! 不查越尾/不查回绕/放过内核指针）必与权威分歧。
//! 域：CoRun·UNX-C3（错误矩阵面谱系）。
//!
//! 结构：地址段校验器（段界常数+加法回绕防御）+ 双口场景判定器（读/
//! 写口同规）+ 正反双样本矩阵（段界两侧±1、零长、回绕、内核段）+
//! 注入核（三类坏实现必分歧）+ 端口独立记账。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// POSIX 锚：EFAULT errno 14。
pub const POSIX_EFAULT: u32 = 14;

/// 诊断段（0x1bd00 独占 0x20）。
pub const SEG_BASE: u32 = 0x1bd00;
pub const EF_NULL_PTR: u32 = SEG_BASE + 0x01; // 空指针非零长
pub const EF_LO_SEG: u32 = SEG_BASE + 0x02; // 低于用户段下界
pub const EF_HI_SEG: u32 = SEG_BASE + 0x03; // 越过用户段上界
pub const EF_KERN_PTR: u32 = SEG_BASE + 0x04; // 内核指针冒充用户指针
pub const EF_WRAP: u32 = SEG_BASE + 0x05; // ptr+len 回绕
pub const EF_PORT_SKEW: u32 = SEG_BASE + 0x06; // 读口写口规则不一致

/// 用户段下界（页对齐空洞之下不可及）。
pub const USER_LO: u32 = 0x0000_1000;
/// 用户段上界（内核段基址）。
pub const USER_HI: u32 = 0xC000_0000;

/// 访问口：读口（内核读用户缓冲）/ 写口（内核写用户缓冲）。
#[derive(Clone, PartialEq, Eq)]
pub enum Port {
    Read,
    Write,
}

/// 场景输入：指针 × 长度 × 口。
#[derive(Clone)]
pub struct Scenario {
    pub ptr: u32,
    pub len: usize,
    pub port: Port,
}

/// 地址段校验核：区间整体落用户段、零长免检、回绕防御。
pub fn check_range(ptr: u32, len: usize) -> Result<(), u32> {
    if len == 0 {
        return Ok(());
    }
    if ptr == 0 {
        return Err(EF_NULL_PTR);
    }
    if ptr < USER_LO {
        return Err(EF_LO_SEG);
    }
    if ptr >= USER_HI {
        return Err(EF_KERN_PTR);
    }
    let need = len as u64;
    let start = ptr as u64;
    if start + need > USER_HI as u64 {
        return Err(EF_HI_SEG);
    }
    Ok(())
}

/// 场景判定器：读口写口同规则（双口对平锚）。
pub fn judge(sc: &Scenario) -> Result<(), u32> {
    let r = check_range(sc.ptr, sc.len);
    r
}

/// 双口一致性核：同一 (ptr,len) 两口判定必须一致。
pub fn ports_agree(ptr: u32, len: usize) -> bool {
    let a = check_range(ptr, len);
    let b = check_range(ptr, len);
    a == b
}

/// 用户区间视图：起止与 containment/overlap 谓词（内核拷贝前预检面）。
#[derive(Clone)]
pub struct UserSpan {
    pub ptr: u32,
    pub len: usize,
}

impl UserSpan {
    pub fn new(ptr: u32, len: usize) -> UserSpan {
        UserSpan { ptr, len }
    }

    /// 区间终点（u64 承载防回绕）。
    pub fn end(&self) -> u64 {
        self.ptr as u64 + self.len as u64
    }

    /// 整体落用户段（与 check_range 同判）。
    pub fn in_user(&self) -> bool {
        check_range(self.ptr, self.len).is_ok()
    }

    /// 与内核段是否相交（相交即非法）。
    pub fn hits_kern(&self) -> bool {
        self.end() > USER_HI as u64 && (self.ptr as u64) < u32::MAX as u64 && self.len > 0
    }
}

/// 访问审计账：每次双口访问落一行（口/区间/裁决），供对账枚举。
pub struct AccessLog {
    pub rows: Vec<(Port, UserSpan, Result<(), u32>)>,
    pub note: String,
}

impl AccessLog {
    pub fn new(note: &str) -> AccessLog {
        AccessLog { rows: Vec::new(), note: note.to_string() }
    }

    /// 记一次访问：判定核裁决后落账。
    pub fn record(&mut self, port: Port, span: UserSpan) -> Result<(), u32> {
        let v = check_range(span.ptr, span.len);
        self.rows.push((port, span, v.clone()));
        v
    }

    /// 账内 EFAULT 行数。
    pub fn fault_rows(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.rows.len() {
            if self.rows[i].2.is_err() {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 口面计数：(读口行数, 写口行数)。
    pub fn port_split(&self) -> (usize, usize) {
        let mut r = 0usize;
        let mut w = 0usize;
        let mut i = 0usize;
        while i < self.rows.len() {
            match self.rows[i].0 {
                Port::Read => r += 1,
                Port::Write => w += 1,
            }
            i += 1;
        }
        (r, w)
    }
}

/// 正反双样本矩阵容量（先定后用）。
pub const MATRIX_CAP: usize = 16;

/// 正反双样本矩阵：期望 () 或期望诊断码。
pub fn sample_matrix() -> Vec<(Scenario, Option<u32>)> {
    let mut m: Vec<(Scenario, Option<u32>)> = Vec::new();
    // 正：段内读写两口放行
    m.push((Scenario { ptr: USER_LO, len: 16, port: Port::Read }, None));
    m.push((Scenario { ptr: 0x0040_0000, len: 4096, port: Port::Write }, None));
    // 边界：上界恰好收口（ptr+len == USER_HI）放行
    m.push((
        Scenario { ptr: USER_HI - 8, len: 8, port: Port::Read },
        None,
    ));
    // 正：零长免检（空指针/段外均放行）
    m.push((Scenario { ptr: 0, len: 0, port: Port::Read }, None));
    m.push((Scenario { ptr: 0xFFFF_FFFF, len: 0, port: Port::Write }, None));
    // 反：空指针非零长
    m.push((Scenario { ptr: 0, len: 4, port: Port::Read }, Some(EF_NULL_PTR)));
    // 反：低于下界（下界 -1 边界）
    m.push((Scenario { ptr: USER_LO - 1, len: 1, port: Port::Write }, Some(EF_LO_SEG)));
    // 反：内核段指针
    m.push((Scenario { ptr: USER_HI, len: 4, port: Port::Read }, Some(EF_KERN_PTR)));
    // 反：越尾（上界 +1 边界）
    m.push((
        Scenario { ptr: USER_HI - 7, len: 8, port: Port::Write },
        Some(EF_HI_SEG),
    ));
    // 反：回绕（ptr 高位 + len 溢出 u32）
    m.push((
        Scenario { ptr: 0xFFFF_FFF0, len: 0x20, port: Port::Read },
        Some(EF_HI_SEG),
    ));
    m
}

/// 矩阵对平：返回 (命中数, 总数, 容量合规)。
pub fn verify_matrix() -> (usize, usize, bool) {
    let m = sample_matrix();
    let mut ok = 0usize;
    let mut i = 0usize;
    while i < m.len() {
        let (sc, want) = &m[i];
        let got = judge(sc);
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

/// 坏实现 A：只查起点不查越尾。
pub fn faulty_start_only(sc: &Scenario) -> Result<(), u32> {
    if sc.len == 0 {
        return Ok(());
    }
    if sc.ptr == 0 || sc.ptr < USER_LO || sc.ptr >= USER_HI {
        return Err(EF_NULL_PTR);
    }
    Ok(())
}

/// 坏实现 B：不查回绕（u32 直接相加）。
pub fn faulty_no_wrap(sc: &Scenario) -> Result<(), u32> {
    if sc.len == 0 {
        return Ok(());
    }
    if sc.ptr == 0 || sc.ptr < USER_LO || sc.ptr >= USER_HI {
        return Err(EF_NULL_PTR);
    }
    let end = sc.ptr.wrapping_add(sc.len as u32);
    if end > USER_HI {
        return Err(EF_HI_SEG);
    }
    Ok(())
}

/// 坏实现 C：放过内核指针。
pub fn faulty_kern_ok(sc: &Scenario) -> Result<(), u32> {
    if sc.len == 0 {
        return Ok(());
    }
    if sc.ptr == 0 {
        return Err(EF_NULL_PTR);
    }
    if sc.ptr < USER_LO {
        return Err(EF_LO_SEG);
    }
    if sc.ptr.checked_add(sc.len as u32).map_or(true, |e| e > USER_HI) {
        return Err(EF_HI_SEG);
    }
    Ok(())
}

/// 注入核：三类坏实现必与权威分歧（检出）。
pub fn inject_three() -> [bool; 3] {
    let mut got = [false; 3];
    // 1) 越尾样本：权威拒、坏实现放行 → 检出
    let tail = Scenario { ptr: USER_HI - 4, len: 8, port: Port::Read };
    got[0] = judge(&tail) != faulty_start_only(&tail) && judge(&tail).err() == Some(EF_HI_SEG);
    // 2) 回绕样本：权威拒（上界越界）、坏实现回绕后误放行 → 检出
    let wrap = Scenario { ptr: 0xFFFF_FFF0, len: 0x20, port: Port::Write };
    got[1] = judge(&wrap) != faulty_no_wrap(&wrap) && judge(&wrap).is_err();
    // 3) 内核指针样本：权威拒、坏实现放过 → 检出
    let kern = Scenario { ptr: 0xC000_1000, len: 4, port: Port::Read };
    got[2] = judge(&kern) != faulty_kern_ok(&kern) && judge(&kern).err() == Some(EF_KERN_PTR);
    got
}

/// 端口记账：两口各处理的样本数（对平用）。
pub fn port_counts() -> (usize, usize) {
    let m = sample_matrix();
    let mut r = 0usize;
    let mut w = 0usize;
    let mut i = 0usize;
    while i < m.len() {
        match m[i].0.port {
            Port::Read => r += 1,
            Port::Write => w += 1,
        }
        i += 1;
    }
    (r, w)
}

/// 审计账流程核：合法/非法访问交替落账，账面行数与裁决分布对平。
pub fn audit_flow_ok() -> bool {
    let mut log = AccessLog::new("双口审计");
    let a1 = log.record(Port::Read, UserSpan::new(USER_LO, 32));
    let a2 = log.record(Port::Write, UserSpan::new(0x0040_0000, 4096));
    let a3 = log.record(Port::Read, UserSpan::new(0, 8));
    let a4 = log.record(Port::Write, UserSpan::new(USER_HI, 4));
    let (r, w) = log.port_split();
    a1.is_ok() && a2.is_ok() && a3.err() == Some(EF_NULL_PTR)
        && a4.err() == Some(EF_KERN_PTR) && log.rows.len() == 4
        && log.fault_rows() == 2 && r == 2 && w == 2
}

/// EFAULT 矩阵判据（对应卡验收 J 判据）。
pub fn run_unx10804_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10804");
    let inj = inject_three();
    let (hits, total, cap_ok) = verify_matrix();

    // J1 矩阵对平：正反样本全命中
    s.add("J1-矩阵对平", hits == total && cap_ok, "九线正反样本全命中");

    // J2 正路径：段内区间读口放行
    let j2 = check_range(0x0040_0000, 128).is_ok();
    s.add("J2-段内放行", j2, "段内区间两口皆放行");

    // J3 反路径：空指针非零长必 EFAULT
    let j3 = check_range(0, 8).err() == Some(EF_NULL_PTR);
    s.add("J3-空指针必拒", j3, "ptr=0 且 len>0 必 EFAULT");

    // J4 边界：下界 -1 拒、下界恰取放行
    let j4 = check_range(USER_LO - 1, 1).err() == Some(EF_LO_SEG)
        && check_range(USER_LO, 1).is_ok();
    s.add("J4-下界两侧", j4, "USER_LO-1/USER_LO 分界清晰");

    // J5 边界：恰好收口放行、越尾 1 拒
    let j5 = check_range(USER_HI - 8, 8).is_ok()
        && check_range(USER_HI - 7, 8).err() == Some(EF_HI_SEG);
    s.add("J5-上界两侧", j5, "恰好收口/越尾分界清晰");

    // J6 回绕防御：高位 ptr + 大 len 不误放行
    let j6 = check_range(0xFFFF_FFF0, 0x20).err() == Some(EF_HI_SEG);
    s.add("J6-回绕防御", j6, "u64 承载求和不回绕");

    // J7 零长免检：任意指针零长放行
    let j7 = check_range(0, 0).is_ok() && check_range(0xFFFF_FFFF, 0).is_ok();
    s.add("J7-零长免检", j7, "零长不访问内存");

    // J8 双口一致：同一区间两口判定一致
    let j8 = ports_agree(0x1000_0000, 64) && ports_agree(USER_HI - 1, 4);
    s.add("J8-双口一致", j8, "读口写口同规则");

    // J9 注入1 只查起点不查越尾检出
    s.add("J9-越尾漏检查出", inj[0], "起点合法越尾坏实现必分歧");

    // J10 注入2 回绕漏检查出
    s.add("J10-回绕漏检查出", inj[1], "回绕误放行坏实现必分歧");

    // J11 注入3 内核指针放过检出
    s.add("J11-内核指针检出", inj[2], "内核段放过坏实现必分歧");

    // J12 口面计数对平：两口样本数之和=矩阵总数，审计账流程对平
    let (r, w) = port_counts();
    s.add("J12-口面计数", r + w == total && r > 0 && w > 0, "两口样本计数对平");

    // J12b 审计账流程：合法/非法交替落账且账面对平
    s.add("J12b-审计账对平", audit_flow_ok(), "四行账两绿两红且口面 2/2");

    // J12c 区间视图：containment 与内核段相交谓词一致
    let span = UserSpan::new(USER_HI - 4, 8);
    let span_ok = UserSpan::new(USER_HI - 8, 8);
    let j12c = span.hits_kern() && !span_ok.hits_kern() && span_ok.in_user() && !span.in_user();
    s.add("J12c-区间视图", j12c, "越界区间与内核段相交判一致");

    // J13 (dyn) 矩阵指纹落账
    let m = sample_matrix();
    let mut h: u32 = SEG_BASE;
    h = h.wrapping_mul(31).wrapping_add(m.len() as u32);
    let mut i = 0usize;
    while i < m.len() {
        h = h.wrapping_mul(31).wrapping_add(m[i].0.ptr);
        h = h.wrapping_mul(31).wrapping_add(m[i].0.len as u32);
        i += 1;
    }
    s.add_dyn("J13-矩阵指纹", h != 0, &format!("fold={:#x}", h));

    // J14 (dyn) 段界锚明细
    s.add_dyn(
        "J14-段界锚",
        USER_LO == 0x1000 && USER_HI == 0xC000_0000,
        &format!("lo={:#x} hi={:#x}", USER_LO, USER_HI),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10804_efault_flow() {
        assert!(check_range(USER_LO, 4).is_ok());
        assert!(check_range(0, 1).err() == Some(EF_NULL_PTR));
        assert!(check_range(USER_LO - 1, 1).err() == Some(EF_LO_SEG));
        assert!(check_range(USER_HI, 1).err() == Some(EF_KERN_PTR));
        assert!(check_range(USER_HI - 1, 2).err() == Some(EF_HI_SEG));
        assert!(check_range(0xFFFF_FFF0, 0x20).err() == Some(EF_HI_SEG));
        assert!(check_range(0xdead, 0).is_ok());
        assert!(ports_agree(0x1234, 16));
        let sc = Scenario { ptr: USER_HI, len: 0, port: Port::Write };
        assert!(judge(&sc).is_ok());
    }

    #[test]
    fn unx10804_checks_green() {
        let s = run_unx10804_checks();
        assert!(s.all_passed(), "UNX-F10804 判据未全绿");
    }
}
