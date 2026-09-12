//! `codeanalysis-entry` · 壳C ELF 入口（C13~C15 落地载体）。
//!
//! - `x86_64-unknown-none`：`_start` → 注册清单自检 → 开事件端口订阅
//!   键位/退出 → VFS 桥写回启动标记 → 帧循环消费键位事件。
//! - 宿主（`cargo build`/`test`）：占位说明入口，保证零告警可构建。
//!
//! 内核联调（真实加载 ELF、事件到达、VFS 落盘）**挂账**——
//! 对应 C24 等价清单壳C 行的 🔶 状态，待 VARIX 运行时就绪后消除。

#![cfg_attr(target_os = "none", no_std, no_main)]

#[cfg(target_os = "none")]
mod imp {
    use codeanalysis::eventloop::{run_until_quit, EventSource, RawEvent, SUBSCRIBE_MASK};
    use codeanalysis::manifest::AppManifest;
    use codeanalysis::vfsbridge::{FsBridge, KernelBridge};
    use varix_std::event;
    use varix_std::io;

    /// 内核事件端口源（C15 内核侧实现层）。
    struct PortSrc(event::EventPort);

    impl EventSource for PortSrc {
        fn poll(&mut self) -> Option<RawEvent> {
            match self.0.poll() {
                Ok(ev) => Some(RawEvent { kind: ev.kind, data: [ev.data[0], ev.data[1]] }),
                Err(_) => None, // Empty / 内核离线：当帧空转
            }
        }
    }

    #[no_mangle]
    pub extern "C" fn _start() -> ! {
        // C13：清单自检（能力位 × 栈对齐），异常即带码退出。
        let m = AppManifest::codeanalysis();
        if !m.valid() {
            varix_std::proc::exit(2);
        }

        // C14：VFS 桥写回启动标记（同 core 侧 WriteChannel 相对路径契约）。
        let mut bridge = KernelBridge::new(m.caps);
        let _ = bridge.write("state/startup.txt", b"codeanalysis shell-c up\n");

        // C15：订阅键位 + 退出，帧循环消费。
        match event::EventPort::open(SUBSCRIBE_MASK) {
            Ok(port) => {
                let mut src = PortSrc(port);
                // 键位处理骨架：真实绑定表由 ca-core keymap 域下发（挂账）。
                let _ = run_until_quit(&mut src, &mut |_ev: RawEvent| true, u32::MAX);
            }
            Err(_) => {}
        }
        varix_std::proc::exit(0);
    }

    // 声明使用的 io 能力（防未用告警；close/read 由桥内部消化）。
    #[allow(dead_code)]
    fn _io_uses() {
        let _ = (io::R_ONLY, io::W_ONLY);
    }
}

#[cfg(target_os = "none")]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    varix_std::proc::exit(101)
}

#[cfg(not(target_os = "none"))]
fn main() {
    println!(
        "codeanalysis-entry: 请以 x86_64-unknown-none 目标构建以生成 entry.elf；\
         逻辑层测试见 `cargo test`（C13/C14/C15 纯逻辑在宿主验证）。"
    );
}
