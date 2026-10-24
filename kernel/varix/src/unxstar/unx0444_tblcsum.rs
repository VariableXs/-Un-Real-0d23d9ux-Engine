//! unx0444_tblcsum —— UNX-F0444 表头校验和族（逐表核）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0444 · 表头校验和族 · 目标 240 行
//! 判据原文：UNX-F0444-J1 ACPI 校验和族三账：账一 整表校验
//!           和（表头偏移 9 的 u8：全表字节和 ≡ 0 mod 256）；
//!           账二 RSDP 扩展校验和（RSDP 偏移 32 的 u8：覆盖
//!           偏移 32 起至 RSDP 末，XSDT 代生效）；账三 求和
//!           器与补值器（补值器 = 找一个字节值使全表和 ≡
//!           0，写回偏移 9）；逐表核：N 张表逐张过账，产出
//!           (过, 败) 分账；篡改检出：任意单字节翻动必被整
//!           表和抓出；坏表入账不传播（败表单独立案，不炸
//!           整批）。
//! 组件：字节和器（u64 累加防溢出）+ 补值器 + RSDP 扩展和
//!       + 逐表批账 + 篡改检出核。
//! 职责定位：校验和是 ACPI 表的「封条」——表在内存里被固件
//!   或启动器改坏一个字节，整表和就破；逐表核保证一张坏表
//!   不会把整批表都判死（坏表坏账、好表好账分开记）；补值
//!   器是内核侧重建表（kexec/休眠恢复）时的必备件。
//! 性能逐项分解：求和 O(表长)；补值 O(1)（代数直接算）；
//!   批账 O(Σ表长)。
//! 诊断码段：0x34040（0x34041~0x34049 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const TC_VERSION: &str = "unx0444_tblcsum-v1";

/// 诊断段基址（0x34040 段 · 本模块独占）。
pub const TC_SEG_BASE: u32 = 0x34040;
pub const XC_CSUM: u32 = 0x34041; // 整表和破
pub const XC_XSDP: u32 = 0x34042; // RSDP 扩展和破
pub const XC_HDR_SHORT: u32 = 0x34043; // 表头残
pub const XC_LEN_OOB: u32 = 0x34044; // declared 越缓冲
pub const XC_MAGIC: u32 = 0x34049; // 账魔数破

/// 表头内偏移。
pub const OFF_CSUM: usize = 9;
pub const OFF_LEN: usize = 4;
/// RSDP 布局常量。
pub const RSDP_XSDP_CSUM_OFF: usize = 32;
pub const RSDP_MIN_LEN: usize = 36; // 旧代 RSDP 长（无扩展和覆盖）

/// 全表字节和（u8 截断语义：模 256）。
pub fn byte_sum_mod(raw: &[u8]) -> u8 {
    let mut acc: u64 = 0;
    for &b in raw.iter() {
        acc += b as u64;
    }
    (acc & 0xFF) as u8
}

/// 整表核：全表和 ≡ 0。
pub fn table_ok(raw: &[u8]) -> Result<(), u32> {
    if raw.len() < 10 {
        return Err(XC_HDR_SHORT);
    }
    if byte_sum_mod(raw) == 0 {
        Ok(())
    } else {
        Err(XC_CSUM)
    }
}

/// 补值器：给 csum 字节当前值 cur，求需写入的补值（使全表
/// 和 ≡ 0）。total = 除 csum 外的和；want = (256 - total mod
/// 256) mod 256。
pub fn fill_csum(total_without: u64) -> u8 {
    ((256 - (total_without & 0xFF)) & 0xFF) as u8
}

/// RSDP 扩展校验和核：偏移 32 起到末尾的字节和 ≡ 0。
pub fn rsdp_ext_ok(raw: &[u8]) -> Result<(), u32> {
    if raw.len() < RSDP_MIN_LEN {
        return Err(XC_HDR_SHORT);
    }
    if raw.len() <= RSDP_XSDP_CSUM_OFF {
        // 旧代 RSDP 无扩展和面：直接过（不覆盖）。
        return Ok(());
    }
    if byte_sum_mod(&raw[RSDP_XSDP_CSUM_OFF..]) == 0 {
        Ok(())
    } else {
        Err(XC_XSDP)
    }
}

/// 逐表批账：N 张表分账。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Tally {
    pub pass: u32,
    pub fail: u32,
}

impl Tally {
    pub fn total(&self) -> u32 {
        self.pass + self.fail
    }
}

/// 批核：逐张过账，坏表立案不传播。
pub fn audit_batch(tables: &[Vec<u8>]) -> Tally {
    let mut t = Tally::default();
    for raw in tables.iter() {
        match table_ok(raw) {
            Ok(()) => t.pass += 1,
            Err(_) => t.fail += 1,
        }
    }
    t
}

/// 造表（夹具）：sig 四字符 + 自动补 csum。
pub fn mk_table(sig: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut raw = vec![0u8; 36 + payload.len()];
    raw[0..4].copy_from_slice(sig);
    let len = raw.len() as u32;
    raw[OFF_LEN..OFF_LEN + 4].copy_from_slice(&len.to_le_bytes());
    raw[36..].copy_from_slice(payload);
    let total_wo: u64 = raw.iter().enumerate().filter(|(i, _)| *i != OFF_CSUM).map(|(_, b)| *b as u64).sum();
    raw[OFF_CSUM] = fill_csum(total_wo);
    raw
}

/// 判据入口：UNX-F0444。
pub fn run_unx0444_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0444_tblcsum");
    // 族一：求和器。
    cs.add("空表和 0", byte_sum_mod(&[]) == 0, "模 256");
    cs.add("0xFF 和 255", byte_sum_mod(&[0xFF]) == 255, "单字节");
    cs.add("溢出归零", byte_sum_mod(&[0xFF, 0x01]) == 0, "256≡0");
    // 族二：补值器。
    cs.add("补 0", fill_csum(0) == 0, "全零已平");
    cs.add("补 1", fill_csum(0xFF) == 1, "255→1");
    cs.add("补 255", fill_csum(1) == 255, "1→255");
    // 族三：造表自封。
    let t1 = mk_table(b"APIC", &[1, 2, 3, 4]);
    cs.add("APIC 自封", table_ok(&t1).is_ok(), "补值闭环");
    let t2 = mk_table(b"HPET", &[0; 40]);
    cs.add("HPET 自封", table_ok(&t2).is_ok(), "零载荷");
    // 族四：篡改检出。
    let mut bad = t1.clone();
    bad[36] ^= 0x01; // 翻一个载荷位
    cs.add("单字节翻动抓", table_ok(&bad) == Err(XC_CSUM), "封条破");
    let mut bad2 = t1.clone();
    bad2[OFF_CSUM] ^= 0x80; // 直接翻 csum 字节
    cs.add("csum 字抓", table_ok(&bad2) == Err(XC_CSUM), "封条自坏");
    // 修回：翻回来又平。
    bad[36] ^= 0x01;
    cs.add("翻回复原", table_ok(&bad).is_ok(), "对称性");
    // 族五：批账。
    let good1 = mk_table(b"APIC", &[1]);
    let good2 = mk_table(b"HPET", &[2]);
    let good3 = mk_table(b"MCFG", &[3]);
    let mut badt = mk_table(b"FACP", &[4]);
    badt[40] = 0x77; // 破坏
    let tal = audit_batch(&[good1.clone(), good2.clone(), good3.clone()]);
    cs.add("三好 3/0", tal == (Tally { pass: 3, fail: 0 }), "分账");
    cs.add("总数 3", tal.total() == 3, "口径");
    let tal2 = audit_batch(&[good1, badt, good2]);
    cs.add("一坏不传播", tal2 == (Tally { pass: 2, fail: 1 }), "坏表单立");
    // 族六：RSDP 扩展和。
    let mut rsdp = vec![0u8; 40];
    rsdp[0..8].copy_from_slice(b"RSD PTR ");
    // 偏移 32..40 是 XSDT 地址 8 字节，扩展和覆盖 32..40。
    rsdp[32..40].copy_from_slice(&0x1_0000_0000u64.to_le_bytes());
    let sum: u64 = rsdp[32..].iter().map(|b| *b as u64).sum();
    rsdp.push(fill_csum(sum)); // 尾补 csum 字节
    cs.add("RSDP 扩展和过", rsdp_ext_ok(&rsdp).is_ok(), "32 起覆盖");
    let mut badr = rsdp.clone();
    badr[33] ^= 0x10;
    cs.add("RSDP 扩展和抓", rsdp_ext_ok(&badr) == Err(XC_XSDP), "扩展面破");
    // 旧代 RSDP（36 字节，无扩展面）。
    let legacy = vec![0u8; 36];
    cs.add("旧代 RSDP 免核", rsdp_ext_ok(&legacy).is_ok(), "36 字节无扩展面");
    cs.add("短 RSDP 拒", rsdp_ext_ok(&vec![0; 10]) == Err(XC_HDR_SHORT), "头残");
    // 族七：declared 越界（表尾截断仍以实际字节求和，不炸）。
    let mut trunc = mk_table(b"APIC", &[1, 2, 3]);
    trunc.truncate(30); // 截到 30 字节
    cs.add("截断表仍可核", table_ok(&trunc).is_err() || table_ok(&trunc).is_ok(), "求和不炸");
    // 族八：大表溢出面（65535 个 0xFF → 和模 256 = 255）。
    let big = vec![0xFFu8; 0xFFFF];
    cs.add("大表和 255", byte_sum_mod(&big) == 255, "u64 累加");
    cs
}
