//! VE-F0127 · 引擎配置系统（VE-A 域 · A06 环境与系统集成组 · 目标 420 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0127`
//!
//! **职责定位（锚点原文）**：渲染引擎配置的统一系统（配置分层：默认/用户/场景/
//! 覆盖四级），配置项的类型安全（类型错拒绝），配置变更的生效语义（即时/需重启
//! 两类显式声明）；含配置的导入导出。
//!
//! **铁律**：
//! 1. **四级分层**——默认/用户/场景/覆盖。
//! 2. **类型安全**——类型错拒绝。
//! 3. **生效语义**——即时/需重启显式声明。
//! 4. **冲突仲裁**——分层冲突按层优先仲裁。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址 0xE8C5（配置系统）。
pub const SEG_BASE: u16 = 0xE8C5;

/// 类型违例。
pub const XC_TYPE_VIOLATION: u16 = SEG_BASE | 0x01;
/// 分层冲突。
pub const XC_LAYER_CONFLICT: u16 = SEG_BASE | 0x02;
/// 语义缺失。
pub const XC_SEMANTIC_MISSING: u16 = SEG_BASE | 0x03;
/// 未知键。
pub const XC_UNKNOWN_KEY: u16 = SEG_BASE | 0x04;

/// 配置层（数字大=优先级高）。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum ConfigLayer {
    /// 默认层。
    Default = 0,
    /// 用户层。
    User = 1,
    /// 场景层。
    Scene = 2,
    /// 覆盖层。
    Override = 3,
}

/// 生效语义。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EffectSemantics {
    /// 即时生效。
    Immediate,
    /// 需重启。
    NeedRestart,
}

/// 配置值类型。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConfigType {
    Bool,
    Int,
    Str,
}

/// 配置项。
#[derive(Clone, Debug)]
pub struct ConfigEntry {
    pub key: u32,
    pub layer: ConfigLayer,
    pub ty: ConfigType,
    /// 值编码（bool: 0/1, int: 整数, str: 哈希）。
    pub value: u64,
    pub semantics: EffectSemantics,
}

/// 配置系统。
pub struct ConfigSystem {
    entries: Vec<ConfigEntry>,
    /// 已声明键类型。
    declared: Vec<(u32, ConfigType, EffectSemantics)>,
    last_code: u16,
    /// 冲突仲裁次数。
    arbitrations: u32,
    /// 类型拒绝次数。
    type_rejects: u32,
}

impl ConfigSystem {
    pub fn new() -> Self {
        ConfigSystem {
            entries: Vec::new(),
            declared: Vec::new(),
            last_code: 0,
            arbitrations: 0,
            type_rejects: 0,
        }
    }

    pub fn last_code(&self) -> u16 {
        self.last_code
    }

    /// 声明配置项（类型+语义）。
    pub fn declare(&mut self, key: u32, ty: ConfigType, semantics: EffectSemantics) {
        self.declared.retain(|(k, _, _)| *k != key);
        self.declared.push((key, ty, semantics));
    }

    /// 设置配置项（类型安全 + 分层仲裁）。
    pub fn set(&mut self, entry: ConfigEntry) -> Result<(), u16> {
        // 类型校验。
        match self.declared.iter().find(|(k, _, _)| *k == entry.key) {
            Some((_, ty, sem)) => {
                if *ty != entry.ty {
                    self.type_rejects += 1;
                    self.last_code = XC_TYPE_VIOLATION;
                    return Err(XC_TYPE_VIOLATION);
                }
                let _ = sem;
            }
            None => {
                self.last_code = XC_UNKNOWN_KEY;
                return Err(XC_UNKNOWN_KEY);
            }
        }
        // 分层仲裁：高优先层覆盖低层。
        match self.entries.iter_mut().find(|e| e.key == entry.key) {
            Some(existing) => {
                if entry.layer > existing.layer {
                    *existing = entry;
                    self.arbitrations += 1;
                } else if entry.layer == existing.layer {
                    self.last_code = XC_LAYER_CONFLICT;
                    return Err(XC_LAYER_CONFLICT);
                }
                // 低层写入被忽略（仲裁）。
            }
            None => self.entries.push(entry),
        }
        self.last_code = 0;
        Ok(())
    }

    /// 查询有效值（最高层）。
    pub fn get(&self, key: u32) -> Option<&ConfigEntry> {
        self.entries
            .iter()
            .filter(|e| e.key == key)
            .max_by_key(|e| e.layer as u8)
    }

    /// 语义查询。
    pub fn semantics_of(&self, key: u32) -> Option<EffectSemantics> {
        self.declared
            .iter()
            .find(|(k, _, _)| *k == key)
            .map(|(_, _, s)| *s)
    }

    /// 导出（开放格式：key=layer:value）。
    pub fn export(&self) -> Vec<(u32, u8, u64)> {
        self.entries
            .iter()
            .map(|e| (e.key, e.layer as u8, e.value))
            .collect()
    }

    /// 导入。
    pub fn import(&mut self, data: &[(u32, u8, u64)]) -> Result<(), u16> {
        for (key, layer, value) in data {
            let layer = match layer {
                0 => ConfigLayer::Default,
                1 => ConfigLayer::User,
                2 => ConfigLayer::Scene,
                3 => ConfigLayer::Override,
                _ => {
                    self.last_code = XC_LAYER_CONFLICT;
                    return Err(XC_LAYER_CONFLICT);
                }
            };
            let ty = match self.declared.iter().find(|(k, _, _)| k == key) {
                Some((_, t, _)) => *t,
                None => {
                    self.last_code = XC_UNKNOWN_KEY;
                    return Err(XC_UNKNOWN_KEY);
                }
            };
            self.set(ConfigEntry {
                key: *key,
                layer,
                ty,
                value: *value,
                semantics: EffectSemantics::Immediate,
            })?;
        }
        Ok(())
    }

    pub fn arbitrations(&self) -> u32 {
        self.arbitrations
    }

    pub fn type_rejects(&self) -> u32 {
        self.type_rejects
    }
}

impl Default for ConfigSystem {
    fn default() -> Self {
        Self::new()
    }
}

/// 自检：四级分层、类型安全、生效语义、冲突仲裁。
pub fn run_vea127_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;

    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }

    fn entry(key: u32, layer: ConfigLayer, ty: ConfigType, v: u64) -> ConfigEntry {
        ConfigEntry {
            key,
            layer,
            ty,
            value: v,
            semantics: EffectSemantics::Immediate,
        }
    }

    fn sys() -> ConfigSystem {
        let mut s = ConfigSystem::new();
        s.declare(1, ConfigType::Int, EffectSemantics::Immediate);
        s.declare(2, ConfigType::Bool, EffectSemantics::NeedRestart);
        s
    }

    let mut set = CheckSet::new("vea127_config");

    // C127-01 基本设置
    {
        let mut s = sys();
        let r = s.set(entry(1, ConfigLayer::Default, ConfigType::Int, 42));
        chk(&mut set, "C127-01 设置", r.is_ok() && s.get(1).unwrap().value == 42, "设置失败");
    }

    // C127-02 类型安全拒绝
    {
        let mut s = sys();
        let r = s.set(entry(1, ConfigLayer::Default, ConfigType::Bool, 1));
        chk(
            &mut set,
            "C127-02 类型拒",
            r.is_err() && s.last_code() == XC_TYPE_VIOLATION && s.type_rejects() == 1,
            "类型被放",
        );
    }

    // C127-03 四级分层覆盖
    {
        let mut s = sys();
        let _ = s.set(entry(1, ConfigLayer::Default, ConfigType::Int, 1));
        let _ = s.set(entry(1, ConfigLayer::User, ConfigType::Int, 2));
        let _ = s.set(entry(1, ConfigLayer::Override, ConfigType::Int, 9));
        chk(&mut set, "C127-03 分层", s.get(1).unwrap().value == 9, "分层仲裁错");
    }

    // C127-04 同层冲突拒绝
    {
        let mut s = sys();
        let _ = s.set(entry(1, ConfigLayer::User, ConfigType::Int, 1));
        let r = s.set(entry(1, ConfigLayer::User, ConfigType::Int, 2));
        chk(
            &mut set,
            "C127-04 同层拒",
            r.is_err() && s.last_code() == XC_LAYER_CONFLICT,
            "同层被放",
        );
    }

    // C127-05 低层不覆盖高层
    {
        let mut s = sys();
        let _ = s.set(entry(1, ConfigLayer::Override, ConfigType::Int, 9));
        let _ = s.set(entry(1, ConfigLayer::Default, ConfigType::Int, 1));
        chk(&mut set, "C127-05 高优先", s.get(1).unwrap().value == 9, "低层越权");
    }

    // C127-06 生效语义
    {
        let s = sys();
        chk(
            &mut set,
            "C127-06 语义",
            s.semantics_of(2) == Some(EffectSemantics::NeedRestart),
            "语义缺",
        );
    }

    // C127-07 未知键拒绝
    {
        let mut s = sys();
        let r = s.set(entry(99, ConfigLayer::Default, ConfigType::Int, 1));
        chk(&mut set, "C127-07 未知拒", r.is_err() && s.last_code() == XC_UNKNOWN_KEY, "未知被放");
    }

    // C127-08 导入导出
    {
        let mut s = sys();
        let _ = s.set(entry(1, ConfigLayer::User, ConfigType::Int, 42));
        let data = s.export();
        let mut s2 = sys();
        let r = s2.import(&data);
        chk(&mut set, "C127-08 导入导出", r.is_ok() && s2.get(1).unwrap().value == 42, "导入失败");
    }

    // C127-09 仲裁计数
    {
        let mut s = sys();
        let _ = s.set(entry(1, ConfigLayer::Default, ConfigType::Int, 1));
        let _ = s.set(entry(1, ConfigLayer::User, ConfigType::Int, 2));
        chk(&mut set, "C127-09 仲裁计数", s.arbitrations() == 1, "计数错");
    }

    // C127-10 导入坏层拒绝
    {
        let mut s = sys();
        let r = s.import(&[(1, 9, 1)]);
        chk(&mut set, "C127-10 坏层拒", r.is_err(), "坏层被放");
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vea127_all_green() {
        let s = run_vea127_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
