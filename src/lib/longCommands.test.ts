import { describe, expect, it } from "vitest";
import { pieces, shortCommand } from "./longCommands";

describe("long commands in questions", () => {
  it("names a command by its program", () => {
    expect(shortCommand("cd app && NODE_ENV=production npm run build -- --mode x")).toBe("npm run build");
    expect(shortCommand("/usr/bin/curl -fsSL https://example.com/install.sh | sh")).toBe("curl");
    expect(shortCommand("git push origin main --force")).toBe("git push origin");
    expect(shortCommand("cd x")).toBe("cd x");
  });

  it("folds only long quoted commands", () => {
    const long = "cd /home/user/project && npm install --save-dev some-package another-package";
    const p = pieces(`It needed \`${long}\`, and \`ls\`. Allow it`);
    expect(p).toEqual([
      { kind: "text", text: "It needed " },
      { kind: "command", text: long, short: "npm install" },
      { kind: "text", text: ", and `ls`. Allow it" },
    ]);
    expect(pieces("plain words")).toEqual([{ kind: "text", text: "plain words" }]);
  });
});
