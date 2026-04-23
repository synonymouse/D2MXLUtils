import { vitePreprocess } from "@sveltejs/vite-plugin-svelte";

/** @type {import('svelte').Config} */
const config = {
  preprocess: vitePreprocess(),
  compilerOptions: {
    // This is a desktop overlay with keyboard-first dialogs and dropdowns;
    // the a11y compiler warnings (click-events-have-key-events,
    // no-static-element-interactions, autofocus) spam the dev console and
    // don't reflect real defects here. Drop the whole category.
    warningFilter: (warning) => !warning.code.startsWith("a11y_"),
  },
};

export default config;


