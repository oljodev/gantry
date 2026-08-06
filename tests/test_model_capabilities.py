"""Model capability guard: the registry, the vision pre-pass, and the routing
decision that connects them.

The behaviour under test is the promise of the whole feature: a prompt carrying
an image reaches a vision model as an image, reaches a text-only worker as a
transcription, and never reaches either as a provider error.
"""

from __future__ import annotations

import uuid
from typing import Any

import pytest

from gantry.attachments import vision
from gantry.attachments.capabilities import (
    Modality,
    capabilities_for,
    missing_modalities,
    normalize_slug,
    supports,
)
from gantry.attachments.context import goal_message
from gantry.attachments.extract import AttachmentKind
from gantry.attachments.prepare import (
    delivered_as_text,
    prepare_task_attachments,
    unreadable_modalities,
)
from gantry.attachments.snapshot import PAYLOAD_KEY, inheritable
from gantry.attachments.storage import AttachmentNotStored
from gantry.runtime.llm import LLMResponse, LLMUsage, Message, ToolSchema
from gantry.runtime.state import initial_messages

PNG_BYTES = b"\x89PNG\r\n\x1a\n" + b"fake pixels"


# --- the registry --------------------------------------------------------


@pytest.mark.parametrize(
    "model",
    [
        "anthropic/claude-opus-4-8",
        "anthropic/claude-haiku-4-5",
        "claude-sonnet-5",
        "openrouter/anthropic/claude-3.5-sonnet",
        "bedrock/anthropic.claude-opus-4-8",
    ],
)
def test_claude_reads_images_and_pdfs(model: str) -> None:
    assert supports(model, Modality.IMAGE)
    assert supports(model, Modality.PDF)


@pytest.mark.parametrize(
    "model",
    [
        "openrouter/deepseek/deepseek-r1",
        "deepseek/deepseek-r1",
        "deepseek-chat",
        "openrouter/qwen/qwen3-coder",
        "qwen2.5-coder-32b-instruct",
        "openai/gpt-3.5-turbo",
        "mistralai/mistral-large",
        "openrouter/moonshotai/kimi-k2",
    ],
)
def test_coder_and_reasoning_models_are_text_only(model: str) -> None:
    """The models a swarm actually routes cheap work to. Getting any of these
    wrong is what sends an image into a provider that 400s on it."""
    assert capabilities_for(model) == frozenset({Modality.TEXT})
    assert not supports(model, Modality.IMAGE)


@pytest.mark.parametrize(
    "model",
    ["openai/gpt-4o", "openai/gpt-4o-mini", "openai/gpt-5", "openai/o3", "gpt-4.1"],
)
def test_openai_vision_models(model: str) -> None:
    assert supports(model, Modality.IMAGE)
    # OpenAI takes images but not PDFs as native document blocks.
    assert not supports(model, Modality.PDF)


def test_gemini_is_the_omni_family() -> None:
    caps = capabilities_for("gemini/gemini-2.5-pro")
    assert Modality.AUDIO in caps
    assert Modality.VIDEO in caps
    assert Modality.IMAGE in caps


@pytest.mark.parametrize(
    "model",
    ["openrouter/qwen/qwen2.5-vl-72b-instruct", "mistralai/pixtral-large", "llama-3.2-90b-vision"],
)
def test_open_weight_vision_variants_beat_their_text_only_family(model: str) -> None:
    """``qwen`` is text-only but ``qwen-vl`` is not — rule order has to encode
    that, or every VLM in the menu gets a pointless transcription pre-pass."""
    assert supports(model, Modality.IMAGE)


def test_unknown_models_default_to_text_only() -> None:
    """The safe direction: under-claiming costs one cheap pre-pass, over-claiming
    is a hard provider rejection mid-run."""
    assert capabilities_for("some-vendor/brand-new-model-v9") == frozenset({Modality.TEXT})
    assert capabilities_for("") == frozenset({Modality.TEXT})


def test_matching_ignores_case_and_route_prefixes() -> None:
    assert (
        normalize_slug("  OpenRouter/DeepSeek/DeepSeek-R1  ") == "openrouter/deepseek/deepseek-r1"
    )
    assert capabilities_for("OPENROUTER/ANTHROPIC/CLAUDE-3.5-SONNET") == capabilities_for(
        "openrouter/anthropic/claude-3.5-sonnet"
    )


def test_missing_modalities_reports_exactly_what_needs_a_pre_pass() -> None:
    wanted = [Modality.TEXT, Modality.IMAGE, Modality.VIDEO]
    assert missing_modalities("openrouter/deepseek/deepseek-r1", wanted) == frozenset(
        {Modality.IMAGE, Modality.VIDEO}
    )
    assert missing_modalities("anthropic/claude-opus-4-8", wanted) == frozenset({Modality.VIDEO})
    assert missing_modalities("gemini/gemini-2.5-flash", wanted) == frozenset()


# --- picking the transcriber --------------------------------------------


@pytest.mark.parametrize(
    ("worker_model", "expected_prefix"),
    [
        ("openrouter/deepseek/deepseek-r1", "openrouter/"),
        ("anthropic/claude-opus-4-8", "anthropic/"),
        ("openai/gpt-3.5-turbo", "openai/"),
        ("gemini/gemini-2.0-flash", "gemini/"),
    ],
)
def test_default_vision_model_stays_on_the_workers_own_key(
    worker_model: str, expected_prefix: str
) -> None:
    """A worker holds exactly one decrypted key, so the fallback vision model has
    to route through the same provider or the pre-pass fails to authenticate."""
    picked = vision.default_vision_model(worker_model)
    assert picked is not None
    assert picked.startswith(expected_prefix)
    assert supports(picked, Modality.IMAGE)


def test_no_default_vision_model_for_an_unknown_provider() -> None:
    assert vision.default_vision_model("openai/gpt-4o") is not None
    assert vision.default_vision_model("some-local-vllm-model") is None


def test_configured_vision_model_overrides_the_default() -> None:
    picked = vision.resolve_vision_model("openrouter/deepseek/deepseek-r1", "openai/gpt-4o")
    assert picked == "openai/gpt-4o"


def test_a_text_only_override_is_refused_rather_than_used() -> None:
    """Pointing GANTRY_VISION_MODEL at a text-only slug must degrade to 'cannot
    read this file', never to a 400 halfway through the run."""
    assert (
        vision.resolve_vision_model("openrouter/qwen/qwen3-coder", "deepseek/deepseek-r1") is None
    )


def test_transcription_prompt_carries_the_image_and_the_goal() -> None:
    messages = vision.transcription_messages(
        "image/png", PNG_BYTES, filename="mock.png", goal="Rebuild this screen in React"
    )
    assert messages[0]["role"] == "system"
    blocks = messages[1]["content"]
    assert isinstance(blocks, list)
    text = blocks[0]["text"]
    assert "mock.png" in text
    assert "Rebuild this screen in React" in text
    assert blocks[1]["image_url"]["url"].startswith("data:image/png;base64,")


# --- the routing decision -----------------------------------------------


class RecordingLLM:
    """An LLM that records what it was asked and answers with a fixed description."""

    def __init__(self, content: str = "A login form with two inputs.") -> None:
        self.calls: list[dict[str, Any]] = []
        self._content = content

    async def complete(
        self,
        *,
        model: str,
        messages: list[Message],
        tools: Any = (),
        on_delta: Any = None,
    ) -> LLMResponse:
        self.calls.append({"model": model, "messages": messages})
        return LLMResponse(content=self._content, model=model, usage=LLMUsage(10, 20))


class FailingLLM(RecordingLLM):
    async def complete(self, **kwargs: Any) -> LLMResponse:
        raise RuntimeError("provider exploded")


class FakeStore:
    def __init__(self, data: bytes | None = PNG_BYTES) -> None:
        self._data = data

    async def put(self, key: str, data: bytes) -> None: ...

    async def get(self, key: str) -> bytes:
        if self._data is None:
            raise AttachmentNotStored(key)
        return self._data

    async def delete(self, key: str) -> None: ...


class FakeRow:
    """Stands in for an Attachment ORM row (these tests need no database)."""

    def __init__(self, row_id: uuid.UUID, **kwargs: Any) -> None:
        self.id = row_id
        self.filename = kwargs.get("filename", "mock.png")
        self.media_type = kwargs.get("media_type", "image/png")
        self.storage_key = "key"
        self.transcript = kwargs.get("transcript", "")
        self.transcript_model = kwargs.get("transcript_model", "")


def _payload(**overrides: Any) -> dict[str, Any]:
    entry: dict[str, Any] = {
        "id": str(uuid.uuid4()),
        "filename": "mock.png",
        "media_type": "image/png",
        "kind": "image",
        "size_bytes": len(PNG_BYTES),
    }
    entry.update(overrides)
    return {"goal": "Build the login screen", PAYLOAD_KEY: [entry]}


async def _prepare(
    payload: dict[str, Any],
    model: str,
    *,
    llm: Any = None,
    store: Any = None,
    row: Any = None,
    saved: list[tuple[str, str]] | None = None,
    monkeypatch: pytest.MonkeyPatch,
    vision_model: str | None = None,
) -> Any:
    """Run the guard with the DB reads/writes stubbed out."""
    llm = llm or RecordingLLM()
    entry = payload[PAYLOAD_KEY][0]
    rows = {} if row is False else {str(entry["id"]): row or FakeRow(uuid.UUID(entry["id"]))}

    async def fake_rows(sessions: Any, entries: Any) -> Any:
        return rows

    async def fake_save(sessions: Any, r: Any, text: str, model_name: str) -> None:
        if saved is not None:
            saved.append((text, model_name))

    monkeypatch.setattr("gantry.attachments.prepare._load_rows", fake_rows)
    monkeypatch.setattr("gantry.attachments.prepare._save_transcript", fake_save)
    await prepare_task_attachments(
        None,  # type: ignore[arg-type]
        payload,
        model=model,
        llm=llm,
        store=store if store is not None else FakeStore(),
        vision_model=vision_model,
    )
    return llm


async def test_vision_model_gets_the_image_and_no_pre_pass_runs(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    payload = _payload()
    llm = await _prepare(payload, "anthropic/claude-opus-4-8", monkeypatch=monkeypatch)
    entry = payload[PAYLOAD_KEY][0]
    assert entry["data_url"].startswith("data:image/png;base64,")
    assert "transcript" not in entry
    assert llm.calls == []  # nothing was spent describing what the model can see


async def test_text_only_model_triggers_the_transcription_pre_pass(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    payload = _payload()
    saved: list[tuple[str, str]] = []
    llm = await _prepare(
        payload, "openrouter/deepseek/deepseek-r1", saved=saved, monkeypatch=monkeypatch
    )
    entry = payload[PAYLOAD_KEY][0]

    assert "data_url" not in entry  # never handed an image to a text-only model
    assert entry["transcript"] == "A login form with two inputs."
    assert entry["transcript_model"].startswith("openrouter/")
    # One call, on a vision model reachable with the worker's own key.
    assert len(llm.calls) == 1
    assert supports(llm.calls[0]["model"], Modality.IMAGE)
    # And it is cached, so a resume reuses the identical text.
    assert saved == [("A login form with two inputs.", entry["transcript_model"])]


async def test_a_cached_transcript_is_reused_without_calling_the_model(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    payload = _payload()
    row = FakeRow(
        uuid.UUID(payload[PAYLOAD_KEY][0]["id"]),
        transcript="Previously described.",
        transcript_model="openai/gpt-4o",
    )
    llm = await _prepare(payload, "openrouter/qwen/qwen3-coder", row=row, monkeypatch=monkeypatch)
    assert payload[PAYLOAD_KEY][0]["transcript"] == "Previously described."
    assert payload[PAYLOAD_KEY][0]["transcript_model"] == "openai/gpt-4o"
    assert llm.calls == []


async def test_no_available_vision_model_leaves_an_explicit_note(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """The agent is told the file exists and is unreadable — a fact it can act
    on. A silent omission is not."""
    payload = _payload()
    await _prepare(payload, "some-local-vlm-less-model", monkeypatch=monkeypatch)
    entry = payload[PAYLOAD_KEY][0]
    assert "transcript" not in entry
    assert "GANTRY_VISION_MODEL" in entry["note"]


async def test_a_failed_pre_pass_degrades_instead_of_failing_the_task(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    payload = _payload()
    await _prepare(
        payload, "openrouter/deepseek/deepseek-r1", llm=FailingLLM(), monkeypatch=monkeypatch
    )
    assert "provider exploded" in payload[PAYLOAD_KEY][0]["note"]


async def test_a_missing_blob_degrades_instead_of_failing_the_task(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    payload = _payload()
    await _prepare(
        payload, "anthropic/claude-opus-4-8", store=FakeStore(None), monkeypatch=monkeypatch
    )
    assert "no longer available" in payload[PAYLOAD_KEY][0]["note"]


async def test_extracted_text_needs_no_routing_at_all(monkeypatch: pytest.MonkeyPatch) -> None:
    payload = _payload(kind="text", media_type="text/plain", text="spec body", filename="spec.md")
    llm = await _prepare(payload, "openrouter/deepseek/deepseek-r1", monkeypatch=monkeypatch)
    assert llm.calls == []
    assert payload[PAYLOAD_KEY][0]["text"] == "spec body"


def test_delivered_as_text_sends_a_scanned_pdf_to_the_pre_pass() -> None:
    readable = {"text": "the whole spec"}
    scanned: dict[str, Any] = {}
    assert delivered_as_text(AttachmentKind.PDF, readable) is True
    assert delivered_as_text(AttachmentKind.PDF, scanned) is False
    assert delivered_as_text(AttachmentKind.TEXT, scanned) is True
    assert delivered_as_text(AttachmentKind.IMAGE, readable) is False


def test_unreadable_modalities_is_the_guards_verdict_alone() -> None:
    payload = _payload()
    assert unreadable_modalities(payload, "openrouter/deepseek/deepseek-r1") == {Modality.IMAGE}
    assert unreadable_modalities(payload, "anthropic/claude-opus-4-8") == set()


# --- what the model finally reads ---------------------------------------


def test_goal_message_stays_a_plain_string_without_attachments() -> None:
    assert goal_message("Do the thing", []) == {"role": "user", "content": "Do the thing"}


def test_goal_message_inlines_extracted_text_for_a_text_only_worker() -> None:
    message = goal_message(
        "Implement the spec",
        [
            {
                "filename": "spec.md",
                "media_type": "text/markdown",
                "kind": "text",
                "size_bytes": 20,
                "text": "## Requirements\n- login",
            }
        ],
    )
    assert isinstance(message["content"], str)
    assert "Implement the spec" in message["content"]
    assert "spec.md" in message["content"]
    assert "## Requirements" in message["content"]


def test_goal_message_promotes_to_blocks_only_for_real_images() -> None:
    message = goal_message(
        "Rebuild this",
        [
            {
                "filename": "mock.png",
                "media_type": "image/png",
                "kind": "image",
                "size_bytes": 100,
                "data_url": "data:image/png;base64,AAAA",
            }
        ],
    )
    blocks = message["content"]
    assert isinstance(blocks, list)
    assert blocks[0]["type"] == "text"
    assert blocks[1] == {"type": "image_url", "image_url": {"url": "data:image/png;base64,AAAA"}}


def test_goal_message_gives_a_text_only_worker_the_transcription_not_a_block() -> None:
    message = goal_message(
        "Rebuild this",
        [
            {
                "filename": "mock.png",
                "media_type": "image/png",
                "kind": "image",
                "size_bytes": 100,
                "transcript": "A login form with two inputs.",
                "transcript_model": "openai/gpt-4o",
            }
        ],
    )
    assert isinstance(message["content"], str)
    assert "A login form with two inputs." in message["content"]
    assert "openai/gpt-4o" in message["content"]


def test_goal_message_never_silently_drops_an_unreadable_file() -> None:
    message = goal_message(
        "Do it",
        [
            {
                "filename": "clip.mov",
                "media_type": "video/quicktime",
                "kind": "video",
                "size_bytes": 9,
            }
        ],
    )
    assert isinstance(message["content"], str)
    assert "clip.mov" in message["content"]
    assert "could not be included" in message["content"]


def test_attachments_ride_on_the_goal_anchor_so_compaction_keeps_them() -> None:
    """The goal message is an untouchable compaction anchor. Folding attachments
    into it is what makes a spec survive a long run."""
    payload = {
        "goal": "Build it",
        PAYLOAD_KEY: [
            {
                "filename": "spec.md",
                "media_type": "text/markdown",
                "kind": "text",
                "size_bytes": 5,
                "text": "the spec",
            }
        ],
    }
    messages = initial_messages(payload)
    assert len(messages) == 2
    assert messages[0].message["role"] == "system"
    assert "the spec" in str(messages[1].message["content"])
    # Pure function of the payload: a resume reconstructs it byte-identically.
    assert initial_messages(payload)[1].message == messages[1].message


def test_children_inherit_the_snapshot_but_not_the_parents_resolution() -> None:
    """A leader on a vision model must not push its base64 image (or its own
    transcript) into every child payload — each child re-resolves for ITS model."""
    parent = [
        {
            "id": "abc",
            "filename": "mock.png",
            "kind": "image",
            "media_type": "image/png",
            "size_bytes": 100,
            "data_url": "data:image/png;base64," + "A" * 10_000,
            "transcript": "described for the leader",
            "transcript_model": "anthropic/claude-haiku-4-5",
            "note": "",
        }
    ]
    child = inheritable(parent)
    assert child == [
        {
            "id": "abc",
            "filename": "mock.png",
            "kind": "image",
            "media_type": "image/png",
            "size_bytes": 100,
        }
    ]


def test_llm_protocol_accepts_the_transcription_messages() -> None:
    """Type-level guard: the pre-pass builds ordinary OpenAI-style messages, so
    it goes through the same client as every other call."""
    messages: list[Message] = vision.transcription_messages("image/png", PNG_BYTES)
    tools: tuple[ToolSchema, ...] = ()
    assert len(messages) == 2
    assert tools == ()
