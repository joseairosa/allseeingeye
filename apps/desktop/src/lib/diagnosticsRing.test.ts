import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { PipelineEvent } from "@aseye/shared-types";
import {
  RING_SIZE,
  __resetForTests,
  addEvent,
  clear,
  getRecent,
  subscribe,
} from "./diagnosticsRing";

function makeEvent(id: string): PipelineEvent {
  return { event: "componentUpserted", id, kind: "inserted" };
}

describe("diagnosticsRing", () => {
  beforeEach(() => {
    __resetForTests();
  });

  afterEach(() => {
    __resetForTests();
  });

  it("evicts oldest entries once the buffer overflows RING_SIZE", () => {
    // Push RING_SIZE + 5 events; expect ring to retain RING_SIZE most recent.
    for (let i = 0; i < RING_SIZE + 5; i += 1) {
      addEvent(makeEvent(`id-${i}`));
    }
    const recent = getRecent();
    expect(recent).toHaveLength(RING_SIZE);
    // Newest-first: first element must be the last id we pushed.
    const head = recent[0];
    expect(head).toBeDefined();
    if (head?.event.event === "componentUpserted") {
      expect(head.event.id).toBe(`id-${RING_SIZE + 4}`);
    } else {
      throw new Error("expected componentUpserted at head of ring");
    }
    // Oldest retained event must be id-5 (id-0..id-4 were evicted).
    const tail = recent[recent.length - 1];
    if (tail?.event.event === "componentUpserted") {
      expect(tail.event.id).toBe("id-5");
    } else {
      throw new Error("expected componentUpserted at tail of ring");
    }
  });

  it("getRecent returns events newest-first", () => {
    addEvent(makeEvent("first"));
    addEvent(makeEvent("second"));
    addEvent(makeEvent("third"));
    const recent = getRecent();
    expect(recent).toHaveLength(3);
    const ids = recent.map((s) =>
      s.event.event === "componentUpserted" ? s.event.id : null,
    );
    expect(ids).toEqual(["third", "second", "first"]);
  });

  it("clear empties the ring and notifies subscribers", () => {
    addEvent(makeEvent("a"));
    addEvent(makeEvent("b"));
    expect(getRecent()).toHaveLength(2);
    const listener = vi.fn();
    const unsub = subscribe(listener);
    clear();
    expect(getRecent()).toHaveLength(0);
    expect(listener).toHaveBeenCalledTimes(1);
    unsub();
  });

  it("subscribe is idempotent across multiple subscribers", () => {
    const a = vi.fn();
    const b = vi.fn();
    const unsubA = subscribe(a);
    const unsubB = subscribe(b);
    addEvent(makeEvent("x"));
    expect(a).toHaveBeenCalledTimes(1);
    expect(b).toHaveBeenCalledTimes(1);
    unsubA();
    addEvent(makeEvent("y"));
    expect(a).toHaveBeenCalledTimes(1); // unsubscribed
    expect(b).toHaveBeenCalledTimes(2);
    unsubB();
    addEvent(makeEvent("z"));
    expect(a).toHaveBeenCalledTimes(1);
    expect(b).toHaveBeenCalledTimes(2);
  });
});
