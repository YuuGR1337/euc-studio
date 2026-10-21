"""Native EUC simulation with local, explicit own or custom firmware paths."""

from __future__ import annotations

import json
from os import PathLike

from . import _native
from ._native import Scenario, __version__, load_scenario
from .models import (
    BleFrame,
    BodyInfo,
    Channel,
    CheckResult,
    Event,
    Geom,
    Outcome,
    Outputs,
    OutputsAt,
    PhaseResult,
    Pose,
    Recording,
    RunResult,
    Signals,
    TelemetrySample,
    _result_from_json,
    _recording_from_json,
    _sample_from_dict,
)

__all__ = [
    "BleFrame",
    "BodyInfo",
    "Channel",
    "CheckResult",
    "Event",
    "Geom",
    "Outcome",
    "Outputs",
    "OutputsAt",
    "PhaseResult",
    "Pose",
    "Recording",
    "RunResult",
    "Scenario",
    "Session",
    "Signals",
    "TelemetrySample",
    "__version__",
    "channels",
    "load_scenario",
    "load_recording",
    "run_scenario",
]


def channels() -> tuple[Channel, ...]:
    """Return channel names, units, and stable identifiers from Rust."""
    return tuple(Channel(**item) for item in json.loads(_native.channels_json()))


def run_scenario(
    scenario: Scenario | str | PathLike[str],
    firmware_path: str | PathLike[str],
    *,
    profiles_dir: str | PathLike[str] | None = None,
    symbols_path: str | PathLike[str] | None = None,
    sample_hz: float = 100.0,
    max_samples: int = 100_000,
    max_seconds: float = 600.0,
    max_wall_seconds: float = 300.0,
) -> RunResult:
    """Run a parsed scenario or TOML path in this process with the own CPU.

    Pass an ELF image, or a raw image with an optional matching ELF symbols_path.
    profiles_dir explicitly replaces the built-in profile set.
    The native call releases the GIL during simulation.
    File errors raise OSError subclasses. Invalid inputs raise ValueError.
    Elapsed time limits raise TimeoutError. Firmware halts return a failed result.
    """
    parsed = scenario if isinstance(scenario, Scenario) else load_scenario(scenario)
    return _result_from_json(
        _native.run_scenario_json(
            parsed,
            firmware_path,
            profiles_dir=profiles_dir,
            symbols_path=symbols_path,
            sample_hz=sample_hz,
            max_samples=max_samples,
            max_seconds=max_seconds,
            max_wall_seconds=max_wall_seconds,
        )
    )


def load_recording(path: str | PathLike[str]) -> Recording:
    """Read a desktop, CLI, or Python .eucrec capture through the shared Rust reader.

    The file limit is 256 MiB. No firmware or simulator is needed.
    Invalid metadata raises ValueError. Inspect truncated for recovered partial files.
    """
    return _recording_from_json(_native.load_recording_json(path))


class Session:
    """Step a local scenario on the creating thread, then save a .eucrec capture.

    Construction, reset, and stepping retain the GIL. Use run_scenario for a
    detached full run. The elapsed-time limit includes time between step calls.
    """

    def __init__(
        self,
        scenario: Scenario | str | PathLike[str],
        firmware_path: str | PathLike[str],
        *,
        profiles_dir: str | PathLike[str] | None = None,
        symbols_path: str | PathLike[str] | None = None,
        sample_hz: float = 100.0,
        max_samples: int = 100_000,
        max_seconds: float = 600.0,
        max_wall_seconds: float = 300.0,
    ) -> None:
        parsed = scenario if isinstance(scenario, Scenario) else load_scenario(scenario)
        self._session = _native.Session(
            parsed, firmware_path, profiles_dir=profiles_dir, symbols_path=symbols_path,
            sample_hz=sample_hz, max_samples=max_samples, max_seconds=max_seconds,
            max_wall_seconds=max_wall_seconds,
        )

    @property
    def time_s(self) -> float:
        return self._session.time_s

    @property
    def sample_hz(self) -> float:
        return self._session.sample_hz

    @property
    def sample_count(self) -> int:
        return self._session.sample_count

    @property
    def finished(self) -> bool:
        return self._session.finished

    @property
    def halted(self) -> str | None:
        return self._session.halted

    @property
    def samples(self) -> tuple[TelemetrySample, ...]:
        return tuple(_sample_from_dict(item) for item in json.loads(self._session.samples_json()))

    @property
    def events(self) -> tuple[Event, ...]:
        return tuple(Event(**item) for item in json.loads(self._session.events_json()))

    def step(self, n: int = 1) -> TelemetrySample:
        """Advance up to n samples, stopping at the end. Return the last sample."""
        return _sample_from_dict(json.loads(self._session.step_json(n)))

    def reset(self) -> None:
        """Rebuild from the same paths and clear all captured data."""
        self._session.reset()

    def result(self) -> RunResult:
        """Return the run so far with the same typed data as run_scenario."""
        return _result_from_json(self._session.result_json())

    def set_manual(self, lean: float, steer: float) -> None:
        """Set finite rider commands, clamped to -1..1. Forward and left are positive."""
        self._session.set_manual(lean, steer)

    def release_manual(self) -> None:
        """Return rider control to the scenario."""
        self._session.release_manual()

    def set_push(self, newtons: float) -> None:
        """Set a heading-relative push. A later scenario push can replace it."""
        self._session.set_push(newtons)

    def set_held(self, held: bool) -> None:
        """Set the stand state. The scenario releases it after release_s."""
        self._session.set_held(held)

    def step_off(self) -> None:
        """Release the rider from the pedals."""
        self._session.step_off()

    def save(self, path: str | PathLike[str]) -> None:
        """Save captured data to a new file. Existing files raise FileExistsError."""
        self._session.save(path)
