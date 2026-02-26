{
  lib,
  rustPlatform,
  versionCheckHook,
}:
let
  inherit (lib.fileset)
    toSource
    unions
    fileFilter
    intersection
    ;

  root = ../..;
  src = toSource {
    inherit root;
    fileset = unions [
      # Note: May eventually want to include `README.md` for help output?
      ../../Cargo.toml
      ../../Cargo.lock
      (intersection (fileFilter ({ hasExt, ... }: hasExt "rs") root) (unions [
        ../../src
        ../../tests
      ]))
    ];
  };

  cargoToml = builtins.fromTOML (builtins.readFile ../../Cargo.toml);
  pname = cargoToml.package.name;
  version = cargoToml.package.version;
in
rustPlatform.buildRustPackage {
  inherit pname version src;

  cargoHash = "sha256-WSKG1emAhOSkuggqztk5v+dQOto/2eabWRSYNNSEPbk=";

  doInstallCheck = true;
  nativeInstallCheckInputs = [ versionCheckHook ];
  versionCheckProgram = "${placeholder "out"}/bin/claude-mergetool";

  meta = {
    homepage = "https://github.com/9999years/claude-mergetool";
    changelog = "https://github.com/9999years/claude-mergetool/releases/tag/v${version}";
    description = "Resolve Git/jj merge conflicts automatically with claude-code";
    license = [ lib.licenses.mit ];
    maintainers = [ lib.maintainers._9999years ];
    mainProgram = "claude-mergetool";
  };
}
