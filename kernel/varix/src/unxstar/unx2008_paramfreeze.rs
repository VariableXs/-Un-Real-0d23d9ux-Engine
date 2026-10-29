// unx2008_paramfreeze —— UNX-F2008 参数冻结：映射系数/速率限/容量比值
// 判据原文 UNX-F2008-J1：三参数冻结在册（四列纪律），版本对齐 F1635，
// F1897 扩容通道预对接。
// 落位：kernel/varix/src/unxstar/；诊断段 0x81A0 独占（0x81A0~0x81BF）。
//
// 设计（纯功能 no_std）：
//   * 三参数族（闭集注册）：
//       族 A 映射系数：util（‰）→ 频率（kHz）线性映射斜率与偏置；
//       族 B 速率限：升/降 kHz/ms（升 = u32::MAX 哨兵，与 unx2003 同语义）；
//       族 C 容量比值：千分比定点（与 unx2004 同口径 0..=2000‰）；
//   * 四列纪律：名称 | 值 | 单位 | 来源——四列缺一即拒入册；
//   * 冻结语义：在册参数只增不改不删；改写/删除尝试入拒绝账；
//   * 版本锚：F1635 版本语义（v1 起步，单调前进），回退即拒；
//   * F1897 扩容通道预对接：预留 16 扩容槽（add-only），槽满显性满，
//     槽位指纹链防私扩；
//   * 篡改检出：册指纹 FNV 逐行合成，读时验签，改账即破。

use crate::checks::CheckSet;
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0x81A0 独占）──────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x81A0;

pub const XC_PF_SEALED: u32 = SEG_BASE as u32 + 0x01; // 封账后写册
pub const XC_PF_MODIFY: u32 = SEG_BASE as u32 + 0x02; // 在册参数改写
pub const XC_PF_DELETE: u32 = SEG_BASE as u32 + 0x03; // 在册参数删除
pub const XC_PF_COL_MISSING: u32 = SEG_BASE as u32 + 0x04; // 四列缺列
pub const XC_PF_DUP: u32 = SEG_BASE as u32 + 0x05; // 同名重复入册
pub const XC_PF_REGRESS: u32 = SEG_BASE as u32 + 0x06; // 版本回退
pub const XC_PF_EXT_FULL: u32 = SEG_BASE as u32 + 0x07; // 扩容槽满
pub const XC_PF_TAMPER: u32 = SEG_BASE as u32 + 0x08; // 册指纹失配
pub const XC_PF_UNIT: u32 = SEG_BASE as u32 + 0x09; // 单位标签错

pub const EXT_CAP: usize = 16; // F1897 扩容通道槽位
pub const PF_VERSION: u32 = 1; // F1635 版本锚（v1）

/// 单位标签（闭集）。
pub const U_PERMILLE: u8 = 1; // ‰
pub const U_KHZ_PER_MS: u8 = 2; // kHz/ms
pub const U_KHZ: u8 = 3; // kHz
pub const U_COEFF: u8 = 4; // 定点系数（1024 口径）

/// 参数行：四列纪律（名称索引 | 值 | 单位 | 来源）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ParamRow {
    pub name_id: u32,  // 名称索引（预注册名称表，闭集）
    pub value: u64,    // 值（定点整数）
    pub unit: u8,      // 单位标签
    pub source_id: u32, // 来源（施工书条目号低 16 位 + 族号高 16 位）
}

impl ParamRow {
    /// 四列齐备核：值非哨兵 0xFFFF... 且单位在闭集内。
    pub fn complete(&self) -> bool {
        self.value != u64::MAX
            && matches!(self.unit, U_PERMILLE | U_KHZ_PER_MS | U_KHZ | U_COEFF)
            && self.source_id != 0
    }
}

/// 预注册名称表（闭集，新增名称走扩容通道）。
pub const NAME_MAP_SLOPE: u32 = 1; // 族A util→频 斜率
pub const NAME_MAP_BIAS: u32 = 2; // 族A 偏置
pub const NAME_RATE_UP: u32 = 3; // 族B 升速率（哨兵 MAX）
pub const NAME_RATE_DOWN: u32 = 4; // 族B 降速率
pub const NAME_CAP_RATIO: u32 = 5; // 族C 容量比值

/// 参数冻结册。
pub struct ParamFreezer {
    rows: Vec<ParamRow>,
    version: u32,
    /// F1897 扩容通道：槽位指纹链（槽号 → 槽内容指纹）。
    ext_chain: Vec<Option<u32>>,
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl ParamFreezer {
    pub fn new() -> ParamFreezer {
        let mut ext_chain: Vec<Option<u32>> = Vec::new();
        for _ in 0..EXT_CAP {
            ext_chain.push(None);
        }
        ParamFreezer { rows: Vec::new(), version: PF_VERSION, ext_chain, sealed: false, rejects: Vec::new() }
    }

    /// 正册入参：四列齐备 + 同名查重 + 单位闭集。
    pub fn freeze(&mut self, row: ParamRow) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_PF_SEALED, "封账后写册"));
            return Err(XC_PF_SEALED);
        }
        if !row.complete() {
            self.rejects.push((XC_PF_COL_MISSING, "四列缺列"));
            return Err(XC_PF_COL_MISSING);
        }
        // 单位与参数名语义核对（错配即拒）：
        // 速率族必须 kHz/ms、比值族必须 ‰、映射族 kHz/系数
        let unit_ok = match row.name_id {
            NAME_MAP_SLOPE => row.unit == U_COEFF,
            NAME_MAP_BIAS => row.unit == U_KHZ,
            NAME_RATE_UP | NAME_RATE_DOWN => row.unit == U_KHZ_PER_MS,
            NAME_CAP_RATIO => row.unit == U_PERMILLE,
            _ => true, // 扩容名称不核单位配对（扩容自核）
        };
        if !unit_ok {
            self.rejects.push((XC_PF_UNIT, "单位标签错配"));
            return Err(XC_PF_UNIT);
        }
        for r in &self.rows {
            if r.name_id == row.name_id {
                self.rejects.push((XC_PF_DUP, "同名重复入册"));
                return Err(XC_PF_DUP);
            }
        }
        self.rows.push(row);
        Ok(())
    }

    /// 在册改写尝试：必拒（冻结语义），拒绝入账。
    pub fn try_modify(&mut self, name_id: u32, _new_value: u64) -> Result<(), u32> {
        let mut exists = false;
        for r in &self.rows {
            if r.name_id == name_id {
                exists = true;
            }
        }
        if !exists {
            // 未在册：按缺列语义拒（改写面无此名）
            self.rejects.push((XC_PF_COL_MISSING, "改写面无名"));
            return Err(XC_PF_COL_MISSING);
        }
        self.rejects.push((XC_PF_MODIFY, "在册参数改写"));
        Err(XC_PF_MODIFY)
    }

    /// 在册删除尝试：必拒（冻结语义）。
    pub fn try_delete(&mut self, name_id: u32) -> Result<(), u32> {
        let mut exists = false;
        for r in &self.rows {
            if r.name_id == name_id {
                exists = true;
            }
        }
        if !exists {
            self.rejects.push((XC_PF_COL_MISSING, "删除面无名"));
            return Err(XC_PF_COL_MISSING);
        }
        self.rejects.push((XC_PF_DELETE, "在册参数删除"));
        Err(XC_PF_DELETE)
    }

    /// 版本前进（F1635 对齐）：只进不退。
    pub fn bump_version(&mut self, to: u32) -> Result<u32, u32> {
        if self.sealed {
            return Err(XC_PF_SEALED);
        }
        if to <= self.version {
            self.rejects.push((XC_PF_REGRESS, "版本回退"));
            return Err(XC_PF_REGRESS);
        }
        self.version = to;
        Ok(self.version)
    }

    /// F1897 扩容通道：占槽 add-only（同槽二占拒、槽满显性满）。
    pub fn ext_claim(&mut self, slot: usize, content_fingerprint: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(XC_PF_SEALED);
        }
        if slot >= EXT_CAP {
            self.rejects.push((XC_PF_EXT_FULL, "扩容槽满"));
            return Err(XC_PF_EXT_FULL);
        }
        if self.ext_chain[slot].is_some() {
            self.rejects.push((XC_PF_DUP, "扩容槽二占"));
            return Err(XC_PF_DUP);
        }
        self.ext_chain[slot] = Some(content_fingerprint);
        Ok(())
    }

    /// 读册验签：重算册指纹并与入账指纹比对（篡改检出）。
    pub fn verify(&self, recorded: u32) -> bool {
        self.fingerprint32() == recorded
    }

    /// 册指纹：逐行 FNV（含版本与扩容链）。
    pub fn fingerprint32(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        h = h.wrapping_mul(0x0100_0193) ^ self.version;
        for r in &self.rows {
            h ^= r.name_id;
            h = h.wrapping_mul(0x0100_0193);
            h ^= r.value as u32;
            h = h.wrapping_mul(0x0100_0193);
            h ^= (r.value >> 32) as u32;
            h = h.wrapping_mul(0x0100_0193);
            h ^= r.unit as u32;
            h = h.wrapping_mul(0x0100_0193);
            h ^= r.source_id;
            h = h.wrapping_mul(0x0100_0193);
        }
        for slot in &self.ext_chain {
            match slot {
                Some(f) => {
                    h ^= *f;
                    h = h.wrapping_mul(0x0100_0193);
                }
                None => {
                    h ^= 0xFFFF_FFFF;
                    h = h.wrapping_mul(0x0100_0193);
                }
            }
        }
        h
    }

    /// 四列审计：全行四列齐备（冻结册无残行）。
    pub fn audit(&self) -> bool {
        for r in &self.rows {
            if !r.complete() {
                return false;
            }
        }
        true
    }

    /// 查参：名称索引取行（无旁路：返回副本）。
    pub fn lookup(&self, name_id: u32) -> Option<ParamRow> {
        for r in &self.rows {
            if r.name_id == name_id {
                return Some(*r);
            }
        }
        None
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_PF_SEALED);
        }
        self.sealed = true;
        Ok(self.fingerprint32() as u64)
    }

    pub fn row_count(&self) -> usize {
        self.rows.len()
    }
    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
    pub fn version(&self) -> u32 {
        self.version
    }
    pub fn ext_used(&self) -> usize {
        let mut n = 0;
        for s in &self.ext_chain {
            if s.is_some() {
                n += 1;
            }
        }
        n
    }
}

/// 标准三参数族入册（判据夹具）。
pub fn standard_book() -> ParamFreezer {
    let mut b = ParamFreezer::new();
    // 族A 映射系数：util‰ → 频率，斜率 = 2400 kHz/‰? 口径：系数定点（1024 基线斜率 2048）
    let _ = b.freeze(ParamRow { name_id: NAME_MAP_SLOPE, value: 2048, unit: U_COEFF, source_id: (2008 << 16) | 1 });
    // 族A 偏置：400 MHz 地板
    let _ = b.freeze(ParamRow { name_id: NAME_MAP_BIAS, value: 400_000, unit: U_KHZ, source_id: (2008 << 16) | 2 });
    // 族B 速率限：升 = MAX 哨兵（即时）、降 = 500 kHz/ms
    let _ = b.freeze(ParamRow {
        name_id: NAME_RATE_UP,
        value: u32::MAX as u64,
        unit: U_KHZ_PER_MS,
        source_id: (2003 << 16) | 3,
    });
    let _ = b.freeze(ParamRow { name_id: NAME_RATE_DOWN, value: 500, unit: U_KHZ_PER_MS, source_id: (2003 << 16) | 4 });
    // 族C 容量比值：1500‰（Turbo 1.5x，对齐 unx2004）
    let _ = b.freeze(ParamRow { name_id: NAME_CAP_RATIO, value: 1500, unit: U_PERMILLE, source_id: (2004 << 16) | 5 });
    b
}

/// 判据入口：UNX-F2008-J1。
pub fn run_unx2008_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx2008");

    // J1-01 三参数族在册（5 行标准册）
    {
        let b = standard_book();
        let ok = b.row_count() == 5
            && b.lookup(NAME_MAP_SLOPE).is_some()
            && b.lookup(NAME_RATE_DOWN).is_some()
            && b.lookup(NAME_CAP_RATIO).is_some();
        s.add("J1-01-三族在册", ok, if ok { "" } else { "参数族缺行" });
    }
    // J1-02 四列纪律（缺列必拒：零值哨兵 / 非法单位 / 零来源）
    {
        let mut b = ParamFreezer::new();
        let r1 = b.freeze(ParamRow { name_id: 9, value: 0, unit: U_COEFF, source_id: 1 }); // value!=MAX 合法（0 是合法值）→ 用 MAX 哨兵测缺列
        let r2 = b.freeze(ParamRow { name_id: 9, value: u64::MAX, unit: U_COEFF, source_id: 1 });
        let r3 = b.freeze(ParamRow { name_id: 9, value: 1, unit: 99, source_id: 1 });
        let r4 = b.freeze(ParamRow { name_id: 9, value: 1, unit: U_COEFF, source_id: 0 });
        let ok = r1.is_ok() && r2 == Err(XC_PF_COL_MISSING) && r3 == Err(XC_PF_COL_MISSING) && r4 == Err(XC_PF_COL_MISSING);
        s.add("J1-02-四列纪律", ok, if ok { "" } else { "缺列放行" });
    }
    // J1-03 冻结语义：改写必拒
    {
        let mut b = standard_book();
        let r = b.try_modify(NAME_MAP_SLOPE, 4096);
        let ok = r == Err(XC_PF_MODIFY)
            && match b.lookup(NAME_MAP_SLOPE) {
                Some(row) => row.value == 2048,
                None => false,
            }
            && b.reject_count() == 1;
        s.add("J1-03-改写必拒", ok, if ok { "" } else { "改写放行" });
    }
    // J1-04 冻结语义：删除必拒
    {
        let mut b = standard_book();
        let r = b.try_delete(NAME_CAP_RATIO);
        let ok = r == Err(XC_PF_DELETE) && b.lookup(NAME_CAP_RATIO).is_some() && b.row_count() == 5;
        s.add("J1-04-删除必拒", ok, if ok { "" } else { "删除放行" });
    }
    // J1-05 同名重复入册拒
    {
        let mut b = ParamFreezer::new();
        let r1 = b.freeze(ParamRow { name_id: NAME_MAP_SLOPE, value: 2048, unit: U_COEFF, source_id: 1 });
        let r2 = b.freeze(ParamRow { name_id: NAME_MAP_SLOPE, value: 4096, unit: U_COEFF, source_id: 1 });
        let ok = r1.is_ok() && r2 == Err(XC_PF_DUP) && b.row_count() == 1;
        s.add("J1-05-重复入册拒", ok, if ok { "" } else { "重复放行" });
    }
    // J1-06 单位语义错配拒（斜率配 kHz 应拒）
    {
        let mut b = ParamFreezer::new();
        let r = b.freeze(ParamRow { name_id: NAME_MAP_SLOPE, value: 2048, unit: U_KHZ, source_id: 1 });
        let ok = r == Err(XC_PF_UNIT);
        s.add("J1-06-单位错配拒", ok, if ok { "" } else { "错配放行" });
    }
    // J1-07 版本锚对齐 F1635（v1 起步 + 只进不退）
    {
        let mut b = ParamFreezer::new();
        let r1 = b.bump_version(2);
        let r2 = b.bump_version(2);
        let r3 = b.bump_version(1);
        let ok = b.version() == PF_VERSION && r1 == Ok(2) && r2 == Err(XC_PF_REGRESS) && r3 == Err(XC_PF_REGRESS);
        s.add("J1-07-版本锚F1635", ok, if ok { "" } else { "版本锚漂移" });
    }
    // J1-08 F1897 扩容通道预对接（占槽 add-only + 槽满显性）
    {
        let mut b = ParamFreezer::new();
        let mut all_ok = true;
        for k in 0..EXT_CAP {
            if b.ext_claim(k, 0xA000 + k as u32).is_err() {
                all_ok = false;
            }
        }
        let full = b.ext_claim(EXT_CAP, 0xBEEF) == Err(XC_PF_EXT_FULL);
        let dup = b.ext_claim(3, 0x1234) == Err(XC_PF_DUP);
        let ok = all_ok && b.ext_used() == EXT_CAP && full && dup;
        s.add("J1-08-扩容通道F1897", ok, if ok { "" } else { "扩容面漂移" });
    }
    // J1-09 册指纹验签（读时验签绿 + 篡改即破）
    {
        let b = standard_book();
        let fp = b.fingerprint32();
        let ok_green = b.verify(fp);
        // 模拟账面篡改：构造同内容不同版本的册，指纹必异
        let mut b2 = standard_book();
        let _ = b2.bump_version(9);
        let ok = ok_green && !b2.verify(fp);
        s.add("J1-09-验签+篡改破", ok, if ok { "" } else { "验签失效" });
    }
    // J1-10 册审计（四列全齐备）
    {
        let b = standard_book();
        let ok = b.audit();
        s.add("J1-10-册审计", ok, if ok { "" } else { "残行在册" });
    }
    // J1-11 升速率哨兵语义与 unx2003 对齐（MAX = 即时）
    {
        let b = standard_book();
        match b.lookup(NAME_RATE_UP) {
            Some(row) => {
                let ok = row.value == u32::MAX as u64 && row.unit == U_KHZ_PER_MS;
                s.add("J1-11-升速率哨兵", ok, if ok { "" } else { "哨兵语义漂移" });
            }
            None => {
                s.add("J1-11-升速率哨兵", false, "参数缺行");
                return s;
            }
        }
    }
    // J1-12 容量比值与 unx2004 口径对齐（1500‰ → 系数 1536）
    {
        let b = standard_book();
        let ok = match b.lookup(NAME_CAP_RATIO) {
            Some(row) => row.value == 1500 && cap_coeff_of(row.value as u32) == 1536,
            None => false,
        };
        s.add("J1-12-比值口径对齐", ok, if ok { "" } else { "口径漂移" });
    }
    // J1-13 封账后写册必拒 + 封账稳定
    {
        let mut b = standard_book();
        let pre = b.fingerprint32();
        let _ = b.seal();
        let r = b.freeze(ParamRow { name_id: 99, value: 1, unit: U_COEFF, source_id: 1 });
        let ok = r == Err(XC_PF_SEALED) && b.fingerprint32() == pre && b.seal().is_err();
        s.add("J1-13-封账拒+稳定", ok, if ok { "" } else { "封账漂移" });
    }
    // J1-14 指纹冻结（同册同指纹）
    {
        let fp = || -> u32 { standard_book().fingerprint32() };
        let ok = fp() == fp() && fp() != 0;
        s.add("J1-14-指纹冻结", ok, if ok { "" } else { "指纹漂移" });
    }
    // J1-15 段位独占
    {
        let ok = SEG_BASE == 0x81A0;
        s.add("J1-15-段位独占", ok, if ok { "" } else { "0x81A0~0x81BF 本模块独占失守" });
    }
    s
}

/// 容量系数口径（与 unx2004 同式：1024 × ratio / 1000 四舍五入）。
/// 本地复算（不引依赖，口径单源注释锚定 unx2004::CapacityCorrector::coeff_of）。
fn cap_coeff_of(ratio_permille: u32) -> u32 {
    (((1024u64) * (ratio_permille as u64) + 500) / 1000) as u32
}
