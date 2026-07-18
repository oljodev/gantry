from __future__ import annotations

import pytest

from gantry.config import Settings


@pytest.fixture
def settings() -> Settings:
    # _env_file=None keeps a developer's local .env from leaking into tests.
    return Settings(_env_file=None)
