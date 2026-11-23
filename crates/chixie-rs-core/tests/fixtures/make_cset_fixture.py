"""Regenerate the synthetic ChiXie3 device-parameter fixture (`device_params.cset`).

A `.cset` is a pickle (protocol 4) of ChiXie3's DeviceParams dict `{device name: {key: value}}`.
The devices and values here are made up, so tests never need a real user's file. Values mix int,
float and str the way ChiXie3's settings do. The second device leaves out `y_plus` on purpose, so
the reader has to take that axis from `y_reduce`.

    uv run python -I crates/chixie-rs-core/tests/fixtures/make_cset_fixture.py
"""

import pathlib
import pickle

HERE = pathlib.Path(__file__).parent

devices = {
    "测试步进机": {
        "firmware": 3,
        "deviceType": 0,
        "writeSpeed": "19000",
        "pauseTime": 0.5,
        "penUp": 0,
        "penDown": 2.0,
        "x_plus": "X+",
        "x_reduce": "X-",
        "y_plus": "Y+",
        "y_reduce": "Y-",
        "WriteDonePos": 1,
    },
    "测试舵机": {
        "firmware": 0,
        "deviceType": 1,
        "writeSpeed": 8000,
        "pauseTime": "0.2",
        "penUp": 90.0,
        "penDown": "30",
        "x_plus": "Y-",
        "x_reduce": "Y+",
        "y_reduce": "X-",
        "WriteDonePos": 2,
    },
}

(HERE / "device_params.cset").write_bytes(pickle.dumps(devices, protocol=4))
print("ok")
