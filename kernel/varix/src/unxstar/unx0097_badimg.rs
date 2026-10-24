//! UNX-F0097 · 畸形内核镜像拒载路径 · 诊断段 0x22D0
//! 验收锚点：UNX-F0097-J1 注入 20 组畸形镜像全部拒载，诊断信息完整
//! 不 panic 死循环。零浮点、零 unwrap、零 expect。


use alloc::vec;
use crate::alloc::{format, string::{String, ToString}, vec::Vec};

pub const SEG_BASE: u16 = 0x22D0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_SHORT: u16 = SEG_BASE + 1;  // 头部不足 64 字节
const E_MAGIC: u16 = SEG_BASE + 2;  // 魔数不符
const E_VER: u16 = SEG_BASE + 3;    // 版本越纲
const E_ALIGN: u16 = SEG_BASE + 4;  // 对齐非法（非 2 幂/越窗）
const E_LEN: u16 = SEG_BASE + 5;    // 长度为零或超上限
const E_TRUNC: u16 = SEG_BASE + 6;  // 声明长度超出实际载荷（截断）
const E_ENTRY: u16 = SEG_BASE + 7;  // 入口越界或未对齐
const E_SUM: u16 = SEG_BASE + 8;    // 校验和不符
const E_RANGE: u16 = SEG_BASE + 9;  // 落位窗口与保留区重叠

pub fn detail(code: u16) -> &'static str {
    match code {
        E_SHORT => "头部不足 64 字节：镜像不完整，拒载",
        E_MAGIC => "魔数不符：非 VRIX 镜像，拒载",
        E_VER => "版本越纲：头版本号超出支持闭集",
        E_ALIGN => "对齐非法：页对齐须为 4K~2M 的 2 幂",
        E_LEN => "长度非法：镜像长度为零或超装载上限",
        E_TRUNC => "载荷截断：声明长度大于实际剩余字节",
        E_ENTRY => "入口非法：越出镜像范围或未按页对齐",
        E_SUM => "校验和不符：载荷被篡改或损坏",
        E_RANGE => "落位越窗：目标页窗与保留区重叠",
        _ => "未知诊断码",
    }
}

// ── 镜像格式常量 ─────────────────────────────────────────────
pub const MAGIC: u32 = 0x5849_5256; // 'VRIX'（小端呈现）
pub const HDR_LEN: usize = 64;
pub const VER_SUPPORTED: u16 = 3; // 支持版本闭集 [1, 3]
pub const ALIGN_MIN: u64 = 4096;
pub const ALIGN_MAX: u64 = 2 * 1024 * 1024;
pub const LEN_CAP: u64 = 64 * 1024 * 1024;

/// 镜像头（磁盘表示 64 字节，字段固定小端）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ImageHeader {
    pub magic: u32,
    pub version: u16,
    pub flags: u16,
    pub entry_va: u64,
    pub image_len: u64,
    pub load_va: u64,
    pub page_align: u64,
    pub checksum: u32,
}

impl ImageHeader {
    pub fn parse(buf: &[u8]) -> Result<ImageHeader, u16> {
        if buf.len() < HDR_LEN {
            return Err(E_SHORT);
        }
        let rd32 = |o: usize| -> u32 {
            u32::from_le_bytes([buf[o], buf[o + 1], buf[o + 2], buf[o + 3]])
        };
        let rd64 = |o: usize| -> u64 {
            let mut b = [0u8; 8];
            b.copy_from_slice(&buf[o..o + 8]);
            u64::from_le_bytes(b)
        };
        Ok(ImageHeader {
            magic: rd32(0),
            version: u16::from_le_bytes([buf[8], buf[9]]),
            flags: u16::from_le_bytes([buf[10], buf[11]]),
            entry_va: rd64(16),
            image_len: rd64(24),
            load_va: rd64(32),
            page_align: rd64(40),
            checksum: rd32(56),
        })
    }

    pub fn serialize(&self) -> Vec<u8> {
        let mut b = vec![0u8; HDR_LEN];
        b[0..4].copy_from_slice(&self.magic.to_le_bytes());
        b[8..10].copy_from_slice(&self.version.to_le_bytes());
        b[10..12].copy_from_slice(&self.flags.to_le_bytes());
        b[16..24].copy_from_slice(&self.entry_va.to_le_bytes());
        b[24..32].copy_from_slice(&self.image_len.to_le_bytes());
        b[32..40].copy_from_slice(&self.load_va.to_le_bytes());
        b[40..48].copy_from_slice(&self.page_align.to_le_bytes());
        b[56..60].copy_from_slice(&self.checksum.to_le_bytes());
        b
    }
}

/// FNV-1a 32 位校验（载荷完整性根函数）。
pub fn fnv1a32(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in data {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

// ── 驳回账本：一次引导全部拒载记录可渲染可对账 ──────────────────
#[derive(Clone)]
pub struct RejectRecord {
    pub case_no: u16,
    pub tag: String,
    pub code: u16,
}

/// 镜像闸门：对每个镜像样本逐项校验，任何一项不符即拒。
pub struct ImageGate {
    pub window_base: u64,
    pub window_pages: u64,
    pub rejects: Vec<RejectRecord>,
    pub accepted: u32,
}

impl ImageGate {
    pub fn new(window_base: u64, window_pages: u64) -> Result<ImageGate, u16> {
        if window_pages == 0 {
            return Err(E_RANGE);
        }
        Ok(ImageGate {
            window_base,
            window_pages,
            rejects: Vec::new(),
            accepted: 0,
        })
    }

    pub fn window_end(&self) -> u64 {
        self.window_base + self.window_pages * ALIGN_MIN
    }

    /// 核心校验链：顺序即优先级，第一处不符立即返回。
    pub fn validate(&self, buf: &[u8]) -> Result<ImageHeader, u16> {
        let h = ImageHeader::parse(buf)?;

        if h.magic != MAGIC {
            return Err(E_MAGIC);
        }
        if h.version == 0 || h.version > VER_SUPPORTED {
            return Err(E_VER);
        }
        if h.page_align < ALIGN_MIN
            || h.page_align > ALIGN_MAX
            || !h.page_align.is_power_of_two()
        {
            return Err(E_ALIGN);
        }
        if h.image_len == 0 || h.image_len > LEN_CAP {
            return Err(E_LEN);
        }
        if h.image_len as usize > buf.len() - HDR_LEN {
            return Err(E_TRUNC);
        }
        let load_end = h
            .load_va
            .checked_add(h.image_len)
            .ok_or(E_RANGE)?;
        if h.entry_va < h.load_va || h.entry_va >= load_end {
            return Err(E_ENTRY);
        }
        if h.entry_va % h.page_align != h.load_va % h.page_align {
            return Err(E_ENTRY);
        }
        if load_end > self.window_end() {
            return Err(E_RANGE);
        }
        let body_sum = fnv1a32(&buf[HDR_LEN..HDR_LEN + h.image_len as usize]);
        if body_sum != h.checksum {
            return Err(E_SUM);
        }
        Ok(h)
    }

    /// 带账受理：拒载记账、收载计数，永不 panic。
    pub fn admit(&mut self, case_no: u16, tag: &str, buf: &[u8]) -> bool {
        match self.validate(buf) {
            Ok(_) => {
                self.accepted += 1;
                true
            }
            Err(code) => {
                self.rejects.push(RejectRecord {
                    case_no,
                    tag: tag.to_string(),
                    code,
                });
                false
            }
        }
    }

    pub fn reject_count(&self) -> u32 {
        self.rejects.len() as u32
    }

    /// 诊断渲染：逐条「案号-标签-码(建议)」。
    pub fn render(&self) -> String {
        if self.rejects.is_empty() {
            return "闸门账空：无拒载记录".to_string();
        }
        let mut s = format!(
            "拒载账 {} 条，收载 {}：",
            self.rejects.len(),
            self.accepted
        );
        let mut first = true;
        for r in &self.rejects {
            if !first {
                s.push_str("；");
            }
            first = false;
            s.push_str(&format!(
                "#{} {}→{:04X}({})",
                r.case_no,
                r.tag,
                r.code,
                detail(r.code)
            ));
        }
        s
    }
}

// ── 样本作坊：合法基线 + 20 组确定性畸形化 ──────────────────────
pub fn baseline_image() -> Vec<u8> {
    let body: Vec<u8> = (0..512u32).map(|i| (i * 7 + 3) as u8).collect();
    let h = ImageHeader {
        magic: MAGIC,
        version: 3,
        flags: 1,
        entry_va: 0xFFFF_8000_0010_0000,
        image_len: body.len() as u64,
        load_va: 0xFFFF_8000_0010_0000,
        page_align: ALIGN_MIN,
        checksum: fnv1a32(&body),
    };
    let mut buf = h.serialize();
    buf.extend_from_slice(&body);
    buf
}

/// 畸形化算子：对基线做一处确定性的破坏，返回（标签，改后样本）。
/// 20 组各破坏一处，覆盖诊断码闭集全部九码。
pub fn malformed_variants(base: &[u8]) -> Vec<(&'static str, Vec<u8>)> {
    let mut out: Vec<(&'static str, Vec<u8>)> = Vec::new();

    // V1~V2：头部过短。
    out.push(("截头47B", base[..47].to_vec()));
    out.push(("空镜像", Vec::new()));

    // V3~V4：魔数破坏。
    let mut v = base.to_vec();
    v[0] ^= 0xFF;
    out.push(("魔数一字节", v));
    let mut v = base.to_vec();
    v[3] = b'Q';
    out.push(("魔数末字节", v));

    // V5~V6：版本越纲。
    let mut v = base.to_vec();
    v[8] = 0;
    v[9] = 0; // 版本 0
    out.push(("版本零", v));
    let mut v = base.to_vec();
    v[9] = 0x10; // 版本 4096
    out.push(("版本超纲", v));

    // V7~V9：对齐非法。
    let mut v = base.to_vec();
    v[40] = 0x01; // 对齐 4097（非 2 幂）
    out.push(("非2幂对齐", v));
    let mut v = base.to_vec();
    v[43] = 0x10; // 对齐超 2M
    out.push(("对齐超2M", v));
    let mut v = base.to_vec();
    v[40] = 0x00; // 对齐零
    out.push(("对齐零", v));

    // V10~V11：长度非法。
    let mut v = base.to_vec();
    v[24] = 0; // 长度零
    out.push(("长度零", v));
    let mut v = base.to_vec();
    v[27] = 0x10; // 长度 4G+ 超上限
    out.push(("长度超上限", v));

    // V12~V13：截断。
    let mut v = base.to_vec();
    v[24] = 0x10; // 声明 64K，实际仅 512
    out.push(("声明超实际", v));
    out.push(("砍尾留头", base[..HDR_LEN + 8].to_vec()));

    // V14~V15：入口非法。
    let mut v = base.to_vec();
    v[16] = 0x20; // 入口跳到镜像外
    out.push(("入口越界", v));
    let mut v = base.to_vec();
    v[16] = 0x08; // 入口未按页对齐
    out.push(("入口未对齐", v));

    // V16：校验和破坏。
    let mut v = base.to_vec();
    let n = v.len();
    v[n - 1] ^= 0xFF;
    out.push(("尾字节翻转", v));

    // V17：落位越窗（入口合法、落位高于窗口顶）。
    let mut v = base.to_vec();
    v[32] = 0x00;
    v[34] = 0x40; // load_va 抬到 0x4000_0000_0010_0000 量级
    out.push(("落位越窗", v));

    // V18：落位顶界——入口同步抬顶，载荷尾越出窗口一字节。
    let mut v = base.to_vec();
    v[17] = 0xFF; // entry_va = load+0xFF00（与 load 同余且 >= load）
    v[33] = 0xFF; // load_va  +0xFF00，load_end 越出窗顶
    out.push(("落位顶界越窗", v));

    // V19：版本 4 越纲（边界外一格）。
    let mut v = base.to_vec();
    v[8] = 4;
    v[9] = 0;
    out.push(("版本界外一格", v));

    // V20：校验和破坏两处（多字节损坏同样只报 E_SUM 一码）。
    let mut v = base.to_vec();
    let n = v.len();
    v[n - 1] ^= 0xFF;
    v[n - 2] ^= 0xFF;
    out.push(("双字节损坏", v));

    out
}

// ── 确定性演练：合法 2 例 + 20 组畸形全拒 ────────────────────────
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut gate = ImageGate::new(0xFFFF_8000_0010_0000, 256)?;

    // R1：合法基线收载。
    let base = baseline_image();
    if !gate.admit(0, "基线", &base) {
        return Err(E_SUM);
    }
    if gate.accepted != 1 {
        return Err(E_RANGE);
    }
    hits += 1;

    // R2：低版本合法样本收载（闭集下界）。
    let mut low = base.clone();
    low[8] = 1;
    low[9] = 0;
    if !gate.admit(0, "低版", &low) {
        return Err(E_VER);
    }
    if gate.accepted != 2 {
        return Err(E_RANGE);
    }
    hits += 1;

    // J1：20 组畸形全部拒载，每码落账可追。
    let variants = malformed_variants(&base);
    if variants.len() != 20 {
        return Err(E_RANGE);
    }
    for (i, (tag, img)) in variants.iter().enumerate() {
        let case_no = (i + 1) as u16;
        if gate.admit(case_no, tag, img) {
            // 畸形样本被收载 = 闸门失守，判据红。
            return Err(E_MAGIC);
        }
    }
    if gate.reject_count() != 20 {
        return Err(E_RANGE);
    }
    hits += 1;

    // R3：诊断渲染完整——每条账有码有建议。
    let rep = gate.render();
    if !rep.contains("拒载账 20 条") || !rep.contains("魔数一字节") {
        return Err(E_SUM);
    }
    hits += 1;

    // R4：九码闭集全覆盖检查——20 组至少触及九个不同码。
    let mut seen: Vec<u16> = Vec::new();
    for r in &gate.rejects {
        if !seen.contains(&r.code) {
            seen.push(r.code);
        }
    }
    if seen.len() < 9 {
        return Err(E_RANGE);
    }
    hits += 1;

    // R5：detail() 对九码逐条非空、对越纲码走未知兜底。
    for c in [E_SHORT, E_MAGIC, E_VER, E_ALIGN, E_LEN, E_TRUNC, E_ENTRY, E_SUM, E_RANGE] {
        if detail(c).is_empty() || detail(c) == "未知诊断码" {
            return Err(c);
        }
    }
    if detail(SEG_BASE + 15) != "未知诊断码" {
        return Err(E_RANGE);
    }
    hits += 1;

    // R6：空账渲染路径。
    let g2 = ImageGate::new(0x2000_0000, 16)?;
    if g2.render() != "闸门账空：无拒载记录" || g2.reject_count() != 0 {
        return Err(E_RANGE);
    }
    hits += 1;

    Ok(hits)
}
