import { useCallback, useEffect, useRef, useState } from "react";
import type { SVGProps } from "react";
import { useUi } from "@/store/ui";
import { maskSecret, type SecretFieldKind } from "@/lib/secrets";

/**
 * Auto-mask delay in ms when the consumer doesn't override `revealedFor`.
 * Mirrors the 5-second default named in `docs/12-security.md`.
 */
const DEFAULT_REVEAL_MS = 5_000;

interface SecretFieldProps {
  value: string;
  label?: string;
  /**
   * Drives the icon glyph and placeholder text. `auth-header` and `password`
   * carry the same affordances as `token`; the kind exists so screen
   * readers and the placeholder copy match the underlying secret shape.
   */
  kind?: SecretFieldKind;
  /**
   * Auto-mask after this many ms of being revealed. Set to 0 to disable
   * auto-mask (NOT recommended - panic mode still wins).
   */
  revealedFor?: number;
  disabled?: boolean;
  /**
   * Optional copy callback. The consumer is responsible for the actual
   * clipboard write so the component itself never touches the system
   * clipboard.
   */
  onCopyConfirmed?: (value: string) => void;
}

function EyeIcon(props: SVGProps<SVGSVGElement>) {
  return (
    <svg aria-hidden="true" viewBox="0 0 24 24" {...props}>
      <path d="M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7S2 12 2 12Z" />
      <circle cx="12" cy="12" r="3" />
    </svg>
  );
}

function EyeOffIcon(props: SVGProps<SVGSVGElement>) {
  return (
    <svg aria-hidden="true" viewBox="0 0 24 24" {...props}>
      <path d="M3 3l18 18" />
      <path d="M10.6 6.1A10.5 10.5 0 0 1 12 6c6.5 0 10 6 10 6a17.4 17.4 0 0 1-3.2 3.9" />
      <path d="M6.6 6.6A17.4 17.4 0 0 0 2 12s3.5 6 10 6a10.5 10.5 0 0 0 4.4-.9" />
      <path d="M9.9 9.9a3 3 0 0 0 4.2 4.2" />
    </svg>
  );
}

function CopyIcon(props: SVGProps<SVGSVGElement>) {
  return (
    <svg aria-hidden="true" viewBox="0 0 24 24" {...props}>
      <rect x="9" y="9" width="11" height="11" rx="2" ry="2" />
      <path d="M5 15V5a2 2 0 0 1 2-2h10" />
    </svg>
  );
}

function placeholderFor(kind: SecretFieldKind): string {
  switch (kind) {
    case "password":
      return "password (hidden)";
    case "auth-header":
      return "Authorization header (hidden)";
    case "token":
    default:
      return "secret token (hidden)";
  }
}

/**
 * SecretField renders a masked secret value with a click-to-reveal eye and
 * an explicit copy affordance. Reveal auto-masks after `revealedFor` ms.
 *
 * Hard rules (see `docs/12-security.md`):
 * - Default state is masked.
 * - Reveal requires an explicit click; auto-masks after 5 s.
 * - Never copies to the clipboard automatically.
 * - Panic mode forces masking regardless of internal state.
 */
export function SecretField({
  value,
  label,
  kind = "token",
  revealedFor = DEFAULT_REVEAL_MS,
  disabled = false,
  onCopyConfirmed,
}: SecretFieldProps) {
  const panicMode = useUi((s) => s.panicMode);
  const [revealed, setRevealed] = useState(false);
  const [askingCopy, setAskingCopy] = useState(false);
  const timerRef = useRef<number | null>(null);

  const clearAutoMask = useCallback((): void => {
    if (timerRef.current !== null) {
      window.clearTimeout(timerRef.current);
      timerRef.current = null;
    }
  }, []);

  // Panic mode wins instantly. Drop any pending reveal/copy state.
  useEffect(() => {
    if (panicMode) {
      setRevealed(false);
      setAskingCopy(false);
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
  const display = isMasked ? maskSecret(value) : value;
  const placeholder = placeholderFor(kind);

  function handleToggle(): void {
    if (disabled || panicMode) return;
    setRevealed((r) => !r);
    setAskingCopy(false);
  }

  function handleCopyClick(): void {
    if (disabled || panicMode) return;
    if (!askingCopy) {
      setAskingCopy(true);
      return;
    }
    setAskingCopy(false);
    onCopyConfirmed?.(value);
  }

  return (
    <div className="secret-field" data-panic={panicMode ? "true" : undefined}>
      {label ? (
        <span className="secret-field-label">{label}</span>
      ) : null}
      <div className="secret-field-row">
        <output
          className="secret-field-value mono"
          aria-label={label ?? placeholder}
          aria-live="polite"
        >
          {value ? display : <span className="secret-field-placeholder">{placeholder}</span>}
        </output>
        <button
          type="button"
          className="icon-button"
          onClick={handleToggle}
          aria-pressed={revealed && !panicMode}
          aria-label={revealed && !panicMode ? "hide secret" : "reveal secret"}
          title={panicMode ? "panic mode active" : revealed ? "hide" : "reveal"}
          disabled={disabled || panicMode || !value}
        >
          {revealed && !panicMode ? <EyeOffIcon /> : <EyeIcon />}
        </button>
        <button
          type="button"
          className="icon-button"
          onClick={handleCopyClick}
          aria-label={askingCopy ? "confirm copy revealed value" : "copy secret"}
          title={askingCopy ? "copy revealed value?" : "copy"}
          disabled={disabled || panicMode || !value || !revealed}
        >
          <CopyIcon />
        </button>
      </div>
      {askingCopy ? (
        <span className="secret-field-confirm" role="status">
          copy revealed value?
        </span>
      ) : null}
    </div>
  );
}
