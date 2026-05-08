// Generated TS bindings live in apps/desktop/src-tauri/bindings/.
// `ts-rs` regenerates them on `cargo test`; never hand-edit.
//
// Phase 1.1 ships the registry surface; later phases extend this barrel.

export type { ToolId } from "../../../apps/desktop/src-tauri/bindings/registry/ToolId";
export type { ComponentType } from "../../../apps/desktop/src-tauri/bindings/registry/ComponentType";
export type { Scope } from "../../../apps/desktop/src-tauri/bindings/registry/Scope";
export type { Format } from "../../../apps/desktop/src-tauri/bindings/registry/Format";
export type { ComponentRoot } from "../../../apps/desktop/src-tauri/bindings/registry/ComponentRoot";
export type { ToolDescriptor } from "../../../apps/desktop/src-tauri/bindings/registry/ToolDescriptor";
export type { DetectedTool } from "../../../apps/desktop/src-tauri/bindings/registry/DetectedTool";

// Hand-rolled UI summary type used by the sidebar; not on the IPC wire.
import type { ToolId } from "../../../apps/desktop/src-tauri/bindings/registry/ToolId";

export interface ToolSummary {
  id: ToolId;
  displayName: string;
  detected: boolean;
  componentCount: number;
}
