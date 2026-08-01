# 4-Play Product Vision

## Vision

4-Play will provide a four-seat arcade table where each seated player can independently select and play a game, or join other players in a shared game session.

The same software platform shall support other client configurations, including bartop cabinets, upright cabinets, tablets, mobile devices, PCs, and commercial arcade installations.

The table is the flagship reference implementation. The server platform is the reusable product.

## Primary experience

A user sits at an available seat and sees:

- games available to start
- active sessions available to join
- available player positions within those sessions
- sessions available to spectate

The user may start a new game or join an existing game without needing to know which emulator, ROM, host computer, or command-line configuration is involved.

## Product principles

1. Social play is more important than presenting a large ROM list.
2. Players should be able to discover what is happening throughout the arcade.
3. The gameplay experience should feel as close to local emulation as reasonably possible.
4. Game-specific arcade behavior should be preserved rather than hidden.
5. The backend shall support multiple client types without being designed specifically around one physical cabinet.
6. Development shall proceed through small, demonstrable, playable milestones.
7. Every session should follow the same repeatable runtime path rather than treating the first or primary emulator instance specially.
8. Media previews, spectators, and operational features must not compromise the latency of active players.

## Evidence supporting the vision

The project has demonstrated that a Linux runtime host can launch independent headless MAME sessions and send synchronized live audio/video to a remote Windows seat. Multiple simultaneous sessions, game-specific resolutions and refresh rates, CHD-backed content, and runtime-created Linux virtual controllers have also been validated.

These results support the central thin-seat architecture, but they do not yet prove the complete user experience. Remote controller delivery, objective latency measurement, session discovery, joining, spectating, and player-slot workflows remain future milestones.

## Near-term product proof

The next meaningful product proof is a single remote seat that can:

1. request or launch one approved game session
2. send controller state to the runtime host
3. receive synchronized live media
4. play with measured practical latency
5. disconnect without leaving controls stuck

Once that foundation is measured and repeatable, development can advance toward the social discovery and shared-session experience that defines 4-Play.
