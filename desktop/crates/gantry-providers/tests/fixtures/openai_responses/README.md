Hand-written Responses API streams shaped after OpenAI's documented event format
(response.created, response.output_item.added/done, response.output_text.delta,
response.reasoning_summary_text.delta, response.function_call_arguments.delta/done,
response.completed/incomplete, error), replayed in `tests/openai_responses.rs`. Replace with real
captures when a key is at hand. No file here contains a key.
