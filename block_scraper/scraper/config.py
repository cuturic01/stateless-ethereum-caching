"""Runtime configuration, loaded from environment / .env."""

from __future__ import annotations

from pathlib import Path

from pydantic_settings import BaseSettings, SettingsConfigDict

REPO_ROOT = Path(__file__).resolve().parents[2]
DEFAULT_DATA_DIR = REPO_ROOT / "data"


class Settings(BaseSettings):
    model_config = SettingsConfigDict(
        env_file=str(Path(__file__).resolve().parents[1] / ".env"),
        env_file_encoding="utf-8",
        extra="ignore",
    )

    alchemy_api_key: str = ""
    alchemy_network: str = "eth-mainnet"

    @property
    def rpc_url(self) -> str:
        if not self.alchemy_api_key:
            raise ValueError(
                "ALCHEMY_API_KEY is not set. Copy block_scraper/.env.example to "
                "block_scraper/.env and fill it in (needs a Pay-As-You-Go plan for debug_*)."
            )
        return f"https://{self.alchemy_network}.g.alchemy.com/v2/{self.alchemy_api_key}"


def load_settings() -> Settings:
    return Settings()
