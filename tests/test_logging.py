from __future__ import annotations

import json
import logging

import pytest

from gantry.config import Settings
from gantry.logging import configure_logging, get_logger


def test_json_logs_are_valid_json(settings: Settings, capsys: pytest.CaptureFixture[str]) -> None:
    configure_logging(Settings(_env_file=None, log_format="json"))
    get_logger("test").info("hello.world", answer=42)

    line = capsys.readouterr().err.strip().splitlines()[-1]
    record = json.loads(line)
    assert record["event"] == "hello.world"
    assert record["answer"] == 42
    assert record["level"] == "info"
    assert "timestamp" in record


def test_stdlib_loggers_flow_through_structlog(
    settings: Settings, capsys: pytest.CaptureFixture[str]
) -> None:
    configure_logging(Settings(_env_file=None, log_format="json"))
    logging.getLogger("thirdparty.lib").warning("plain stdlib message")

    line = capsys.readouterr().err.strip().splitlines()[-1]
    record = json.loads(line)
    assert record["event"] == "plain stdlib message"
    assert record["logger"] == "thirdparty.lib"
    assert record["level"] == "warning"


def test_configure_is_idempotent(settings: Settings) -> None:
    configure_logging(settings)
    configure_logging(settings)
    assert len(logging.getLogger().handlers) == 1
