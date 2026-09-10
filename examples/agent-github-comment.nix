# Agent-facing GitHub comment wrapper with a fixed repository and verb.
# underwrap delivers the credential; the trusted helper narrows the operation.
#
# Prerequisites:
# - pass the flake input as module argument `underwrap`
# - create the local user `automation` and configure the backend URL/CA
# - replace `myorg/myrepo` with the one allowed repository
# - enroll the instance identity, then restart `underwrapd-agent.service`
# - put a least-privilege GitHub token in resource `github/agent-comment` as
#   env key GH_TOKEN, never in Nix configuration, and unlock the permission row
#
# Follow docs/operations.md for enrollment and docs/wrappers.md for target review.
# Recheck the pinned gh build, configuration and proxy behavior before relying
# on the helper's downstream scope. Nix evaluation is not a live GitHub test.

{ pkgs, underwrap, ... }:

let
  agentGhComment = pkgs.writeShellApplication {
    name = "agent-gh-comment";
    runtimeInputs = [ pkgs.coreutils pkgs.gh ];
    text = ''
      set -euo pipefail
      umask 077

      if [ "$#" -ne 1 ]; then
        echo "usage: agent-gh-comment <pr-number>  # comment body on stdin" >&2
        exit 2
      fi

      pr="$1"

      case "$pr" in
        ""|*[!0-9]*)
          echo "pr-number must be numeric" >&2
          exit 2
          ;;
      esac
      if [ "''${#pr}" -gt 9 ] || [ "$pr" -eq 0 ]; then
        echo "pr-number must be between 1 and 999999999" >&2
        exit 2
      fi

      # Buffer at most 64 KiB plus one byte in the target's private runtime
      # directory. Accepting a caller-selected path here would let the target
      # be directed at its own /proc state, including the projected token.
      body_file="$(mktemp -- "$XDG_RUNTIME_DIR/agent-gh-comment.XXXXXX")"
      cleanup() { rm -f -- "$body_file"; }
      trap cleanup EXIT
      trap 'exit 129' HUP
      trap 'exit 130' INT
      trap 'exit 143' TERM

      head -c 65537 > "$body_file"
      body_size="$(wc -c < "$body_file")"
      if [ "$body_size" -gt 65536 ]; then
        echo "comment body exceeds 65536 bytes" >&2
        exit 2
      fi

      # Fixed repo, fixed verb. The caller controls only the validated PR
      # number and the bounded bytes supplied on stdin.
      ${pkgs.gh}/bin/gh pr comment \
        --repo myorg/myrepo \
        "$pr" \
        --body-file "$body_file"
    '';
  };
in
{
  imports = [ underwrap.nixosModules.default ];

  services.underwrap = {
    enable = true;
    package = underwrap.packages.${pkgs.stdenv.hostPlatform.system}.default;

    instances.agent = {
      # Installs the `agent-gh-comment` shim for an existing local automation user.
      # This is PATH integration, not local-user authorization.
      allowedUsers = [ "automation" ];

      backend = {
        url = "https://backend.example.internal:18443";
        caFile = "/etc/underwrap/ca.pem";
      };

      wrappers.agent-gh-comment = {
        # IMPORTANT: point underwrap at your own narrow wrapper, not directly at
        # the full gh CLI, when the command is meant for an agent.
        target = "${agentGhComment}/bin/agent-gh-comment";
        resource = "github/agent-comment";
        env = true;

        # Optional: set fixed env vars for the target from trusted config.
        envSet = {
          GH_NO_UPDATE_NOTIFIER = "1";
        };
      };
    };
  };
}
