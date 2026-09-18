# -*- coding: utf-8 -*-
"""任务41 winapi.rs 补丁：API_TABLE 扩容 + dispatch 路由 + read_user_str_pub"""
import io

P = 'kernel/varix/src/proc/winapi.rs'
s = io.open(P, encoding='utf-8').read()

def rep(old, new, tag):
    global s
    assert old in s, 'ANCHOR MISS: ' + tag
    assert s.count(old) == 1, 'ANCHOR DUP: ' + tag
    s = s.replace(old, new, 1)

# ---------- 1) API_TABLE：user32/gdi32 Stub → Full/Partial，新增 comdlg32/kernel32 条目 ----------
rep(
"""    // ---- user32.dll（窗口面：任务41 记事本闭环范围，首层全 Stub）----
    ApiDef { dll: "user32.dll", name: "MessageBoxW", status: ApiStatus::Stub },
    ApiDef { dll: "user32.dll", name: "RegisterClassExW", status: ApiStatus::Stub },
    ApiDef { dll: "user32.dll", name: "CreateWindowExW", status: ApiStatus::Stub },
    ApiDef { dll: "user32.dll", name: "GetMessageW", status: ApiStatus::Stub },
    ApiDef { dll: "user32.dll", name: "DispatchMessageW", status: ApiStatus::Stub },
    ApiDef { dll: "user32.dll", name: "PostQuitMessage", status: ApiStatus::Stub },
    ApiDef { dll: "user32.dll", name: "DefWindowProcW", status: ApiStatus::Stub },
    // ---- gdi32.dll（文本/绘制面：任务41 范围，首层全 Stub）----
    ApiDef { dll: "gdi32.dll", name: "TextOutW", status: ApiStatus::Stub },
    ApiDef { dll: "gdi32.dll", name: "BeginPaint", status: ApiStatus::Stub },
    ApiDef { dll: "gdi32.dll", name: "EndPaint", status: ApiStatus::Stub },
    ApiDef { dll: "gdi32.dll", name: "CreateFontW", status: ApiStatus::Stub },
];""",
"""    // ---- user32.dll（窗口面：任务41 记事本闭环——多参 API 走 NT 风格参数块桥接，
    //      语义完整实现于 winsrv::win32_dispatch；派发类由用户态循环承担如实 Partial）----
    ApiDef { dll: "user32.dll", name: "MessageBoxW", status: ApiStatus::Partial },
    ApiDef { dll: "user32.dll", name: "RegisterClassExW", status: ApiStatus::Full },
    ApiDef { dll: "user32.dll", name: "CreateWindowExW", status: ApiStatus::Full },
    ApiDef { dll: "user32.dll", name: "ShowWindow", status: ApiStatus::Full },
    ApiDef { dll: "user32.dll", name: "UpdateWindow", status: ApiStatus::Full },
    ApiDef { dll: "user32.dll", name: "InvalidateRect", status: ApiStatus::Full },
    ApiDef { dll: "user32.dll", name: "GetMessageW", status: ApiStatus::Full },
    ApiDef { dll: "user32.dll", name: "TranslateMessage", status: ApiStatus::Partial },
    ApiDef { dll: "user32.dll", name: "DispatchMessageW", status: ApiStatus::Partial },
    ApiDef { dll: "user32.dll", name: "PostQuitMessage", status: ApiStatus::Full },
    ApiDef { dll: "user32.dll", name: "DefWindowProcW", status: ApiStatus::Partial },
    // ---- gdi32.dll（文本/绘制面：任务41——画布字模渲染，hdc 约定 1）----
    ApiDef { dll: "gdi32.dll", name: "TextOutW", status: ApiStatus::Full },
    ApiDef { dll: "gdi32.dll", name: "BeginPaint", status: ApiStatus::Full },
    ApiDef { dll: "gdi32.dll", name: "EndPaint", status: ApiStatus::Full },
    ApiDef { dll: "gdi32.dll", name: "CreateFontW", status: ApiStatus::Partial },
    // ---- comdlg32.dll（文件对话框面：任务41——虚拟文件槽+轮转选择器，
    //      选择 UI 实机渲染后置任务 27，内核语义如实登记）----
    ApiDef { dll: "comdlg32.dll", name: "GetOpenFileNameW", status: ApiStatus::Full },
    ApiDef { dll: "comdlg32.dll", name: "GetSaveFileNameW", status: ApiStatus::Full },
    // ---- kernel32.dll（文件句柄扩展：handle 1=控制台（现状）/2=保存目标/3=读取源）----
    ApiDef { dll: "kernel32.dll", name: "ReadFile", status: ApiStatus::Full },
    ApiDef { dll: "kernel32.dll", name: "CloseHandle", status: ApiStatus::Partial },
];""",
    'API_TABLE')

# ---------- 2) dispatch：user32/gdi32/comdlg32 路由到 winsrv；kernel32 文件句柄扩展 ----------
rep(
"""        ApiStatus::Full | ApiStatus::Partial => match (def.dll, def.name) {""",
"""        ApiStatus::Full | ApiStatus::Partial => {
            // 任务41 · 窗口/文本/文件服务台：user32/gdi32/comdlg32 全量走
            // winsrv::win32_dispatch（NT 风格参数块桥接，见 winsrv 模块头）。
            if def.dll == "user32.dll" || def.dll == "gdi32.dll" || def.dll == "comdlg32.dll" {
                return super::winsrv::win32_dispatch(def.dll, def.name, a1, a2, a3);
            }
            match (def.dll, def.name) {""",
    'dispatch route open')

# WriteFile 臂扩展 handle 2；新增 ReadFile/CloseHandle 臂（kernel32 分支内）。
rep(
"""            // 文件组 → SYS_WRITE(2) 控制台路：handle==1 only（Partial 边界）。
            ("kernel32.dll", "WriteFile") | ("ntdll.dll", "NtWriteFile") => {
                PARTIAL_CALLS.fetch_add(1, Ordering::Relaxed);
                if a1 != 1 {
                    return -(ErrNo::Ebadf.to_i32()) as i64;
                }
                crate::proc::ring3::user_write(a2, a3)
            }""",
"""            // 文件组 → SYS_WRITE(2) 控制台路：handle==1 控制台（现状边界）；
            // handle==2 任务41 虚拟文件保存目标（winsrv 句柄分类扩展）。
            ("kernel32.dll", "WriteFile") | ("ntdll.dll", "NtWriteFile") => {
                PARTIAL_CALLS.fetch_add(1, Ordering::Relaxed);
                if a1 == 2 {
                    return super::winsrv::win32_dispatch(def.dll, def.name, a1, a2, a3);
                }
                if a1 != 1 {
                    return -(ErrNo::Ebadf.to_i32()) as i64;
                }
                crate::proc::ring3::user_write(a2, a3)
            }
            // 任务41 新增：ReadFile（handle 3=读取源）/ CloseHandle。
            ("kernel32.dll", "ReadFile") | ("kernel32.dll", "CloseHandle") => {
                FULL_CALLS.fetch_add(1, Ordering::Relaxed);
                super::winsrv::win32_dispatch(def.dll, def.name, a1, a2, a3)
            }""",
    'WriteFile/ReadFile')

# match 收尾闭合（原 match 结束多加一层括号）。
rep(
"""            // Full/Partial 声明与实现必须一一对应——漏臂是注册表 bug。
            _ => {
                STUB_CALLS.fetch_add(1, Ordering::Relaxed);
                -(ErrNo::Enosys.to_i32()) as i64
            }
        },
    }
}""",
"""            // Full/Partial 声明与实现必须一一对应——漏臂是注册表 bug。
            _ => {
                STUB_CALLS.fetch_add(1, Ordering::Relaxed);
                -(ErrNo::Enosys.to_i32()) as i64
            }
            }
        }
    }
}""",
    'match close')

# ---------- 3) read_user_str 公开包装 ----------
rep(
"""/// 读用户态 NUL 结尾串（≤cap 字节 + 终止符）。用户半区校验同 sys_write
/// 口径；越界/超长/无终止符 → None（由调用方决定 NULL/错误语义）。
fn read_user_str(buf: u64, cap: usize) -> Option<alloc::vec::Vec<u8>> {""",
"""/// winsrv 复用入口（任务41）。
pub fn read_user_str_pub(buf: u64, cap: usize) -> Option<alloc::vec::Vec<u8>> {
    read_user_str(buf, cap)
}

/// 读用户态 NUL 结尾串（≤cap 字节 + 终止符）。用户半区校验同 sys_write
/// 口径；越界/超长/无终止符 → None（由调用方决定 NULL/错误语义）。
fn read_user_str(buf: u64, cap: usize) -> Option<alloc::vec::Vec<u8>> {""",
    'read_user_str_pub')

io.open(P, 'w', encoding='utf-8', newline='\n').write(s)
print('winapi patched,', len(s.splitlines()), 'lines')
