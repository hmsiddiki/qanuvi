import { describe, expect, it } from "vitest";
import {
  absPath,
  adapterFor,
  defaultConfigs,
  errorOf,
  foldRenamed,
  forAdapter,
  launchTemplate,
  readLaunch,
  relPath,
  sourceBreakpoints,
  stripJsonc,
  substitute,
  toggleLine,
  unsupported,
} from "./protocol";

const ctx = {
  root: "E:\\Work\\shop",
  file: "src/app.js",
  line: 7,
  selectedText: "",
  env: { PORT: "3000" },
  home: "C:\\Users\\Test",
};

describe("launch.json", () => {
  it("reads comments and trailing commas as VS Code does", () => {
    const text = `{
      // A comment
      "version": "0.2.0",
      /* block */ "configurations": [
        { "type": "node", "request": "launch", "name": "Run // not a comment", "program": "\${file}", },
        { "type": "node", "request": "nope", "name": "skipped" },
      ],
    }`;
    expect(JSON.parse(stripJsonc('{"a": "x, }",}'))).toEqual({ a: "x, }" });
    const r = readLaunch(text);
    expect(r.error).toBe("");
    expect(r.configs.map((c) => c.name)).toEqual(["Run // not a comment"]);
  });

  it("says why a file cannot be used", () => {
    expect(readLaunch("{ nope").error).toMatch(/not valid JSON/);
    expect(readLaunch("{}").error).toMatch(/no "configurations"/);
  });

  it("writes a template it can read back", () => {
    const r = readLaunch(launchTemplate());
    expect(r.configs[0]).toMatchObject({ type: "node", request: "launch", program: "${file}" });
  });
});

describe("variables", () => {
  it("fills in VS Code's variables", () => {
    const { config, unknown } = substitute(
      {
        name: "x",
        type: "node",
        request: "launch",
        program: "${file}",
        cwd: "${workspaceFolder}",
        args: ["${fileBasenameNoExtension}", "${relativeFile}", "${env:PORT}", "${env:NONE}", "${lineNumber}"],
        env: { DIR: "${fileDirname}", ROOT: "${workspaceFolderBasename}", Q: "${input:pick}" },
      },
      ctx,
    );
    expect(config.program).toBe("E:\\Work\\shop\\src\\app.js");
    expect(config.cwd).toBe("E:\\Work\\shop");
    expect(config.args).toEqual(["app", "src\\app.js", "3000", "", "7"]);
    expect(config.env).toEqual({ DIR: "E:\\Work\\shop\\src", ROOT: "shop", Q: "${input:pick}" });
    expect(unknown).toEqual(["${input:pick}"]);
  });

  it("uses forward slashes for a project on Linux or macOS", () => {
    const { config } = substitute(
      { name: "x", type: "node", request: "launch", program: "${file}" },
      { ...ctx, root: "/home/test/shop/" },
    );
    expect(config.program).toBe("/home/test/shop/src/app.js");
  });
});

describe("paths", () => {
  it("goes between relative and full paths", () => {
    expect(absPath("E:\\Work\\shop", "src/app.js")).toBe("E:\\Work\\shop\\src\\app.js");
    expect(relPath("E:\\Work\\shop", "e:\\work\\Shop\\src\\app.js")).toBe("src/app.js");
    expect(relPath("E:\\Work\\shop", "file:///E:/Work/shop/a.js")).toBe("a.js");
    expect(relPath("E:\\Work\\shop", "E:\\Work\\shopping\\a.js")).toBeNull();
    expect(relPath("/home/t/shop", "/home/t/Shop/a.js")).toBeNull();
    expect(relPath("/home/t/shop", "/home/t/shop/lib/a.js")).toBe("lib/a.js");
  });
});

describe("configurations", () => {
  it("knows which debugger serves a type", () => {
    expect(adapterFor("node")).toBe("node");
    expect(adapterFor("pwa-node")).toBe("node");
    expect(adapterFor("python")).toBeNull();
    expect(unsupported({ name: "p", type: "python", request: "launch" })).toMatch(/no debugger for "python"/);
    expect(unsupported({ name: "n", type: "node", request: "launch" })).toBe("");
    expect(forAdapter("node", { name: "n", type: "node", request: "launch" }).type).toBe("pwa-node");
  });

  it("offers to run the shown script", () => {
    expect(defaultConfigs("src/a.ts").map((c) => c.type)).toEqual(["node", "node", "php"]);
    expect(defaultConfigs("index.php").map((c) => c.name)).toEqual([
      "PHP: run the current file",
      "PHP: listen for Xdebug (port 9003)",
      "Node: attach (port 9229)",
    ]);
    expect(defaultConfigs("README.md").map((c) => c.request)).toEqual(["attach", "launch"]);
  });

  it("writes VS Code's PHP template for a PHP file", () => {
    const r = readLaunch(launchTemplate("web/index.php"));
    expect(r.configs.map((c) => c.name)).toEqual(["Listen for Xdebug", "Launch currently open script"]);
    expect(r.configs[1]).toMatchObject({ type: "php", program: "${file}", port: 0 });
  });

  const php = {
    php: "E:\\php\\php.exe",
    version: "8.2.3",
    build: "NTS x64",
    xdebug: "3.5.3",
    via: "file",
    note: "",
    xdebugFile: "E:\\x\\php_xdebug.dll",
  };

  it("gives a PHP run its PHP, the Xdebug file and the Xdebug settings", () => {
    expect(adapterFor("php")).toBe("php");
    const c = forAdapter("php", { name: "r", type: "php", request: "launch", program: "a.php", port: 0 }, php);
    expect(c.runtimeExecutable).toBe("E:\\php\\php.exe");
    expect(c.runtimeArgs).toEqual([
      "-dzend_extension=E:\\x\\php_xdebug.dll",
      "-dxdebug.mode=debug",
      "-dxdebug.start_with_request=yes",
      "-dxdebug.client_host=127.0.0.1",
      "-dxdebug.client_port=${port}",
    ]);
  });

  it("keeps the configuration's own PHP and settings", () => {
    const c = forAdapter(
      "php",
      {
        name: "r",
        type: "php",
        request: "launch",
        program: "a.php",
        runtimeExecutable: "php8",
        runtimeArgs: ["-dxdebug.mode=debug,develop", "-n"],
      },
      { ...php, via: "ini" },
    );
    expect(c.runtimeExecutable).toBe("php8");
    expect(c.runtimeArgs).toEqual([
      "-dxdebug.start_with_request=yes",
      "-dxdebug.client_host=127.0.0.1",
      "-dxdebug.client_port=${port}",
      "-dxdebug.mode=debug,develop",
      "-n",
    ]);
  });

  it("leaves a listen-only configuration alone", () => {
    const listen = { name: "l", type: "php", request: "launch" as const, port: 9003 };
    expect(forAdapter("php", listen, php)).toEqual(listen);
  });

  it("leaves ${port} for PHP Debug to fill", () => {
    const { config, unknown } = substitute(
      { name: "r", type: "php", request: "launch", runtimeArgs: ["-dxdebug.client_port=${port}"] },
      ctx,
    );
    expect(unknown).toEqual([]);
    expect(config.runtimeArgs).toEqual(["-dxdebug.client_port=${port}"]);
  });
});

describe("Xdebug output", () => {
  it("takes out the renamed-setting warnings and names the old keys", () => {
    const out = [
      "Xdebug: [Config] The setting 'xdebug.remote_enable' has been renamed, see the upgrading guide at https://x (See: y)",
      "PHP Warning:  Xdebug: [Config] The setting 'xdebug.remote_port' has been renamed, see z",
      "total is 16",
    ].join("\n");
    expect(foldRenamed(out)).toEqual({ text: "total is 16", renamed: ["xdebug.remote_enable", "xdebug.remote_port"] });
    expect(foldRenamed("hello\n")).toEqual({ text: "hello\n", renamed: [] });
  });
});

describe("breakpoints", () => {
  it("toggles a line and sends only the enabled ones", () => {
    let list = toggleLine([], 9);
    list = toggleLine(list, 3);
    expect(list.map((b) => b.line)).toEqual([3, 9]);
    list = toggleLine(list, 9);
    expect(list.map((b) => b.line)).toEqual([3]);
    list = [...list, { line: 5, enabled: false }, { line: 6, enabled: true, condition: "i > 2" }];
    expect(sourceBreakpoints(list)).toEqual([{ line: 3 }, { line: 6, condition: "i > 2" }]);
  });

  it("reads an adapter's error", () => {
    expect(errorOf({ body: { error: { format: "Cannot find {path}", variables: { path: "a.js" } } } })).toBe(
      "Cannot find a.js",
    );
    expect(errorOf({ message: "nope" })).toBe("nope");
  });
});
