# Follow-ups: std/net, std/tls, std/http, std/json

Running list of things deferred from the v1 networking/data plan. Add to this file as decisions are made.

## HTTP client

- **Cookies** — client-side cookie jar, automatic `Cookie` header on repeat requests, Set-Cookie parsing. v1 workaround: users set cookies manually via headers.
- **Multipart form encoding** — `multipart/form-data` body builder. Needed for file uploads. v1 workaround: users construct the body bytes themselves.
- **Streaming request/response bodies** — iterator/generator API for request body chunks and response chunks, so multi-GB downloads don't have to fit in memory. v1 workaround: bodies are `String`/`Bytes`, fully buffered.
- **Content-encoding (gzip/deflate/br)** — negotiate `Accept-Encoding`, decompress response bodies transparently.
- **HTTP/2** — requires ALPN (deferred from std/tls) and a whole new framing layer.
- **HTTP/3 / QUIC** — further off.
- **HTTP server** — only client in v1. Server can be built on `TcpListener` + the HTTP protocol code in Aster, but not packaged as a std/http server API yet.

## std/dns

- **Explicit DNS module** — `dns.resolve(name)` returning all addresses, AAAA preference, custom resolvers, TTL access, SRV/MX records. v1 hides DNS inside `TcpStream.builder(host: ...)` which does getaddrinfo via the blocking pool.

## std/tls

- **Pinned cert / fingerprint verification** — `tls.connect(url, pinned_cert: "SHA256:...")`. Max-security case for known peers. Niche; add when someone needs it.
- **Client certs (mutual TLS)** — deferred from decision 5.
- **ALPN** — deferred from decision 5. Required for HTTP/2.
- **Session resumption exposed as API** — v1 may use it internally but won't surface knobs.

## JSON

- **Streaming parser** — for documents too large to fit in memory. v1 is all-in-memory.
- **Preserve-key-order on parse** — round-trip fidelity for objects. v1 uses `Map` order (whatever that is).
- **Arbitrary-precision decimals** — v1 parses numbers as `Int` (no decimal) or `Float` (has decimal). Financial/scientific use cases may need exact decimals.
