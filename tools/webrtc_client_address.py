"""Address selection for same-host control-plane-to-publisher signaling.

Only loopback callers may provide the private browser-IP header. Direct LAN
callers are identified by the TCP peer, not by spoofable forwarding headers.
"""
import ipaddress


def unicast_ip(value):
    if "%" in value:
        raise ValueError("scoped browser addresses are not supported by this signaling bridge")
    address = ipaddress.ip_address(value.strip())
    if isinstance(address, ipaddress.IPv6Address) and address.ipv4_mapped:
        address = address.ipv4_mapped
    if address.is_unspecified or address.is_multicast or address.is_loopback or str(address) == "255.255.255.255":
        raise ValueError("browser address must be a non-loopback unicast IP")
    return str(address)


def browser_host_for_request(headers, peer_ip, fallback=None):
    peer = ipaddress.ip_address(peer_ip)
    if isinstance(peer, ipaddress.IPv6Address) and peer.ipv4_mapped:
        peer = peer.ipv4_mapped
    forwarded = headers.get("x-4play-browser-ip")
    if peer.is_loopback and forwarded is not None:
        return unicast_ip(forwarded)
    if not peer.is_loopback:
        return unicast_ip(str(peer))
    # Legacy manual signaling may lack trusted transport metadata. Never rewrite
    # browser mDNS candidates to the local proxy's 127.0.0.1 by default.
    return unicast_ip(fallback) if fallback else None
