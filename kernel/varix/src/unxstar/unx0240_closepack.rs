//! unx0240_closepack —— UNX-F0240 波 01 A1 侧闭账物料包。
//! 锚点（CoRun Unxreal 主册）：UNX-F0240 · 闭账物料包 · 目标 260 行
//! 判据原文：UNX-F0240-J1 波 01 闭账包五节齐备：快照（F0234）/
//!           自查（F0235）/ 反馈（F0236）/ 对照（F0237）/ 清点
//!           （F0239）五节各带内容签；包级校验和复算过；缺节
//!           或签漂即拒包——闭账包必须自洽可复跑。
//! 组件：五节信封（节号 + 内容签 + 词位体）+ 物料包（节集 + 包级
//!       FNV + 完整性闸）+ 反序列化复跑（五节逐签复算）。
//! 职责定位：闭账包是「域交付的集装箱」——五个上游单的产出各装
//!   一节，每节自带内容签（节被改 = 签漂 = 当场拒），包级签罩
//!   全部节签（节签清单被换 = 包签漂）。三层签（体签/节签/包签）
//!   保证任何一个字被改都能定位到改动层。
//! 性能逐项分解：装包 O(节体长)；验包 O(节体长)；闸 O(1)。
//! 诊断码段：0x20180（0x20181~0x20189 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const CP_VERSION: &str = "unx0240_closepack-v1";

/// 诊断段基址（0x20180 段 · 本模块独占）。
pub const CP_SEG_BASE: u32 = 0x20180;
pub const XC_BAD_SECTION: u32 = 0x20181; // 节号越域
pub const XC_SIG_DRIFT: u32 = 0x20182; // 节签漂
pub const XC_PACK_SIG: u32 = 0x20183; // 包签漂
pub const XC_MISS_SECTION: u32 = 0x20184; // 缺节
pub const XC_DUP_SECTION: u32 = 0x20185; // 重节
pub const XC_EMPTY_BODY: u32 = 0x20186; // 空节体拒
pub const XC_EMPTY_PACK: u32 = 0x20187; // 空包拒
pub const XC_MAGIC: u32 = 0x20188; // 包魔数破
pub const XC_LEN_BAD: u32 = 0x20189; // 包长不合

/// 五节闭集编号。
pub const S_SNAP: u32 = 1; // 快照节（F0234）
pub const S_AUDIT: u32 = 2; // 自查节（F0235）
pub const S_FEEDBACK: u32 = 3; // 反馈节（F0236）
pub const S_DOCDIFF: u32 = 4; // 对照节（F0237）
pub const S_TALLY: u32 = 5; // 清点节（F0239）

/// FNV-1a 64 位。
pub fn fnv64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 节信封：节号 + 节体词位 + 节签（FNV(节号||体)）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Section {
    pub id: u32,
    pub body: Vec<u32>,
    sig: u64,
}

impl Section {
    /// 构造：节号闭集 + 空体拒；签在构造时冻结。
    pub fn new(id: u32, body: &[u32]) -> Result<Section, u32> {
        if id < S_SNAP || id > S_TALLY {
            return Err(XC_BAD_SECTION);
        }
        if body.is_empty() {
            return Err(XC_EMPTY_BODY);
        }
        let sig = section_sig(id, body);
        Ok(Section { id, body: body.to_vec(), sig })
    }

    /// 节签。
    pub fn sig(&self) -> u64 {
        self.sig
    }

    /// 节签复算核（被改 = 漂）。
    pub fn sig_ok(&self) -> bool {
        section_sig(self.id, &self.body) == self.sig
    }
}

/// 节签 = FNV(节号 le || 逐词 le)。
pub fn section_sig(id: u32, body: &[u32]) -> u64 {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&id.to_le_bytes());
    for w in body.iter() {
        bytes.extend_from_slice(&w.to_le_bytes());
    }
    fnv64(&bytes)
}

/// 物料包：五节 + 包级签（FNV(逐节签)）。
pub struct ClosePack {
    sections: Vec<Section>,
    pack_sig: u64,
}

impl ClosePack {
    /// 装包：五节齐 + 零重才可装（缺节/重节在源头拦）。
    pub fn new(sections: &[Section]) -> Result<ClosePack, u32> {
        if sections.is_empty() {
            return Err(XC_EMPTY_PACK);
        }
        for s in sections.iter() {
            if sections.iter().filter(|x| x.id == s.id).count() > 1 {
                return Err(XC_DUP_SECTION);
            }
        }
        // 五节齐备闸。
        for id in S_SNAP..=S_TALLY {
            if !sections.iter().any(|s| s.id == id) {
                return Err(XC_MISS_SECTION);
            }
        }
        let mut bytes = Vec::new();
        for s in sections.iter() {
            bytes.extend_from_slice(&s.sig.to_le_bytes());
        }
        Ok(ClosePack { sections: sections.to_vec(), pack_sig: fnv64(&bytes) })
    }

    /// 包签。
    pub fn pack_sig(&self) -> u64 {
        self.pack_sig
    }

    /// 完整性闸：逐节签复算 + 包签复算，双过才闭账。
    pub fn verify(&self) -> Result<(), u32> {
        let mut bytes = Vec::new();
        for s in self.sections.iter() {
            if !s.sig_ok() {
                return Err(XC_SIG_DRIFT);
            }
            bytes.extend_from_slice(&s.sig.to_le_bytes());
        }
        if fnv64(&bytes) != self.pack_sig {
            return Err(XC_PACK_SIG);
        }
        Ok(())
    }

    /// 序列化：魔数 + 节数 + (节号+体长+体)+… + 包签低 32。
    pub fn serialize(&self) -> Vec<u32> {
        let mut words = vec![0x434C504Bu32, self.sections.len() as u32]; // "CLPB"
        for s in self.sections.iter() {
            words.push(s.id);
            words.push(s.body.len() as u32);
            words.extend_from_slice(&s.body);
        }
        words.push(self.pack_sig as u32);
        words
    }

    /// 反序列化 + 全签复算（复跑入口）。
    pub fn deserialize(words: &[u32]) -> Result<ClosePack, u32> {
        if words.len() < 3 || words[0] != 0x434C504B {
            return Err(XC_MAGIC);
        }
        let n = words[1] as usize;
        let mut i = 2usize;
        let mut sections = Vec::new();
        for _ in 0..n {
            if i + 2 > words.len() {
                return Err(XC_LEN_BAD);
            }
            let id = words[i];
            let bl = words[i + 1] as usize;
            if i + 2 + bl > words.len() {
                return Err(XC_LEN_BAD);
            }
            sections.push(Section::new(id, &words[i + 2..i + 2 + bl])?);
            i += 2 + bl;
        }
        if i + 1 != words.len() {
            return Err(XC_LEN_BAD);
        }
        let pack = ClosePack::new(&sections)?;
        if pack.pack_sig as u32 != words[words.len() - 1] {
            return Err(XC_PACK_SIG);
        }
        Ok(pack)
    }
}

pub fn run_unx0240_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0240_closepack");
    // 族一：五节装包。
    let secs = vec![
        Section::new(S_SNAP, &[11, 22, 33]).unwrap_or(Section { id: S_SNAP, body: vec![11], sig: 0 }),
        Section::new(S_AUDIT, &[44, 55]).unwrap_or(Section { id: S_AUDIT, body: vec![44], sig: 0 }),
        Section::new(S_FEEDBACK, &[66]).unwrap_or(Section { id: S_FEEDBACK, body: vec![66], sig: 0 }),
        Section::new(S_DOCDIFF, &[77, 88, 99]).unwrap_or(Section { id: S_DOCDIFF, body: vec![77], sig: 0 }),
        Section::new(S_TALLY, &[1010]).unwrap_or(Section { id: S_TALLY, body: vec![1010], sig: 0 }),
    ];
    let pack = match ClosePack::new(&secs) {
        Ok(p) => p,
        Err(_) => {
            cs.add("装包", false, "被拒");
            return cs;
        }
    };
    cs.add("装包过", pack.verify().is_ok(), "三层签全过");
    // 族二：节签敏感。
    let mut tsec = secs.clone();
    tsec[0].body[0] ^= 1;
    let tpack = ClosePack::new(&tsec).unwrap_or(ClosePack { sections: alloc::vec::Vec::new(), pack_sig: 0 });
    cs.add("体改签漂", matches!(tpack.verify(), Err(XC_SIG_DRIFT)), "节签先拦");
    // 族三：包签敏感（节签全对但包签旧——节集被换装）。
    let swap = vec![
        Section::new(S_SNAP, &[11, 22, 33]).unwrap_or(Section { id: S_SNAP, body: vec![11], sig: 0 }),
        Section::new(S_AUDIT, &[44, 55]).unwrap_or(Section { id: S_AUDIT, body: vec![44], sig: 0 }),
        Section::new(S_FEEDBACK, &[66]).unwrap_or(Section { id: S_FEEDBACK, body: vec![66], sig: 0 }),
        Section::new(S_DOCDIFF, &[77, 88, 99]).unwrap_or(Section { id: S_DOCDIFF, body: vec![77], sig: 0 }),
        Section::new(S_TALLY, &[9999]).unwrap_or(Section { id: S_TALLY, body: vec![9999], sig: 0 }),
    ];
    let _ = &swap; // 清点节体不同 → 节签不同 → 包签必不同（由构造保证）。
    cs.add("包签罩全节", ClosePack::new(&swap).map(|p| p.pack_sig()) != Ok(pack.pack_sig()),
           "任一节体变 → 包签变");
    // 族四：缺节/重节/空体拒。
    let four = vec![
        Section::new(S_SNAP, &[1]).unwrap_or(Section { id: S_SNAP, body: vec![1], sig: 0 }),
        Section::new(S_AUDIT, &[1]).unwrap_or(Section { id: S_AUDIT, body: vec![1], sig: 0 }),
        Section::new(S_FEEDBACK, &[1]).unwrap_or(Section { id: S_FEEDBACK, body: vec![1], sig: 0 }),
        Section::new(S_DOCDIFF, &[1]).unwrap_or(Section { id: S_DOCDIFF, body: vec![1], sig: 0 }),
    ];
    cs.add("缺清点节拒", ClosePack::new(&four).is_err(), "MISS_SECTION");
    let dup5 = vec![
        Section::new(S_SNAP, &[1]).unwrap_or(Section { id: S_SNAP, body: vec![1], sig: 0 }),
        Section::new(S_SNAP, &[2]).unwrap_or(Section { id: S_SNAP, body: vec![2], sig: 0 }),
        Section::new(S_AUDIT, &[1]).unwrap_or(Section { id: S_AUDIT, body: vec![1], sig: 0 }),
        Section::new(S_FEEDBACK, &[1]).unwrap_or(Section { id: S_FEEDBACK, body: vec![1], sig: 0 }),
        Section::new(S_TALLY, &[1]).unwrap_or(Section { id: S_TALLY, body: vec![1], sig: 0 }),
    ];
    cs.add("重节拒", ClosePack::new(&dup5).is_err(), "DUP_SECTION");
    cs.add("空体拒", Section::new(S_SNAP, &[]).is_err(), "EMPTY_BODY");
    cs.add("节号越域拒", Section::new(6, &[1]).is_err(), "BAD_SECTION");
    cs.add("空包拒", ClosePack::new(&[]).is_err(), "EMPTY_PACK");
    // 族五：序列化复跑。
    let words = pack.serialize();
    cs.add("往返闭账", match ClosePack::deserialize(&words) {
        Ok(p) => p.pack_sig() == pack.pack_sig() && p.verify().is_ok(),
        Err(_) => false,
    }, "包签一致+全签复算");
    // 族六：包级篡改检出。
    let mut b1 = words.clone();
    let last = b1.len() - 1;
    b1[last] = b1[last].wrapping_add(1);
    cs.add("包签词拒", matches!(ClosePack::deserialize(&b1), Err(XC_PACK_SIG)), "尾词=包签");
    let mut b2 = words.clone();
    b2[0] = 0x41414141;
    cs.add("魔数拒", matches!(ClosePack::deserialize(&b2), Err(XC_MAGIC)), "MAGIC");
    let mut b3 = words.clone();
    b3[2] = b3[2].wrapping_add(1); // 改首节号 → 节体重建后签漂
    cs.add("节号拒", ClosePack::deserialize(&b3).is_err(), "节签或包签拦");
    cs
}
