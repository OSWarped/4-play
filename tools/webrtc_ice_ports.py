"""Validated, UDP-only ICE policy shared by both GStreamer publishers."""
from dataclasses import dataclass
import os


DEFAULT_ICE_UDP_PORT_START = 50000
DEFAULT_ICE_UDP_PORT_COUNT = 1000


@dataclass(frozen=True)
class IceUdpPortRange:
    start: int = DEFAULT_ICE_UDP_PORT_START
    count: int = DEFAULT_ICE_UDP_PORT_COUNT

    def __post_init__(self):
        if type(self.start) is not int or type(self.count) is not int:
            raise ValueError("ICE UDP port start/count must be integers")
        if self.start < 1 or self.count < 1 or self.end > 65535:
            raise ValueError("ICE UDP port range must stay within 1-65535 with a positive count")

    @property
    def end(self):
        return self.start + self.count - 1

    def validate_service_pools(self, settings):
        for prefix, default in [("INPUT", 42000), ("MEDIA", 41000)]:
            reserved = IceUdpPortRange(int(settings.get(f"FOURPLAY_{prefix}_PORT_START", default)),
                                      int(settings.get(f"FOURPLAY_{prefix}_PORT_COUNT", 100)))
            if max(self.start, reserved.start) <= min(self.end, reserved.end):
                raise ValueError(f"ICE UDP range overlaps the configured {prefix.lower()} port pool")
        return self

    @classmethod
    def from_args(cls, args):
        def configured(name, variable, default):
            value = getattr(args, name, None)
            return value if value is not None else int(os.environ.get(variable, default))
        return cls(
            configured("ice_udp_port_start", "FOURPLAY_WEBRTC_ICE_UDP_PORT_START", DEFAULT_ICE_UDP_PORT_START),
            configured("ice_udp_port_count", "FOURPLAY_WEBRTC_ICE_UDP_PORT_COUNT", DEFAULT_ICE_UDP_PORT_COUNT),
        ).validate_service_pools(os.environ)


def add_ice_port_arguments(parser):
    parser.add_argument("--ice-udp-port-start", type=int,
                        default=os.environ.get("FOURPLAY_WEBRTC_ICE_UDP_PORT_START", str(DEFAULT_ICE_UDP_PORT_START)))
    parser.add_argument("--ice-udp-port-count", type=int,
                        default=os.environ.get("FOURPLAY_WEBRTC_ICE_UDP_PORT_COUNT", str(DEFAULT_ICE_UDP_PORT_COUNT)))


def configure_ice_udp_ports(webrtc, ports):
    ice = webrtc.get_property("ice-agent")
    if ice is None:
        raise RuntimeError("GStreamer WebRTC ICE agent is unavailable")
    # GStreamer 1.26.2 constructs its default GstInitiallyUnowned ICE object
    # without sinking it. GI wrapping can consume the bin's sole floating ref.
    # Restore that native owner only when no separate bin reference remains;
    # the bin's normal dispose releases it. Public ref() has transfer-full
    # bookkeeping and does not restore this reference in PyGObject.
    if getattr(ice, "ref_count", 2) == 1:
        ice._ref()
    required = ("min-rtp-port", "max-rtp-port", "ice-udp", "ice-tcp", "agent")
    if any(ice.find_property(name) is None for name in required):
        raise RuntimeError("GStreamer ICE agent cannot enforce bounded UDP-only ports; upgrade the WebRTC/libnice stack")
    # Set before READY/PLAYING and before any candidate gathering. No unbounded
    # TCP fallback is advertised, so the firewall plan matches the transport.
    ice.set_property("min-rtp-port", ports.start)
    ice.set_property("max-rtp-port", ports.end)
    ice.set_property("ice-udp", True)
    ice.set_property("ice-tcp", False)
    nice = ice.get_property("agent")
    if nice is None or nice.find_property("upnp") is None:
        raise RuntimeError("GStreamer ICE agent cannot disable automatic UPnP mapping")
    nice.set_property("upnp", False)
    if nice.get_property("upnp"):
        raise RuntimeError("automatic UPnP mapping could not be disabled")
    expected = {"min-rtp-port": ports.start, "max-rtp-port": ports.end, "ice-udp": True, "ice-tcp": False}
    if any(ice.get_property(name) != value for name, value in expected.items()):
        raise RuntimeError("GStreamer ICE UDP configuration was not applied")


def validate_answer_candidates(answer, ports):
    """Regression guard for local host candidates; never return unbounded SDP."""
    found = False
    for line in answer.splitlines():
        if not line.startswith("a=candidate:"):
            continue
        fields = line.split()
        if len(fields) < 8 or fields[6:8] != ["typ", "host"]:
            raise RuntimeError("unexpected ICE candidate; host-only LAN transport required")
        if fields[2].lower() != "udp" or not ports.start <= int(fields[5]) <= ports.end:
            raise RuntimeError("local ICE candidate violates the configured UDP port range")
        found = True
    if not found:
        raise RuntimeError("no bounded UDP host candidates gathered; check port availability and ICE setup")


def main():
    import argparse
    parser = argparse.ArgumentParser(description="Check bounded UDP-only ICE support without gathering or opening media ports")
    parser.add_argument("--check", action="store_true", required=True)
    add_ice_port_arguments(parser)
    ports = IceUdpPortRange.from_args(parser.parse_args())
    import gi
    gi.require_version("Gst", "1.0")
    gi.require_version("GstWebRTC", "1.0")
    from gi.repository import Gst, GstWebRTC
    Gst.init(None)
    pipeline = Gst.Pipeline.new("ice-port-prerequisite-check")
    try:
        peer = Gst.ElementFactory.make("webrtcbin", "peer")
        if peer is None:
            raise RuntimeError("GStreamer webrtcbin is unavailable")
        pipeline.add(peer)
        configure_ice_udp_ports(peer, ports)
        print(f"Bounded UDP-only ICE supported: {ports.start}-{ports.end}")
    finally:
        pipeline.set_state(Gst.State.NULL)


if __name__ == "__main__":
    main()
