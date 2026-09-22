{ nixpkgs, system, underwrapModule, infoPackage, skillSource, skillPackage }:

let
  lib = nixpkgs.lib;
  pkgs = import nixpkgs { inherit system; };
  publicDescription = "Search \"project\" documents.\nDownload selected files — no automatic service operations.";
  baseWrapper = {
    target = "/bin/false";
    resource = "not-in-discovery";
    env = true;
    files.paths = [ ".config/tool" ];
    libsecret = true;
    accessPaths = [ "~" "/srv/tasks" ];
    envSet = {
      HOME = "/var/empty";
      TMPDIR = "/srv/tasks/tmp";
      NOT_IN_DISCOVERY = "ordinary-fixed-setting";
    };
  };
  evaluate = described: enabled: nixpkgs.lib.nixosSystem {
    inherit system;
    modules = [
      underwrapModule
      {
        boot.isContainer = true;
        system.stateVersion = "25.11";
        users.users.alice.isNormalUser = true;
        services.underwrap = {
          enable = enabled;
          package = infoPackage;
          hardenTargetHost = false;
          instances = {
            empty.backend.url = "https://backend.example.invalid";
            # A valid instance whose entry point shares the discovery basename.
            # Explicit /run/current-system/sw/bin discovery must remain distinct.
            info.backend.url = "https://backend.example.invalid";
            other = {
              backend.url = "https://backend.example.invalid";
              wrappers.docs = baseWrapper;
            };
            work = {
              allowedUsers = [ "alice" ];
              backend.url = "https://backend.example.invalid/private-prefix/";
              wrappers.docs = baseWrapper // lib.optionalAttrs described {
                description = publicDescription;
              };
            };
          };
        };
      }
    ];
  };
  described = evaluate true true;
  baseline = evaluate false true;
  disabled = evaluate false false;
  etc = described.config.environment.etc;
  catalogFile = etc."underwrap/discovery.json";
  catalog = builtins.fromJSON catalogFile.text;
  expectedWrapper = instance: description: users: {
    inherit instance description;
    name = "docs";
    command = [ "/run/wrappers/bin/underwrap-${instance}" "docs" ];
    alias = { name = "docs"; inherit users; };
    access_paths = [ "~" "/srv/tasks" ];
    projections = { env = true; files = [ ".config/tool" ]; libsecret = true; };
    fixed_home = true;
    fixed_tmpdir = true;
  };
  helpers = lib.findFirst
    (package: package.name or "" == "underwrap-tools-bin")
    (throw "missing public Underwrap helper package")
    described.config.environment.systemPackages;
in
assert (builtins.tryEval described.config.system.build.toplevel).success;
assert catalog == {
  schema_version = 1;
  instances = [ "empty" "info" "other" "work" ];
  wrappers = [
    (expectedWrapper "other" "" [])
    (expectedWrapper "work" publicDescription [ "alice" ])
  ];
};
assert catalogFile.mode == "0644";
assert catalogFile.user == "root" && catalogFile.group == "root";
assert !(disabled.config.environment.etc ? "underwrap/discovery.json");
# A description must never reach the privileged handoff or daemon schemas.
assert lib.all (name:
  etc.${name}.source == baseline.config.environment.etc.${name}.source
) [ "underwrap/daemon/work.yaml" "underwrap/instances/work.yaml" ];
assert described.config.security.wrappers == baseline.config.security.wrappers;
assert described.config.security.wrappers.underwrap-info.source == "${infoPackage}/bin/underwrap-exec";
assert lib.all (wrapper: wrapper.source != "${infoPackage}/bin/underwrap-info")
  (builtins.attrValues described.config.security.wrappers);
pkgs.runCommand "underwrap-wrapper-discovery-eval" {
  nativeBuildInputs = [ pkgs.python3 ];
  inherit helpers infoPackage skillSource skillPackage;
  catalogSource = catalogFile.source;
} ''
  test "$(readlink "$helpers/bin/underwrap-info")" = "$infoPackage/bin/underwrap-info"
  test "$(readlink "$helpers/bin/underwrap-keygen")" = "$infoPackage/bin/underwrap-keygen"
  test "$(find "$helpers/bin" -mindepth 1 -maxdepth 1 | wc -l)" -eq 2
  "$helpers/bin/underwrap-info" --help > help.txt
  "$helpers/bin/underwrap-info" --version > version.txt
  grep -F -- 'without executing them' help.txt
  grep -F -- 'underwrap-info 0.1.0' version.txt
  cmp "$skillSource/SKILL.md" "$skillPackage/SKILL.md"
  test -s "$skillPackage/LICENSE-MIT"
  test -s "$skillPackage/LICENSE-APACHE"
  python3 - <<'PY'
  import json, os, pathlib, re
  source = pathlib.Path(os.environ["skillSource"])
  text = (source / "SKILL.md").read_text()
  assert text.startswith("---\n")
  frontmatter, body = text[4:].split("\n---\n", 1)
  assert re.search(r"^name: underwrap$", frontmatter, re.M)
  description = re.search(r"^description: (.+)$", frontmatter, re.M).group(1)
  assert 0 < len(description) <= 1024
  assert "underwrap-info" in body
  assert "TMPDIR" in body
  assert "access_paths" in body
  # A standalone exported skill cannot depend on sibling repository docs.
  for target in re.findall(r"\]\(([^)]+)\)", body):
      if "://" not in target and not target.startswith("#"):
          resolved = (source / target.split("#")[0]).resolve()
          assert resolved.is_relative_to(source.resolve()) and resolved.exists()
  catalog = json.loads(pathlib.Path(os.environ["catalogSource"]).read_text())
  assert len(catalog["wrappers"]) == 2
  assert catalog["wrappers"][0]["description"] == ""
  assert catalog["wrappers"][1]["description"].count("\n") == 1
  PY
  mkdir -p "$out"
  cp "$catalogSource" "$out/discovery.json"
  cp help.txt version.txt "$out/"
''
