Hand-written Interactions API streams shaped after Google's reference as read on 2026-09-07
(interaction.created, step.start/delta/stop with text, thought_summary, arguments_delta and
thought_signature deltas, interaction.completed, error), replayed in `tests/gemini.rs`. The live
conformance run is where a renamed event shows up; replace these with real captures then. No file
here contains a key.
