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
interface ValidationError {
  line: number;
  column: number;
  message: string;
  severity: "error" | "warning" | "info";
}

/**
 * Create a linter extension that validates DSL via Tauri command
 *
 * @param debounceMs - Debounce delay in milliseconds (default: 300)
 * @returns Linter extension for CodeMirror
 */
export function d2rulesLinter(debounceMs = 300) {
  return linter(
    async (view) => {
      const doc = view.state.doc;
      const text = doc.toString();

      // Skip validation for empty documents
      if (!text.trim()) {
        return [];
      }

      try {
        // Call Tauri backend for validation
        const errors: ValidationError[] = await invoke("validate_filter_dsl", {
          text,
        });

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
        return [];
      }
    },
    {
      delay: debounceMs,
    }
  );
}

/**
 * Standalone validation function for manual validation (e.g., on save)
 *
 * @param text - DSL text to validate
 * @returns Array of validation errors
 */
export async function validateDsl(text: string): Promise<ValidationError[]> {
  if (!text.trim()) {
    return [];
  }

  try {
    return await invoke("validate_filter_dsl", { text });
  } catch (e) {
    console.error("[d2rules-linter] Validation error:", e);
    return [];
  }
}


