# Example NixOS configuration for an agent-facing GitHub comment wrapper.
#
# Goal:
# - the agent can already read local files
# - the agent may comment on PRs in one fixed repo
# - the agent should NOT receive the reusable GitHub token itself
#
# underwrap provides the hardened secret delivery.
# The custom target wrapper provides the narrow policy surface.

{ pkgs, underwrap, ... }:

let
  agentGhComment = pkgs.writeShellApplication {
    name = "agent-gh-comment";
    runtimeInputs = [ pkgs.gh ];
    text = ''
      set -euo pipefail

      if [ "$#" -ne 2 ]; then
        echo "usage: agent-gh-comment <pr-number> <body-file>" >&2
        exit 2
      fi

      pr="$1"
      body_file="$2"

      case "$pr" in
        ""|*[!0-9]*)
          echo "pr-number must be numeric" >&2
          exit 2
          ;;
      esac

      if [ ! -r "$body_file" ]; then
        echo "body-file not readable: $body_file" >&2
        exit 2
      fi

      # Fixed repo, fixed verb.
      # The caller can choose the PR number and comment body file,
      # but not arbitrary gh subcommands or arbitrary repositories.
      exec ${pkgs.gh}/bin/gh pr comment \
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
      wrappers.gh-comment = {
        # IMPORTANT: point underwrap at your own narrow wrapper, not directly at
        # the full gh CLI, when the command is meant for an agent.
        target = "${agentGhComment}/bin/agent-gh-comment";
        secret = "github/agent-comment";
        injection = {
          mode = "env";
          variable = "GH_TOKEN";
        };

        # Optional: set fixed env vars for the target from trusted config.
        envSet = {
          GH_NO_UPDATE_NOTIFIER = "1";
        };
      };

      secrets."github/agent-comment" = {
        # Prefer the narrowest token you can issue for this workflow.
        mutable = false;
      };
    };
  };
}
