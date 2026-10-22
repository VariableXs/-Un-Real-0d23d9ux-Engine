//! VE-F0460 · 组移交（IR 组）（VE-C 域 · 语义组→IR 组交接）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0460`
//!
//! **锚点原文**：「语义组向 IR 组移交包：衔接接口冻结文档（F0458）、类型系统
//! 映射表（语义类型到 IR 类型）、诊断条款引用迁移说明、经验包；IR 组已先行
//! 部分开发（fake 之上），移交包补记符合性三态核对；移交后接口变更走升版
//! 重核对。判据：四件套、三态核对、fake 衔接、升版重核、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **四件套**（判据一）。移交包恰四件（冻结文档 / 类型映射 / 条款迁移 /
//!    经验包），缺件即移交阻断（[`HandoverPack::missing`]）。
//!
//! 2. **三态核对**（判据二）。IR 组先行开发面逐接口三态核对（符合 / 偏差
//!    登记判定 / 待核显性），不允许隐式通过（[`Conformity::Pending`] 不算过）。
//!
//! 3. **fake 衔接**（判据三）。先行开发建立在 fake 之上——移交时逐项记录
//!    fake→真实现替换位（[`FakeSpot`]），替换位未清 = 先行债务显性。
//!
//! 4. **升版重核**（判据四）。接口版本变更 → 全部三态重置为待核（版本
//!    漂移不继承旧核对结论），重核完成才恢复移交态。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0460";

// ---------------------------------------------------------------------------
// 移交包四件套
// ---------------------------------------------------------------------------

/// 移交件（登记形态：件名 + 产出锚点 + 内容指纹占位）。
#[derive(Clone, Debug, PartialEq)]
pub struct HandoverItem {
    pub name: &'static str,
    /// 产出锚点（来源任务号，机检回溯）。
    pub from: &'static str,
}

/// 四件套正本。
pub static PACK_ITEMS: &[HandoverItem] = &[
    HandoverItem { name: "衔接接口冻结文档", from: "VE-F0458" },
    HandoverItem { name: "类型系统映射表", from: "VE-F0460" },
    HandoverItem { name: "诊断条款引用迁移说明", from: "VE-F0459" },
    HandoverItem { name: "经验包", from: "VE-F0459" },
];

/// 移交包：登记齐备性（缺件即阻断）。
#[derive(Debug, Default)]
pub struct HandoverPack {
    pub delivered: Vec<&'static str>,
}

impl HandoverPack {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn deliver(&mut self, name: &'static str) {
        if !self.delivered.contains(&name) {
            self.delivered.push(name);
        }
    }
    /// 缺件清单（对四件套正本差集）。
    pub fn missing(&self) -> Vec<&'static str> {
        PACK_ITEMS
            .iter()
            .filter(|i| !self.delivered.contains(&i.name))
            .map(|i| i.name)
            .collect()
    }
    pub fn complete(&self) -> bool {
        self.missing().is_empty()
    }
}

// ---------------------------------------------------------------------------
// 类型系统映射表（语义类型 → IR 类型）
// ---------------------------------------------------------------------------

/// IR 类型形态（移交映射的目标面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IrTy {
    F32,
    I32,
    U32,
    Bool,
    Vec2F,
    Vec3F,
    Vec4F,
    Mat4F,
    Sampler2D,
    Tex2D,
    /// 语义态无对应 IR 形态（映射缺失面显性）。
    Opaque,
}

impl IrTy {
    pub fn digest(&self) -> &'static str {
        match self {
            IrTy::F32 => "ir.f32",
            IrTy::I32 => "ir.i32",
            IrTy::U32 => "ir.u32",
            IrTy::Bool => "ir.b1",
            IrTy::Vec2F => "ir.v2f",
            IrTy::Vec3F => "ir.v3f",
            IrTy::Vec4F => "ir.v4f",
            IrTy::Mat4F => "ir.m4f",
            IrTy::Sampler2D => "ir.smp2d",
            IrTy::Tex2D => "ir.tex2d",
            IrTy::Opaque => "ir.opaque",
        }
    }
}

/// 类型映射表：语义摘要前缀 → IR 形态（一表移交）。
pub fn sem_ty_to_ir(digest: &str) -> IrTy {
    if digest.starts_with("s:float") {
        IrTy::F32
    } else if digest.starts_with("s:int") {
        IrTy::I32
    } else if digest.starts_with("s:uint") {
        IrTy::U32
    } else if digest.starts_with("s:bool") {
        IrTy::Bool
    } else if digest.starts_with("v:floatx2") {
        IrTy::Vec2F
    } else if digest.starts_with("v:floatx3") {
        IrTy::Vec3F
    } else if digest.starts_with("v:floatx4") {
        IrTy::Vec4F
    } else if digest.starts_with("m:floatx4x4") {
        IrTy::Mat4F
    } else if digest.starts_with("sm:") {
        IrTy::Sampler2D
    } else if digest.starts_with("im:") {
        IrTy::Tex2D
    } else {
        // 未知摘要：映射缺失显性（Opaque），不静默猜。
        IrTy::Opaque
    }
}

// ---------------------------------------------------------------------------
// 符合性三态核对
// ---------------------------------------------------------------------------

/// 三态：符合 / 偏差（带登记判定）/ 待核（显性，不算过）。
#[derive(Clone, Debug, PartialEq)]
pub enum Conformity {
    Conform,
    Deviated { verdict: String },
    Pending,
}

/// 逐接口核对表：接口名 + 三态 + fake 替换位记录。
#[derive(Clone, Debug, PartialEq)]
pub struct FakeSpot {
    pub iface: String,
    /// fake 上先行开发的摘要（替换债务显性）。
    pub fake_note: String,
}

#[derive(Debug, Default)]
pub struct ConformityBoard {
    pub entries: Vec<(String, Conformity)>,
    pub fake_spots: Vec<FakeSpot>,
    /// 核对基于的接口版本（升版重核基准）。
    pub audited_iface_ver: u32,
}

impl ConformityBoard {
    pub fn new(iface_ver: u32) -> Self {
        ConformityBoard {
            entries: Vec::new(),
            fake_spots: Vec::new(),
            audited_iface_ver: iface_ver,
        }
    }
    pub fn audit(&mut self, iface: &str, c: Conformity) {
        if let Some(e) = self.entries.iter_mut().find(|(n, _)| n == iface) {
            e.1 = c;
        } else {
            self.entries.push((iface.to_string(), c));
        }
    }
    /// 核对通过：无待核、偏差全部带判定。
    pub fn passed(&self) -> bool {
        self.entries.iter().all(|(_, c)| match c {
            Conformity::Conform => true,
            Conformity::Deviated { verdict } => !verdict.is_empty(),
            Conformity::Pending => false,
        })
    }
    pub fn pending_list(&self) -> Vec<String> {
        self.entries
            .iter()
            .filter(|(_, c)| matches!(c, Conformity::Pending))
            .map(|(n, _)| n.clone())
            .collect()
    }
    pub fn record_fake_spot(&mut self, iface: &str, note: &str) {
        self.fake_spots.push(FakeSpot {
            iface: iface.to_string(),
            fake_note: note.to_string(),
        });
    }
    /// 升版重核对：版本变更 → 全部三态重置待核 + 版本基准更新。
    pub fn on_iface_version_bump(&mut self, new_ver: u32) {
        if new_ver != self.audited_iface_ver {
            for e in self.entries.iter_mut() {
                e.1 = Conformity::Pending;
            }
            self.audited_iface_ver = new_ver;
        }
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn vec60_raw_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();

    // 一、四件套。
    {
        // 正本恰四件。
        v.push(("F0460-移交-四件套", PACK_ITEMS.len() == 4));
        // 缺件检测有牙：只交两件 → 缺两件可定位。
        let mut pack = HandoverPack::new();
        pack.deliver("衔接接口冻结文档");
        pack.deliver("经验包");
        let miss = pack.missing();
        v.push((
            "F0460-移交-缺件阻断",
            !pack.complete()
                && miss.contains(&"类型系统映射表")
                && miss.contains(&"诊断条款引用迁移说明"),
        ));
        // 四件齐 → 完整。
        for i in PACK_ITEMS.iter() {
            pack.deliver(i.name);
        }
        v.push(("F0460-移交-齐备放行", pack.complete()));
    }

    // 二、类型映射表。
    {
        // 语义摘要 → IR 形态逐族映射（标量/向量/矩阵/资源四族）。
        v.push((
            "F0460-映射-四族映射",
            sem_ty_to_ir("s:float") == IrTy::F32
                && sem_ty_to_ir("v:floatx3") == IrTy::Vec3F
                && sem_ty_to_ir("m:floatx4x4") == IrTy::Mat4F
                && sem_ty_to_ir("sm:sampler2d") == IrTy::Sampler2D,
        ));
        // 未知摘要 → Opaque 显性（不静默猜）。
        v.push((
            "F0460-映射-缺失显性",
            sem_ty_to_ir("x:unknown") == IrTy::Opaque
                && sem_ty_to_ir("x:unknown").digest() == "ir.opaque",
        ));
    }

    // 三、三态核对。
    {
        let mut board = ConformityBoard::new(1);
        board.audit("type_query", Conformity::Conform);
        board.audit("symbol_query", Conformity::Deviated { verdict: "偏差已登记：scope 语义按 F0442 裁定".to_string() });
        board.audit("const_query", Conformity::Pending);
        // 待核不算过（隐式通过被拒）。
        v.push(("F0460-核对-待核不算过", !board.passed()));
        // 偏差无判定也不算过。
        let mut b2 = ConformityBoard::new(1);
        b2.audit("type_query", Conformity::Deviated { verdict: String::new() });
        v.push(("F0460-核对-偏差须判定", !b2.passed()));
        // 待核清单可定位。
        board.audit("const_query", Conformity::Conform);
        v.push((
            "F0460-核对-清单定位",
            board.passed() && board.pending_list().is_empty(),
        ));
    }

    // 四、fake 衔接 + 升版重核。
    {
        let mut board = ConformityBoard::new(1);
        board.audit("type_query", Conformity::Conform);
        board.audit("const_query", Conformity::Conform);
        board.record_fake_spot("const_query", "fake 硬编码 MAX_LIGHTS=8，真实现走 F0448 缓存");
        // 先行债务显性：替换位可查。
        v.push((
            "F0460-fake-替换位显性",
            board.fake_spots.len() == 1 && board.fake_spots[0].iface == "const_query",
        ));
        // 升版 → 全部重置待核（旧结论不继承）。
        board.on_iface_version_bump(2);
        v.push((
            "F0460-升版-全部重核",
            !board.passed() && board.pending_list().len() == 2 && board.audited_iface_ver == 2,
        ));
        // 同版本调用不重置（幂等）。
        board.audit("type_query", Conformity::Conform);
        board.on_iface_version_bump(2);
        v.push(("F0460-升版-同版幂等", board.entries[0].1 == Conformity::Conform));
    }

    // 五、判据自检。
    {
        v.push(("F0460-判据-锚点入册", ANCHOR == "VE-F0460"));
    }

    v
}

/// 注册表入口：Vec 判据 → CheckSet（域标签 svstar2-vec60）。
pub fn run_vec60_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-vec60");
    for (name, ok) in vec60_raw_checks() {
        set.add(name, ok, "");
    }
    set
}

#[cfg(test)]
mod tests {
    use super::vec60_raw_checks as run_vec60_checks;
    #[test]
    fn vec60_handover_checks() {
        for (name, ok) in run_vec60_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}
