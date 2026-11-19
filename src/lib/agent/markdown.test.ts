import { describe, expect, it } from "vitest";
import { renderMarkdown } from "./markdown";

describe("agent answer markdown", () => {
  it("formats the usual answer", () => {
    const html = renderMarkdown(
      "Done! Here's a summary:\n\n1. **First edit:** appended `core one`.\n2. *Second* edit.\n\n- a\n- b\n\n## Next\nSee [docs](https://example.com).",
    );
    expect(html).toContain("<ol><li><strong>First edit:</strong> appended <code>core one</code>.</li>");
    expect(html).toContain("<li><em>Second</em> edit.</li></ol>");
    expect(html).toContain("<ul><li>a</li><li>b</li></ul>");
    expect(html).toContain("<h4>Next</h4>");
    expect(html).toContain('<a href="https://example.com" target="_blank" rel="noreferrer">docs</a>');
  });

  it("never lets the text add HTML of its own", () => {
    const html = renderMarkdown('<img src=x onerror=alert(1)> [x](javascript:alert(1)) "q"');
    expect(html).not.toContain("<img");
    expect(html).not.toContain("href=");
    expect(html).toContain("&lt;img");
    expect(renderMarkdown("```\n<b>raw</b> **not bold**\n```")).toBe(
      "<pre><code>&lt;b&gt;raw&lt;/b&gt; **not bold**</code></pre>",
    );
  });

  it("keeps numbered paragraphs one list, numbered on, and a list's own first number", () => {
    expect(renderMarkdown("1. one\n\n2. two\n\n\n3. three")).toBe("<ol><li>one</li><li>two</li><li>three</li></ol>");
    expect(renderMarkdown("- a\n\n- b")).toBe("<ul><li>a</li><li>b</li></ul>");
    expect(renderMarkdown("1. a\n\nAfter.\n\n2. b")).toBe(
      '<ol><li>a</li></ol><p>After.</p><ol start="2"><li>b</li></ol>',
    );
    expect(renderMarkdown("1. a\n\n- b")).toBe("<ol><li>a</li></ol><ul><li>b</li></ul>");
  });

  it("shows checklist lines as ticks and empty boxes (A5)", () => {
    expect(renderMarkdown("1. [x] Read **a**\n2. [ ] Run it")).toBe(
      '<ol><li class="task done"><span class="box" aria-label="done">☑</span> Read <strong>a</strong></li>' +
        '<li class="task"><span class="box" aria-label="open">☐</span> Run it</li></ol>',
    );
    expect(renderMarkdown("- [link](x) stays")).not.toContain("task");
  });

  it("keeps lines of a paragraph and leaves snake_case and 2*3*4 alone", () => {
    expect(renderMarkdown("one\ntwo")).toBe("<p>one<br>two</p>");
    expect(renderMarkdown("grant_minutes and 2*3*4")).toBe("<p>grant_minutes and 2*3*4</p>");
  });
});
