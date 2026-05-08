/**
 * Subscriber for the `pipeline-event` Tauri event channel.
 *
 * The Rust side emits a `PipelineEvent` after every classify+index step
 * (see `apps/desktop/src-tauri/src/pipeline/event.rs`). The React layer
 * uses this to invalidate the relevant TanStack Query caches without
 * polling.
 */
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { PipelineEvent } from "@aseye/shared-types";

/** Tauri event channel name; must match the Rust emitter. */
export const PIPELINE_EVENT_CHANNEL = "pipeline-event";

export type PipelineEventHandler = (event: PipelineEvent) => void;

/**
 * Subscribe to pipeline events. Returns the cleanup function returned
 * by `listen()` - call it from a `useEffect` cleanup to detach.
 */
export async function subscribeToPipelineEvents(
  handler: PipelineEventHandler,
): Promise<UnlistenFn> {
  return listen<PipelineEvent>(PIPELINE_EVENT_CHANNEL, (e) => handler(e.payload));
}
