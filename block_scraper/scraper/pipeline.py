"""Producer/consumer orchestration of the scrape.

Worker tasks pull block numbers from a queue, fetch read-set + write-set +
timestamp for each, parse to a ``BlockRecord``, and hand it to a single writer
coroutine that owns the ShardStore (so all file writes are serialized). Progress
is shown with rich; the scrape is fully resumable via the checkpoint.
"""

from __future__ import annotations

import asyncio
import logging

from rich.logging import RichHandler
from rich.progress import (
    BarColumn,
    MofNCompleteColumn,
    Progress,
    TextColumn,
    TimeElapsedColumn,
    TimeRemainingColumn,
)

from .models import BlockRecord
from .rpc import AlchemyRPC
from .tracer import parse_read_set, parse_write_set
from .writer import ShardStore

log = logging.getLogger("scraper")

# Sentinel pushed once per worker to signal the writer to drain and stop.
_DONE = object()


def configure_logging(level: int = logging.INFO) -> None:
    logging.basicConfig(
        level=level,
        format="%(message)s",
        datefmt="[%X]",
        handlers=[RichHandler(rich_tracebacks=True, show_path=False)],
    )


async def _fetch_block(rpc: AlchemyRPC, block_number: int) -> BlockRecord:
    read_trace, write_trace, ts = await asyncio.gather(
        rpc.trace_block(block_number, diff=False),
        rpc.trace_block(block_number, diff=True),
        rpc.get_block_timestamp(block_number),
    )
    return BlockRecord(
        block_number=block_number,
        timestamp=ts,
        read_set=parse_read_set(read_trace),
        write_set=parse_write_set(write_trace),
    )


async def run_scrape(
    rpc: AlchemyRPC,
    store: ShardStore,
    *,
    n_workers: int = 8,
    queue_size: int = 256,
) -> None:
    pending = store.pending_block_numbers()
    if not pending:
        log.info("Nothing to do: all blocks already scraped.")
        return

    m = store.manifest
    log.info(
        "Scraping %d blocks (range %d..%d, %d already done) with %d workers",
        len(pending),
        m.start_block,
        m.end_block,
        (m.end_block - m.start_block + 1) - len(pending),
        n_workers,
    )

    work: asyncio.Queue[int] = asyncio.Queue()
    results: asyncio.Queue = asyncio.Queue(maxsize=queue_size)
    for bn in pending:
        work.put_nowait(bn)

    progress = Progress(
        TextColumn("[bold blue]scrape"),
        BarColumn(),
        MofNCompleteColumn(),
        TextColumn("{task.fields[rate]}"),
        TimeElapsedColumn(),
        TimeRemainingColumn(),
    )
    async def worker() -> None:
        while True:
            try:
                bn = work.get_nowait()
            except asyncio.QueueEmpty:
                break
            try:
                record = await _fetch_block(rpc, bn)
                await results.put(record)
            except Exception:
                log.exception("Failed to fetch block %d; re-queueing", bn)
                # Re-queue so it is retried after others; avoids losing the block.
                await work.put(bn)
                await asyncio.sleep(1.0)
            finally:
                work.task_done()
        await results.put(_DONE)

    async def writer(task_id) -> None:
        done_workers = 0
        completed = 0
        while done_workers < n_workers:
            item = await results.get()
            if item is _DONE:
                done_workers += 1
                continue
            store.add(item)
            completed += 1
            est_usd = rpc.cu_spent / 1_000_000 * 0.45
            progress.update(
                task_id,
                advance=1,
                rate=(
                    f"~${est_usd:.2f} | {rpc.cu_spent/1e6:.2f}M CU | "
                    f"{rpc.retry_count} retries"
                ),
            )

    with progress:
        task_id = progress.add_task("scrape", total=len(pending), rate="")
        workers = [asyncio.create_task(worker()) for _ in range(n_workers)]
        await writer(task_id)
        await asyncio.gather(*workers)

    flushed = store.flush_complete()
    if flushed:
        log.info("Flushed %d trailing shard(s) at shutdown", len(flushed))
    incomplete = store.incomplete_shards()
    if incomplete:
        log.warning(
            "%d shard(s) still incomplete (will be re-fetched on resume): %s",
            len(incomplete),
            incomplete,
        )
    log.info(
        "Done. ~%.2fM CU (~$%.2f), %d retries.",
        rpc.cu_spent / 1e6,
        rpc.cu_spent / 1_000_000 * 0.45,
        rpc.retry_count,
    )


async def resolve_range(
    rpc: AlchemyRPC,
    *,
    n_blocks: int | None,
    start: int | None,
    end: int | None,
    safety_margin: int,
) -> tuple[int, int]:
    if start is not None and end is not None:
        return start, end
    latest = await rpc.get_latest_block_number()
    resolved_end = end if end is not None else latest - safety_margin
    if start is not None:
        return start, resolved_end
    assert n_blocks is not None
    return resolved_end - n_blocks + 1, resolved_end
