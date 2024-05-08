# Fingered

**[FINGER](https://en.wikipedia.org/wiki/Finger_(protocol))** ([RFC1288](https://datatracker.ietf.org/doc/html/rfc1288)) daemon implementation by **ED**gar (that's me)

## Building

That's Rust, so `cargo build --release`.

## Context

Finger is an old and super basic protocol that allows you to type `finger user@example.com` to get unstructured information about Unix user `user` on hostname `example.com`, mostly designed for end-users. A more modern, famous, more secure, machine-oriented alternative is WebFinger.

Finger hasn't really aged well due to both having known a major vulnerability[*](https://en.wikipedia.org/wiki/Morris_worm) and being (by design) very indiscreet (to today's standards) about the kind of information it publicly gives. While the original implementation can be heavily configured, most of its privacy-violating features are opt-out. It will — by default — happily give the home directory path, last login date or configured shell name to anyone who can reach it. Also, it doesn't feature any kind of encryption.

Despite that, in today's very high-tech world, there's something attractive to these older, almost obsolete protocols. **`fingered` aims to be a spec-compliant server implementation of the finger protocol, with sensible defaults, that you
can safely run on a server.** Most importantly, it doesn't try to resolve requested usernames as actual Unix users, and will instead serve static information written in a single config file.

## Installing & running

`fingered` can run on a TCP socket, a Unix domain socket or an inetd socket (stdin/stdout are treated as a socket). The TCP socket can be given explicitly or come from the `LISTEN_FDS` environment variable (systemd socket activation).

```
FINGER reimplementation by EDgar

Usage: fingered [OPTIONS] [BIND_TO]

Arguments:
  [BIND_TO]
          IP address or Unix socket path to listen on
          
          May be omitted if the program is started with socket activation. Must be omitted if `--inetd` is given.

Options:
      --inetd
          Run as an inetd-compatible child process, treating stdin and stdout as a socket

      --users-file <USERS_FILE>
          Path to the `users.toml` file
          
          [default: /etc/fingered/users.toml]

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version
```

### Configuration (`users.toml`)

Refer to `src/config.rs` for help on the config keys.

```toml
# Allow listing remote users (WARNING: true by default)
enable-index = true

# Short config syntax
users.alice = "Alice Doe <alice@example.com>"

# Long config syntax
[users.bob]
info = "Hi internet!" # returned by default, or when the client uses the `-s` flag
long-info = """Hi internet!
My name is Bob and I like pizza, sports car and sparkling water.""" # returned when the client uses the `-l` flag
unlisted = true
```

`finger` recommends CRLF line endings in the info and long info messages. By default `fingered` fixes line endings when reading the config file, so you don't have to worry about that.

## Packaging

If you ever want to package this program for any OS (why?), you can use `users.template.toml` as a default template for `/etc/fingered/users.toml`.
