import { describe, expect, it, vi, beforeEach, afterEach, type MockInstance } from "vitest";
import { ipc } from "../../../lib/ipc";
import { classifyDropPaths, importSoftwareInbox, registerDroppedFolder } from "../thirdApps";

/**
 * 批次F 回归：拖入软件文件夹 → 智能扫描自动登记——
 * - classifyDropPaths 纯逻辑：exe/lnk/bat/cmd 直接登记，其余交文件夹扫描；
 * - registerDroppedFolder：只登记 recommended 候选、单条失败不中断、
 *   普通文件（isFolder=false）静默返回；
 * - ipc 层全部 stub（node 环境无 Tauri）。
 */

describe("classifyDropPaths（拖放路径分类）", () => {
  it("exe/lnk/bat/cmd（大小写不敏感）归入 files，其余归入 rest", () => {
    const { files, rest } = classifyDropPaths([
      "C:/Tools/A.exe",
      "C:/Tools/b.LNK",
      "C:/Tools/c.bat",
      "C:/Tools/d.CMD",
      "C:/Games/",
      "D:/资料.zip",
      "E:/readme.txt",
    ]);
    expect(files).toEqual(["C:/Tools/A.exe", "C:/Tools/b.LNK", "C:/Tools/c.bat", "C:/Tools/d.CMD"]);
    expect(rest).toEqual(["C:/Games/", "D:/资料.zip", "E:/readme.txt"]);
  });

  it("无后缀路径与中文/空格路径原样透传到 rest（不抛错）", () => {
    const { files, rest } = classifyDropPaths(["D:\\我的软件\\微信 文件夹", "noext"]);
    expect(files).toEqual([]);
    expect(rest).toEqual(["D:\\我的软件\\微信 文件夹", "noext"]);
  });

  it("空数组输入返回双空（不抛错）", () => {
    expect(classifyDropPaths([])).toEqual({ files: [], rest: [] });
  });
});

describe("registerDroppedFolder（文件夹自动登记）", () => {
  // afterEach 的 restoreAllMocks 会卸下 spy → 每个用例前重新挂载，
  // 保证 mockResolvedValue 作用在绑定 ipc 对象的真实属性上
  let scanSpy: MockInstance<typeof ipc.tpScanFolder>;
  let addSpy: MockInstance<typeof ipc.tpAdd>;

  beforeEach(() => {
    scanSpy = vi.spyOn(ipc, "tpScanFolder");
    addSpy = vi.spyOn(ipc, "tpAdd");
  });
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("只登记 recommended 候选；found 计全部候选（含未推荐的辅助组件）", async () => {
    scanSpy.mockResolvedValue({
      isFolder: true,
      candidates: [
        { path: "D:/PotPlayer/PotPlayer.exe", name: "PotPlayer", recommended: true },
        { path: "D:/PotPlayer/unins000.exe", name: "unins000", recommended: false },
        { path: "D:/PotPlayer/codec.exe", name: "codec", recommended: false },
      ],
    } as never);
    addSpy.mockResolvedValue({} as never);

    const r = await registerDroppedFolder("D:/PotPlayer");
    expect(r).toEqual({ isFolder: true, added: 1, found: 3 });
    expect(addSpy).toHaveBeenCalledTimes(1);
    expect(addSpy).toHaveBeenCalledWith("D:/PotPlayer/PotPlayer.exe", "PotPlayer");
  });

  it("单条登记失败不中断批次，added 如实计数", async () => {
    scanSpy.mockResolvedValue({
      isFolder: true,
      candidates: [
        { path: "D:/X/a.exe", name: "A", recommended: true },
        { path: "D:/X/b.exe", name: "B", recommended: true },
        { path: "D:/X/c.exe", name: "C", recommended: true },
      ],
    } as never);
    addSpy.mockImplementation(async (_p: string, n?: string) => {
      if (n === "B") throw new Error("os error 5");
      return {} as never;
    });

    const r = await registerDroppedFolder("D:/X");
    expect(r).toEqual({ isFolder: true, added: 2, found: 3 });
    expect(addSpy).toHaveBeenCalledTimes(3);
  });

  it("无推荐候选（空文件夹/纯数据目录）→ added=0、found 如实", async () => {
    scanSpy.mockResolvedValue({ isFolder: true, candidates: [] } as never);
    const r = await registerDroppedFolder("D:/Empty");
    expect(r).toEqual({ isFolder: true, added: 0, found: 0 });
    expect(addSpy).not.toHaveBeenCalled();
  });

  it("普通文件（isFolder=false）静默返回，不触发任何登记", async () => {
    scanSpy.mockResolvedValue({ isFolder: false, candidates: [] } as never);
    const r = await registerDroppedFolder("D:/readme.txt");
    expect(r).toEqual({ isFolder: false, added: 0, found: 0 });
    expect(addSpy).not.toHaveBeenCalled();
  });

  it("扫描命令异常向上抛出（调用方 toast 兜底，不吞错）", async () => {
    scanSpy.mockRejectedValue(new Error("rpc timeout"));
    await expect(registerDroppedFolder("D:/X")).rejects.toThrow("rpc timeout");
  });
});

describe("importSoftwareInbox（软件收件箱启动导入）", () => {
  let inboxSpy: MockInstance<typeof ipc.tpInboxImport>;
  let listSpy: MockInstance<typeof ipc.tpList>;

  beforeEach(() => {
    inboxSpy = vi.spyOn(ipc, "tpInboxImport");
    listSpy = vi.spyOn(ipc, "tpList");
  });
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("有新增 → 重载列表（桌面即时可见）并如实返回计数与收件箱路径", async () => {
    inboxSpy.mockResolvedValue({ added: 3, path: "C:/Users/t/AppData/Roaming/com.variable.app/SoftwareInbox" } as never);
    listSpy.mockResolvedValue([] as never);

    const r = await importSoftwareInbox();
    expect(r).toEqual({
      added: 3,
      path: "C:/Users/t/AppData/Roaming/com.variable.app/SoftwareInbox",
    });
    expect(listSpy).toHaveBeenCalledTimes(1);
  });

  it("无新增 → 不重载列表（启动路径不制造噪音）", async () => {
    inboxSpy.mockResolvedValue({ added: 0, path: "D:/inbox" } as never);
    const r = await importSoftwareInbox();
    expect(r.added).toBe(0);
    expect(listSpy).not.toHaveBeenCalled();
  });

  it("重载失败不吞计数（added 仍如实返回；下次挂载再补）", async () => {
    inboxSpy.mockResolvedValue({ added: 2, path: "D:/inbox" } as never);
    listSpy.mockRejectedValue(new Error("rpc down"));
    const r = await importSoftwareInbox();
    expect(r.added).toBe(2);
  });

  it("导入命令异常向上抛出（调用方 toast 兜底）", async () => {
    inboxSpy.mockRejectedValue(new Error("os error 5"));
    await expect(importSoftwareInbox()).rejects.toThrow("os error 5");
  });
});
