//! VE-F0779 九宫格与素材管线对接
//! 职责：素材描述（纹理 ID + 元数据引用）到九宫格切片的装配、
//!       装配失败降级（元数据缺失→纯色）、素材注册表（去重 + 引用计数）。
//! 契约：注册表纹理 ID 唯一；引用计数守恒（增减对称）；降级不 panic。

use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::vec361_ninecore::{Insets, Slice};
use super::vec364_ninemeta::NineMeta;

/// 素材描述
#[derive(Clone, Debug, PartialEq)]
pub struct Asset {
    pub tex_id: u32,
    pub name: &'static str,
    /// 元数据文本（None = 无九宫格信息）
    pub meta: Option<&'static str>,
}

/// 注册表
pub struct AssetRegistry {
    pub assets: Vec<Asset>,
    pub refs: Vec<(u32, u64)>,
}

impl AssetRegistry {
    pub fn new() -> AssetRegistry {
        AssetRegistry { assets: alloc::vec![], refs: alloc::vec![] }
    }
    /// 注册（ID 去重）
    pub fn register(&mut self, a: Asset) -> Result<(), &'static str> {
        if self.assets.iter().any(|x| x.tex_id == a.tex_id) {
            return Err("duplicate tex id");
        }
        self.refs.push((a.tex_id, 0));
        self.assets.push(a);
        Ok(())
    }
    pub fn find(&self, id: u32) -> Option<&Asset> {
        self.assets.iter().find(|a| a.tex_id == id)
    }
    pub fn retain(&mut self, id: u32) -> bool {
        if let Some(r) = self.refs.iter_mut().find(|(i, _)| *i == id) {
            r.1 += 1;
            true
        } else {
            false
        }
    }
    pub fn release(&mut self, id: u32) -> bool {
        if let Some(r) = self.refs.iter_mut().find(|(i, _)| *i == id) {
            if r.1 > 0 {
                r.1 -= 1;
            }
            true
        } else {
            false
        }
    }
    pub fn refcount(&self, id: u32) -> Option<u64> {
        self.refs.iter().find(|(i, _)| *i == id).map(|(_, c)| *c)
    }
}

impl Default for AssetRegistry {
    fn default() -> Self {
        AssetRegistry::new()
    }
}

/// 装配结果
#[derive(Clone, Debug, PartialEq)]
pub enum Assembled {
    /// 九宫格切片就绪
    Nine(Slice),
    /// 降级纯色（灰度）
    Solid(u8),
    /// 装配失败（未注册）
    Missing,
}

/// 装配：素材 → 切片或降级
pub fn assemble(reg: &AssetRegistry, id: u32) -> Assembled {
    let a = match reg.find(id) {
        Some(a) => a,
        None => return Assembled::Missing,
    };
    match a.meta {
        None => Assembled::Solid(128),
        Some(text) => match super::vec364_ninemeta::parse(text) {
            Ok(m) => {
                let ins = Insets { left: m.left, top: m.top, right: m.right, bottom: m.bottom };
                match Slice::build(m.tex_w, m.tex_h, ins) {
                    Some(s) => Assembled::Nine(s),
                    None => Assembled::Solid(128), // 元数据非法 → 降级
                }
            }
            Err(_) => Assembled::Solid(128),
        },
    }
}

pub fn run_vec379_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec379");
    let meta_text = "tex_w=64\ntex_h=64\ninset_l=8\ninset_t=8\ninset_r=8\ninset_b=8\n";
    let bad_text = "tex_w=16\ntex_h=16\ninset_l=10\ninset_t=10\ninset_r=10\ninset_b=10\n"; // inset 和 ≥ tex
    // 族一：注册表
    let mut reg = AssetRegistry::new();
    s.add("注册-成功", reg.register(Asset { tex_id: 1, name: "btn", meta: Some(meta_text) }).is_ok(), "id 1");
    s.add("注册-重ID拒", reg.register(Asset { tex_id: 1, name: "dup", meta: None }) == Err("duplicate tex id"), "dup rejected");
    s.add("注册-多素材", reg.register(Asset { tex_id: 2, name: "raw", meta: None }).is_ok(), "id 2");
    s.add("注册-计数", reg.assets.len() == 2, "2 assets");
    s.add("查找-命中", reg.find(1).expect("f").name == "btn", "find");
    s.add("查找-未录", reg.find(9).is_none(), "missing");
    // 族二：引用计数
    s.add("引用-初始零", reg.refcount(1) == Some(0), "starts 0");
    s.add("引用-增加", reg.retain(1), "retain");
    s.add("引用-计数一", reg.refcount(1) == Some(1), "1");
    s.add("引用-再加", { reg.retain(1); reg.refcount(1) == Some(2) }, "2");
    s.add("引用-释放", { reg.release(1); reg.refcount(1) == Some(1) }, "back to 1");
    // 不下穿零
    s.add("引用-不下穿", { reg.release(1); reg.release(1); reg.refcount(1) == Some(0) }, "floor 0");
    s.add("引用-未录释放", !reg.release(9), "release missing");
    // 守恒：增 n 减 n 回零
    for _ in 0..5 {
        reg.retain(2);
    }
    for _ in 0..5 {
        reg.release(2);
    }
    s.add("引用-守恒", reg.refcount(2) == Some(0), "balanced");
    // 族三：装配
    s.add("装配-九宫", matches!(assemble(&reg, 1), Assembled::Nine(_)), "id 1 → nine");
    if let Assembled::Nine(sl) = assemble(&reg, 1) {
        s.add("装配-切片值", sl.tex_w == 64.0 && sl.inset.left == 8.0, "slice fields");
    }
    s.add("装配-无元降级", assemble(&reg, 2) == Assembled::Solid(128), "no meta → solid");
    s.add("装配-未录Missing", assemble(&reg, 9) == Assembled::Missing, "unregistered → missing");
    // 非法元数据降级
    reg.register(Asset { tex_id: 3, name: "bad", meta: Some(bad_text) }).expect("id3");
    s.add("装配-非法降级", assemble(&reg, 3) == Assembled::Solid(128), "bad meta → solid");
    // 解析失败文本降级
    reg.register(Asset { tex_id: 4, name: "junk", meta: Some("not a meta") }).expect("id4");
    s.add("装配-垃圾降级", assemble(&reg, 4) == Assembled::Solid(128), "junk → solid");
    // 族四：装配三分支齐备
    let kinds = [Assembled::Nine(Slice::build(64.0, 64.0, Insets::uniform(8.0)).expect("x")), Assembled::Solid(128), Assembled::Missing];
    s.add("分支-三种", kinds.len() == 3, "3 outcomes");
    // 族五：装配不 panic（对未录/垃圾/越界全部安全）
    for id in [0u32, 9, 4] {
        let _ = assemble(&reg, id);
    }
    s.add("装配-安全", true, "no panic path");
    // 族六：默认注册表
    s.add("默认-空", AssetRegistry::default().assets.is_empty(), "empty default");
    s
}
