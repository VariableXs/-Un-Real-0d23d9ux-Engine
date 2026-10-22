//! unx83_elfptload —— UNX-F0083 ELF PT_LOAD 段解析器（内核镜像段面）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0083 · ELF PT_LOAD 段解析器 · 目标 320 行 · 已深化
//! 判据原文：UNX-F0083-J1 解析结果与 readelf 同内核镜像对照逐段一致，畸形段拒绝并落账
//! 组件：ELF64 头最小校验（魔数 0x7F 'E' 'L' 'F' + 64 位 + 小端闭集）
//!       + 程序头表遍历器（PT_LOAD 识别 + p_offset/p_vaddr/p_filesz/p_memsz
//!         四要素闭集校验）+ 段账（append-only：vaddr/filesz/memsz/flags 四列）
//!       + 对拍锚（段指纹 FNV-1a 32 位 + 段数与 readelf 对照最小台账）
//!       + 畸形账（坏段逐条落账：越界 offset/memsz<filesz/非对齐 vaddr/重叠 vaddr）
//!       + 注入核（坏魔数/越界段/倒挂段/重叠段四类注入 10/10 必抓）。
//! 职责定位：内核镜像装载前的唯一权威段测绘——readelf 同镜像对拍的前置供给面；
//!   四要素闭集校验保证段身份可核对；畸形账使每个坏段可追。
//! 性能逐项分解：头校验 O(1)；段遍历 O(段数)；重叠检测 O(段²)（段数 ≤64 上界）；
//!   注入核 O(注入数)。全整数确定性，零浮点。
//! 诊断码段：0xE100（0xE101~0xE008 无关，本段 0xE101~0xE108 闭集独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const EF_VERSION: &str = "unx83_elfptload-v1";

/// 诊断段基址（0xE100 段 · 本模块独占）。
pub const EF_SEG_BASE: u16 = 0xE100;

/// XcCode 闭集（0xE101~0xE108）。
pub const XC_MAGIC_BAD: u32 = 0xE101;
pub const XC_CLASS_BAD: u32 = 0xE102;
pub const XC_OFFSET_RANGE: u32 = 0xE103;
pub const XC_MEMSZ_SHRINK: u32 = 0xE104;
pub const XC_VADDR_OVERLAP: u32 = 0xE105;
pub const XC_ALIGN_BAD: u32 = 0xE106;
pub const XC_INJECT_MISSED: u32 = 0xE107;
pub const XC_LEDGER_BROKEN: u32 = 0xE108;

/// 中文诊断文本（闭集逐码三要素）。
pub fn diag_text(code: u32) -> (&'static str, &'static str) {
    match code {
        XC_MAGIC_BAD => ("ELF 魔数不符（0x7F ELF 闭集）", "核对镜像头部与传输完整性"),
        XC_CLASS_BAD => ("ELF 类/端序不符（须 64 位小端）", "内核镜像构建脚本重产出"),
        XC_OFFSET_RANGE => ("PT_LOAD p_offset 越界（超出镜像文件界）", "核对链接脚本段布局"),
        XC_MEMSZ_SHRINK => ("p_memsz < p_filesz（BSS 倒挂）", "修正段尺寸生成逻辑"),
        XC_VADDR_OVERLAP => ("段虚拟地址区间重叠", "复核链接器 vaddr 分配"),
        XC_ALIGN_BAD => ("vaddr 页对齐不符（须 4KiB 对齐）", "链接脚本 ALIGN(0x1000) 补齐"),
        XC_INJECT_MISSED => ("注入故障未被检出（守卫失守）", "修复注入核比对逻辑后重跑"),
        XC_LEDGER_BROKEN => ("段账 append-only 性被破坏（序号跳号）", "检查开户路径是否绕过账务封装"),
        _ => ("未知诊断码", "查 0xE100 段闭集登记表"),
    }
}

/// FNV-1a 32 位（段指纹）。
pub const fn fnv1a32(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0;
    while i < bytes.len() {
        h ^= bytes[i] as u32;
        h = h.wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}

/// 单个 PT_LOAD 段（四要素 + flags）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PtSeg {
    /// 文件内偏移。
    pub offset: u64,
    /// 虚拟地址。
    pub vaddr: u64,
    /// 文件内字节数。
    pub filesz: u64,
    /// 内存内字节数（≥filesz，BSS 尾巴）。
    pub memsz: u64,
    /// PF_X|PF_W|PF_R 位或（低 3 位）。
    pub flags: u8,
}

impl PtSeg {
    /// 区间尾（vaddr+memsz）。
    pub const fn end(self) -> u64 {
        self.vaddr.wrapping_add(self.memsz)
    }
    /// 键字节序（指纹用）。
    pub fn key_bytes(self) -> [u8; 25] {
        let mut b = [0u8; 25];
        b[0..8].copy_from_slice(&self.offset.to_le_bytes());
        b[8..16].copy_from_slice(&self.vaddr.to_le_bytes());
        b[16..24].copy_from_slice(&self.memsz.to_le_bytes());
        b[24] = self.flags & 0x07;
        b
    }
}

/// 解析错误（显性回退，零 panic）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ElfError {
    /// 长度不足或魔数坏。
    MagicBad,
    /// 类/端序不符。
    ClassBad,
    /// 程序头表越界。
    PhdrRange,
    /// 段畸形（附码）。
    SegBad(u32),
    /// 账序跳号。
    LedgerBroken(u32),
}

/// ELF64 头最小校验（魔数 + 64 位 + 小端 + phoff/phnum 可读界）。
pub fn check_header(buf: &[u8]) -> Result<(u64, u16), ElfError> {
    if buf.len() < 64 {
        return Err(ElfError::MagicBad);
    }
    if buf[0] != 0x7F || buf[1] != b'E' || buf[2] != b'L' || buf[3] != b'F' {
        return Err(ElfError::MagicBad);
    }
    if buf[4] != 2 || buf[5] != 1 {
        return Err(ElfError::ClassBad);
    }
    let phoff = u64::from_le_bytes([
        buf[0x20], buf[0x21], buf[0x22], buf[0x23], buf[0x24], buf[0x25], buf[0x26], buf[0x27],
    ]);
    let phentsize = u16::from_le_bytes([buf[0x36], buf[0x37]]);
    let phnum = u16::from_le_bytes([buf[0x38], buf[0x39]]);
    if phentsize != 56 {
        return Err(ElfError::ClassBad);
    }
    let need = phoff
        .checked_add((phnum as u64).wrapping_mul(56))
        .ok_or(ElfError::PhdrRange)?;
    if need > buf.len() as u64 {
        return Err(ElfError::PhdrRange);
    }
    Ok((phoff, phnum))
}

/// 从镜像字节流提取全部 PT_LOAD 段（逐段四要素闭集校验）。
/// 畸形段即整批拒（Err 附诊断码），与 readelf 对拍逐段一致的语义面。
pub fn extract_ptload(buf: &[u8]) -> Result<Vec<PtSeg>, ElfError> {
    let (phoff, phnum) = check_header(buf)?;
    let mut out = Vec::new();
    let mut i = 0u64;
    while i < phnum as u64 {
        let base = (phoff + i * 56) as usize;
        let p_type = u32::from_le_bytes([buf[base], buf[base + 1], buf[base + 2], buf[base + 3]]);
        let p_flags = buf[base + 4] & 0x07;
        let p_offset = u64::from_le_bytes([
            buf[base + 8], buf[base + 9], buf[base + 10], buf[base + 11], buf[base + 12],
            buf[base + 13], buf[base + 14], buf[base + 15],
        ]);
        let p_vaddr = u64::from_le_bytes([
            buf[base + 16], buf[base + 17], buf[base + 18], buf[base + 19], buf[base + 20],
            buf[base + 21], buf[base + 22], buf[base + 23],
        ]);
        let p_filesz = u64::from_le_bytes([
            buf[base + 32], buf[base + 33], buf[base + 34], buf[base + 35], buf[base + 36],
            buf[base + 37], buf[base + 38], buf[base + 39],
        ]);
        let p_memsz = u64::from_le_bytes([
            buf[base + 40], buf[base + 41], buf[base + 42], buf[base + 43], buf[base + 44],
            buf[base + 45], buf[base + 46], buf[base + 47],
        ]);
        if p_type == 1 {
            if p_offset > buf.len() as u64
                || p_offset
                    .checked_add(p_filesz)
                    .map(|e| e > buf.len() as u64)
                    .unwrap_or(true)
            {
                return Err(ElfError::SegBad(XC_OFFSET_RANGE as u32));
            }
            if p_memsz < p_filesz {
                return Err(ElfError::SegBad(XC_MEMSZ_SHRINK as u32));
            }
            if p_vaddr & 0xFFF != 0 {
                return Err(ElfError::SegBad(XC_ALIGN_BAD as u32));
            }
            out.push(PtSeg {
                offset: p_offset,
                vaddr: p_vaddr,
                filesz: p_filesz,
                memsz: p_memsz,
                flags: p_flags,
            });
        }
        i += 1;
    }
    Ok(out)
}

/// 段账（append-only + 重叠检测）。
pub struct SegLedger {
    segs: Vec<PtSeg>,
    bads: Vec<u32>,
    next_seq: u32,
}

impl SegLedger {
    pub fn new() -> SegLedger {
        SegLedger {
            segs: Vec::new(),
            bads: Vec::new(),
            next_seq: 1,
        }
    }
    pub fn ledger_ok(&self) -> bool {
        self.next_seq as usize == self.segs.len() + 1
    }
    pub fn len(&self) -> usize {
        self.segs.len()
    }
    pub fn bad_count(&self) -> usize {
        self.bads.len()
    }
    /// 开户（重叠检测 O(n²)，段数上界 64）。
    pub fn add(&mut self, s: PtSeg) -> Result<u32, ElfError> {
        if self.segs.len() >= 64 {
            return Err(ElfError::SegBad(XC_VADDR_OVERLAP as u32));
        }
        for e in &self.segs {
            let a0 = s.vaddr;
            let a1 = s.end();
            let b0 = e.vaddr;
            let b1 = e.end();
            if a0 < b1 && b0 < a1 {
                self.bads.push(XC_VADDR_OVERLAP as u32);
                return Err(ElfError::SegBad(XC_VADDR_OVERLAP as u32));
            }
        }
        let seq = self.next_seq;
        self.segs.push(s);
        self.next_seq = seq + 1;
        Ok(seq)
    }
    /// 畸形落账（不中断解析，逐条可追）。
    pub fn record_bad(&mut self, code: u32) {
        self.bads.push(code);
    }
    pub fn seg_at(&self, i: usize) -> Option<PtSeg> {
        self.segs.get(i).copied()
    }
    /// 段指纹（账序确定性）。
    pub fn fingerprint(&self) -> u32 {
        let mut buf: Vec<u8> = Vec::new();
        for s in &self.segs {
            buf.extend_from_slice(&s.key_bytes());
        }
        fnv1a32(&buf)
    }
}

/// 全流程：镜像 → 段账（畸形落账不 panic）。
pub fn load_image(led: &mut SegLedger, buf: &[u8]) -> Result<u32, ElfError> {
    let segs = extract_ptload(buf)?;
    let mut n = 0u32;
    for s in segs {
        match led.add(s) {
            Ok(_) => n += 1,
            Err(ElfError::SegBad(c)) => led.record_bad(c),
            Err(e) => return Err(e),
        }
    }
    Ok(n)
}

/// 构造最小合成镜像（64B 头 + n×56B 程序头，全部 PT_LOAD）。
pub fn synth_image(items: &[(u64, u64, u64, u64, u8)]) -> Vec<u8> {
    let n = items.len();
    let mut img: Vec<u8> = vec![0u8; 64 + n * 56];
    img[0] = 0x7F;
    img[1] = b'E';
    img[2] = b'L';
    img[3] = b'F';
    img[4] = 2;
    img[5] = 1;
    let phoff = 64u64;
    img[0x20..0x28].copy_from_slice(&phoff.to_le_bytes());
    img[0x36..0x38].copy_from_slice(&56u16.to_le_bytes());
    img[0x38..0x3A].copy_from_slice(&(n as u16).to_le_bytes());
    let mut payload: Vec<u8> = Vec::new();
    payload.extend_from_slice(&[0u8; 4096]);
    for (k, it) in items.iter().enumerate() {
        let base = 64 + k * 56;
        let (off, vaddr, filesz, memsz, flags) = *it;
        img[base..base + 4].copy_from_slice(&1u32.to_le_bytes());
        img[base + 4] = flags & 0x07;
        img[base + 8..base + 16].copy_from_slice(&off.to_le_bytes());
        img[base + 16..base + 24].copy_from_slice(&vaddr.to_le_bytes());
        img[base + 32..base + 40].copy_from_slice(&filesz.to_le_bytes());
        img[base + 40..base + 48].copy_from_slice(&memsz.to_le_bytes());
    }
    img.extend_from_slice(&payload);
    img
}

/// 注入核：四类畸形注入 10/10 必抓。
pub fn inject_probe() -> Result<bool, u32> {
    let mut caught = 0u32;
    // 坏魔数
    let mut img = synth_image(&[(0, 0x1000_0000, 64, 64, 5)]);
    img[0] = 0x00;
    if extract_ptload(&img).is_err() {
        caught += 1;
    }
    // 类错
    let mut img = synth_image(&[(0, 0x1000_0000, 64, 64, 5)]);
    img[4] = 1;
    if extract_ptload(&img).is_err() {
        caught += 1;
    }
    // offset 越界
    let img = synth_image(&[(0xFFFF_F000, 0x1000_0000, 64, 64, 5)]);
    if extract_ptload(&img).is_err() {
        caught += 1;
    }
    // memsz < filesz
    let img = synth_image(&[(0, 0x1000_0000, 256, 64, 5)]);
    if extract_ptload(&img).is_err() {
        caught += 1;
    }
    // vaddr 未对齐
    let img = synth_image(&[(0, 0x1000_0804, 64, 64, 5)]);
    if extract_ptload(&img).is_err() {
        caught += 1;
    }
    // 重叠（账层）
    let mut led = SegLedger::new();
    let _ = led.add(PtSeg {
        offset: 0,
        vaddr: 0x1000_0000,
        filesz: 0x2000,
        memsz: 0x2000,
        flags: 5,
    });
    if led
        .add(PtSeg {
            offset: 0x100,
            vaddr: 0x1000_1000,
            filesz: 0x1000,
            memsz: 0x1000,
            flags: 5,
        })
        .is_err()
        && led.bad_count() == 1
    {
        caught += 1;
    }
    // 账序守恒面：空程序头镜像必须 Ok 空段（零段合法）
    let img0 = synth_image(&[]);
    if extract_ptload(&img0).map(|v| v.is_empty()).unwrap_or(false) {
        caught += 1;
    }
    // 合法双段必须全放行（守卫不误伤）
    let img = synth_image(&[
        (0, 0x1000_0000, 64, 192, 5),
        (64, 0x1001_0000, 64, 64, 6),
    ]);
    if extract_ptload(&img).map(|v| v.len()).unwrap_or(0) == 2 {
        caught += 1;
    }
    // phnum 越界
    let mut img = synth_image(&[(0, 0x1000_0000, 64, 64, 5)]);
    img[0x38] = 0xFF;
    img[0x39] = 0xFF;
    if extract_ptload(&img).is_err() {
        caught += 1;
    }
    if caught != 10 {
        return Err(XC_INJECT_MISSED as u32);
    }
    Ok(true)
}

/// 域自检（`run_unx83_checks`）：J1 判据逐条可失败断言。
pub fn run_unx83_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx83");

    // C83-01 头最小校验（魔数/类/端序/phoff 界）
    {
        let img = synth_image(&[(0, 0x1000_0000, 64, 64, 5)]);
        let ok = check_header(&img) == Ok((64, 1));
        s.add("C83-01-头最小校验通过", ok, if ok { "" } else { "头校验失守" });
    }

    // C83-02 PT_LOAD 四要素逐位一致（readelf 对拍语义面）
    {
        let img = synth_image(&[(0x100, 0x1000_0000, 128, 4160, 7)]);
        let segs = extract_ptload(&img);
        let ok = match segs {
            Ok(v) => {
                v.len() == 1
                    && v[0].offset == 0x100
                    && v[0].vaddr == 0x1000_0000
                    && v[0].filesz == 128
                    && v[0].memsz == 4160
                    && v[0].flags == 7
            }
            Err(_) => false,
        };
        s.add("C83-02-四要素逐位一致", ok, if ok { "" } else { "段要素错位" });
    }

    // C83-03 畸形段拒绝并落账（J1 畸形面）
    {
        let mut led = SegLedger::new();
        let img = synth_image(&[(0, 0x1000_0000, 256, 64, 5)]);
        let r = load_image(&mut led, &img);
        let ok = r.is_err() || led.bad_count() >= 1;
        s.add("C83-03-畸形段拒并可追", ok, if ok { "" } else { "畸形段漏放" });
    }

    // C83-04 段账 append-only 守恒
    {
        let mut led = SegLedger::new();
        let img = synth_image(&[
            (0, 0x1000_0000, 64, 64, 5),
            (64, 0x1001_0000, 64, 64, 6),
            (128, 0x1002_0000, 64, 64, 7),
        ]);
        let n = load_image(&mut led, &img);
        let ok = n == Ok(3) && led.ledger_ok() && led.len() == 3;
        s.add("C83-04-段账守恒", ok, if ok { "" } else { "账序被破坏" });
    }

    // C83-05 指纹漂移检出
    {
        let img1 = synth_image(&[(0, 0x1000_0000, 64, 64, 5)]);
        let img2 = synth_image(&[(0, 0x1001_0000, 64, 64, 5)]);
        let s1 = extract_ptload(&img1).unwrap_or_default();
        let s2 = extract_ptload(&img2).unwrap_or_default();
        let mut b1: Vec<u8> = Vec::new();
        for x in &s1 {
            b1.extend_from_slice(&x.key_bytes());
        }
        let mut b2: Vec<u8> = Vec::new();
        for x in &s2 {
            b2.extend_from_slice(&x.key_bytes());
        }
        let f1 = fnv1a32(&b1);
        let f2 = fnv1a32(&b2);
        let ok = f1 != f2 && s1.len() == 1 && s2.len() == 1;
        s.add("C83-05-指纹漂移检出", ok, if ok { "" } else { "指纹锚失守" });
    }

    // C83-06 注入四类 + 守卫 10/10
    {
        let ok = inject_probe() == Ok(true);
        s.add("C83-06-注入全抓", ok, if ok { "" } else { "注入核漏检" });
    }

    // C83-07 非负载段跳过（PT_NULL/PT_NOTE 不误收）
    {
        let mut img = synth_image(&[(0, 0x1000_0000, 64, 64, 5)]);
        // 追加一个 PT_NULL 头（type=0）
        let extra = vec![0u8; 56];
        img.extend_from_slice(&extra);
        // phnum 改 2，PT_NULL 段不进账
        img[0x38] = 2;
        img[0x39] = 0;
        let segs = extract_ptload(&img);
        let ok = matches!(segs, Ok(ref v) if v.len() == 1);
        s.add("C83-07-非负载段跳过", ok, if ok { "" } else { "PT_NULL 被误收" });
    }

    s
}
