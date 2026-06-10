from __future__ import annotations

import asyncio

import aiohttp
import orjson
from tenacity import (
    AsyncRetrying,
    retry_if_exception_type,
    stop_after_attempt,
    wait_exponential_jitter,
)

# CU cost per method (for spend estimation/logging only).
CU_TRACE_BLOCK = 40
CU_GET_BLOCK = 20

# Retried on these. RpcError (a JSON-RPC error body) is *not* retried by default.
_RETRYABLE = (
    aiohttp.ClientError,
    asyncio.TimeoutError,
    ConnectionError,
)


class RpcError(RuntimeError):
    """A JSON-RPC level error (the response carried an ``error`` object)."""

    def __init__(self, code: int, message: str):
        super().__init__(f"RPC error {code}: {message}")
        self.code = code
        self.message = message


class RetryableHttpError(Exception):
    """HTTP status that should be retried (429, 5xx)."""


def _hex(n: int) -> str:
    return hex(n)


class AlchemyRPC:
    def __init__(
        self,
        url: str,
        *,
        max_concurrency: int = 8,
        request_timeout: float = 60.0,
        max_attempts: int = 6,
    ):
        self._url = url
        self._sem = asyncio.Semaphore(max_concurrency)
        self._timeout = aiohttp.ClientTimeout(total=request_timeout)
        self._max_attempts = max_attempts
        self._session: aiohttp.ClientSession | None = None
        # Running totals for progress reporting.
        self.cu_spent = 0
        self.retry_count = 0

    async def __aenter__(self) -> AlchemyRPC:
        self._session = aiohttp.ClientSession(
            timeout=self._timeout,
            json_serialize=lambda o: orjson.dumps(o).decode(),
        )
        return self

    async def __aexit__(self, *exc) -> None:
        if self._session is not None:
            await self._session.close()
            self._session = None

    async def _post(self, payload: dict) -> dict:
        assert self._session is not None, "use AlchemyRPC as an async context manager"

        async def attempt() -> dict:
            async with self._sem:
                async with self._session.post(self._url, json=payload) as resp:
                    if resp.status == 429 or resp.status >= 500:
                        retry_after = resp.headers.get("Retry-After")
                        if retry_after:
                            try:
                                await asyncio.sleep(float(retry_after))
                            except ValueError:
                                pass
                        raise RetryableHttpError(f"HTTP {resp.status}")
                    raw = await resp.read()
            body = orjson.loads(raw)
            if isinstance(body, dict) and body.get("error"):
                err = body["error"]
                raise RpcError(err.get("code", 0), err.get("message", "unknown"))
            return body

        retrying = AsyncRetrying(
            stop=stop_after_attempt(self._max_attempts),
            wait=wait_exponential_jitter(initial=1, max=30),
            retry=retry_if_exception_type((*_RETRYABLE, RetryableHttpError)),
            reraise=True,
        )
        async for twith in retrying:
            with twith:
                if retrying.statistics.get("attempt_number", 1) > 1:
                    self.retry_count += 1
                return await attempt()
        raise RuntimeError("unreachable")  # pragma: no cover

    async def trace_block(self, block_number: int, *, diff: bool) -> list:
        """``debug_traceBlockByNumber`` with prestateTracer. Returns the tx list."""
        payload = {
            "jsonrpc": "2.0",
            "id": 1,
            "method": "debug_traceBlockByNumber",
            "params": [
                _hex(block_number),
                {"tracer": "prestateTracer", "tracerConfig": {"diffMode": diff}},
            ],
        }
        body = await self._post(payload)
        self.cu_spent += CU_TRACE_BLOCK
        return body["result"]

    async def get_block_timestamp(self, block_number: int) -> int:
        payload = {
            "jsonrpc": "2.0",
            "id": 1,
            "method": "eth_getBlockByNumber",
            "params": [_hex(block_number), False],
        }
        body = await self._post(payload)
        self.cu_spent += CU_GET_BLOCK
        return int(body["result"]["timestamp"], 16)

    async def get_latest_block_number(self) -> int:
        payload = {"jsonrpc": "2.0", "id": 1, "method": "eth_blockNumber", "params": []}
        body = await self._post(payload)
        return int(body["result"], 16)
