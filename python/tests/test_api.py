"""Exercise the installed wheel and the shared Rust simulator, with local firmware only."""

import json
import os
import struct
import time
from dataclasses import FrozenInstanceError
from pathlib import Path

import pytest

import euc_studio as euc

ROOT = Path(__file__).resolve().parents[2]


@pytest.fixture(scope="session")
def firmware():
    path = Path(os.environ.get(
        "EUC_STUDIO_FIRMWARE", ROOT / "firmware/build/target-clean/euc-fw-begode-extreme.elf"
    ))
    if not path.is_file():
        if os.environ.get("EUC_STUDIO_REQUIRE_FIRMWARE") == "1":
            pytest.fail(f"Build clean firmware before this suite: {path}")
        pytest.skip("Build local clean firmware to run simulator integration tests.")
    return path


@pytest.fixture
def scenario(tmp_path):
    path = tmp_path / "short.toml"
    path.write_text('''schema = 1
name = "python-short"
description = "A short held run."
wheel = "begode-extreme"
seconds = 0.1
release_s = 0.09
speed_kmh = [[0, 0]]
''')
    return path


def test_existing_run_api_returns_typed_result(scenario, firmware):
    result = euc.run_scenario(scenario, firmware)
    assert result.completed and result.passed
    assert result.sim_seconds == pytest.approx(0.1)
    assert len(result.samples) == 10
    assert result.outcome.phases == ()
    assert result.samples[-1].signals.battery_v > 0
    assert len(result.samples[-1].bodies) == len(result.bodies)
    assert json.loads(result.to_json())["cpu"] == "own"
    with pytest.raises(FrozenInstanceError):
        result.passed = False


def test_halted_firmware_is_not_completed(scenario, tmp_path):
    path = tmp_path / "halt.bin"
    # A local test image: valid vectors followed by an undefined Thumb instruction.
    path.write_bytes(struct.pack("<IIH", 0x2000C000, 0x08003009, 0xDE00))
    result = euc.run_scenario(scenario, path)
    assert result.halted is not None
    assert not result.completed
    assert not result.passed


def test_measured_phase_results_use_typed_values(scenario, firmware):
    scenario.write_text(scenario.read_text().replace("seconds = 0.1", "seconds = 2.2") + '''
[[phase]]
name = "short-phase"
lean_deg = 1.0
until_speed_kmh = 1.0
crossing = "above"
timeout_s = 0.1
''')
    result = euc.run_scenario(scenario, firmware)
    phase = result.outcome.phases[0]
    assert phase.name == "short-phase"
    assert phase.status == "target_unreachable"
    assert phase.reason == "timeout"


def test_session_steps_resets_and_records(scenario, firmware, tmp_path):
    session = euc.Session(scenario, firmware)
    created_by_ms = int(time.time() * 1000)
    assert session.result().completed is False
    with pytest.raises(ValueError):
        session.save(tmp_path / "empty.eucrec")
    sample = session.step(3)
    assert sample.t_s == pytest.approx(0.03)
    assert session.sample_count == 3
    session.set_manual(0.2, -0.3)
    session.set_push(10)
    session.set_held(False)
    session.step_off()
    session.step(20)
    assert session.finished
    assert session.sample_count == 10
    assert any(event.text == "manual control" for event in session.events)
    path = tmp_path / "run.eucrec"
    session.save(path)
    assert path.read_bytes().startswith(b"EUCREC\r\n")
    recording = euc.load_recording(path)
    assert recording.samples == session.samples
    assert recording.header["wheel"] == "begode-extreme"
    assert recording.header["started_unix_ms"] <= created_by_ms
    assert recording.header["scenario_toml"] == scenario.read_text()
    assert recording.context["run_mode"] == "scenario"
    assert recording.context["rider_model"] == "articulated_sagittal_v1"
    assert recording.outputs
    assert recording.outputs[0].outputs.controller_powered is True
    assert recording.truncated is None
    assert any("Scenario python-short" in event.text for event in recording.events)
    assert json.loads(recording.to_json())["header"]["cpu"] == "own"
    with pytest.raises(FileExistsError):
        session.save(path)
    with pytest.raises(RuntimeError):
        session.step()
    session.reset()
    assert session.time_s == 0
    assert session.sample_count == 0
    assert not session.finished
    assert session.samples == ()
    assert session.step().t_s == pytest.approx(0.01, abs=2 / 72_000_000)


def test_session_and_run_match(scenario, firmware):
    direct = euc.run_scenario(euc.load_scenario(scenario), firmware)
    session = euc.Session(euc.load_scenario(scenario), firmware)
    session.step(10)
    result = session.result()
    assert result.samples == direct.samples
    assert result.outcome == direct.outcome


@pytest.mark.parametrize("kwargs", [
    {"sample_hz": 0}, {"sample_hz": float("nan")}, {"sample_hz": 1001},
    {"max_samples": 1}, {"max_samples": 100001}, {"max_seconds": 0.01},
    {"max_wall_seconds": 0}, {"max_wall_seconds": float("inf")},
])
def test_run_rejects_invalid_limits_before_firmware_read(scenario, kwargs):
    with pytest.raises(ValueError):
        euc.run_scenario(scenario, "missing.bin", **kwargs)


@pytest.mark.parametrize("text", ["seconds = nan", "seconds = 601.0"])
def test_scenario_rejects_invalid_duration(scenario, text):
    scenario.write_text(scenario.read_text().replace("seconds = 0.1", text))
    with pytest.raises(ValueError):
        euc.load_scenario(scenario)


def test_missing_inputs_preserve_os_errors(scenario, tmp_path):
    with pytest.raises(FileNotFoundError):
        euc.load_scenario(tmp_path / "missing.toml")
    with pytest.raises(FileNotFoundError):
        euc.run_scenario(scenario, tmp_path / "missing.bin")
    with pytest.raises(FileNotFoundError):
        euc.load_recording(tmp_path / "missing.eucrec")


def test_bad_recording_raises_value_error(tmp_path):
    path = tmp_path / "bad.eucrec"
    path.write_bytes(b"not a recording")
    with pytest.raises(ValueError):
        euc.load_recording(path)


def test_context_validation_uses_shared_rust_gate(tmp_path):
    header = json.dumps({
        "schema": 1, "producer": "test", "started_unix_ms": 0,
        "wheel": "begode-extreme", "firmware": "test", "sample_hz": 100,
        "channels": [], "context": {
            "schema": 1, "run_mode": "free_ride", "rider_model": "rigid_ankle_v1",
            "model_aids": {"roll_stiffness": 0, "roll_damping": 0,
                           "steer_kp": 0, "steer_kd": 0, "steer_max_nm": 0},
        },
    }).encode()
    path = tmp_path / "missing-map.eucrec"
    path.write_bytes(b"EUCREC\r\n" + struct.pack("<HHI", 1, 0, len(header)) + header)
    with pytest.raises(ValueError, match="height_map"):
        euc.load_recording(path)


def test_session_rejects_nonfinite_controls(scenario, firmware):
    session = euc.Session(scenario, firmware)
    with pytest.raises(ValueError):
        session.step(0)
    with pytest.raises(ValueError):
        session.set_manual(float("nan"), 0)
    with pytest.raises(ValueError):
        session.set_push(float("inf"))
    with pytest.raises(ValueError):
        session.set_push(10001)


def test_ride_20kmh_stop(firmware):
    result = euc.run_scenario(ROOT / "scenarios/ride-20kmh-stop.toml", firmware)
    assert result.completed and result.halted is None
    assert result.passed, result.outcome.to_json()
