//! UNX-F0146 · 构建可复现性（固定时间戳/排序）· 诊断段 0x2550
//!
//! 判据原文（UNX-F0146-J1）：同输入两次构建产物逐字节一致（SOURCE_DATE_EPOCH
//! 语义：时间戳取固定值；输入排序后打包，遍历序不进产物）。
//!
//! 设计要点：可复现打包器（ReproPacker）——输入物料按「键名字典序 +
//! 等长稳定次键」排序；产物头注入固定纪元时间戳；两次同料打包
//! 逐字节比对账。不可复现因素闭集：遍历序 / 时间戳 / 临时路径——
//! 三者均被切断。零浮点、零 unwrap/expect。

use alloc::vec;
use crate::alloc::{format, string::{String, ToString}, vec::Vec};

pub const SEG_BASE: u16 = 0x2550;

const E_NAME: u16 = SEG_BASE + 1; // 物料名非法（空/重复）
const E_BAL: u16 = SEG_BASE + 2; // 复现账守恒被破坏
const E_LEN: u16 = SEG_BASE + 3; // 产物长度账不符
const E_SEAL: u16 = SEG_BASE + 4; // 打包器已冻结
const E_ORDER: u16 = SEG_BASE + 5; // 排序不变量违约

/// 固定纪元（SOURCE_DATE_EPOCH 语义，秒）。
pub const FIXED_EPOCH: u64 = 1_700_000_000;
/// 产物魔数 'VRPB'。
pub const PACK_MAGIC: u32 = 0x5652_5042;

/// 构建物料。
#[derive(Clone)]
pub struct Artifact {
    pub name: String,
    pub content: Vec<u8>,
}

/// 可复现产物头（12 字节）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct PackHeader {
    pub magic: u32,
    pub epoch: u64, // 固定纪元
}

impl PackHeader {
    pub fn serialize(&self) -> Vec<u8> {
        let mut b = Vec::with_capacity(12);
        b.extend_from_slice(&self.magic.to_le_bytes());
        b.extend_from_slice(&self.epoch.to_le_bytes());
        b
    }
}

/// 物料排序不变量：键名字典序；同名即装配错误（构造期拒）。
pub fn sort_artifacts(items: &mut Vec<Artifact>) -> Result<(), u16> {
    // 重名检测（排序前闭集校验）。
    let mut seen: Vec<&str> = Vec::new();
    for a in items.iter() {
        if a.name.is_empty() {
            return Err(E_NAME);
        }
        if seen.contains(&a.name.as_str()) {
            return Err(E_NAME);
        }
        seen.push(a.name.as_str());
    }
    items.sort_by(|x, y| x.name.cmp(&y.name));
    // 排序不变量复核：严格升序。
    for w in items.windows(2) {
        if w[0].name >= w[1].name {
            return Err(E_ORDER);
        }
    }
    Ok(())
}

/// 打包：固定头 + 排序物料（名长 + 名 + 内容长 + 内容），确定性遍历。
pub fn pack(items: &[Artifact]) -> Result<Vec<u8>, u16> {
    if items.is_empty() {
        return Err(E_LEN);
    }
    let mut out = Vec::new();
    let h = PackHeader {
        magic: PACK_MAGIC,
        epoch: FIXED_EPOCH,
    };
    out.extend_from_slice(&h.serialize());
    out.extend_from_slice(&(items.len() as u32).to_le_bytes());
    for a in items {
        out.extend_from_slice(&(a.name.len() as u16).to_le_bytes());
        out.extend_from_slice(a.name.as_bytes());
        out.extend_from_slice(&(a.content.len() as u32).to_le_bytes());
        out.extend_from_slice(&a.content);
    }
    Ok(out)
}

/// 可复现账本：两次打包逐字节比对记账。
pub struct ReproBook {
    pub builds: u32,
    pub byte_equal: bool,
    pub frozen: bool,
}

impl ReproBook {
    pub fn new() -> Result<Self, u16> {
        Ok(ReproBook {
            builds: 0,
            byte_equal: false,
            frozen: false,
        })
    }

    /// 两次构建复现验证：同料双包，逐字节等账。
    pub fn verify(&mut self, items: &[Artifact]) -> Result<(), u16> {
        if self.frozen {
            return Err(E_SEAL);
        }
        if self.builds != 0 {
            return Err(E_BAL); // 账面只记一对验证
        }
        let mut sorted = items.to_vec();
        sort_artifacts(&mut sorted)?;
        let p1 = pack(&sorted)?;
        // 第二次：故意打乱输入序——排序后应还原同一产物。
        let mut scrambled = items.to_vec();
        scrambled.reverse();
        sort_artifacts(&mut scrambled)?;
        let p2 = pack(&scrambled)?;
        self.byte_equal = p1 == p2;
        self.builds = 2;
        if !self.byte_equal {
            return Err(E_BAL);
        }
        Ok(())
    }

    pub fn audit(&self) -> Result<(), u16> {
        if self.builds == 0 && self.byte_equal {
            return Err(E_BAL); // 未验证却记等——矛盾
        }
        if self.builds == 2 && !self.byte_equal {
            return Err(E_BAL); // 已验证却不等——矛盾（verify 已拦，此处兜底）
        }
        Ok(())
    }

    pub fn freeze(&mut self) {
        self.frozen = true;
    }

    pub fn render(&self) -> String {
        format!(
            "复现账 构建对:{} 逐字节等:{} 冻结:{}",
            self.builds, self.byte_equal, self.frozen
        )
    }
}

// ── 确定性演练：合法 4 例 + 注入拦截 4 例 ─────────────────────────

/// 演练入口（ktest 挂点）。
pub fn drill() -> Result<(), u16> {
    let mut hits = 0u32;

    // L1：同料双包（第二次故意乱序输入）逐字节一致。
    let items = vec![
        Artifact { name: "kernel.elf".to_string(), content: vec![1, 2, 3] },
        Artifact { name: "boot.cfg".to_string(), content: vec![9] },
        Artifact { name: "initrd.img".to_string(), content: vec![4, 5] },
    ];
    let mut b = ReproBook::new()?;
    b.verify(&items)?;
    b.audit()?;
    hits += 1;

    // L2：产物头固定纪元——两次打包头一致。
    let mut sorted = items.clone();
    sort_artifacts(&mut sorted)?;
    let p1 = pack(&sorted)?;
    let h = PackHeader {
        magic: PACK_MAGIC,
        epoch: FIXED_EPOCH,
    };
    if p1[..12] != h.serialize()[..] {
        return Err(E_BAL);
    }
    hits += 1;

    // L3：排序不变量——乱序输入排序后字典序升序。
    let names: Vec<&str> = sorted.iter().map(|a| a.name.as_str()).collect();
    if names != vec!["boot.cfg", "initrd.img", "kernel.elf"] {
        return Err(E_ORDER);
    }
    hits += 1;

    // L4：渲染账面。
    let _ = b.render();
    hits += 1;

    // I1：重名物料构造即拒。
    let dup = vec![
        Artifact { name: "a".to_string(), content: vec![1] },
        Artifact { name: "a".to_string(), content: vec![2] },
    ];
    let mut v = dup.clone();
    if sort_artifacts(&mut v) != Err(E_NAME) {
        return Err(E_NAME);
    }
    hits += 1;

    // I2：空名物料拒。
    let mut v2 = vec![Artifact { name: String::new(), content: vec![] }];
    if sort_artifacts(&mut v2) != Err(E_NAME) {
        return Err(E_NAME);
    }
    hits += 1;

    // I3：空料打包拒。
    if pack(&[]) != Err(E_LEN) {
        return Err(E_LEN);
    }
    hits += 1;

    // I4：复现账注入——手工把 byte_equal 改假，audit 必抓红。
    let mut i4 = ReproBook::new()?;
    i4.verify(&items)?;
    i4.byte_equal = false; // 破账
    if i4.audit() != Err(E_BAL) {
        return Err(E_BAL);
    }
    hits += 1;

    // I5：冻结拒验 + 二次验证拒。
    let mut i5 = ReproBook::new()?;
    i5.verify(&items)?;
    if i5.verify(&items) != Err(E_BAL) {
        return Err(E_BAL);
    }
    i5.freeze();
    if i5.verify(&items) != Err(E_SEAL) {
        return Err(E_SEAL);
    }
    hits += 1;

    let _ = hits;
    Ok(())
}
