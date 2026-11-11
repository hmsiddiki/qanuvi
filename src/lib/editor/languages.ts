/**
 * Syntax highlighting per file type. Each language is loaded only when a file of that type is first opened, so the
 * app starts light.
 */
import type { Extension } from "@codemirror/state";
import type { StreamParser } from "@codemirror/language";

type Loader = () => Promise<Extension>;

// eslint-disable-next-line @typescript-eslint/no-explicit-any
const legacy = async (load: () => Promise<StreamParser<any>>) => {
  const [{ StreamLanguage }, mode] = await Promise.all([import("@codemirror/language"), load()]);
  return StreamLanguage.define(mode);
};

const LOADERS: Record<string, Loader> = {
  php: async () => (await import("@codemirror/lang-php")).php(),
  javascript: async () => (await import("@codemirror/lang-javascript")).javascript(),
  jsx: async () => (await import("@codemirror/lang-javascript")).javascript({ jsx: true }),
  typescript: async () => (await import("@codemirror/lang-javascript")).javascript({ typescript: true }),
  tsx: async () => (await import("@codemirror/lang-javascript")).javascript({ typescript: true, jsx: true }),
  css: async () => (await import("@codemirror/lang-css")).css(),
  html: async () => (await import("@codemirror/lang-html")).html(),
  markdown: async () => (await import("@codemirror/lang-markdown")).markdown(),
  python: async () => (await import("@codemirror/lang-python")).python(),
  json: async () => (await import("@codemirror/lang-json")).json(),
  yaml: async () => (await import("@codemirror/lang-yaml")).yaml(),
  sql: async () => (await import("@codemirror/lang-sql")).sql(),
  rust: async () => (await import("@codemirror/lang-rust")).rust(),
  xml: async () => (await import("@codemirror/lang-xml")).xml(),
  dart: () => legacy(async () => (await import("@codemirror/legacy-modes/mode/clike")).dart),
  csharp: () => legacy(async () => (await import("@codemirror/legacy-modes/mode/clike")).csharp),
  java: () => legacy(async () => (await import("@codemirror/legacy-modes/mode/clike")).java),
  kotlin: () => legacy(async () => (await import("@codemirror/legacy-modes/mode/clike")).kotlin),
  cpp: () => legacy(async () => (await import("@codemirror/legacy-modes/mode/clike")).cpp),
  shell: () => legacy(async () => (await import("@codemirror/legacy-modes/mode/shell")).shell),
  powershell: () => legacy(async () => (await import("@codemirror/legacy-modes/mode/powershell")).powerShell),
  toml: () => legacy(async () => (await import("@codemirror/legacy-modes/mode/toml")).toml),
  properties: () => legacy(async () => (await import("@codemirror/legacy-modes/mode/properties")).properties),
  dockerfile: () => legacy(async () => (await import("@codemirror/legacy-modes/mode/dockerfile")).dockerFile),
  groovy: () => legacy(async () => (await import("@codemirror/legacy-modes/mode/groovy")).groovy),
  nginx: () => legacy(async () => (await import("@codemirror/legacy-modes/mode/nginx")).nginx),
};

const BY_EXTENSION: Record<string, string> = {
  php: "php",
  phtml: "php",
  js: "javascript",
  mjs: "javascript",
  cjs: "javascript",
  jsx: "jsx",
  ts: "typescript",
  mts: "typescript",
  cts: "typescript",
  tsx: "tsx",
  css: "css",
  scss: "css",
  less: "css",
  html: "html",
  htm: "html",
  svelte: "html",
  vue: "html",
  md: "markdown",
  markdown: "markdown",
  py: "python",
  pyw: "python",
  json: "json",
  jsonc: "json",
  yaml: "yaml",
  yml: "yaml",
  sql: "sql",
  rs: "rust",
  xml: "xml",
  svg: "xml",
  xaml: "xml",
  csproj: "xml",
  dart: "dart",
  cs: "csharp",
  java: "java",
  kt: "kotlin",
  kts: "kotlin",
  c: "cpp",
  h: "cpp",
  cpp: "cpp",
  hpp: "cpp",
  cc: "cpp",
  sh: "shell",
  bash: "shell",
  zsh: "shell",
  ps1: "powershell",
  psm1: "powershell",
  toml: "toml",
  ini: "properties",
  properties: "properties",
  env: "properties",
  gradle: "groovy",
  conf: "nginx",
};

const BY_NAME: Record<string, string> = {
  dockerfile: "dockerfile",
  ".env": "properties",
  ".gitignore": "properties",
  ".editorconfig": "properties",
  "nginx.conf": "nginx",
  artisan: "php",
};

/** The language for a file path, or null for plain text. */
export function languageFor(path: string): string | null {
  const name = (path.split(/[\\/]/).pop() ?? "").toLowerCase();
  if (BY_NAME[name]) return BY_NAME[name];
  if (name.startsWith(".env")) return "properties";
  const dot = name.lastIndexOf(".");
  if (dot < 0) return null;
  return BY_EXTENSION[name.slice(dot + 1)] ?? null;
}

const cache = new Map<string, Promise<Extension>>();

export function loadLanguage(language: string | null): Promise<Extension> {
  if (!language || !LOADERS[language]) return Promise.resolve([]);
  let p = cache.get(language);
  if (!p) {
    p = LOADERS[language]().catch(() => []);
    cache.set(language, p);
  }
  return p;
}

export const LANGUAGE_LABELS: Record<string, string> = {
  php: "PHP",
  javascript: "JavaScript",
  jsx: "JavaScript JSX",
  typescript: "TypeScript",
  tsx: "TypeScript JSX",
  css: "CSS",
  html: "HTML",
  markdown: "Markdown",
  python: "Python",
  json: "JSON",
  yaml: "YAML",
  sql: "SQL",
  rust: "Rust",
  xml: "XML",
  dart: "Dart",
  csharp: "C#",
  java: "Java",
  kotlin: "Kotlin",
  cpp: "C/C++",
  shell: "Shell",
  powershell: "PowerShell",
  toml: "TOML",
  properties: "Properties",
  dockerfile: "Dockerfile",
  groovy: "Groovy",
  nginx: "Nginx",
};

export function hasLoader(language: string): boolean {
  return language in LOADERS;
}
