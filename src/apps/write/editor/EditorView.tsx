import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { EditorContent, useEditor, type Editor } from "@tiptap/react";
import StarterKit from "@tiptap/starter-kit";
import Underline from "@tiptap/extension-underline";
import TextAlign from "@tiptap/extension-text-align";
import TextStyle from "@tiptap/extension-text-style";
import Color from "@tiptap/extension-color";
import Image from "@tiptap/extension-image";
import Placeholder from "@tiptap/extension-placeholder";
import { convertFileSrc } from "@tauri-apps/api/core";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open, save } from "@tauri-apps/plugin-dialog";
import {
  Bold, Italic, Underline as UIcon, Strikethrough, Heading1, Heading2, Heading3,
  List, ListOrdered, Quote, Minus, AlignLeft, AlignCenter, AlignRight, Palette,
  Undo2, Redo2, Star, Trash2, Download, FolderInput, Paperclip, Search, X,
  ChevronUp, ChevronDown, Type, Video, ImageIcon, FileText,
} from "lucide-react";
import { useI18n } from "../../../i18n";
import { ipc, errMessage } from "../../../lib/ipc";
import type { DocumentFull } from "../../../lib/types";
import { countWords, readingMinutes, stripHtmlToText, uid } from "../../../lib/format";
import { markdownToHtml } from "../../../lib/markdown";
import { sanitizeHtml } from "../../../lib/sanitize";
import { SaveCoordinator } from "../../../lib/saveQueue";
import { bumpDocList, pushToast, setSaveStatus, uiStore, useUi } from "../../../state/uiStore";
import { askConfirm } from "../../../components/Modal";
import { mindCardDataUrl, useXDropTarget, xfGet, type XfClip } from "../../../lib/xflow";
import { collectXrefs, checkStaleXrefs, openXref, parseXrefHref, xrefAnchorHtml, xrefHref, type StaleXref } from "../../../lib/xref";
import { XrefMark } from "./XrefMark";
import { xrefStaleExtension, setXrefStaleHighlight } from "./xrefStale";
import { FontSize } from "./FontSize";
import { VideoNode } from "./VideoNode";

const IMAGE_FILTERS = [{ name: "Images", extensions: ["png", "jpg", "jpeg", "webp", "gif", "bmp", "avif"] }];
const VIDEO_FILTERS = [{ name: "Videos", extensions: ["mp4", "webm", "ogv", "mov", "m4v"] }];
const ATTACH_FILTERS = [
  { name: "Documents & media", extensions: ["pdf", "txt", "md", "doc", "docx", "xls", "xlsx", "pptx", "csv", "zip", "7z", "png", "jpg", "jpeg", "webp", "gif", "mp4", "mp3", "wav"] },
];

interface DocPayload {
  id: string;
  title: string;
  contentHtml: string;
  contentText: string;
}

/** 本地文件 → 文档 的持久链接：同一 .md 反复打开时回到同一文档上下文。 */
const FILE_LINK_LS = "write.fileLink.v1";
function loadFileLinks(): Record<string, string> {
  try {
    const raw = localStorage.getItem(FILE_LINK_LS);
    if (raw) return JSON.parse(raw) as Record<string, string>;
  } catch { /* corrupt → empty */ }
  return {};
}
function saveFileLink(path: string, docId: string): void {
  try {
    const links = loadFileLinks();
    links[path] = docId;
    localStorage.setItem(FILE_LINK_LS, JSON.stringify(links));
  } catch { /* quota */ }
}

const saveCoordinator = new SaveCoordinator(async (payload: DocPayload) => {
  await ipc.saveDocument({ ...payload });
});

export interface EditorSettingsProps {
  fontFamily: string;
  fontSize: number;
  lineHeight: number;
  widthPct: number;
  align: "center" | "left" | "right";
  autosaveDelayMs: number;
  showStatusBar: boolean;
}

export function EditorView(props: { settings: EditorSettingsProps }): React.ReactElement {
  const { t, lang } = useI18n();
  const currentDocId = useUi((s) => s.currentDocId);
  const docListVersion = useUi((s) => s.docListVersion);
  const [doc, setDoc] = useState<DocumentFull | null>(null);
  const [title, setTitle] = useState("");
  const [tags, setTags] = useState<string[]>([]);
  const [allTags, setAllTags] = useState<string[]>([]);
  const [tagInput, setTagInput] = useState("");
  const [attachments, setAttachments] = useState<{ id: string; displayName: string; absPath: string; mediaType: string; missing: boolean }[]>([]);
  const [showAttachPop, setShowAttachPop] = useState(false);
  const [words, setWords] = useState(0);
  const [findOpen, setFindOpen] = useState(false);
  const [clockNow, setClockNow] = useState(() => new Date());
  /** 批次C（规格 5.7.3）：文档内跨软件引用的"内容已更新"清单。 */
  const [xrefStale, setXrefStale] = useState<StaleXref[]>([]);
  const [xrefBannerHidden, setXrefBannerHidden] = useState(false);
  const xrefStaleRef = useRef<Set<string>>(new Set());
  const dirtyRef = useRef(false);
  const saveTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const loadedIdRef = useRef<string | null>(null);

  // ---- TipTap ----
  const editor = useEditor({
    extensions: [
      StarterKit.configure({
        heading: { levels: [1, 2, 3] },
        history: { depth: 200 },
      }),
      Underline,
      TextAlign.configure({ types: ["heading", "paragraph"] }),
      TextStyle,
      Color,
      FontSize,
      VideoNode,
      Image.configure({ allowBase64: true }),
      Placeholder.configure({ placeholder: t("bodyPlaceholder") }),
      XrefMark,
      xrefStaleExtension(),
    ],
    content: "",
    autofocus: false,
    onUpdate: () => {
      markDirty();
    },
    editorProps: {
      handlePaste: (_view, event) => {
        // 批次C（规格 5.7.1）：Variable 富剪贴板优先 —— Mind 节点→图片、
        // Code 代码→代码块、Fate 角色→人物档案、Write 记录→正文。
        const clip = xfGet();
        if (clip && insertXfClip(clip)) return true;
        // 批次C（规格 5.7.3）：粘贴引用 —— Mind/Code 复制的 xref 链接 → 插入引用锚点
        const plain = event.clipboardData?.getData("text/plain") ?? "";
        const xm = /^xref:([^/\s]+)\/([^\s]+)\/(\d+)(?:\s+(.*))?$/.exec(plain.trim());
        if (xm) {
          const x = parseXrefHref(`xref:${xm[1]}/${xm[2]}/${xm[3]}`);
          if (x) {
            editor
              ?.chain()
              .focus()
              .insertContent(xrefAnchorHtml(x.kind, x.id, x.ver, (xm[4] ?? "").trim() || x.kind))
              .run();
            pushToast("success", t("xrefInserted"));
            return true;
          }
        }
        const items = event.clipboardData?.items ?? [];
        for (const item of items) {
          if (item.type.startsWith("image/")) {
            const file = item.getAsFile();
            if (file) {
              void insertPastedImage(file);
              return true;
            }
          }
        }
        return false;
      },
      handleClick: (_view, _pos, event) => {
        // 批次C（规格 5.7.3）：点击引用锚点 → 跳转目标软件（Mind 节点 / Write 文档）
        const target = event.target as HTMLElement | null;
        const anchor = target?.closest?.("a[href^='xref:']") as HTMLAnchorElement | null;
        if (!anchor) return false;
        const href = anchor.getAttribute("href") ?? "";
        const x = parseXrefHref(href);
        if (!x) return false;
        if (xrefStaleRef.current.has(href)) pushToast("info", t("xrefUpdatedToast"));
        void openXref(x);
        return true;
      },
    },
  });

  // ---- 批次C（规格 5.7.3）：引用过期校验 + 引用位置高亮同步 ----
  const refreshXrefStale = useCallback((html: string): void => {
    void checkStaleXrefs(collectXrefs(html))
      .then((stale) => setXrefStale(stale))
      .catch(() => {});
  }, []);

  useEffect(() => {
    if (!editor || editor.isDestroyed) return;
    const hrefs = new Set(xrefStale.map((s) => xrefHref(s.x.kind, s.x.id, s.x.ver)));
    xrefStaleRef.current = hrefs;
    setXrefStaleHighlight(editor.view, hrefs, t("xrefStaleBadge"));
  }, [xrefStale, editor, t]);

  // ---- loading / switching documents ----
  const flushPendingSave = useCallback(async (): Promise<void> => {
    if (!loadedIdRef.current || !dirtyRef.current || !editor) return;
    if (saveTimer.current) {
      clearTimeout(saveTimer.current);
      saveTimer.current = null;
    }
    await performSave();
  }, [editor]);

  useEffect(() => {
    let cancelled = false;
    async function load() {
      if (!editor) return;
      await flushPendingSave();
      if (cancelled) return;
      if (!currentDocId) {
        setDoc(null);
        loadedIdRef.current = null;
        return;
      }
      try {
        const d = await ipc.getDocument(currentDocId);
        if (cancelled) return;
        setDoc(d);
        setTitle(d.title);
        setTags(d.tags);
        setWords(countWords(d.contentText));
        dirtyRef.current = false;
        loadedIdRef.current = d.id;
        editor.commands.setContent(sanitizeHtml(d.contentHtml), false);
        try {
          const atts = await ipc.listAttachments(d.id, null);
          if (!cancelled) setAttachments(atts.map((a) => ({
            id: a.id, displayName: a.displayName, absPath: a.copied ? a.absPath : a.originalPath,
            mediaType: a.mediaType, missing: !a.copied && a.originalPath === "",
          })));
        } catch {
          setAttachments([]);
        }
        // 批次C（规格 5.7.3）：校验文档内跨软件引用，源已更新 → 顶部提示
        setXrefStale([]);
        setXrefBannerHidden(false);
        refreshXrefStale(d.contentHtml);
      } catch (e) {
        const err = errMessage(e);
        pushToast("error", t("saveFailed"), err.message);
        uiStore.setState({ currentDocId: null });
      }
    }
    void load();
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [currentDocId]); // load on document switch

  // ---- 打开本地 .md/.txt：定位到对应文档上下文（路径 ↔ 文档 的去重回链） ----
  const writePendingOpen = useUi((s) => s.writePendingOpen);
  useEffect(() => {
    if (!writePendingOpen || !editor) return;
    uiStore.setState({ writePendingOpen: null }); // consume once
    let cancelled = false;
    void (async () => {
      const norm = writePendingOpen.replace(/\\/g, "/");
      try {
        // 已打开过的文件 → 直接回到原文档，不重复建档。
        const existing = loadFileLinks()[norm];
        if (existing) {
          try {
            const d = await ipc.getDocument(existing);
            if (!cancelled) {
              uiStore.setState({ currentDocId: d.id });
              pushToast("info", t("located"), norm);
            }
            return;
          } catch { /* stale link → recreate below */ }
        }
        const text = await ipc.wsReadText(writePendingOpen);
        if (cancelled) return;
        const name = norm.split("/").pop() ?? "untitled";
        const html = markdownToHtml(text);
        const d = await ipc.createDocument(null, name.replace(/\.[^.]+$/, ""));
        await ipc.saveDocument({ id: d.id, title: d.title, contentHtml: html, contentText: stripHtmlToText(html) });
        saveFileLink(norm, d.id);
        bumpDocList();
        if (!cancelled) {
          uiStore.setState({ currentDocId: d.id });
          pushToast("success", t("openedFile"), norm);
        }
      } catch (e) {
        if (!cancelled) pushToast("error", t("openFailed"), errMessage(e).message);
      }
    })();
    return () => { cancelled = true; };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [writePendingOpen, editor]);

  // refresh tags list occasionally
  useEffect(() => {
    void ipc.listAllTags().then(setAllTags).catch(() => {});
  }, [docListVersion]);

  // live clock
  useEffect(() => {
    const i = setInterval(() => setClockNow(new Date()), 30_000);
    return () => clearInterval(i);
  }, []);

  // ---- saving ----
  function collectPayload(): DocPayload | null {
    if (!editor || !loadedIdRef.current) return null;
    const html = editor.getHTML();
    const text = stripHtmlToText(html);
    return { id: loadedIdRef.current, title, contentHtml: html, contentText: text };
  }

  async function performSave(): Promise<void> {
    const payload = collectPayload();
    if (!payload) return;
    const docId = payload.id;
    setSaveStatus(docId, "saving");
    try {
      await saveCoordinator.submit(docId, payload);
      if (uiStore.getState().currentDocId === docId) {
        dirtyRef.current = false;
        setSaveStatus(docId, "saved");
        // 批次C（规格 5.7.3）：引用随编辑增删/源更新 → 保存后重新校验
        refreshXrefStale(payload.contentHtml);
      }
      bumpDocList();
    } catch (e) {
      const err = errMessage(e);
      setSaveStatus(docId, "error");
      pushToast("error", t("saveFailed"), `${err.code}: ${err.message}`);
    }
  }

  function scheduleSave(): void {
    if (!loadedIdRef.current) return;
    setSaveStatus(loadedIdRef.current, "dirty");
    if (saveTimer.current) clearTimeout(saveTimer.current);
    saveTimer.current = setTimeout(() => {
      saveTimer.current = null;
      void performSave();
    }, props.settings.autosaveDelayMs);
  }

  function markDirty(): void {
    dirtyRef.current = true;
    if (!editor || !loadedIdRef.current) return;
    const html = editor.getHTML();
    setWordsThrottled(html);
    scheduleSave();
  }

  const wordTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  function setWordsThrottled(html: string): void {
    if (wordTimer.current) return;
    wordTimer.current = setTimeout(() => {
      wordTimer.current = null;
      setWords(countWords(stripHtmlToText(html)));
    }, 300);
  }

  // Ctrl+S listener
  useEffect(() => {
    const onKey = (e: KeyboardEvent): void => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "s") {
        e.preventDefault();
        void performSave().then(() => {
          if (loadedIdRef.current) pushToast("success", t("savedOk"));
        });
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [title]);

  // expose flush to the app close flow; clear debounce timers on unmount so no
  // callback fires against a dead component (contract-5: leak-free).
  useEffect(() => {
    const handler = () => performSave();
    window.addEventListener("variable:flush-save", handler);
    return () => {
      window.removeEventListener("variable:flush-save", handler);
      if (saveTimer.current) clearTimeout(saveTimer.current);
      if (wordTimer.current) clearTimeout(wordTimer.current);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    const newDocHandler = () => void createNewDoc();
    window.addEventListener("variable:new-doc", newDocHandler);
    return () => window.removeEventListener("variable:new-doc", newDocHandler);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  async function createNewDoc(): Promise<void> {
    try {
      await flushPendingSave();
      const d = await ipc.createDocument(null);
      bumpDocList();
      uiStore.setState({ currentDocId: d.id, mode: "write" });
    } catch (e) {
      pushToast("error", "Cannot create document", errMessage(e).message);
    }
  }

  // ---- media insertion ----
  /** 富剪贴板统一插入（粘贴与跨窗口拖放共用）。返回是否已消费。 */
  function insertXfClip(clip: XfClip): boolean {
    if (!editor) return false;
    try {
      if (clip.kind === "mind-node") {
        // 规格 5.7.1：Mind 节点 → 图片（确定性 SVG 卡片，零网络）
        const p = (clip.payload ?? {}) as { nodes?: { textPlain?: string }[] };
        const first = p.nodes?.[0];
        const raw = (first?.textPlain ?? clip.text ?? "").trim();
        if (!raw) return false;
        const lines = raw.split("\n");
        const src = mindCardDataUrl(lines[0] ?? "", lines.slice(1).join("\n"));
        editor.chain().focus().setImage({ src }).run();
        scheduleSave();
        return true;
      }
      if (clip.kind === "code-block") {
        const p = (clip.payload ?? {}) as { code?: string };
        const code = p.code ?? clip.text;
        if (!code.trim()) return false;
        editor.chain().focus().insertContent({
          type: "codeBlock",
          content: [{ type: "text", text: code }],
        }).run();
        scheduleSave();
        return true;
      }
      if (clip.kind === "fate-character") {
        // 规格 5.7.1：Fate 角色 → 人物档案（标题 + 逐行条目）
        const p = (clip.payload ?? {}) as { name?: string; text?: string };
        const name = (p.name ?? "").trim();
        const text = (p.text ?? clip.text ?? "").trim();
        if (!name && !text) return false;
        const items = text.split("\n").filter(Boolean)
          .map((l) => `<li>${l.replace(/[<>&"]/g, "")}</li>`).join("");
        editor.chain().focus().insertContent(
          `<h3>${name.replace(/[<>&"]/g, "")}</h3><ul>${items}</ul>`,
        ).run();
        scheduleSave();
        return true;
      }
      if (clip.kind === "write-record") {
        const p = (clip.payload ?? {}) as { contentHtml?: string };
        const html = p.contentHtml ?? "";
        if (!html.trim()) return false;
        editor.chain().focus().insertContent(sanitizeHtml(html)).run();
        scheduleSave();
        return true;
      }
      return false;
    } catch {
      return false;
    }
  }

  async function insertPastedImage(file: File): Promise<void> {
    try {
      const dataUrl = await fileToDataUrl(file);
      const att = await ipc.importDataUrl(dataUrl, `paste-${uid().slice(0, 8)}.png`);
      registerAttachment(att);
      const url = att.absPath ? convertFileSrc(att.absPath) : "";
      editor?.chain().focus().setImage({ src: url }).run();
    } catch (e) {
      pushToast("error", "Paste failed", errMessage(e).message);
    }
  }

  function fileToDataUrl(f: File): Promise<string> {
    return new Promise((resolve, reject) => {
      const r = new FileReader();
      r.onload = () => resolve(String(r.result));
      r.onerror = reject;
      r.readAsDataURL(f);
    });
  }

  function registerAttachment(att: { id: string; displayName: string; absPath: string; originalPath: string; copied: boolean; mediaType: string; relPath: string }): void {
    setAttachments((prev) => [
      ...prev.filter((p) => p.id !== att.id),
      { id: att.id, displayName: att.displayName, absPath: att.copied ? att.absPath : att.originalPath, mediaType: att.mediaType, missing: false },
    ]);
  }

  async function pickAndInsert(kind: "image" | "video" | "file"): Promise<void> {
    if (!editor || !doc) return;
    try {
      const selected = await open({
        multiple: false,
        filters: kind === "image" ? IMAGE_FILTERS : kind === "video" ? VIDEO_FILTERS : ATTACH_FILTERS,
      });
      if (typeof selected !== "string") return;
      const atts = await ipc.importMedia({ paths: [selected], mode: "copy", documentId: doc.id });
      const att = atts[0];
      if (!att) return;
      registerAttachment(att);
      const url = convertFileSrc(att.absPath);
      if (att.mediaType === "image") {
        editor.chain().focus().setImage({ src: url }).run();
      } else if (att.mediaType === "video") {
        editor.chain().focus().setVideo(url).run();
      } else {
        pushToast("info", t("attachments"), att.displayName);
      }
      scheduleSave();
    } catch (e) {
      pushToast("error", "Insert failed", errMessage(e).message);
    }
  }

  // ---- 批次C（规格 5.7.2）跨软件拖放落点 ----
  // Mind 节点→图片卡片、文件→复制进资源库并嵌入、Write 记录→嵌入正文。
  const dropHot = useXDropTarget(["mind-node", "file", "write-record"], (clip, cx, cy) => {
    void (async () => {
      if (!editor) return;
      // 光标定位到释放点附近（可解析时），再执行插入
      const coords = editor.view.posAtCoords({ left: cx, top: cy });
      if (coords) editor.commands.setTextSelection(coords.pos);
      if (clip.kind === "mind-node" || clip.kind === "write-record" || clip.kind === "fate-character" || clip.kind === "code-block") {
        if (!insertXfClip(clip)) pushToast("info", t("attachments"), clip.text.slice(0, 60));
        return;
      }
      if (clip.kind === "file") {
        if (!doc) return;
        const p = (clip.payload ?? {}) as { path?: string };
        if (!p.path) return;
        try {
          const atts = await ipc.importMedia({ paths: [p.path], mode: "copy", documentId: doc.id });
          const att = atts[0];
          if (!att) return;
          registerAttachment(att);
          const url = convertFileSrc(att.absPath);
          if (att.mediaType === "image") editor.chain().focus().setImage({ src: url }).run();
          else if (att.mediaType === "video") editor.chain().focus().setVideo(url).run();
          else editor.chain().focus().insertContent(`<p>📎 ${att.displayName.replace(/[<>&"]/g, "")}</p>`).run();
          scheduleSave();
        } catch (e) {
          pushToast("error", "Insert failed", errMessage(e).message);
        }
      }
    })();
  });

  // ---- 批次C（规格 5.7.2）外部文件拖入（Windows 资源管理器 → Variable）----
  // 任意文件复制进 Variable 资源目录（importMedia mode:"copy"），图片/视频嵌入正文。
  useEffect(() => {
    if (!editor) return undefined;
    let disposed = false;
    let un: (() => void) | undefined;
    const p = getCurrentWebview().onDragDropEvent(async (event) => {
      try {
        if (event.payload.type !== "drop" || !doc) return;
        const paths: string[] = [...(event.payload.paths ?? [])];
        if (paths.length === 0) return;
        let inserted = 0;
        for (const path of paths.slice(0, 6)) {
          const atts = await ipc.importMedia({ paths: [path], mode: "copy", documentId: doc.id });
          const att = atts[0];
          if (!att) continue;
          registerAttachment(att);
          const url = convertFileSrc(att.absPath);
          if (att.mediaType === "image") editor.chain().focus().setImage({ src: url }).run();
          else if (att.mediaType === "video") editor.chain().focus().setVideo(url).run();
          else editor.chain().focus().insertContent(`<p>📎 ${att.displayName.replace(/[<>&"]/g, "")}</p>`).run();
          inserted++;
        }
        if (inserted > 0) {
          scheduleSave();
          pushToast("success", t("attachments"), lang !== "en" ? `已复制 ${inserted} 个文件到 Variable 资源库` : `Copied ${inserted} file(s) into Variable`);
        }
      } catch (e) {
        pushToast("error", "Insert failed", errMessage(e).message);
      }
    });
    void p.then((u) => {
      if (disposed) u();
      else un = u;
    }).catch(() => {});
    return () => {
      disposed = true;
      un?.();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [editor, doc]);

  // ---- title / meta actions ----
  async function onTitleChange(v: string): Promise<void> {
    setTitle(v);
    dirtyRef.current = true;
    scheduleSave();
  }

  async function toggleFavorite(): Promise<void> {
    if (!doc) return;
    const next = !doc.favorite;
    setDoc({ ...doc, favorite: next });
    await ipc.setFavorite(doc.id, next).catch((e) => pushToast("error", "Failed", errMessage(e).message));
    bumpDocList();
  }

  async function addTag(): Promise<void> {
    const name = tagInput.trim();
    if (!name || !doc) return;
    if (name.length > 40) {
      pushToast("error", t("filterTag"), "tag ≤ 40 chars");
      return;
    }
    const next = Array.from(new Set([...tags, name]));
    setTags(next);
    setTagInput("");
    try {
      await ipc.setTags(doc.id, next);
      bumpDocList();
    } catch (e) {
      pushToast("error", "Tag failed", errMessage(e).message);
    }
  }

  async function removeTag(name: string): Promise<void> {
    if (!doc) return;
    const next = tags.filter((x) => x !== name);
    setTags(next);
    await ipc.setTags(doc.id, next).catch(() => {});
    bumpDocList();
  }

  async function trashCurrent(): Promise<void> {
    if (!doc) return;
    const ok = await askConfirm({
      title: t("deleteToTrash"),
      body: lang !== "en" ? `将「${doc.title || "无标题"}」移入回收站？` : `Move "${doc.title || "Untitled"}" to trash?`,
      danger: true,
      okLabel: t("delete"),
    });
    if (!ok) return;
    await flushPendingSave();
    await ipc.trashDocument(doc.id);
    bumpDocList();
    uiStore.setState({ currentDocId: null });
  }

  async function exportDoc(): Promise<void> {
    if (!doc) return;
    await flushPendingSave();
    const choice = await openContextMenuAsyncForExport(lang);
    if (!choice) return;
    const ext: "md" | "html" | "txt" | "json" = choice;
    const path = await save({
      defaultPath: `${sanitizeFileName(doc.title || "untitled")}.${ext}`,
      filters: [{ name: choice.toUpperCase(), extensions: [ext] }],
    });
    if (!path) return;
    try {
      await ipc.exportDocuments([doc.id], ext as "md" | "html" | "txt" | "json", path);
      pushToast("success", t("exportedOk"), path);
    } catch (e) {
      pushToast("error", "Export failed", errMessage(e).message);
    }
  }

  // ---- find in document ----
  function openFind(): void {
    setFindOpen(true);
  }

  // date line
  const dateLine = useMemo(() => formatLongDateLocal(clockNow.getTime(), lang), [clockNow, lang]);
  const timeLine = `${String(clockNow.getHours()).padStart(2, "0")}:${String(clockNow.getMinutes()).padStart(2, "0")}`;

  const status = useUi((s) => (currentDocId ? s.saveStatuses[currentDocId] ?? "saved" : "saved"));
  const focusMode = useUi((s) => s.focusMode);

  return (
    <div className="editor-page">
      <div className={`writing-col align-${props.settings.align}`} style={{ width: `${props.settings.widthPct}%`, maxWidth: `${props.settings.widthPct}%` }}>
        <div className="date-line">
          <span>{dateLine}</span>
          <span className="clock">{timeLine}</span>
        </div>

        {!doc ? (
          <EmptyState onCreate={() => void createNewDoc()} />
        ) : (
          <>
            <input
              className="title-input"
              value={title}
              placeholder={t("titlePlaceholder")}
              aria-label={t("titlePlaceholder")}
              maxLength={300}
              onChange={(e) => void onTitleChange(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") {
                  e.preventDefault();
                  editor?.commands.focus("start");
                }
                e.stopPropagation();
              }}
            />
            <div className="tag-row">
              {tags.map((tg) => (
                <button key={tg} type="button" className="tag-chip" onClick={() => void removeTag(tg)} title={t("delete")}>
                  #{tg} ✕
                </button>
              ))}
              <input
                className="tag-input"
                value={tagInput}
                placeholder={t("tagsPlaceholder")}
                list="all-tags"
                onChange={(e) => setTagInput(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") {
                    e.preventDefault();
                    void addTag();
                  }
                  e.stopPropagation();
                }}
              />
              <datalist id="all-tags">
                {allTags.map((tg) => (
                  <option key={tg} value={tg} />
                ))}
              </datalist>
            </div>
            {/* 批次C（规格 5.7.3）：引用对象更新提示；位置高亮见 xrefStale 装饰 */}
            {xrefStale.length > 0 && !xrefBannerHidden && (
              <div className="xref-banner" role="status">
                <span className="xb-text">{t("xrefStaleBanner", { n: xrefStale.length })}</span>
                <button
                  type="button"
                  className="xb-close"
                  onClick={() => setXrefBannerHidden(true)}
                  aria-label={t("close")}
                >
                  ×
                </button>
              </div>
            )}

            <EditorToolbar editor={editor} onPickImage={() => void pickAndInsert("image")} onPickVideo={() => void pickAndInsert("video")} onPickFile={() => void pickAndInsert("file")} />

            <div className={`editor-scroll${dropHot ? " xf-hot" : ""}`}>
              <EditorContent
                editor={editor}
                className="prose-host"
                style={{ fontSize: props.settings.fontSize, lineHeight: props.settings.lineHeight }}
              />
            </div>

            <SlashMenu editor={editor} />
            <FindBar editor={editor} open={findOpen} onClose={() => setFindOpen(false)} />
          </>
        )}
      </div>

      {/* right-side floating doc actions */}
      {doc && !focusMode && (
        <div className="doc-actions">
          <button type="button" className={`icon-btn ${doc.favorite ? "active star" : ""}`} data-tip={doc.favorite ? t("unfavorite") : t("favorite")} aria-label={t("favorite")} onClick={() => void toggleFavorite()}>
            <Star size={16} fill={doc.favorite ? "currentColor" : "none"} />
          </button>
          <button type="button" className="icon-btn" data-tip={t("export")} aria-label={t("export")} onClick={() => void exportDoc()}>
            <Download size={16} />
          </button>
          <MoveMenu docId={doc.id} hasFolder={!!doc.folderId} onChanged={() => bumpDocList()} />
          <button type="button" className="icon-btn" data-tip={`${t("attachments")} (${attachments.length})`} aria-label={t("attachments")} onClick={() => setShowAttachPop(!showAttachPop)}>
            <Paperclip size={16} />
          </button>
          <button type="button" className="icon-btn" data-tip={t("findInDoc")} aria-label={t("findInDoc")} onClick={openFind}>
            <Search size={16} />
          </button>
          <button type="button" className="icon-btn danger-hover" data-tip={t("deleteToTrash")} aria-label={t("deleteToTrash")} onClick={() => void trashCurrent()}>
            <Trash2 size={16} />
          </button>
          {showAttachPop && (
            <AttachmentPopup
              items={attachments}
              onClose={() => setShowAttachPop(false)}
              onRelocate={async (id) => {
                const p = await open({ multiple: false });
                if (typeof p !== "string") return;
                const updated = await ipc.resolveMediaPath(id, p);
                setAttachments((prev) => prev.map((a) => (a.id === id ? { ...a, absPath: updated.copied ? updated.absPath : updated.originalPath, missing: false } : a)));
              }}
              onMissing={(id) =>
                setAttachments((prev) => prev.map((a) => (a.id === id ? { ...a, missing: true } : a)))
              }
            />
          )}
        </div>
      )}

      {props.settings.showStatusBar && !focusMode && (
        <footer className="status-bar">
          <span>{words} {t("words")}</span>
          <span className="sep">·</span>
          <span>{t("readTime", { n: readingMinutes(words) })}</span>
          <span className="flex-1" />
          <span className={`save-text ${status}`}>
            {status === "saved"
              ? t("savedLocally")
              : status === "saving"
                ? t("saving")
                : status === "error"
                  ? t("saveFailed")
                  : t("unsaved")}
          </span>
        </footer>
      )}
    </div>
  );
}

// ---------- sub components ----------

function EmptyState(props: { onCreate: () => void }): React.ReactElement {
  const { t } = useI18n();
  return (
    <div className="empty-editor">
      <p className="dim">{lang_now()}</p>
      <button type="button" className="btn primary" onClick={props.onCreate}>
        {t("newRecord")}
      </button>
    </div>
  );
}
function lang_now(): string {
  return document.documentElement.lang === "en"
    ? "No record open. Create one to start writing."
    : "当前没有打开的记录，新建一条即可开始书写。";
}

function formatLongDateLocal(ts: number, lang: string): string {
  const d = new Date(ts);
  const wdEn = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"][d.getDay()];
  const wdZh = ["星期日", "星期一", "星期二", "星期三", "星期四", "星期五", "星期六"][d.getDay()];
  const moEn = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"][d.getMonth()];
  return lang !== "en"
    ? `${d.getFullYear()}年${d.getMonth() + 1}月${d.getDate()}日 · ${wdZh}`
    : `${wdEn}, ${moEn} ${d.getDate()}`;
}

function sanitizeFileName(s: string): string {
  return s.replace(/[\\/:*?"<>|]/g, "_").slice(0, 80);
}

async function openContextMenuAsyncForExport(lang: string): Promise<"md" | "html" | "txt" | "json" | null> {
  return await new Promise((resolve) => {
    void import("../../../components/ContextMenu").then(({ openContextMenu }) => {
      openContextMenu(window.innerWidth / 2 - 60, 140, [
        { label: "Markdown (.md)", onClick: () => resolve("md") },
        { label: "HTML (.html)", onClick: () => resolve("html") },
        { label: lang !== "en" ? "纯文本 (.txt)" : "Plain text (.txt)", onClick: () => resolve("txt") },
        { label: "JSON (.json)", onClick: () => resolve("json") },
        { label: lang !== "en" ? "取消" : "Cancel", onClick: () => resolve(null) },
      ]);
    });
    // Fallback resolve when menu closes without selection:
    setTimeout(() => resolve(null), 60_000);
  });
}

function MoveMenu(props: { docId: string; hasFolder: boolean; onChanged: () => void }): React.ReactElement {
  const { t, lang } = useI18n();
  void props.hasFolder;
  return (
    <button
      type="button"
      className="icon-btn"
      data-tip={t("moveToFolder")}
      aria-label={t("moveToFolder")}
      onClick={async (e) => {
        const folders = await ipc.listFolders().catch((): import("../../../lib/types").Folder[] => []);
        const { openContextMenu } = await import("../../../components/ContextMenu");
        const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
        const folderItems: import("../../../components/ContextMenu").MenuItem[] = [
          {
            label: t("noFolder"),
            onClick: async () => {
              await ipc.moveDocument(props.docId, null);
              props.onChanged();
            },
          },
          ...(folders.length > 0 ? ([{ separator: true }] as import("../../../components/ContextMenu").MenuItem[]) : []),
          ...folders.map((f) => ({
            label: f.name,
            onClick: async () => {
              await ipc.moveDocument(props.docId, f.id);
              props.onChanged();
            },
          })),
        ];
        const items: import("../../../components/ContextMenu").MenuItem[] =
          folderItems.length > 1 ? folderItems : [{ label: lang !== "en" ? "（暂无文件夹，可在列表中新建）" : "(no folders yet)", disabled: true }];
        openContextMenu(r.left - 180, r.bottom + 4, items);
      }}
    >
      <FolderInput size={16} />
    </button>
  );
}

function AttachmentPopup(props: {
  items: { id: string; displayName: string; absPath: string; mediaType: string; missing: boolean }[];
  onClose: () => void;
  onRelocate: (id: string) => Promise<void>;
  onMissing: (id: string) => void;
}): React.ReactElement {
  const { t, lang } = useI18n();
  useEffect(() => {
    // verify existence of referenced files once opened
    props.items.forEach(async (a) => {
      if (a.mediaType === "image" || a.mediaType === "video") return;
      const res = await ipc.checkPaths([a.absPath]).catch(() => []);
      if (res.length > 0 && res[0] && !res[0].exists) props.onMissing(a.id);
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [props.items.length]);
  return (
    <div className="attach-pop card-pop">
      <div className="row between">
        <strong>{t("attachments")}</strong>
        <button type="button" className="icon-btn tiny" onClick={props.onClose} aria-label="close"><X size={12} /></button>
      </div>
      {props.items.length === 0 && <p className="dim small">—</p>}
      {props.items.map((a) => (
        <div key={a.id} className="attach-row">
          {a.mediaType === "image" ? <ImageIcon size={14} /> : a.mediaType === "video" ? <Video size={14} /> : <FileText size={14} />}
          <span className="attach-name ellipsis" title={a.displayName}>{a.displayName}</span>
          {a.missing ? (
            <>
              <span className="missing-tag">{t("missingMedia")}</span>
              <button type="button" className="btn tiny" onClick={() => void props.onRelocate(a.id)}>{t("relocate")}</button>
            </>
          ) : (
            <>
              <button type="button" className="btn tiny ghost" onClick={() => void ipc.openPath(a.absPath).catch((e) => pushToast("error", t("mediaLoadError"), errMessage(e).message))}>
                {t("openFile")}
              </button>
              <button type="button" className="btn tiny ghost" onClick={() => void ipc.revealPath(a.absPath).catch(() => {})}>
                {lang !== "en" ? "定位" : "Reveal"}
              </button>
            </>
          )}
        </div>
      ))}
    </div>
  );
}

// ---------- toolbar ----------

function EditorToolbar(props: {
  editor: Editor | null;
  onPickImage: () => void;
  onPickVideo: () => void;
  onPickFile: () => void;
}): React.ReactElement | null {
  const { t } = useI18n();
  const ed = props.editor;
  const [, force] = useState(0);
  useEffect(() => {
    if (!ed) return;
    const rerender = () => force((n) => n + 1);
    ed.on("selectionUpdate", rerender);
    ed.on("transaction", rerender);
    return () => {
      ed.off("selectionUpdate", rerender);
      ed.off("transaction", rerender);
    };
  }, [ed]);
  if (!ed) return null;

  const sizes = [12, 14, 16, 18, 20, 24, 28];
  return (
    <div className="editor-toolbar" role="toolbar" aria-label="formatting">
      <ToolBtn icon={<Undo2 size={15} />} tip={t("undo")} disabled={!ed.can().undo()} onClick={() => ed.chain().focus().undo().run()} />
      <ToolBtn icon={<Redo2 size={15} />} tip={t("redo")} disabled={!ed.can().redo()} onClick={() => ed.chain().focus().redo().run()} />
      <span className="tb-sep" />
      <ToolBtn icon={<Bold size={15} />} tip={t("bold")} active={ed.isActive("bold")} onClick={() => ed.chain().focus().toggleBold().run()} />
      <ToolBtn icon={<Italic size={15} />} tip={t("italic")} active={ed.isActive("italic")} onClick={() => ed.chain().focus().toggleItalic().run()} />
      <ToolBtn icon={<UIcon size={15} />} tip={t("underline")} active={ed.isActive("underline")} onClick={() => ed.chain().focus().toggleUnderline().run()} />
      <ToolBtn icon={<Strikethrough size={15} />} tip={t("strike")} active={ed.isActive("strike")} onClick={() => ed.chain().focus().toggleStrike().run()} />
      <span className="tb-sep" />
      <ToolBtn icon={<Heading1 size={15} />} tip={t("h1")} active={ed.isActive("heading", { level: 1 })} onClick={() => ed.chain().focus().toggleHeading({ level: 1 }).run()} />
      <ToolBtn icon={<Heading2 size={15} />} tip={t("h2")} active={ed.isActive("heading", { level: 2 })} onClick={() => ed.chain().focus().toggleHeading({ level: 2 }).run()} />
      <ToolBtn icon={<Heading3 size={15} />} tip={t("h3")} active={ed.isActive("heading", { level: 3 })} onClick={() => ed.chain().focus().toggleHeading({ level: 3 }).run()} />
      <span className="tb-sep" />
      <ToolBtn icon={<List size={15} />} tip={t("bulletList")} active={ed.isActive("bulletList")} onClick={() => ed.chain().focus().toggleBulletList().run()} />
      <ToolBtn icon={<ListOrdered size={15} />} tip={t("orderedList")} active={ed.isActive("orderedList")} onClick={() => ed.chain().focus().toggleOrderedList().run()} />
      <ToolBtn icon={<Quote size={15} />} tip={t("blockquote")} active={ed.isActive("blockquote")} onClick={() => ed.chain().focus().toggleBlockquote().run()} />
      <ToolBtn icon={<Minus size={15} />} tip={t("hr")} onClick={() => ed.chain().focus().setHorizontalRule().run()} />
      <span className="tb-sep" />
      <ColorPicker editor={ed} />
      <label className="size-select" data-tip={t("fontSize")} aria-label={t("fontSize")}>
        <Type size={13} />
        <select
          value=""
          onChange={(e) => {
            const v = e.target.value;
            if (v === "reset") ed.chain().focus().setMark("textStyle", { fontSize: null }).run();
            else ed.chain().focus().setFontSize(Number(v)).run();
          }}
        >
          <option value="" hidden>Aa</option>
          <option value="reset">—</option>
          {sizes.map((s) => (
            <option key={s} value={s}>{s}</option>
          ))}
        </select>
      </label>
      <ToolBtn icon={<AlignLeft size={15} />} tip={t("alignLeft")} active={ed.isActive({ textAlign: "left" })} onClick={() => ed.chain().focus().setTextAlign("left").run()} />
      <ToolBtn icon={<AlignCenter size={15} />} tip={t("alignCenter")} active={ed.isActive({ textAlign: "center" })} onClick={() => ed.chain().focus().setTextAlign("center").run()} />
      <ToolBtn icon={<AlignRight size={15} />} tip={t("alignRight")} active={ed.isActive({ textAlign: "right" })} onClick={() => ed.chain().focus().setTextAlign("right").run()} />
      <span className="tb-sep" />
      <ToolBtn icon={<ImageIcon size={15} />} tip={t("insertImage")} onClick={props.onPickImage} />
      <ToolBtn icon={<Video size={15} />} tip={t("insertVideo")} onClick={props.onPickVideo} />
      <ToolBtn icon={<Paperclip size={15} />} tip={t("attachFile")} onClick={props.onPickFile} />
    </div>
  );
}

function ToolBtn(props: { icon: React.ReactNode; tip: string; active?: boolean; disabled?: boolean; onClick: () => void }): React.ReactElement {
  return (
    <button
      type="button"
      className={`tool-btn ${props.active ? "active" : ""}`}
      data-tip={props.tip}
      aria-label={props.tip}
      aria-pressed={props.active}
      disabled={props.disabled}
      onMouseDown={(e) => e.preventDefault()}
      onClick={props.onClick}
    >
      {props.icon}
    </button>
  );
}

const SWATCHES = ["#e6ecf7", "#9db8e8", "#5b7bd0", "#7fc8a9", "#e8c26b", "#e07f7f", "#c68fe0", "#8a93a6"];

function ColorPicker(props: { editor: Editor }): React.ReactElement {
  const { t } = useI18n();
  const [open, setOpen] = useState(false);
  return (
    <span className="color-wrap">
      <ToolBtn icon={<Palette size={15} />} tip={t("color")} active={open} onClick={() => setOpen(!open)} />
      {open && (
        <span className="swatch-pop">
          {SWATCHES.map((c) => (
            <button
              key={c}
              type="button"
              className="swatch"
              style={{ background: c }}
              aria-label={`${t("color")} ${c}`}
              onClick={() => {
                props.editor.chain().focus().setColor(c).run();
                setOpen(false);
              }}
            />
          ))}
          <button type="button" className="swatch reset" onClick={() => { props.editor.chain().focus().unsetColor().run(); setOpen(false); }}>✕</button>
        </span>
      )}
    </span>
  );
}

// ---------- slash menu ----------

interface SlashItem {
  label: string;
  keywords: string;
  run: (ed: Editor) => void;
}

function SlashMenu(props: { editor: Editor | null }): React.ReactElement | null {
  const { t, lang } = useI18n();
  const [state, setState] = useState<{ pos: number; x: number; y: number; query: string } | null>(null);
  const [idx, setIdx] = useState(0);
  const ed = props.editor;

  const items: SlashItem[] = useMemo(
    () => [
      { label: "H1", keywords: "h1 heading 标题", run: (e) => e.chain().focus().toggleHeading({ level: 1 }).run() },
      { label: "H2", keywords: "h2 heading 标题", run: (e) => e.chain().focus().toggleHeading({ level: 2 }).run() },
      { label: "H3", keywords: "h3 heading 标题", run: (e) => e.chain().focus().toggleHeading({ level: 3 }).run() },
      { label: lang !== "en" ? "无序列表" : "Bullet list", keywords: "list ul 列表", run: (e) => e.chain().focus().toggleBulletList().run() },
      { label: lang !== "en" ? "有序列表" : "Numbered list", keywords: "list ol 编号", run: (e) => e.chain().focus().toggleOrderedList().run() },
      { label: lang !== "en" ? "引用" : "Quote", keywords: "quote 引用", run: (e) => e.chain().focus().toggleBlockquote().run() },
      { label: lang !== "en" ? "分割线" : "Divider", keywords: "hr divider 分割线", run: (e) => e.chain().focus().setHorizontalRule().run() },
      { label: lang !== "en" ? "代码块" : "Code block", keywords: "code 代码", run: (e) => e.chain().focus().setCodeBlock().run() },
    ],
    [lang],
  );

  useEffect(() => {
    if (!ed) return;
    const onUpdate = () => {
      const { state: st } = ed;
      const { $from } = st.selection;
      const textBefore = $from.parent.textBetween(0, $from.parentOffset, undefined, "\ufffc");
      const m = /(?:^|\s)\/([^\s/]*)$/.exec(textBefore);
      if (m && m[1] !== undefined) {
        const coords = ed.view.coordsAtPos(st.selection.from);
        setState({ pos: st.selection.from, x: coords.left, y: coords.bottom + 6, query: m[1].toLowerCase() });
        setIdx(0);
      } else {
        setState(null);
      }
    };
    ed.on("update", onUpdate);
    ed.on("selectionUpdate", onUpdate);
    return () => {
      ed.off("update", onUpdate);
      ed.off("selectionUpdate", onUpdate);
    };
  }, [ed]);

  useEffect(() => {
    if (!state || !ed) return;
    const filtered = items.filter((it) => it.label.toLowerCase().includes(state.query) || it.keywords.includes(state.query));
    const onKey = (e: KeyboardEvent): void => {
      if (filtered.length === 0) return;
      if (e.key === "ArrowDown") {
        e.preventDefault();
        setIdx((i) => (i + 1) % filtered.length);
      } else if (e.key === "ArrowUp") {
        e.preventDefault();
        setIdx((i) => (i - 1 + filtered.length) % filtered.length);
      } else if (e.key === "Enter") {
        e.preventDefault();
        apply(filtered[Math.min(idx, filtered.length - 1)]);
      } else if (e.key === "Escape") {
        setState(null);
      }
    };
    const apply = (item?: SlashItem) => {
      if (!item || !ed || state === null) return;
      // remove "/query"
      const from = Math.max(0, state.pos - state.query.length - 1);
      ed.chain().focus().deleteRange({ from, to: state.pos }).run();
      item.run(ed);
      setState(null);
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [state, idx, items, ed]);

  if (!state || !ed) return null;
  const filtered = items.filter((it) => it.label.toLowerCase().includes(state.query) || it.keywords.includes(state.query));
  if (filtered.length === 0) return null;
  return (
    <div className="slash-menu" style={{ left: Math.min(state.x, window.innerWidth - 220), top: state.y }} role="listbox">
      <div className="slash-hint dim">{t("slashHint")}</div>
      {filtered.map((it, i) => (
        <button
          key={it.label}
          type="button"
          role="option"
          aria-selected={i === idx}
          className={`slash-item ${i === idx ? "sel" : ""}`}
          onMouseEnter={() => setIdx(i)}
          onClick={() => {
            const from = Math.max(0, state.pos - state.query.length - 1);
            ed.chain().focus().deleteRange({ from, to: state.pos }).run();
            it.run(ed);
            setState(null);
          }}
        >
          {it.label}
        </button>
      ))}
    </div>
  );
}

// ---------- find bar ----------

interface TextPos {
  index: number; // prosemirror position
  plainIndex: number;
}

function buildPlainText(ed: Editor): { text: string; positions: TextPos[] } {
  let text = "";
  const positions: TextPos[] = [];
  ed.state.doc.descendants((node, pos) => {
    if (node.isText && node.text) {
      positions.push({ index: pos, plainIndex: text.length });
      text += node.text;
    } else if (node.isBlock && text.length > 0 && !text.endsWith("\n")) {
      // block boundaries count as newline for search continuity
      text += "\n";
    }
    return true;
  });
  return { text, positions };
}

function plainToProsemirror(positions: TextPos[], plainIdx: number): number | null {
  let best: TextPos | null = null;
  for (const p of positions) {
    if (p.plainIndex <= plainIdx) best = p;
    else break;
  }
  if (!best) return null;
  return best.index + (plainIdx - best.plainIndex);
}

function FindBar(props: { editor: Editor | null; open: boolean; onClose: () => void }): React.ReactElement | null {
  const { t } = useI18n();
  const [query, setQuery] = useState("");
  const [count, setCount] = useState(0);
  const [current, setCurrent] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (props.open) inputRef.current?.focus();
  }, [props.open]);

  const matches = useMemo(() => {
    if (!props.editor || query.length === 0) return [] as number[];
    const { text } = buildPlainText(props.editor);
    const lower = text.toLowerCase();
    const q = query.toLowerCase();
    const out: number[] = [];
    let i = lower.indexOf(q);
    while (i !== -1 && out.length < 500) {
      out.push(i);
      i = lower.indexOf(q, i + q.length);
    }
    return out;
  }, [query, props.editor, count === -1]);

  useEffect(() => {
    setCount(matches.length);
    setCurrent(matches.length > 0 ? 0 : -1);
  }, [matches]);

  function jump(i: number): void {
    if (!props.editor || matches.length === 0) return;
    const wrapped = ((i % matches.length) + matches.length) % matches.length;
    const at = matches[wrapped];
    if (at === undefined) return;
    setCurrent(wrapped);
    const start = plainToProsemirror(buildPlainText(props.editor).positions, at);
    if (start !== null) {
      props.editor.chain().setTextSelection({ from: start, to: start + query.length }).focus().scrollIntoView().run();
    }
  }

  if (!props.open) return null;
  return (
    <div className="find-bar" role="search">
      <Search size={14} />
      <input
        ref={inputRef}
        value={query}
        placeholder={t("findInDoc")}
        onChange={(e) => setQuery(e.target.value)}
        onKeyDown={(e) => {
          e.stopPropagation();
          if (e.key === "Enter") jump(current + (e.shiftKey ? -1 : 1));
          if (e.key === "Escape") props.onClose();
        }}
      />
      <span className="dim small">
        {matches.length > 0 ? t("matchOf", { i: current + 1, n: matches.length }) : query ? t("noMatch") : ""}
      </span>
      <button type="button" className="icon-btn tiny" aria-label="prev" onClick={() => jump(current - 1)}><ChevronUp size={13} /></button>
      <button type="button" className="icon-btn tiny" aria-label="next" onClick={() => jump(current + 1)}><ChevronDown size={13} /></button>
      <button type="button" className="icon-btn tiny" aria-label="close" onClick={props.onClose}><X size={13} /></button>
    </div>
  );
}
