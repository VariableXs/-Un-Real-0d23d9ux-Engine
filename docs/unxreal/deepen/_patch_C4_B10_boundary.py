# -*- coding: utf-8 -*-
"""C4-B10 边界注记批量插入（AI-14 · finalize ⑤ 四项齐备修复）
对 19 条正文行不足 300 字的条目，在「与现存内核衔接点：…。」句号后插入「边界注记：…」技术边界条款。
"""
import re
import sys

FN = "C4-B10.md"

PATCHES = {
    "UNX-F10582": "边界注记：addrlen 按实际回填长度（不预填 110 定值）；匿名发送者回填不造假（不空填不假名）；地址快照随报文入队（TOCTOU 防护：回填时对端已重绑不影响已入队快照）。",
    "UNX-F10583": "边界注记：隐式绑定幂等（二次 send 复用首次地址不重绑）；connect 与隐式绑定判定序（connect 后再 send 走已连接态免地址路径）；匿名地址不参与占名互斥账（不进占用表）。",
    "UNX-F10584": "边界注记：报文截断即错账（SEQPACKET 无 MSG_TRUNC 截断许可——请求量小于包长按声明账显性报错，与 DGRAM 截断 F10585 判据边界分立）；连接断开在途包随销毁弃（不投递声明）；connect 必经（无连接路径判据外）。",
    "UNX-F10585": "边界注记：两种含义判定序（请求旗标先于返回旗标，二者不互斥可同现）；截断丢弃不可恢复（无续读声明，区别于 STREAM）；MSG_PEEK 组合行为（截断探查不消费队列，重复 peek 结果一致幂等）注账。",
    "UNX-F10586": "边界注记：抽象名长度以整段定界（Linux 语义长度定界非 NUL 定界，名内 NUL 合法注账）；抽象域不经 fs 权限判定（访问控制缺位声明落账，绑定即全局可达）；同名互斥只查抽象表（与文件域 F10597 隔离）。",
    "UNX-F10587": "边界注记：回收触发点=fd 引用计数归零（非首次 close 调用，dup 场不早释）；自动回收幂等（重复销毁不重释不报错）；抽象域占用表与文件域占名表分锁（锁序账落册防交叉死锁）。",
    "UNX-F10588": "边界注记：判定序=上限→容量→入队三段不可乱序；EMSGSIZE 与 EAGAIN 分派（超上限直接拒、容量暂满才 EAGAIN）；零长报文不受上限约束（原子性特例账）；SO_SNDBUF 变更不追回已入队报文（只约束新入队）。",
    "UNX-F10589": "边界注记：满溢丢弃与发送端错误判定序（先判容量后判阻塞态）；阻塞态发送方按声明账（Varix 选显性 EAGAIN 非阻塞面注差异，阻塞面语义注出处）；丢弃计数账非回执（不提供对端确认语义，声明落账）。",
    "UNX-F10590": "边界注记：EOPNOTSUPP 与 ENOSYS 分派序（地址形态先判、语义路径后判，判定序注账）；红账含触发用例全文（显性化纪律零静默）；反向断言含已连接态空地址合法路径对照组（默认目标语义不误伤）；网络域组播选项进 unix 域的交叉拒绝声明落册。",
    "UNX-F10591": "边界注记：三桥账为架构登记非实现条（接口签名定版后由消费批次核销）；签名漂移核销红即阻断消费批次立项（防脱节）；桥间不互调（fd 型/proc 型/fs 型判据独立，混合场景归 B15 压测场，不占本条判据）。",
    "UNX-F10592": "边界注记：多发送者「到达序」=入队序快照（无全局全序声明，交叉序不可复现落账不装保序）；保序面与 F10595 就绪位独立（事件序≠报文序双序对照声明）；SEQPACKET 保序走连接通道与 DGRAM 队列保序实现分立；判据边界=单发者万包 FIFO 设红绿、多者交错仅落账不设判据。",
    "UNX-F10593": "边界注记：截断判定先于 fd 注入（B11 消费前置约束：cmsg 空间不足不启动注入防半注）；MSG_CTRUNC 与 MSG_TRUNC 判定独立（控制面/数据面分立旗标不串位）；反向断言含零 cmsg 与 cmsg 充足两组对照；B11 收口时核销本条前置位（漂移即红）。",
    "UNX-F10594": "边界注记：EOF 判定只认连接终态位（0 返回值不触发终态判定，判定源分立声明）；零长报文计报不计字（统计账口径注账）；STREAM 零字节 write 不入队（与 DGRAM 差异点落表）；往返探针含空信与混信两场（零长与变长交替验证队列不粘连）。",
    "UNX-F10595": "边界注记：就绪位即时快照（非承诺投递——快照后满溢丢弃不追账，同先查后挂纪律）；POLLOUT 判定=发送队列非满（与对端接收容量解耦声明，满溢对端不回压本端）；就绪位与 F10589 丢弃计数双账对平（一致性探针常跑）。",
    "UNX-F10596": "边界注记：RESET 态置位一次性不可逆（置位后 recv 恒 ECONNRESET 直至 fd 关闭）；在途计数与销毁动作同锁原子（竞态场防线）；正常关闭判定序=读净（在途归零）先于 close（顺序颠倒即误报，次序探针常跑）。",
    "UNX-F10597": "边界注记：双表分锁（抽象域锁/文件域锁独立，锁序账落册防死锁）；跨域 connect 判定序=先域标记后路由（域标记误判探针零容忍）；同名并存不影响各自互斥账（F10586/F10587 独立判据不复用不串账）。",
    "UNX-F10598": "边界注记：三身份项创建期只读（setsockopt 拒绝声明，与 ENOPROTOOPT 分派账分立）；缓冲值域账含下限（低于下限 set 显性报错不静默夹逼）；倍增语义选直通（声明注差异，判据按直通值计，复测口径唯一）。",
    "UNX-F10599": "边界注记：前置位核销制（B11 F10601 收口时回填核销，签名漂移即红）；两层声明判据边界=fd 表操作归 fd 型桥账、键空间操作归 SysV 账（交叉即越界红）；三型可用性以 B11 实测判据为准（本条只声明不设实现判据）。",
    "UNX-F10600": "边界注记：回收断言覆盖三账（socket 数/fd 数/抽象名数归零三探针缺一不可）；挂号三对应核验不可跳项（缺一即红不豁免）；B11 交接清单为预告账（消费核销在 B11 收口回填，不占本批判据不提前翻绿）。",
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
        newline = line[:end + 1] + note + line[end + 1:]
        if text.count(line) != 1:
            skipped.append((fid, "正文行不唯一，拒绝盲替换"))
            continue
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
