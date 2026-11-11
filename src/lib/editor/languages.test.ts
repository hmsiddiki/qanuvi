import { describe, expect, it } from "vitest";
import { LANGUAGE_LABELS, hasLoader, languageFor } from "./languages";

describe("languageFor", () => {
  it("knows the owner's project languages", () => {
    expect(languageFor("content/plugins/qcommerce/plugin.php")).toBe("php");
    expect(languageFor("resources/views/home.blade.php")).toBe("php");
    expect(languageFor("lib/main.dart")).toBe("dart");
    expect(languageFor("Controllers/HomeController.cs")).toBe("csharp");
    expect(languageFor("app/models.py")).toBe("python");
    expect(languageFor("src/routes/+page.svelte")).toBe("html");
    expect(languageFor("src/lib/api.ts")).toBe("typescript");
    expect(languageFor("android/app/build.gradle")).toBe("groovy");
    expect(languageFor("pubspec.yaml")).toBe("yaml");
  });

  it("uses file names and Windows paths", () => {
    expect(languageFor("Dockerfile")).toBe("dockerfile");
    expect(languageFor(".env.local")).toBe("properties");
    expect(languageFor(String.raw`E:\site\artisan`)).toBe("php");
    expect(languageFor("README.MD")).toBe("markdown");
  });

  it("falls back to plain text", () => {
    expect(languageFor("notes")).toBeNull();
    expect(languageFor("photo.png")).toBeNull();
  });

  it("has a loader and a label for every language it names", () => {
    for (const path of ["a.php", "a.dart", "a.cs", "a.ps1", "a.toml", "Dockerfile", "a.conf", "a.kt", "a.xml"]) {
      const lang = languageFor(path)!;
      expect(hasLoader(lang), lang).toBe(true);
      expect(LANGUAGE_LABELS[lang], lang).toBeTruthy();
    }
  });
});
