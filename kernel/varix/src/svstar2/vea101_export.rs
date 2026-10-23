//! VE-F0101 · 诊断数据脱敏导出（VE-A 域 · A04b 诊断·降级·安全组 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0101`
//!
//! **职责定位（锚点原文）**：诊断数据导出统一走脱敏通道——复用 F0097 规则表先过滤
//! 后打包，导出包带清单（含哪些数据类别、条数、脱敏规则版本），用户确认对话框
//! 列出包内全部类别；导出动作入审计日志，半成品临时文件失败即清理。
//!
//! **铁律**：
//! 1. **先滤后包**——过滤器不可用即拒绝导出，不降级放行。
//! 2. **清单透明**——清单与实际内容强一致，不符即导出失败。
//! 3. **用户确认**——确认列出全部类别后才落包。
//! 4. **审计留痕**——导出动作入审计（时间×触发方×包哈希）。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址 0xE8AC（脱敏导出）。
pub const SEG_BASE: u16 = 0xE8AC;

/// 过滤器不可用。
pub const XC_FILTER_UNAVAILABLE: u16 = SEG_BASE | 0x01;
/// 清单与内容不符。
pub const XC_MANIFEST_MISMATCH: u16 = SEG_BASE | 0x02;
/// 用户未确认。
pub const XC_NOT_CONFIRMED: u16 = SEG_BASE | 0x03;
/// 打包中断。
pub const XC_PACK_INTERRUPTED: u16 = SEG_BASE | 0x04;
/// 半成品清理失败。
pub const XC_TEMP_CLEANUP: u16 = SEG_BASE | 0x05;

/// 数据类别（清单单元）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DataClass {
    /// 性能指标。
    Metrics,
    /// 事件日志。
    Events,
    /// 配置快照。
    Config,
    /// 回放片段。
    Replay,
}

/// 清单项。
#[derive(Clone, Copy, Debug)]
pub struct ManifestItem {
    pub class: DataClass,
    pub count: u32,
    pub rule_version: u32,
}

/// 导出清单。
#[derive(Clone, Debug)]
pub struct Manifest {
    pub items: Vec<ManifestItem>,
}

impl Manifest {
    /// 清单自洽：类别不重复、计数非零。
    pub fn validate(&self) -> bool {
        if self.items.is_empty() {
            return false;
        }
        for (i, a) in self.items.iter().enumerate() {
            if a.count == 0 {
                return false;
            }
            for b in self.items.iter().skip(i + 1) {
                if a.class == b.class {
                    return false;
                }
            }
        }
        true
    }
}

/// 审计记录。
#[derive(Clone, Debug)]
pub struct ExportAudit {
    pub tick: u64,
    /// 触发方（0 用户 1 系统 2 自动巡检）。
    pub trigger: u8,
    pub package_hash: u32,
}

/// 导出包。
#[derive(Clone, Debug)]
pub struct ExportPackage {
    pub manifest: Manifest,
    /// 脱敏后载荷（类别×条目字节）。
    pub payload: Vec<(DataClass, Vec<u8>)>,
    pub hash: u32,
}

impl ExportPackage {
    fn compute_hash(manifest: &Manifest, payload: &[(DataClass, Vec<u8>)]) -> u32 {
        let mut acc: u32 = 0xC0FF_EE00;
        for it in &manifest.items {
            acc = acc.rotate_left(5) ^ (it.class as u32) ^ it.count ^ it.rule_version;
        }
        for (c, p) in payload {
            acc = acc.rotate_left(3) ^ (*c as u32);
            for b in p {
                acc = acc.rotate_left(1) ^ (*b as u32);
            }
        }
        acc
    }

    /// 清单与实际载荷一致性核对。
    pub fn verify_consistency(&self) -> bool {
        if !self.manifest.validate() {
            return false;
        }
        for it in &self.manifest.items {
            let actual = self
                .payload
                .iter()
                .filter(|(c, _)| *c == it.class)
                .map(|(_, p)| p.len() as u32)
                .sum::<u32>();
            // 计数按条目语义：每条 payload 字节块记 1 条（此处以块数核对）。
            let blocks = self.payload.iter().filter(|(c, _)| *c == it.class).count() as u32;
            let _ = actual;
            if blocks != it.count {
                return false;
            }
        }
        Self::compute_hash(&self.manifest, &self.payload) == self.hash
    }
}

/// 脱敏导出器。
pub struct SanitizedExporter {
    /// 规则表版本（与 F0097 同源单源）。
    rule_version: u32,
    /// 过滤器健康位。
    filter_ok: bool,
    /// 半成品临时区。
    temp: Vec<(DataClass, Vec<u8>)>,
    /// 确认状态。
    confirmed: bool,
    last_code: u16,
    audits: Vec<ExportAudit>,
    /// 累计成功导出。
    exported: u32,
    /// 累计清理次数。
    cleanups: u32,
}

impl SanitizedExporter {
    pub fn new(rule_version: u32) -> Result<Self, u16> {
        if rule_version == 0 {
            return Err(XC_FILTER_UNAVAILABLE);
        }
        Ok(SanitizedExporter {
            rule_version,
            filter_ok: true,
            temp: Vec::new(),
            confirmed: false,
            last_code: 0,
            audits: Vec::new(),
            exported: 0,
            cleanups: 0,
        })
    }

    pub fn last_code(&self) -> u16 {
        self.last_code
    }

    /// 过滤器状态注入（F0097 健康传播）。
    pub fn set_filter_health(&mut self, ok: bool) {
        self.filter_ok = ok;
        if !ok {
            self.last_code = XC_FILTER_UNAVAILABLE;
        }
    }

    /// 先滤后包：推入一条已脱敏数据（调用方须先过 F0097）。
    pub fn stage(&mut self, class: DataClass, masked: Vec<u8>) -> Result<(), u16> {
        if !self.filter_ok {
            self.last_code = XC_FILTER_UNAVAILABLE;
            return Err(XC_FILTER_UNAVAILABLE);
        }
        self.temp.push((class, masked));
        self.last_code = 0;
        Ok(())
    }

    /// 用户确认（列出全部类别后确认）。
    pub fn confirm(&mut self, classes_seen: &[DataClass]) -> Result<(), u16> {
        // 确认必须覆盖暂存区全部类别（透明清单）。
        for (c, _) in &self.temp {
            if !classes_seen.contains(c) {
                self.last_code = XC_NOT_CONFIRMED;
                return Err(XC_NOT_CONFIRMED);
            }
        }
        self.confirmed = true;
        self.last_code = 0;
        Ok(())
    }

    /// 打包：清单生成 + 哈希 + 一致性核对。中断即清半成品。
    pub fn pack(&mut self, tick: u64, trigger: u8) -> Result<ExportPackage, u16> {
        if !self.filter_ok {
            let _ = self.cleanup_temp();
            self.last_code = XC_FILTER_UNAVAILABLE;
            return Err(XC_FILTER_UNAVAILABLE);
        }
        if !self.confirmed {
            let _ = self.cleanup_temp();
            self.last_code = XC_NOT_CONFIRMED;
            return Err(XC_NOT_CONFIRMED);
        }
        if self.temp.is_empty() {
            let _ = self.cleanup_temp();
            self.last_code = XC_PACK_INTERRUPTED;
            return Err(XC_PACK_INTERRUPTED);
        }
        // 按类别聚合计数。
        let mut items: Vec<ManifestItem> = Vec::new();
        for (c, _) in &self.temp {
            match items.iter_mut().find(|i| i.class == *c) {
                Some(i) => i.count += 1,
                None => items.push(ManifestItem {
                    class: *c,
                    count: 1,
                    rule_version: self.rule_version,
                }),
            }
        }
        let manifest = Manifest { items };
        let payload = self.temp.clone();
        let hash = ExportPackage::compute_hash(&manifest, &payload);
        let pkg = ExportPackage {
            manifest,
            payload,
            hash,
        };
        // 清单与实际核对。
        if !pkg.verify_consistency() {
            let _ = self.cleanup_temp();
            self.last_code = XC_MANIFEST_MISMATCH;
            return Err(XC_MANIFEST_MISMATCH);
        }
        // 审计留痕。
        self.audits.push(ExportAudit {
            tick,
            trigger,
            package_hash: hash,
        });
        self.exported += 1;
        self.confirmed = false;
        self.temp.clear();
        self.last_code = 0;
        Ok(pkg)
    }

    /// 清理半成品。
    pub fn cleanup_temp(&mut self) -> Result<(), u16> {
        self.temp.clear();
        self.confirmed = false;
        self.cleanups += 1;
        self.last_code = 0;
        Ok(())
    }

    pub fn audits(&self) -> &[ExportAudit] {
        &self.audits
    }

    pub fn exported(&self) -> u32 {
        self.exported
    }

    pub fn cleanups(&self) -> u32 {
        self.cleanups
    }

    pub fn rule_version(&self) -> u32 {
        self.rule_version
    }
}

/// 自检：先滤后包、清单透明、用户确认、审计留痕。
pub fn run_vea101_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;

    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }

    let mut set = CheckSet::new("vea101_export");

    // C101-01 正常导出通路
    {
        let mut ex = SanitizedExporter::new(3).unwrap();
        let _ = ex.stage(DataClass::Metrics, vec![1, 2, 3]);
        let _ = ex.confirm(&[DataClass::Metrics]);
        let r = ex.pack(1, 0);
        chk(&mut set, "C101-01 正常导出", r.is_ok() && ex.exported() == 1, "导出失败");
    }

    // C101-02 过滤器不可用拒绝导出
    {
        let mut ex = SanitizedExporter::new(3).unwrap();
        ex.set_filter_health(false);
        let r = ex.stage(DataClass::Metrics, vec![1]);
        chk(
            &mut set,
            "C101-02 先滤后包",
            r.is_err() && ex.last_code() == XC_FILTER_UNAVAILABLE,
            "滤器失效仍放行",
        );
    }

    // C101-03 未确认打包拒绝
    {
        let mut ex = SanitizedExporter::new(3).unwrap();
        let _ = ex.stage(DataClass::Events, vec![9]);
        let r = ex.pack(1, 0);
        chk(
            &mut set,
            "C101-03 未确认拒绝",
            r.is_err() && ex.last_code() == XC_NOT_CONFIRMED,
            "未确认被放行",
        );
    }

    // C101-04 确认须覆盖全部类别
    {
        let mut ex = SanitizedExporter::new(3).unwrap();
        let _ = ex.stage(DataClass::Metrics, vec![1]);
        let _ = ex.stage(DataClass::Events, vec![2]);
        let r = ex.confirm(&[DataClass::Metrics]);
        chk(
            &mut set,
            "C101-04 确认覆盖",
            r.is_err() && ex.last_code() == XC_NOT_CONFIRMED,
            "漏类别被放行",
        );
    }

    // C101-05 清单与内容一致
    {
        let mut ex = SanitizedExporter::new(3).unwrap();
        let _ = ex.stage(DataClass::Metrics, vec![1]);
        let _ = ex.stage(DataClass::Metrics, vec![2]);
        let _ = ex.stage(DataClass::Config, vec![3]);
        let _ = ex.confirm(&[DataClass::Metrics, DataClass::Config]);
        let pkg = ex.pack(2, 0).unwrap();
        let ok = pkg.manifest.items.len() == 2 && pkg.verify_consistency();
        chk(&mut set, "C101-05 清单一致", ok, "清单失配");
    }

    // C101-06 审计留痕
    {
        let mut ex = SanitizedExporter::new(3).unwrap();
        let _ = ex.stage(DataClass::Replay, vec![7]);
        let _ = ex.confirm(&[DataClass::Replay]);
        let _ = ex.pack(9, 1);
        let ok = ex.audits().len() == 1 && ex.audits()[0].trigger == 1;
        chk(&mut set, "C101-06 审计留痕", ok, "审计缺失");
    }

    // C101-07 打包中断清半成品
    {
        let mut ex = SanitizedExporter::new(3).unwrap();
        let _ = ex.stage(DataClass::Metrics, vec![1]);
        let r = ex.pack(1, 0); // 未确认 → 拒 + 清
        let ok = r.is_err() && ex.cleanups() >= 1;
        chk(&mut set, "C101-07 中断清理", ok, "半成品残留");
    }

    // C101-08 规则版本零拒绝
    {
        let r = SanitizedExporter::new(0);
        chk(
            &mut set,
            "C101-08 版本零拒",
            r.err() == Some(XC_FILTER_UNAVAILABLE),
            "版本零被放行",
        );
    }

    // C101-09 清单版本带规则版本
    {
        let mut ex = SanitizedExporter::new(7).unwrap();
        let _ = ex.stage(DataClass::Metrics, vec![1]);
        let _ = ex.confirm(&[DataClass::Metrics]);
        let pkg = ex.pack(1, 0).unwrap();
        chk(
            &mut set,
            "C101-09 清单带版本",
            pkg.manifest.items[0].rule_version == 7,
            "版本缺失",
        );
    }

    // C101-10 空暂存打包拒绝
    {
        let mut ex = SanitizedExporter::new(3).unwrap();
        ex.confirmed = true;
        let r = ex.pack(1, 0);
        chk(
            &mut set,
            "C101-10 空包拒绝",
            r.is_err() && ex.last_code() == XC_PACK_INTERRUPTED,
            "空包被放行",
        );
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vea101_all_green() {
        let s = run_vea101_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
