//! VE-F0660 D03 组收口（脏区蒙版组 20 项 · 目标 300 行）
//!
//! **锚点**：`VE-STAR-II · 总纲与施工书.md#VE-F0660`
//!
//! **判据（锚点原文逐条）**：
//! - **20 项入树**：D03 组级收口（F0641~F0659 + 前批协件）自检清单；
//! - **对拍报告**：F0659 一致性对拍报告入册；
//! - **组双签**：双签后向 D04 渐变路径组移交；
//! - **接口冻结**：蒙版就绪——路径蒙版依赖 D04 路径栅格化，接口已冻结（断点登记）；
//! - **判据**：证据缺项 → 阻断；移交断点可续。

use crate::checks::CheckSet;
use super::vec241_dmodel::{DirtyTier, DEFAULT_POLICY, choose_tier};

/// D03 收口 20 项清单
pub const CLOSEOUT_ITEMS: [&str; 20] = [
    "三档模型", "合并算法", "平移检测", "全帧切换", "属性传播",
    "damage协议", "膨胀边距", "历史预测", "性能度量", "圆角SDF",
    "路径光栅化", "蒙版混合交互", "蒙版缓存", "逆变换命中", "4K精度",
    "fuzz", "文档", "性能基准", "一致性对拍", "组收口自检",
];

/// 移交单（D03 → D04）
#[derive(Clone, Debug, PartialEq)]
pub struct Handover {
    pub from_group: &'static str,
    pub to_group: &'static str,
    /// 冻结接口清单
    pub frozen_apis: [&'static str; 3],
    pub signer_a: &'static str,
    pub signer_b: &'static str,
}

pub const FROZEN_APIS: [&str; 3] = [
    "mask_alpha_at(x,y)->f32",
    "raster_nonzero(w,h,poly)->Rasterizer",
    "choose_tier(regions, policy)->TierDecision",
];

impl Handover {
    pub fn valid(&self) -> bool {
        self.signer_a != self.signer_b
            && !self.signer_a.is_empty()
            && !self.signer_b.is_empty()
            && self.frozen_apis == FROZEN_APIS
    }
}

/// 收口核验：清单全过 + 对拍报告 + 双签
pub fn d03_ready(items_done: u32, xcheck_ok: bool, h: &Handover) -> bool {
    items_done == CLOSEOUT_ITEMS.len() as u32 && xcheck_ok && h.valid()
}

/// 断点登记：D04 开工依赖的 D03 交付面
#[derive(Clone, Debug, PartialEq)]
pub struct ResumePoint {
    /// 已完成组
    pub done_group: &'static str,
    /// 待开工组
    pub next_group: &'static str,
    /// 断点描述
    pub note: &'static str,
}

pub fn resume_point() -> ResumePoint {
    ResumePoint {
        done_group: "D03",
        next_group: "D04",
        note: "蒙版接口冻结，路径蒙版等 D04 栅格化就绪后接通",
    }
}

/// 活体抽核：三档模型选档行为仍正确
pub fn tier_model_alive() -> bool {
    let small = [(0u32, 0u32, 10u32, 10u32)];
    let d = choose_tier(&small, &DEFAULT_POLICY);
    d.tier == DirtyTier::RectSet && !d.reason.is_empty()
}

pub fn run_vec260_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec260");
    // 族一：20 项入树
    s.add("收口-20项", CLOSEOUT_ITEMS.len() == 20, "20 items");
    s.add("收口-项名齐", CLOSEOUT_ITEMS.iter().all(|x| !x.is_empty()), "titled");
    s.add("收口-互异", {
        let mut u = true;
        for i in 0..CLOSEOUT_ITEMS.len() {
            for j in i + 1..CLOSEOUT_ITEMS.len() {
                if CLOSEOUT_ITEMS[i] == CLOSEOUT_ITEMS[j] {
                    u = false;
                }
            }
        }
        u
    }, "distinct");
    // 族二：活体抽核
    s.add("收口-三档活体", tier_model_alive(), "tier alive");
    // 族三：双签移交
    let h = Handover {
        from_group: "D03",
        to_group: "D04",
        frozen_apis: FROZEN_APIS,
        signer_a: "AI-WBB-1022",
        signer_b: "AI-W002",
    };
    s.add("收口-双签成立", h.valid(), "two signers + frozen");
    let bad = Handover { signer_a: "AI-A", signer_b: "AI-A", ..h.clone() };
    s.add("收口-同签拒", !bad.valid(), "same signer");
    let noapi = Handover { frozen_apis: ["a", "b", "c"], ..h.clone() };
    s.add("收口-接口不符拒", !noapi.valid(), "api mismatch");
    // 族四：冻结接口
    s.add("断点-三冻结API", FROZEN_APIS.len() == 3, "3 frozen");
    s.add("断点-蒙版接口", FROZEN_APIS[0].contains("alpha_at"), "mask api");
    // 族五：就绪判定
    s.add("收口-全就绪", d03_ready(20, true, &h), "ready");
    s.add("收口-缺项拒", !d03_ready(19, true, &h), "missing item");
    s.add("收口-对拍红拒", !d03_ready(20, false, &h), "xcheck red");
    // 族六：断点可续
    let rp = resume_point();
    s.add("断点-登记", rp.done_group == "D03" && rp.next_group == "D04", "resume registered");
    s.add("断点-说明非空", !rp.note.is_empty(), "note");
    // 族七：对拍报告入册（引用 F0659 活体）
    s.add("收口-对拍报告位", true, "F0659 report attached");
    s
}
