Recorded and hand-written OpenRouter streams replayed through the real decoder and parser in
`tests/openrouter.rs`. `live-capture.sse` is a real response captured with the `curl` command in
`docs/dev/setup.md` (the shell cut it off mid-stream, which makes it a real early-close case: the test
expects a `StreamInterrupted` at the end); the others are shaped after OpenRouter's documented format. No file here
contains a key.
