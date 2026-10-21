"""Immutable simulator results. Units match the Rust telemetry schema."""

from __future__ import annotations

import json
from dataclasses import asdict, dataclass, field
from typing import Any, Literal


class Serializable:
    """Export values without firmware bytes or input paths."""

    __slots__ = ()

    def to_dict(self) -> dict[str, Any]:
        """Return a new dictionary with nested data classes expanded."""
        return asdict(self)  # type: ignore[arg-type]

    def to_json(self, *, indent: int | None = None) -> str:
        """Return strict JSON. Unknown signals use null."""
        return json.dumps(self.to_dict(), allow_nan=False, indent=indent)


@dataclass(frozen=True, slots=True)
class Signals(Serializable):
    """Scalar channels. None means an unavailable or non-finite value."""

    speed_kmh: float | None
    wheel_rad_s: float | None
    pwm_pct: float | None
    phase_a_a: float | None
    phase_b_a: float | None
    phase_c_a: float | None
    iq_a: float | None
    torque_nm: float | None
    battery_v: float | None
    battery_a: float | None
    soc_pct: float | None
    pitch_deg: float | None
    roll_deg: float | None
    board_temp_c: float | None
    motor_temp_c: float | None
    distance_m: float | None
    isr_load_pct: float | None
    rider_lean_deg: float | None
    target_speed_kmh: float | None
    suspension_mm: float | None
    shock_mm: float | None
    modulation_pct: float | None


@dataclass(frozen=True, slots=True)
class Pose(Serializable):
    """Position in metres and quaternion [w, x, y, z]. World z points up."""

    pos: tuple[float, float, float]
    quat: tuple[float, float, float, float]


@dataclass(frozen=True, slots=True)
class TelemetrySample(Serializable):
    """Time, scalar signals, and poses in RunResult.bodies order."""

    t_s: float
    signals: Signals
    bodies: tuple[Pose, ...]


@dataclass(frozen=True, slots=True)
class Geom(Serializable):
    """A body shape from the existing physics model."""

    kind: Literal["capsule", "box", "sphere", "cylinder", "ellipsoid"]
    size: tuple[float, float, float]
    pos: tuple[float, float, float]
    quat: tuple[float, float, float, float]


@dataclass(frozen=True, slots=True)
class BodyInfo(Serializable):
    """A body name, role, and shape list."""

    name: str
    role: Literal["wheel", "shell", "rider", "other"]
    geoms: tuple[Geom, ...]


@dataclass(frozen=True, slots=True)
class Event(Serializable):
    """A simulator event at a simulated time."""

    t_s: float
    kind: Literal[
        "alarm", "cutoff", "tiltback", "command", "firmware", "scenario", "fault"
    ]
    text: str


@dataclass(frozen=True, slots=True)
class CheckResult(Serializable):
    """One existing scenario expectation and its measured result."""

    what: str
    passed: bool
    measured: str


@dataclass(frozen=True, slots=True)
class PhaseResult(Serializable):
    """Measured results from one bounded lean phase."""

    name: str
    status: Literal["not_started", "running", "reached", "target_unreachable"]
    reason: str | None
    start_s: float | None
    end_s: float | None
    elapsed_s: float | None
    distance_m: float | None
    start_speed_kmh: float | None
    end_speed_kmh: float | None
    max_speed_kmh: float | None
    max_abs_phase_current_a: float | None
    iq_min_a: float | None
    iq_max_a: float | None
    battery_current_min_a: float | None
    battery_current_max_a: float | None
    battery_voltage_min_v: float | None
    battery_voltage_max_v: float | None


@dataclass(frozen=True, slots=True)
class Outcome(Serializable):
    """Scenario checks and summary values from Ride.outcome."""

    checks: tuple[CheckResult, ...]
    top_speed_kmh: float
    max_abs_pitch_deg: float
    distance_m: float
    phases: tuple[PhaseResult, ...] = ()

    @property
    def passed(self) -> bool:
        """Return whether every expectation passed. RunResult also checks completion."""
        return all(check.passed for check in self.checks)


@dataclass(frozen=True, slots=True)
class Channel(Serializable):
    """A scalar channel from the Rust channel schema."""

    id: int
    channel: str
    key: str
    label: str
    unit: str
    rate: Literal["fast", "slow"]


@dataclass(frozen=True, slots=True)
class RunResult(Serializable):
    """A completed or halted run. passed requires completion and every check."""

    wheel: str
    scenario: str
    cpu: Literal["own"]
    firmware_sha256: str
    firmware_format: Literal["raw", "elf"]
    firmware_probes: bool
    requested_sample_hz: float
    sample_hz: float
    sim_seconds: float
    wall_seconds: float
    completed: bool
    passed: bool
    halted: str | None
    outcome: Outcome
    samples: tuple[TelemetrySample, ...] = field(repr=False)
    events: tuple[Event, ...]
    bodies: tuple[BodyInfo, ...] = field(repr=False)


@dataclass(frozen=True, slots=True)
class BleFrame(Serializable):
    """One decoded BLE segment, including the original byte values."""

    t_s: float
    kind: str
    summary: str
    fields: tuple[tuple[str, str], ...]
    bytes: tuple[int, ...]
    reply: bool


@dataclass(frozen=True, slots=True)
class Outputs(Serializable):
    """Board display, lights, buzzer, and controller supply state."""

    display_grid: tuple[int, ...]
    display_on: bool
    display_brightness: int
    leds: tuple[tuple[int, int, int], ...]
    buzzer_hz: int
    headlight_pin_high: bool | None = None
    headlight_duty: float | None = None
    headlight_source: str | None = None
    power_latched: bool | None = None
    controller_powered: bool | None = None


@dataclass(frozen=True, slots=True)
class OutputsAt(Serializable):
    """The board outputs from a given simulated time."""

    t_s: float
    outputs: Outputs


@dataclass(frozen=True, slots=True)
class Recording(Serializable):
    """A shared .eucrec capture. Metadata dictionaries retain unknown header fields."""

    header: dict[str, Any]
    context: dict[str, Any] | None
    start_s: float
    end_s: float
    samples: tuple[TelemetrySample, ...] = field(repr=False)
    events: tuple[Event, ...]
    frames: tuple[BleFrame, ...] = field(repr=False)
    outputs: tuple[OutputsAt, ...] = field(repr=False)
    truncated: str | None
    skipped: int


def _sample_from_dict(sample: dict[str, Any]) -> TelemetrySample:
    return TelemetrySample(
        t_s=sample["t_s"],
        signals=Signals(**sample["signals"]),
        bodies=tuple(
            Pose(tuple(pose["pos"]), tuple(pose["quat"]))
            for pose in sample["bodies"]
        ),
    )


def _result_from_json(text: str) -> RunResult:
    """Convert the private native transport into typed Python values."""
    data = json.loads(text)
    data["samples"] = tuple(_sample_from_dict(sample) for sample in data["samples"])
    data["events"] = tuple(Event(**event) for event in data["events"])
    data["bodies"] = tuple(
        BodyInfo(
            name=body["name"],
            role=body["role"],
            geoms=tuple(
                Geom(
                    geom["kind"],
                    tuple(geom["size"]),
                    tuple(geom["pos"]),
                    tuple(geom["quat"]),
                )
                for geom in body["geoms"]
            ),
        )
        for body in data["bodies"]
    )
    outcome = data["outcome"]
    outcome["checks"] = tuple(CheckResult(**check) for check in outcome["checks"])
    outcome["phases"] = tuple(PhaseResult(**phase) for phase in outcome.get("phases", ()))
    data["outcome"] = Outcome(**outcome)
    return RunResult(**data)


def _recording_from_json(text: str) -> Recording:
    data = json.loads(text)
    data["samples"] = tuple(_sample_from_dict(sample) for sample in data["samples"])
    data["events"] = tuple(Event(**event) for event in data["events"])
    data["frames"] = tuple(
        BleFrame(**{**frame, "fields": tuple(map(tuple, frame["fields"])),
                    "bytes": tuple(frame["bytes"])})
        for frame in data["frames"]
    )
    data["outputs"] = tuple(
        OutputsAt(at["t_s"], Outputs(**{
            **at["outputs"],
            "display_grid": tuple(at["outputs"]["display_grid"]),
            "leds": tuple(map(tuple, at["outputs"]["leds"])),
        }))
        for at in data["outputs"]
    )
    return Recording(**data)
