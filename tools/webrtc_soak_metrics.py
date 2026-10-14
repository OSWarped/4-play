"""Non-secret, process-local Linux measurements for generated WebRTC soaks."""
import os
from pathlib import Path
import time


def read_process_sample(proc=Path('/proc/self'), now=None, ticks=None):
    # comm may contain spaces and parentheses; fields start after its last ')'.
    fields = (proc / 'stat').read_text().rsplit(')', 1)[1].split()
    status = {}
    for line in (proc / 'status').read_text().splitlines():
        key, _, value = line.partition(':')
        status[key] = value.strip()
    return {
        'monotonic_seconds': time.monotonic() if now is None else now,
        'cpu_seconds': (int(fields[11]) + int(fields[12])) /
                       (os.sysconf('SC_CLK_TCK') if ticks is None else ticks),
        'rss_mib': int(status['VmRSS'].split()[0]) / 1024,
        'threads': int(status['Threads']),
        'fds': sum(1 for _ in (proc / 'fd').iterdir()),
    }


def interval_sample(previous, current):
    elapsed = current['monotonic_seconds'] - previous['monotonic_seconds']
    if elapsed <= 0:
        raise ValueError('sample clock must advance')
    return dict(current, cpu_percent=100 *
                (current['cpu_seconds'] - previous['cpu_seconds']) / elapsed)


def progress_rates(previous, current, elapsed):
    if elapsed <= 0 or len(previous) != len(current):
        raise ValueError('invalid media sampling interval')
    rates = []
    for before, after in zip(previous, current):
        deltas = {kind: after[kind] - before[kind] for kind in ('video', 'audio')}
        if any(value <= 0 for value in deltas.values()):
            raise AssertionError('a receiver stopped decoding video or audio')
        rates.append({kind + '_blocks_per_second': value / elapsed
                      for kind, value in deltas.items()})
    return rates
