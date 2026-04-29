import { useLayoutEffect, useRef, useState, useSyncExternalStore, useCallback } from "react";
import type { LogStore } from "./LogStore";

const CHUNK_SIZE = 150;
const OVERSCAN_CHUNKS = 2;
const EST_LINE_HEIGHT = 24;

export function VirtualLog({ store }: { store: LogStore }) {
  // Subscribe to store updates — version number is the snapshot
  const version = useSyncExternalStore(store.subscribe, store.getSnapshot);

  const containerRef = useRef<HTMLDivElement>(null);
  const [scrollTop, setScrollTop] = useState(0);
  const [viewHeight, setViewHeight] = useState(0);
  const stickToBottomRef = useRef(true);
  const [showJumpBottom, setShowJumpBottom] = useState(false);
  const chunkHeights = useRef<Map<number, number>>(new Map());
  const chunkRefs = useRef<Map<number, HTMLDivElement>>(new Map());
  const [measureGen, setMeasureGen] = useState(0);

  // Read lines directly from the store (no copy, no split)
  const lines = store.lines;
  const lineCount = lines.length;

  // Reset measurements when line count drops (new run)
  const prevLineCount = useRef(0);
  if (lineCount < prevLineCount.current) {
    chunkHeights.current.clear();
  }
  prevLineCount.current = lineCount;

  const chunkCount = Math.ceil(lineCount / CHUNK_SIZE);
  const estChunkHeight = CHUNK_SIZE * EST_LINE_HEIGHT;

  const getChunkHeight = useCallback(
    (i: number) => chunkHeights.current.get(i) ?? estChunkHeight,
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [estChunkHeight, measureGen],
  );

  const chunkTops: number[] = [];
  let totalHeight = 0;
  for (let i = 0; i < chunkCount; i++) {
    chunkTops.push(totalHeight);
    totalHeight += getChunkHeight(i);
  }

  // Track container size
  useLayoutEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    const ro = new ResizeObserver(() => setViewHeight(el.clientHeight));
    ro.observe(el);
    setViewHeight(el.clientHeight);
    return () => ro.disconnect();
  }, []);

  // Auto-scroll to bottom when new content arrives
  useLayoutEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    if (stickToBottomRef.current) {
      el.scrollTop = el.scrollHeight;
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [version, totalHeight]);

  // Measure rendered chunks
  useLayoutEffect(() => {
    let changed = false;
    chunkRefs.current.forEach((el, idx) => {
      const h = Math.round(el.getBoundingClientRect().height);
      if (h > 0 && chunkHeights.current.get(idx) !== h) {
        chunkHeights.current.set(idx, h);
        changed = true;
      }
    });
    if (changed) {
      setMeasureGen((g) => g + 1);
    }
  });

  function onScroll() {
    const el = containerRef.current;
    if (!el) return;
    setScrollTop(el.scrollTop);
    const atBottom = el.scrollTop + el.clientHeight >= el.scrollHeight - 50;
    stickToBottomRef.current = atBottom;
    setShowJumpBottom(!atBottom);
  }

  function scrollToBottom() {
    const el = containerRef.current;
    if (!el) return;
    el.scrollTop = el.scrollHeight;
    stickToBottomRef.current = true;
    setShowJumpBottom(false);
  }

  // Find visible chunk range
  let firstVisible = 0;
  let lastVisible = 0;
  for (let i = 0; i < chunkCount; i++) {
    const bottom = (chunkTops[i] ?? 0) + getChunkHeight(i);
    if (bottom > scrollTop) {
      firstVisible = i;
      break;
    }
  }
  for (let i = firstVisible; i < chunkCount; i++) {
    lastVisible = i;
    if ((chunkTops[i] ?? 0) > scrollTop + viewHeight) break;
  }

  const startChunk = Math.max(0, firstVisible - OVERSCAN_CHUNKS);
  const endChunk = Math.min(chunkCount - 1, lastVisible + OVERSCAN_CHUNKS);

  const rendered: React.ReactNode[] = [];
  for (let ci = startChunk; ci <= endChunk; ci++) {
    const lineStart = ci * CHUNK_SIZE;
    const lineEnd = Math.min(lineStart + CHUNK_SIZE, lineCount);

    const chunkDivs: React.ReactNode[] = [];
    for (let li = lineStart; li < lineEnd; li++) {
      chunkDivs.push(
        <div key={li} className="break-all px-5 leading-6">
          {lines[li] || "\u00A0"}
        </div>,
      );
    }

    rendered.push(
      <div
        key={ci}
        ref={(el) => {
          if (el) chunkRefs.current.set(ci, el);
          else chunkRefs.current.delete(ci);
        }}
        style={{
          position: "absolute",
          top: chunkTops[ci] ?? 0,
          left: 0,
          right: 0,
        }}
      >
        {chunkDivs}
      </div>,
    );
  }

  // Suppress unused var — version drives re-render via useSyncExternalStore
  void version;

  return (
    <div className="relative min-h-0 flex-1">
      <div
        ref={containerRef}
        onScroll={onScroll}
        className="log-container absolute inset-0 overflow-y-auto overflow-x-hidden bg-[#0a0a0c] font-mono text-[13px] text-text-secondary"
      >
        <div style={{ height: totalHeight, position: "relative" }}>
          {rendered}
        </div>
      </div>
      {showJumpBottom && (
        <button
          type="button"
          onClick={scrollToBottom}
          className="absolute bottom-4 right-5 flex items-center gap-1.5 rounded-full border border-border bg-surface-2 px-3 py-1.5 text-xs text-text-secondary shadow-lg transition hover:bg-surface-3 hover:text-text-primary"
        >
          <svg width="12" height="12" viewBox="0 0 12 12" fill="none">
            <path d="M6 2v8M3 7l3 3 3-3" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
          </svg>
          Bottom
        </button>
      )}
    </div>
  );
}
