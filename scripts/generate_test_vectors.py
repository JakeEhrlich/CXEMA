#!/usr/bin/env python3
"""
Generate test vectors for CXEMA levels.

This script simulates the "perfect" behavior of each chip and generates:
1. Correct output waveforms
2. Test vectors with 'x' for don't-care ticks (1 tick after transitions)

Usage: python generate_test_vectors.py [level_file.json ...]
       If no files specified, processes all levels/*.json files.
"""

import json
import sys
from pathlib import Path
from typing import Callable


# =============================================================================
# Simulation Functions
# Each function takes input waveforms dict and returns output waveforms dict
# Keys are pin names, values are lists of 0/1 values
# =============================================================================

def sim_cx04(inputs: dict, n_ticks: int) -> dict:
    """CX04: Four inverters - ~A, ~B, ~C, ~D"""
    return {
        '~A': [1 - inputs['A'][t] for t in range(n_ticks)],
        '~B': [1 - inputs['B'][t] for t in range(n_ticks)],
        '~C': [1 - inputs['C'][t] for t in range(n_ticks)],
        '~D': [1 - inputs['D'][t] for t in range(n_ticks)],
    }


def sim_cx08(inputs: dict, n_ticks: int) -> dict:
    """CX08: Four-input AND/OR gates"""
    return {
        'Y0': [inputs['A'][t] & inputs['B'][t] & inputs['C'][t] & inputs['D'][t] for t in range(n_ticks)],
        'Y1': [inputs['A'][t] | inputs['B'][t] | inputs['C'][t] | inputs['D'][t] for t in range(n_ticks)],
    }


def sim_cx02(inputs: dict, n_ticks: int) -> dict:
    """CX02: Three NOR gates"""
    return {
        'Y0': [1 - (inputs['A'][t] | inputs['B'][t]) for t in range(n_ticks)],
        'Y1': [1 - (inputs['C'][t] | inputs['D'][t]) for t in range(n_ticks)],
        'Y2': [1 - (inputs['E'][t] | inputs['F'][t]) for t in range(n_ticks)],
    }


def sim_cx00(inputs: dict, n_ticks: int) -> dict:
    """CX00: Three NAND gates"""
    return {
        'Y0': [1 - (inputs['A'][t] & inputs['B'][t]) for t in range(n_ticks)],
        'Y1': [1 - (inputs['C'][t] & inputs['D'][t]) for t in range(n_ticks)],
        'Y2': [1 - (inputs['E'][t] & inputs['F'][t]) for t in range(n_ticks)],
    }


def sim_cxr01(inputs: dict, n_ticks: int) -> dict:
    """CXR01: Power-on reset circuit
    Output is 1 for first 4 ticks, then transitions to 0
    """
    reset_period = 4
    return {
        '~R': [1 if t < reset_period else 0 for t in range(n_ticks)],
    }


def sim_cx279(inputs: dict, n_ticks: int) -> dict:
    """CX279: RS flip-flop
    S=1, R=0 -> Q=1
    S=0, R=1 -> Q=0
    S=0, R=0 -> memory (hold)
    """
    q = [0] * n_ticks
    state = 0  # Initial state
    for t in range(n_ticks):
        s = inputs['S'][t]
        r = inputs['R'][t]
        if s and not r:
            state = 1
        elif r and not s:
            state = 0
        # else: hold state
        q[t] = state
    return {
        'Q': q,
        '~Q': [1 - q[t] for t in range(n_ticks)],
    }


def sim_cx279e(inputs: dict, n_ticks: int) -> dict:
    """CX279E: RS flip-flop with enable
    When E=1, behaves like RS flip-flop
    When E=0, holds state
    """
    q = [0] * n_ticks
    state = 0
    for t in range(n_ticks):
        e = inputs['E'][t]
        s = inputs['S'][t]
        r = inputs['R'][t]
        if e:
            if s and not r:
                state = 1
            elif r and not s:
                state = 0
        q[t] = state
    return {
        'Q': q,
        '~Q': [1 - q[t] for t in range(n_ticks)],
    }


def sim_cx556(inputs: dict, n_ticks: int) -> dict:
    """CX556: Dual clock generator (ring oscillator)
    CL1: period 4 ticks (2 high, 2 low), starting high
    CL2: period 8 ticks (4 high, 4 low), starting high
    """
    return {
        'CL1': [1 - (t // 2) % 2 for t in range(n_ticks)],
        'CL2': [1 - (t // 4) % 2 for t in range(n_ticks)],
    }


def sim_cx74(inputs: dict, n_ticks: int) -> dict:
    """CX74: D flip-flop (positive edge triggered)
    On rising edge of CLK, Q takes value of D
    """
    q = [0] * n_ticks
    state = 0
    prev_clk = 0
    for t in range(n_ticks):
        clk = inputs['CLK'][t]
        d = inputs['D'][t]
        # Rising edge detection
        if clk and not prev_clk:
            state = d
        q[t] = state
        prev_clk = clk
    return {
        'Q': q,
        '~Q': [1 - q[t] for t in range(n_ticks)],
    }


def sim_cxf02(inputs: dict, n_ticks: int) -> dict:
    """CXF02: Frequency doubler
    S=0: Y = CLK (passthrough)
    S=1: Y = CLK XOR CLK_delayed_by_2 (doubles frequency with 50% duty cycle)
    """
    y = [0] * n_ticks
    for t in range(n_ticks):
        clk = inputs['CLK'][t]
        s = inputs['S'][t]
        if not s:
            y[t] = clk
        else:
            # XOR with 2-tick delayed version for period 4 output
            delayed_clk = inputs['CLK'][t-2] if t >= 2 else 0
            y[t] = clk ^ delayed_clk
    return {'Y': y}


def sim_cx139(inputs: dict, n_ticks: int) -> dict:
    """CX139: 2-to-4 decoder with enable
    E=0: all outputs 0
    E=1: output selected by (A1,A0) is 1
    """
    y0, y1, y2, y3 = [0] * n_ticks, [0] * n_ticks, [0] * n_ticks, [0] * n_ticks
    for t in range(n_ticks):
        e = inputs['E'][t]
        a0 = inputs['A0'][t]
        a1 = inputs['A1'][t]
        if e:
            addr = a1 * 2 + a0
            if addr == 0: y0[t] = 1
            elif addr == 1: y1[t] = 1
            elif addr == 2: y2[t] = 1
            elif addr == 3: y3[t] = 1
    return {'Y0': y0, 'Y1': y1, 'Y2': y2, 'Y3': y3}


def sim_cx83(inputs: dict, n_ticks: int) -> dict:
    """CX83: 2-bit full adder
    {CO, S1, S0} = A + B + CI
    where A = (A1, A0), B = (B1, B0)
    """
    s0, s1, co = [0] * n_ticks, [0] * n_ticks, [0] * n_ticks
    for t in range(n_ticks):
        a = inputs['A1'][t] * 2 + inputs['A0'][t]
        b = inputs['B1'][t] * 2 + inputs['B0'][t]
        ci = inputs['CI'][t]
        result = a + b + ci
        s0[t] = result & 1
        s1[t] = (result >> 1) & 1
        co[t] = (result >> 2) & 1
    return {'S0': s0, 'S1': s1, 'CO': co}


def sim_cx93(inputs: dict, n_ticks: int) -> dict:
    """CX93: Divide by 4 counter
    Output toggles every 2 rising edges of CLK
    """
    y = [0] * n_ticks
    count = 0
    prev_clk = 0
    for t in range(n_ticks):
        clk = inputs['CLK'][t]
        if clk and not prev_clk:  # Rising edge
            count = (count + 1) % 4
        y[t] = 1 if count >= 2 else 0
        prev_clk = clk
    return {'Y': y}


def sim_cx153(inputs: dict, n_ticks: int) -> dict:
    """CX153: 4-to-1 multiplexer
    Y = D[S] where S = (S1, S0)
    """
    y = [0] * n_ticks
    for t in range(n_ticks):
        s = inputs['S1'][t] * 2 + inputs['S0'][t]
        if s == 0: y[t] = inputs['D0'][t]
        elif s == 1: y[t] = inputs['D1'][t]
        elif s == 2: y[t] = inputs['D2'][t]
        else: y[t] = inputs['D3'][t]
    return {'Y': y}


def sim_cx161(inputs: dict, n_ticks: int) -> dict:
    """CX161: 4-bit counter with synchronous clear
    On rising edge of CLK:
      CLR=1: Q = 0
      CLR=0: Q = Q + 1
    """
    q0, q1, q2, q3 = [0] * n_ticks, [0] * n_ticks, [0] * n_ticks, [0] * n_ticks
    count = 0
    prev_clk = 0
    for t in range(n_ticks):
        clk = inputs['CLK'][t]
        clr = inputs['CLR'][t]
        if clk and not prev_clk:  # Rising edge
            if clr:
                count = 0
            else:
                count = (count + 1) % 16
        q0[t] = count & 1
        q1[t] = (count >> 1) & 1
        q2[t] = (count >> 2) & 1
        q3[t] = (count >> 3) & 1
        prev_clk = clk
    return {'Q0': q0, 'Q1': q1, 'Q2': q2, 'Q3': q3}


def sim_cx195(inputs: dict, n_ticks: int) -> dict:
    """CX195: 4-bit shift register
    On rising edge of CLK:
      DIN -> Q0 -> Q1 -> Q2 -> Q3
    """
    q0, q1, q2, q3 = [0] * n_ticks, [0] * n_ticks, [0] * n_ticks, [0] * n_ticks
    reg = [0, 0, 0, 0]  # Q0, Q1, Q2, Q3
    prev_clk = 0
    for t in range(n_ticks):
        clk = inputs['CLK'][t]
        din = inputs['DIN'][t]
        if clk and not prev_clk:  # Rising edge
            reg = [din, reg[0], reg[1], reg[2]]
        q0[t], q1[t], q2[t], q3[t] = reg
        prev_clk = clk
    return {'Q0': q0, 'Q1': q1, 'Q2': q2, 'Q3': q3}


def sim_cx6116(inputs: dict, n_ticks: int) -> dict:
    """CX6116: 8x1 bit RAM
    WE=1, RE=0: Write D_in to address
    WE=0, RE=1: Read from address to D_out
    D_in is the input waveform, D_out is what we compute
    """
    d_out = [0] * n_ticks
    memory = [0] * 8  # 8 cells
    for t in range(n_ticks):
        addr = inputs['A2'][t] * 4 + inputs['A1'][t] * 2 + inputs['A0'][t]
        we = inputs['WE'][t]
        re = inputs['RE'][t]
        d_in = inputs['D_in'][t]  # Use D_in for write data
        if we and not re:
            memory[addr] = d_in
        if re and not we:
            d_out[t] = memory[addr]
    return {'D_out': d_out}


def sim_cx181(inputs: dict, n_ticks: int) -> dict:
    """CX181: 2-bit ALU (logic unit only)
    F1 F0 | Operation
    0  0  | Y = A AND B
    0  1  | Y = A OR B
    1  0  | Y = NOT A
    1  1  | Y = A XOR B
    Z = 1 when Y = 00
    """
    y0, y1, z = [0] * n_ticks, [0] * n_ticks, [0] * n_ticks
    for t in range(n_ticks):
        a = inputs['A1'][t] * 2 + inputs['A0'][t]
        b = inputs['B1'][t] * 2 + inputs['B0'][t]
        f = inputs['F1'][t] * 2 + inputs['F0'][t]

        if f == 0:  # AND
            result = a & b
        elif f == 1:  # OR
            result = a | b
        elif f == 2:  # NOT A
            result = (~a) & 3  # 2-bit NOT
        else:  # XOR
            result = a ^ b

        y0[t] = result & 1
        y1[t] = (result >> 1) & 1
        z[t] = 1 if result == 0 else 0
    return {'Y0': y0, 'Y1': y1, 'Z': z}


# =============================================================================
# Level Configuration
# =============================================================================

LEVEL_CONFIG = {
    'CX04': {
        'sim': sim_cx04,
        'inputs': ['A', 'B', 'C', 'D'],
        'outputs': ['~A', '~B', '~C', '~D'],
        'pin_map': {'A': 1, 'B': 2, 'C': 3, 'D': 4, '~A': 7, '~B': 8, '~C': 9, '~D': 10},
    },
    'CX08': {
        'sim': sim_cx08,
        'inputs': ['A', 'B', 'C', 'D'],
        'outputs': ['Y0', 'Y1'],
        'pin_map': {'A': 1, 'B': 2, 'C': 3, 'D': 4, 'Y0': 7, 'Y1': 8},
    },
    'CX02': {
        'sim': sim_cx02,
        'inputs': ['A', 'B', 'C', 'D', 'E', 'F'],
        'outputs': ['Y0', 'Y1', 'Y2'],
        'pin_map': {'A': 1, 'B': 2, 'C': 3, 'D': 4, 'E': 9, 'F': 10, 'Y0': 7, 'Y1': 8, 'Y2': 11},
    },
    'CX00': {
        'sim': sim_cx00,
        'inputs': ['A', 'B', 'C', 'D', 'E', 'F'],
        'outputs': ['Y0', 'Y1', 'Y2'],
        'pin_map': {'A': 1, 'B': 2, 'C': 3, 'D': 4, 'E': 9, 'F': 10, 'Y0': 7, 'Y1': 8, 'Y2': 11},
    },
    'CXR01': {
        'sim': sim_cxr01,
        'inputs': [],
        'outputs': ['~R'],
        'pin_map': {'~R': 7},
    },
    'CX279': {
        'sim': sim_cx279,
        'inputs': ['S', 'R'],
        'outputs': ['Q', '~Q'],
        'pin_map': {'S': 1, 'R': 2, 'Q': 7, '~Q': 8},
        'stability_ticks': 2,
        'warmup_ticks': 2,
    },
    'CX279E': {
        'sim': sim_cx279e,
        'inputs': ['S', 'R', 'E'],
        'outputs': ['Q', '~Q'],
        'pin_map': {'S': 1, 'R': 2, 'E': 3, 'Q': 7, '~Q': 8},
        'stability_ticks': 2,
        'warmup_ticks': 2,
    },
    'CX556': {
        'sim': sim_cx556,
        'inputs': [],
        'outputs': ['CL1', 'CL2'],
        'pin_map': {'CL1': 4, 'CL2': 8},
        'stability_ticks': 0,
    },
    'CX74': {
        'sim': sim_cx74,
        'inputs': ['D', 'CLK'],
        'outputs': ['Q', '~Q'],
        'pin_map': {'D': 1, 'CLK': 2, 'Q': 7, '~Q': 8},
        'stability_ticks': 3,
    },
    'CXF02': {
        'sim': sim_cxf02,
        'inputs': ['CLK', 'S'],
        'outputs': ['Y'],
        'pin_map': {'CLK': 1, 'S': 2, 'Y': 7},
        'stability_ticks': 0,
    },
    'CX139': {
        'sim': sim_cx139,
        'inputs': ['A0', 'A1', 'E'],
        'outputs': ['Y0', 'Y1', 'Y2', 'Y3'],
        'pin_map': {'A0': 1, 'A1': 2, 'E': 3, 'Y0': 7, 'Y1': 8, 'Y2': 9, 'Y3': 10},
        'stability_ticks': 2,
    },
    'CX83': {
        'sim': sim_cx83,
        'inputs': ['A0', 'A1', 'B0', 'B1', 'CI'],
        'outputs': ['S0', 'S1', 'CO'],
        'pin_map': {'A0': 1, 'A1': 2, 'B0': 3, 'B1': 4, 'CI': 5, 'S0': 7, 'S1': 8, 'CO': 9},
        'stability_ticks': 4,
    },
    'CX93': {
        'sim': sim_cx93,
        'inputs': ['CLK'],
        'outputs': ['Y'],
        'pin_map': {'CLK': 1, 'Y': 7},
        'stability_ticks': 2,
    },
    'CX153': {
        'sim': sim_cx153,
        'inputs': ['D0', 'D1', 'D2', 'D3', 'S0', 'S1'],
        'outputs': ['Y'],
        'pin_map': {'D0': 1, 'D1': 2, 'D2': 3, 'D3': 4, 'S0': 6, 'S1': 7, 'Y': 8},
        'stability_ticks': 2,
    },
    'CX161': {
        'sim': sim_cx161,
        'inputs': ['CLK', 'CLR'],
        'outputs': ['Q0', 'Q1', 'Q2', 'Q3'],
        'pin_map': {'CLK': 1, 'CLR': 2, 'Q0': 7, 'Q1': 8, 'Q2': 9, 'Q3': 10},
        'stability_ticks': 2,
    },
    'CX195': {
        'sim': sim_cx195,
        'inputs': ['DIN', 'CLK'],
        'outputs': ['Q0', 'Q1', 'Q2', 'Q3'],
        'pin_map': {'DIN': 1, 'CLK': 2, 'Q0': 7, 'Q1': 8, 'Q2': 9, 'Q3': 10},
        'stability_ticks': 2,
    },
    'CX6116': {
        'sim': sim_cx6116,
        'inputs': ['A0', 'A1', 'A2', 'WE', 'RE', 'D_in'],
        'outputs': ['D_out'],
        'pin_map': {'A0': 1, 'A1': 2, 'A2': 3, 'WE': 4, 'RE': 5, 'D_in': 7, 'D_out': 7},
        'bidirectional': {'D_in': 7, 'D_out': 7},  # Both map to pin 7
        'stability_ticks': 2,
    },
    'CX181': {
        'sim': sim_cx181,
        'inputs': ['A0', 'A1', 'B0', 'B1', 'F0', 'F1'],
        'outputs': ['Y0', 'Y1', 'Z'],
        'pin_map': {'A0': 1, 'A1': 2, 'B0': 3, 'B1': 4, 'F0': 6, 'F1': 7, 'Y0': 8, 'Y1': 9, 'Z': 10},
        'stability_ticks': 2,
    },
}


def generate_test_vector(values: list[int], stability_ticks: int = 1, warmup_ticks: int = 0) -> str:
    """Generate test vector with 'x' for ticks after transitions.

    Args:
        values: Output values (list of 0/1)
        stability_ticks: Number of ticks after a transition to mark as don't care
        warmup_ticks: Number of ticks at the start to mark as don't care
    """
    test = ['?'] * len(values)

    # Mark warmup ticks as don't care
    for i in range(min(warmup_ticks, len(values))):
        test[i] = 'x'

    # Mark ticks after transitions as don't care
    for i in range(1, len(values)):
        if values[i] != values[i-1]:
            # Mark this tick and the next (stability_ticks - 1) ticks as don't care
            for j in range(stability_ticks):
                if i + j < len(values):
                    test[i + j] = 'x'

    return ''.join(test)


def process_level(level_path: Path, dry_run: bool = False) -> bool:
    """Process a single level file."""
    with open(level_path, 'r') as f:
        data = json.load(f)

    level_name = data.get('name', '')
    config = LEVEL_CONFIG.get(level_name)

    if not config:
        print(f"  {level_path.name}: No config for {level_name}, skipping")
        return False

    # Build pin_index -> name mapping from level's pins array
    pins = data.get('pins', [])
    pin_map = config.get('pin_map', {})

    # Build reverse map: pin_index -> signal_name for inputs
    input_signals = config.get('inputs', [])
    idx_to_input = {}
    for sig in input_signals:
        if sig in pin_map:
            idx_to_input[pin_map[sig]] = sig

    # Get input waveforms
    inputs = {}
    n_ticks = 64

    for wf in data.get('waveforms', []):
        pin_idx = wf.get('pin_index')
        if not wf.get('is_input', True):
            continue

        # Find signal name using config's pin_map
        sig_name = idx_to_input.get(pin_idx)
        if not sig_name:
            # Fall back to pin name from level
            sig_name = pins[pin_idx] if pin_idx < len(pins) else None
        if sig_name in ['vcc', 'nc', None]:
            continue

        values_str = wf.get('values', '')
        n_ticks = len(values_str)
        values = [1 if c == '1' else 0 for c in values_str]
        inputs[sig_name] = values

    # Run simulation
    try:
        outputs = config['sim'](inputs, n_ticks)
    except Exception as e:
        print(f"  {level_path.name}: Simulation error: {e}")
        import traceback
        traceback.print_exc()
        return False

    # Build reverse map for outputs: pin_index -> signal_name
    output_signals = config.get('outputs', [])
    idx_to_output = {}
    for sig in output_signals:
        if sig in pin_map:
            idx_to_output[pin_map[sig]] = sig

    # Get test vector parameters
    stability_ticks = config.get('stability_ticks', 1)
    warmup_ticks = config.get('warmup_ticks', 0)

    # Update output waveforms
    modified = False
    for wf in data.get('waveforms', []):
        if wf.get('is_input', True):
            continue

        pin_idx = wf.get('pin_index')

        # Find output signal name
        out_name = idx_to_output.get(pin_idx)
        if not out_name:
            # Fall back to pin name
            out_name = pins[pin_idx] if pin_idx < len(pins) else None

        if out_name not in outputs:
            continue

        new_values = outputs[out_name]
        new_values_str = ''.join(str(v) for v in new_values)
        new_test = generate_test_vector(new_values, stability_ticks, warmup_ticks)

        old_values = wf.get('values', '')
        old_test = wf.get('test', '')

        if old_values != new_values_str or old_test != new_test:
            print(f"  {level_path.name}: {out_name} (pin {pin_idx})")
            print(f"    old: {old_values[:50]}{'...' if len(old_values) > 50 else ''}")
            print(f"    new: {new_values_str[:50]}{'...' if len(new_values_str) > 50 else ''}")
            print(f"    tst: {new_test[:50]}{'...' if len(new_test) > 50 else ''}")

            wf['values'] = new_values_str
            wf['test'] = new_test
            modified = True

    if modified and not dry_run:
        with open(level_path, 'w') as f:
            json.dump(data, f, indent=2, ensure_ascii=False)
            f.write('\n')

    return modified


def main():
    import argparse
    parser = argparse.ArgumentParser(description='Generate test vectors for CXEMA levels')
    parser.add_argument('files', nargs='*', help='Level JSON files (default: all in levels/)')
    parser.add_argument('--dry-run', '-n', action='store_true', help='Show changes without writing')
    args = parser.parse_args()

    if args.files:
        level_files = [Path(f) for f in args.files]
    else:
        script_dir = Path(__file__).parent
        levels_dir = script_dir.parent / 'levels'
        level_files = sorted(levels_dir.glob('*.json'))

    if not level_files:
        print("No level files found!")
        return 1

    print(f"Processing {len(level_files)} level files...")
    if args.dry_run:
        print("(dry run - no files will be modified)")
    print()

    modified_count = 0
    for level_path in level_files:
        if process_level(level_path, args.dry_run):
            modified_count += 1

    print(f"\n{'Would modify' if args.dry_run else 'Modified'} {modified_count} files.")
    return 0


if __name__ == '__main__':
    sys.exit(main())
