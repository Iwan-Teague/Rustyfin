# Rustyfin Backups Media-First Direction

Status: active product direction

## Purpose

Rustyfin Backups should be a user-account feature for backing up media from a device, not a server-side restore workflow wearing a consumer label.

The long-term product shape is closer to an Immich-style mobile media backup flow:

- a user logs in
- the client identifies media on that device
- the user backs up photos and videos into their Rustyfin account
- the account owns the media backup history and recovery state

## Current Reality

The code that exists today is still the account archive export flow on `/backups`:

- profile state
- preferences
- AI conversation history
- playback progress
- continue-watching state
- activity history
- optional RustyVault export snapshot

That is useful, but it is a companion export path, not the principal Backups story.

## Scope Boundaries

Do not describe Rustyfin Backups as:

- a server restore console
- a host/system backup feature
- an admin-only operational tool disguised as a user feature

Those operational concerns belong under the separate host/system backup surfaces at `/api/v1/system/backups`.

## Product Vocabulary

Use these terms consistently:

- `media backup` or `device backup` for the primary user-facing feature
- `account archive export` for the current implemented archive path
- `system backup` for host/admin operational backup and restore
- `gallery backup` only when referring to the future media-oriented surface in the Rustyfin product

## Directional Goals

The eventual media-backup flow should support:

- authenticated user login
- device-originated media capture
- mobile-first upload and backup workflows
- clear ownership tied to the Rustyfin account
- durable backup history and restore visibility

## What This Doc Is Not

This is not a server-backup implementation spec.
This is not a restore-runbook for host maintenance.
This is the product direction document for the user-facing Backups area.
