# ADR-0001: Repository and Project Structure

- Status: Accepted
- Date: 2026-07-26

## Context

4-Play is intended to become a long-lived arcade platform containing server
software, emulator-host software, seat clients, shared protocols, tools, and
product documentation.

The project is currently maintained by a small development effort and should
not begin as a collection of independently deployed microservices.

## Decision

4-Play will initially use one monorepository.

The repository will be organized around product responsibilities rather than
around a single programming language:

- `docs`
- `server`
- `clients`
- `shared`
- `tools`
- `examples`
- `assets`

The initial backend will be implemented as a small number of executable
applications with internally separated modules.

Major architecture decisions will be recorded as Architecture Decision Records.

## Consequences

### Benefits

- One repository contains the complete product context.
- Documentation and implementation evolve together.
- Shared protocol changes can be made atomically.
- Initial development, debugging, and versioning remain simple.
- The repository can later be divided if actual scaling needs justify it.

### Costs

- Repository size may grow as additional clients and tools are added.
- Build automation will eventually need to identify which components changed.
- Care must be taken not to store ROMs or large copyrighted game media in the
  source repository.