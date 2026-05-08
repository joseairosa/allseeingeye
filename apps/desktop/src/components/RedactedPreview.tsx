import { useCallback, useEffect, useRef, useState } from "react";
import { useUi } from "@/store/ui";

/**
 * Auto-mask delay in ms when the consumer doesn't override `revealedFor`.
 * Mirrors the 5-second default named in `docs/12-security.md` and matches
 * `SecretField`'s default so users get the same dismissal cadence
 * everywhere.
 */
const DEFAULT_REVEAL_MS = 5_000;

interface RedactedPreviewProps {
  /**
   * The already-redacted value coming from the Rust scanner (e.g.
   * `"abcd…wxyz"`). The full secret never reaches this component - we
   * only ever toggle the visibility of the safe redacted form. Treat
   * the value as "safe text" but still respect panic mode and require
   * an explicit click to reveal.
   */
  value: string;
  /** Optional ARIA label for the masked output element. */
  label?: string;
  /** Auto-mask after this many ms. Set to 0 to disable - panic mode still wins. */
  revealedFor?: number;
}

/**
 * Compact, non-input-shaped sibling of `SecretField` for displaying a
 * pre-redacted preview in dense surfaces (Security view rows, Quick
 * Look Security section). Differs from `SecretField` in three ways:
 *
 *  1. No copy affordance - the user is looking at a redacted preview,
 *     not the underlying secret, so the clipboard write would be
 *     misleading.
 *  2. Inline layout instead of the form-row + label split.
 *  3. Smaller default footprint suited to list rows.
 *
 * Hard rules (mirrored from `docs/12-security.md`):
 * - Default state is masked (`••••`).
 * - Reveal requires an explicit click; auto-masks after `revealedFor` ms.
 * - Panic mode (`Cmd-Shift-.`) forces masking regardless of internal state.
 */
export function RedactedPreview({
  value,
  label,
  revealedFor = DEFAULT_REVEAL_MS,
}: RedactedPreviewProps) {
  const panicMode = useUi((s) => s.panicMode);
  const [revealed, setRevealed] = useState(false);
  const timerRef = useRef<number | null>(null);

  const clearAutoMask = useCallback((): void => {
    if (timerRef.current !== null) {
      window.clearTimeout(timerRef.current);
      timerRef.current = null;
    }
  }, []);

  // Panic mode wins instantly.
  useEffect(() => {
    if (panicMode) {
      setRevealed(false);
      clearAutoMask();
    }
  }, [panicMode, clearAutoMask]);

  // Auto-mask after `revealedFor` ms when revealed.
  useEffect(() => {
    if (!revealed || revealedFor <= 0) return;
    clearAutoMask();
    timerRef.current = window.setTimeout(() => {
      setRevealed(false);
      timerRef.current = null;
    }, revealedFor);
    return clearAutoMask;
  }, [revealed, revealedFor, clearAutoMask]);

  // Cleanup on unmount.
  useEffect(() => clearAutoMask, [clearAutoMask]);

  const isMasked = panicMode || !revealed;
  const display = isMasked ? "••••••••" : value;

  function handleToggle(): void {
    if (panicMode) return;
    setRevealed((r) => !r);
  }

  return (
    <button
      type="button"
      className="redacted-preview"
      data-panic={panicMode ? "true" : undefined}
      onClick={handleToggle}
      aria-pressed={revealed && !panicMode}
      aria-label={
        revealed && !panicMode
          ? `hide ${label ?? "redacted preview"}`
          : `reveal ${label ?? "redacted preview"}`
      }
      title={
        panicMode
          ? "panic mode active"
          : revealed
            ? "hide redacted preview"
            : "reveal redacted preview"
      }
      disabled={panicMode || value.length === 0}
    >
      <span className="mono">{display}</span>
    </button>
  );
}
