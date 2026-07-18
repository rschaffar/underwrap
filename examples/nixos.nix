# Complete NixOS frontend example for one stateless env-projection wrapper.
#
# Prerequisites:
# - pass the flake input as module argument `underwrap`
# - local user `alice` exists
# - `/etc/underwrap/ca.pem` verifies the backend server certificate
# - the backend is reachable at the configured URL
#
# After rebuilding, run `underwrap-keygen --instance-config
# /etc/underwrap/daemon/demo.yaml`, enroll the printed certificate, enable
# wrapper-asset auto-create (or create the assets manually), add env key
# DEMO_TOKEN to resource demo/token, and unlock the permission row.

{ pkgs, underwrap, ... }:

let
  secretCheck = pkgs.writeShellApplication {
    name = "underwrap-secret-check";
    text = ''
      set -euo pipefail
      if [[ -z "''${DEMO_TOKEN:-}" ]]; then
        echo "DEMO_TOKEN is missing" >&2
        exit 1
      fi
      echo "DEMO_TOKEN is available to the target"
    '';
  };
in
{
  imports = [ underwrap.nixosModules.default ];

  services.underwrap = {
    enable = true;
    package = underwrap.packages.${pkgs.stdenv.hostPlatform.system}.default;

    instances.demo = {
      # Installs `secret-check` and `debug-secret-check` shims for Alice.
      # This is PATH integration, not local-user authorization.
      allowedUsers = [ "alice" ];

      backend = {
        url = "https://backend.example.internal:18443";
        caFile = "/etc/underwrap/ca.pem";
      };

      wrappers.secret-check = {
        target = "${secretCheck}/bin/underwrap-secret-check";
        resource = "demo/token";
        env = true;
      };
    };
  };
}
