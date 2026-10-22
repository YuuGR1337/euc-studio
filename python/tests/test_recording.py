"""Independent stored-chunk fixtures exercise format compatibility without firmware."""

import json
import struct
import zlib

import pytest

import euc_studio as euc


def chunk(kind, payload, count=1):
    head = struct.pack("<BBHIIIdd", kind, 0, 0, count, len(payload), len(payload), 0.1, 0.2)
    return head + struct.pack("<II", zlib.crc32(head + payload), 0) + payload


@pytest.fixture
def external_recording(tmp_path):
    heights = [0] * 16641
    heights[0] = 40
    header = {
        "schema": 1, "producer": "independent-format-fixture", "started_unix_ms": 0,
        "wheel": "fixture-wheel", "firmware": "fixture-firmware", "sample_hz": 10,
        "channels": [{"id": 0, "key": "speed_kmh", "unit": "km/h"}],
        "future_field": {"keep": True},
        "context": {
            "schema": 1, "run_mode": "free_ride", "rider_model": "articulated_sagittal_v1",
            "model_aids": {"roll_stiffness": 0, "roll_damping": 0,
                           "steer_kp": 0, "steer_kd": 0, "steer_max_nm": 0},
            "height_map": {"revision": 7, "nx": 129, "ny": 129, "spacing_m": 0.25,
                           "origin_xy_m": [-16, -16], "heights_mm": heights},
        },
    }
    encoded = json.dumps(header).encode()
    data = b"EUCREC\r\n" + struct.pack("<HHI", 1, 0, len(encoded)) + encoded
    data += chunk(1, struct.pack("<IIII2d2f", 2, 1, 0, 0, 0.1, 0.2, 10.0, 20.0), 2)
    data += chunk(2, json.dumps([{"t_s": 0.1, "kind": "scenario", "text": "fixture event"}]).encode())
    data += chunk(3, json.dumps([{
        "t_s": 0.1, "kind": "text", "summary": "V reply", "fields": [["version", "test"]],
        "bytes": [86, 10], "reply": True,
    }]).encode())
    # Older output records omit the newer optional headlight and controller fields.
    data += chunk(4, json.dumps([{
        "t_s": 0.2, "outputs": {"display_grid": [1, 2], "display_on": True,
                                "display_brightness": 3, "leds": [[255, 0, 1]], "buzzer_hz": 2700},
    }]).encode())
    data += chunk(42, b"unknown future chunk")
    path = tmp_path / "external.eucrec"
    # No index: this models a writer that stopped after its last complete chunk.
    path.write_bytes(data)
    return path


def test_reads_saved_map_frames_outputs_and_unknown_header(external_recording):
    result = euc.load_recording(external_recording)
    assert result.header["future_field"] == {"keep": True}
    assert result.context["rider_model"] == "articulated_sagittal_v1"
    assert result.context["height_map"]["revision"] == 7
    assert result.context["height_map"]["heights_mm"][0] == 40
    assert len(result.context["height_map"]["heights_mm"]) == 16641
    assert result.start_s == 0.1 and result.end_s == 0.2
    assert [sample.signals.speed_kmh for sample in result.samples] == [10.0, 20.0]
    assert result.samples[0].signals.battery_v is None
    assert result.events[0].text == "fixture event"
    assert result.frames[0].fields == (("version", "test"),)
    assert result.frames[0].bytes == (86, 10)
    assert result.frames[0].reply is True
    assert result.outputs[0].outputs.buzzer_hz == 2700
    assert result.outputs[0].outputs.leds == ((255, 0, 1),)
    assert result.outputs[0].outputs.headlight_duty is None
    assert result.outputs[0].outputs.controller_powered is None
    assert result.truncated
    assert result.skipped == 1
    assert json.loads(result.to_json())["samples"][0]["signals"]["battery_v"] is None


def test_rejects_future_format_version(external_recording):
    data = bytearray(external_recording.read_bytes())
    data[8:10] = struct.pack("<H", 99)
    external_recording.write_bytes(data)
    with pytest.raises(ValueError, match="format 99"):
        euc.load_recording(external_recording)


def test_python_limits_sparse_items_before_loading_them(tmp_path):
    header = json.dumps({
        "schema": 1, "producer": "limit-fixture", "started_unix_ms": 0,
        "wheel": "fixture-wheel", "firmware": "fixture-firmware", "sample_hz": 100,
        "channels": [],
    }).encode()
    data = b"EUCREC\r\n" + struct.pack("<HHI", 1, 0, len(header)) + header
    # Unknown items still count toward the memory budget; they cannot bypass it by being skipped.
    data += chunk(2, b"[" + b"null," * 100_000 + b"null]", 100_001)
    path = tmp_path / "too-many-events.eucrec"
    path.write_bytes(data)
    with pytest.raises(ValueError, match="events limit"):
        euc.load_recording(path)
