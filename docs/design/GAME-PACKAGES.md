# Game Package Design

## Purpose

A game package describes how 4-Play presents and launches a title without exposing arbitrary runtime commands to clients.

A package references locally supplied runtime content; it does not embed copyrighted ROMs, BIOS files, or scraped media.

## Minimum manifest

```yaml
schema_version: 1
id: mame.xmen.4p
title: X-Men
runtime:
  adapter: mame
  machine: xmen
players:
  minimum: 1
  maximum: 4
  slot_mode: fixed-character
media:
  marquee: media/marquee.png
  preview: media/preview.mp4
controls:
  profile: arcade-standard
```

## Package responsibilities

- presentation metadata
- compatible runtime adapter
- approved launch identifiers
- player-slot semantics
- control profile
- aspect ratio and rotation
- save and configuration policy
- preview and artwork references
- health-check hints
- package version

## Validation

A validator shall reject:

- unknown schema versions
- path traversal
- absolute executable paths
- arbitrary command fragments
- missing required assets
- unsupported adapters
- duplicate package IDs
- invalid slot/controller mappings

Runtime adapters construct command lines from typed package fields and administrator configuration.
