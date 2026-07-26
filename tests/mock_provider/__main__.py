"""Launcher: ``python -m tests.mock_provider [--host H] [--port P]``.

Starts the mock provider for manual/end-to-end runs against the full Gantry
stack. Point Gantry at the printed base URL (see README).
"""

from __future__ import annotations

import argparse

import uvicorn

from .app import create_app


def main() -> None:
    parser = argparse.ArgumentParser(description="Gantry local mock LLM provider")
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=8000)
    args = parser.parse_args()

    base_url = f"http://{args.host}:{args.port}/v1"
    print(f"Gantry mock provider listening on {base_url}")
    print("Wire it up either way:")
    print(f"  - a LOCAL provider with base_url={base_url} (models: mock/happy-path, ...)")
    print(f"  - or export OPENAI_API_BASE={base_url} and use model openai/mock/happy-path")
    print("Scenarios: mock/happy-path, mock/repair-loop, mock/budget-runaway")
    uvicorn.run(create_app(), host=args.host, port=args.port, log_level="warning")


if __name__ == "__main__":
    main()
