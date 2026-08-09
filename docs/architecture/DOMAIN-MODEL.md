# 4-Play Domain Model

## Purpose

This document defines the core domain model of the 4-Play platform.

The domain model is implementation independent. It describes the concepts that
exist within the platform and their relationships. It is not a Rust design
document.

Whenever new functionality is added, this document should be updated before
implementation if the domain model changes.

---

# Domain Overview

The platform consists of four primary domains:

1. Game Domain
2. Session Domain
3. Seat Domain
4. Host Domain

Each domain owns a distinct portion of the system.

```
                    +------------------+
                    |  Game Catalog    |
                    +------------------+
                             │
                       owns Games
                             │
                        Game Package
                             │
                     Runtime Profiles

                     Session Broker
                            │
                      owns Sessions
                            │
                    Player Slots
                            │
                         Seats

                     Host Manager
                            │
                          Hosts
```

---

# Aggregate Roots

The following objects are aggregate roots.

## Game

Represents a playable title.

Examples

- TMNT
- Street Fighter II
- Super Metroid

A Game owns:

- Runtime Profiles
- Metadata
- Categories
- Media references

A Game does NOT own:

- Sessions
- Seats
- Hosts

---

## Session

Represents one running instance of a game.

Examples

TMNT Session #42

Street Fighter Session #18

A Session owns:

- Player Slots
- Stream
- Statistics
- Runtime State

A Session references:

- Game
- Runtime Profile
- Host

A Session does NOT own:

- Seat
- Host

---

## Seat

Represents one client capable of participating in sessions.

Examples

Table 1 Seat A

Bartop Cabinet 2

Tablet 14

Seat owns:

Current Connection State

Client Capabilities

Current Session ID

Seat never owns:

Game

Session

Host

---

## Host

Represents a machine capable of running emulators.

Host owns:

Running Processes

Virtual Controllers

Encoder Resources

Host never owns:

Games

Seats

---

## Game Package

Represents player-facing content.

Contains:

Artwork

Videos

Marquees

Cabinet Photos

Screenshots

Descriptions

Historical Information

Game Package does NOT define runtime behavior.

---

# Supporting Entities

## Runtime Profile

Defines how a Game executes.

Contains:

Emulator Adapter

ROM Identifier

Launch Parameters

Controller Configuration

Player Slot Policy

Join Behavior

Display Orientation

Maximum Players

---

## Player Slot

Represents one controller position inside one Session.

A Player Slot owns:

Virtual Controller

Assigned Seat (optional)

Slot State

Examples

TMNT Player 1

TMNT Player 2

TMNT Player 3

TMNT Player 4

Player Slot identity is:

(Session ID, Slot ID)

Player Slots are never global.

---

## Virtual Controller

Represents one emulator-visible controller.

Owned exclusively by one Player Slot.

Receives:

Controller State

Produces:

Operating System input events

---

## Stream

Represents the audio/video output of one Session.

Contains:

Video Encoder

Audio Encoder

Connected Viewers

---

# Services

Services coordinate entities but do not own business data.

## Game Catalog

Owns Games

Provides search

Provides metadata

Provides package loading

---

## Session Broker

Owns Sessions

Starts Sessions

Ends Sessions

Assigns Hosts

Reserves Player Slots

Routes Join Requests

---

## Seat Manager

Owns Seats

Tracks Connections

Tracks Heartbeats

Tracks Capabilities

---

## Host Manager

Owns Hosts

Tracks Capacity

Allocates Sessions

Monitors Health

---

# Ownership Rules

Game Catalog
    owns Games

Game
    owns Runtime Profiles

Game
    references Game Package

Session Broker
    owns Sessions

Session
    owns Player Slots

Player Slot
    owns Virtual Controller

Seat Manager
    owns Seats

Host Manager
    owns Hosts

Host
    owns Emulator Processes

---

# Identity Types

Every aggregate root has a strongly typed identifier.

GameId

RuntimeProfileId

SessionId

PlayerSlotId

SeatId

HostId

Identifiers shall not be represented by primitive strings throughout the codebase.

---

# Lifetime

Persistent

Game

Game Package

Runtime Profile

Seat

Host

Temporary

Session

Player Slot

Virtual Controller

Stream

---

# Reference Rules

Entities should reference other aggregate roots by identifier whenever practical.

Preferred

Session
    HostId

Avoid

Session
    Host

Preferred

Seat
    SessionId

Avoid

Seat
    Session

This minimizes ownership complexity.

---

# Design Principles

The backend shall be emulator agnostic.

The backend shall be client agnostic.

Player Slots belong to Sessions.

Virtual Controllers belong to Player Slots.

Games describe titles.

Runtime Profiles describe execution.

Game Packages describe presentation.

Services coordinate.

Entities own data.
