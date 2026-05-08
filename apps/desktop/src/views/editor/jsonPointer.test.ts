/**
 * RFC 6901 JSON Pointer helper tests.
 *
 * Four cases covering parse + get + set against the patterns the
 * form pane and the validator both produce: top-level keys, nested
 * keys with the `~` / `/` escapes, array indices, and the "set
 * creates parent" behaviour.
 */
import { describe, expect, it } from "vitest";
import {
  buildPointer,
  decodeSegment,
  encodeSegment,
  getAtPointer,
  parsePointer,
  setAtPointer,
} from "./jsonPointer";

describe("jsonPointer", () => {
  it("decodes ~0 and ~1 per RFC 6901", () => {
    expect(decodeSegment("foo~0bar")).toBe("foo~bar");
    expect(decodeSegment("foo~1bar")).toBe("foo/bar");
    expect(decodeSegment("~01")).toBe("~1");
    expect(decodeSegment("a~1b~0c")).toBe("a/b~c");
  });

  it("encodes inverse of decode", () => {
    expect(encodeSegment("foo~bar")).toBe("foo~0bar");
    expect(encodeSegment("foo/bar")).toBe("foo~1bar");
    expect(encodeSegment("a/b~c")).toBe("a~1b~0c");
  });

  it("parses and rebuilds a pointer", () => {
    expect(parsePointer("")).toEqual([]);
    expect(parsePointer("/foo")).toEqual(["foo"]);
    expect(parsePointer("/foo/bar")).toEqual(["foo", "bar"]);
    expect(parsePointer("/a~1b/c")).toEqual(["a/b", "c"]);
    expect(buildPointer([])).toBe("");
    expect(buildPointer(["foo", "bar"])).toBe("/foo/bar");
    expect(buildPointer(["a/b", "c"])).toBe("/a~1b/c");
  });

  it("getAtPointer reads top-level, nested, and array values", () => {
    const doc = {
      name: "x",
      tools: ["a", "b", "c"],
      "a/b": { c: 1 },
      meta: { version: 2 },
    };
    expect(getAtPointer(doc, "")).toBe(doc);
    expect(getAtPointer(doc, "/name")).toBe("x");
    expect(getAtPointer(doc, "/tools/1")).toBe("b");
    expect(getAtPointer(doc, "/a~1b/c")).toBe(1);
    expect(getAtPointer(doc, "/meta/version")).toBe(2);
    expect(getAtPointer(doc, "/missing")).toBeUndefined();
    expect(getAtPointer(doc, "/tools/99")).toBeUndefined();
  });

  it("setAtPointer returns a new tree, leaves the input untouched", () => {
    const doc = { name: "x", meta: { version: 1 } };
    const next = setAtPointer(doc, "/name", "y");
    // Original document is unchanged.
    expect(doc.name).toBe("x");
    expect((next as { name: string }).name).toBe("y");
    // The nested branch is preserved by reference (structural sharing).
    expect((next as { meta: object }).meta).toBe(doc.meta);
  });

  it("setAtPointer creates intermediate objects when missing", () => {
    const next = setAtPointer({}, "/a/b/c", 42);
    expect(next).toEqual({ a: { b: { c: 42 } } });
  });

  it("setAtPointer at the root replaces the document", () => {
    expect(setAtPointer({ a: 1 }, "", { b: 2 })).toEqual({ b: 2 });
  });

  it("setAtPointer writes into an array by index", () => {
    const next = setAtPointer({ tools: ["a", "b", "c"] }, "/tools/1", "B");
    expect(next).toEqual({ tools: ["a", "B", "c"] });
  });
});
