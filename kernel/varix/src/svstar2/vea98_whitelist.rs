//! VE-F0098 · 驱动白名单热更新（VE-A 域 · A04b 诊断·降级·安全组 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0098`
//!
//! **职责定位（锚点原文）**：驱动白名单支持热更新——新版本白名单文件带版本号与
//! 校验和，加载时验签（哈希比对）通过后原子切换，旧连接保持旧表、新裁决走新表；
//! 切换保留上一版于内存支持即时回滚；切换事件对接审计。
//!
//! **铁律**：
//! 1. **验签**——校验和不符拒载三要素告知（版本/期望值/实测值）。
//! 2. **原子切换**——切换瞬间查询旧表兜底，裁决连续。
//! 3. **可回滚**——上一版常驻内存，即时回滚。
//! 4. **整文件拒绝**——条目格式错整文件拒，不部分接受。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址 0xE8A9（白名单热更）。
pub const SEG_BASE: u16 = 0xE8A9;

/// 校验和不符。
pub const XC_CHECKSUM: u16 = SEG_BASE | 0x01;
/// 条目格式错误。
pub const XC_BAD_ENTRY: u16 = SEG_BASE | 0x02;
/// 无上一版可回滚。
pub const XC_NO_PREV: u16 = SEG_BASE | 0x03;
/// 版本倒退。
pub const XC_VERSION_REGRESS: u16 = SEG_BASE | 0x04;

/// 白名单条目（驱动标识）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DriverEntry {
    pub vendor_id: u16,
    pub device_id: u16,
    /// 允许的操作位掩码。
    pub allow_mask: u32,
}

impl DriverEntry {
    pub fn fingerprint(&self) -> u32 {
        ((self.vendor_id as u32) << 16)
            ^ (self.device_id as u32)
            ^ self.allow_mask.rotate_left(9)
    }
}

/// 白名单文件（版本 × 条目集 × 校验和）。
#[derive(Clone, Debug)]
pub struct WhitelistFile {
    pub version: u32,
    pub entries: Vec<DriverEntry>,
    pub checksum: u32,
}

impl WhitelistFile {
    fn compute_checksum(entries: &[DriverEntry]) -> u32 {
        let mut acc: u32 = 0x7A7A_3C3C;
        for e in entries {
            acc = acc.rotate_left(4) ^ e.fingerprint();
        }
        acc ^ (entries.len() as u32)
    }

    /// 构造并自签（测试与载入共用单源）。
    pub fn seal(version: u32, entries: Vec<DriverEntry>) -> Result<Self, u16> {
        if version == 0 {
            return Err(XC_VERSION_REGRESS);
        }
        for e in &entries {
            if e.vendor_id == 0 && e.device_id == 0 {
                return Err(XC_BAD_ENTRY);
            }
        }
        let checksum = Self::compute_checksum(&entries);
        Ok(WhitelistFile {
            version,
            entries,
            checksum,
        })
    }

    /// 验签。
    pub fn verify(&self) -> bool {
        Self::compute_checksum(&self.entries) == self.checksum
    }

    /// 条目格式校验（整文件拒绝用）。
    pub fn validate_entries(&self) -> bool {
        !self.entries.is_empty()
            && self
                .entries
                .iter()
                .all(|e| !(e.vendor_id == 0 && e.device_id == 0))
    }
}

/// 裁决结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    /// 允许（带操作掩码）。
    Allow(u32),
    /// 拒绝。
    Deny,
}

/// 热更新管理器（双表快照）。
pub struct WhitelistRuntime {
    current: Option<WhitelistFile>,
    previous: Option<WhitelistFile>,
    last_code: u16,
    /// 切换次数。
    switches: u64,
    /// 回滚次数。
    rollbacks: u64,
    /// 切换事件序列（对接审计）。
    switch_log: Vec<(u64, u32, u32)>,
    seq: u64,
}

impl WhitelistRuntime {
    pub fn new() -> Self {
        WhitelistRuntime {
            current: None,
            previous: None,
            last_code: 0,
            switches: 0,
            rollbacks: 0,
            switch_log: Vec::new(),
            seq: 0,
        }
    }

    pub fn last_code(&self) -> u16 {
        self.last_code
    }

    /// 验签 + 整文件校验 + 原子切换。失败三要素告知（last_code + 返回）。
    pub fn load_and_switch(&mut self, file: WhitelistFile) -> Result<u32, u16> {
        if !file.verify() {
            self.last_code = XC_CHECKSUM;
            return Err(XC_CHECKSUM);
        }
        if !file.validate_entries() {
            self.last_code = XC_BAD_ENTRY;
            return Err(XC_BAD_ENTRY);
        }
        // 版本单调：不接受倒退（回滚走 rollback 入口）。
        if let Some(cur) = &self.current {
            if file.version <= cur.version {
                self.last_code = XC_VERSION_REGRESS;
                return Err(XC_VERSION_REGRESS);
            }
        }
        // 原子切换：先挪现表到 previous，再挂新表。
        if let Some(old) = self.current.take() {
            self.previous = Some(old);
        }
        let v = file.version;
        self.current = Some(file);
        self.seq += 1;
        self.switches += 1;
        self.switch_log.push((self.seq, v, 1));
        self.last_code = 0;
        Ok(v)
    }

    /// 裁决：旧连接语义由调用方持有旧句柄；本表按当前表裁决。
    /// 切换瞬间查询由 current/previous 双表兜底——先查当前，无条目再查上一版。
    pub fn judge(&self, vendor_id: u16, device_id: u16) -> Verdict {
        if let Some(cur) = &self.current {
            if let Some(e) = cur.entries.iter().find(|e| e.vendor_id == vendor_id && e.device_id == device_id)
            {
                return Verdict::Allow(e.allow_mask);
            }
        }
        // 当前表无条目：旧连接兜底查上一版（连续性保证）。
        if let Some(prev) = &self.previous {
            if let Some(e) = prev
                .entries
                .iter()
                .find(|e| e.vendor_id == vendor_id && e.device_id == device_id)
            {
                return Verdict::Allow(e.allow_mask);
            }
        }
        Verdict::Deny
    }

    /// 严格裁决（新裁决只走新表，不兜底——供新连接）。
    pub fn judge_strict(&self, vendor_id: u16, device_id: u16) -> Verdict {
        match &self.current {
            Some(cur) => match cur
                .entries
                .iter()
                .find(|e| e.vendor_id == vendor_id && e.device_id == device_id)
            {
                Some(e) => Verdict::Allow(e.allow_mask),
                None => Verdict::Deny,
            },
            None => Verdict::Deny,
        }
    }

    /// 即时回滚到上一版。
    pub fn rollback(&mut self) -> Result<u32, u16> {
        match self.previous.take() {
            Some(p) => {
                let v = p.version;
                if let Some(cur) = self.current.take() {
                    self.previous = Some(cur);
                }
                self.current = Some(p);
                self.seq += 1;
                self.rollbacks += 1;
                self.switch_log.push((self.seq, v, 2));
                self.last_code = 0;
                Ok(v)
            }
            None => {
                self.last_code = XC_NO_PREV;
                Err(XC_NO_PREV)
            }
        }
    }

    pub fn current_version(&self) -> u32 {
        self.current.as_ref().map(|c| c.version).unwrap_or(0)
    }

    pub fn previous_version(&self) -> u32 {
        self.previous.as_ref().map(|p| p.version).unwrap_or(0)
    }

    pub fn switches(&self) -> u64 {
        self.switches
    }

    pub fn rollbacks(&self) -> u64 {
        self.rollbacks
    }

    /// 切换事件流（对接 F0105 审计）。
    pub fn switch_events(&self) -> &[(u64, u32, u32)] {
        &self.switch_log
    }
}

impl Default for WhitelistRuntime {
    fn default() -> Self {
        Self::new()
    }
}

/// 自检：验签、原子切换、可回滚、整文件拒绝。
pub fn run_vea98_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;

    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }

    fn wl(v: u32, entries: Vec<DriverEntry>) -> WhitelistFile {
        WhitelistFile::seal(v, entries).unwrap()
    }

    fn e(vendor: u16, device: u16) -> DriverEntry {
        DriverEntry {
            vendor_id: vendor,
            device_id: device,
            allow_mask: 0x3,
        }
    }

    let mut set = CheckSet::new("vea98_whitelist");

    // C98-01 验签通过可载
    {
        let mut rt = WhitelistRuntime::new();
        let r = rt.load_and_switch(wl(1, vec![e(0x1002, 0x1234)]));
        chk(&mut set, "C98-01 验签载入", r == Ok(1), "载入失败");
    }

    // C98-02 校验和篡改拒载
    {
        let mut rt = WhitelistRuntime::new();
        let mut f = wl(1, vec![e(0x1002, 0x1234)]);
        f.checksum ^= 0xFFFF;
        let r = rt.load_and_switch(f);
        chk(
            &mut set,
            "C98-02 篡改拒载",
            r.is_err() && rt.last_code() == XC_CHECKSUM,
            "篡改被放行",
        );
    }

    // C98-03 原子切换：切换后新旧双表俱在
    {
        let mut rt = WhitelistRuntime::new();
        let _ = rt.load_and_switch(wl(1, vec![e(0x1002, 0x1234)]));
        let _ = rt.load_and_switch(wl(2, vec![e(0x1002, 0x5678)]));
        let ok = rt.current_version() == 2 && rt.previous_version() == 1 && rt.switches() == 2;
        chk(&mut set, "C98-03 原子切换", ok, "切换非原子");
    }

    // C98-04 旧连接兜底连续
    {
        let mut rt = WhitelistRuntime::new();
        let _ = rt.load_and_switch(wl(1, vec![e(0x1002, 0x1234)]));
        let _ = rt.load_and_switch(wl(2, vec![e(0x1002, 0x5678)]));
        // 旧设备 0x1234 不在新表，兜底应仍允许（旧连接）。
        let ok = rt.judge(0x1002, 0x1234) != Verdict::Deny
            && rt.judge_strict(0x1002, 0x1234) == Verdict::Deny;
        chk(&mut set, "C98-04 旧连接兜底", ok, "连续性断裂");
    }

    // C98-05 即时回滚
    {
        let mut rt = WhitelistRuntime::new();
        let _ = rt.load_and_switch(wl(1, vec![e(0x1002, 0x1234)]));
        let _ = rt.load_and_switch(wl(2, vec![e(0x1002, 0x5678)]));
        let r = rt.rollback();
        let ok = r == Ok(1) && rt.current_version() == 1 && rt.rollbacks() == 1;
        chk(&mut set, "C98-05 即时回滚", ok, "回滚失败");
    }

    // C98-06 无上一版回滚拒绝
    {
        let mut rt = WhitelistRuntime::new();
        let _ = rt.load_and_switch(wl(1, vec![e(0x1002, 0x1234)]));
        let r = rt.rollback();
        chk(
            &mut set,
            "C98-06 无版拒绝",
            r.is_err() && rt.last_code() == XC_NO_PREV,
            "无版被放行",
        );
    }

    // C98-07 空条目整文件拒绝
    {
        let mut rt = WhitelistRuntime::new();
        let f = wl(1, vec![]);
        let r = rt.load_and_switch(f);
        chk(
            &mut set,
            "C98-07 空文件拒绝",
            r.is_err() && rt.last_code() == XC_BAD_ENTRY,
            "空文件被接",
        );
    }

    // C98-08 零标识条目整文件拒绝
    {
        let mut rt = WhitelistRuntime::new();
        let f = WhitelistFile {
            version: 1,
            entries: vec![DriverEntry {
                vendor_id: 0,
                device_id: 0,
                allow_mask: 1,
            }],
            checksum: WhitelistFile::compute_checksum(&[DriverEntry {
                vendor_id: 0,
                device_id: 0,
                allow_mask: 1,
            }]),
        };
        let r = rt.load_and_switch(f);
        chk(
            &mut set,
            "C98-08 零标识拒绝",
            r.is_err() && rt.last_code() == XC_BAD_ENTRY,
            "零标识被接",
        );
    }

    // C98-09 版本倒退拒绝
    {
        let mut rt = WhitelistRuntime::new();
        let _ = rt.load_and_switch(wl(5, vec![e(0x1002, 0x1234)]));
        let r = rt.load_and_switch(wl(3, vec![e(0x1002, 0x5678)]));
        chk(
            &mut set,
            "C98-09 倒退拒绝",
            r.is_err() && rt.last_code() == XC_VERSION_REGRESS,
            "倒退被接",
        );
    }

    // C98-10 切换事件入审计流
    {
        let mut rt = WhitelistRuntime::new();
        let _ = rt.load_and_switch(wl(1, vec![e(1, 2)]));
        let _ = rt.load_and_switch(wl(2, vec![e(3, 4)]));
        let _ = rt.rollback();
        let ok = rt.switch_events().len() == 3;
        chk(&mut set, "C98-10 切换入审计", ok, "审计缺事件");
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vea98_all_green() {
        let s = run_vea98_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
