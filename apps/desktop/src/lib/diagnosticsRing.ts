/**
 * Diagnostics ring buffer (Phase 4.2).
 *
 * Module-level singleton holding the last `RING_SIZE` `PipelineEvent`s.
 * The Diagnostics panel reads from it; `App.tsx` mounts a single
 * subscription via `useDiagnosticsRing` so every event also funnels into
 * this buffer regardless of which views are open.
 *
 * The buffer is in-memory only - it is intentionally never persisted.
 * Diagnostics export sanitises whatever is here at the moment of copy.
 */
import { useEffect, useState } from "react";
import type { PipelineEvent } from "@aseye/shared-types";
import { subscribeToPipelineEvents } from "@/ipc/events";

/** Maximum number of events retained. Anything older is evicted. */
export const RING_SIZE = 100;

/**
 * Stamped event: the wire `PipelineEvent` plus the local clock at the
 * moment we received it. We never trust a Rust-side timestamp - the
 * diagnostics panel is the only consumer and it wants the user's local
 * clock for the "Recent file events" feed.
 */
export interface StampedPipelineEvent {
  event: PipelineEvent;
  /** `Date.now()` at the moment the event was appended. */
  timestamp: number;
}

/**
 * Internal mutable state. Kept module-level so the buffer survives
 * re-renders and HMR-induced remounts; tests reset it via `clear()`.
 */
const buffer: StampedPipelineEvent[] = [];

/** Listeners notified after every successful append (for `useEffect`). */
type Listener = () => void;
const listeners = new Set<Listener>();

/**
 * Append an event to the ring. Evicts the oldest entry once the buffer
 * exceeds `RING_SIZE`. Notifies subscribers synchronously.
 */
export function addEvent(event: PipelineEvent): void {
  buffer.push({ event, timestamp: Date.now() });
  while (buffer.length > RING_SIZE) {
    buffer.shift();
  }
  for (const listener of listeners) {
    listener();
  }
}

/**
 * Snapshot of the current ring, newest-first. Returns a fresh array so
 * callers cannot mutate the underlying buffer.
 */
export function getRecent(): StampedPipelineEvent[] {
  return buffer.slice().reverse();
}

/** Empty the ring. Used by tests and the panel's "clear" affordance. */
export function clear(): void {
  buffer.length = 0;
  for (const listener of listeners) {
    listener();
  }
}

/**
 * Subscribe to ring updates. Returns a cleanup function. Fires on every
 * `addEvent` and `clear` call.
 */
export function subscribe(listener: Listener): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/**
 * Idempotency guard for `useDiagnosticsRing`. The hook is meant to be
 * mounted once near the App root; this flag guarantees that even if the
 * effect runs twice (StrictMode double-invoke) we only attach a single
 * pipeline-event listener.
 */
let mountCount = 0;
let activeUnlisten: (() => void) | null = null;

/**
 * Subscribe the diagnostics ring to `pipeline-event` channel emissions.
 * Mount this once in `App.tsx`. Subsequent mounts (StrictMode, HMR) are
 * tracked via a refcount so cleanup happens exactly when the last caller
 * unmounts.
 */
export function useDiagnosticsRing(): void {
  useEffect(() => {
    mountCount += 1;
    let cancelled = false;

    if (mountCount === 1) {
      void subscribeToPipelineEvents((event) => {
        addEvent(event);
      }).then((un) => {
        if (cancelled) {
          un();
          activeUnlisten = null;
          return;
        }
        activeUnlisten = un;
      });
    }

    return () => {
      cancelled = true;
      mountCount -= 1;
      if (mountCount === 0 && activeUnlisten) {
        activeUnlisten();
        activeUnlisten = null;
      }
    };
  }, []);
}

/**
 * React-friendly hook returning a live snapshot of the ring. Re-renders
 * the calling component on every append/clear.
 */
export function useDiagnosticsEvents(): StampedPipelineEvent[] {
  const [snapshot, setSnapshot] = useState<StampedPipelineEvent[]>(() =>
    getRecent(),
  );
  useEffect(() => {
    const unsubscribe = subscribe(() => {
      setSnapshot(getRecent());
    });
    return unsubscribe;
  }, []);
  return snapshot;
}

/**
 * Test-only helper: reset the idempotency refcount. Production code
 * should never call this - it exists so unit tests can simulate fresh
 * mounts without polluting the module singleton.
 */
export function __resetForTests(): void {
  mountCount = 0;
  if (activeUnlisten) {
    activeUnlisten();
    activeUnlisten = null;
  }
  clear();
  listeners.clear();
}
