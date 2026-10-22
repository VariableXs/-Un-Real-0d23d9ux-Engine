//! F244 vxapp 清单声明解析：`hotkey=ctrl+alt+f9|说明文本` 行格式的
//! 逐 token 校验面（未知 token / 双键名 / 重复修饰键 / 缺键名显性
//! 拒绝——清单错误不静默）。

use super::keys::{Combo, mod_from_token, name_to_key};
use super::registry::DescBuf;

/// 清单声明行格式：`hotkey=ctrl+alt+f9|说明文本`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParsedDecl {
    pub mods: u8,
    pub key: u16,
    pub desc: DescBuf,
}

const DECL_PREFIX: &[u8] = b"hotkey=";

/// 单 token 归位（parse_decl 内联提取——嵌套 5 层压至 2 层）：修饰键
/// 置位或键名落位。返回 None = 显性拒绝（键名出现在键位之前 / 重复
/// 修饰键 / 双键名 / 未知 token）。
fn fold_token(tok: &[u8], mods: &mut u8, key: &mut Option<u16>) -> Option<()> {
    if tok.is_empty() {
        return None;
    }
    match mod_from_token(tok) {
        Some(m) => {
            if key.is_some() || *mods & m != 0 {
                return None; // 键名出现在键位之前 / 重复修饰键
            }
            *mods |= m;
            Some(())
        }
        None => {
            if key.is_some() {
                return None; // 双键名
            }
            *key = Some(name_to_key(tok)?);
            Some(())
        }
    }
}

/// 解析一条声明行：前缀校验 + 逐 token 校验（未知 token、双键名、
/// 重复修饰键、缺键名显性拒绝——清单错误不静默）。
pub fn parse_decl(line: &[u8]) -> Option<ParsedDecl> {
    if line.len() < DECL_PREFIX.len() + 3 || &line[..DECL_PREFIX.len()] != DECL_PREFIX {
        return None;
    }
    let rest = &line[DECL_PREFIX.len()..];
    let bar = rest.iter().position(|&b| b == b'|')?;
    let combo_part = &rest[..bar];
    let desc_part = &rest[bar + 1..];
    let mut mods = 0u8;
    let mut key: Option<u16> = None;
    let mut start = 0usize;
    for i in 0..=combo_part.len() {
        if i == combo_part.len() || combo_part[i] == b'+' {
            fold_token(&combo_part[start..i], &mut mods, &mut key)?;
            start = i + 1;
        }
    }
    let key = key?;
    if !Combo::new(mods, key).sane() {
        return None;
    }
    Some(ParsedDecl { mods, key, desc: DescBuf::from_bytes(desc_part) })
}
