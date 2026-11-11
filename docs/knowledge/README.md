# Publishing Qanuvi's knowledge

Qanuvi checks once a day for a newer knowledge file: setup guides and the list of free providers. It takes a file
only when its signature matches the public key built into Qanuvi (`src-tauri/src/knowledge.rs`), shows the user what
changes, and uses it after the user's yes.

`knowledge.json` here is the source of the published file.

## Publish a new version

1. Edit `knowledge.json`:
   - raise `version` by one (Qanuvi offers only a version newer than the one it uses);
   - set `published` to today;
   - say what changed in `note`.
2. Copy it to the published folder and sign it there:

   ```
   copy docs\knowledge\knowledge.json <web folder>\qanuvi-updates\knowledge.json
   src-tauri\target\release\qanuvi-knowledge.exe sign <web folder>\qanuvi-updates\knowledge.json <private key file>
   ```

   Signing writes `knowledge.json.sig` next to the file. Both files must be published together; a file changed after
   signing is refused by every Qanuvi.
3. In Qanuvi: Agents → Qanuvi agent → "Check for newer".

The published address is `knowledge::DEFAULT_URL` (the test host for now; qanuvi.com later).

## The key

- The private key file stays with the publisher: never in this repository and never in the published folder. Keep a
  backup; without it no new knowledge can be published for the Qanuvi versions already out.
- `qanuvi-knowledge keygen <file>` makes a new pair (it never replaces an existing file). A new pair means a new
  public key in `knowledge.rs` and a new Qanuvi release; older Qanuvi versions keep trusting only the old key.

## Format

```json
{
  "format": 1,
  "version": 2,
  "published": "2026-11-12",
  "note": "What changed",
  "guides": [],
  "freeProviders": [{ "id": "groq", "name": "Groq", "how": "key", "url": "https://console.groq.com/keys", "note": "" }],
  "freeProvidersCheckedOn": "2026-11-12"
}
```

`guides` use the same shape as the guides built into Qanuvi (`src/lib/agents/guides.ts`); either list may be left out.
Addresses must be https.
