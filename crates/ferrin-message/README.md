# ferrin-message

Application-level messages for Ferrin: `Message` (system/user/assistant/tool
with string-or-parts content), user/assistant/tool parts, `FileSource`
(bytes, base64, URL, provider reference, inline text, local path), RFC 2397
`data:` URL parsing and `prune` for trimming reasoning, tool calls and empty
messages from a history.

Conversion to provider prompts lives in `ferrin-core`.

Part of the [Ferrin](../../README.md) workspace. Design:
`docs/01-architecture/03-core-data-model.md` §7,
`docs/01-architecture/05-prompt-conversion.md`.
