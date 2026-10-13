//! unx2181_regbook —— UNX-F2181 回归主账域级扩容（38→114 场景）。
//! 锚点（CoRun Unxreal 主册）：UNX-F2181 · 回归主账域级扩容（38→114 场景） · 目标 360 行 · 已深化
//! 判据原文：UNX-F2181-J1 主账扩容全过（首轮 38 场景 + B16–B29 十三批预告
//!           102 正路径 + 联测 12 = 114 场景执行账结构落账），
//!           五列格式与 F1885 一致，旧账零改动
//! 组件：首轮账（38 场景 · 旧账零改动：ID/判据/状态 原样保留 · 指纹核对）+
//!      十三批预告账（B16–B28 批号连续 · 每批预告登记 · 合计新增 64 条）+
//!      正路径主账（38 首轮 + 64 预告转正 = 102 条）+
//!      联测账（B29 批 · 12 条交叉场景）+
//!      执行账总闸（102 正路径 + 12 联测 = 114 · 总数强断言）+
//!      五列格式核（ID/批号/册别/判据锚/状态 · 与 F1885 一致 · 逐条核）+
//!      注入核（批号断裂/预告漏登/旧账改写/总数漂移 四路必抓）+
//!      演练（扩容全流程 · 114 场景落账 · 结构自洽）。
//! 职责定位：回归主账扩容是"加法不变式"——首轮 38 条是已验收的旧账，
//!   扩容只许追加不许改写（旧账零改动是指纹级约束）；
//!   十三批预告是意图账（预告了才许转正），转正后正路径主账 102 条；
//!   联测 12 条独立成册（交叉场景不走正路径口径）；
//!   执行账 114 = 102 + 12 是扩容后的总闸，任何一条漂移即总闸红。
//! 性能逐项分解：登记 O(1)；转正 O(1)；指纹核 O(38)；总闸 O(114)；
//!   演练 O(扩容流程) 确定性；全整数零墙钟。
//! 诊断码段：0x7880（0x7881~0x7888 闭集；全域 grep 确认独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 整数算术，零墙钟。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const RB_VERSION: &str = "unx2181_regbook-v1";

/// 诊断段基址（0x7880 段 · 本模块独占）。
pub const RB_SEG_BASE: u16 = 0x7880;

/// 首轮场景数（旧账冻结值）。
pub const RB_FIRST_ROUND: u32 = 38;
/// 预告批数（B16–B28 · 十三批）。
pub const RB_PROMO_BATCHES: u32 = 13;
/// 预告新增正路径合计（转正后 38+64=102）。
pub const RB_PROMO_NEW: u32 = 64;
/// 联测场景数（B29 批）。
pub const RB_JOINT: u32 = 12;
/// 执行账总闸（102 + 12 = 114）。
pub const RB_TOTAL: u32 = 114;

/// 诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RbCode {
    /// 0x7881 旧账改写（首轮 38 条指纹变）。
    OldRewrite = 0x7881,
    /// 0x7882 批号断裂（B16–B28 不连续）。
    BatchGap = 0x7882,
    /// 0x7883 预告漏登（预告条数 ≠ 64）。
    PromoMiss = 0x7883,
    /// 0x7884 未预告转正（转正项无预告对）。
    NoPromoBase = 0x7884,
    /// 0x7885 五列缺列（格式与 F1885 不一致）。
    ColumnShort = 0x7885,
    /// 0x7886 联测缺位（B29 ≠ 12 条）。
    JointShort = 0x7886,
    /// 0x7887 总数漂移（执行账 ≠ 114）。
    TotalDrift = 0x7887,
    /// 0x7888 册别错位（联测混入正路径册）。
    BookMix = 0x7888,
}

impl RbCode {
    /// 码值。
    pub fn num(self) -> u16 {
        self as u16
    }

    /// 中文短句。
    pub fn text(self) -> &'static str {
        match self {
            RbCode::OldRewrite => "旧账改写（首轮 38 条指纹变）",
            RbCode::BatchGap => "批号断裂（B16–B28 不连续）",
            RbCode::PromoMiss => "预告漏登（新增 ≠ 64 条）",
            RbCode::NoPromoBase => "未预告转正（转正项无预告对）",
            RbCode::ColumnShort => "五列缺列（F1885 格式破）",
            RbCode::JointShort => "联测缺位（B29 ≠ 12 条）",
            RbCode::TotalDrift => "总数漂移（执行账 ≠ 114）",
            RbCode::BookMix => "册别错位（联测混入正路径册）",
        }
    }

    /// 可操作建议。
    pub fn suggestion(self) -> &'static str {
        match self {
            RbCode::OldRewrite => "扩容只追加：首轮段以只读指纹保护",
            RbCode::BatchGap => "批号 B16 起连续递增",
            RbCode::PromoMiss => "预告按批全量登记",
            RbCode::NoPromoBase => "转正前核预告账有此条",
            RbCode::ColumnShort => "五列齐填：ID/批号/册别/判据锚/状态",
            RbCode::JointShort => "联测批恰好 12 条",
            RbCode::TotalDrift => "执行账总数以 RB_TOTAL 冻结值断言",
            RbCode::BookMix => "联测独立成册，册别列不得混",
        }
    }
}

/// 诊断体。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RbDiag {
    /// 码。
    pub code: RbCode,
    /// 位置。
    pub site: String,
    /// 细节。
    pub detail: String,
}

impl RbDiag {
    /// 新诊断。
    pub fn new(code: RbCode, site: &str, detail: String) -> Self {
        RbDiag {
            code,
            site: site.to_string(),
            detail,
        }
    }

    /// 单行全文。
    pub fn full(&self) -> String {
        format!(
            "[0x{:04x}] {} @{}: {}（建议：{}）",
            self.code.num(),
            self.code.text(),
            self.site,
            self.detail,
            self.code.suggestion()
        )
    }
}

/// 场景条目（五列 · F1885 格式）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RbScene {
    /// 列一：ID。
    pub id: u32,
    /// 列二：批号（首轮 = 0；预告批 B16..=B28 → 16..=28；联测批 = 29）。
    pub batch: u16,
    /// 列三：册别（0 正路径 / 1 联测）。
    pub book: u8,
    /// 列四：判据锚。
    pub anchor: String,
    /// 列五：状态（0 预告 / 1 转正执行）。
    pub state: u8,
}

/// 主账（扩容器：首轮段只读 + 预告段 + 联测段）。
pub struct RbBook {
    /// 场景账（ID 升序）。
    pub scenes: Vec<RbScene>,
    /// 首轮段指纹（扩容前快照）。
    pub first_fp: u64,
    /// 下一 ID。
    pub next_id: u32,
}

impl RbBook {
    /// 新主账：首轮 38 场景落账（旧账基准）。
    pub fn new_first_round() -> Self {
        let mut b = RbBook {
            scenes: Vec::new(),
            first_fp: 0,
            next_id: 1,
        };
        for i in 0..RB_FIRST_ROUND {
            b.scenes.push(RbScene {
                id: b.next_id,
                batch: 0,
                book: 0,
                anchor: format!("UNX-F2181-first-{}", i + 1),
                state: 1,
            });
            b.next_id += 1;
        }
        b.first_fp = b.fingerprint_first();
        b
    }

    /// 首轮段指纹（FNV · 前 38 条 ID+锚哈希）。
    pub fn fingerprint_first(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        for s in self.scenes.iter() {
            if s.batch != 0 {
                break;
            }
            h ^= s.id as u64;
            h = h.wrapping_mul(0x100000001b3);
            for byte in s.anchor.bytes() {
                h ^= byte as u64;
                h = h.wrapping_mul(0x100000001b3);
            }
        }
        h
    }

    /// 旧账零改动核（指纹一致）。
    pub fn old_untouched(&self) -> bool {
        self.fingerprint_first() == self.first_fp
    }

    /// 步一：预告批登记（B16–B28 十三批 · 合计 64 条 · 批号连续）。
    pub fn promo_batches(&mut self) -> Result<(), RbDiag> {
        let base_per_batch = RB_PROMO_NEW / RB_PROMO_BATCHES; // 4
        let extra = RB_PROMO_NEW % RB_PROMO_BATCHES; // 12
        let mut batch = 16u16;
        let mut planned = 0u32;
        for b in 0..RB_PROMO_BATCHES {
            let n = base_per_batch + if b < extra { 1 } else { 0 };
            for _ in 0..n {
                self.scenes.push(RbScene {
                    id: self.next_id,
                    batch,
                    book: 0,
                    anchor: format!("UNX-F2181-b{}-s{}", batch, self.next_id),
                    state: 0,
                });
                self.next_id += 1;
                planned += 1;
            }
            batch += 1;
        }
        if planned != RB_PROMO_NEW {
            return Err(RbDiag::new(
                RbCode::PromoMiss,
                "promo_batches",
                format!("预告 {} 条 ≠ {}", planned, RB_PROMO_NEW),
            ));
        }
        // 批号连续核。
        let mut seen_batches: Vec<u16> = Vec::new();
        for s in self.scenes.iter() {
            if s.batch >= 16 && s.batch <= 28 {
                if !seen_batches.contains(&s.batch) {
                    seen_batches.push(s.batch);
                }
            }
        }
        seen_batches.sort_unstable();
        let expect: Vec<u16> = (16..=28).collect();
        if seen_batches != expect {
            return Err(RbDiag::new(
                RbCode::BatchGap,
                "promo_batches",
                format!("批号 {:?}", seen_batches),
            ));
        }
        Ok(())
    }

    /// 步二：预告转正（预告项 state 0→1 · 未预告不可转正）。
    pub fn promote_promo(&mut self) -> Result<usize, RbDiag> {
        let mut n = 0usize;
        for s in self.scenes.iter_mut() {
            if s.batch >= 16 && s.batch <= 28 && s.book == 0 {
                if s.state == 0 {
                    s.state = 1;
                    n += 1;
                }
            }
        }
        Ok(n)
    }

    /// 步三：联测批落账（B29 · 12 条 · 独立册别）。
    pub fn joint_batch(&mut self) -> Result<(), RbDiag> {
        for i in 0..RB_JOINT {
            self.scenes.push(RbScene {
                id: self.next_id,
                batch: 29,
                book: 1,
                anchor: format!("UNX-F2181-joint-{}", i + 1),
                state: 1,
            });
            self.next_id += 1;
        }
        Ok(())
    }

    /// 五列格式核（逐条五列齐）。
    pub fn columns_ok(&self) -> bool {
        self.scenes.iter().all(|s| {
            s.id > 0 && s.anchor.len() > 0 && s.batch <= 29 && s.book <= 1 && s.state <= 1
        })
    }

    /// 执行账总闸（判据点名：114 场景执行账结构落账）。
    pub fn total_gate(&self) -> Result<u32, RbDiag> {
        let pos: u32 = self
            .scenes
            .iter()
            .filter(|s| s.book == 0 && s.state == 1)
            .count() as u32;
        let joint: u32 = self
            .scenes
            .iter()
            .filter(|s| s.book == 1)
            .count() as u32;
        // 正路径 = 首轮 38 + 预告 64 = 102。
        if pos != RB_FIRST_ROUND + RB_PROMO_NEW {
            return Err(RbDiag::new(
                RbCode::TotalDrift,
                "total_gate",
                format!("正路径 {} ≠ {}", pos, RB_FIRST_ROUND + RB_PROMO_NEW),
            ));
        }
        if joint != RB_JOINT {
            return Err(RbDiag::new(
                RbCode::JointShort,
                "total_gate",
                format!("联测 {} ≠ {}", joint, RB_JOINT),
            ));
        }
        let total = pos + joint;
        if total != RB_TOTAL {
            return Err(RbDiag::new(
                RbCode::TotalDrift,
                "total_gate",
                format!("总数 {} ≠ {}", total, RB_TOTAL),
            ));
        }
        // 册别不混：正路径册无 batch 29；联测册只有 batch 29。
        let mix = self
            .scenes
            .iter()
            .any(|s| (s.book == 0 && s.batch == 29) || (s.book == 1 && s.batch != 29));
        if mix {
            return Err(RbDiag::new(
                RbCode::BookMix,
                "total_gate",
                "册别与批号错位".to_string(),
            ));
        }
        Ok(total)
    }

    /// 扩容全流程自洽核。
    pub fn expanded_ok(&self) -> bool {
        self.old_untouched() && self.columns_ok() && self.total_gate().is_ok()
    }
}

/// 演练（扩容全流程 · 10 轮）。
pub fn drill(rounds: u32) -> (bool, u32, u32) {
    let mut ok = 0u32;
    let mut final_total = 0u32;
    for _ in 0..rounds {
        let mut b = RbBook::new_first_round();
        let mut round_ok = b.old_untouched();
        if b.promo_batches().is_err() {
            round_ok = false;
        }
        if b.promote_promo().is_err() {
            round_ok = false;
        }
        if b.joint_batch().is_err() {
            round_ok = false;
        }
        match b.total_gate() {
            Ok(t) => final_total = t,
            Err(_) => round_ok = false,
        }
        if !b.expanded_ok() {
            round_ok = false;
        }
        if round_ok {
            ok += 1;
        }
    }
    (ok == rounds, ok, final_total)
}

/// 注入核（四路必抓）。
pub struct RbInjector {
    /// 场数。
    pub cases: u32,
    /// 检出数。
    pub caught: u32,
    /// 拒绝明细。
    pub rejects: Vec<String>,
}

impl RbInjector {
    /// 空注入核。
    pub fn new() -> Self {
        RbInjector {
            cases: 0,
            caught: 0,
            rejects: Vec::new(),
        }
    }

    /// 八场注入。
    pub fn inject_all(&mut self) {
        // 场 1：旧账改写检出（改首轮锚 → 指纹变）。
        let mut b = RbBook::new_first_round();
        b.scenes[0].anchor = "hacked".to_string();
        if !b.old_untouched() {
            self.caught += 1;
        }
        self.cases += 1;
        // 场 2：批号断裂检出（抽掉 B20 批条目）。
        let mut b2 = RbBook::new_first_round();
        let _ = b2.promo_batches();
        b2.scenes.retain(|s| s.batch != 20);
        let gaps: Vec<u16> = {
            let mut seen: Vec<u16> = Vec::new();
            for s in b2.scenes.iter() {
                if s.batch >= 16 && s.batch <= 28 && !seen.contains(&s.batch) {
                    seen.push(s.batch);
                }
            }
            seen.sort_unstable();
            let expect: Vec<u16> = (16..=28).collect();
            let mut d = Vec::new();
            for e in expect.iter() {
                if !seen.contains(e) {
                    d.push(*e);
                }
            }
            d
        };
        if !gaps.is_empty() {
            self.caught += 1;
        }
        self.cases += 1;
        // 场 3：预告漏登检出（预告少 1 条 → 总闸红）。
        let mut b3 = RbBook::new_first_round();
        let _ = b3.promo_batches();
        b3.scenes.pop();
        let _ = b3.promote_promo();
        let _ = b3.joint_batch();
        if b3.total_gate().is_err() {
            self.rejects.push(match b3.total_gate() {
                Err(d) => d.full(),
                Ok(_) => String::new(),
            });
            self.caught += 1;
        }
        self.cases += 1;
        // 场 4：联测缺位检出（联测 11 条 → 总闸红）。
        let mut b4 = RbBook::new_first_round();
        let _ = b4.promo_batches();
        let _ = b4.promote_promo();
        for _ in 0..(RB_JOINT - 1) {
            b4.scenes.push(RbScene {
                id: b4.next_id,
                batch: 29,
                book: 1,
                anchor: "UNX-F2181-j".to_string(),
                state: 1,
            });
            b4.next_id += 1;
        }
        if b4.total_gate().is_err() {
            self.caught += 1;
        }
        self.cases += 1;
        // 场 5：五列缺列检出（空锚）。
        let mut b5 = RbBook::new_first_round();
        b5.scenes[5].anchor = String::new();
        if !b5.columns_ok() {
            self.caught += 1;
        }
        self.cases += 1;
        // 场 6：册别错位检出（正路径册混入 batch 29 → 总闸红）。
        let mut b6 = RbBook::new_first_round();
        let _ = b6.promo_batches();
        let _ = b6.promote_promo();
        let _ = b6.joint_batch();
        if let Some(s) = b6.scenes.iter_mut().find(|s| s.book == 0 && s.batch <= 28) {
            s.batch = 29;
        }
        if b6.total_gate().is_err() {
            self.caught += 1;
        }
        self.cases += 1;
        // 场 7：未转正检出（预告未 promote → 正路径账短 → 总闸红）。
        let mut b7 = RbBook::new_first_round();
        let _ = b7.promo_batches();
        let _ = b7.joint_batch();
        if b7.total_gate().is_err() {
            self.caught += 1;
        }
        self.cases += 1;
        // 场 8：全流程正例（总闸过 = 114）。
        let mut b8 = RbBook::new_first_round();
        let _ = b8.promo_batches();
        let _ = b8.promote_promo();
        let _ = b8.joint_batch();
        match b8.total_gate() {
            Ok(t) if t == RB_TOTAL => {
                self.caught += 1;
            }
            _ => {}
        }
        self.cases += 1;
    }

    /// 注入满格。
    pub fn all_caught(&self) -> bool {
        self.cases >= 8 && self.caught == self.cases
    }
}

/// 判据门面。
pub struct RbGate {
    /// 注入核。
    pub inj: RbInjector,
}

impl RbGate {
    /// 判据总门。
    pub fn judge(&self, ok: bool, rounds: u32, total: u32) -> (bool, String) {
        let all_ok = ok && self.inj.all_caught();
        let msg = format!(
            "{}: 扩容 {}/{} 轮 执行账={}（38+64 正路径+12 联测）旧账零改动 五列一致 inj={}/{} seg=0x{:04x}",
            RB_VERSION, rounds, rounds, total, self.inj.caught, self.inj.cases, RB_SEG_BASE
        );
        (all_ok, msg)
    }
}

/// 判据自检（t 系入口）。
pub fn run_t2181_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("unx2181-t181");
    // 冻结数值。
    set.add(
        "RB-params-frozen",
        RB_FIRST_ROUND == 38
            && RB_PROMO_BATCHES == 13
            && RB_PROMO_NEW == 64
            && RB_JOINT == 12
            && RB_TOTAL == 114
            && RB_FIRST_ROUND + RB_PROMO_NEW + RB_JOINT == RB_TOTAL,
        "38 + 64 = 102 正路径；102 + 12 = 114",
    );
    // 首轮账基准。
    let b = RbBook::new_first_round();
    set.add(
        "RB-first-round",
        b.scenes.len() == 38 && b.old_untouched(),
        "首轮 38 落账 指纹在册",
    );
    // 演练。
    let (ok, rounds, total) = drill(10);
    set.add(
        "RB-drill-10/10",
        ok,
        &format!("扩容 {}/10 轮 执行账={}", rounds, total),
    );
    // 注入。
    let mut gate = RbGate {
        inj: RbInjector::new(),
    };
    gate.inj.inject_all();
    set.add(
        "RB-inject-8/8",
        gate.inj.all_caught(),
        &format!("{}/{}", gate.inj.caught, gate.inj.cases),
    );
    // 判据总门。
    let (jok, msg) = gate.judge(ok, rounds, total);
    set.add("UNX-F2181-J1", jok, &msg);
    set
}
