from __future__ import annotations

import argparse
import asyncio
import hashlib
import logging
import sys
from pathlib import Path

from .checkpoint import Manifest
from .config import DEFAULT_DATA_DIR, load_settings
from .pipeline import configure_logging, resolve_range, run_scrape
from .rpc import AlchemyRPC
from .writer import ShardStore

log = logging.getLogger("scraper")

# ~1 week of mainnet at 12s/block. Default target for the thesis dataset.
DEFAULT_BLOCKS = 50_400
DEFAULT_SHARD_SIZE = 500
DEFAULT_SAFETY_MARGIN = 128  # blocks below tip, for reorg finality


async def _scrape(args: argparse.Namespace, *, resume: bool) -> int:
    settings = load_settings()
    data_dir = Path(args.data_dir)
    existing = Manifest.load(data_dir)

    async with AlchemyRPC(
        settings.rpc_url,
        max_concurrency=args.concurrency,
    ) as rpc:
        if resume:
            if existing is None:
                log.error("No manifest at %s; nothing to resume. Run `scrape` first.", data_dir)
                return 1
            manifest = existing
            log.info("Resuming run: blocks %d..%d", manifest.start_block, manifest.end_block)
        else:
            if existing is not None:
                log.error(
                    "A run already exists at %s (blocks %d..%d). Use `resume`, or pick a "
                    "fresh --data-dir.",
                    data_dir,
                    existing.start_block,
                    existing.end_block,
                )
                return 1
            start, end = await resolve_range(
                rpc,
                n_blocks=args.blocks,
                start=args.start,
                end=args.end,
                safety_margin=args.safety_margin,
            )
            manifest = Manifest(
                network=settings.alchemy_network,
                start_block=start,
                end_block=end,
                shard_size=args.shard_size,
            )
            manifest.save(data_dir)
            log.info("New run: blocks %d..%d (%d blocks)", start, end, end - start + 1)

        store = ShardStore(data_dir, manifest)
        await run_scrape(rpc, store, n_workers=args.concurrency)
    return 0


def _verify(args: argparse.Namespace) -> int:
    data_dir = Path(args.data_dir)
    manifest = Manifest.load(data_dir)
    if manifest is None:
        log.error("No manifest at %s", data_dir)
        return 1

    ok = True
    covered: list[tuple[int, int]] = []
    for s in manifest.shards:
        path = data_dir / s.file
        if not path.exists():
            log.error("MISSING shard file: %s", s.file)
            ok = False
            continue
        digest = hashlib.sha256(path.read_bytes()).hexdigest()
        if digest != s.sha256:
            log.error("SHA256 MISMATCH: %s", s.file)
            ok = False
        if s.last_block - s.first_block + 1 != s.count:
            log.error("COUNT MISMATCH: %s (%d..%d, count=%d)", s.file, s.first_block,
                      s.last_block, s.count)
            ok = False
        covered.append((s.first_block, s.last_block))

    covered.sort()
    expected = manifest.start_block
    for first, last in covered:
        if first != expected:
            log.error("GAP before block %d (expected %d)", first, expected)
            ok = False
        expected = last + 1
    if covered and expected - 1 != manifest.end_block:
        log.warning(
            "Coverage ends at %d but manifest end_block is %d (run not finished)",
            expected - 1,
            manifest.end_block,
        )
        ok = False
    if not covered:
        log.error("No shards recorded yet.")
        ok = False

    if ok:
        log.info(
            "OK: %d shards, blocks %d..%d, all hashes match, no gaps.",
            len(manifest.shards),
            manifest.start_block,
            manifest.end_block,
        )
        return 0
    return 2


def build_parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(
        prog="scraper", description="Ethereum block read/write trace scraper"
    )
    p.add_argument("--data-dir", default=str(DEFAULT_DATA_DIR), help="Output data directory")
    sub = p.add_subparsers(dest="cmd", required=True)

    sc = sub.add_parser("scrape", help="Start a new scrape run")
    sc.add_argument("--blocks", type=int, default=DEFAULT_BLOCKS,
                    help="Number of most-recent blocks (ignored if --start and --end given)")
    sc.add_argument("--start", type=int, default=None, help="Pin start block (reproducible)")
    sc.add_argument("--end", type=int, default=None, help="Pin end block (reproducible)")
    sc.add_argument("--shard-size", type=int, default=DEFAULT_SHARD_SIZE)
    sc.add_argument("--concurrency", type=int, default=8)
    sc.add_argument("--safety-margin", type=int, default=DEFAULT_SAFETY_MARGIN,
                    help="Blocks below chain tip to stop at, for reorg finality")

    rs = sub.add_parser("resume", help="Resume an interrupted run")
    rs.add_argument("--concurrency", type=int, default=8)

    sub.add_parser("verify", help="Check shard hashes and block-range coverage")
    return p


def main(argv: list[str] | None = None) -> int:
    configure_logging()
    args = build_parser().parse_args(argv)
    if args.cmd == "scrape":
        return asyncio.run(_scrape(args, resume=False))
    if args.cmd == "resume":
        return asyncio.run(_scrape(args, resume=True))
    if args.cmd == "verify":
        return _verify(args)
    return 1


if __name__ == "__main__":
    sys.exit(main())
