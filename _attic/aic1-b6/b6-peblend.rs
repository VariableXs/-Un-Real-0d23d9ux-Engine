
// ---------------------------------------------------------------------------
// F002 · 深化批次六：**导出表（EAT）真解析**——名称/序号/地址三表 + 按名解析
//
// 主册依据（G-A-02【功能定义】）：「静态 PE 完整装载」的姊妹面——EXE/ DLL
// 的导出目录（DataDir[0]）：按 PE Spec 解析 Export Directory 结构（特征/时间
// 戳/版本/名称/序号基/三表 RVA），按名解析 → 序号 → RVA。零堆：调用方供
// 映像字节切片。
// ---------------------------------------------------------------------------

/// 导出目录字段偏移（PE Spec 8.4——自目录起点）。
const EAT_OFF_FLAGS: usize = 16;
const EAT_OFF_NAME_RVA: usize = 24;
const EAT_OFF_ORD_BASE: usize = 32;
const EAT_OFF_ADDR_TABLE: usize = 36;
const EAT_OFF_NAME_PTRS: usize = 40;
const EAT_OFF_ORDINALS: usize = 44;
const EAT_OFF_COUNT: usize = 20;

fn rva_to_file_off(mapped: &[u8], image_base_rva: usize, rva: u32) -> Option<usize> {
    // 本模型：RVA 即文件偏移（build_static_pe 同构对拍面——节对齐换算随
    // 既有 BlendImage 语义）。
    let off = image_base_rva + rva as usize;
    if off >= mapped.len() {
        return None;
    }
    Some(off)
}

fn read_u32(mapped: &[u8], off: usize) -> Option<u32> {
    if off + 4 > mapped.len() {
        return None;
    }
    Some(u32::from_le_bytes([mapped[off], mapped[off + 1], mapped[off + 2], mapped[off + 3]]))
}

/// 导出表解析结果（计数面——名称数/函数数/序号基）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExportDirectory {
    pub name_rva: u32,
    pub ordinal_base: u32,
    pub addr_table_rva: u32,
    pub name_ptrs_rva: u32,
    pub ordinals_rva: u32,
    pub functions: u32,
    pub names: u32,
}

/// 解析导出目录（`eat_off` = DataDir[0] 指向的文件偏移）。结构不符 → None。
pub fn parse_export_directory(mapped: &[u8], eat_off: usize) -> Option<ExportDirectory> {
    let functions = read_u32(mapped, eat_off + EAT_OFF_COUNT)?;
    let name_rva = read_u32(mapped, eat_off + EAT_OFF_NAME_RVA)?;
    let ordinal_base = read_u32(mapped, eat_off + EAT_OFF_ORD_BASE)?;
    let addr_table_rva = read_u32(mapped, eat_off + EAT_OFF_ADDR_TABLE)?;
    let name_ptrs_rva = read_u32(mapped, eat_off + EAT_OFF_NAME_PTRS)?;
    let ordinals_rva = read_u32(mapped, eat_off + EAT_OFF_ORDINALS)?;
    let names = read_u32(mapped, eat_off + EAT_OFF_COUNT + 4)?;
    if addr_table_rva == 0 || ordinals_rva == 0 {
        return None;
    }
    Some(ExportDirectory {
        name_rva, ordinal_base, addr_table_rva, name_ptrs_rva, ordinals_rva, functions, names,
    })
}

/// 按名解析导出：名称指针表线性查名（ASCII 精确匹配）→ 序号表取序号 →
/// 地址表取 RVA。任一步越界 → None（不猜不冒充）。
pub fn export_resolve(mapped: &[u8], eat: &ExportDirectory, name: &str) -> Option<u32> {
    let name_b = name.as_bytes();
    for i in 0..eat.names as usize {
        let np_off = rva_to_file_off(mapped, 0, eat.name_ptrs_rva)? + i * 4;
        let name_rva = read_u32(mapped, np_off)?;
        let n_off = rva_to_file_off(mapped, 0, name_rva)?;
        let mut end = n_off;
        while end < mapped.len() && mapped[end] != 0 {
            end += 1;
        }
        if end > mapped.len() {
            return None;
        }
        if &mapped[n_off..end] == name_b {
            let ord_off = rva_to_file_off(mapped, 0, eat.ordinals_rva)? + i * 2;
            if ord_off + 2 > mapped.len() {
                return None;
            }
            let ord_idx = u16::from_le_bytes([mapped[ord_off], mapped[ord_off + 1]]) as usize;
            let a_off = rva_to_file_off(mapped, 0, eat.addr_table_rva)? + ord_idx * 4;
            return read_u32(mapped, a_off);
        }
    }
    None
}

/// F002 深化批次六自检。
pub fn run_peblend_deep5_checks() -> CheckSet {
    let mut cs = CheckSet::new("F002-peblend-deep5");
    // 1) 构造最小导出表（1 名 "VarixLoad"）→ 解析目录 + 按名取 RVA。
    let mut m = alloc::vec![0u8; 0x400];
    let eat_off = 0x100usize;
    m[eat_off + EAT_OFF_COUNT..eat_off + EAT_OFF_COUNT + 4].copy_from_slice(&1u32.to_le_bytes());
    m[eat_off + EAT_OFF_NAME_RVA..eat_off + EAT_OFF_NAME_RVA + 4].copy_from_slice(&0x300u32.to_le_bytes());
    m[eat_off + EAT_OFF_ORD_BASE..eat_off + EAT_OFF_ORD_BASE + 4].copy_from_slice(&1u32.to_le_bytes());
    m[eat_off + EAT_OFF_ADDR_TABLE..eat_off + EAT_OFF_ADDR_TABLE + 4].copy_from_slice(&0x360u32.to_le_bytes());
    m[eat_off + EAT_OFF_NAME_PTRS..eat_off + EAT_OFF_NAME_PTRS + 4].copy_from_slice(&0x340u32.to_le_bytes());
    m[eat_off + EAT_OFF_ORDINALS..eat_off + EAT_OFF_ORDINALS + 4].copy_from_slice(&0x380u32.to_le_bytes());
    m[eat_off + EAT_OFF_COUNT + 4..eat_off + EAT_OFF_COUNT + 8].copy_from_slice(&1u32.to_le_bytes());
    // 名称串 @0x340，名指针 @0x340，序号 @0x380（序号 0），地址 @0x360。
    m[0x340..0x34A].copy_from_slice(b"VarixLoad");
    m[0x340..0x344].copy_from_slice(&0x340u32.to_le_bytes());
    m[0x380..0x382].copy_from_slice(&0u16.to_le_bytes());
    m[0x360..0x364].copy_from_slice(&0x1234_5678u32.to_le_bytes());
    let eat = parse_export_directory(&m, eat_off);
    let resolved = match eat {
        Some(e) => export_resolve(&m, &e, "VarixLoad"),
        None => None,
    };
    cs.add(
        "eat_parse_and_resolve",
        matches!(eat, Some(e) if e.functions == 1 && e.names == 1 && e.ordinal_base == 1)
            && resolved == Some(0x1234_5678),
        "",
    );
    // 2) 未知导出名如实 None（不猜）；结构不符（地址表 RVA 0）拒。
    let unknown = match eat {
        Some(e) => export_resolve(&m, &e, "NoSuchFunc"),
        None => None,
    };
    let mut broken = m.clone();
    broken[eat_off + EAT_OFF_ADDR_TABLE..eat_off + EAT_OFF_ADDR_TABLE + 4].copy_from_slice(&0u32.to_le_bytes());
    cs.add(
        "eat_unknown_and_malformed",
        unknown.is_none() && parse_export_directory(&broken, eat_off).is_none(),
        "",
    );
    cs
}
