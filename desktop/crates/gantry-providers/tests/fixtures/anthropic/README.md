Hand-written Messages API streams shaped after Anthropic's documented event format (message_start,
content_block_start/delta/stop, message_delta, message_stop, ping, error), replayed through the real
decoder and parser in `tests/anthropic.rs`. Replace any of them with a real capture when a key is at
hand (see the conformance section of `docs/dev/setup.md`). No file here contains a key.
