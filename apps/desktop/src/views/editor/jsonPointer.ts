/**
 * RFC 6901 JSON Pointer helpers.
 *
 * The validator returns errors keyed by JSON Pointer (`/foo/bar/0`),
 * the form pane needs to read the value at that path, and form edits
 * need to write it back. We don't pull a full JSON-pointer crate -
 * the surface we use is small (parse, get, set) and the pointer set
 * is bounded by the bundled schemas.
 *
 * Empty string `""` references the root document. Per the spec:
 *   * `~0` decodes to `~`
 *   * `~1` decodes to `/`
 *
 * Inverse encoding (escape `~` first, then `/`) is handled by
 * `encodeSegment`.
 */

/** Decode a single pointer segment per RFC 6901 (§4). */
export function decodeSegment(segment: string): string {
  // Per RFC 6901 the order is `~1` first then `~0`; we replace `~1`
  // → `/` first to avoid double-decoding `~01` (which means `~1`,
  // not `/`).
  return segment.replace(/~1/g, "/").replace(/~0/g, "~");
}

/** Encode a single segment per RFC 6901 (`~` first, then `/`). */
export function encodeSegment(segment: string): string {
  return segment.replace(/~/g, "~0").replace(/\//g, "~1");
}

/** Split a pointer into decoded segments. `""` → `[]`. */
export function parsePointer(pointer: string): string[] {
  if (pointer === "") return [];
  if (!pointer.startsWith("/")) {
    throw new Error(`invalid JSON pointer: ${pointer}`);
  }
  return pointer.slice(1).split("/").map(decodeSegment);
}

/** Re-assemble a pointer from raw (un-encoded) segments. */
export function buildPointer(segments: readonly string[]): string {
  if (segments.length === 0) return "";
  return "/" + segments.map(encodeSegment).join("/");
}

/**
 * Read the value at `pointer` in `value`. Returns `undefined` when
 * any segment is missing or the value is the wrong shape (object key
 * lookup on an array, etc.). Empty pointer returns `value` itself.
 */
export function getAtPointer(value: unknown, pointer: string): unknown {
  const segments = parsePointer(pointer);
  let current: unknown = value;
  for (const segment of segments) {
    if (current === null || current === undefined) return undefined;
    if (Array.isArray(current)) {
      const idx = Number(segment);
      if (!Number.isInteger(idx) || idx < 0 || idx >= current.length) {
        return undefined;
      }
      current = current[idx];
      continue;
    }
    if (typeof current !== "object") return undefined;
    current = (current as Record<string, unknown>)[segment];
  }
  return current;
}

/**
 * Return a NEW copy of `root` with the value at `pointer` replaced
 * by `nextValue`. Plain-object / array branches along the path are
 * cloned shallowly; siblings are preserved by reference for
 * structural-sharing efficiency.
 *
 * Setting at the empty pointer replaces the whole document.
 *
 * Missing intermediate keys are auto-created as objects (or arrays
 * when the next segment is a numeric index that fits into an array
 * push). This is the same heuristic JSON Patch's `add` operation
 * uses for out-of-bounds writes, and matches what a form edit needs
 * when the user fills in a previously-empty field.
 */
export function setAtPointer(
  root: unknown,
  pointer: string,
  nextValue: unknown,
): unknown {
  const segments = parsePointer(pointer);
  if (segments.length === 0) return nextValue;

  function recurse(current: unknown, depth: number): unknown {
    const segment = segments[depth];
    if (segment === undefined) return nextValue;
    const isLeaf = depth === segments.length - 1;
    const inner = isLeaf ? nextValue : recurse(stepInto(current, segment), depth + 1);
    return assignAt(current, segment, inner);
  }

  return recurse(root, 0);
}

/**
 * Read the value at the given segment, falling back to a fresh
 * container when the parent is missing or wrongly typed.
 */
function stepInto(parent: unknown, segment: string): unknown {
  if (Array.isArray(parent)) {
    const idx = Number(segment);
    if (Number.isInteger(idx) && idx >= 0 && idx < parent.length) {
      return parent[idx];
    }
    return {};
  }
  if (parent !== null && typeof parent === "object") {
    return (parent as Record<string, unknown>)[segment];
  }
  return {};
}

/**
 * Return a NEW copy of `parent` with `segment` set to `value`.
 * Arrays preserve sibling order; objects preserve sibling keys.
 */
function assignAt(parent: unknown, segment: string, value: unknown): unknown {
  if (Array.isArray(parent)) {
    const idx = Number(segment);
    const next = parent.slice();
    if (Number.isInteger(idx) && idx >= 0) {
      next[idx] = value;
    } else {
      // Non-numeric segment on an array: degrade to an object copy
      // so the caller's edit isn't silently dropped.
      const obj: Record<string, unknown> = {};
      parent.forEach((v, i) => {
        obj[String(i)] = v;
      });
      obj[segment] = value;
      return obj;
    }
    return next;
  }
  if (parent !== null && typeof parent === "object") {
    return { ...(parent as Record<string, unknown>), [segment]: value };
  }
  // Parent is a scalar/null - replace it with a fresh object
  // containing just this segment.
  return { [segment]: value };
}
