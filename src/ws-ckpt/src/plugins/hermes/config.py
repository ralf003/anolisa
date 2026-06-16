"""Configuration for the ws-ckpt Hermes plugin."""

import os
from dataclasses import dataclass, field
from typing import Dict, List

# Message truncation length, hardcoded at 80 characters.
MSG_TRUNCATE_LEN = 80


@dataclass
class HermesPluginConfig:
    workspace: str  # Workspace directory path
    auto_checkpoint: bool  # Whether to auto-checkpoint on each turn
    cron_schedules: Dict[str, List[str]] = field(default_factory=dict)  # {workspace: [cron_expr]}


def _read_yaml_config() -> dict:
    """Read plugin config from ~/.hermes/config.yaml safely.

    Returns the 'plugins.ws-ckpt' section as a dict, or empty dict on failure.
    """
    try:
        from hermes_cli.config import cfg_get, load_config as hermes_load_config

        config = hermes_load_config()
        return cfg_get(config, "plugins", "ws-ckpt", default={}) or {}
    except Exception:
        # hermes_cli not available (e.g. standalone testing) or config missing
        return {}


def load_config() -> HermesPluginConfig:
    """Load plugin config. Priority: env vars > config.yaml > defaults.

    Config in ~/.hermes/config.yaml (camelCase keys, matching OpenClaw):
        plugins:
          ws-ckpt:
            autoCheckpoint: true
            workspace: /path/to/project

    Environment variable overrides:
        WS_CKPT_AUTO_CHECKPOINT=true
        WS_CKPT_WORKSPACE=/path/to/project
    """
    yaml_cfg = _read_yaml_config()

    # workspace: env > yaml > empty (no fallback — caller must handle absence)
    env_ws = os.environ.get("WS_CKPT_WORKSPACE", "").strip()
    yaml_ws = str(yaml_cfg.get("workspace", "")).strip() if yaml_cfg.get("workspace") else ""
    workspace = env_ws or yaml_ws

    # autoCheckpoint: env > yaml > False
    env_auto = os.environ.get("WS_CKPT_AUTO_CHECKPOINT", "").strip().lower()
    if env_auto:
        auto_checkpoint = env_auto in ("true", "1", "yes", "on")
    else:
        auto_checkpoint = bool(yaml_cfg.get("autoCheckpoint", False))

    # cronSchedules: yaml only (no env override — structure too complex for a flat var)
    from .cron import validate_cron_expr
    raw_cron = yaml_cfg.get("cronSchedules")
    cron_schedules: Dict[str, List[str]] = {}
    if isinstance(raw_cron, dict):
        for ws_key, exprs in raw_cron.items():
            if isinstance(exprs, list):
                valid = [str(e) for e in exprs if e and validate_cron_expr(str(e))]
                skipped = [str(e) for e in exprs if e and not validate_cron_expr(str(e))]
                if skipped:
                    print(
                        f"[ws-ckpt] Ignoring invalid cron expression(s) for {ws_key}: {skipped}",
                        flush=True,
                    )
                if valid:
                    cron_schedules[str(ws_key)] = valid

    return HermesPluginConfig(
        workspace=workspace,
        auto_checkpoint=auto_checkpoint,
        cron_schedules=cron_schedules,
    )
