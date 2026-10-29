//! UNX-F11293 静态断言与 ABI 自检诊断（0x1f680 段）
//!
//! 锚点：UNX-F11293-J1 ABI 自检面（结构偏移/尺寸断言集）落账，断言零违例。
//!
//! 纯功能模型：把 x86-64 LP64 下八个 ELF 结构的偏移/宽度/尺寸写成静态断言表
//! （Ehdr 64 / Phdr 56 / Shdr 64 / Sym 24 / Dyn 16 / Rel 16 / Rela 24 /
//! Nhdr 12），自检器逐结构校验四件事：字段不越界、字段不重叠且按偏移递增、
//! 每个字段按自身宽度自然对齐、结构尺寸为最大字段宽度的整数倍。任一违例
//! 即定位到「结构 + 字段」并拒止，诊断面不留盲区。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1f680;
/// 字段偏移回退或重叠
pub const AB_EORDER: u32 = SEG_BASE + 0x01;
/// 字段越过结构末尾
pub const AB_ECROSS: u32 = SEG_BASE + 0x02;
/// 字段未按宽度自然对齐
pub const AB_EALIGN: u32 = SEG_BASE + 0x03;
/// 结构尺寸非字段宽度整数倍
pub const AB_ESIZE: u32 = SEG_BASE + 0x04;
/// 空结构或空字段表
pub const AB_EEMPTY: u32 = SEG_BASE + 0x05;
/// 冻结后登记
pub const AB_EFROZEN: u32 = SEG_BASE + 0x06;
/// 结构名重复
pub const AB_EDUP: u32 = SEG_BASE + 0x07;
/// 字段宽度非法（0 或非 2 的幂）
pub const AB_EWIDTH: u32 = SEG_BASE + 0x08;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 单字段断言：名 + 偏移 + 宽度（字节）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FieldSpec {
    pub name: &'static str,
    pub offset: u32,
    pub width: u8,
}

impl FieldSpec {
    /// 宽度合法：非零且为 2 的幂（1/2/4/8/16）。
    pub fn width_ok(&self) -> bool {
        self.width != 0 && (self.width & self.width.wrapping_sub(1)) == 0
    }

    /// 自然对齐：偏移是宽度的整数倍。
    pub fn aligned(&self) -> bool {
        self.width != 0 && self.offset % self.width as u32 == 0
    }

    /// 字段末端（不含）。
    pub fn end(&self) -> u32 {
        self.offset.wrapping_add(self.width as u32)
    }
}

/// 单结构断言：名 + 尺寸 + 字段表。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StructSpec {
    pub name: &'static str,
    pub size: u32,
    pub fields: &'static [FieldSpec],
}

impl StructSpec {
    /// 最大字段宽度（尺寸整除判据用）。
    pub fn max_width(&self) -> u8 {
        let mut m = 0u8;
        let mut k = 0usize;
        while k < self.fields.len() {
            if let Some(f) = self.fields.get(k) {
                if f.width > m {
                    m = f.width;
                }
            }
            k = k.wrapping_add(1);
        }
        m
    }

    /// 逐字段四关自检；返回首个违例（字段下标, 错误码）。
    pub fn self_check(&self) -> Result<(), (usize, u32)> {
        if self.size == 0 || self.fields.is_empty() {
            return Err((0, AB_EEMPTY));
        }
        let mut prev_end = 0u32;
        let mut k = 0usize;
        while k < self.fields.len() {
            let f = match self.fields.get(k) {
                Some(v) => *v,
                None => return Err((k, AB_EEMPTY)),
            };
            if !f.width_ok() {
                return Err((k, AB_EWIDTH));
            }
            if !f.aligned() {
                return Err((k, AB_EALIGN));
            }
            if f.end() > self.size {
                return Err((k, AB_ECROSS));
            }
            if k > 0 && f.offset < prev_end {
                return Err((k, AB_EORDER));
            }
            prev_end = f.end();
            k = k.wrapping_add(1);
        }
        let mw = self.max_width();
        if mw == 0 || self.size % mw as u32 != 0 {
            return Err((0, AB_ESIZE));
        }
        Ok(())
    }

    /// 字段总数。
    pub fn field_count(&self) -> usize {
        self.fields.len()
    }

    /// 结构指纹：名 + 尺寸 + 每字段三元组。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        let nb = self.name.as_bytes();
        let mut b = 0usize;
        while b < nb.len() {
            h = (h ^ nb[b] as u64).wrapping_mul(FNV_PRIME);
            b = b.wrapping_add(1);
        }
        h = (h ^ self.size as u64).wrapping_mul(FNV_PRIME);
        let mut k = 0usize;
        while k < self.fields.len() {
            if let Some(f) = self.fields.get(k) {
                h = (h ^ f.offset as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ f.width as u64).wrapping_mul(FNV_PRIME);
            }
            k = k.wrapping_add(1);
        }
        h
    }
}

/// Ehdr 字段（14 项）。
pub const EHDR_FIELDS: [FieldSpec; 14] = [
    FieldSpec { name: "e_ident", offset: 0, width: 16 },
    FieldSpec { name: "e_type", offset: 16, width: 2 },
    FieldSpec { name: "e_machine", offset: 18, width: 2 },
    FieldSpec { name: "e_version", offset: 20, width: 4 },
    FieldSpec { name: "e_entry", offset: 24, width: 8 },
    FieldSpec { name: "e_phoff", offset: 32, width: 8 },
    FieldSpec { name: "e_shoff", offset: 40, width: 8 },
    FieldSpec { name: "e_flags", offset: 48, width: 4 },
    FieldSpec { name: "e_ehsize", offset: 52, width: 2 },
    FieldSpec { name: "e_phentsize", offset: 54, width: 2 },
    FieldSpec { name: "e_phnum", offset: 56, width: 2 },
    FieldSpec { name: "e_shentsize", offset: 58, width: 2 },
    FieldSpec { name: "e_shnum", offset: 60, width: 2 },
    FieldSpec { name: "e_shstrndx", offset: 62, width: 2 },
];

/// Phdr 字段（8 项）。
pub const PHDR_FIELDS: [FieldSpec; 8] = [
    FieldSpec { name: "p_type", offset: 0, width: 4 },
    FieldSpec { name: "p_flags", offset: 4, width: 4 },
    FieldSpec { name: "p_offset", offset: 8, width: 8 },
    FieldSpec { name: "p_vaddr", offset: 16, width: 8 },
    FieldSpec { name: "p_paddr", offset: 24, width: 8 },
    FieldSpec { name: "p_filesz", offset: 32, width: 8 },
    FieldSpec { name: "p_memsz", offset: 40, width: 8 },
    FieldSpec { name: "p_align", offset: 48, width: 8 },
];

/// Shdr 字段（10 项）。
pub const SHDR_FIELDS: [FieldSpec; 10] = [
    FieldSpec { name: "sh_name", offset: 0, width: 4 },
    FieldSpec { name: "sh_type", offset: 4, width: 4 },
    FieldSpec { name: "sh_flags", offset: 8, width: 8 },
    FieldSpec { name: "sh_addr", offset: 16, width: 8 },
    FieldSpec { name: "sh_offset", offset: 24, width: 8 },
    FieldSpec { name: "sh_size", offset: 32, width: 8 },
    FieldSpec { name: "sh_link", offset: 40, width: 4 },
    FieldSpec { name: "sh_info", offset: 44, width: 4 },
    FieldSpec { name: "sh_addralign", offset: 48, width: 8 },
    FieldSpec { name: "sh_entsize", offset: 56, width: 8 },
];

/// Sym 字段（6 项）。
pub const SYM_FIELDS: [FieldSpec; 6] = [
    FieldSpec { name: "st_name", offset: 0, width: 4 },
    FieldSpec { name: "st_info", offset: 4, width: 1 },
    FieldSpec { name: "st_other", offset: 5, width: 1 },
    FieldSpec { name: "st_shndx", offset: 6, width: 2 },
    FieldSpec { name: "st_value", offset: 8, width: 8 },
    FieldSpec { name: "st_size", offset: 16, width: 8 },
];

/// Dyn 字段（2 项）。
pub const DYN_FIELDS: [FieldSpec; 2] = [
    FieldSpec { name: "d_tag", offset: 0, width: 8 },
    FieldSpec { name: "d_un", offset: 8, width: 8 },
];

/// Rel 字段（2 项）。
pub const REL_FIELDS: [FieldSpec; 2] = [
    FieldSpec { name: "r_offset", offset: 0, width: 8 },
    FieldSpec { name: "r_info", offset: 8, width: 8 },
];

/// Rela 字段（3 项）。
pub const RELA_FIELDS: [FieldSpec; 3] = [
    FieldSpec { name: "r_offset", offset: 0, width: 8 },
    FieldSpec { name: "r_info", offset: 8, width: 8 },
    FieldSpec { name: "r_addend", offset: 16, width: 8 },
];

/// Nhdr 字段（3 项）。
pub const NHDR_FIELDS: [FieldSpec; 3] = [
    FieldSpec { name: "n_namesz", offset: 0, width: 4 },
    FieldSpec { name: "n_descsz", offset: 4, width: 4 },
    FieldSpec { name: "n_type", offset: 8, width: 4 },
];

/// x86-64 LP64 标准断言表（八结构，四十八字段）。
pub const ABI_TABLE: [StructSpec; 8] = [
    StructSpec { name: "Elf64_Ehdr", size: 64, fields: &EHDR_FIELDS },
    StructSpec { name: "Elf64_Phdr", size: 56, fields: &PHDR_FIELDS },
    StructSpec { name: "Elf64_Shdr", size: 64, fields: &SHDR_FIELDS },
    StructSpec { name: "Elf64_Sym", size: 24, fields: &SYM_FIELDS },
    StructSpec { name: "Elf64_Dyn", size: 16, fields: &DYN_FIELDS },
    StructSpec { name: "Elf64_Rel", size: 16, fields: &REL_FIELDS },
    StructSpec { name: "Elf64_Rela", size: 24, fields: &RELA_FIELDS },
    StructSpec { name: "Elf64_Nhdr", size: 12, fields: &NHDR_FIELDS },
];

/// ABI 自检册：结构表 + 冻结位。
#[derive(Clone, Debug)]
pub struct AbiAudit {
    specs: Vec<StructSpec>,
    frozen: bool,
    pub scans: u64,
    pub rejects: u64,
}

impl AbiAudit {
    /// 空册。
    pub fn empty() -> AbiAudit {
        AbiAudit { specs: Vec::new(), frozen: false, scans: 0, rejects: 0 }
    }

    /// 标准册：装入 ABI_TABLE 八结构。
    pub fn standard() -> AbiAudit {
        let mut a = AbiAudit::empty();
        let mut k = 0usize;
        while k < ABI_TABLE.len() {
            if let Some(spec) = ABI_TABLE.get(k) {
                let _ = a.register(*spec);
            }
            k = k.wrapping_add(1);
        }
        a
    }

    pub fn len(&self) -> usize {
        self.specs.len()
    }

    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 按名查结构。
    pub fn find(&self, name: &str) -> Option<&StructSpec> {
        let mut k = 0usize;
        while k < self.specs.len() {
            if let Some(spec) = self.specs.get(k) {
                if spec.name == name {
                    return Some(spec);
                }
            }
            k = k.wrapping_add(1);
        }
        None
    }

    /// 登记结构：冻结 / 重名 / 空字段三关。
    pub fn register(&mut self, spec: StructSpec) -> Result<(), u32> {
        if self.frozen {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(AB_EFROZEN);
        }
        let r = self.register_inner(spec);
        if r.is_err() {
            self.rejects = self.rejects.wrapping_add(1);
        }
        r
    }

    fn register_inner(&mut self, spec: StructSpec) -> Result<(), u32> {
        if spec.name.is_empty() || spec.fields.is_empty() || spec.size == 0 {
            return Err(AB_EEMPTY);
        }
        if self.find(spec.name).is_some() {
            return Err(AB_EDUP);
        }
        self.specs.push(spec);
        Ok(())
    }

    /// 字段总数。
    pub fn field_total(&self) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.specs.len() {
            if let Some(spec) = self.specs.get(k) {
                n = n.wrapping_add(spec.field_count());
            }
            k = k.wrapping_add(1);
        }
        n
    }

    /// 违例清单：逐结构自检，收集「结构名 + 字段名 + 码」。
    pub fn violations(&self) -> Vec<(String, String, u32)> {
        let mut out: Vec<(String, String, u32)> = Vec::new();
        let mut k = 0usize;
        while k < self.specs.len() {
            if let Some(spec) = self.specs.get(k) {
                if let Err((ix, code)) = spec.self_check() {
                    let fname = match spec.fields.get(ix) {
                        Some(f) => f.name.to_string(),
                        None => "-".to_string(),
                    };
                    out.push((spec.name.to_string(), fname, code));
                }
            }
            k = k.wrapping_add(1);
        }
        out
    }

    /// 全表自检：违例为零才算落账，返回账指纹。
    pub fn audit(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(AB_EFROZEN);
        }
        if self.specs.is_empty() {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(AB_EEMPTY);
        }
        self.scans = self.scans.wrapping_add(1);
        let bad = self.violations();
        if let Some((_, _, code)) = bad.first() {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(*code);
        }
        Ok(self.fold())
    }

    /// 册指纹：逐结构链式 fold。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut k = 0usize;
        while k < self.specs.len() {
            if let Some(spec) = self.specs.get(k) {
                h = (h ^ spec.fold()).wrapping_mul(FNV_PRIME);
            }
            k = k.wrapping_add(1);
        }
        h
    }

    /// 冻结：幂等取回锁定指纹。
    pub fn freeze(&mut self) -> Option<u64> {
        if self.frozen {
            return Some(self.fold());
        }
        self.frozen = true;
        Some(self.fold())
    }
}

/// 标准装配：装表 → 自检。
pub fn build_audited() -> (AbiAudit, Option<u64>) {
    let mut a = AbiAudit::standard();
    let v = a.audit().ok();
    (a, v)
}

/// 四路注入：错位字段 / 越界字段 / 乱序字段 / 冻结后登记，全部须拒。
pub fn inject_four() -> [bool; 4] {
    static BAD_ALIGN: [FieldSpec; 2] = [
        FieldSpec { name: "a", offset: 0, width: 4 },
        FieldSpec { name: "b", offset: 5, width: 4 },
    ];
    static BAD_CROSS: [FieldSpec; 2] = [
        FieldSpec { name: "a", offset: 0, width: 8 },
        FieldSpec { name: "b", offset: 16, width: 8 },
    ];
    static BAD_ORDER: [FieldSpec; 3] = [
        FieldSpec { name: "a", offset: 0, width: 4 },
        FieldSpec { name: "b", offset: 8, width: 4 },
        FieldSpec { name: "c", offset: 4, width: 4 },
    ];
    let mut got = [false; 4];

    // ① 未按宽度对齐（off=5, width=4）
    let s1 = StructSpec { name: "BadAlign", size: 16, fields: &BAD_ALIGN };
    got[0] = s1.self_check().err() == Some((1, AB_EALIGN));

    // ② 越界（off=16 + 8 > 20）
    let s2 = StructSpec { name: "BadCross", size: 20, fields: &BAD_CROSS };
    got[1] = s2.self_check().err() == Some((1, AB_ECROSS));

    // ③ 乱序（c 回到 4）
    let s3 = StructSpec { name: "BadOrder", size: 16, fields: &BAD_ORDER };
    got[2] = s3.self_check().err() == Some((2, AB_EORDER));

    // ④ 冻结后登记 + 冻结幂等
    let mut a = AbiAudit::standard();
    let root = a.freeze();
    let dup = StructSpec { name: "Elf64_Dyn", size: 16, fields: &DYN_FIELDS };
    got[3] = root.is_some()
        && a.freeze() == root
        && a.register(dup).err() == Some(AB_EFROZEN);
    got
}

/// 判据台：ABI 自检面的十六条判据。
pub fn run_unx11293_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11293");

    let (mut audit, value) = build_audited();

    // J1 八结构在册
    s.add(
        "J1-八结构在册",
        audit.len() == 8 && audit.find("Elf64_Rela").is_some(),
        "Ehdr/Phdr/Shdr/Sym/Dyn/Rel/Rela/Nhdr 八结构齐",
    );

    // J2 四十八字段在册
    s.add(
        "J2-四十八字段",
        audit.field_total() == 48,
        "14+8+10+6+2+2+3+3 = 48 字段逐项落表",
    );

    // J3 断言零违例
    s.add(
        "J3-零违例",
        audit.violations().is_empty() && value.is_some(),
        "标准表自检无任何违例",
    );

    // J4 Ehdr 尺寸与入口偏移
    let eh_check = match audit.find("Elf64_Ehdr") {
        Some(spec) => {
            spec.size == 64
                && spec.fields.iter().any(|f| f.name == "e_entry" && f.offset == 24)
                && spec.fields.iter().any(|f| f.name == "e_ident" && f.width == 16)
        }
        None => false,
    };
    s.add("J4-Ehdr 断言", eh_check, "Ehdr 64 字节、e_entry@24、e_ident 16 字节");

    // J5 Phdr 尺寸与对齐字段
    let ph_check = match audit.find("Elf64_Phdr") {
        Some(spec) => {
            spec.size == 56
                && spec.fields.iter().any(|f| f.name == "p_align" && f.offset == 48)
        }
        None => false,
    };
    s.add("J5-Phdr 断言", ph_check, "Phdr 56 字节、p_align@48");

    // J6 Shdr 尺寸与 entsize 偏移
    let sh_check = match audit.find("Elf64_Shdr") {
        Some(spec) => {
            spec.size == 64
                && spec.fields.iter().any(|f| f.name == "sh_entsize" && f.offset == 56)
        }
        None => false,
    };
    s.add("J6-Shdr 断言", sh_check, "Shdr 64 字节、sh_entsize@56");

    // J7 小结构尺寸族
    let small_ok = ["Elf64_Sym:24", "Elf64_Dyn:16", "Elf64_Rel:16", "Elf64_Rela:24", "Elf64_Nhdr:12"]
        .iter()
        .all(|tag| match tag.split_once(':') {
            Some((name, sz)) => match audit.find(name) {
                Some(spec) => match sz.parse::<u32>() {
                    Ok(v) => spec.size == v,
                    Err(_) => false,
                },
                None => false,
            },
            None => false,
        });
    s.add("J7-小结构尺寸", small_ok, "Sym24/Dyn16/Rel16/Rela24/Nhdr12 逐项对平");

    // J8 自检账指纹稳定
    let before = audit.scans;
    let first = audit.audit();
    let second = audit.audit();
    s.add(
        "J8-账指纹稳定",
        first.is_ok() && first == second && audit.scans == before.wrapping_add(2),
        "两轮自检取回同一账指纹",
    );

    // J9 重名结构拒
    let mut dup = AbiAudit::standard();
    let dyn_spec = StructSpec { name: "Elf64_Dyn", size: 16, fields: &DYN_FIELDS };
    s.add(
        "J9-重名拒",
        dup.register(dyn_spec).err() == Some(AB_EDUP) && dup.len() == 8,
        "同名结构二次登记拒",
    );

    // J10 空册自检拒
    let mut e = AbiAudit::empty();
    s.add(
        "J10-空册拒",
        e.audit().err() == Some(AB_EEMPTY) && e.field_total() == 0,
        "空册不得落账",
    );

    // J11 错位可定位
    static OFF_ALIGN: [FieldSpec; 2] = [
        FieldSpec { name: "lo", offset: 0, width: 8 },
        FieldSpec { name: "hi", offset: 12, width: 8 },
    ];
    let mut mis = AbiAudit::empty();
    let _ = mis.register(StructSpec { name: "Mis", size: 24, fields: &OFF_ALIGN });
    let v = mis.violations();
    s.add(
        "J11-错位可定位",
        v.len() == 1
            && v.first().map(|x| x.1.as_str()) == Some("hi")
            && v.first().map(|x| x.2) == Some(AB_EALIGN),
        "错位违例定位到结构+字段+码",
    );

    // J12 越界可定位
    static OFF_CROSS: [FieldSpec; 2] = [
        FieldSpec { name: "head", offset: 0, width: 8 },
        FieldSpec { name: "tail", offset: 16, width: 8 },
    ];
    let mut cross = AbiAudit::empty();
    let _ = cross.register(StructSpec { name: "Cross", size: 20, fields: &OFF_CROSS });
    s.add(
        "J12-越界可定位",
        cross.audit().err() == Some(AB_ECROSS),
        "字段越过结构末尾即拒",
    );

    // J13 乱序可定位
    static OFF_ORDER: [FieldSpec; 3] = [
        FieldSpec { name: "a", offset: 0, width: 4 },
        FieldSpec { name: "b", offset: 8, width: 4 },
        FieldSpec { name: "c", offset: 4, width: 4 },
    ];
    let mut order = AbiAudit::empty();
    let _ = order.register(StructSpec { name: "Order", size: 16, fields: &OFF_ORDER });
    s.add(
        "J13-乱序可定位",
        order.audit().err() == Some(AB_EORDER),
        "字段偏移回退即判乱序",
    );

    // J14 冻结幂等
    let root = audit.freeze();
    s.add(
        "J14-冻结幂等",
        root.is_some() && audit.freeze() == root && audit.is_frozen(),
        "重复冻结取回同一指纹",
    );

    // J15 冻结拒登记
    let late = StructSpec { name: "Late", size: 8, fields: &DYN_FIELDS };
    s.add(
        "J15-冻结拒登记",
        audit.register(late).err() == Some(AB_EFROZEN) && audit.len() == 8,
        "冻结后登记拒且册长不增",
    );

    // J16 注入四拒
    let hits = inject_four();
    let mut all = true;
    let mut i = 0usize;
    while i < hits.len() {
        if !hits[i] {
            all = false;
        }
        i = i.wrapping_add(1);
    }
    s.add("J16-注入四拒", all, "错位/越界/乱序/冻结四路全拒");

    // J17 动态明细
    s.add_dyn(
        "J17-自检明细",
        audit.violations().is_empty() && audit.rejects == 1,
        &format!(
            "structs={} fields={} scans={} rej={}",
            audit.len(),
            audit.field_total(),
            audit.scans,
            audit.rejects
        ),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11293_abi_flow() {
        let (mut a, value) = build_audited();
        assert_eq!(a.len(), 8);
        assert_eq!(a.field_total(), 48);
        assert!(a.violations().is_empty());
        assert!(value.is_some());

        let eh = match a.find("Elf64_Ehdr") {
            Some(v) => v.size,
            None => 0,
        };
        assert_eq!(eh, 64);
        assert!(a.find("Elf64_Phdr").map(|s| s.size) == Some(56));
        assert!(a.find("Elf64_Rela").map(|s| s.size) == Some(24));
        assert!(a.find("Elf64_Nhdr").map(|s| s.size) == Some(12));

        assert_eq!(a.audit(), a.audit());
        assert_eq!(a.register(ABI_TABLE[0]).err(), Some(AB_EDUP));

        assert!(a.freeze().is_some());
        assert_eq!(a.freeze(), a.freeze());
        assert_eq!(a.register(ABI_TABLE[1]).err(), Some(AB_EFROZEN));

        let mut e = AbiAudit::empty();
        assert_eq!(e.audit().err(), Some(AB_EEMPTY));

        assert!(inject_four().iter().all(|h| *h));
    }

    #[test]
    fn unx11293_checks_green() {
        let s = run_unx11293_checks();
        assert!(s.all_passed(), "UNX-F11293 判据未全绿");
    }
}
