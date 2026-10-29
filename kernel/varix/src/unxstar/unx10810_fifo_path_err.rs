//! UNX-F10810 FIFO 路径解析错误面（ENOENT/ELOOP/ENAMETOOLONG）（0x1bdc0 段）
//!
//! 锚点：UNX-F10810-J1 路径解析三错误对平——分量缺失必 ENOENT、符号链
//! 追踪深度超 SYMLOOP_MAX 必 ELOOP、分量超 NAME_MAX 或全径超 PATH_MAX
//! 必 ENAMETOOLONG，中间分量非目录必 ENOTDIR；注入坏实现（不追踪符号链/
//! 深度阈值失守）必与权威分歧。
//! 域：CoRun·UNX-C3（错误矩阵面谱系）。
//!
//! 结构：路径分量模型（目录/FIFO/符号链/缺失四态，符号链带目标子径）+
//! 解析器（迭代步进：长度核→存在核→类型核→深度核）+ 深度计数追踪 +
//! 注入核（两类坏实现必分歧）+ 正反双样本矩阵。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use alloc::vec;
use crate::checks::CheckSet;

/// POSIX 锚：ENOENT=2、ELOOP=40、ENAMETOOLONG=36、ENOTDIR=20。
pub const POSIX_ENOENT: u32 = 2;
pub const POSIX_ELOOP: u32 = 40;
pub const POSIX_ENAMETOOLONG: u32 = 36;

/// 诊断段（0x1bdc0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1bdc0;
pub const FP_NAME_LONG: u32 = SEG_BASE + 0x01; // 分量超 NAME_MAX
pub const FP_PATH_LONG: u32 = SEG_BASE + 0x02; // 全径超 PATH_MAX
pub const FP_LOOP: u32 = SEG_BASE + 0x03; // 符号链深度超限
pub const FP_MISSING: u32 = SEG_BASE + 0x04; // 分量缺失
pub const FP_NOTDIR: u32 = SEG_BASE + 0x05; // 中间分量非目录
pub const FP_TERM_SKEW: u32 = SEG_BASE + 0x06; // 终端非 FIFO 误放行

/// NAME_MAX / PATH_MAX / SYMLOOP_MAX 锚。
pub const NAME_MAX: usize = 255;
pub const PATH_MAX: usize = 4096;
pub const SYMLOOP_MAX: usize = 8;

/// 分量四态。
#[derive(Clone)]
pub enum Comp {
    Dir(String),
    Fifo(String),
    Missing(String),
    /// 符号链：名 + 目标子径。
    Symlink(String, Vec<Comp>),
}

impl Comp {
    /// 分量名。
    pub fn name(&self) -> &str {
        match self {
            Comp::Dir(n) => n,
            Comp::Fifo(n) => n,
            Comp::Missing(n) => n,
            Comp::Symlink(n, _) => n,
        }
    }
}

/// 路径全径长度（分量名长 + 分隔符 1 字节/分量）。
pub fn path_len(path: &[Comp]) -> usize {
    let mut n = 0usize;
    let mut i = 0usize;
    while i < path.len() {
        n += path[i].name().len() + 1;
        i += 1;
    }
    if n > 0 {
        n - 1
    } else {
        0
    }
}

/// 解析器：迭代步进（深度参数追踪符号链）。
pub fn walk(path: &[Comp], depth: usize) -> Result<(), u32> {
    // 长度核 1：全径超 PATH_MAX
    if path_len(path) > PATH_MAX {
        return Err(FP_PATH_LONG);
    }
    // 长度核 2：单分量超 NAME_MAX
    let mut i = 0usize;
    while i < path.len() {
        if path[i].name().len() > NAME_MAX {
            return Err(FP_NAME_LONG);
        }
        i += 1;
    }
    // 步进核：逐分量存在性/类型
    let mut i = 0usize;
    while i < path.len() {
        let last = i + 1 == path.len();
        match &path[i] {
            Comp::Missing(_) => return Err(FP_MISSING),
            Comp::Dir(_) => {
                if last {
                    return Err(FP_TERM_SKEW);
                }
            }
            Comp::Fifo(_) => {
                if !last {
                    return Err(FP_NOTDIR);
                }
                return Ok(());
            }
            Comp::Symlink(_, target) => {
                if depth + 1 > SYMLOOP_MAX {
                    return Err(FP_LOOP);
                }
                // 追踪：目标子径 + 余下后缀拼接
                let mut rest: Vec<Comp> = Vec::new();
                let mut k = 0usize;
                while k < target.len() {
                    rest.push(target[k].clone());
                    k += 1;
                }
                let mut j = i + 1;
                while j < path.len() {
                    rest.push(path[j].clone());
                    j += 1;
                }
                return walk(&rest, depth + 1);
            }
        }
        i += 1;
    }
    Ok(())
}

/// 场景输入：路径 + 初始深度（判定器按字段计算）。
#[derive(Clone)]
pub struct Scenario {
    pub path: Vec<Comp>,
    pub depth: usize,
}

/// 场景判定器。
pub fn judge(sc: &Scenario) -> Result<(), u32> {
    walk(&sc.path, sc.depth)
}

/// 正反双样本矩阵容量（先定后用）。
pub const MATRIX_CAP: usize = 16;

/// 正反双样本矩阵。
pub fn sample_matrix() -> Vec<(Scenario, Option<u32>)> {
    let mut m: Vec<(Scenario, Option<u32>)> = Vec::new();
    // 正：目录+FIFO 两级直达
    m.push((
        Scenario {
            path: vec![Comp::Dir("tmp".to_string()), Comp::Fifo("pipe0".to_string())],
            depth: 0,
        },
        None,
    ));
    // 正：单级 FIFO
    m.push((Scenario { path: vec![Comp::Fifo("p".to_string())], depth: 0 }, None));
    // 正：一层符号链追踪至 FIFO
    m.push((
        Scenario {
            path: vec![Comp::Symlink(
                "lnk".to_string(),
                vec![Comp::Fifo("target".to_string())],
            )],
            depth: 0,
        },
        None,
    ));
    // 反：中间缺失
    m.push((
        Scenario {
            path: vec![Comp::Dir("a".to_string()), Comp::Missing("nope".to_string()), Comp::Fifo("p".to_string())],
            depth: 0,
        },
        Some(FP_MISSING),
    ));
    // 反：终端缺失
    m.push((Scenario { path: vec![Comp::Missing("gone".to_string())], depth: 0 }, Some(FP_MISSING)));
    // 反：分量超 NAME_MAX
    let long = "x".repeat(NAME_MAX + 1);
    m.push((
        Scenario { path: vec![Comp::Fifo(long)], depth: 0 },
        Some(FP_NAME_LONG),
    ));
    // 反：全径超 PATH_MAX
    let seg = "y".repeat(1024);
    m.push((
        Scenario {
            path: vec![Comp::Dir(seg.clone()), Comp::Dir(seg.clone()), Comp::Dir(seg.clone()), Comp::Dir(seg.clone()), Comp::Fifo(seg)],
            depth: 0,
        },
        Some(FP_PATH_LONG),
    ));
    // 反：中间分量非目录（FIFO 在中间）
    m.push((
        Scenario {
            path: vec![Comp::Fifo("mid".to_string()), Comp::Fifo("leaf".to_string())],
            depth: 0,
        },
        Some(FP_NOTDIR),
    ));
    // 反：符号链深链超 SYMLOOP_MAX
    let deep = chain_of(10);
    m.push((Scenario { path: deep, depth: 0 }, Some(FP_LOOP)));
    // 边界：恰好 SYMLOOP_MAX 层可解（尾接 FIFO）
    let edge = chain_of(SYMLOOP_MAX);
    m.push((Scenario { path: edge, depth: 0 }, None));
    m
}

/// 构造 n 层符号链链（最内层接 FIFO）。
pub fn chain_of(n: usize) -> Vec<Comp> {
    let mut cur: Vec<Comp> = vec![Comp::Fifo("core".to_string())];
    let mut k = 0usize;
    while k < n {
        let name = format!("s{}", k);
        cur = vec![Comp::Symlink(name, cur)];
        k += 1;
    }
    cur
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

/// 坏实现 A：不追踪符号链（链面分量按终端核）。
pub fn faulty_no_follow(path: &[Comp]) -> Result<(), u32> {
    let mut i = 0usize;
    while i < path.len() {
        match &path[i] {
            Comp::Missing(_) => return Err(FP_MISSING),
            Comp::Symlink(_, _) => return Err(FP_TERM_SKEW),
            Comp::Fifo(_) => {
                if i + 1 == path.len() {
                    return Ok(());
                }
                return Err(FP_NOTDIR);
            }
            Comp::Dir(_) => {}
        }
        i += 1;
    }
    Err(FP_MISSING)
}

/// 坏实现 B：深度阈值失守（SYMLOOP_MAX 误用 2 倍）。
pub fn faulty_depth_x2(path: &[Comp], depth: usize) -> Result<(), u32> {
    let mut i = 0usize;
    while i < path.len() {
        if path[i].name().len() > NAME_MAX {
            return Err(FP_NAME_LONG);
        }
        if let Comp::Symlink(_, target) = &path[i] {
            if depth + 1 > SYMLOOP_MAX * 2 {
                return Err(FP_LOOP);
            }
            let mut rest: Vec<Comp> = Vec::new();
            let mut k = 0usize;
            while k < target.len() {
                rest.push(target[k].clone());
                k += 1;
            }
            let mut j = i + 1;
            while j < path.len() {
                rest.push(path[j].clone());
                j += 1;
            }
            return faulty_depth_x2(&rest, depth + 1);
        }
        i += 1;
    }
    Ok(())
}

/// 注入核：两类坏实现必与权威分歧（检出）。
pub fn inject_two() -> [bool; 2] {
    let mut got = [false; 2];
    // 1) 不追踪符号链：一层链，权威 Ok、坏实现拒 → 检出
    let link = vec![Comp::Symlink("lnk".to_string(), vec![Comp::Fifo("t".to_string())])];
    got[0] = walk(&link, 0).is_ok() && faulty_no_follow(&link).is_err();
    // 2) 深度阈值失守：SYMLOOP_MAX+1 层链，权威 ELOOP、坏实现放行 → 检出
    let deep = chain_of(SYMLOOP_MAX + 1);
    got[1] = walk(&deep, 0).err() == Some(FP_LOOP)
        && faulty_depth_x2(&deep, 0).is_ok();
    got
}

/// 路径解析判据（对应卡验收 J 判据）。
pub fn run_unx10810_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10810");
    let inj = inject_two();
    let (hits, total, cap_ok) = verify_matrix();

    // J1 矩阵对平
    s.add("J1-矩阵对平", hits == total && cap_ok, "十线解析样本全命中");

    // J2 正路径：两级目录+FIFO 直达
    let ok_path = vec![Comp::Dir("tmp".to_string()), Comp::Fifo("p0".to_string())];
    let j2 = walk(&ok_path, 0).is_ok();
    s.add("J2-直达放行", j2, "目录→FIFO 两级直达");

    // J3 反路径：中间缺失必 ENOENT
    let miss = vec![Comp::Missing("gone".to_string())];
    let j3 = walk(&miss, 0).err() == Some(FP_MISSING);
    s.add("J3-缺失必拒", j3, "缺失分量 ENOENT");

    // J4 边界：NAME_MAX 恰好放行、+1 必拒
    let ok_name = "n".repeat(NAME_MAX);
    let bad_name = "n".repeat(NAME_MAX + 1);
    let j4 = walk(&vec![Comp::Fifo(ok_name)], 0).is_ok()
        && walk(&vec![Comp::Fifo(bad_name)], 0).err() == Some(FP_NAME_LONG);
    s.add("J4-NAME_MAX两侧", j4, "255/256 分界清晰");

    // J5 边界：SYMLOOP_MAX 恰好可解、+1 必 ELOOP
    let j5 = walk(&chain_of(SYMLOOP_MAX), 0).is_ok()
        && walk(&chain_of(SYMLOOP_MAX + 1), 0).err() == Some(FP_LOOP);
    s.add("J5-SYMLOOP两侧", j5, "8/9 层分界清晰");

    // J6 中间非目录必 ENOTDIR
    let notdir = vec![Comp::Fifo("mid".to_string()), Comp::Fifo("leaf".to_string())];
    let j6 = walk(&notdir, 0).err() == Some(FP_NOTDIR);
    s.add("J6-ENOTDIR面", j6, "FIFO 作中间分量拒");

    // J7 全径长度核：5×1024 段超 PATH_MAX 必拒
    let seg = "z".repeat(1024);
    let longp = vec![
        Comp::Dir(seg.clone()),
        Comp::Dir(seg.clone()),
        Comp::Dir(seg.clone()),
        Comp::Dir(seg.clone()),
        Comp::Fifo(seg),
    ];
    let j7 = walk(&longp, 0).err() == Some(FP_PATH_LONG);
    s.add("J7-PATH_MAX核", j7, "全径 5120>4096 拒");

    // J8 注入1 不追踪符号链检出
    s.add("J8-不追踪检出", inj[0], "符号链按终端核坏实现必分歧");

    // J9 注入2 深度阈值失守检出
    s.add("J9-阈值失守检出", inj[1], "SYMLOOP_MAX 误 2 倍坏实现必分歧");

    // J10 字面锚：2/40/36 三锚
    let j10 = POSIX_ENOENT == 2 && POSIX_ELOOP == 40 && POSIX_ENAMETOOLONG == 36;
    s.add("J10-字面锚对平", j10, "2/40/36 三锚");

    // J11 全径长度函数：两级径 = 名长和 + 1 分隔
    let two = vec![Comp::Dir("ab".to_string()), Comp::Fifo("cde".to_string())];
    let j11 = path_len(&two) == 6 && path_len(&vec![Comp::Fifo("x".to_string())]) == 1;
    s.add("J11-径长函数", j11, "名长和加分隔符");

    // J12 终端核：终端为目录误放行必拒
    let dir_term = vec![Comp::Dir("only".to_string())];
    let j12 = walk(&dir_term, 0).err() == Some(FP_TERM_SKEW);
    s.add("J12-终端非FIFO", j12, "终端目录拒");

    // J13 (dyn) 链深指纹落账
    let chain8 = chain_of(SYMLOOP_MAX);
    let mut h: u32 = SEG_BASE;
    h = h.wrapping_mul(31).wrapping_add(chain8.len() as u32);
    h = h.wrapping_mul(31).wrapping_add(path_len(&chain8) as u32);
    s.add_dyn(
        "J13-链深指纹",
        h != 0 && walk(&chain8, 0).is_ok(),
        &format!("fold={:#x} rows={}", h, chain8.len()),
    );

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
    fn unx10810_walk_flow() {
        let two = vec![Comp::Dir("tmp".to_string()), Comp::Fifo("p".to_string())];
        assert!(walk(&two, 0).is_ok());
        assert!(walk(&[Comp::Missing("x".to_string())], 0).err() == Some(FP_MISSING));
        assert!(walk(&chain_of(8), 0).is_ok());
        assert!(walk(&chain_of(9), 0).err() == Some(FP_LOOP));
        let link = vec![Comp::Symlink("l".to_string(), vec![Comp::Fifo("t".to_string())])];
        assert!(walk(&link, 0).is_ok());
        assert_eq!(path_len(&two), 6);
    }

    #[test]
    fn unx10810_checks_green() {
        let s = run_unx10810_checks();
        assert!(s.all_passed(), "UNX-F10810 判据未全绿");
    }
}
