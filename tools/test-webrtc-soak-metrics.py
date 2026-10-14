from pathlib import Path
import tempfile
import unittest

from webrtc_soak_metrics import interval_sample, progress_rates, read_process_sample


class MetricsTests(unittest.TestCase):
    def test_linux_stat_comm_and_units(self):
        with tempfile.TemporaryDirectory() as directory:
            proc = Path(directory)
            fields = ['0'] * 20
            fields[11:13] = ['100', '50']
            (proc / 'stat').write_text('123 (test (with spaces)) ' + ' '.join(fields))
            (proc / 'status').write_text('VmRSS:\t2048 kB\nThreads:\t3\n')
            (proc / 'fd').mkdir()
            (proc / 'fd' / '1').touch()
            result = read_process_sample(proc, now=10, ticks=100)
            self.assertEqual(result, dict(monotonic_seconds=10, cpu_seconds=1.5,
                                         rss_mib=2, threads=3, fds=1))

    def test_cpu_not_divided_by_number_of_cores(self):
        result = interval_sample(dict(monotonic_seconds=1, cpu_seconds=0),
                                 dict(monotonic_seconds=3, cpu_seconds=4))
        self.assertEqual(result['cpu_percent'], 200)
        with self.assertRaises(ValueError):
            interval_sample(result, result)

    def test_progress_and_stall(self):
        before = [dict(video=10, audio=20)]
        self.assertEqual(progress_rates(before, [dict(video=130, audio=120)], 2),
                         [dict(video_blocks_per_second=60, audio_blocks_per_second=50)])
        for after in [dict(video=10, audio=120), dict(video=130, audio=20),
                      dict(video=1, audio=1)]:
            with self.assertRaises(AssertionError):
                progress_rates(before, [after], 2)
        with self.assertRaises(ValueError):
            progress_rates(before, [], 2)


if __name__ == '__main__':
    unittest.main()
