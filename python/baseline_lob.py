#!/usr/bin/env python3
"""Naive Python limit-order-book baseline for comparison with the Rust engine.

Uses standard dict/list structures. Integer prices and quantities only.
Deterministic synthetic workload (fixed pattern, no RNG or wall-clock in logic).
"""

from __future__ import annotations

import argparse
import json
import time
from dataclasses import dataclass
from typing import Literal

Side = Literal["buy", "sell"]


@dataclass
class RestingOrder:
    order_id: int
    side: Side
    price: int
    qty: int
    timestamp: int


@dataclass
class Trade:
    taker_order_id: int
    maker_order_id: int
    price: int
    qty: int
    aggressor_side: Side


class NaiveLimitOrderBook:
    """Price-time priority LOB using dicts and lists (naive baseline)."""

    def __init__(self) -> None:
        # price -> list of resting orders (time priority within level)
        self._bids: dict[int, list[RestingOrder]] = {}
        self._asks: dict[int, list[RestingOrder]] = {}
        self._index: dict[int, tuple[Side, int]] = {}
        self.trades: list[Trade] = []

    def _best_bid_price(self) -> int | None:
        return max(self._bids) if self._bids else None

    def _best_ask_price(self) -> int | None:
        return min(self._asks) if self._asks else None

    def _best_opposite_order(self, side: Side) -> RestingOrder | None:
        if side == "buy":
            price = self._best_ask_price()
            levels = self._asks
        else:
            price = self._best_bid_price()
            levels = self._bids
        if price is None:
            return None
        queue = levels.get(price)
        return queue[0] if queue else None

    def _can_cross(self, side: Side, price: int) -> bool:
        if side == "buy":
            best_ask = self._best_ask_price()
            return best_ask is not None and price >= best_ask
        best_bid = self._best_bid_price()
        return best_bid is not None and price <= best_bid

    def _remove_level_if_empty(self, side: Side, price: int) -> None:
        levels = self._bids if side == "buy" else self._asks
        if price in levels and not levels[price]:
            del levels[price]

    def _execute_at_best(
        self, incoming_side: Side, incoming_qty: int, taker_order_id: int
    ) -> int:
        maker = self._best_opposite_order(incoming_side)
        if maker is None:
            return incoming_qty

        fill_qty = min(incoming_qty, maker.qty)
        maker_side: Side = "sell" if incoming_side == "buy" else "buy"
        levels = self._asks if maker_side == "sell" else self._bids
        queue = levels[maker.price]

        self.trades.append(
            Trade(
                taker_order_id=taker_order_id,
                maker_order_id=maker.order_id,
                price=maker.price,
                qty=fill_qty,
                aggressor_side=incoming_side,
            )
        )

        if fill_qty == maker.qty:
            queue.pop(0)
            del self._index[maker.order_id]
            self._remove_level_if_empty(maker_side, maker.price)
        else:
            maker.qty -= fill_qty

        return incoming_qty - fill_qty

    def add_limit(
        self, order_id: int, side: Side, price: int, qty: int, timestamp: int
    ) -> None:
        if qty <= 0 or price <= 0 or order_id in self._index:
            return

        remaining = qty
        while remaining > 0 and self._can_cross(side, price):
            remaining = self._execute_at_best(side, remaining, order_id)

        if remaining <= 0:
            return

        order = RestingOrder(order_id, side, price, remaining, timestamp)
        levels = self._bids if side == "buy" else self._asks
        levels.setdefault(price, []).append(order)
        self._index[order_id] = (side, price)

    def cancel(self, order_id: int) -> bool:
        location = self._index.pop(order_id, None)
        if location is None:
            return False
        side, price = location
        levels = self._bids if side == "buy" else self._asks
        queue = levels.get(price)
        if queue is None:
            return False
        for idx, order in enumerate(queue):
            if order.order_id == order_id:
                queue.pop(idx)
                self._remove_level_if_empty(side, price)
                return True
        return False


def deterministic_event(event_idx: int) -> tuple[str, dict]:
    """Fully deterministic event pattern (no RNG)."""
    phase = event_idx % 11
    order_id = event_idx + 1
    timestamp = event_idx

    if phase in (0, 1, 2):
        side: Side = "buy" if phase != 2 else "sell"
        price = 100 + (event_idx % 7)
        qty = 1 + (event_idx % 5)
        return (
            "add",
            {
                "order_id": order_id,
                "side": side,
                "price": price,
                "qty": qty,
                "timestamp": timestamp,
            },
        )

    if phase in (3, 4):
        side = "sell" if phase == 3 else "buy"
        # Aggressive prices that cross resting liquidity from earlier phases.
        price = 95 if side == "sell" else 105
        qty = 2 + (event_idx % 4)
        return (
            "add",
            {
                "order_id": order_id,
                "side": side,
                "price": price,
                "qty": qty,
                "timestamp": timestamp,
            },
        )

    if phase == 5:
        cancel_id = max(1, order_id - 3)
        return ("cancel", {"order_id": cancel_id})

    if phase in (6, 7):
        side = "buy" if phase == 6 else "sell"
        price = 100
        qty = 10
        return (
            "add",
            {
                "order_id": order_id,
                "side": side,
                "price": price,
                "qty": qty,
                "timestamp": timestamp,
            },
        )

    if phase == 8:
        side = "sell" if (event_idx // 11) % 2 == 0 else "buy"
        price = 100
        qty = 3
        return (
            "add",
            {
                "order_id": order_id,
                "side": side,
                "price": price,
                "qty": qty,
                "timestamp": timestamp,
            },
        )

    if phase == 9:
        cancel_id = max(1, order_id - 7)
        return ("cancel", {"order_id": cancel_id})

    side = "buy" if event_idx % 2 == 0 else "sell"
    price = 98 + (event_idx % 5)
    qty = 1
    return (
        "add",
        {
            "order_id": order_id,
            "side": side,
            "price": price,
            "qty": qty,
            "timestamp": timestamp,
        },
    )


def run_workload(event_count: int) -> tuple[NaiveLimitOrderBook, int]:
    book = NaiveLimitOrderBook()
    for idx in range(event_count):
        kind, payload = deterministic_event(idx)
        if kind == "add":
            book.add_limit(
                payload["order_id"],
                payload["side"],
                payload["price"],
                payload["qty"],
                payload["timestamp"],
            )
        else:
            book.cancel(payload["order_id"])
    return book, event_count


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Naive Python limit-order-book baseline (dict/list LOB)."
    )
    parser.add_argument(
        "--events",
        type=int,
        default=10_000,
        help="Number of synthetic events to process (default: 10000).",
    )
    args = parser.parse_args()

    if args.events <= 0:
        raise SystemExit("--events must be positive")

    start = time.perf_counter()
    book, events = run_workload(args.events)
    elapsed = time.perf_counter() - start

    trades = len(book.trades)
    events_per_second = events / elapsed if elapsed > 0.0 else 0.0

    summary = {
        "label": "naive baseline",
        "implementation": "python/baseline_lob.py",
        "events": events,
        "trades": trades,
        "elapsed_seconds": round(elapsed, 6),
        "events_per_second": round(events_per_second, 2),
    }
    print(json.dumps(summary, indent=2))


if __name__ == "__main__":
    main()
