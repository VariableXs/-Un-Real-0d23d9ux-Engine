//! CGPU-F1282 · PCIe 带宽模型（CGPU-I 域 · 带宽与传输域 · I02 组 · 目标 380 行）。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F1282`
//!
//! **判据（锚点原文）**：代际表、共享瓶颈、探测、公开、判据。
//!
//! **职责定位（锚点原文）**：PCIe 带宽建模（代际带宽表（Gen3/4/5×
//! lane 数）——理论带宽与实测有效带宽差异声明；共享瓶颈（PCIe 为
//! CPU-GPU 共享通道——多消费者竞争建模）；带宽探测（实测有效带宽——
//! 理论 vs 实测对拍）；模型公开。
//!
//! ## 一、代际带宽表（判据一）
//!
//! 代际理论带宽表（[`gen_theory_mbps`]）：Gen3=985MB/s/lane、Gen4=
//! 1969MB/s/lane、Gen5=3938MB/s/lane（单向、128b/130b 编码后——表值
//! 写死并注明来源：PCIe 规范 8/16/32 GT/s × 128/130 × /8 字节）。×
//! lane 数得整卡理论带宽。**差异声明**：理论值是物理层上限，实测
//! 有效带宽受协议开销（TLP 头/ACK/流控）与工作负载影响，通常 75~90%
//! ——模型输出永远同时给理论值与有效系数（[`EFFICIENCY_DEFAULT_PPM`]
//! 默认 850000ppm），单边输出理论值当有效值是撒谎。
//!
//! ## 二、共享瓶颈建模（判据二）
//!
//! PCIe 是 CPU-GPU **共享通道**：上传/下载/页表迁移/中断同时挤一条
//! 总线。消费者竞争模型（[`ShareLedger`]）：各消费者按 ppm 权重共享
//! （权重和恰 1e6——配置错误显性拒，与 F0322 池比例同纪律）；拥塞
//! 检出（申请总量 > 有效带宽 → 超载消费者清单——检出是查询，仲裁
//! 交给 F1459 融合规划器）。
//!
//! ## 三、带宽探测与对拍（判据三）
//!
//! 实测有效带宽注入（探测执行在 F1281 上游——本模块建模对拍）：
//! 实测/理论比值落入 [600000, 1000000]ppm 合理带；低于 600000ppm
//! 判链路异常（降速协商/链路重训练），高于理论值判探测错误（物理
//! 不可能——探测面有毛病）。对拍结论（[`CalibVerdict`]）三态 + 人读
//! 建议链。
//!
//! ## 四、模型公开（判据四）
//!
//! 模型公开（[`model_card`]）：代际表、有效系数、共享规则、对拍口径
//! 全部一卡输出——下游消费方拿卡对账，不读源码猜语义（公开是纪律
//! 不是文档装饰：模型口径变了卡必须跟着变版本）。
//!
//! **对接**：F1281（I 域开工与带宽总架构——账户化传输面消费本模型）；
//! F1459（主源融合——拥塞仲裁）；F1441+（J 域功耗——带宽-功耗联动
//! 下游）。零 panic 面、零 IO、零墙钟、no_std、零浮点。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量
// ---------------------------------------------------------------------------

/// 本项版本。
pub const PCIE_BW_VERSION: &str = "I02-pciebw-v1";

/// 每 lane 单向理论带宽（MB/s）：Gen3=985（8 GT/s ×128/130 ÷8）。
pub const GEN3_MBPS_PER_LANE: u64 = 985;
/// Gen4=1969（16 GT/s，恰 2×Gen3）。
pub const GEN4_MBPS_PER_LANE: u64 = 1_969;
/// Gen5=3938（32 GT/s，恰 2×Gen4）。
pub const GEN5_MBPS_PER_LANE: u64 = 3_938;

/// 默认有效带宽系数（ppm；理论→有效 85%——协议开销经验中值，
/// 当前策略值，可调）。
pub const EFFICIENCY_DEFAULT_PPM: u64 = 850_000;

/// 对拍合理带下界（ppm；低于=链路异常）。
pub const CALIB_FLOOR_PPM: u64 = 600_000;

/// 对拍合理带上界（ppm；高于=探测错误——物理不可能超过理论值）。
pub const CALIB_CEIL_PPM: u64 = 1_000_000;

// ---------------------------------------------------------------------------
// 二、诊断码（0x3A 段独占：I02 PCIe 模型面）
// ---------------------------------------------------------------------------

/// 诊断码闭集（0x3A 段）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XcCode {
    /// 代际表外（Gen 1/2/6+ 未建模——闭集三档，扩表走 ADR）。
    UnknownGen,
    /// lane 数非法（x1/x4/x8/x16 闭集之外）。
    UnknownLanes,
    /// 共享权重和≠1e6。
    BadShareSum,
    /// 实测值物理不可能（有效带宽 > 理论带宽）。
    MeasureImpossible,
}

impl XcCode {
    /// 人读诊断。
    pub const fn detail(self) -> &'static str {
        match self {
            XcCode::UnknownGen => "代际表外：Gen3/4/5 三档闭集，新代际扩表走 ADR 并同步 model card",
            XcCode::UnknownLanes => "lane 数表外：x1/x4/x8/x16 四档闭集，建议按实际链路宽度取用",
            XcCode::BadShareSum => "共享权重和≠1000000ppm：显性拒不静默归一，修正消费者权重",
            XcCode::MeasureImpossible => "实测超理论带宽：物理不可能，探测面有毛病——先修探测再对拍",
        }
    }
}

// ---------------------------------------------------------------------------
// 三、代际带宽表（判据一）
// ---------------------------------------------------------------------------

/// 链路代际闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PcieGen {
    /// Gen3（8 GT/s）。
    Gen3,
    /// Gen4（16 GT/s）。
    Gen4,
    /// Gen5（32 GT/s）。
    Gen5,
}

impl PcieGen {
    /// 每 lane 单向理论带宽（MB/s；表值写死见常量区）。
    pub const fn mbps_per_lane(self) -> u64 {
        match self {
            PcieGen::Gen3 => GEN3_MBPS_PER_LANE,
            PcieGen::Gen4 => GEN4_MBPS_PER_LANE,
            PcieGen::Gen5 => GEN5_MBPS_PER_LANE,
        }
    }

    /// 代际名（人读）。
    pub const fn zh(self) -> &'static str {
        match self {
            PcieGen::Gen3 => "Gen3",
            PcieGen::Gen4 => "Gen4",
            PcieGen::Gen5 => "Gen5",
        }
    }
}

/// lane 数闭集（x1/x4/x8/x16）。
pub const LANES_SET: [u32; 4] = [1, 4, 8, 16];

/// 理论带宽（MB/s 单向；代际 × lane；闭集校验）。
pub fn gen_theory_mbps(gen: PcieGen, lanes: u32) -> Result<u64, XcCode> {
    let mut hit = false;
    let mut i = 0usize;
    while i < LANES_SET.len() {
        if let Some(&l) = LANES_SET.get(i) {
            if l == lanes {
                hit = true;
                break;
            }
        }
        i += 1;
    }
    if !hit {
        return Err(XcCode::UnknownLanes);
    }
    Ok(gen.mbps_per_lane().saturating_mul(lanes as u64))
}

/// 有效带宽（MB/s；理论 × 有效系数——双值输出见 model card）。
pub const fn effective_mbps(theory_mbps: u64, eff_ppm: u64) -> u64 {
    theory_mbps.saturating_mul(eff_ppm) / 1_000_000
}

// ---------------------------------------------------------------------------
// 四、共享瓶颈建模（判据二）
// ---------------------------------------------------------------------------

/// 共享消费者行。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Consumer {
    /// 消费者名（纹理上传/下载/交换/流送/页表迁移/中断——闭集外允许
    /// 扩展但名字必须显性）。
    pub name: String,
    /// 共享权重（ppm；和恰 1e6）。
    pub weight_ppm: u64,
    /// 本拍申请带宽（MB/s）。
    pub demand_mbps: u64,
}

/// 共享账（多消费者竞争建模；检出是查询不是仲裁）。
#[derive(Clone, Debug, Default)]
pub struct ShareLedger {
    consumers: Vec<Consumer>,
}

impl ShareLedger {
    /// 登记消费者（同名重复登记拒绝——账一行一人）。
    pub fn register(&mut self, name: &str, weight_ppm: u64, demand_mbps: u64) -> Result<(), XcCode> {
        let mut i = 0usize;
        while i < self.consumers.len() {
            if let Some(c) = self.consumers.get(i) {
                if c.name == name {
                    return Err(XcCode::BadShareSum); // 语义：键冲突按配置错拒
                }
            }
            i += 1;
        }
        self.consumers.push(Consumer {
            name: String::from(name),
            weight_ppm,
            demand_mbps,
        });
        Ok(())
    }

    /// 权重和校验（判据二的闭式前提）。
    pub fn share_sum_ok(&self) -> bool {
        let mut sum = 0u64;
        let mut i = 0usize;
        while i < self.consumers.len() {
            if let Some(c) = self.consumers.get(i) {
                sum = sum.saturating_add(c.weight_ppm);
            }
            i += 1;
        }
        sum == 1_000_000
    }

    /// 拥塞检出（申请总量 vs 有效带宽；返回超载消费者名清单——
    /// 检出是查询，仲裁交 F1459）。
    pub fn congested(&self, effective_mbps_cap: u64) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        if !self.share_sum_ok() {
            // 权重配置错=全单拥塞（账本身不可信，全部列出待仲裁复核）。
            let mut i = 0usize;
            while i < self.consumers.len() {
                if let Some(c) = self.consumers.get(i) {
                    out.push(String::from(c.name.as_str()));
                }
                i += 1;
            }
            return out;
        }
        let mut total = 0u64;
        let mut i = 0usize;
        while i < self.consumers.len() {
            if let Some(c) = self.consumers.get(i) {
                total = total.saturating_add(c.demand_mbps);
            }
            i += 1;
        }
        if total <= effective_mbps_cap {
            return out;
        }
        // 按需求/权重比超出者列名（需求占比 > 权重占比即挤出他人）。
        let mut i = 0usize;
        while i < self.consumers.len() {
            if let Some(c) = self.consumers.get(i) {
                let demand_share = c.demand_mbps.saturating_mul(1_000_000) / total.max(1);
                if demand_share > c.weight_ppm {
                    out.push(String::from(c.name.as_str()));
                }
            }
            i += 1;
        }
        out
    }

    /// 消费者数。
    pub const fn len(&self) -> usize {
        self.consumers.len()
    }

    /// 空判定。
    pub const fn is_empty(&self) -> bool {
        self.consumers.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 五、带宽探测对拍（判据三）
// ---------------------------------------------------------------------------

/// 对拍结论三态（合理带/链路异常/探测错误）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CalibVerdict {
    /// 合理（600000~1000000ppm 带）。
    Sound,
    /// 链路异常（实测效率 < 下界——降速/重训练嫌疑）。
    LinkDegraded,
    /// 探测错误（实测 > 理论——物理不可能）。
    ProbeBroken,
}

impl CalibVerdict {
    /// 结论名（人读）。
    pub const fn zh(self) -> &'static str {
        match self {
            CalibVerdict::Sound => "对拍合理",
            CalibVerdict::LinkDegraded => "链路异常",
            CalibVerdict::ProbeBroken => "探测错误",
        }
    }

    /// 建议链（错误路径带可操作建议）。
    pub const fn advice(self) -> &'static str {
        match self {
            CalibVerdict::Sound => "无需处置",
            CalibVerdict::LinkDegraded => "查链路降速协商与重训练记录，复核槽位/线缆后重测",
            CalibVerdict::ProbeBroken => "实测超理论物理不可能：修探测计时面（回查 F1281 采样口径）",
        }
    }
}

/// 对拍（理论 × 有效系数 vs 实测——ppm 口径判定；物理不可能显性判）。
pub const fn calibrate(theory_mbps: u64, measured_mbps: u64) -> CalibVerdict {
    if theory_mbps == 0 {
        return CalibVerdict::ProbeBroken;
    }
    if measured_mbps > theory_mbps {
        return CalibVerdict::ProbeBroken;
    }
    let ratio_ppm = measured_mbps.saturating_mul(1_000_000) / theory_mbps;
    if ratio_ppm < CALIB_FLOOR_PPM {
        CalibVerdict::LinkDegraded
    } else {
        CalibVerdict::Sound
    }
}

// ---------------------------------------------------------------------------
// 六、模型公开（判据四）
// ---------------------------------------------------------------------------

/// 模型卡（公开口径：代际表+有效系数+共享规则+对拍带——下游对账面）。
pub fn model_card(gen: PcieGen, lanes: u32, eff_ppm: u64) -> String {
    let mut s = String::from("PCIe 模型卡 v");
    s.push_str(PCIE_BW_VERSION);
    s.push_str("｜代际 ");
    s.push_str(gen.zh());
    s.push_str("，lane x");
    s.push_str(&lanes.to_string());
    s.push_str("，理论 ");
    match gen_theory_mbps(gen, lanes) {
        Ok(t) => {
            s.push_str(&t.to_string());
            s.push_str("MB/s，有效 ");
            s.push_str(&effective_mbps(t, eff_ppm).to_string());
            s.push_str("MB/s（系数 ");
            s.push_str(&eff_ppm.to_string());
            s.push_str("ppm）");
        }
        Err(e) => {
            s.push('：');
            s.push_str(e.detail());
        }
    }
    s.push_str("｜共享：权重和恰 1e6，拥塞检出按需求/权重比｜对拍带 ");
    s.push_str(&CALIB_FLOOR_PPM.to_string());
    s.push('~');
    s.push_str(&CALIB_CEIL_PPM.to_string());
    s.push_str("ppm");
    s
}

// ---------------------------------------------------------------------------
// 七、传输时延估算与升级建议（判据一、三的消费面深化）
// ---------------------------------------------------------------------------

/// 传输时延估算（μs：字节量 ÷ 有效带宽——整数算术，MB/s ×1e6 →
/// B/s；带宽为零显性拒）。传输时延 = bytes×1e6/(mbps×1e6) 约成
/// bytes/mbps μs（MB/s = 1e6 B/s → bytes/(mbps) μs）。
pub const fn transfer_us(bytes: u64, effective_mbps: u64) -> Result<u64, XcCode> {
    if effective_mbps == 0 {
        return Err(XcCode::UnknownLanes); // 语义：零带宽按链路非法拒
    }
    // bytes × 1e6 μs/s ÷ (mbps × 1e6 B/s) = bytes/mbps μs。
    Ok(bytes / effective_mbps)
}

/// 公平份额投影（判据二的深化：每消费者按权重可得的有效带宽
/// （MB/s）——权重和错时返 None（账不可信不给投影））。
pub fn fair_share(l: &ShareLedger, effective_mbps_cap: u64) -> Result<Vec<(String, u64)>, XcCode> {
    if !l.share_sum_ok() {
        return Err(XcCode::BadShareSum);
    }
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < l.consumers.len() {
        if let Some(c) = l.consumers.get(i) {
            let share = effective_mbps_cap.saturating_mul(c.weight_ppm) / 1_000_000;
            out.push((String::from(c.name.as_str()), share));
        }
        i += 1;
    }
    Ok(out)
}

/// 升级建议（判据三的深化：需求总量 vs 有效带宽——占用比 ≥90% 建议升
/// lane，≥100% 建议升代际+lane（同代加 lane 不够时换代际收益更大）；
/// 结论带依据数字）。
pub fn upgrade_advice(
    gen: PcieGen,
    lanes: u32,
    demand_total_mbps: u64,
    eff_ppm: u64,
) -> String {
    let mut s = String::from("升级建议：");
    let theory = match gen_theory_mbps(gen, lanes) {
        Ok(t) => t,
        Err(e) => {
            s.push_str(e.detail());
            return s;
        }
    };
    let effective = effective_mbps(theory, eff_ppm);
    if demand_total_mbps <= effective {
        let usage_ppm = demand_total_mbps.saturating_mul(1_000_000) / effective.max(1);
        s.push_str("无需升级（占用 ");
        s.push_str(&usage_ppm.to_string());
        s.push_str("ppm）");
        return s;
    }
    let usage_ppm = demand_total_mbps.saturating_mul(1_000_000) / effective.max(1);
    if usage_ppm >= 1_000_000 {
        s.push_str("需求超有效带宽——建议升代际（");
        s.push_str(gen.zh());
        s.push_str("→下一档）或加 lane；同代加 lane 收益线性、换代际收益翻倍");
    } else {
        s.push_str("占用 ");
        s.push_str(&usage_ppm.to_string());
        s.push_str("ppm ≥90% 警戒线——建议加 lane（x");
        s.push_str(&lanes.to_string());
        s.push_str("→x16）");
    }
    s
}
