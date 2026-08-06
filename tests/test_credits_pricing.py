"""The margin engine: raw provider cost -> credits, and the price catalog.

These are the numbers the business runs on, so the tests assert exact Decimals
rather than approximate floats — a rounding change here is a revenue change and
should have to be written down deliberately.
"""

from __future__ import annotations

from decimal import Decimal

import pytest

from gantry.billing.catalog import (
    CATALOG,
    FALLBACK,
    TokenPrice,
    parse_openrouter_models,
    price_for_model,
)
from gantry.billing.credits import calculate_credits_for_call, gross_margin, raw_cost_usd
from gantry.config import Settings


@pytest.fixture(autouse=True)
def _clean_catalog() -> None:
    """Every test starts from the static table, never a snapshot a sibling left."""
    CATALOG.clear()


# --- the margin identity -------------------------------------------------


def test_a_call_is_billed_at_exactly_the_target_margin() -> None:
    """The whole point of the engine: revenue - cost == 40% of revenue."""
    charge = calculate_credits_for_call("anthropic/claude-sonnet-5", 1_000_000, 1_000_000)
    # $3/1M in + $15/1M out at list.
    assert charge.raw_cost_usd == Decimal("18.00000000")
    # 18 / 0.60 = $30 charged, x100 GC/USD.
    assert charge.credits_deducted == Decimal("3000.000000")

    revenue_usd = charge.credits_deducted / Decimal(100)
    realised_margin = (revenue_usd - charge.raw_cost_usd) / revenue_usd
    assert realised_margin == Decimal("0.4")


def test_gross_margin_names_the_ratio_it_inverts() -> None:
    # The knob is a COST ratio; the number an operator cares about is its inverse.
    assert gross_margin(0.60) == pytest.approx(0.40)
    assert gross_margin(0.50) == pytest.approx(0.50)


def test_a_tighter_cost_ratio_widens_the_margin() -> None:
    settings = Settings(_env_file=None, credit_cost_ratio=0.5)
    charge = calculate_credits_for_call(
        "anthropic/claude-sonnet-5", 1_000_000, 0, settings=settings
    )
    # $3 raw / 0.5 = $6 charged = 600 GC, i.e. a 50% margin.
    assert charge.credits_deducted == Decimal("600.000000")


def test_a_nonsense_cost_ratio_cannot_divide_by_zero_or_bill_below_cost() -> None:
    """A misconfigured ratio is a config bug, not a state to propagate: 0 would
    charge infinity, and >1 would guarantee a loss on every single call."""
    zero = calculate_credits_for_call(
        "anthropic/claude-sonnet-5",
        1_000_000,
        0,
        settings=Settings(_env_file=None, credit_cost_ratio=0.0),
    )
    assert zero.credits_deducted == Decimal("500.000000")  # fell back to 0.6

    over = calculate_credits_for_call(
        "anthropic/claude-sonnet-5",
        1_000_000,
        0,
        settings=Settings(_env_file=None, credit_cost_ratio=5.0),
    )
    # Clamped to 1.0 => billed at cost, never below it.
    assert over.credits_deducted == Decimal("300.000000")


def test_credits_per_usd_scales_the_charge() -> None:
    settings = Settings(_env_file=None, credits_per_usd=1000.0)
    charge = calculate_credits_for_call(
        "anthropic/claude-sonnet-5", 1_000_000, 0, settings=settings
    )
    assert charge.credits_deducted == Decimal("5000.000000")


# --- cache-aware costing -------------------------------------------------


def test_cache_reads_are_billed_at_the_cached_rate_not_as_fresh_input() -> None:
    """A cache hit is a discount we RECEIVED; billing it as fresh input would
    charge the user for a saving they should be getting."""
    warm = calculate_credits_for_call(
        "anthropic/claude-sonnet-5",
        1_000_000,
        0,
        cache_read_tokens=1_000_000,
    )
    cold = calculate_credits_for_call("anthropic/claude-sonnet-5", 1_000_000, 0)
    assert warm.raw_cost_usd == Decimal("0.30000000")  # the $0.30/1M cached rate
    assert cold.raw_cost_usd == Decimal("3.00000000")
    assert warm.credits_deducted < cold.credits_deducted


def test_cache_tokens_are_not_double_counted_in_the_total() -> None:
    # Providers already count cache reads inside prompt_tokens.
    charge = calculate_credits_for_call(
        "anthropic/claude-sonnet-5", 1000, 500, cache_read_tokens=800
    )
    assert charge.total_tokens == 1500


def test_negative_token_counts_cannot_produce_a_credit() -> None:
    """A garbled provider usage block must never hand a user free credits."""
    charge = calculate_credits_for_call("anthropic/claude-sonnet-5", -5000, -5000)
    assert charge.raw_cost_usd == Decimal(0)
    assert charge.credits_deducted == Decimal(0)


def test_a_cache_read_larger_than_the_prompt_does_not_go_negative() -> None:
    charge = calculate_credits_for_call(
        "anthropic/claude-sonnet-5", 100, 0, cache_read_tokens=100_000
    )
    assert charge.raw_cost_usd >= 0


# --- the price catalog ---------------------------------------------------


def test_an_unknown_slug_falls_back_conservatively_rather_than_free() -> None:
    """Pricing a model we've never seen at zero would be silent free inference."""
    assert price_for_model("some-vendor/never-heard-of-it") == FALLBACK
    charge = calculate_credits_for_call("some-vendor/never-heard-of-it", 1_000_000, 0)
    assert charge.credits_deducted > 0


def test_a_live_snapshot_overrides_the_static_table() -> None:
    CATALOG.load({"anthropic/claude-sonnet-5": TokenPrice(Decimal(9), Decimal(9), Decimal(9))})
    assert price_for_model("anthropic/claude-sonnet-5").input_per_mtok == Decimal(9)


def test_a_route_prefixed_slug_matches_the_catalogs_bare_key() -> None:
    """We address models as ``openrouter/qwen/qwen3-coder``; OpenRouter's own
    catalog keys them ``qwen/qwen3-coder``. A miss here would price a whole
    provider's traffic at the fallback rate."""
    CATALOG.load({"qwen/qwen3-coder": TokenPrice(Decimal(7), Decimal(8), Decimal(1))})
    assert price_for_model("openrouter/qwen/qwen3-coder").input_per_mtok == Decimal(7)


def test_lookup_is_case_insensitive_and_tolerates_blanks() -> None:
    assert price_for_model("ANTHROPIC/Claude-Sonnet-5").input_per_mtok == Decimal("3.0")
    assert price_for_model("") == FALLBACK


# --- parsing OpenRouter's list ------------------------------------------


def test_openrouter_per_token_prices_become_per_mtok() -> None:
    prices = parse_openrouter_models(
        {
            "data": [
                {
                    "id": "deepseek/deepseek-r1",
                    "pricing": {
                        "prompt": "0.000001",
                        "completion": "0.000003",
                        "input_cache_read": "0.0000001",
                    },
                }
            ]
        }
    )
    price = prices["deepseek/deepseek-r1"]
    assert price.input_per_mtok == Decimal(1)
    assert price.output_per_mtok == Decimal(3)
    assert price.cache_read_per_mtok == Decimal("0.1")


def test_a_model_without_a_published_cache_rate_assumes_full_input() -> None:
    """Assuming a discount we may not receive would quietly eat the margin."""
    prices = parse_openrouter_models(
        {"data": [{"id": "x/y", "pricing": {"prompt": "0.000002", "completion": "0.000004"}}]}
    )
    assert prices["x/y"].cache_read_per_mtok == Decimal(2)


def test_unusable_rows_are_skipped_so_they_fall_through_to_the_static_table() -> None:
    """A malformed row must not override a known price with garbage — a model
    that parsed as free would be billed at zero margin forever."""
    prices = parse_openrouter_models(
        {
            "data": [
                {"id": "no-pricing"},
                {"id": "bad-numbers", "pricing": {"prompt": "abc", "completion": "1"}},
                {"id": "missing-completion", "pricing": {"prompt": "0.000001"}},
                {"pricing": {"prompt": "0.000001", "completion": "0.000001"}},
                "not-a-dict",
                {"id": "good", "pricing": {"prompt": "0.000001", "completion": "0.000002"}},
            ]
        }
    )
    assert set(prices) == {"good"}


def test_a_junk_payload_yields_nothing_rather_than_raising() -> None:
    assert parse_openrouter_models({"data": "nope"}) == {}
    assert parse_openrouter_models([]) == {}
    assert parse_openrouter_models(None) == {}


def test_raw_cost_is_exact_decimal_arithmetic() -> None:
    """Money is never a float: 0.1 + 0.2 problems in a ledger are unauditable."""
    price = TokenPrice(Decimal("0.1"), Decimal("0.2"), Decimal("0.01"))
    cost = raw_cost_usd(price, prompt_tokens=3_000_000, completion_tokens=0)
    assert cost == Decimal("0.30000000")
    assert isinstance(cost, Decimal)
