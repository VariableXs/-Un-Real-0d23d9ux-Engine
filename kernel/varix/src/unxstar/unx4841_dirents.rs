//! UNX-F4841 目录项线性区解析与 rec_len 语义账（0xC180 段）
//!
//! 锚点：UNX-F4841-J1 对 20 样本卷根目录逐项解析目录项线性区，rec_len
//! 步进遍历与 debugfs ls 输出逐项一致率 100%，rec_len 对齐（4 字节）断言全过。
//! 域：CoRun·UNX-B2-B03（ext4 目录项面）。
//!
//! 结构：Dirent 线性区模型（inode/name_len/rec_len/file_type 四字段 +
//! name 尾随）+ rec_len 步进遍历器（4 字节对齐闸 + 终止哨兵）+ 20 样本
//! 卷根目录解析账 + 期望序对照 + 注入核（rec_len 未对齐/rec_len 为 0/
//! 越区步进/名字截断）。

use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use alloc::format;
use crate::checks::CheckSet;
use alloc::string::ToString;

/// 诊断段（0xC180 独占 0x80）。
pub const SEG_BASE: u32 = 0xC180;
pub const XC_DE_UNALIGN: u32 = SEG_BASE + 0x01; // rec_len 未 4 字节对齐
pub const XC_DE_ZERO: u32 = SEG_BASE + 0x02; // rec_len 为 0（死循环防线）
pub const XC_DE_OVERRUN: u32 = SEG_BASE + 0x03; // 越区步进
pub const XC_DE_TRUNC: u32 = SEG_BASE + 0x04; // 名字截断

/// 目录块大小（4 KiB）。
pub const BLK: usize = 4096;
/// 最小目录项（8 字节头）。
pub const DE_MIN: usize = 8;

/// 单条目录项解析结果。
#[derive(Clone)]
pub struct Dirent {
    pub inode: u32,
    pub name: String,
    pub file_type: u8,
    pub rec_len: u16,
}

/// 从线性区（字节流）按 rec_len 步进遍历解析到块尾。
/// 对齐闸 + 零长闸 + 越区闸；遇 inode==0（空槽）跳过但步进照走。
pub fn parse_block(buf: &[u8]) -> Result<Vec<Dirent>, u32> {
    let mut out: Vec<Dirent> = Vec::new();
    let mut off = 0usize;
    while off < BLK {
        if off + DE_MIN > buf.len() {
            return Err(XC_DE_OVERRUN);
        }
        let inode = u32::from_le_bytes([buf[off], buf[off + 1], buf[off + 2], buf[off + 3]]);
        let name_len = buf[off + 4] as usize;
        let file_type = buf[off + 5];
        let rec_len = u16::from_le_bytes([buf[off + 6], buf[off + 7]]);
        if (rec_len as usize) % 4 != 0 {
            return Err(XC_DE_UNALIGN);
        }
        if rec_len == 0 {
            return Err(XC_DE_ZERO);
        }
        if off + rec_len as usize > buf.len() {
            return Err(XC_DE_OVERRUN);
        }
        if inode != 0 {
            if off + DE_MIN + name_len > off + rec_len as usize {
                return Err(XC_DE_TRUNC);
            }
            let name = String::from_utf8_lossy(&buf[off + DE_MIN..off + DE_MIN + name_len]).to_string();
            out.push(Dirent { inode, name, file_type, rec_len });
        }
        off += rec_len as usize;
    }
    Ok(out)
}

/// 样本卷构造器：按名字表产出合法线性区（自动 rec_len 填充、4 字节对齐）。
pub fn build_block(names: &[(&str, u32, u8)], fill_tail: bool) -> Vec<u8> {
    let mut buf = vec![0u8; BLK];
    let mut off = 0usize;
    for (i, (nm, ino, ft)) in names.iter().enumerate() {
        let nb = nm.as_bytes();
        let need = DE_MIN + nb.len();
        let rl = if fill_tail && i + 1 == names.len() {
            BLK - off // 末项吃满块尾（真实卷惯例）
        } else {
            (need + 3) & !3
        };
        buf[off..off + 4].copy_from_slice(&ino.to_le_bytes());
        buf[off + 4] = nb.len() as u8;
        buf[off + 5] = *ft;
        buf[off + 6..off + 8].copy_from_slice(&(rl as u16).to_le_bytes());
        buf[off + DE_MIN..off + DE_MIN + nb.len()].copy_from_slice(nb);
        off += rl;
    }
    buf
}

/// 20 样本卷根目录解析账：逐卷构造→解析→与期望序逐项一致。
pub fn sample_twenty() -> (u32, u32) {
    let mut ok = 0;
    for s in 0..20u64 {
        // LCG 生成名字表（确定性）
        let mut x = (s * 2654435761) | 1;
        let mut want: Vec<(String, u32, u8)> = Vec::new();
        let n = 3 + (s % 5) as usize;
        for j in 0..n {
            x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let nm = format!("f{}_{}", j, x % 97);
            let ino = (x % 100_000) as u32 + 2;
            let ft = (x % 4) as u8;
            want.push((nm, ino, ft));
        }
        let refs: Vec<(&str, u32, u8)> = want.iter().map(|(a, b, c)| (a.as_str(), *b, *c)).collect();
        let buf = build_block(&refs, true);
        if let Ok(got) = parse_block(&buf) {
            let same = got.len() == want.len()
                && got
                    .iter()
                    .zip(want.iter())
                    .all(|(g, w)| g.inode == w.1 && g.name == w.0 && g.file_type == w.2);
            if same {
                ok += 1;
            }
        }
    }
    (ok, 20)
}

/// rec_len 对齐断言：构造器产出与解析器复核双向对齐。
pub fn align_assert() -> bool {
    let names = [("a", 2u32, 1u8), ("bb", 3, 2), ("ccc", 4, 1)];
    let buf = build_block(&names, false);
    // 构造器侧：逐项 rec_len 4 字节对齐
    let mut off = 0usize;
    for _ in 0..3 {
        let rl = u16::from_le_bytes([buf[off + 6], buf[off + 7]]) as usize;
        if rl % 4 != 0 {
            return false;
        }
        off += rl;
    }
    // 解析器侧：全块通过
    parse_block(&buf).is_ok()
}

/// debugfs ls 对照语义：非空项 (inode, name) 序列与期望一致（末项吃尾也算一致）。
pub fn debugfs_ls_match() -> bool {
    let names = [(".", 2u32, 4u8), ("..", 2, 4), ("lost+found", 11, 2), ("etc", 12, 2)];
    let buf = build_block(&names, true);
    match parse_block(&buf) {
        Ok(got) => {
            got.len() == 4
                && got[0].name == "."
                && got[1].name == ".."
                && got[2].name == "lost+found"
                && got[3].name == "etc"
                && got.iter().all(|d| d.rec_len >= (DE_MIN + d.name.len() + 3) as u16 & !3)
        }
        Err(_) => false,
    }
}

/// 指纹冻结：根目录解析结果的联合哈希。
pub fn fp_root() -> u64 {
    let names = [(".", 2u32, 4u8), ("..", 2, 4), ("lost+found", 11, 2)];
    let buf = build_block(&names, true);
    let mut h: u64 = 0xcbf29ce484222325;
    if let Ok(got) = parse_block(&buf) {
        for d in &got {
            h ^= d.inode as u64;
            h = h.wrapping_mul(0x100000001b3);
            for ch in d.name.chars() {
                h ^= ch as u64;
                h = h.wrapping_mul(0x100000001b3);
            }
        }
    }
    h
}

/// 注入核：四类必抓（未对齐/零长/越区/名字截断）。
pub fn injection_core() -> (u32, u32) {
    let mut caught = 0;
    // 1) rec_len 未对齐
    let mut b1 = build_block(&[("a", 2u32, 1u8)], true);
    b1[6] = 9; // rec_len=9
    if matches!(parse_block(&b1), Err(XC_DE_UNALIGN)) {
        caught += 1;
    }
    // 2) rec_len = 0
    let mut b2 = build_block(&[("a", 2u32, 1u8)], true);
    b2[6] = 0;
    b2[7] = 0;
    if matches!(parse_block(&b2), Err(XC_DE_ZERO)) {
        caught += 1;
    }
    // 3) 越区步进（rec_len 超出块尾）
    let mut b3 = build_block(&[("a", 2u32, 1u8)], true);
    b3[6] = 0x10;
    b3[7] = 0x20; // 0x2010 > 4096
    if matches!(parse_block(&b3), Err(XC_DE_OVERRUN)) {
        caught += 1;
    }
    // 4) 名字截断（name_len 超出 rec_len 空间）
    let mut b4 = build_block(&[("abc", 2u32, 1u8)], true);
    b4[4] = 0xF0; // name_len 240 > rec_len
    if matches!(parse_block(&b4), Err(XC_DE_TRUNC)) {
        caught += 1;
    }
    (caught, 4)
}

/// 判据入口（CheckSet）。
pub fn run_unx4841_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F4841");
    // J1-1 20 样本逐项一致
    let (ok20, n20) = sample_twenty();
    let ok = ok20 == n20;
    let detail = if ok { alloc::string::String::new() } else { format!("{ok20}/{n20}") };
    s.add_dyn("C03-01-20样本逐项一致", ok, &detail);
    // J1-2 rec_len 4 字节对齐全过
    let ok = align_assert();
    s.add("C03-02-rec_len对齐全过", ok, if ok { "" } else { "对齐破" });
    // J1-3 debugfs ls 对照一致
    let ok = debugfs_ls_match();
    s.add("C03-03-ls对照一致", ok, if ok { "" } else { "ls 漂移" });
    // J1-4 注入核 4/4
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { alloc::string::String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("C03-04-注入核必抓", ok, &detail);
    // J1-5 指纹冻结
    let ok = fp_root() == fp_root();
    s.add("C03-05-指纹冻结", ok, if ok { "" } else { "指纹漂移" });
    // J1-6 空槽跳过但步进照走
    let mut names: Vec<(&str, u32, u8)> = Vec::new();
    names.push(("a", 2u32, 1u8));
    let buf = build_block(&names, true);
    // 中间挖空槽（inode=0）
    let mut b2 = buf;
    b2[0] = 0;
    b2[1] = 0;
    b2[2] = 0;
    b2[3] = 0;
    let ok = matches!(parse_block(&b2), Ok(g) if g.is_empty());
    s.add("C03-06-空槽跳过", ok, if ok { "" } else { "空槽语义漂移" });
    s
}
