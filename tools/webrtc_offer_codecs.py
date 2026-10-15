"""Select offered RTP mappings; never log or persist SDP/ICE credentials."""
import re


def offered_payloads(sdp):
    sections = []
    current = None
    session_direction = 'sendrecv'
    for line in sdp.replace('\r\n', '\n').splitlines():
        if line.startswith('m='):
            parts = line[2:].split()
            if len(parts) < 4:
                raise ValueError('Malformed media line in browser offer')
            current = dict(kind=parts[0], port=parts[1], formats=parts[3:],
                           mappings={}, direction=session_direction, bundle=False)
            sections.append(current)
        elif line in ('a=recvonly', 'a=sendrecv', 'a=sendonly', 'a=inactive'):
            if current is None:
                session_direction = line[2:]
            else:
                current['direction'] = line[2:]
        elif current is not None:
            if line == 'a=bundle-only':
                current['bundle'] = True
            match = re.fullmatch(r'a=rtpmap:(\d+)\s+(\S+)', line, re.IGNORECASE)
            if match:
                previous = current['mappings'].get(match[1])
                if previous is not None and previous != match[2].lower():
                    raise ValueError('Conflicting RTP payload mappings in browser offer')
                current['mappings'][match[1]] = match[2].lower()
    selected = {}
    for kind, codec in [('video', 'vp8/90000'), ('audio', 'opus/48000/2')]:
        media = [section for section in sections if section['kind'] == kind]
        if not media and kind == 'audio':
            continue
        if len(media) != 1:
            raise ValueError(f'Expected one offered {kind} section')
        section = media[0]
        if (section['port'] == '0' and not section['bundle']) or section['direction'] not in ('recvonly', 'sendrecv'):
            raise ValueError(f'Browser offer cannot receive {kind}')
        for payload in section['formats']:
            mapping = section['mappings'].get(payload)
            compatible = mapping == codec or (kind == 'audio' and mapping == 'opus/48000')
            if compatible and payload.isdigit() and 0 <= int(payload) <= 127:
                selected[kind] = int(payload)
                break
        else:
            raise ValueError(f'Browser did not offer supported {codec} {kind}; audio-only negotiation is not accepted')
    return selected
