<div id="top"></div>

<!-- PROJECT LOGO -->
<br />

<div align="center">
  <a>
    <img src="https://raw.githubusercontent.com/lineage-foundation/valence/main/assets/hero.svg" alt="Logo" width="200px">
  </a>

  <h3>Lineage Valence</h3>

  <p align="center">
    An axum REST relay for exchanging E2E-encrypted data between peers.
    <br />
    <br />
    <a href="https://lineage.foundation"><strong>Lineage Foundation »</strong></a>
    <br />
    <br />
  </p>
</div>

**Repository:** [lineage-foundation/valence](https://github.com/lineage-foundation/valence) — migrated from [AIBlockOfficial/Valence](https://github.com/AIBlockOfficial/Valence).

<!-- TABLE OF CONTENTS -->
<details>
  <summary>Table of Contents</summary>
  <ol>
    <li><a href="#how-it-works">How it Works</a></li>
    <li>
      <a href="#getting-started">Getting Started</a>
      <ul>
        <li><a href="#prerequisites">Prerequisites</a></li>
        <li><a href="#running-the-server">Running the server</a></li>
      </ul>
    </li>
    <li><a href="#configuration">Configuration</a></li>
    <li>
      <a href="#messages-api">Messages API</a>
      <ul>
        <li><a href="#authentication">Authentication</a></li>
        <li><a href="#routes">Routes</a></li>
      </ul>
    </li>
  </ol>
</details>

## How it Works

Valence is a per-mailbox store of end-to-end encrypted blobs, backed by **Redis only** (no MongoDB, no cuckoo filter). Clients exchange data with each other through public key addresses. If Alice wants to send data to Bob, she `POST`s it to Bob's mailbox (Bob's address) under a signed request. The next time Bob calls `GET` on his own mailbox, he'll find what Alice sent.

Valence never sees plaintext: clients are expected to encrypt payloads for the recipient before sending, so the relay only ever stores and forwards ciphertext.

<p align="left">(<a href="#top">back to top</a>)</p>

## Getting Started

### Prerequisites

- **Rust** (2021 edition)
- A Redis instance (any recent version)

### Running the server

Start a Redis instance:

```sh
docker run -p 6379:6379 redis:6.2.6-alpine
```

Then run Valence:

```sh
cargo run --release
```

With no configuration at all, Valence listens on port `3030` and connects to `redis://127.0.0.1:6379`. See [`.env.example`](.env.example) for the full set of overrides.

A `Dockerfile` is also provided for building a distroless production image; there is no bundled `docker-compose.yml` — point `VALENCE_CACHE_URL` at whatever Redis you're running.

<p align="left">(<a href="#top">back to top</a>)</p>

## Configuration

Valence is configured via environment variables (optionally loaded from a `.env` file), layered over built-in defaults. An optional `config.toml`/`config.*` file in the working directory can also supply values, taking precedence over the built-in defaults but not over environment variables.

| Variable                    | Default                     | Description                                   |
| ---------------------------- | ---------------------------- | ---------------------------------------------- |
| `VALENCE_EXTERN_PORT`        | `3030`                       | Port the server listens on                     |
| `VALENCE_CACHE_URL`          | `redis://127.0.0.1:6379`     | Redis connection URL                           |
| `VALENCE_CACHE_TTL_SECS`     | `600`                        | TTL applied to stored messages                 |
| `VALENCE_BODY_LIMIT_BYTES`   | `8192`                       | Max accepted request body size, in bytes       |
| `VALENCE_DEBUG`              | `false`                      | Enable debug behavior/logging                  |

Nothing on this path panics on a missing or unparsable source — configuration simply falls back to the defaults above.

<p align="left">(<a href="#top">back to top</a>)</p>

## Messages API

### Authentication

Every route under `/messages` requires three headers, verified before the request is handled:

```json
{
  "address": "76e...dd6",     // caller's address — also the mailbox key
  "public_key": "a4c...e45",  // caller's ed25519 public key, hex-encoded
  "signature": "b9f...506"    // hex-encoded ed25519 detached signature over `address`'s raw UTF-8 bytes
}
```

A request is accepted iff `signature` is a valid detached signature of `address` under `public_key` — i.e. the caller proves control of `public_key` and signed the target address. This is intentionally **verify-only**: there is no `address == derived-from(public_key)` binding, since a sender addresses mail to a *recipient's* address while signing with their *own* key (this matches how the [tw_chain](https://crates.io/crates/tw_chain)-based Lineage/AIBlock JS SDK signs). Confidentiality comes from the payload being E2E-encrypted for the recipient, not from mailbox access control.

### Routes

All routes below require the headers described above; `address` selects the mailbox being read from or written to.

#### `POST /messages`

Stores an entry in the caller-addressed mailbox.

Body:

```json
{ "id": "EntryId", "data": "hello Bob" }
```

`id` is required and allows multiple entries per mailbox — posting with an existing `id` overwrites that entry. Returns `201 Created` with `{ "id": "EntryId" }`.

#### `GET /messages`

Returns the full mailbox as an `id -> data` map, e.g. `{ "msg1": { "hello": "world" } }`.

#### `GET /messages/{id}`

Returns a single entry as `{ "id": "...", "data": ... }`, or `404` if not found.

#### `DELETE /messages/{id}`

Deletes a single entry. Returns `204 No Content`.

#### `DELETE /messages`

Clears the entire mailbox. Returns `204 No Content`.

#### `GET /healthz`

Unauthenticated liveness check; returns `200 ok`.

<p align="left">(<a href="#top">back to top</a>)</p>

## Links

- [Lineage Foundation](https://lineage.foundation)
- [GitHub organization](https://github.com/lineage-foundation)

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

GPL-3.0 — see [LICENSE](LICENSE).
