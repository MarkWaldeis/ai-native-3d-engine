"""Deterministic approach/action, gated exit and held-input acceptance traces."""
def validations():
    inputs = []
    tick = 0
    for axis, value, duration, light in [("move_z", -1, 151, True), ("move_x", 1, 96, True),
            ("move_z", 1, 144, True), ("move_x", 1, 72, True), ("move_z", -1, 120, False),
            ("move_x", 1, 55, False), ("move_z", -1, 50, False)]:
        inputs.append({"tick": tick, "action": axis, "value": value})
        tick += duration
        inputs.append({"tick": tick, "action": axis, "value": 0})
        if light:
            inputs.append({"tick": tick, "action": "action", "value": 1})
            tick += 2
            inputs.append({"tick": tick, "action": "action", "value": 0})
    return {
        "win": {"ticks": 720, "inputs": inputs, "asserts": [
            {"state": "score", "equals": 4}, {"state": "won", "equals": True},
            {"state": "gate_open", "equals": True}, {"state": "_respawns", "equals": 0},
            {"state": "score", "equals": 3, "at_tick": 450},
            {"state": "gate_open", "equals": False, "at_tick": 450},
            {"entities_with_tag": "gate_bar", "count": 1, "at_tick": 450},
            {"entities_with_tag": "gate_bar", "count": 0},
            {"entities_with_tag": "lantern", "count": 0},
            {"entities_with_tag": "lit_lantern", "count": 4},
            {"entity": "Licht-1", "position_near": [-4.9, .98, -3.3], "tolerance": .01},
            {"entity": "Licht-4", "position_near": [2.5, .98, 3.3], "tolerance": .01}]},
        "no_action": {"ticks": 720, "inputs": [event for event in inputs if event["action"] != "action"],
            "asserts": [{"state": "score", "equals": 0}, {"state": "gate_open", "equals": False},
                {"state": "won", "equals": False}, {"entities_with_tag": "gate_bar", "count": 1},
                {"entities_with_tag": "lantern", "count": 4}]},
        "held_action": {"ticks": 720, "inputs": [event for event in inputs if event["action"] != "action"] +
            [{"tick": 151, "action": "action", "value": 1}],
            "asserts": [{"state": "score", "equals": 1}, {"state": "gate_open", "equals": False},
                {"state": "won", "equals": False}, {"entities_with_tag": "lantern", "count": 3}]},
        "idle": {"ticks": 120, "inputs": [], "asserts": [{"state": "score", "equals": 0},
            {"state": "won", "equals": False}, {"entity": "Lumi", "position_near": [-4.5, .32, 3.3], "tolerance": .15}]},
        "far_action": {"ticks": 30, "inputs": [{"tick": 0, "action": "action", "value": 1},
            {"tick": 2, "action": "action", "value": 0}], "asserts": [{"state": "score", "equals": 0},
            {"state": "gate_open", "equals": False}, {"state": "won", "equals": False},
            {"entities_with_tag": "lantern", "count": 4}]},
    }
