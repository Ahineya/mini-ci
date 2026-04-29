type Listener = () => void;

/**
 * Holds log data outside of React state. Appends are O(new_chars), not O(total).
 * Lines are split incrementally — no re-splitting the full log on each update.
 * React components subscribe via useSyncExternalStore.
 */
export class LogStore {
  private _lines: string[] = [];
  private _partial = ""; // trailing text not yet terminated by \n
  private _version = 0;
  private _listeners = new Set<Listener>();
  private _totalBytes = 0;

  get lines(): readonly string[] {
    return this._lines;
  }

  get version(): number {
    return this._version;
  }

  get totalBytes(): number {
    return this._totalBytes;
  }

  /** Append a chunk of text (may contain multiple lines). */
  append(chunk: string) {
    if (!chunk) return;

    this._totalBytes += chunk.length;

    // Combine with any leftover partial line from last append
    const text = this._partial + chunk;
    const parts = text.split("\n");

    // Last element is either "" (if chunk ended with \n) or a partial line
    this._partial = parts.pop()!;

    // All complete lines
    for (const line of parts) {
      this._lines.push(line);
    }

    this._version++;
    this._notify();
  }

  /** Reset for a new run. */
  clear() {
    this._lines = [];
    this._partial = "";
    this._totalBytes = 0;
    this._version++;
    this._notify();
  }

  /** Load a full log string (e.g. when selecting a finished run). */
  load(text: string) {
    this.clear();
    if (text) {
      this.append(text);
    }
  }

  // --- useSyncExternalStore glue ---

  subscribe = (listener: Listener): (() => void) => {
    this._listeners.add(listener);
    return () => this._listeners.delete(listener);
  };

  getSnapshot = (): number => {
    return this._version;
  };

  private _notify() {
    for (const l of this._listeners) l();
  }
}
