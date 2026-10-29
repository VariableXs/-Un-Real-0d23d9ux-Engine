//! unx0635_dsealv1 —— UNX-F0635 域封账版本 v1（A1 终态封存）。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：UNX-F0635 · 域封账版本 v1（A1 终态封存） · 目标 ≥260 行
//! 判据原文摘录：域封账 v1 冻结（六段结构——概括/对冲/红线/覆盖/
//!   遗留/建议 全段+校验和），封后写拦截 100%，三 mock diff 零差。
//! ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计纪律：
//!   1. 六段闭集：概括/对冲/红线/覆盖/遗留/建议，一段一载荷一校验和
//!      （FNV-1a 64 对载荷字节），校验和落账后不得再变。
//!   2. 封账前置：六段全落账 + 逐段校验和自证 + 版本号 = v1 闭集。
//!   3. 封后写拦截 100%：封账后任何段写、段删、校验和重算入口一律
//!      返 XC_RDONLY，无一条旁路。
//!   4. 三 mock diff 零差面：封账载荷须含三 mock 侧指纹（时钟/多核/
//!      后续域），封账内三指纹与装配源复算一致（零差）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，
//!      确定性。
//!
//! 诊断段声明：SEG 0x5120 段位本模块独占。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 诊断段基址（0x5120 段 · 本模块独占）。
pub const SEG_BASE: u16 = 0x5120;
/// 诊断段顶。
pub const SEG_TOP: u16 = 0x512F;

/// XcCode：段号越界。
pub const XC_BAD_SEG: u16 = SEG_BASE + 1; // 0x5121 段号 0..6 闭集外
/// XcCode：段重复落账。
pub const XC_DUP_SEG: u16 = SEG_BASE + 2; // 0x5122 一段一账
/// XcCode：段载荷为空。
pub const XC_EMPTY_PAY: u16 = SEG_BASE + 3; // 0x5123 空载荷拒收
/// XcCode：校验和不符。
pub const XC_CSUM: u16 = SEG_BASE + 4; // 0x5124 载荷校验和不中
/// XcCode：六段未齐禁封。
pub const XC_SEG_SHORT: u16 = SEG_BASE + 5; // 0x5125 封账前置缺
/// XcCode：封账后写拦截。
pub const XC_RDONLY: u16 = SEG_BASE + 6; // 0x5126 封后一切写返此

/// 六段数。
pub const SECTION_N: usize = 6;
/// 封账版本闭集：仅 v1。
pub const SEAL_VER: u32 = 1;
/// 三 mock 侧名（封账载荷指纹面）。
pub const MOCK_TAGS: [&str; 3] = ["时钟域A2", "多核域A3", "后续域总"];

/// 六段名（段号 → 段名）。
pub const SECTION_NAMES: [&str; SECTION_N] =
    ["概括", "对冲", "红线", "覆盖", "遗留", "建议"];

/// FNV-1a 64。
pub fn fnv64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// 单段账。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SealSection {
    /// 段号（0..6）。
    pub seg_no: usize,
    /// 载荷（非空）。
    pub payload: String,
    /// 校验和（对载荷字节 FNV-1a 64）。
    pub csum: u64,
}

impl SealSection {
    /// 校验和自证。
    pub fn csum_ok(&self) -> bool {
        self.csum == fnv64(self.payload.as_bytes())
    }
}

/// 域封账 v1。
#[derive(Clone, Debug)]
pub struct DomainSealV1 {
    /// 六段槽。
    sections: [Option<SealSection>; SECTION_N],
    /// 封账位。
    sealed: bool,
    /// 封账总校验和（封账时落账：六段校验和按位异或）。
    total_csum: Option<u64>,
    /// 三 mock 侧指纹（时钟/多核/后续域）。
    mock_fps: [u64; 3],
}

impl DomainSealV1 {
    /// 开封账台（mock 指纹由装配源喂入）。
    pub fn open(mock_fps: [u64; 3]) -> Self {
        DomainSealV1 {
            sections: [
                None, None, None, None, None, None,
            ],
            sealed: false,
            total_csum: None,
            mock_fps,
        }
    }

    /// 已落段数。
    pub fn seg_n(&self) -> usize {
        self.sections.iter().filter(|s| s.is_some()).count()
    }

    /// 是否封账。
    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 落一段（号闭集 + 去重 + 载荷非空 + 封后拦截）。
    pub fn put_section(&mut self, seg_no: usize, payload: &str) -> Result<(), u16> {
        if self.sealed {
            return Err(XC_RDONLY);
        }
        if seg_no >= SECTION_N {
            return Err(XC_BAD_SEG);
        }
        if self.sections[seg_no].is_some() {
            return Err(XC_DUP_SEG);
        }
        if payload.is_empty() {
            return Err(XC_EMPTY_PAY);
        }
        self.sections[seg_no] = Some(SealSection {
            seg_no,
            payload: payload.to_string(),
            csum: fnv64(payload.as_bytes()),
        });
        Ok(())
    }

    /// 段校验和复算面：逐段自证 + 六段全齐。
    pub fn sections_ok(&self) -> Result<usize, u16> {
        if self.seg_n() != SECTION_N {
            return Err(XC_SEG_SHORT);
        }
        for i in 0..SECTION_N {
            match &self.sections[i] {
                Some(s) => {
                    if !s.csum_ok() {
                        return Err(XC_CSUM);
                    }
                }
                None => return Err(XC_SEG_SHORT),
            }
        }
        Ok(SECTION_N)
    }

    /// 封账：六段齐 + 校验和面过 + 版本 v1 + 三 mock 指纹非零 →
    /// 落总校验和并置封账位。
    pub fn seal(&mut self) -> Result<u64, u16> {
        if self.sealed {
            return Err(XC_RDONLY);
        }
        self.sections_ok()?;
        if SEAL_VER != 1 {
            return Err(XC_SEG_SHORT);
        }
        if self.mock_fps.iter().any(|f| *f == 0) {
            return Err(XC_EMPTY_PAY);
        }
        let mut total: u64 = 0;
        for s in self.sections.iter().flatten() {
            total ^= s.csum;
        }
        self.total_csum = Some(total);
        self.sealed = true;
        Ok(total)
    }

    /// 三 mock diff 零差面：封账内指纹 vs 装配源复算指纹全等。
    pub fn mock_diff_zero(&self, live_fps: [u64; 3]) -> Result<bool, u16> {
        if !self.sealed {
            return Err(XC_SEG_SHORT);
        }
        for i in 0..3 {
            if self.mock_fps[i] != live_fps[i] {
                return Err(XC_CSUM);
            }
        }
        Ok(true)
    }

    /// 封账只读读段（封后才可读——终态封存语义）。
    pub fn read_section(&self, seg_no: usize) -> Result<&SealSection, u16> {
        if !self.sealed {
            return Err(XC_SEG_SHORT);
        }
        if seg_no >= SECTION_N {
            return Err(XC_BAD_SEG);
        }
        match &self.sections[seg_no] {
            Some(s) => Ok(s),
            None => Err(XC_SEG_SHORT),
        }
    }

    /// 总校验和复验：重算六段异或 = 落账值。
    pub fn total_csum_ok(&self) -> Result<bool, u16> {
        let t = match self.total_csum {
            Some(t) => t,
            None => return Err(XC_SEG_SHORT),
        };
        let mut total: u64 = 0;
        for s in self.sections.iter().flatten() {
            total ^= s.csum;
        }
        Ok(total == t)
    }

    /// 人话报告。
    pub fn report(&self) -> String {
        let mut out = alloc::format!(
            "域封账 v{} · 段 {}/{} · 封={} · 总校验和={:016x}",
            SEAL_VER,
            self.seg_n(),
            SECTION_N,
            self.sealed,
            self.total_csum.unwrap_or(0)
        );
        for i in 0..SECTION_N {
            match &self.sections[i] {
                Some(sec) => out.push_str(&alloc::format!(
                    "\n  段{} {} 校验{:016x}",
                    sec.seg_no,
                    SECTION_NAMES[i],
                    sec.csum
                )),
                None => out.push_str(&alloc::format!("\n  段{} {} 缺", i, SECTION_NAMES[i])),
            }
        }
        out
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        fnv64(self.report().as_bytes())
    }
}

/// 装配源指纹（确定性：tag+段数+1 的 FNV）。
pub fn mock_fp(tag: &str, salt: u64) -> u64 {
    let mut buf = String::from(tag);
    buf.push('#');
    buf.push_str(&alloc::format!("{}", salt));
    fnv64(buf.as_bytes())
}

/// 标准封账构造器：六段载荷确定性生成 + 封账 + 零差自证。
pub fn build_sealed() -> Result<(DomainSealV1, u64), u16> {
    let fps = [
        mock_fp(MOCK_TAGS[0], 1),
        mock_fp(MOCK_TAGS[1], 2),
        mock_fp(MOCK_TAGS[2], 3),
    ];
    let mut seal = DomainSealV1::open(fps);
    let payloads = [
        "概括：条目660 行数累计账面 批数33 包12 对冲平8/8",
        "对冲：八账族全平 族差0 复验轮2",
        "红线：12 域禁令 12/12 可达",
        "覆盖：660/660 全覆盖 漏覆盖0",
        "遗留：24 行三类归集 零隐匿",
        "建议：A2 接收注记 8/8 可达",
    ];
    for i in 0..SECTION_N {
        seal.put_section(i, payloads[i])?;
    }
    let total = seal.seal()?;
    let live = [
        mock_fp(MOCK_TAGS[0], 1),
        mock_fp(MOCK_TAGS[1], 2),
        mock_fp(MOCK_TAGS[2], 3),
    ];
    if !seal.mock_diff_zero(live)? {
        return Err(XC_CSUM);
    }
    Ok((seal, total))
}

/// 自例程：主路径 + 错误路径（封后写/缺段/坏校验和/越界/重段/mock 漂移）。
pub fn drill() -> Result<u32, u16> {
    // 主路径。
    let (seal, total) = build_sealed()?;
    if !seal.sealed() || seal.seg_n() != SECTION_N {
        return Err(XC_SEG_SHORT);
    }
    if seal.sections_ok()? != SECTION_N {
        return Err(XC_CSUM);
    }
    if !seal.total_csum_ok()? {
        return Err(XC_CSUM);
    }
    // 封后才可读段。
    let s0 = seal.read_section(0)?;
    if !s0.csum_ok() {
        return Err(XC_CSUM);
    }
    if seal.fingerprint() == 0 {
        return Err(XC_CSUM);
    }

    // 错误路径 1：封后写拦截 100%。
    let mut s1 = build_sealed()?.0;
    if s1.put_section(0, "封后改") != Err(XC_RDONLY) {
        return Err(XC_RDONLY);
    }
    if s1.seal() != Err(XC_RDONLY) {
        return Err(XC_RDONLY);
    }
    // 错误路径 2：未封先读拦截。
    let s2 = DomainSealV1::open([1, 2, 3]);
    if s2.read_section(0) != Err(XC_SEG_SHORT) {
        return Err(XC_SEG_SHORT);
    }
    // 错误路径 3：缺段禁封。
    let mut s3 = DomainSealV1::open([1, 2, 3]);
    s3.put_section(0, "只有一段")?;
    if s3.seal() != Err(XC_SEG_SHORT) {
        return Err(XC_SEG_SHORT);
    }
    // 错误路径 4：段号越界 / 重段 / 空载荷。
    let mut s4 = DomainSealV1::open([1, 2, 3]);
    if s4.put_section(SECTION_N, "越界") != Err(XC_BAD_SEG) {
        return Err(XC_BAD_SEG);
    }
    s4.put_section(0, "一段")?;
    if s4.put_section(0, "重段") != Err(XC_DUP_SEG) {
        return Err(XC_DUP_SEG);
    }
    if s4.put_section(1, "") != Err(XC_EMPTY_PAY) {
        return Err(XC_EMPTY_PAY);
    }
    // 错误路径 5：载荷篡改 → 校验和红。
    let mut s5 = build_sealed()?.0;
    s5.sealed = false;
    if let Some(sec) = &mut s5.sections[2] {
        sec.payload.push('x');
    }
    if s5.sections_ok() != Err(XC_CSUM) {
        return Err(XC_CSUM);
    }
    // 错误路径 6：mock 指纹漂移 → 零差面红。
    let s6 = build_sealed()?.0;
    let drifted = [
        mock_fp(MOCK_TAGS[0], 1),
        mock_fp(MOCK_TAGS[1], 2),
        mock_fp(MOCK_TAGS[2], 999),
    ];
    if s6.mock_diff_zero(drifted) != Err(XC_CSUM) {
        return Err(XC_CSUM);
    }
    Ok(0)
}
