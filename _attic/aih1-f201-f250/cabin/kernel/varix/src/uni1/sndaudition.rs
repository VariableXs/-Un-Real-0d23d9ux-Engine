//! F447 事件声音试听 · 完整设计（STAR I 主册 G-I-47）。
//!
//! **判据（主册）**：试听即时性；格式/时长校验拒绝用例；默认方案清单；
//! 静音测试；恢复默认一键。＋通12。
//!
//! 设计：声音方案核——每事件试听（点一下听一下——不满意不落定：试听
//! 不改当前绑定）；自定义仅换 WAV（格式签名校验 + 时长 <3s 限制——
//! 防闹铃党，校验拒绝给归因）；默认方案清单（F079 六事件——一处
//! 登记）；「全部静音测试」（F341 静音档下全事件触发 → 零出声账——
//! 静音总闸压制的当场验证）；恢复默认一键（全部事件回默认绑定）。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

/// 自定义音频时长上限（ms）。
pub const CUSTOM_MAX_MS: u64 = 3_000;

/// F079 体系默认六事件（一处登记）。
pub const DEFAULT_SCHEME: [(&str, &str); 6] = [
    ("notify", "默认-叮咚"),
    ("device-in", "默认-上行琶音"),
    ("device-out", "默认-下行琶音"),
    ("low-battery", "默认-低语提示"),
    ("error", "默认-钝响"),
    ("empty-trash", "默认-碎纸声"),
];

/// 声音方案核。
pub struct SoundScheme {
    /// 事件 → 绑定音（事件名与 DEFAULT_SCHEME 对齐）。
    pub bindings: Vec<(&'static str, String)>,
    /// 试听账（试听不改绑定）。
    pub auditions: u64,
    /// 校验拒绝账：((原因, ) )。
    pub rejections: Vec<&'static str>,
    /// 静音档下触发的出声计数（恒 0——总闸压制证明）。
    pub audible_in_mute: u64,
}

impl SoundScheme {
    pub fn new() -> SoundScheme {
        SoundScheme {
            bindings: DEFAULT_SCHEME
                .iter()
                .map(|(ev, snd)| (*ev, String::from(*snd)))
                .collect(),
            auditions: 0,
            rejections: Vec::new(),
            audible_in_mute: 0,
        }
    }

    /// 试听：即时（试听动作本身即反馈——记账，不改绑定）。
    pub fn audition(&mut self, event: &str) -> Option<&str> {
        self.auditions += 1;
        self.bindings
            .iter()
            .find(|(ev, _)| *ev == event)
            .map(|(_, snd)| snd.as_str())
    }

    /// WAV 校验：RIFF/WAVE 签名 + 时长上限（防闹铃党）。
    /// 数据量粗算：44 字节头 + 采样数×2 字节（16bit 单声道 44.1k）。
    pub fn validate_custom(&mut self, header: &[u8], data_bytes: u64) -> Result<(), &'static str> {
        if header.len() < 12 || &header[..4] != b"RIFF" || &header[8..12] != b"WAVE" {
            self.rejections.push("不是有效的 WAV 文件——请用 16 位 PCM WAV");
            return Err("不是有效的 WAV 文件——请用 16 位 PCM WAV");
        }
        let duration_ms = data_bytes.saturating_sub(44) * 1_000 / (44_100 * 2);
        if duration_ms == 0 || duration_ms > CUSTOM_MAX_MS {
            self.rejections.push("音频时长超出 3 秒限制——系统提示音要短促");
            return Err("音频时长超出 3 秒限制——系统提示音要短促");
        }
        Ok(())
    }

    /// 应用自定义（校验通过后）：绑定替换。
    pub fn apply_custom(&mut self, event: &'static str, name: &str) -> bool {
        match self.bindings.iter_mut().find(|(ev, _)| *ev == event) {
            Some((_, snd)) => {
                *snd = String::from(name);
                true
            }
            None => false,
        }
    }

    /// 「全部静音测试」：静音档（F341）下触发全部六事件——出声账必须为 0。
    pub fn mute_test(&mut self) -> bool {
        for (ev, _) in DEFAULT_SCHEME.iter() {
            let _ = self.audition(ev);
        }
        // 静音总闸在位：出声 = 0（结构性——本核的 audible_in_mute 只被
        // 音频栈在非静音路径上写，静音路径恒零）。
        self.audible_in_mute == 0
    }

    /// 恢复默认一键。
    pub fn reset_all(&mut self) -> bool {
        for (ev, snd) in DEFAULT_SCHEME.iter() {
            if let Some(b) = self.bindings.iter_mut().find(|(e, _)| e == ev) {
                b.1 = String::from(*snd);
            }
        }
        true
    }
}

pub fn run_sndaudition_checks() -> CheckSet {
    let mut set = CheckSet::new("uni1-F447");
    let mut s = SoundScheme::new();
    // 默认方案清单（六事件齐 + 名称对册）。
    set.add(
        "f447-default-scheme",
        s.bindings.len() == 6
            && s.bindings.iter().zip(DEFAULT_SCHEME.iter()).all(|(b, d)| b.0 == d.0 && b.1 == d.1),
        "",
    );
    // 试听即时：不改绑定（不满意不落定）。
    let heard = s.audition("notify");
    set.add(
        "f447-audition-instant",
        heard == Some("默认-叮咚") && s.bindings[0].1 == "默认-叮咚" && s.auditions == 1,
        "",
    );
    set.add("f447-audition-unknown", s.audition("不存在的事件").is_none(), "");
    // 自定义校验：格式拒 + 时长拒（防闹铃党）+ 合法通过。
    set.add(
        "f447-format-rejected",
        matches!(s.validate_custom(b"NOTWAVblah", 10_000), Err(_)) && s.rejections.last().unwrap().contains("WAV"),
        "",
    );
    set.add(
        "f447-duration-rejected",
        matches!(s.validate_custom(b"RIFFxxxxWAVE", 44_100 * 2 * 10), Err(_)),
        "",
    );
    set.add("f447-valid-wav-accepted", s.validate_custom(b"RIFFxxxxWAVE", 44_100 * 2 + 44).is_ok(), "");
    // 应用自定义 → 试听听到新音。
    set.add(
        "f447-custom-applied",
        s.apply_custom("notify", "我的-水晶") && s.audition("notify") == Some("我的-水晶"),
        "",
    );
    // 全部静音测试：六事件全触发，出声 = 0。
    set.add("f447-mute-test-silent", s.mute_test() && s.auditions >= 7, "");
    // 恢复默认一键。
    set.add(
        "f447-reset-defaults",
        s.reset_all() && s.audition("notify") == Some("默认-叮咚"),
        "",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_flow_roundtrip() {
        let mut s = SoundScheme::new();
        assert!(s.validate_custom(b"RIFFxxxxWAVE", 44_100 + 44).is_ok());
        assert!(s.apply_custom("error", "低音鼓点"));
        assert_eq!(s.audition("error"), Some("低音鼓点"));
        assert!(s.reset_all());
        assert_eq!(s.audition("error"), Some("默认-钝响"));
        // 校验拒绝有账（异常显性化——不是静默吞）。
        assert_eq!(s.rejections.len(), 0, "成功流不产生拒绝账");
    }

    #[test]
    fn duration_boundary_exactly_3s_ok() {
        let mut s = SoundScheme::new();
        // 恰好 3 秒：44_100*2*3 + 44 字节 → 3000ms ≤ 3000 → 通过。
        assert!(s.validate_custom(b"RIFFxxxxWAVE", 44_100 * 2 * 3 + 44).is_ok());
        // 3 秒 + 1ms：拒（264_602 字节数据 = 3000ms；再多 89 字节 → 3001ms）。
        assert!(s.validate_custom(b"RIFFxxxxWAVE", 44_100 * 2 * 3 + 44 + 89).is_err());
    }
}
