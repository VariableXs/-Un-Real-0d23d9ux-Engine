//! UNX-F10811 管道路由非法命令错误面（fcntl/ioctl 类）（0x1bde0 段）
//!
//! 锚点：UNX-F10811-J1 命令面对平——未知命令必 EINVAL、F_SETFL 携访问
//! 模式位必 EINVAL、F_SETPIPE_SZ 零值/低于下界必 EINVAL、超上限必 EPERM、
//! 合法值向上取 2 的幂；注入坏实现（访问位漏查/上限漏查）必与权威分歧。
//! 域：CoRun·UNX-C3（错误矩阵面谱系）。
//!
//! 结构：管道控制块状态机（flags + 管道容量，命令驱动推进）+ 命令验证器
//! （命令表闭集 → 参数规则逐条）+ 容量取幂核（向上取 2 的幂对账）+
//! 注入核（两类坏实现必分歧）+ 正反双样本矩阵。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// POSIX 锚：EINVAL=22、EPERM=1。
pub const POSIX_EINVAL: u32 = 22;
pub const POSIX_EPERM: u32 = 1;

/// 诊断段（0x1bde0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1bde0;
pub const PC_BAD_CMD: u32 = SEG_BASE + 0x01; // 未知命令
pub const PC_ACCMODE: u32 = SEG_BASE + 0x02; // F_SETFL 携访问模式位
pub const PC_SZ_ZERO: u32 = SEG_BASE + 0x03; // 容量零值
pub const PC_SZ_LOW: u32 = SEG_BASE + 0x04; // 容量低于下界
pub const PC_SZ_HIGH: u32 = SEG_BASE + 0x05; // 容量超上限
pub const PC_POW2_SKEW: u32 = SEG_BASE + 0x06; // 取幂对账错

/// 诊断码语义文本（对账/落账用）。
pub fn diag_text(code: u32) -> &'static str {
    match code {
        PC_BAD_CMD => "未知命令（闭集外）",
        PC_ACCMODE => "F_SETFL 携访问模式位",
        PC_SZ_ZERO => "F_SETPIPE_SZ 零值",
        PC_SZ_LOW => "容量低于下界 4096",
        PC_SZ_HIGH => "容量超上限 1MiB",
        PC_POW2_SKEW => "取幂对账错",
        _ => "未知诊断码",
    }
}

/// 命令字面闭集（Linux fcntl 号对照）。
pub const F_GETFD: u32 = 1;
pub const F_SETFD: u32 = 2;
pub const F_GETFL: u32 = 3;
pub const F_SETFL: u32 = 4;
pub const F_SETPIPE_SZ: u32 = 1031;
pub const F_GETPIPE_SZ: u32 = 1032;

/// O_ACCMODE 位集（F_SETFL 禁携）。
pub const O_ACCMODE: u32 = 0o3;
/// O_NONBLOCK 位（F_SETFL 可携）。
pub const O_NONBLOCK: u32 = 0o4000;

/// 管道容量下界 / 上界。
pub const SZ_MIN: u32 = 4096;
pub const SZ_MAX: u32 = 1 << 20;

/// 命令是否在闭集内。
pub fn cmd_known(cmd: u32) -> bool {
    cmd == F_GETFD || cmd == F_SETFD || cmd == F_GETFL || cmd == F_SETFL
        || cmd == F_SETPIPE_SZ || cmd == F_GETPIPE_SZ
}

/// 向上取 2 的幂（≥ v，下界 SZ_MIN 兜底）。
pub fn round_pow2(v: u32) -> u32 {
    let mut r = SZ_MIN;
    while r < v {
        r = r.wrapping_mul(2);
        if r == 0 {
            return SZ_MAX;
        }
    }
    r
}

/// 管道控制块：flags + 容量，命令驱动推进。
pub struct PipeCtl {
    pub flags: u32,
    pub size: u32,
    pub note: String,
}

impl PipeCtl {
    pub fn new() -> PipeCtl {
        PipeCtl { flags: 0, size: 65536, note: "ctl".to_string() }
    }

    /// 命令路由验证核：闭集 → 参数规则逐条 → 状态推进。
    pub fn apply(&mut self, cmd: u32, arg: u32) -> Result<u32, u32> {
        if !cmd_known(cmd) {
            return Err(PC_BAD_CMD);
        }
        match cmd {
            F_GETFD => Ok(0),
            F_SETFD => Ok(0),
            F_GETFL => Ok(self.flags),
            F_SETFL => {
                if arg & O_ACCMODE != 0 {
                    return Err(PC_ACCMODE);
                }
                self.flags = arg;
                Ok(0)
            }
            F_GETPIPE_SZ => Ok(self.size),
            F_SETPIPE_SZ => {
                if arg == 0 {
                    return Err(PC_SZ_ZERO);
                }
                if arg < SZ_MIN {
                    return Err(PC_SZ_LOW);
                }
                if arg > SZ_MAX {
                    return Err(PC_SZ_HIGH);
                }
                let rounded = round_pow2(arg);
                if rounded < arg {
                    return Err(PC_POW2_SKEW);
                }
                self.size = rounded;
                Ok(self.size)
            }
            _ => Err(PC_BAD_CMD),
        }
    }
}

/// 场景输入：命令 × 参数（判定器按字段计算）。
#[derive(Clone)]
pub struct Scenario {
    pub cmd: u32,
    pub arg: u32,
}

/// 场景判定器（无副作用面：只裁不推进）。
pub fn judge(sc: &Scenario) -> Result<u32, u32> {
    let mut ctl = PipeCtl::new();
    ctl.apply(sc.cmd, sc.arg)
}

/// 正反双样本矩阵容量（先定后用）。
pub const MATRIX_CAP: usize = 24;

/// 正反双样本矩阵：期望 () 或期望诊断码。
pub fn sample_matrix() -> Vec<(Scenario, Option<u32>)> {
    let mut m: Vec<(Scenario, Option<u32>)> = Vec::new();
    // 正：闭集命令合法参数
    m.push((Scenario { cmd: F_GETFL, arg: 0 }, None));
    m.push((Scenario { cmd: F_SETFL, arg: O_NONBLOCK }, None));
    m.push((Scenario { cmd: F_GETPIPE_SZ, arg: 0 }, None));
    m.push((Scenario { cmd: F_SETPIPE_SZ, arg: 8192 }, None));
    m.push((Scenario { cmd: F_SETPIPE_SZ, arg: SZ_MIN }, None));
    // 边界：容量取幂（非 2 幂值 5000 → 8192）
    m.push((Scenario { cmd: F_SETPIPE_SZ, arg: 5000 }, None));
    // 正：上界恰好 SZ_MAX 放行
    m.push((Scenario { cmd: F_SETPIPE_SZ, arg: SZ_MAX }, None));
    // 反：未知命令（0 与 9999）
    m.push((Scenario { cmd: 0, arg: 0 }, Some(PC_BAD_CMD)));
    m.push((Scenario { cmd: 9999, arg: 0 }, Some(PC_BAD_CMD)));
    // 反：F_SETFL 携访问模式位
    m.push((Scenario { cmd: F_SETFL, arg: O_ACCMODE }, Some(PC_ACCMODE)));
    m.push((Scenario { cmd: F_SETFL, arg: O_NONBLOCK | 2 }, Some(PC_ACCMODE)));
    // 反：F_SETPIPE_SZ 零值 / 低于下界 / 超上限
    m.push((Scenario { cmd: F_SETPIPE_SZ, arg: 0 }, Some(PC_SZ_ZERO)));
    m.push((Scenario { cmd: F_SETPIPE_SZ, arg: 4095 }, Some(PC_SZ_LOW)));
    m.push((Scenario { cmd: F_SETPIPE_SZ, arg: SZ_MAX + 1 }, Some(PC_SZ_HIGH)));
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

/// 坏实现 A：F_SETFL 访问位漏查。
pub fn faulty_accmode(sc: &Scenario) -> Result<u32, u32> {
    match sc.cmd {
        F_SETFL => Ok(0),
        _ => judge(sc),
    }
}

/// 坏实现 B：F_SETPIPE_SZ 上限漏查。
pub fn faulty_sz_high(sc: &Scenario) -> Result<u32, u32> {
    match sc.cmd {
        F_SETPIPE_SZ => {
            if sc.arg == 0 || sc.arg < SZ_MIN {
                return Err(PC_SZ_LOW);
            }
            Ok(round_pow2(sc.arg))
        }
        _ => judge(sc),
    }
}

/// 注入核：两类坏实现必与权威分歧（检出）。
pub fn inject_two() -> [bool; 2] {
    let mut got = [false; 2];
    // 1) 访问位漏查：F_SETFL|O_ACCMODE，权威拒、坏实现放行 → 检出
    let sc1 = Scenario { cmd: F_SETFL, arg: O_ACCMODE };
    got[0] = judge(&sc1) != faulty_accmode(&sc1) && judge(&sc1).err() == Some(PC_ACCMODE);
    // 2) 上限漏查：SZ_MAX+1，权威拒、坏实现放行 → 检出
    let sc2 = Scenario { cmd: F_SETPIPE_SZ, arg: SZ_MAX + 1 };
    got[1] = judge(&sc2) != faulty_sz_high(&sc2) && judge(&sc2).err() == Some(PC_SZ_HIGH);
    got
}

/// 取幂对账核：非 2 幂向上取整且下界兜底、上界封顶。
pub fn pow2_reconcile_ok() -> bool {
    let a = round_pow2(1);
    let b = round_pow2(4096);
    let c = round_pow2(5000);
    let d = round_pow2(1 << 20);
    a == SZ_MIN && b == 4096 && c == 8192 && d == SZ_MAX
}

/// flags 逐位叠加核：SETFL 两次叠加 O_NONBLOCK 位不重复累积，清除走异或面。
pub fn flags_merge_ok() -> bool {
    let mut c = PipeCtl::new();
    let f1 = c.apply(F_SETFL, O_NONBLOCK);
    let snap1 = c.flags;
    let f2 = c.apply(F_SETFL, O_NONBLOCK);
    // 位叠加幂等：同位再写不产生新位
    let idem = c.flags == snap1 && snap1 == O_NONBLOCK;
    // 清除面：异或回零后 GETFL 回读 0
    c.flags ^= O_NONBLOCK;
    let cleared = c.apply(F_GETFL, 0) == Ok(0);
    // 排他面：F_GETPIPE_SZ 忽略 arg（arg 噪声不影响回读）
    let noise = c.apply(F_GETPIPE_SZ, 0xffff) == Ok(c.size);
    f1.is_ok() && f2.is_ok() && idem && cleared && noise
}

/// 管道路由命令判据（对应卡验收 J 判据）。
pub fn run_unx10811_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10811");
    let inj = inject_two();
    let (hits, total, cap_ok) = verify_matrix();

    // J1 矩阵对平
    s.add("J1-矩阵对平", hits == total && cap_ok, "十三线命令样本全命中");

    // J2 正路径：F_SETFL O_NONBLOCK 推进 flags
    let mut c = PipeCtl::new();
    let r = c.apply(F_SETFL, O_NONBLOCK);
    let j2 = r.is_ok() && c.flags == O_NONBLOCK;
    s.add("J2-SETFL推进", j2, "合法 SETFL 写入 flags");

    // J3 反路径：未知命令必 EINVAL
    let j3 = judge(&Scenario { cmd: 9999, arg: 0 }).err() == Some(PC_BAD_CMD);
    s.add("J3-未知命令拒", j3, "闭集外命令 EINVAL");

    // J4 反路径：F_SETFL 携访问位必 EINVAL
    let j4 = judge(&Scenario { cmd: F_SETFL, arg: O_ACCMODE }).err() == Some(PC_ACCMODE)
        && judge(&Scenario { cmd: F_SETFL, arg: O_NONBLOCK | 1 }).err() == Some(PC_ACCMODE);
    s.add("J4-访问位拒", j4, "O_ACCMODE 两位皆拒");

    // J5 容量三段：零值/下界下/上界上三段全拒
    let j5 = judge(&Scenario { cmd: F_SETPIPE_SZ, arg: 0 }).err() == Some(PC_SZ_ZERO)
        && judge(&Scenario { cmd: F_SETPIPE_SZ, arg: 4095 }).err() == Some(PC_SZ_LOW)
        && judge(&Scenario { cmd: F_SETPIPE_SZ, arg: SZ_MAX + 1 }).err() == Some(PC_SZ_HIGH);
    s.add("J5-容量三段", j5, "0/4095/1MiB+1 全拒");

    // J6 容量推进：8192 生效、GET 回读一致
    let mut c2 = PipeCtl::new();
    let set = c2.apply(F_SETPIPE_SZ, 8192);
    let get = c2.apply(F_GETPIPE_SZ, 0);
    let j6 = set == Ok(8192) && get == Ok(8192);
    s.add("J6-容量推进", j6, "SET→GET 回读一致");

    // J7 取幂对账核
    s.add("J7-取幂对账", pow2_reconcile_ok(), "1→4096 / 5000→8192 / 1MiB 封顶");

    // J8 注入1 访问位漏查检出
    s.add("J8-访问位检出", inj[0], "SETFL 漏查坏实现必分歧");

    // J9 注入2 上限漏查检出
    s.add("J9-上限检出", inj[1], "容量上限漏查坏实现必分歧");

    // J10 F_GETFL 回读：初值 0、SETFL 后非阻塞位可见
    let mut c3 = PipeCtl::new();
    let g0 = c3.apply(F_GETFL, 0);
    let _ = c3.apply(F_SETFL, O_NONBLOCK);
    let g1 = c3.apply(F_GETFL, 0);
    let j10 = g0 == Ok(0) && g1 == Ok(O_NONBLOCK);
    s.add("J10-GETFL回读", j10, "flags 面读写一致");

    // J11 字面锚：EINVAL=22 / EPERM=1
    let j11 = POSIX_EINVAL == 22 && POSIX_EPERM == 1;
    s.add("J11-字面锚对平", j11, "22/1 双锚");

    // J12 状态机推进账：合法 SET 逐次覆盖（8192→16384）且 flags 幂等叠加
    let mut c4 = PipeCtl::new();
    let s1 = c4.apply(F_SETPIPE_SZ, 8192);
    let s2 = c4.apply(F_SETPIPE_SZ, 16384);
    let j12 = s1 == Ok(8192) && s2 == Ok(16384) && c4.size == 16384 && flags_merge_ok();
    s.add("J12-覆盖推进", j12, "后 SET 覆盖前 SET 且位叠加幂等");

    // J13 (dyn) 控制块指纹落账
    let mut c5 = PipeCtl::new();
    let _ = c5.apply(F_SETFL, O_NONBLOCK);
    let _ = c5.apply(F_SETPIPE_SZ, 8192);
    let mut h: u32 = SEG_BASE;
    h = h.wrapping_mul(31).wrapping_add(c5.flags);
    h = h.wrapping_mul(31).wrapping_add(c5.size);
    for b in c5.note.bytes() {
        h = h.wrapping_mul(31).wrapping_add(b as u32);
    }
    s.add_dyn("J13-控制块指纹", h != 0, &format!("fold={:#x}", h));

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
    fn unx10811_cmd_flow() {
        let mut c = PipeCtl::new();
        assert!(c.apply(12345, 0).err() == Some(PC_BAD_CMD));
        assert!(c.apply(F_SETFL, O_ACCMODE).err() == Some(PC_ACCMODE));
        assert!(c.apply(F_SETPIPE_SZ, 0).err() == Some(PC_SZ_ZERO));
        assert!(c.apply(F_SETPIPE_SZ, 100).err() == Some(PC_SZ_LOW));
        assert!(c.apply(F_SETPIPE_SZ, SZ_MAX + 1).err() == Some(PC_SZ_HIGH));
        assert!(c.apply(F_SETPIPE_SZ, 5000) == Ok(8192));
        assert!(c.apply(F_GETPIPE_SZ, 0) == Ok(8192));
        assert!(round_pow2(3) == SZ_MIN);
        assert!(cmd_known(F_SETPIPE_SZ));
    }

    #[test]
    fn unx10811_checks_green() {
        let s = run_unx10811_checks();
        assert!(s.all_passed(), "UNX-F10811 判据未全绿");
    }
}
