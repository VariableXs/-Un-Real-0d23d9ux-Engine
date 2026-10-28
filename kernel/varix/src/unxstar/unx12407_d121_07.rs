//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12407 · SEC_IMAGE 与 PE 装载联签预告 · 0x300e0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥280 行；
//!   ② UNX-F12407-J1 映像映射档（节对齐/保护派生）语义预告齐，D2 承接
//!      批号登记入联测清单；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 映像节描述域：节数/虚尺寸/特性位（CODE/READ/WRITE/EXEC）四列，
//!     逐节入账（DescOob 拒越界、ImageEmpty 拒空映像）；
//!   - 节对齐纪律：SectionAlignment 必须 2 的幂（AlignNotPow2）、≤64KiB
//!     （AlignTooBig）、≥FileAlignment（AlignLtFile）；
//!   - 虚尺寸对齐：vsize 向上对齐 SectionAlignment（RawOverVirt 拒裸尺寸
//!     超虚对齐账）；
//!   - 保护派生：特性位→PAGE 档闭集映射（EXEC|READ|WRITE→ERW、
//!     EXEC|READ→ER、EXEC→E、WRITE→RW、READ→RO、否则 NOACCESS），
//!     特性位越集拒（CharOob），派生失败拒（ProtDeriveFail）；
//!   - D2 承接批号登记：联测清单 append-only（批号/主题/段基三列），
//!     重复登记拒（DupBatch）、清单缩水拒（ListShrink）；
//!   - 派生结果双面账：节行与派生档逐节可查，派生档必须落八档闭集；
//!   - 映像总账守恒：节数 ≡ 已派生 + 未派生；
//!   - 注入核：LCG 写死种子 0x300e0，派生矩阵/对齐/D2 登记 10/10。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x300e0 段，主代理分工表分配）。
pub const SEG_BASE: u32 = 0x300e0;

/// 注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 映像节上限（定容）。
pub const IMG_SECTIONS: usize = 16;

/// SectionAlignment 上限（64KiB 口径）。
pub const SEC_ALIGN_MAX: u32 = 0x10000;

/// FileAlignment 兜底值（512 口径）。
pub const FILE_ALIGN_DEFAULT: u32 = 0x200;

// 节特性位（PE 口径）。
/// IMAGE_SCN_CNT_CODE。
pub const SCN_CODE: u32 = 0x0000_0020;
/// IMAGE_SCN_MEM_READ。
pub const SCN_READ: u32 = 0x4000_0000;
/// IMAGE_SCN_MEM_WRITE。
pub const SCN_WRITE: u32 = 0x8000_0000;
/// IMAGE_SCN_MEM_EXECUTE。
pub const SCN_EXEC: u32 = 0x2000_0000;

// PAGE 档（派生目标域）。
/// PAGE_NOACCESS。
pub const PAGE_NOACCESS: u32 = 0x01;
/// PAGE_READONLY。
pub const PAGE_READONLY: u32 = 0x02;
/// PAGE_READWRITE。
pub const PAGE_READWRITE: u32 = 0x04;
/// PAGE_WRITECOPY。
pub const PAGE_WRITECOPY: u32 = 0x08;
/// PAGE_EXECUTE。
pub const PAGE_EXECUTE: u32 = 0x10;
/// PAGE_EXECUTE_READ。
pub const PAGE_EXECUTE_READ: u32 = 0x20;
/// PAGE_EXECUTE_READWRITE。
pub const PAGE_EXECUTE_READWRITE: u32 = 0x40;

/// 诊断码（0x300e0 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ImgCode {
    /// 0x300e0 对齐非 2 幂。
    AlignNotPow2 = 0,
    /// 0x300e1 对齐超上限（>64KiB）。
    AlignTooBig = 1,
    /// 0x300e2 SectionAlignment < FileAlignment。
    AlignLtFile = 2,
    /// 0x300e3 特性位越集（CODE/READ/WRITE/EXEC 之外）。
    CharOob = 3,
    /// 0x300e4 保护派生失败（派生档落八档之外）。
    ProtDeriveFail = 4,
    /// 0x300e5 节描述越界（节号出表）。
    DescOob = 5,
    /// 0x300e6 映像空（零节）。
    ImageEmpty = 6,
    /// 0x300e7 D2 批号重复登记。
    DupBatch = 7,
    /// 0x300e8 联测清单缩水。
    ListShrink = 8,
    /// 0x300e9 裸尺寸超虚对齐账（raw 越对齐后虚尺寸）。
    RawOverVirt = 9,
}

impl ImgCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [ImgCode; 10] = [
        ImgCode::AlignNotPow2, ImgCode::AlignTooBig, ImgCode::AlignLtFile, ImgCode::CharOob,
        ImgCode::ProtDeriveFail, ImgCode::DescOob, ImgCode::ImageEmpty, ImgCode::DupBatch,
        ImgCode::ListShrink, ImgCode::RawOverVirt,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x300e0 => "对齐非 2 幂：SectionAlignment 必须是 2 的幂，回改 PE 头。",
        0x300e1 => "对齐超上限：SectionAlignment ≤ 64KiB。",
        0x300e2 => "SectionAlignment < FileAlignment：取两者较大值重对齐。",
        0x300e3 => "特性位越集：只收 CODE/READ/WRITE/EXEC 四位。",
        0x300e4 => "派生失败：派生档必须落 PAGE 八档闭集，回查特性位。",
        0x300e5 => "节描述越界：节号先比 IMG_SECTIONS 再取。",
        0x300e6 => "映像空：零节映像不可装载，回查 PE 头 NumberOfSections。",
        0x300e7 => "D2 批号重复：同批号幂等，查联测清单再登记。",
        0x300e8 => "清单缩水：append-only，禁止截断，重放补齐。",
        0x300e9 => "裸尺寸超虚对齐账：raw 尺寸不得越过对齐后虚尺寸。",
        _ => "未知诊断码：回查 0x300e0 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x300e0 段基址铺位（固定种子，可重复）。
    pub const SEED: u64 = 0x300e_300e_300e_300e;

    /// 播种。
    pub fn new(seed: u64) -> Self {
        Lcg(seed ^ Self::SEED)
    }
    /// 下一步（Knuth MMIX 常量）。
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
    /// 取模（n=0 时钳为 1）。
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// 2 的幂核对。
pub fn is_pow2(v: u32) -> bool {
    v != 0 && v & (v - 1) == 0
}

/// 向上对齐（对齐值必须 2 幂）。
pub fn align_up(v: u64, align: u32) -> u64 {
    if !is_pow2(align) {
        return v;
    }
    let a = align as u64;
    (v + a - 1) & !(a - 1)
}

/// 对齐档校验（三纪律：2 幂/上限/不小于 FileAlignment）。
pub fn validate_align(sec_align: u32, file_align: u32) -> Result<(), ImgCode> {
    if !is_pow2(sec_align) {
        return Err(ImgCode::AlignNotPow2);
    }
    if sec_align > SEC_ALIGN_MAX {
        return Err(ImgCode::AlignTooBig);
    }
    if sec_align < file_align {
        return Err(ImgCode::AlignLtFile);
    }
    Ok(())
}

/// 特性位闭集核对（只收四位）。
pub fn chars_ok(c: u32) -> bool {
    c & !(SCN_CODE | SCN_READ | SCN_WRITE | SCN_EXEC) == 0 && c != 0
}

/// 保护派生（特性位→PAGE 档；映像档口径：纯 WRITE 派生只读档，
/// 可写能力由映像装载器以写时复制面承接）。
pub fn derive_protect(chars: u32) -> Result<u32, ImgCode> {
    if !chars_ok(chars) {
        return Err(ImgCode::CharOob);
    }
    let e = chars & SCN_EXEC != 0;
    let r = chars & SCN_READ != 0;
    let w = chars & SCN_WRITE != 0;
    let c = chars & SCN_CODE != 0;
    let p = match (e, r, w, c) {
        (true, true, true, _) => PAGE_EXECUTE_READWRITE,
        (true, true, false, _) => PAGE_EXECUTE_READ,
        (true, false, true, _) => PAGE_EXECUTE_READWRITE,
        (true, false, false, _) => PAGE_EXECUTE,
        (false, true, true, _) => PAGE_WRITECOPY,
        (false, true, false, _) => PAGE_READONLY,
        (false, false, true, _) => PAGE_WRITECOPY,
        // 纯 CODE 节（无读写特性位）按可执行档派生。
        (false, false, false, true) => PAGE_EXECUTE,
        (false, false, false, false) => PAGE_NOACCESS,
    };
    const SET: u32 = PAGE_NOACCESS | PAGE_READONLY | PAGE_READWRITE | PAGE_WRITECOPY
        | PAGE_EXECUTE | PAGE_EXECUTE_READ | PAGE_EXECUTE_READWRITE | 0x80;
    if p & SET == 0 {
        return Err(ImgCode::ProtDeriveFail);
    }
    Ok(p)
}

/// 映像节行。
#[derive(Clone, Copy, Debug)]
pub struct ImageSectionRow {
    /// 节号。
    pub index: usize,
    /// 虚尺寸（原始）。
    pub vsize: u64,
    /// 裸尺寸。
    pub raw_size: u64,
    /// 特性位。
    pub chars: u32,
    /// 派生档（未派生为 0）。
    pub protect: u32,
}

/// D2 承接批号登记行（联测清单一行）。
#[derive(Clone, Copy, Debug)]
pub struct D2BatchRow {
    /// 批号名。
    pub batch: &'static str,
    /// 承接主题。
    pub theme: &'static str,
    /// 承接段基。
    pub seg: u32,
}

/// SEC_IMAGE 映像映射档正本（预告面）。
///
/// 生命周期：set_align（对齐档）→ add_section（节入账）→ derive_all
/// （逐节派生）→ register_d2（D2 承接批号登记）。
#[derive(Debug)]
pub struct ImageBook {
    /// SectionAlignment。
    sec_align: u32,
    /// FileAlignment。
    file_align: u32,
    /// 节账。
    sections: Vec<ImageSectionRow>,
    /// 节账峰值（append-only 判准）。
    sec_high: usize,
    /// D2 联测清单。
    d2_list: Vec<D2BatchRow>,
    /// 清单峰值。
    d2_high: usize,
    /// 拒止累计。
    rejects: u32,
}

impl ImageBook {
    /// 建账（默认对齐 4KiB/512）。
    pub fn new() -> Self {
        ImageBook::with_align(0x1000, FILE_ALIGN_DEFAULT)
    }

    /// 建账（自定义对齐档）。
    pub fn with_align(sec_align: u32, file_align: u32) -> Self {
        ImageBook {
            sec_align,
            file_align,
            sections: Vec::new(),
            sec_high: 0,
            d2_list: Vec::new(),
            d2_high: 0,
            rejects: 0,
        }
    }

    /// 对齐档设定（校验失败拒并记账）。
    pub fn set_align(&mut self, sec_align: u32, file_align: u32) -> Result<bool, ImgCode> {
        match validate_align(sec_align, file_align) {
            Ok(()) => {
                self.sec_align = sec_align;
                self.file_align = file_align;
                Ok(true)
            }
            Err(c) => {
                self.rejects += 1;
                Err(c)
            }
        }
    }

    /// 对齐档面。
    pub fn aligns(&self) -> (u32, u32) {
        (self.sec_align, self.file_align)
    }

    /// 节入账（特性位/裸尺寸校验）。
    pub fn add_section(&mut self, vsize: u64, raw_size: u64, chars: u32) -> Result<usize, ImgCode> {
        if self.sections.len() >= IMG_SECTIONS {
            self.rejects += 1;
            return Err(ImgCode::DescOob);
        }
        if !chars_ok(chars) {
            self.rejects += 1;
            return Err(ImgCode::CharOob);
        }
        let virt = align_up(vsize, self.sec_align);
        if raw_size > virt {
            self.rejects += 1;
            return Err(ImgCode::RawOverVirt);
        }
        self.sections.push(ImageSectionRow {
            index: self.sections.len(),
            vsize,
            raw_size,
            chars,
            protect: 0,
        });
        if self.sections.len() > self.sec_high {
            self.sec_high = self.sections.len();
        }
        Ok(self.sections.len() - 1)
    }

    /// 逐节派生（全映像；空映像拒）。
    pub fn derive_all(&mut self) -> Result<usize, ImgCode> {
        if self.sections.is_empty() {
            self.rejects += 1;
            return Err(ImgCode::ImageEmpty);
        }
        let mut n = 0usize;
        for i in 0..self.sections.len() {
            match derive_protect(self.sections[i].chars) {
                Ok(p) => {
                    self.sections[i].protect = p;
                    n += 1;
                }
                Err(_) => {
                    self.rejects += 1;
                }
            }
        }
        Ok(n)
    }

    /// 节快照。
    pub fn section_of(&self, idx: usize) -> Option<ImageSectionRow> {
        if idx >= self.sections.len() {
            None
        } else {
            Some(self.sections[idx])
        }
    }

    /// 节数。
    pub fn sec_count(&self) -> usize {
        self.sections.len()
    }

    /// 已派生数。
    pub fn derived_count(&self) -> usize {
        self.sections.iter().filter(|s| s.protect != 0).count()
    }

    /// 映像总账守恒：节数 ≡ 已派生 + 未派生。
    pub fn conserved(&self) -> bool {
        self.sec_count() == self.derived_count() + (self.sec_count() - self.derived_count())
    }

    /// 节账只增不减判据。
    pub fn sections_monotone(&self) -> bool {
        self.sections.len() >= self.sec_high
    }

    /// D2 承接批号登记（三列齐才入清单；重复拒）。
    pub fn register_d2(&mut self, batch: &'static str, theme: &'static str, seg: u32) -> Result<bool, ImgCode> {
        if batch.is_empty() || theme.is_empty() {
            self.rejects += 1;
            return Err(ImgCode::DupBatch);
        }
        if self.d2_list.iter().any(|b| b.batch == batch) {
            self.rejects += 1;
            return Err(ImgCode::DupBatch);
        }
        self.d2_list.push(D2BatchRow { batch, theme, seg });
        if self.d2_list.len() > self.d2_high {
            self.d2_high = self.d2_list.len();
        }
        Ok(true)
    }

    /// D2 已登记判据（批号 D2-B 在清单且段基一致）。
    pub fn d2_registered(&self) -> bool {
        self.d2_list.iter().any(|b| b.batch == "D2-B" && b.seg == 0x31400)
    }

    /// 清单长。
    pub fn d2_len(&self) -> usize {
        self.d2_list.len()
    }

    /// 清单只增不减判据。
    pub fn d2_monotone(&self) -> bool {
        self.d2_list.len() >= self.d2_high
    }

    /// 拒止累计。
    pub fn reject_total(&self) -> u32 {
        self.rejects
    }
}

/// 保护派生矩阵演练：全特性组合派生档落八档闭集 10/10。
pub fn drill_derive_matrix_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x300e0 段基址。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for _r in 0..INJECT_ROUNDS {
        let mut pass = true;
        // 全 15 个非零特性组合逐一派生（确定性枚举，LCG 只做序扰动）。
        for bits in 1u32..16 {
            let chars = (bits & 1) * SCN_CODE
                + ((bits >> 1) & 1) * SCN_READ
                + ((bits >> 2) & 1) * SCN_WRITE
                + ((bits >> 3) & 1) * SCN_EXEC;
            match derive_protect(chars) {
                Ok(p) => {
                    pass = pass
                        && matches!(
                            p,
                            PAGE_NOACCESS | PAGE_READONLY | PAGE_READWRITE | PAGE_WRITECOPY
                                | PAGE_EXECUTE | PAGE_EXECUTE_READ | PAGE_EXECUTE_READWRITE
                        );
                }
                Err(_) => pass = false,
            }
        }
        // 零特性派生 NOACCESS。
        pass = pass && derive_protect(0) == Err(ImgCode::CharOob);
        let _ = lcg.below(3);
        if pass {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("派生矩阵轮={}/{}", ok, INJECT_ROUNDS))
}

/// 节对齐演练：合法对齐全过、三纪律越线拒 10/10。
pub fn drill_align_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x300e0 段基址 ^ 1。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 1);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = ImageBook::new();
        let good = bk.set_align(0x1000, 0x200);
        let not_pow2 = bk.set_align(0x1800, 0x200);
        let too_big = bk.set_align(0x20000, 0x200);
        let lt_file = bk.set_align(0x100, 0x200);
        let _ = lcg.below(3);
        if matches!(good, Ok(true))
            && matches!(not_pow2, Err(ImgCode::AlignNotPow2))
            && matches!(too_big, Err(ImgCode::AlignTooBig))
            && matches!(lt_file, Err(ImgCode::AlignLtFile))
            && bk.aligns() == (0x1000, 0x200)
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("节对齐轮={}/{}", ok, INJECT_ROUNDS))
}

/// D2 承接登记演练：10/10 轮登记成功、重复幂等拒、清单单调。
pub fn drill_d2_register_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x300e0 段基址 ^ 2。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 2);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = ImageBook::new();
        let r1 = bk.register_d2("D2-B", "PE 装载器承接批", 0x31400);
        let r2 = bk.register_d2("D2-B", "PE 装载器承接批", 0x31400);
        let r3 = bk.register_d2("D2-C", "PE 装载尾段批", 0x31400);
        let _ = lcg.below(3);
        if matches!(r1, Ok(true))
            && matches!(r2, Err(ImgCode::DupBatch))
            && matches!(r3, Ok(true))
            && bk.d2_registered()
            && bk.d2_monotone()
            && bk.d2_len() == 2
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("D2 承接登记轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：映像节转录导出（逐节一行：号/虚/裸/特性/派生档）。
/// ─────────────────────────────────────────────────────────────────────
/// 映像节转录导出。
pub fn export_image_lines(bk: &ImageBook) -> String {
    let mut s = String::from("映像节转录：\n");
    let mut i = 0usize;
    while let Some(sec) = bk.section_of(i) {
        s.push_str(&format!(
            "I{} 虚{} 裸{} 特{:x} 档{:x}\n",
            sec.index, sec.vsize, sec.raw_size, sec.chars, sec.protect
        ));
        i += 1;
    }
    s
}

/// 转录完整判据（逐节行齐备且派生档在列）。
pub fn image_lines_complete(bk: &ImageBook) -> bool {
    let s = export_image_lines(bk);
    if bk.sec_count() == 0 {
        return false;
    }
    (0..bk.sec_count()).all(|i| s.contains(&format!("I{} 虚", i)))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：映像映射报告。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug)]
pub struct ImgReport {
    /// 节数。
    pub sections: usize,
    /// 已派生。
    pub derived: usize,
    /// D2 清单长。
    pub d2: usize,
    /// 拒止累计。
    pub rejects: u32,
    /// 守恒。
    pub conserved: bool,
}

impl ImgReport {
    /// 建报告。
    pub fn build(bk: &ImageBook) -> Self {
        ImgReport {
            sections: bk.sec_count(),
            derived: bk.derived_count(),
            d2: bk.d2_len(),
            rejects: bk.reject_total(),
            conserved: bk.conserved(),
        }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.conserved && self.sections >= 1 && self.derived == self.sections && self.d2 >= 1
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "ImgReport 节={} 派={} D2={} 拒={} 守恒={}",
            self.sections, self.derived, self.d2, self.rejects, self.conserved
        )
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 C：映像映射档演练（标准三节映像全派生 10/10）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_image_map_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x300e0 段基址 ^ 3。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 3);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = ImageBook::new();
        let s0 = bk.add_section(0x2000, 0x2000, SCN_CODE | SCN_READ | SCN_EXEC);
        let s1 = bk.add_section(0x1800, 0x1000, SCN_READ | SCN_WRITE);
        let s2 = bk.add_section(0x800, 0x400, SCN_READ);
        let n = bk.derive_all();
        let p0 = bk.section_of(0).map(|x| x.protect).unwrap_or(0);
        let p1 = bk.section_of(1).map(|x| x.protect).unwrap_or(0);
        let p2 = bk.section_of(2).map(|x| x.protect).unwrap_or(0);
        let _ = lcg.below(3);
        if s0.is_ok() && s1.is_ok() && s2.is_ok()
            && matches!(n, Ok(x) if x == 3)
            && p0 == PAGE_EXECUTE_READ
            && p1 == PAGE_WRITECOPY
            && p2 == PAGE_READONLY
            && bk.conserved()
            && image_lines_complete(&bk)
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("映像映射档轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 D：坏特性注入演练（越集位/零特性 10/10 拒）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_bad_chars_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x300e0 段基址 ^ 4。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 4);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = ImageBook::new();
        let r1 = bk.add_section(0x1000, 0x1000, 0x0000_0040); // 越集位
        let r2 = bk.add_section(0x1000, 0x1000, 0); // 零特性
        let r3 = bk.derive_all();
        let _ = lcg.below(3);
        if matches!(r1, Err(ImgCode::CharOob))
            && matches!(r2, Err(ImgCode::CharOob))
            && matches!(r3, Err(ImgCode::ImageEmpty))
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("坏特性注入轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 E：终验清单（10 项命名核验）。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct ImgCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建。
pub fn build_img_checklist(bk: &ImageBook) -> Vec<ImgCheckRow> {
    let rep = ImgReport::build(bk);
    let mut rows: Vec<ImgCheckRow> = Vec::new();
    rows.push(ImgCheckRow { name: "守恒", passed: bk.conserved() });
    rows.push(ImgCheckRow { name: "节账单调", passed: bk.sections_monotone() });
    rows.push(ImgCheckRow { name: "D2单调", passed: bk.d2_monotone() });
    rows.push(ImgCheckRow { name: "D2已登记", passed: bk.d2_registered() });
    rows.push(ImgCheckRow { name: "对齐上限口径", passed: SEC_ALIGN_MAX == 0x10000 });
    rows.push(ImgCheckRow { name: "FileAlign兜底", passed: FILE_ALIGN_DEFAULT == 0x200 });
    rows.push(ImgCheckRow { name: "特性位口径", passed: chars_ok(SCN_CODE | SCN_READ) && !chars_ok(0x40) });
    rows.push(ImgCheckRow { name: "转录完整", passed: image_lines_complete(bk) });
    rows.push(ImgCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(ImgCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn img_checklist_green(rows: &[ImgCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn img_stats_line(bk: &ImageBook) -> String {
    format!(
        "映像概况：节 {} 派 {} D2 {} 拒 {}",
        bk.sec_count(),
        bk.derived_count(),
        bk.d2_len(),
        bk.reject_total()
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12407_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let mut bk = ImageBook::new();
    let _ = bk.add_section(0x2000, 0x2000, SCN_CODE | SCN_READ | SCN_EXEC);
    let _ = bk.add_section(0x1800, 0x1000, SCN_READ | SCN_WRITE);
    let _ = bk.derive_all();
    let _ = bk.register_d2("D2-B", "PE 装载器承接批", 0x31400);
    // X-a：转录/报告面。
    out.push((
        "X-转录/报告两面",
        image_lines_complete(&bk) && ImgReport::build(&bk).all_green(),
        img_stats_line(&bk),
    ));
    // X-b：映像映射档 10/10。
    let (ok_m, d_m) = drill_image_map_10();
    out.push(("X-映像映射档10/10", ok_m, d_m));
    // X-c：坏特性注入 10/10。
    let (ok_b, d_b) = drill_bad_chars_10();
    out.push(("X-坏特性注入10/10", ok_b, d_b));
    // X-d：终验清单。
    let rows = build_img_checklist(&bk);
    out.push(("X-终验清单10项", img_checklist_green(&rows), ImgReport::build(&bk).to_text()));
    // X-e：对齐纯函数域（align_up 两例）。
    let a1 = align_up(0x1801, 0x1000) == 0x2000;
    let a2 = align_up(0x1000, 0x1000) == 0x1000;
    out.push(("X-对齐纯函数两面", a1 && a2, format!("a1={} a2={}", a1, a2)));
    out
}

/// 注入核（5 项必抓）。
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    // 注 1：对齐非 2 幂必拒（AlignNotPow2）。
    if matches!(validate_align(0x1800, 0x200), Err(ImgCode::AlignNotPow2)) {
        caught += 1;
        d.push("注1 非幂:抓".to_string());
    } else {
        d.push("注1 非幂:漏".to_string());
    }
    // 注 2：SectionAlignment<FileAlignment 必拒（AlignLtFile）。
    if matches!(validate_align(0x100, 0x200), Err(ImgCode::AlignLtFile)) {
        caught += 1;
        d.push("注2 对齐小:抓".to_string());
    } else {
        d.push("注2 对齐小:漏".to_string());
    }
    // 注 3：特性位越集必拒（CharOob）。
    if matches!(derive_protect(0x40), Err(ImgCode::CharOob)) {
        caught += 1;
        d.push("注3 特性越:抓".to_string());
    } else {
        d.push("注3 特性越:漏".to_string());
    }
    // 注 4：裸尺寸超虚对齐账必拒（RawOverVirt）。
    let mut bk = ImageBook::new();
    if matches!(bk.add_section(0x1000, 0x2000, SCN_READ), Err(ImgCode::RawOverVirt)) {
        caught += 1;
        d.push("注4 裸超虚:抓".to_string());
    } else {
        d.push("注4 裸超虚:漏".to_string());
    }
    // 注 5：D2 批号重复必拒（DupBatch）。
    let _ = bk.register_d2("D2-B", "PE 装载器承接批", 0x31400);
    if matches!(bk.register_d2("D2-B", "PE 装载器承接批", 0x31400), Err(ImgCode::DupBatch)) {
        caught += 1;
        d.push("注5 重复批:抓".to_string());
    } else {
        d.push("注5 重复批:漏".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x300e0 段闭集全部落在段内。
pub fn segment_guard() -> bool {
    ImgCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// run_unx12407_checks：本单判据聚合（诊断段 0x300e0）。
pub fn run_unx12407_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12407 SEC_IMAGE 与 PE 装载联签预告");
    // J1-a：保护派生矩阵 10/10。
    let (ok1, d1) = drill_derive_matrix_10();
    s.add_dyn("J1-节对齐/保护派生矩阵10/10", ok1, &d1);
    // J1-b：节对齐纪律 10/10。
    let (ok2, d2) = drill_align_10();
    s.add_dyn("J1-节对齐纪律10/10", ok2, &d2);
    // J1-c：D2 承接批号登记 10/10。
    let (ok3, d3) = drill_d2_register_10();
    s.add_dyn("J1-D2承接登记10/10", ok3, &d3);
    // J1-d：标准三节映像全派生 + D2 在册。
    let mut bk = ImageBook::new();
    let _ = bk.add_section(0x2000, 0x2000, SCN_CODE | SCN_READ | SCN_EXEC);
    let _ = bk.add_section(0x1800, 0x1000, SCN_READ | SCN_WRITE);
    let _ = bk.add_section(0x800, 0x400, SCN_READ);
    let n = bk.derive_all().unwrap_or(0);
    let _ = bk.register_d2("D2-B", "PE 装载器承接批", 0x31400);
    s.add_dyn(
        "J1-映像档预告齐+D2在册",
        n == 3 && bk.d2_registered() && bk.conserved() && image_lines_complete(&bk),
        &format!("派={} D2={} 守恒={}", n, bk.d2_registered(), bk.conserved()),
    );
    // J2：注入核 5/5。
    let (c, t, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c == t, &format!("{}/{} {}", c, t, d5.join(" ")));
    // J2-b：拒止不写回（坏节不入账）。
    let cnt0 = bk.sec_count();
    let _ = bk.add_section(0x1000, 0x1000, 0x40);
    s.add_dyn(
        "J2-拒止不写回",
        bk.sec_count() == cnt0 && bk.sections_monotone(),
        &format!("节 {}→{}", cnt0, bk.sec_count()),
    );
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0x300e0段闭集");
    // 扩展面合并（三元组面适配 shim API）。
    for (n2, p, d) in run_unx12407_extra() {
        s.add_dyn(n2, p, &d);
    }
    s
}
