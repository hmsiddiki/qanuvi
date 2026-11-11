// Runs inside the test browser's page (Milestone 15). Called as (this function)(mode, arg, max) and answers plain
// values. Interactive elements get a short reference number (data-qx) so an agent can click or type by number.
(mode, arg, max) => {
  const root = document.documentElement;
  let next = Number(root.dataset.qxNext || 0);
  const ref = (el) => {
    if (!el.dataset.qx) el.dataset.qx = String(++next);
    return el.dataset.qx;
  };
  const visible = (el) => {
    const r = el.getBoundingClientRect();
    if (r.width <= 0 || r.height <= 0) return false;
    const s = getComputedStyle(el);
    return s.visibility !== "hidden" && s.display !== "none" && Number(s.opacity) > 0;
  };
  const clean = (t) =>
    String(t || "")
      .replace(/[ \t ]+/g, " ")
      .split("\n")
      .map((l) => l.trim())
      .filter(Boolean)
      .join("\n");
  const short = (t, n) => {
    const s = clean(t).replace(/\n/g, " ");
    return s.length > n ? s.slice(0, n) + "…" : s;
  };
  const cut = (t, n) => (t.length > n ? t.slice(0, n) + "\n… (" + (t.length - n) + " more characters)" : t);
  const label = (el) => {
    const byFor = el.labels && el.labels[0] ? el.labels[0].innerText : "";
    return short(byFor || el.getAttribute("aria-label") || el.getAttribute("placeholder") || el.title || "", 60);
  };
  const FIELDS = "input:not([type=hidden]), textarea, select, [contenteditable=''], [contenteditable=true]";
  const BUTTONS = "button, [role=button], input[type=submit], input[type=button], input[type=reset], summary";
  const finish = (v) => {
    root.dataset.qxNext = String(next);
    return v;
  };
  const find = (a) => {
    if (a.ref) return document.querySelector('[data-qx="' + String(a.ref).replace(/[^0-9]/g, "") + '"]');
    if (a.selector) return document.querySelector(a.selector);
    if (a.text) {
      const want = String(a.text).trim().toLowerCase();
      const all = [...document.querySelectorAll(BUTTONS + ", a, label, " + FIELDS)].filter(visible);
      const text = (el) => short(el.innerText || el.value || label(el) || "", 200).toLowerCase();
      return all.find((el) => text(el) === want) || all.find((el) => text(el).includes(want)) || null;
    }
    return null;
  };
  const describe = (el) => {
    const tag = el.tagName.toLowerCase();
    const type = el.getAttribute("type");
    const words = short(el.innerText || el.value || label(el) || el.getAttribute("href") || "", 60);
    return "[" + ref(el) + "] " + tag + (type ? "[" + type + "]" : "") + (words ? ' "' + words + '"' : "");
  };
  const path = (el) => {
    const parts = [];
    for (let e = el; e && e.nodeType === 1 && e !== document.body; e = e.parentElement) {
      if (e.id) {
        parts.unshift("#" + CSS.escape(e.id));
        break;
      }
      let part = e.tagName.toLowerCase();
      const same = e.parentElement ? [...e.parentElement.children].filter((c) => c.tagName === e.tagName) : [];
      if (same.length > 1) part += ":nth-of-type(" + (same.indexOf(e) + 1) + ")";
      parts.unshift(part);
    }
    return parts.join(" > ") || "body";
  };

  if (mode === "ready") return document.readyState;
  if (mode === "text") {
    const el = arg && arg.selector ? document.querySelector(arg.selector) : document.body;
    if (!el) return finish({ error: "No element matches " + arg.selector });
    return finish({ title: document.title, url: location.href, text: cut(clean(el.innerText), max) });
  }
  if (mode === "summary") {
    const links = [...document.querySelectorAll("a[href]")].filter(visible).length;
    const fields = [...document.querySelectorAll(FIELDS)].filter(visible).length;
    const buttons = [...document.querySelectorAll(BUTTONS)].filter(visible).length;
    const headings = [...document.querySelectorAll("h1, h2, h3")]
      .filter(visible)
      .slice(0, 12)
      .map((h) => h.tagName.toLowerCase() + ": " + short(h.innerText, 80));
    return finish({
      title: document.title,
      url: location.href,
      links,
      fields,
      buttons,
      headings,
      text: cut(clean(document.body ? document.body.innerText : ""), max),
    });
  }
  if (mode === "links") {
    const out = [...document.querySelectorAll("a[href]")]
      .filter(visible)
      .slice(0, max)
      .map(
        (a) =>
          "[" +
          ref(a) +
          "] " +
          (short(a.innerText || a.getAttribute("aria-label") || "", 60) || "(no text)") +
          " → " +
          a.getAttribute("href"),
      );
    return finish(out);
  }
  if (mode === "fields") {
    const out = [];
    for (const el of [...document.querySelectorAll(FIELDS + ", " + BUTTONS)].filter(visible).slice(0, max)) {
      const tag = el.tagName.toLowerCase();
      const isButton = el.matches(BUTTONS);
      let line = "[" + ref(el) + "] " + (isButton ? "button" : tag === "input" ? "input " + (el.type || "text") : tag);
      if (isButton) line += ' "' + short(el.innerText || el.value || el.getAttribute("aria-label") || "", 60) + '"';
      else {
        if (el.name) line += ' name="' + el.name + '"';
        const l = label(el);
        if (l) line += ' label="' + l + '"';
        if (tag === "select")
          line +=
            " options: " +
            [...el.options]
              .slice(0, 12)
              .map((o) => (o.selected ? "*" : "") + short(o.text, 30))
              .join(" | ");
        else if (el.type === "checkbox" || el.type === "radio") line += el.checked ? " checked" : " unchecked";
        else if (el.type !== "password" && "value" in el && el.value) line += ' value="' + short(el.value, 60) + '"';
        if (el.required) line += " required";
      }
      if (el.disabled) line += " disabled";
      if (el.form && el.form.id) line += " (form #" + el.form.id + ")";
      out.push(line);
    }
    return finish(out);
  }
  if (mode === "target") {
    const el = find(arg);
    if (!el)
      return finish({
        error: "No element found for " + JSON.stringify(arg) + ". List them with browser_fields or browser_links.",
      });
    el.scrollIntoView({ block: "center", inline: "center" });
    const r = el.getBoundingClientRect();
    const x = r.left + r.width / 2;
    const y = r.top + r.height / 2;
    const top = document.elementFromPoint(x, y);
    const covered = top && top !== el && !el.contains(top) && !top.contains(el);
    return finish({ x, y, what: describe(el), covered: covered ? describe(top) : "", disabled: !!el.disabled });
  }
  if (mode === "fill") {
    let el = find(arg);
    if (el && el.tagName === "LABEL") el = el.control || el.querySelector(FIELDS);
    if (!el) return finish({ error: "No field found for " + JSON.stringify(arg) + ". List them with browser_fields." });
    el.scrollIntoView({ block: "center" });
    if (el.tagName === "SELECT") {
      const want = String(arg.value).toLowerCase();
      const opt = [...el.options].find((o) => o.value.toLowerCase() === want || o.text.trim().toLowerCase() === want);
      if (!opt) return finish({ error: "No option " + JSON.stringify(arg.value) + " in " + describe(el) });
      el.value = opt.value;
      el.dispatchEvent(new Event("input", { bubbles: true }));
      el.dispatchEvent(new Event("change", { bubbles: true }));
      return finish({ what: describe(el), done: true });
    }
    el.focus();
    if (arg.clear !== false) {
      if ("value" in el) el.value = "";
      else el.textContent = "";
      el.dispatchEvent(new Event("input", { bubbles: true }));
    }
    return finish({ what: describe(el), done: false });
  }
  if (mode === "rect") {
    const el = find(arg);
    if (!el) return finish({ error: "No element found for " + JSON.stringify(arg) });
    el.scrollIntoView({ block: "center" });
    const r = el.getBoundingClientRect();
    return finish({ x: r.left + scrollX, y: r.top + scrollY, width: r.width, height: r.height });
  }
  if (mode === "at") {
    const el = document.elementFromPoint(arg.x, arg.y);
    if (!el) return finish({ error: "Nothing at that point." });
    const r = el.getBoundingClientRect();
    const html = el.outerHTML;
    return finish({
      selector: path(el),
      what: describe(el),
      text: short(el.innerText || "", 300),
      html: html.length > 600 ? html.slice(0, 600) + "…" : html,
      rect: { x: r.left, y: r.top, width: r.width, height: r.height },
      url: location.href,
      title: document.title,
    });
  }
  if (mode === "scroll") {
    scrollBy(0, arg.dy || 0);
    return scrollY;
  }
  return finish({ error: "Unknown mode " + mode });
};
