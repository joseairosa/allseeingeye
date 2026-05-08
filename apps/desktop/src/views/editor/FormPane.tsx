/**
 * Schema-driven form pane.
 *
 * Phase 3.3 - the right-hand side of the editor splits into a
 * Monaco raw view and this form view. The form reads the bundled
 * JSON Schema for the component's `(tool, kind)` tuple and renders
 * one input per top-level property:
 *
 *   * `type: string` (length > 200 OR `format: "markdown"`) → textarea
 *   * `type: string` (otherwise) → text input
 *   * `type: boolean` → checkbox
 *   * `type: array` (string items) → comma-separated input
 *   * `enum` → select
 *   * `oneOf` discriminator → sub-form (deferred to Phase 4.x)
 *
 * Validation errors flow in via JSON pointer; we filter them per
 * field and render them inline. Unknown fields (those NOT in the
 * schema's `properties`) are surfaced read-only with a disabled
 * input and a "not in schema" hint - we never silently fork
 * unknown values.
 */
import type { ReactElement } from "react";
import type { ValidationError } from "@aseye/shared-types";
import { getAtPointer } from "./jsonPointer";
import type { FormAst } from "./EditState";

export interface FormPaneProps {
  /** The AST projected from the raw text. */
  ast: FormAst;
  /** Last validator outcome. `null` until we've run validation once. */
  errors: readonly ValidationError[];
  /** Schema text bundled from the validator, parsed once at the call site. */
  schema: SchemaShape | null;
  /** Format hint (markdownFrontmatter vs json/etc.) drives "additionalProperties" framing. */
  format: string;
  /** Last raw-buffer parse error - surfaced as a banner above the fields. */
  parseError: string | null;
  /** Called when a field changes; pointer is the JSON pointer of the field. */
  onFieldChange: (pointer: string, value: unknown) => void;
  /** True when no component is selected; renders an empty state. */
  empty?: boolean;
}

/** Coarse JSON Schema shape we care about for input rendering. */
export interface SchemaShape {
  type?: string;
  required?: string[];
  properties?: Record<string, SchemaShape>;
  items?: SchemaShape;
  enum?: unknown[];
  oneOf?: SchemaShape[];
  format?: string;
  minLength?: number;
  maxLength?: number;
  description?: string;
  additionalProperties?: boolean | SchemaShape;
  /** Pass-through for unknown keys; we never inspect them. */
  [k: string]: unknown;
}

/**
 * Parse a raw JSON Schema string. Returns `null` when the input is
 * `null` or unparseable - the form pane falls back to a minimal
 * "edit raw text" stub in that case.
 */
export function parseSchema(text: string | null): SchemaShape | null {
  if (text === null) return null;
  try {
    const value: unknown = JSON.parse(text);
    if (value === null || typeof value !== "object") return null;
    return value as SchemaShape;
  } catch {
    return null;
  }
}

/** Filter validator errors to those that reference `pointer`. */
function errorsAt(
  errors: readonly ValidationError[],
  pointer: string,
): ValidationError[] {
  return errors.filter((e) => e.path === pointer);
}

/** Render a single property's input. */
function PropertyField({
  name,
  schema,
  value,
  errors,
  onChange,
  required,
}: {
  name: string;
  schema: SchemaShape;
  value: unknown;
  errors: readonly ValidationError[];
  onChange: (pointer: string, next: unknown) => void;
  required: boolean;
}): ReactElement {
  const pointer = `/${name}`;
  const localErrors = errorsAt(errors, pointer);
  const hasError = localErrors.length > 0;
  const labelText = required ? `${name} *` : name;
  const inputId = `form-field-${name}`;

  let control: ReactElement;
  if (Array.isArray(schema.enum) && schema.enum.length > 0) {
    control = (
      <select
        id={inputId}
        value={typeof value === "string" || typeof value === "number" ? String(value) : ""}
        onChange={(e) => onChange(pointer, e.target.value)}
        aria-invalid={hasError || undefined}
      >
        <option value="">(unset)</option>
        {schema.enum.map((opt) => (
          <option key={String(opt)} value={String(opt)}>
            {String(opt)}
          </option>
        ))}
      </select>
    );
  } else if (schema.type === "boolean") {
    control = (
      <input
        id={inputId}
        type="checkbox"
        checked={value === true}
        onChange={(e) => onChange(pointer, e.target.checked)}
        aria-invalid={hasError || undefined}
      />
    );
  } else if (schema.type === "array") {
    // String-array fields get a comma-separated input; we split on
    // save. Non-string item arrays are out of scope for the MVP form.
    const itemsType = schema.items?.type;
    if (itemsType === "string") {
      const text = Array.isArray(value)
        ? (value as unknown[]).filter((v): v is string => typeof v === "string").join(", ")
        : "";
      control = (
        <input
          id={inputId}
          type="text"
          value={text}
          placeholder="comma, separated, values"
          onChange={(e) => {
            const split = e.target.value
              .split(",")
              .map((s) => s.trim())
              .filter((s) => s.length > 0);
            onChange(pointer, split);
          }}
          aria-invalid={hasError || undefined}
        />
      );
    } else {
      // Read-only fallback for anything we don't render structurally.
      control = (
        <input
          id={inputId}
          type="text"
          readOnly
          value={Array.isArray(value) ? `(array, ${value.length} items)` : "(unset)"}
        />
      );
    }
  } else if (schema.type === "string") {
    const longString =
      schema.format === "markdown" ||
      (typeof schema.maxLength === "number" && schema.maxLength > 200);
    const text = typeof value === "string" ? value : "";
    if (longString) {
      control = (
        <textarea
          id={inputId}
          value={text}
          rows={4}
          onChange={(e) => onChange(pointer, e.target.value)}
          aria-invalid={hasError || undefined}
        />
      );
    } else {
      control = (
        <input
          id={inputId}
          type="text"
          value={text}
          onChange={(e) => onChange(pointer, e.target.value)}
          aria-invalid={hasError || undefined}
        />
      );
    }
  } else {
    // Unknown / unsupported schema - render the JSON-stringified
    // value read-only so the user at least sees what's there.
    const display = value === undefined ? "" : JSON.stringify(value);
    control = <input id={inputId} type="text" readOnly value={display} />;
  }

  return (
    <label
      className="field"
      data-error={hasError ? "true" : undefined}
      htmlFor={inputId}
    >
      <span>{labelText}</span>
      {control}
      {schema.description ? (
        <span className="field-hint">{schema.description}</span>
      ) : null}
      {localErrors.map((err, i) => (
        <span key={`${err.schemaKeyword}-${i}`} className="field-error" role="alert">
          {err.message}
        </span>
      ))}
    </label>
  );
}

export function FormPane({
  ast,
  errors,
  schema,
  format,
  parseError,
  onFieldChange,
  empty,
}: FormPaneProps): ReactElement {
  if (empty === true) {
    return (
      <div className="form-pane">
        <div className="pane-title">
          <span>form view</span>
        </div>
        <p className="settings-todo">Pick a component to start editing.</p>
      </div>
    );
  }

  if (schema === null) {
    // No bundled schema for this tuple - we fall back to a read-only
    // dump of the AST so the form pane stays useful for tuples we
    // haven't covered yet (e.g. Antigravity workflows pre-Phase 4).
    return (
      <div className="form-pane">
        <div className="pane-title">
          <span>form view</span>
          <span className="mono">{format}</span>
        </div>
        <p className="settings-todo">
          No bundled schema for this component yet. Edit the raw text on the right.
        </p>
      </div>
    );
  }

  const properties = schema.properties ?? {};
  const required = new Set(schema.required ?? []);
  const knownKeys = new Set(Object.keys(properties));
  const astKeys = Object.keys(ast);
  const unknownKeys = astKeys.filter((k) => !knownKeys.has(k));

  // Root-level errors - rendered above the fields so the user sees
  // them even when no specific field is highlighted.
  const rootErrors = errors.filter((e) => e.path === "");

  return (
    <form
      className="form-pane"
      aria-label="schema form"
      onSubmit={(e) => e.preventDefault()}
    >
      <div className="pane-title">
        <span>form view</span>
        <span className="mono">{format}</span>
      </div>

      {parseError !== null ? (
        <p className="settings-todo" role="status">
          form is stale - last edit failed to parse: {parseError}
        </p>
      ) : null}

      {rootErrors.length > 0 ? (
        <div className="validation-box" role="alert">
          <span>!</span>
          <div>
            {rootErrors.map((err, i) => (
              <p key={`root-${i}`}>{err.message}</p>
            ))}
          </div>
        </div>
      ) : null}

      {Object.entries(properties).map(([name, propSchema]) => (
        <PropertyField
          key={name}
          name={name}
          schema={propSchema}
          value={getAtPointer(ast, `/${name}`)}
          errors={errors}
          onChange={onFieldChange}
          required={required.has(name)}
        />
      ))}

      {unknownKeys.length > 0 ? (
        <div>
          <p className="settings-todo">
            unknown fields (not in schema, read-only):
          </p>
          {unknownKeys.map((name) => (
            <label key={name} className="field" htmlFor={`form-unknown-${name}`}>
              <span>{name}</span>
              <input
                id={`form-unknown-${name}`}
                type="text"
                readOnly
                value={JSON.stringify(ast[name])}
              />
            </label>
          ))}
        </div>
      ) : null}
    </form>
  );
}
