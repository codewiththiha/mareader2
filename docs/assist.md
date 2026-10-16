# AI assist (`crates/assist`)

The study tools ask an AI for answers: Explain today, more tasks later.
`mareader-assist` keeps three things apart so each can change alone:

| Piece | What it is | Change it to… |
|---|---|---|
| **Task** | a `Prompt` (system text, the reader's text, a JSON Schema) and how to read the answer | ask something new |
| **Backend** | sends a `Prompt`, returns JSON, reports partial answers | add a provider |
| **Service** | `Assist`: a runtime off the UI thread, clients, keys, the settings' jobs | change how the app holds it all |

Every provider gets the same `Prompt`. Nothing in a task knows who
answers, and nothing in a backend knows what was asked.

## The seam

```rust,ignore
pub trait Task: Send + Sync {
    type Output: Send;
    fn prompt(&self) -> Prompt;
    /// A partial answer may lack fields; a whole one may not.
    fn read(&self, answer: &Value, whole: bool) -> Option<Self::Output>;
}

pub trait Backend: Send + Sync {
    fn ask<'a>(&'a self, prompt: &'a Prompt, partial: Partial<'a>)
        -> BoxFuture<'a, Result<Value, Failure>>;
}

// Any task on any backend; `partial` sees each readable answer so far.
let info = run(backend, &Explain { word, context }, &|p| show(p)).await?;
```

Errors are `mareader_study::Failure`: a `Why` the card can explain
(no key, keys turned away, offline, busy, a model to check…) plus a
short detail from the provider.

## Adding a task

1. Write a struct with what the task needs, e.g. `Summarise { text }`.
2. `prompt()`: system text, then the reader's text through `quote()`, which
   flattens it into one JSON string so it reads as data, not instructions.
   The schema must be closed (`additionalProperties: false`), list every
   field in `required`, and use no `$ref`. Apple's guided generation and
   OpenAI's strict mode both need that.
3. `read()`: turn the JSON into the output, leniently. Models wrap objects
   and send lists as comma-separated strings; `Explain::read` shows how.
   Return `None` for a whole answer that is missing what matters.
4. Call `run(backend, &task, partial)`. Every backend already works.

## Adding a provider

Implement `Backend::ask` for it, stream if it can, and map its errors to a
`Failure`. Two exist:

- **`Apple`**, through the [fm-bridge](https://github.com/codewiththiha/fm-bridge)
  helper: the schema goes in as guided generation, snapshots stream as
  partials. Only on macOS 26 with Apple Intelligence; `probe()` says why
  not. Packaging the helper is in `docs/packaging.md`.
- **`Compat`**, any OpenAI-compatible server through
  [ai-pool](https://github.com/codewiththiha/ai-pool), which brings keys,
  rotation, retries and streaming. It asks for `json_schema` first. If the
  server refuses the format, it retries with `json_object` and the schema
  in the system text, then with no format at all. Answers are read from
  the streamed text, cutting the JSON at the last point that parses.

A server-shaped provider usually needs only a `Provider` preset in
`crates/settings/src/ai.rs` (address, default model, whether it takes
keys). Anything else implements `Backend` and gets a match arm in
`Assist::backend`.

## Providers and settings

`AiSettings` holds the chosen `Provider` and an `Endpoint` (address,
model) for each server. Blank fields mean the preset's default. A blank
local model means the server's first model.

| Provider | Answers through | Keys |
|---|---|---|
| Automatic | Apple Intelligence where available; on other systems shown as None | — |
| Apple Intelligence | the fm-bridge helper (macOS only) | — |
| OpenAI, Gemini, OpenRouter | their OpenAI-compatible APIs | required |
| Ollama, LM Studio | the local server | — |
| Custom | any OpenAI-compatible address | optional |

Settings are only saved, never checked, as they are typed. Addresses
(http or https only), keys and models are checked when used, and the
answer says what's wrong.

## Keys

- One encrypted SQLite store, `ai/keys.sqlite` in the app's data folder.
  Each provider's keys are a separate scope.
- The master key sits in the system keychain under the app's bundle id, or
  in `ai/master.key` where there is no keychain. A marker file records the
  keychain choice, so a locked keychain gets reported, never quietly
  replaced by a new key that would orphan the store.
- With several keys, requests take turns. A key the provider rejects is
  set aside with the reason; a rate-limited key rests and comes back. The
  AI tab adds, sets aside, brings back and removes keys, and shows only a
  key's ends.
- A pasted key must be one ASCII word, up to 1024 characters.

## The service in the app

`Assist::new(root, waker)` starts a small Tokio runtime. The UI owns it
through `crates/ui/src/ai.rs`:

- each frame, `follow(&settings.ai)` takes in finished jobs and, when the
  AI settings changed, hands the study tools a new `Explainer`;
- the AI tab sends `Job`s (probe Apple, list keys or models, add, set
  aside, bring back or remove a key, try the provider) and draws the
  `Board`, plain data that each finished job updates. Then `waker`
  repaints.

`Explainer::explain` runs on the study tools' worker thread and blocks on
the runtime there, never on the UI thread. The browser build has no
assist: the AI tab is hidden and Explain says it can't answer.

## Tests

- `tests/compat.rs`: streaming with the schema sent, the format
  fallback, a blank model taking the server's first, error mapping.
- `tests/service.rs`: a keyless local server; keys added, used, rotated,
  set aside, brought back and removed; providers that can't answer saying
  why; Apple Intelligence through a scripted helper (Unix).

A small HTTP server in `tests/support` stands in for providers; it
records what it was asked. Nothing in CI calls a real provider.
