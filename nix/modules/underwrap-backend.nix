# NixOS module for `underwrap-backendd`.
#
# Provides:
#
# - `services.underwrap-backend.declarativeStores.<name>` — one
#   declarative store per entry. The module materialises each into
#   `/etc/underwrap-backend/stores.d/<name>.yaml` via
#   `environment.etc`, which symlinks the file into `/nix/store/`.
#   The backend daemon picks them up at startup via the `stores.d/`
#   union (ticket 230) and the admin API refuses to mutate them
#   (ticket 231).
#
# This module is intentionally narrow: it only owns the
# `declarativeStores` shape. The actual systemd service definition
# stays in the consumer's configuration (or `nix/tests/node.nix`)
# because the deployment story for the backend daemon is still
# pre-1.0 — host-specific bits like TLS material, admin listener,
# session key, etc. all live where the operator wires them up.
{ config, lib, pkgs, ... }:

let
  cfg = config.services.underwrap-backend;

  storeSubmodule = { name, ... }: {
    options = {
      backend = lib.mkOption {
        type = lib.types.enum [ "plain" "encrypted" ];
        description = ''
          Storage backend kind. `plain` writes one file per secret;
          `encrypted` wraps each file with ChaCha20-Poly1305 using a
          32-byte master key read from `keyFile`.
        '';
      };
      dir = lib.mkOption {
        type = lib.types.path;
        description = ''
          Absolute path to the data directory the daemon should
          create and own. Must be writable by the `underwrap-backend`
          system user.
        '';
      };
      keyFile = lib.mkOption {
        type = lib.types.nullOr lib.types.path;
        default = null;
        description = ''
          Path to the 32-byte master key file for encrypted stores.
          Required when `backend = "encrypted"`; ignored otherwise.
          Typically a sops-managed path under `/run/keys/` so the key
          never reaches the Nix store.
        '';
      };
      description = lib.mkOption {
        type = lib.types.nullOr lib.types.str;
        default = null;
        description = ''
          Operator-facing description surfaced in the admin UI.
        '';
      };
    };
  };

  # Build the `StoreDefinition` YAML for one store. Matches the Rust
  # struct in `crates/shared-protocol/src/config.rs`: storage config
  # flattened in (`backend`, `dir`, optional `key_file`) plus the
  # optional metadata fields.
  storeYaml = name: store: pkgs.writeText "underwrap-backend-store-${name}.yaml"
    (builtins.toJSON (
      {
        backend = store.backend;
        dir = toString store.dir;
      }
      // lib.optionalAttrs (store.backend == "encrypted") {
        key_file = toString store.keyFile;
      }
      // lib.optionalAttrs (store.description != null) {
        description = store.description;
      }
    ));
in {
  options.services.underwrap-backend = {
    declarativeStores = lib.mkOption {
      type = lib.types.attrsOf (lib.types.submodule storeSubmodule);
      default = {};
      description = ''
        Declarative store definitions. Each `<name>` entry is
        materialised as
        `/etc/underwrap-backend/stores.d/<name>.yaml`, a symlink into
        `/nix/store/`. The backend daemon refuses admin-API
        mutations on declarative stores; edit this option (and
        rebuild) to change them.
      '';
      example = lib.literalExpression ''
        {
          work = {
            backend = "encrypted";
            dir = "/var/lib/underwrap-backend/work";
            keyFile = "/run/keys/underwrap-work.key";
            description = "Production work secrets.";
          };
          personal = {
            backend = "plain";
            dir = "/var/lib/underwrap-backend/personal";
          };
        }
      '';
    };
  };

  config = {
    # Refuse to evaluate when a declarative encrypted store is
    # missing its keyFile — a typo here would otherwise surface only
    # at backend startup, well after the deploy completes.
    assertions = lib.mapAttrsToList (name: store: {
      assertion = !(store.backend == "encrypted" && store.keyFile == null);
      message = ''
        services.underwrap-backend.declarativeStores."${name}" has
        `backend = "encrypted"` but no `keyFile`; encrypted stores
        require a 32-byte key file path.
      '';
    }) cfg.declarativeStores;

    environment.etc = lib.mapAttrs' (name: store: lib.nameValuePair
      "underwrap-backend/stores.d/${name}.yaml"
      { source = storeYaml name store; }
    ) cfg.declarativeStores;
  };
}
