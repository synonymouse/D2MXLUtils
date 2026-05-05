/**
 * CodeMirror 6 linter integration for D2 Rules DSL
 *
 * Connects to Tauri backend's validate_filter_dsl command for real-time validation
 *
 * @module d2rules-linter
 */
import { linter, type Diagnostic } from "@codemirror/lint";
import { invoke } from "@tauri-apps/api/core";
import { settingsStore } from '../stores';

/**
 * Validation error from Tauri backend
 */
export interface ValidationError {
  line: number;
  column: number;
  message: string;
  severity: "error" | "warning" | "info";
}

/**
 * Result of validation + parsing
 */
export interface ValidationResult {
  errors: ValidationError[];
  ruleCount: number;
}

const SOUND_REF_RE = /\bsound(\d+)\b/gi;

/**
 * Scan the document for `soundN` references that point to slots which
 * are out of range or in the `Empty` state. Emits info-severity
 * diagnostics — the rule itself still parses; the warning just nudges
 * the user that the referenced slot will play silence.
 */
function scanSoundSlotRefs(doc: import('@codemirror/state').Text): Diagnostic[] {
  const out: Diagnostic[] = [];
  const slots = settingsStore.settings.sounds;
  for (let lineNum = 1; lineNum <= doc.lines; lineNum++) {
    const line = doc.line(lineNum);
    const text = line.text;
    SOUND_REF_RE.lastIndex = 0;
    let m: RegExpExecArray | null;
    while ((m = SOUND_REF_RE.exec(text)) !== null) {
      const n = Number.parseInt(m[1], 10);
      if (!Number.isFinite(n) || n < 1 || n > 255) continue;
      const slot = slots[n - 1];
      if (slot && slot.source.kind !== 'empty') continue;
      out.push({
        from: line.from + m.index,
        to: line.from + m.index + m[0].length,
        severity: 'info',
        message: `sound${n} is not configured on the Sounds tab.`,
        source: 'd2rules',
      });
    }
  }
  return out;
}

/**
 * Create a linter extension that validates DSL via Tauri command
 *
 * @param debounceMs - Debounce delay in milliseconds (default: 1000)
 * @param onResult - Optional callback called after each validation with results
 * @returns Linter extension for CodeMirror
 */
export function d2rulesLinter(
  debounceMs = 1000,
  onResult?: (result: ValidationResult) => void
) {
  return linter(
    async (view) => {
      const doc = view.state.doc;
      const text = doc.toString();

      // Skip validation for empty documents
      if (!text.trim()) {
        onResult?.({ errors: [], ruleCount: 0 });
        return [];
      }

      try {
        // Call Tauri backend for validation
        const errors: ValidationError[] = await invoke("validate_filter_dsl", {
          text,
        });

        // If no errors, parse to get rule count
        let ruleCount = 0;
        if (errors.length === 0) {
          try {
            const config = await invoke<{ rules: unknown[] }>(
              "parse_filter_dsl",
              { text }
            );
            ruleCount = config.rules?.length ?? 0;
          } catch {
            // Parse failed - shouldn't happen if validation passed, but handle gracefully
          }
        }

        // Notify parent about results
        onResult?.({ errors, ruleCount });

        // Convert backend errors to CodeMirror diagnostics
        const backendDiagnostics = errors.map((err): Diagnostic => {
          // Clamp line number to valid range
          const lineNum = Math.max(1, Math.min(err.line, doc.lines));
          const line = doc.line(lineNum);

          return {
            from: line.from,
            to: line.to,
            severity: err.severity,
            message: err.message,
            source: "d2rules",
          };
        });
        return [...backendDiagnostics, ...scanSoundSlotRefs(doc)];
      } catch (e) {
        // Log error but don't crash the editor
        console.error("[d2rules-linter] Validation error:", e);
        onResult?.({ errors: [], ruleCount: 0 });
        return [];
      }
    },
    {
      delay: debounceMs,
    }
  );
}
