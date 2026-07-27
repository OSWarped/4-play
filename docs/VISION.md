# 4-Play Product Vision

## Vision

4-Play will provide a four-seat arcade table where each seated player can
independently select and play a game, or join other players in a shared game
session.

The same software platform shall support other client configurations,
including bartop cabinets, upright cabinets, tablets, mobile devices, PCs, and
commercial arcade installations.

The table is the flagship reference implementation. The server platform is
the reusable product.

## Primary Experience

A user sits at an available seat and sees:

- games available to start
- active sessions available to join
- available player positions within those sessions
- sessions available to spectate

The user may start a new game or join an existing game without needing to know
which emulator, ROM, host computer, or command-line configuration is involved.

## Product Principles

1. Social play is more important than presenting a large ROM list.
2. Players should be able to discover what is happening throughout the arcade.
3. The gameplay experience should feel as close to local emulation as
   reasonably possible.
4. Game-specific arcade behavior should be preserved rather than hidden.
5. The backend shall support multiple client types without being designed
   specifically around one physical cabinet.
6. Development shall proceed through small, demonstrable, playable milestones.