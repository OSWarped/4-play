import unittest
from webrtc_offer_codecs import offered_payloads


def offer(video=100, audio=109):
    return (f'v=0\r\nm=video 9 UDP/TLS/RTP/SAVPF 96 {video}\r\n'
            f'a=rtpmap:96 H264/90000\r\na=rtpmap:{video} VP8/90000\r\na=recvonly\r\n'
            f'm=audio 9 UDP/TLS/RTP/SAVPF {audio}\r\na=rtpmap:{audio} opus/48000/2\r\na=recvonly\r\n')


class OfferTests(unittest.TestCase):
    def test_browser_assignments_are_not_fixed_chrome_defaults(self):
        self.assertEqual(offered_payloads(offer()), dict(video=100, audio=109))
        self.assertEqual(offered_payloads(offer().replace('100 VP8', '100 vp8')), dict(video=100, audio=109))
        self.assertEqual(offered_payloads(offer().replace('opus/48000/2', 'opus/48000')), dict(video=100, audio=109))

    def test_codec_must_be_in_active_mline_and_receivable(self):
        for invalid in [offer().replace('96 100', '96'), offer().replace('a=recvonly', 'a=sendonly'),
                        offer().replace('m=video 9', 'm=video 0'), offer().replace('VP8/90000', 'H264/90000'),
                        offer().replace('opus/48000/2', 'PCMU/8000')]:
            with self.assertRaises(ValueError):
                offered_payloads(invalid)

    def test_video_only_and_bundle_only_audio(self):
        self.assertEqual(offered_payloads(offer().split('m=audio')[0]), dict(video=100))
        self.assertEqual(offered_payloads(offer().replace('m=audio 9', 'm=audio 0').replace('a=rtpmap:109', 'a=bundle-only\r\na=rtpmap:109')), dict(video=100, audio=109))

    def test_payload_bounds_and_malformed_or_duplicate_sections(self):
        for invalid in [offer(200), 'm=video', '', offer() + offer(), offer().replace('100 VP8/90000', '96 VP8/90000')]:
            with self.assertRaises(ValueError):
                offered_payloads(invalid)


if __name__ == '__main__':
    unittest.main()
