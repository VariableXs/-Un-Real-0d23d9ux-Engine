# -*- coding: utf-8 -*-
"""C4-B15 边界注记批量插入（AI-14 · finalize ⑤ 四项齐备修复）"""
import re
import sys

FN = "C4-B15.md"

PATCHES = {
    "UNX-F10683": "边界注记：链接计数增减与句柄开闭同锁（对平探针原子）；unlink 幂等（二次 unlink 同名 ENOENT 不报错扩散）；销毁点单点=close 递减后判零（判定与释放同锁防半销毁）。",
    "UNX-F10684": "边界注记：取值与 wait/post 同锁（瞬态性声明下的一致读）；ncnt 采样点=锁内等待队列长度（不含已唤醒未返回者，定义注账）；负值语义为 Varix 选定（移植提示：依赖 0 语义的程序注差异账）。",
    "UNX-F10687": "边界注记：ENOSYS 判定在参数解析后（非法 fd 先报 EBADF，判定序注账）；红账含 fd 与调用点可查；翻绿交接位=预埋账批次标注（不占本批判据）。",
    "UNX-F10690": "边界注记：唤醒判定=计数 0→非零翻转沿（非电平，注账）；EAGAIN 判定先于 8 字节定长校验序（零值读合法报 EAGAIN，序声明）。",
    "UNX-F10692": "边界注记：签名核验=编译期符号级+运行期 grep 双保险（独立时钟接口零旁路断言）；ABSTIME 换算溢出账（超 A3 时刻域→EINVAL 注账）；四拍链账含周期性重装拍（N tick 后重装不跳拍，单调探针覆盖）；联签只读 A3 接口（不反向修改时钟轮，边界声明）。",
    "UNX-F10694": "边界注记：核销判定三要素逐行确认（判据归属/命名空间分立/生命期分账缺一即不核销）；grep 断言双向（POSIX 条目不引 SysV 判据号、SysV 条目不引 POSIX 判据号）；核销后回改探针入回归（族标记字段 grep 常跑）；F10679 总表状态位单向翻（已核销不可逆，注账）。",
    "UNX-F10695": "边界注记：串扰定界=pty 字节账与 socket 报文账分账（各账独立对平，交叉即红）；录制对照含启动段与收尾段（全程录放不留盲区）；唤醒账口径=每轮唤醒数与事件数对平。",
    "UNX-F10697": "边界注记：风暴信号号选实时族（排队语义注账，标准信号不入场防语义混淆）；EINTR 重试由用户态循环演示（内核不自动重试，声明注账）；四件 IPC 账分立（每件独立对平不合并）。",
    "UNX-F10698": "边界注记：10 场景定版含输入脚本与期望帧双要素（缺一不成场）；放像比对=逐字节含时序戳（时刻差 1 tick 容忍注账）；场间复位断言三账（termios/winsize/前台组归零）；主轴链账逐环核销（B05–B08 判据号引用核验）。",
    "UNX-F10699": "边界注记：两态判定=判据号存在性 grep（存在即覆盖、不存在必入待补带原因码）；覆盖≠通过（判据在册即可入账，运行态由各批 finalize 保证）；原因码三值互斥单选（预埋/B16+ 计划/域外移交）；映射账总行数与域判据总数对平（300 条全口径核验）。",
    "UNX-F10700": "边界注记：交接清单批注原文逐字对照（总纲表为唯一事实源，零改写断言）；域账对平链=B01–B15 批内求和逐批核（跳批即红）；挂号面核验含错误码面（挂号点断链即红）。",
}

def main():
    with open(FN, encoding="utf-8") as f:
        text = f.read()

    applied, skipped = [], []
    for fid, note in PATCHES.items():
        m = re.search(r"(### " + fid + r"\b.*?\n)(- 正文：[^\n]*)", text, re.S)
        if not m:
            skipped.append((fid, "正文行未定位"))
            continue
        line = m.group(2)
        if "边界注记：" in line:
            skipped.append((fid, "已含边界注记，跳过"))
            continue
        anchor = "与现存内核衔接点："
        idx = line.find(anchor)
        if idx < 0:
            skipped.append((fid, "衔接点锚未命中"))
            continue
        end = line.find("。", idx)
        if end < 0:
            skipped.append((fid, "衔接点句号未命中"))
            continue
        if text.count(line) != 1:
            skipped.append((fid, "正文行不唯一，拒绝盲替换"))
            continue
        newline = line[:end + 1] + note + line[end + 1:]
        text = text.replace(line, newline, 1)
        applied.append(fid)

    with open(FN, "w", encoding="utf-8", newline="\n") as f:
        f.write(text)

    print(f"applied={len(applied)}: {', '.join(applied)}")
    if skipped:
        print(f"skipped={len(skipped)}:")
        for fid, why in skipped:
            print(f"  {fid}: {why}")
    if len(applied) != len(PATCHES):
        sys.exit(1)

if __name__ == "__main__":
    main()
