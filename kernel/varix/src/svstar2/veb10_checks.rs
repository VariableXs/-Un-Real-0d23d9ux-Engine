//! VE-F0210 · 域自检（判据逐条对应，见 `veb10_cursor.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 通道分离 → `C210-分离-*`（cursorq 独立消息形态、位置更新零拷贝）
//! - 最新位保序 → `C210-最新位-*`（单槽覆盖丢中间帧、丢弃显性计数）
//! - 软件兜底 → `C210-兜底-*`（cursorq 不可用降级、切换显性通知、回切通知）
//! - 热点保留 → `C210-热点-*`（图像内热点 + 消息随行 + 读屏换算读数）

use super::veb02_proto::CMD_UPDATE_CURSOR;
use super::veb10_cursor::*;
use crate::checks::CheckSet;

fn img(id: u32) -> CursorImage {
    CursorImage {
        resource_id: id,
        width: 64,
        height: 64,
        hot_x: 8,
        hot_y: 4,
        data_len: 64 * 64 * 4,
    }
}

/// VE-F0210 域自检。
pub fn run_veb10_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-veb10");
    let cfg = CursorConfig::default_config();

    // ---- 判据：通道分离 ----

    // 位置更新消息零拷贝：8 字 u32、命令码复用 F0202 的 CMD_UPDATE_CURSOR
    {
        let mut ch = CursorChannel::new(cfg);
        let _ = ch.set_image(img(7));
        ch.move_to(0, 100, 50);
        let pos = ch.take_pending_pos().unwrap();
        let wire = ch.encode_pos_msg(&pos);
        set.add(
            "C210-分离-位置消息零拷贝",
            wire.len() == 8 && wire[0] == CMD_UPDATE_CURSOR && wire[2] == 100 && wire[3] == 50,
            "",
        );
    }
    // 软件兜底模式下位置照常记账（合成器消费同一最新位）
    {
        let mut ch = CursorChannel::new(cfg);
        let _ = ch.set_queue_available(false);
        ch.move_to(0, 10, 20);
        set.add(
            "C210-分离-软件模式位置记账",
            ch.mode() == ChannelMode::Software && ch.take_pending_pos().is_some(),
            "",
        );
    }

    // ---- 判据：最新位保序 ----

    // 连发位置：单槽覆盖，中间帧丢弃但最新位保留
    {
        let mut ch = CursorChannel::new(cfg);
        ch.move_to(0, 1, 1);
        ch.move_to(0, 2, 2);
        ch.move_to(0, 3, 3);
        let pos = ch.take_pending_pos().unwrap();
        set.add(
            "C210-最新位-保最新位",
            pos.x == 3 && pos.y == 3 && ch.dropped_intermediate == 2,
            "",
        );
    }
    // 取走后单槽清空
    {
        let mut ch = CursorChannel::new(cfg);
        ch.move_to(0, 5, 6);
        let _ = ch.take_pending_pos();
        set.add("C210-最新位-取走清空", ch.take_pending_pos().is_none(), "");
    }
    // 资源引用随行（未设图像时 resource_id=0）
    {
        let mut ch = CursorChannel::new(cfg);
        let a = ch.move_to(0, 1, 1);
        let _ = ch.set_image(img(9));
        let b = ch.move_to(0, 2, 2);
        set.add(
            "C210-最新位-资源引用随行",
            a.resource_id == 0 && b.resource_id == 9,
            "",
        );
    }

    // ---- 判据：软件兜底 ----

    // cursorq 不可用 → 降级软件光标 + 显性通知
    {
        let mut ch = CursorChannel::new(cfg);
        let n = ch.set_queue_available(false);
        set.add(
            "C210-兜底-降级显性通知",
            ch.mode() == ChannelMode::Software
                && n.as_ref().map(|x| x.is_explicit()).unwrap_or(false)
                && n.as_ref().map(|x| x.from == ChannelMode::Hardware && x.to == ChannelMode::Software).unwrap_or(false),
            "",
        );
    }
    // 恢复可用 → 回切硬件 + 通知
    {
        let mut ch = CursorChannel::new(cfg);
        let _ = ch.set_queue_available(false);
        let n = ch.set_queue_available(true);
        set.add(
            "C210-兜底-回切通知",
            ch.mode() == ChannelMode::Hardware
                && n.as_ref().map(|x| x.to == ChannelMode::Hardware).unwrap_or(false)
                && ch.notices.len() == 2,
            "",
        );
    }
    // 可用性不变时零通知（不刷屏）
    {
        let mut ch = CursorChannel::new(cfg);
        let n1 = ch.set_queue_available(true);
        set.add("C210-兜底-幂等零通知", n1.is_none() && ch.notices.is_empty(), "");
    }

    // ---- 判据：热点保留 ----

    // 图像内热点 + 消息随行 + 读屏读数一致
    {
        let mut ch = CursorChannel::new(cfg);
        let _ = ch.set_image(img(3));
        ch.move_to(0, 100, 100);
        let pos = ch.take_pending_pos().unwrap();
        let wire = ch.encode_pos_msg(&pos);
        set.add(
            "C210-热点-三处一致",
            ch.hotspot() == (8, 4) && wire[5] == 8 && wire[6] == 4,
            "",
        );
    }
    // 无图像时热点退 (0,0)
    {
        let ch = CursorChannel::new(cfg);
        set.add("C210-热点-无图像退零", ch.hotspot() == (0, 0), "");
    }

    // ---- 边界防护：过大图像拒绝保旧图 ----

    // 尺寸超限拒绝且旧图保留
    {
        let mut ch = CursorChannel::new(cfg);
        let _ = ch.set_image(img(1));
        let big = CursorImage {
            resource_id: 2,
            width: 256,
            height: 256,
            hot_x: 0,
            hot_y: 0,
            data_len: 256 * 256 * 4,
        };
        let e = ch.set_image(big).unwrap_err();
        set.add(
            "C210-边界-尺寸超限保旧图",
            e.code == "E_CURSOR_DIM"
                && e.is_complete()
                && ch.image().map(|i| i.resource_id).unwrap_or(0) == 1,
            "",
        );
    }
    // 字节数与宽高不符拒绝
    {
        let mut ch = CursorChannel::new(cfg);
        let bad = CursorImage {
            data_len: 100,
            ..img(2)
        };
        let e = ch.set_image(bad).unwrap_err();
        set.add("C210-边界-字节数不符", e.code == "E_CURSOR_FORMAT", "");
    }
    // 零尺寸拒绝
    {
        let mut ch = CursorChannel::new(cfg);
        let bad = CursorImage {
            width: 0,
            height: 0,
            data_len: 0,
            ..img(3)
        };
        let e = ch.set_image(bad).unwrap_err();
        set.add("C210-边界-零尺寸拒绝", e.code == "E_CURSOR_DIM", "");
    }
    // 合法图像接受
    {
        let mut ch = CursorChannel::new(cfg);
        set.add("C210-边界-合法接受", ch.set_image(img(4)).is_ok() && ch.image_upload_len() == Some(64 * 64 * 4), "");
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    /// VE-F0210 全绿闸：红项逐行枚举（定位用），零红才算过。
    #[test]
    fn veb10_checks_all_green() {
        let set = run_veb10_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "veb10 自检存在红项：{}/{} 绿，红项：{:?}",
            passed,
            passed + failed,
            set.red_items()
                .0
                .iter()
                .flatten()
                .filter(|c| !c.passed)
                .map(|c| c.name)
                .collect::<Vec<_>>()
        );
    }
}
