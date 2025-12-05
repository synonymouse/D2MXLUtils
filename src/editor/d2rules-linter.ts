/**
 * CodeMirror 6 linter integration for D2 Rules DSL
 *
 * Connects to Tauri backend's validate_filter_dsl command for real-time validation
 *
 * @module d2rules-linter
 */
import { linter, type Diagnostic } from "@codemirror/lint";
import { invoke } from "@tauri-apps/api/core";

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
        return errors.map((err): Diagnostic => {
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
